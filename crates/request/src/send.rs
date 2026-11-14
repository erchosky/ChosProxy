//! Sending one complete request and reading its response.
//!
//! This is the Request Engine's send path (ADR-0009): Repeater uses it today
//! and later tools (fuzzer, crawler, scanner) will too. The request bytes are
//! written exactly as given — malformed requests included, which is the
//! point of a manual testing tool — and the response is read with the same
//! wire-exact codec as the proxy, so what is stored is what crossed the wire.

use crate::upstream::{ConnectTiming, Upstream, UpstreamSecurity};
use chosproxy_http::framing::response_framing;
use chosproxy_http::head::{find_head_end, read_head};
use chosproxy_http::{BodyKind, BodyReader, HttpError, MAX_HEAD_BYTES, RequestHead, ResponseHead};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

/// Interim (1xx) responses skipped before giving up on a final one.
const MAX_INTERIM: usize = 32;

/// Where a request is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    /// Host name or IP address to connect to.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// TLS server name (SNI) for HTTPS; usually the host.
    pub tls_name: String,
}

/// Time and size limits of one send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendLimits {
    /// Time allowed between writing the request and the response head.
    pub response_timeout: Duration,
    /// Longest silence tolerated while the body is being received.
    pub body_idle_timeout: Duration,
    /// Response bytes kept (head included); reading stops there.
    pub max_response_bytes: usize,
}

impl Default for SendLimits {
    /// Timeouts from the network settings, 32 MiB of response.
    fn default() -> SendLimits {
        let t = crate::network::network().timeouts;
        SendLimits {
            response_timeout: crate::network::secs(t.response_secs),
            body_idle_timeout: crate::network::secs(t.idle_secs),
            max_response_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Where the time of one send went, in milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SendTiming {
    /// DNS resolution.
    pub dns_ms: u64,
    /// TCP connect.
    pub connect_ms: u64,
    /// TLS handshake (0 for plain HTTP).
    pub tls_ms: u64,
    /// From the request being written to the response head (server time).
    pub wait_ms: u64,
    /// Receiving the response body.
    pub download_ms: u64,
    /// Everything, start to end.
    pub total_ms: u64,
}

/// A response received for a sent request.
#[derive(Debug, Clone)]
pub struct Sent {
    /// Address actually connected to.
    pub peer: SocketAddr,
    /// The final response: head and raw body bytes as received (capped at
    /// [`SendLimits::max_response_bytes`]).
    pub response: Vec<u8>,
    /// Response bytes seen on the wire (head included).
    pub response_len: u64,
    /// `response` was cut at the size limit.
    pub truncated: bool,
    /// Parsed head of the final response.
    pub head: ResponseHead,
    /// Interim (1xx) responses received before it.
    pub interim: usize,
    /// The body could not be read completely; why.
    pub incomplete: Option<String>,
    /// Timing.
    pub timing: SendTiming,
    /// The request went over a connection that had carried an earlier one.
    pub reused: bool,
    /// The connection can carry another request.
    pub keep_alive: bool,
    /// The request that actually went out, when it differs from the one
    /// given (platform credentials, session rules).
    pub sent_request: Option<Vec<u8>>,
    /// What changed it, for the history notes.
    pub notes: Vec<String>,
}

/// A send that produced no response.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SendError {
    /// What went wrong, for the user.
    pub message: String,
    /// Time spent until the failure.
    pub timing: SendTiming,
    /// Address connected to, if the connection was established.
    pub peer: Option<SocketAddr>,
}

fn ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The method token of a raw request (first word of the first line), even
/// when the rest of the request is malformed.
pub fn request_method(request: &[u8]) -> String {
    let line_end = request
        .iter()
        .position(|b| *b == b'\n')
        .unwrap_or(request.len());
    let line = &request[..line_end];
    let start = line
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(line.len());
    let token: Vec<u8> = line[start..]
        .iter()
        .take_while(|b| !b.is_ascii_whitespace())
        .copied()
        .collect();
    String::from_utf8_lossy(&token).into_owned()
}

/// Receives the pieces of a response body while it is read.
pub type BodySink<'a> = &'a mut (dyn FnMut(&[u8]) + Send);

/// Bytes kept up to a cap while everything is counted.
struct Kept {
    buf: Vec<u8>,
    cap: usize,
    total: u64,
}

impl Kept {
    /// Adds bytes; false once the cap is reached.
    fn push(&mut self, bytes: &[u8]) -> bool {
        self.total += bytes.len() as u64;
        let room = self.cap.saturating_sub(self.buf.len());
        if bytes.len() > room {
            self.buf.extend_from_slice(&bytes[..room]);
            false
        } else {
            self.buf.extend_from_slice(bytes);
            true
        }
    }
}

/// An open connection to a destination that can carry several requests in
/// a row (HTTP/1.1 keep-alive). Dropping it closes it.
pub struct Connection {
    up: Upstream,
    /// When it was opened and how long that took; taken by the first
    /// request so its timing includes connecting.
    opened: Option<(Instant, ConnectTiming)>,
    /// Requests sent on it so far.
    sends: u32,
    /// The last exchange does not allow another request on it.
    closed: bool,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("peer", &self.up.peer)
            .field("sends", &self.sends)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl Connection {
    /// Resolves, connects and (for HTTPS) completes the TLS handshake.
    pub async fn open(
        destination: &Destination,
        security: &UpstreamSecurity,
    ) -> Result<Connection, SendError> {
        let started = Instant::now();
        let up = Upstream::connect(
            &destination.host,
            destination.port,
            &destination.tls_name,
            security,
        )
        .await
        .map_err(|message| SendError {
            message,
            timing: SendTiming {
                total_ms: ms(started),
                ..SendTiming::default()
            },
            peer: None,
        })?;
        let timing = up.timing;
        Ok(Connection {
            up,
            opened: Some((started, timing)),
            sends: 0,
            closed: false,
        })
    }

    /// Address connected to.
    pub fn peer(&self) -> SocketAddr {
        self.up.peer
    }

    /// Requests sent on this connection so far.
    pub fn sends(&self) -> u32 {
        self.sends
    }

    /// True if another request can be sent on it: the last response allowed
    /// it, and the server has neither closed it nor sent anything unexpected.
    pub async fn is_reusable(&mut self) -> bool {
        if self.closed {
            return false;
        }
        // Poll once without waiting: pending means idle and healthy; ready
        // means EOF, an error or stray bytes.
        tokio::time::timeout(Duration::ZERO, self.up.reader.fill_buf())
            .await
            .is_err()
    }

    /// Writes `request` exactly as given and reads the response.
    pub async fn send(&mut self, request: &[u8], limits: &SendLimits) -> Result<Sent, SendError> {
        let (started, timing) = self.begin();
        if chosproxy_http::head::request_is_h2(request) {
            return Err(self.fail(
                "HTTP/2 requests are sent on their own connection; turn off \"reuse the \
                 connection\" or send as HTTP/1.1"
                    .into(),
                started,
                timing,
            ));
        }
        if let Err(e) = self.write(request).await {
            return Err(self.fail(format!("could not send the request: {e}"), started, timing));
        }
        self.read_response(request, started, timing, limits, None)
            .await
    }

    /// Like [`Connection::send`], and hands each piece of the response body
    /// to `on_body` as it arrives (transfer coding removed), for answers
    /// that are read while they are written. The whole response is still
    /// returned at the end.
    pub async fn send_streaming(
        &mut self,
        request: &[u8],
        limits: &SendLimits,
        on_body: BodySink<'_>,
    ) -> Result<Sent, SendError> {
        let (started, timing) = self.begin();
        if let Err(e) = self.write(request).await {
            return Err(self.fail(format!("could not send the request: {e}"), started, timing));
        }
        self.read_response(request, started, timing, limits, Some(on_body))
            .await
    }

    /// Closes the connection politely.
    pub async fn close(mut self) {
        let _ = self.up.writer.shutdown().await;
    }

    /// Start time and timing of the next request: the first one includes
    /// opening the connection.
    fn begin(&mut self) -> (Instant, SendTiming) {
        match self.opened.take() {
            Some((started, connect)) => (
                started,
                SendTiming {
                    dns_ms: connect.dns_ms,
                    connect_ms: connect.connect_ms,
                    tls_ms: connect.tls_ms,
                    ..SendTiming::default()
                },
            ),
            None => (Instant::now(), SendTiming::default()),
        }
    }

    async fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let bytes = self.up.outgoing(bytes).into_owned();
        self.up.writer.write_all(&bytes).await?;
        self.up.writer.flush().await
    }

    /// Writes the rest of a request whose start was written with `write`.
    async fn write_more(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.up.writer.write_all(bytes).await?;
        self.up.writer.flush().await
    }

    fn fail(&mut self, message: String, started: Instant, mut timing: SendTiming) -> SendError {
        self.closed = true;
        self.sends += 1;
        timing.total_ms = ms(started);
        let message = if self.sends > 1 {
            format!("{message} (on a reused connection)")
        } else {
            message
        };
        SendError {
            message,
            timing,
            peer: Some(self.up.peer),
        }
    }

    /// Reads the response to `request`, which has just been written.
    async fn read_response(
        &mut self,
        request: &[u8],
        started: Instant,
        mut timing: SendTiming,
        limits: &SendLimits,
        mut on_body: Option<BodySink<'_>>,
    ) -> Result<Sent, SendError> {
        let waiting = Instant::now();
        let method = request_method(request);
        let mut interim = 0usize;
        let head = loop {
            let read = tokio::time::timeout(
                limits.response_timeout,
                read_head(&mut self.up.reader, MAX_HEAD_BYTES),
            )
            .await;
            let failure = match read {
                Err(_) => format!("no response within {}s", limits.response_timeout.as_secs()),
                Ok(Ok(Some(raw))) => match ResponseHead::parse(raw) {
                    Ok(head) if head.is_interim() && interim < MAX_INTERIM => {
                        interim += 1;
                        continue;
                    }
                    Ok(head) => break head,
                    Err(e) => format!("invalid response: {e}"),
                },
                Ok(Ok(None)) | Ok(Err(HttpError::UnexpectedEof)) => {
                    "the server closed the connection without a response".into()
                }
                Ok(Err(HttpError::Io(e)))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    "the server closed the connection without a response".into()
                }
                Ok(Err(e)) => format!("invalid response: {e}"),
            };
            timing.wait_ms = ms(waiting);
            return Err(self.fail(failure, started, timing));
        };
        timing.wait_ms = ms(waiting);

        let downloading = Instant::now();
        let mut kept = Kept {
            buf: Vec::with_capacity(head.raw.len().min(limits.max_response_bytes)),
            cap: limits.max_response_bytes,
            total: 0,
        };
        let mut truncated = !kept.push(&head.raw);
        let mut incomplete = None;
        let opaque = head.status == 101
            || (method.eq_ignore_ascii_case("CONNECT") && (200..300).contains(&head.status));
        let kind = if opaque {
            // The connection now carries another protocol; nothing to read here.
            BodyKind::Empty
        } else {
            match response_framing(&method, &head) {
                Ok(f) => f.kind,
                Err(e) => {
                    incomplete = Some(format!("the body length is ambiguous: {e}"));
                    BodyKind::Empty
                }
            }
        };
        let until_close = kind == BodyKind::UntilClose;
        let mut body = BodyReader::new(kind);
        while !truncated {
            let is_data = body.next_is_data();
            let piece = tokio::time::timeout(
                limits.body_idle_timeout,
                body.next_chunk(&mut self.up.reader),
            )
            .await;
            match piece {
                Err(_) => {
                    incomplete = Some(format!(
                        "the response body stalled for {}s",
                        limits.body_idle_timeout.as_secs()
                    ));
                    break;
                }
                Ok(Ok(Some(bytes))) => {
                    if is_data && let Some(f) = on_body.as_mut() {
                        f(&bytes);
                    }
                    truncated = !kept.push(&bytes);
                }
                Ok(Ok(None)) => break,
                Ok(Err(e)) => {
                    incomplete = Some(format!("the response body is incomplete: {e}"));
                    break;
                }
            }
        }
        let request_keeps = request_head(request).is_some_and(|h| h.keep_alive());
        let keep_alive = request_keeps
            && head.keep_alive()
            && !opaque
            && !until_close
            && !truncated
            && incomplete.is_none();
        let reused = self.sends > 0;
        self.sends += 1;
        self.closed = !keep_alive;
        timing.download_ms = ms(downloading);
        timing.total_ms = ms(started);
        Ok(Sent {
            peer: self.up.peer,
            response: kept.buf,
            response_len: kept.total,
            truncated,
            head,
            interim,
            incomplete,
            timing,
            reused,
            keep_alive,
            sent_request: None,
            notes: Vec::new(),
        })
    }
}

/// The parsed head of a raw request, if it has a well-formed one.
fn request_head(request: &[u8]) -> Option<RequestHead> {
    let end = find_head_end(request)?;
    RequestHead::parse(request[..end].to_vec()).ok()
}

/// Opens a connection, writes `request` exactly as given and reads the
/// response. The connection is closed afterwards. Dropping the returned
/// future cancels the send.
pub async fn send(
    destination: &Destination,
    security: &UpstreamSecurity,
    request: &[u8],
    limits: &SendLimits,
) -> Result<Sent, SendError> {
    if chosproxy_http::head::request_is_h2(request) {
        return crate::h2::send_h2(destination, security, request, limits).await;
    }
    let mut connection = Connection::open(destination, security).await?;
    let sent = connection.send(request, limits).await;
    connection.close().await;
    // NTLM is a conversation on one connection, not one more request.
    if let Ok(first) = &sent
        && let Some(cred) = crate::auth::ntlm_for(request, &first.head, &destination.host)
        && let Some(answer) = ntlm_send(destination, security, request, limits, &cred).await
    {
        return answer;
    }
    // A 401 from a host with platform credentials: answer it once.
    if let Ok(first) = &sent
        && let Some((retry, scheme)) =
            crate::auth::retry_for(request, &first.head, &destination.host)
    {
        let mut connection = Connection::open(destination, security).await?;
        let again = connection.send(&retry, limits).await;
        connection.close().await;
        return again.map(|mut s| {
            s.sent_request = Some(retry);
            s.notes.push(format!(
                "Platform authentication ({scheme}) answered a 401 challenge."
            ));
            s
        });
    }
    sent
}

/// Like [`send`] for an HTTP/1 request whose answer is read while it is
/// written: `on_body` gets each piece of the body (transfer coding removed).
/// No platform-authentication retry.
pub async fn send_streaming(
    destination: &Destination,
    security: &UpstreamSecurity,
    request: &[u8],
    limits: &SendLimits,
    on_body: BodySink<'_>,
) -> Result<Sent, SendError> {
    let mut connection = Connection::open(destination, security).await?;
    let sent = connection.send_streaming(request, limits, on_body).await;
    connection.close().await;
    sent
}

/// Sends `request` with NTLMv2 platform authentication: the negotiate
/// message, the server's challenge and the answer, all on one new
/// connection (NTLM authenticates the connection). `None` when the
/// request cannot carry the header; otherwise the last response received.
async fn ntlm_send(
    destination: &Destination,
    security: &UpstreamSecurity,
    request: &[u8],
    limits: &SendLimits,
    cred: &crate::PlatformCredential,
) -> Option<Result<Sent, SendError>> {
    use ring::rand::SecureRandom;
    let first = crate::auth::with_authorization(
        request,
        &format!("NTLM {}", crate::ntlm::negotiate()),
        true,
    )?;
    let mut connection = match Connection::open(destination, security).await {
        Ok(c) => c,
        Err(e) => return Some(Err(e)),
    };
    let challenged = match connection.send(&first, limits).await {
        Ok(s) => s,
        Err(e) => return Some(Err(e)),
    };
    let note = |mut sent: Sent, text: &str| {
        sent.notes
            .push(format!("Platform authentication (NTLM): {text}."));
        Some(Ok(sent))
    };
    let message = crate::auth::ntlm_challenge(&challenged.head).filter(|m| !m.is_empty());
    let Some(message) = message.filter(|_| challenged.head.status == 401) else {
        connection.close().await;
        return note(challenged, "the server sent no challenge");
    };
    let challenge = match crate::ntlm::parse_challenge(&message) {
        Ok(c) => c,
        Err(e) => {
            connection.close().await;
            return note(challenged, &e);
        }
    };
    if !challenged.keep_alive {
        connection.close().await;
        return note(
            challenged,
            "the server closed the connection after its challenge",
        );
    }
    let mut nonce = [0u8; 8];
    if ring::rand::SystemRandom::new().fill(&mut nonce).is_err() {
        connection.close().await;
        return note(challenged, "no random numbers available");
    }
    let answer = crate::ntlm::authenticate(
        &cred.username,
        &cred.password,
        &challenge,
        &nonce,
        crate::ntlm::filetime_now(),
    );
    let last = crate::auth::with_authorization(request, &format!("NTLM {answer}"), false)?;
    let sent = connection.send(&last, limits).await;
    connection.close().await;
    Some(sent.map(|mut s| {
        s.sent_request = Some(last);
        s.notes
            .push("Platform authentication (NTLM) answered a 401 challenge.".into());
        s
    }))
}

fn same_security(a: &UpstreamSecurity, b: &UpstreamSecurity) -> bool {
    match (a, b) {
        (UpstreamSecurity::Plain, UpstreamSecurity::Plain) => true,
        (
            UpstreamSecurity::Tls {
                verification: va, ..
            },
            UpstreamSecurity::Tls {
                verification: vb, ..
            },
        ) => va == vb,
        _ => false,
    }
}

/// One request of [`send_parallel`].
#[derive(Debug, Clone)]
pub struct ParallelRequest {
    /// Where it goes.
    pub destination: Destination,
    /// How the connection is secured.
    pub security: UpstreamSecurity,
    /// The exact bytes to send.
    pub request: Vec<u8>,
}

/// Sends several requests at the same moment, each on its own connection
/// ("last-byte synchronisation"): every connection is opened and receives
/// its request except the last byte; then the last bytes are written one
/// right after the other, so the server gets the complete requests within a
/// fraction of a millisecond — the way to test race conditions over HTTP/1.
/// Results are in the order of `requests`. Dropping the future cancels all.
pub async fn send_parallel(
    requests: Vec<ParallelRequest>,
    limits: &SendLimits,
) -> Vec<Result<Sent, SendError>> {
    let count = requests.len();
    // HTTP/2 requests to one destination: a single-packet attack instead.
    if let Some(first) = requests.first()
        && requests.iter().all(|r| {
            chosproxy_http::head::request_is_h2(&r.request)
                && r.destination == first.destination
                && same_security(&r.security, &first.security)
        })
        && count <= crate::h2::MAX_SINGLE_PACKET
    {
        let bodies: Vec<Vec<u8>> = requests.iter().map(|r| r.request.clone()).collect();
        return crate::h2::send_h2_single_packet(
            &first.destination,
            &first.security,
            &bodies,
            limits,
        )
        .await;
    }
    let mut results: Vec<Option<Result<Sent, SendError>>> = (0..count).map(|_| None).collect();

    // 1. Open every connection and write all but the last byte.
    let mut opening = tokio::task::JoinSet::new();
    for (i, job) in requests.into_iter().enumerate() {
        opening.spawn(async move {
            let mut connection = match Connection::open(&job.destination, &job.security).await {
                Ok(c) => c,
                Err(e) => return (i, Err(e)),
            };
            let (started, timing) = connection.begin();
            let split = job.request.len().saturating_sub(1);
            if let Err(e) = connection.write(&job.request[..split]).await {
                let message = format!("could not send the request: {e}");
                return (i, Err(connection.fail(message, started, timing)));
            }
            (i, Ok((connection, job.request, started, timing)))
        });
    }
    let mut ready = Vec::with_capacity(count);
    while let Some(joined) = opening.join_next().await {
        match joined {
            Ok((i, Ok(staged))) => ready.push((i, staged)),
            Ok((i, Err(e))) => {
                if let Some(slot) = results.get_mut(i) {
                    *slot = Some(Err(e));
                }
            }
            Err(e) => resume_panic(e),
        }
    }

    // 2. The synchronised moment: the last byte of every request.
    let mut staged = Vec::with_capacity(ready.len());
    for (i, (mut connection, request, started, timing)) in ready {
        let last = request.len().saturating_sub(1);
        match connection.write_more(&request[last..]).await {
            Ok(()) => staged.push((i, connection, request, started, timing)),
            Err(e) => {
                let message = format!("could not send the request: {e}");
                if let Some(slot) = results.get_mut(i) {
                    *slot = Some(Err(connection.fail(message, started, timing)));
                }
            }
        }
    }

    // 3. Read the responses.
    let limits = *limits;
    let mut reading = tokio::task::JoinSet::new();
    for (i, mut connection, request, started, timing) in staged {
        reading.spawn(async move {
            let sent = connection
                .read_response(&request, started, timing, &limits, None)
                .await;
            connection.close().await;
            (i, sent)
        });
    }
    while let Some(joined) = reading.join_next().await {
        match joined {
            Ok((i, sent)) => {
                if let Some(slot) = results.get_mut(i) {
                    *slot = Some(sent);
                }
            }
            Err(e) => resume_panic(e),
        }
    }
    results
        .into_iter()
        .map(|r| {
            r.unwrap_or_else(|| {
                Err(SendError {
                    message: "the send did not finish".into(),
                    timing: SendTiming::default(),
                    peer: None,
                })
            })
        })
        .collect()
}

/// A send task panicked: carry the panic on (a bug, not a network error).
/// Cancelled tasks need nothing.
fn resume_panic(e: tokio::task::JoinError) {
    if e.is_panic() {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use chosproxy_lab::LabServer;
    use chosproxy_tls::{UpstreamTls, UpstreamVerification};
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    fn to(addr: SocketAddr) -> Destination {
        Destination {
            host: addr.ip().to_string(),
            port: addr.port(),
            tls_name: "localhost".into(),
        }
    }

    async fn lab() -> LabServer {
        LabServer::start("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap()
    }

    async fn get(lab: &LabServer, path: &str) -> Sent {
        let req = format!("GET {path} HTTP/1.1\r\nHost: lab\r\n\r\n");
        send(
            &to(lab.addr()),
            &UpstreamSecurity::Plain,
            req.as_bytes(),
            &SendLimits::default(),
        )
        .await
        .unwrap()
    }

    #[test]
    fn method_of_malformed_requests() {
        assert_eq!(request_method(b"POST /x HTTP/1.1\r\n\r\n"), "POST");
        assert_eq!(request_method(b"  PURGE\r\n"), "PURGE");
        assert_eq!(request_method(b""), "");
    }

    #[tokio::test]
    async fn reads_length_chunked_and_error_responses() {
        let lab = lab().await;
        let json = get(&lab, "/json").await;
        assert_eq!(json.head.status, 200);
        assert!(json.response.ends_with(b"\"nested\":{\"ok\":true}}"));
        assert_eq!(json.response_len, json.response.len() as u64);
        assert!(json.incomplete.is_none() && !json.truncated);

        let chunked = get(&lab, "/chunked").await;
        assert!(chunked.response.ends_with(b"0\r\n\r\n"));

        assert_eq!(get(&lab, "/status/404").await.head.status, 404);
    }

    #[tokio::test]
    async fn head_requests_have_no_body() {
        let lab = lab().await;
        let sent = send(
            &to(lab.addr()),
            &UpstreamSecurity::Plain,
            b"HEAD /json HTTP/1.1\r\nHost: lab\r\n\r\n",
            &SendLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(sent.response, sent.head.raw);
    }

    #[tokio::test]
    async fn skips_interim_responses() {
        let lab = lab().await;
        let sent = send(
            &to(lab.addr()),
            &UpstreamSecurity::Plain,
            b"POST /echo HTTP/1.1\r\nHost: lab\r\nExpect: 100-continue\r\nContent-Length: 2\r\n\r\nhi",
            &SendLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(sent.head.status, 200);
        assert!(sent.interim <= 1);
    }

    #[tokio::test]
    async fn truncates_at_the_limit() {
        let lab = lab().await;
        let limits = SendLimits {
            max_response_bytes: 1000,
            ..SendLimits::default()
        };
        let sent = send(
            &to(lab.addr()),
            &UpstreamSecurity::Plain,
            b"GET /large?bytes=200000 HTTP/1.1\r\nHost: lab\r\n\r\n",
            &limits,
        )
        .await
        .unwrap();
        assert!(sent.truncated);
        assert_eq!(sent.response.len(), 1000);
    }

    #[tokio::test]
    async fn https_with_an_unverified_lab_certificate() {
        let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let security = UpstreamSecurity::Tls {
            tls: UpstreamTls::new(&[]).unwrap(),
            verification: UpstreamVerification::NotVerified,
        };
        let sent = send(
            &to(lab.addr()),
            &security,
            b"GET /json HTTP/1.1\r\nHost: localhost\r\n\r\n",
            &SendLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(sent.head.status, 200);

        // Verified, the self-signed lab certificate is refused.
        let refused = send(
            &to(lab.addr()),
            &UpstreamSecurity::Tls {
                tls: UpstreamTls::new(&[]).unwrap(),
                verification: UpstreamVerification::Verified,
            },
            b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n",
            &SendLimits::default(),
        )
        .await
        .unwrap_err();
        assert!(refused.message.contains("TLS"), "{}", refused.message);
    }

    #[tokio::test]
    async fn reuses_keep_alive_connections() {
        let lab = lab().await;
        let mut c = Connection::open(&to(lab.addr()), &UpstreamSecurity::Plain)
            .await
            .unwrap();
        let req = b"GET /json HTTP/1.1\r\nHost: lab\r\n\r\n";
        let first = c.send(req, &SendLimits::default()).await.unwrap();
        assert!(!first.reused && first.keep_alive);
        assert!(c.is_reusable().await);
        let second = c.send(req, &SendLimits::default()).await.unwrap();
        assert!(second.reused);
        assert_eq!(second.head.status, 200);
        assert_eq!((second.timing.dns_ms, second.timing.connect_ms), (0, 0));
        let last = c
            .send(
                b"GET /json HTTP/1.1\r\nHost: lab\r\nConnection: close\r\n\r\n",
                &SendLimits::default(),
            )
            .await
            .unwrap();
        assert!(!last.keep_alive);
        assert!(!c.is_reusable().await);
        assert_eq!(c.sends(), 3);
    }

    #[tokio::test]
    async fn parallel_requests_complete_together() {
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        let arrivals = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let seen = arrivals.clone();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = server.accept().await.unwrap();
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut got = Vec::new();
                    let mut buf = [0u8; 256];
                    while !got.ends_with(b"\r\n\r\n") {
                        let n = s.read(&mut buf).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        got.extend_from_slice(&buf[..n]);
                    }
                    seen.lock().await.push(Instant::now());
                    let body = String::from_utf8_lossy(&got).len().to_string();
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    s.write_all(reply.as_bytes()).await.unwrap();
                });
            }
        });
        let jobs: Vec<ParallelRequest> = (0..4)
            .map(|i| ParallelRequest {
                destination: to(addr),
                security: UpstreamSecurity::Plain,
                request: format!("GET /{i} HTTP/1.1\r\nHost: x\r\n\r\n").into_bytes(),
            })
            .collect();
        let lengths: Vec<usize> = jobs.iter().map(|j| j.request.len()).collect();
        let results = send_parallel(jobs, &SendLimits::default()).await;
        assert_eq!(results.len(), 4);
        for (r, len) in results.iter().zip(lengths) {
            let sent = r.as_ref().unwrap();
            assert!(sent.response.ends_with(len.to_string().as_bytes()));
        }
        let times = arrivals.lock().await.clone();
        assert_eq!(times.len(), 4);
        let first = times.iter().min().unwrap();
        let last = times.iter().max().unwrap();
        assert!(last.duration_since(*first) < Duration::from_millis(100));

        // A destination that cannot be reached fails on its own.
        let unused = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let free = unused.local_addr().unwrap();
        drop(unused);
        let mixed = send_parallel(
            vec![
                ParallelRequest {
                    destination: to(free),
                    security: UpstreamSecurity::Plain,
                    request: b"GET / HTTP/1.1\r\n\r\n".to_vec(),
                },
                ParallelRequest {
                    destination: to(addr),
                    security: UpstreamSecurity::Plain,
                    request: b"GET / HTTP/1.1\r\n\r\n".to_vec(),
                },
            ],
            &SendLimits::default(),
        )
        .await;
        assert!(mixed[0].is_err() && mixed[1].is_ok());
    }

    #[tokio::test]
    async fn sends_malformed_requests_unchanged() {
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut s, _) = server.accept().await.unwrap();
            let mut got = vec![0u8; 23];
            s.read_exact(&mut got).await.unwrap();
            s.write_all(b"HTTP/1.1 400 Bad\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            got
        });
        let raw = b"GARBAGE  /x\r\nno colon\r\n";
        assert_eq!(raw.len(), 23);
        let sent = send(
            &to(addr),
            &UpstreamSecurity::Plain,
            raw,
            &SendLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(sent.head.status, 400);
        assert_eq!(task.await.unwrap(), raw);
    }

    #[tokio::test]
    async fn reports_silence_closes_and_refused_connections() {
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            // First connection: never answers. Second: closes at once.
            let (silent, _) = server.accept().await.unwrap();
            let (closed, _) = server.accept().await.unwrap();
            drop(closed);
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop(silent);
        });
        let limits = SendLimits {
            response_timeout: Duration::from_millis(200),
            ..SendLimits::default()
        };
        let req = b"GET / HTTP/1.1\r\nHost: x\r\n\r\n";
        let silent = send(&to(addr), &UpstreamSecurity::Plain, req, &limits)
            .await
            .unwrap_err();
        assert!(
            silent.message.starts_with("no response"),
            "{}",
            silent.message
        );
        assert!(silent.timing.wait_ms >= 150);
        let closed = send(&to(addr), &UpstreamSecurity::Plain, req, &limits)
            .await
            .unwrap_err();
        assert!(closed.message.contains("closed"), "{}", closed.message);

        let unused = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let free = unused.local_addr().unwrap();
        drop(unused);
        let refused = send(&to(free), &UpstreamSecurity::Plain, req, &limits)
            .await
            .unwrap_err();
        assert!(refused.peer.is_none());
    }
}
