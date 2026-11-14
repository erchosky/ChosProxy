//! HTTP/2 sends (ADR-0030).
//!
//! Requests and responses keep the text form used everywhere else, with the
//! version token `HTTP/2`: the request line and `Host` header become the
//! `:method`, `:path` and `:authority` pseudo-headers, header names are sent
//! in lower case, and connection-specific headers (which HTTP/2 forbids) are
//! dropped with a note. Responses come back as `HTTP/2 <status>` followed by
//! the header fields and the body as received (DATA frames joined).
//!
//! The single-packet attack sends several requests on one connection: every
//! request goes out without its final frame, then the final frames of all
//! requests are written together, so they arrive in one TCP packet.

use crate::send::{Destination, SendError, SendLimits, SendTiming, Sent};
use crate::upstream::{ConnectTiming, Upstream, UpstreamSecurity};
use bytes::Bytes;
use chosproxy_http::content::dechunk;
use chosproxy_http::head::{Headers, find_head_end};
use chosproxy_http::{RequestHead, ResponseHead};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Headers HTTP/2 does not allow (RFC 9113 §8.2.2).
const CONNECTION_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-connection",
    "transfer-encoding",
    "upgrade",
];
/// Most requests in one single-packet attack.
pub const MAX_SINGLE_PACKET: usize = 50;

fn ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// A request translated for HTTP/2.
#[derive(Debug)]
pub struct H2Request {
    /// Pseudo-headers and header fields.
    pub head: http::Request<()>,
    /// The body (chunked framing removed).
    pub body: Vec<u8>,
    /// What was changed to make the request valid HTTP/2.
    pub notes: Vec<String>,
}

/// Translates a text request (`… HTTP/2`) for sending to `destination`.
pub fn to_h2(raw: &[u8], https: bool, destination: &Destination) -> Result<H2Request, String> {
    let end = find_head_end(raw).ok_or("the request head is incomplete (missing blank line)")?;
    let head = RequestHead::parse(raw[..end].to_vec()).map_err(|e| e.to_string())?;
    let mut notes = Vec::new();
    let mut body = raw[end..].to_vec();
    if head
        .header_tokens("transfer-encoding")
        .iter()
        .any(|t| t == "chunked")
    {
        let (plain, complete) = dechunk(&body).map_err(|e| e.to_string())?;
        if !complete {
            return Err("the chunked body is incomplete".into());
        }
        body = plain;
        notes.push("Chunked body sent as plain DATA frames.".into());
    }
    let scheme = if https { "https" } else { "http" };
    let default_port = if https { 443 } else { 80 };
    let fallback = if destination.port == default_port {
        destination.host.clone()
    } else {
        format!("{}:{}", destination.host, destination.port)
    };
    let (authority, path) = match head
        .target
        .strip_prefix("http://")
        .or_else(|| head.target.strip_prefix("https://"))
    {
        Some(rest) => {
            let cut = rest.find(['/', '?']).unwrap_or(rest.len());
            let path = &rest[cut..];
            (
                rest[..cut].to_string(),
                if path.is_empty() {
                    "/".to_string()
                } else {
                    path.to_string()
                },
            )
        }
        None => (
            head.header_str("host").unwrap_or(fallback),
            head.target.clone(),
        ),
    };
    let path = if path.starts_with('?') {
        format!("/{path}")
    } else {
        path
    };
    let uri = http::Uri::builder()
        .scheme(scheme)
        .authority(authority.as_str())
        .path_and_query(path.as_str())
        .build()
        .map_err(|e| format!("cannot build :authority / :path from the request: {e}"))?;
    let method = http::Method::from_bytes(head.method.as_bytes())
        .map_err(|_| format!("invalid method {:?}", head.method))?;
    let mut builder = http::Request::builder()
        .method(method)
        .uri(uri)
        .version(http::Version::HTTP_2);
    let mut dropped = Vec::new();
    for h in &head.headers {
        let lower = h.name.to_ascii_lowercase();
        if lower == "host" {
            continue;
        }
        if CONNECTION_HEADERS.contains(&lower.as_str())
            || (lower == "te" && !h.value.eq_ignore_ascii_case(b"trailers"))
        {
            dropped.push(h.name.clone());
            continue;
        }
        let name = http::HeaderName::from_bytes(lower.as_bytes())
            .map_err(|_| format!("header name {:?} cannot be sent over HTTP/2", h.name))?;
        let value = http::HeaderValue::from_bytes(&h.value)
            .map_err(|_| format!("the value of {} cannot be sent over HTTP/2", h.name))?;
        builder = builder.header(name, value);
    }
    if !dropped.is_empty() {
        notes.push(format!(
            "Not sent (not allowed in HTTP/2): {}.",
            dropped.join(", ")
        ));
    }
    let head = builder.body(()).map_err(|e| e.to_string())?;
    Ok(H2Request { head, body, notes })
}

/// The text form of a response head.
fn response_head_text(parts: &http::response::Parts) -> Vec<u8> {
    let mut out = format!("HTTP/2 {}\r\n", parts.status.as_u16()).into_bytes();
    for (name, value) in &parts.headers {
        out.extend_from_slice(name.as_str().as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"\r\n");
    out
}

fn error_text(e: &h2::Error) -> String {
    match e.reason() {
        Some(reason) if e.is_remote() => format!("the server reset the stream: {reason}"),
        _ => format!("HTTP/2: {e}"),
    }
}

/// Holds writes back while closed, then writes everything held together
/// with the next write.
#[derive(Default)]
struct GateState {
    hold: bool,
    held: Vec<u8>,
}

struct Gated<S> {
    inner: S,
    state: Arc<Mutex<GateState>>,
    pending: Vec<u8>,
}

fn gate_lock(m: &Mutex<GateState>) -> std::sync::MutexGuard<'_, GateState> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl<S: AsyncWrite + Unpin> Gated<S> {
    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        while !self.pending.is_empty() {
            match Pin::new(&mut self.inner).poll_write(cx, &self.pending) {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()));
                }
                Poll::Ready(Ok(n)) => {
                    self.pending.drain(..n);
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Gated<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Gated<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if let Poll::Ready(Err(e)) = self.drain(cx) {
            return Poll::Ready(Err(e));
        }
        if !self.pending.is_empty() {
            return Poll::Pending;
        }
        let state = self.state.clone();
        let mut st = gate_lock(&state);
        if st.hold {
            st.held.extend_from_slice(buf);
            return Poll::Ready(Ok(buf.len()));
        }
        if st.held.is_empty() {
            drop(st);
            return Pin::new(&mut self.inner).poll_write(cx, buf);
        }
        // Released: everything held goes out with this write, in one call.
        let mut all = std::mem::take(&mut st.held);
        drop(st);
        all.extend_from_slice(buf);
        self.pending = all;
        match self.drain(cx) {
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            _ => Poll::Ready(Ok(buf.len())),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        {
            let state = self.state.clone();
            let mut st = gate_lock(&state);
            if !st.hold && !st.held.is_empty() {
                let held = std::mem::take(&mut st.held);
                drop(st);
                self.pending.extend_from_slice(&held);
            }
        }
        match self.drain(cx) {
            Poll::Ready(Ok(())) => Pin::new(&mut self.inner).poll_flush(cx),
            other => other,
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.drain(cx) {
            Poll::Ready(Ok(())) => Pin::new(&mut self.inner).poll_shutdown(cx),
            other => other,
        }
    }
}

/// An HTTP/2 connection.
struct Session {
    sender: h2::client::SendRequest<Bytes>,
    ping: Option<h2::PingPong>,
    gate: Arc<Mutex<GateState>>,
    peer: SocketAddr,
    connect: ConnectTiming,
}

async fn open(destination: &Destination, security: &UpstreamSecurity) -> Result<Session, String> {
    let up = Upstream::connect_h2(
        &destination.host,
        destination.port,
        &destination.tls_name,
        security,
    )
    .await?;
    let peer = up.peer;
    let connect = up.timing;
    let gate = Arc::new(Mutex::new(GateState::default()));
    let io = Gated {
        inner: up.into_io(),
        state: gate.clone(),
        pending: Vec::new(),
    };
    let (sender, mut connection) = h2::client::Builder::new()
        .initial_window_size(1 << 20)
        .handshake::<_, Bytes>(io)
        .await
        .map_err(|e| format!("HTTP/2 handshake failed: {}", error_text(&e)))?;
    let ping = connection.ping_pong();
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::debug!(error = %e, "HTTP/2 connection ended");
        }
    });
    Ok(Session {
        sender,
        ping,
        gate,
        peer,
        connect,
    })
}

/// Reads one response. `waiting` is when the request was complete.
async fn receive(
    future: h2::client::ResponseFuture,
    limits: &SendLimits,
    started: Instant,
    waiting: Instant,
    mut timing: SendTiming,
    peer: SocketAddr,
) -> Result<Sent, SendError> {
    let fail = |message: String, timing: SendTiming| SendError {
        message,
        timing: SendTiming {
            total_ms: ms(started),
            ..timing
        },
        peer: Some(peer),
    };
    let response = match tokio::time::timeout(limits.response_timeout, future).await {
        Err(_) => {
            timing.wait_ms = ms(waiting);
            return Err(fail(
                format!("no response within {}s", limits.response_timeout.as_secs()),
                timing,
            ));
        }
        Ok(Err(e)) => {
            timing.wait_ms = ms(waiting);
            return Err(fail(error_text(&e), timing));
        }
        Ok(Ok(r)) => r,
    };
    timing.wait_ms = ms(waiting);
    let downloading = Instant::now();
    let (parts, mut body) = response.into_parts();
    let mut out = response_head_text(&parts);
    let mut total = out.len() as u64;
    let mut truncated = false;
    let mut incomplete = None;
    loop {
        match tokio::time::timeout(limits.body_idle_timeout, body.data()).await {
            Err(_) => {
                incomplete = Some("the server stopped sending the body".into());
                break;
            }
            Ok(None) => break,
            Ok(Some(Err(e))) => {
                incomplete = Some(error_text(&e));
                break;
            }
            Ok(Some(Ok(chunk))) => {
                let _ = body.flow_control().release_capacity(chunk.len());
                total += chunk.len() as u64;
                let room = limits.max_response_bytes.saturating_sub(out.len());
                if chunk.len() > room {
                    out.extend_from_slice(&chunk[..room]);
                    truncated = true;
                    break;
                }
                out.extend_from_slice(&chunk);
            }
        }
    }
    timing.download_ms = ms(downloading);
    timing.total_ms = ms(started);
    let end = find_head_end(&out).unwrap_or(out.len());
    let head = ResponseHead::parse(out[..end].to_vec())
        .map_err(|e| fail(format!("invalid response head: {e}"), timing))?;
    Ok(Sent {
        peer,
        response: out,
        response_len: total,
        truncated,
        head,
        interim: 0,
        incomplete,
        timing,
        reused: false,
        keep_alive: false,
        sent_request: None,
        notes: Vec::new(),
    })
}

fn connect_timing(c: ConnectTiming) -> SendTiming {
    SendTiming {
        dns_ms: c.dns_ms,
        connect_ms: c.connect_ms,
        tls_ms: c.tls_ms,
        ..SendTiming::default()
    }
}

/// An open HTTP/2 connection that carries many requests at once (the
/// proxy keeps one per client connection).
pub struct H2Connection {
    session: Session,
    destination: Destination,
    https: bool,
}

impl std::fmt::Debug for H2Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("H2Connection")
            .field("peer", &self.session.peer)
            .finish_non_exhaustive()
    }
}

impl H2Connection {
    /// Opens a connection. `Ok(None)` when the TLS server does not offer
    /// HTTP/2 (the caller can fall back to HTTP/1.1).
    pub async fn open(
        destination: &Destination,
        security: &UpstreamSecurity,
    ) -> Result<Option<H2Connection>, String> {
        match open(destination, security).await {
            Ok(session) => Ok(Some(H2Connection {
                session,
                destination: destination.clone(),
                https: matches!(security, UpstreamSecurity::Tls { .. }),
            })),
            Err(e) if e.starts_with(crate::upstream::NO_H2_ALPN) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Address connected to.
    pub fn peer(&self) -> SocketAddr {
        self.session.peer
    }

    /// Sends one text request (`… HTTP/2`) on this connection.
    pub async fn send(&self, request: &[u8], limits: &SendLimits) -> Result<Sent, SendError> {
        let started = Instant::now();
        let peer = self.session.peer;
        let fail = |message: String| SendError {
            message,
            timing: SendTiming {
                total_ms: ms(started),
                ..SendTiming::default()
            },
            peer: Some(peer),
        };
        let req = to_h2(request, self.https, &self.destination).map_err(fail)?;
        let mut sender = self
            .session
            .sender
            .clone()
            .ready()
            .await
            .map_err(|e| fail(error_text(&e)))?;
        let has_body = !req.body.is_empty();
        let (future, mut stream) = sender
            .send_request(req.head, !has_body)
            .map_err(|e| fail(error_text(&e)))?;
        if has_body {
            stream
                .send_data(Bytes::from(req.body), true)
                .map_err(|e| fail(error_text(&e)))?;
        }
        receive(
            future,
            limits,
            started,
            Instant::now(),
            SendTiming::default(),
            peer,
        )
        .await
    }
}

/// Sends one request over a new HTTP/2 connection.
pub async fn send_h2(
    destination: &Destination,
    security: &UpstreamSecurity,
    request: &[u8],
    limits: &SendLimits,
) -> Result<Sent, SendError> {
    let started = Instant::now();
    let early = |message: String| SendError {
        message,
        timing: SendTiming {
            total_ms: ms(started),
            ..SendTiming::default()
        },
        peer: None,
    };
    let https = matches!(security, UpstreamSecurity::Tls { .. });
    let req = to_h2(request, https, destination).map_err(early)?;
    let session = open(destination, security).await.map_err(early)?;
    let timing = connect_timing(session.connect);
    let peer = session.peer;
    let fail = |message: String| SendError {
        message,
        timing: SendTiming {
            total_ms: ms(started),
            ..timing
        },
        peer: Some(peer),
    };
    let mut sender = session
        .sender
        .ready()
        .await
        .map_err(|e| fail(error_text(&e)))?;
    let has_body = !req.body.is_empty();
    let (future, mut stream) = sender
        .send_request(req.head, !has_body)
        .map_err(|e| fail(error_text(&e)))?;
    if has_body {
        stream
            .send_data(Bytes::from(req.body), true)
            .map_err(|e| fail(error_text(&e)))?;
    }
    receive(future, limits, started, Instant::now(), timing, peer).await
}

/// What [`to_h2`] would change in `request`, for the history notes.
pub fn translation_notes(request: &[u8], https: bool, destination: &Destination) -> Vec<String> {
    to_h2(request, https, destination).map_or_else(|_| Vec::new(), |r| r.notes)
}

/// Sends several requests on one HTTP/2 connection so that they complete
/// in the same TCP packet ("single-packet attack"): each request is sent
/// without its final frame (the last body byte, or an empty DATA frame
/// ending the stream), then all final frames are written in one go.
/// Results are in the order of `requests`.
pub async fn send_h2_single_packet(
    destination: &Destination,
    security: &UpstreamSecurity,
    requests: &[Vec<u8>],
    limits: &SendLimits,
) -> Vec<Result<Sent, SendError>> {
    let started = Instant::now();
    let all_fail = |message: String, peer: Option<SocketAddr>, timing: SendTiming| {
        (0..requests.len())
            .map(|_| {
                Err(SendError {
                    message: message.clone(),
                    timing: SendTiming {
                        total_ms: ms(started),
                        ..timing
                    },
                    peer,
                })
            })
            .collect::<Vec<_>>()
    };
    if requests.is_empty() || requests.len() > MAX_SINGLE_PACKET {
        return all_fail(
            format!("a single-packet attack sends 1–{MAX_SINGLE_PACKET} requests"),
            None,
            SendTiming::default(),
        );
    }
    let https = matches!(security, UpstreamSecurity::Tls { .. });
    let mut translated = Vec::new();
    for r in requests {
        match to_h2(r, https, destination) {
            Ok(t) => translated.push(t),
            Err(e) => return all_fail(e, None, SendTiming::default()),
        }
    }
    let mut session = match open(destination, security).await {
        Ok(s) => s,
        Err(e) => return all_fail(e, None, SendTiming::default()),
    };
    let timing = connect_timing(session.connect);
    let peer = session.peer;

    // 1. Every request without its final frame.
    let mut streams = Vec::new();
    for t in translated {
        let mut sender = match session.sender.clone().ready().await {
            Ok(s) => s,
            Err(e) => return all_fail(error_text(&e), Some(peer), timing),
        };
        let mut body = t.body;
        let last: Vec<u8> = body.pop().map(|b| vec![b]).unwrap_or_default();
        let (future, mut stream) = match sender.send_request(t.head, false) {
            Ok(x) => x,
            Err(e) => return all_fail(error_text(&e), Some(peer), timing),
        };
        if !body.is_empty()
            && let Err(e) = stream.send_data(Bytes::from(body), false)
        {
            return all_fail(error_text(&e), Some(peer), timing);
        }
        streams.push((future, stream, last));
    }
    // Let the connection write everything so far.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 2. Hold writes, queue every final frame, then release them together.
    gate_lock(&session.gate).hold = true;
    let expected: usize = streams.iter().map(|(_, _, last)| 9 + last.len()).sum();
    let mut futures = Vec::new();
    for (future, mut stream, last) in streams {
        if let Err(e) = stream.send_data(Bytes::from(last), true) {
            gate_lock(&session.gate).hold = false;
            return all_fail(error_text(&e), Some(peer), timing);
        }
        futures.push((future, stream));
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while gate_lock(&session.gate).held.len() < expected && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    gate_lock(&session.gate).hold = false;
    let released = Instant::now();
    // A PING makes the connection write now; the held frames go first, in
    // the same write.
    if let Some(ping) = session.ping.as_mut() {
        let _ = ping.send_ping(h2::Ping::opaque());
    }

    // 3. Responses.
    let limits = *limits;
    let handles: Vec<_> = futures
        .into_iter()
        .map(|(future, _stream)| {
            tokio::spawn(
                async move { receive(future, &limits, started, released, timing, peer).await },
            )
        })
        .collect();
    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        out.push(h.await.unwrap_or_else(|e| {
            Err(SendError {
                message: format!("the response could not be read: {e}"),
                timing,
                peer: Some(peer),
            })
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chosproxy_lab::LabServer;
    use chosproxy_tls::{UpstreamTls, UpstreamVerification};

    fn dest(addr: SocketAddr) -> Destination {
        Destination {
            host: "127.0.0.1".into(),
            port: addr.port(),
            tls_name: "127.0.0.1".into(),
        }
    }

    fn tls() -> UpstreamSecurity {
        UpstreamSecurity::Tls {
            tls: UpstreamTls::new(&[]).unwrap(),
            verification: UpstreamVerification::NotVerified,
        }
    }

    fn limits() -> SendLimits {
        SendLimits {
            response_timeout: Duration::from_secs(10),
            body_idle_timeout: Duration::from_secs(10),
            max_response_bytes: 1 << 20,
        }
    }

    #[test]
    fn translates_text_requests() {
        let d = Destination {
            host: "lab.test".into(),
            port: 8443,
            tls_name: "lab.test".into(),
        };
        let raw = b"POST /a?b=1 HTTP/2\r\nHost: api.lab.test\r\nConnection: keep-alive\r\nX-Case: 1\r\nTransfer-Encoding: chunked\r\nTE: trailers\r\n\r\n3\r\nabc\r\n0\r\n\r\n";
        let r = to_h2(raw, true, &d).unwrap();
        assert_eq!(r.head.method(), http::Method::POST);
        assert_eq!(r.head.uri().to_string(), "https://api.lab.test/a?b=1");
        assert!(r.head.headers().get("host").is_none());
        assert_eq!(r.head.headers()["x-case"], "1");
        assert_eq!(r.head.headers()["te"], "trailers");
        assert!(r.head.headers().get("connection").is_none());
        assert_eq!(r.body, b"abc");
        assert!(
            r.notes
                .iter()
                .any(|n| n.contains("Connection, Transfer-Encoding"))
        );
        // No Host: the destination is the authority (default port omitted).
        let r = to_h2(b"GET /x HTTP/2\r\n\r\n", true, &d).unwrap();
        assert_eq!(r.head.uri().to_string(), "https://lab.test:8443/x");
        let r = to_h2(b"GET http://h.test:81/p HTTP/2\r\n\r\n", false, &d).unwrap();
        assert_eq!(r.head.uri().to_string(), "http://h.test:81/p");
        assert!(to_h2(b"GET /x HTTP/2\r\nBad Name: 1\r\n\r\n", true, &d).is_err());
    }

    #[tokio::test]
    async fn the_gate_writes_held_bytes_with_the_next_write() {
        let (a, mut b) = tokio::io::duplex(1024);
        let state = Arc::new(Mutex::new(GateState::default()));
        let mut g = Gated {
            inner: a,
            state: state.clone(),
            pending: Vec::new(),
        };
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        g.write_all(b"a").await.unwrap();
        gate_lock(&state).hold = true;
        g.write_all(b"bc").await.unwrap();
        assert_eq!(gate_lock(&state).held, b"bc");
        gate_lock(&state).hold = false;
        g.write_all(b"d").await.unwrap();
        g.flush().await.unwrap();
        let mut out = [0u8; 4];
        b.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abcd");
    }

    #[tokio::test]
    async fn sends_over_tls_and_cleartext_http2() {
        let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let sent = send_h2(
            &dest(lab.addr()),
            &tls(),
            b"POST /version HTTP/2\r\nHost: 127.0.0.1\r\nContent-Length: 5\r\n\r\nhello",
            &limits(),
        )
        .await
        .unwrap();
        let text = String::from_utf8_lossy(&sent.response).to_string();
        assert!(text.starts_with("HTTP/2 200\r\n"), "{text}");
        assert!(text.contains(r#""version":"HTTP/2""#), "{text}");
        assert!(text.contains(r#""body":"hello""#), "{text}");
        assert_eq!(sent.head.status, 200);
        // Timings are in whole ms: a loopback TLS handshake can take less.
        assert!(sent.timing.total_ms >= sent.timing.tls_ms);

        let plain = LabServer::start("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let sent = send_h2(
            &dest(plain.addr()),
            &UpstreamSecurity::Plain,
            b"GET /version HTTP/2\r\n\r\n",
            &limits(),
        )
        .await
        .unwrap();
        assert!(String::from_utf8_lossy(&sent.response).contains(r#""version":"HTTP/2""#));
    }

    #[tokio::test]
    async fn single_packet_attack_completes_every_request() {
        let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let requests: Vec<Vec<u8>> = (1..=5)
            .map(|i| {
                format!("POST /version HTTP/2\r\nHost: 127.0.0.1\r\nContent-Length: 2\r\n\r\nr{i}")
                    .into_bytes()
            })
            .chain(std::iter::once(b"GET /version HTTP/2\r\n\r\n".to_vec()))
            .collect();
        let results = send_h2_single_packet(&dest(lab.addr()), &tls(), &requests, &limits()).await;
        assert_eq!(results.len(), 6);
        for (i, r) in results.iter().enumerate() {
            let sent = r.as_ref().unwrap();
            let text = String::from_utf8_lossy(&sent.response).to_string();
            if i < 5 {
                assert!(text.contains(&format!(r#""body":"r{}""#, i + 1)), "{text}");
            } else {
                assert!(text.contains(r#""method":"GET""#), "{text}");
            }
        }
        let none = send_h2_single_packet(&dest(lab.addr()), &tls(), &[], &limits()).await;
        assert!(none.is_empty());
    }
}
