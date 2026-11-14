//! The interaction server: records every DNS lookup and HTTP(S) request it
//! receives, with the correlation identifiers found in it.
//!
//! It only observes. HTTP clients get a fixed small response and DNS
//! clients the zone's address; nothing received is ever executed or
//! reflected back.

use crate::dns::{self, ZoneAddresses};
use crate::id::extract_ids;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::{Semaphore, broadcast};
use tokio::task::JoinHandle;

/// Bytes of a request kept as evidence.
pub const MAX_RAW: usize = 64 * 1024;
/// Time a client has to send its request.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Connections handled at once per listener.
const MAX_CONNECTIONS: usize = 256;
/// DNS answers per source address per second.
const DNS_RATE: u32 = 20;

/// How the interaction reached the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// A DNS lookup.
    Dns,
    /// A plain HTTP request.
    Http,
    /// An HTTP request over TLS.
    Https,
    /// A mail delivered over SMTP (M31b).
    Smtp,
}

impl Protocol {
    /// Lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Dns => "dns",
            Protocol::Http => "http",
            Protocol::Https => "https",
            Protocol::Smtp => "smtp",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(s: &str) -> Option<Protocol> {
        [
            Protocol::Dns,
            Protocol::Http,
            Protocol::Https,
            Protocol::Smtp,
        ]
        .into_iter()
        .find(|p| p.as_str() == s)
    }
}

/// One recorded interaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interaction {
    /// Position in the server's log (increasing).
    pub seq: u64,
    /// Unix epoch ms.
    pub at: i64,
    /// Protocol.
    pub protocol: Protocol,
    /// Who contacted the server (`ip:port`).
    pub remote: String,
    /// Identifiers found (lowercase).
    pub ids: Vec<String>,
    /// One line: the DNS question or the HTTP request line.
    pub summary: String,
    /// The DNS packet or the HTTP request (cut at [`MAX_RAW`]).
    #[serde(with = "b64")]
    pub raw: Vec<u8>,
}

mod b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}

/// What to listen on.
#[derive(Clone)]
pub struct ServerConfig {
    /// The DNS zone (e.g. `oast.example.com`); names are `<id>.<zone>`.
    pub zone: String,
    /// Addresses answered for names in the zone.
    pub addresses: ZoneAddresses,
    /// Plain HTTP listener.
    pub http: Option<SocketAddr>,
    /// HTTPS listener and its certificate.
    pub https: Option<(SocketAddr, Arc<rustls::ServerConfig>)>,
    /// DNS (UDP) listener.
    pub dns: Option<SocketAddr>,
    /// SMTP listener (M31b): records mail, never relays it.
    pub smtp: Option<SocketAddr>,
    /// Interactions kept in memory (oldest dropped first).
    pub keep: usize,
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("zone", &self.zone)
            .field("http", &self.http)
            .field("https", &self.https.as_ref().map(|h| h.0))
            .field("dns", &self.dns)
            .finish_non_exhaustive()
    }
}

/// The interaction log shared by the listeners.
#[derive(Debug)]
pub struct Log {
    inner: Mutex<(u64, VecDeque<Interaction>)>,
    keep: usize,
    tx: broadcast::Sender<Interaction>,
}

impl Log {
    fn new(keep: usize) -> Log {
        Log {
            inner: Mutex::new((0, VecDeque::new())),
            keep: keep.max(1),
            tx: broadcast::channel(1024).0,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, (u64, VecDeque<Interaction>)> {
        match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn record(
        &self,
        protocol: Protocol,
        remote: SocketAddr,
        summary: String,
        mut raw: Vec<u8>,
        ids: Vec<String>,
    ) {
        raw.truncate(MAX_RAW);
        let interaction = {
            let mut g = self.lock();
            g.0 += 1;
            let i = Interaction {
                seq: g.0,
                at: chosproxy_core::time::now_ms(),
                protocol,
                remote: remote.to_string(),
                ids,
                summary: summary.chars().take(500).collect(),
                raw,
            };
            if g.1.len() >= self.keep {
                g.1.pop_front();
            }
            g.1.push_back(i.clone());
            i
        };
        let _ = self.tx.send(interaction);
    }

    /// Interactions after `seq`, oldest first, at most `limit`.
    pub fn after(&self, seq: u64, limit: usize) -> Vec<Interaction> {
        self.lock()
            .1
            .iter()
            .filter(|i| i.seq > seq)
            .take(limit)
            .cloned()
            .collect()
    }

    /// The newest sequence number.
    pub fn latest(&self) -> u64 {
        self.lock().0
    }

    /// New interactions as they arrive.
    pub fn subscribe(&self) -> broadcast::Receiver<Interaction> {
        self.tx.subscribe()
    }
}

/// A running server; dropping it stops every listener.
#[derive(Debug)]
pub struct OastServer {
    log: Arc<Log>,
    http: Option<SocketAddr>,
    https: Option<SocketAddr>,
    dns: Option<SocketAddr>,
    smtp: Option<SocketAddr>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for OastServer {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

impl OastServer {
    /// Binds every configured listener and starts serving.
    pub async fn start(config: ServerConfig) -> std::io::Result<OastServer> {
        let log = Arc::new(Log::new(config.keep));
        let mut tasks = Vec::new();
        let mut server = OastServer {
            log: log.clone(),
            http: None,
            https: None,
            dns: None,
            smtp: None,
            tasks: Vec::new(),
        };
        if let Some(addr) = config.http {
            let listener = TcpListener::bind(addr).await?;
            server.http = Some(listener.local_addr()?);
            tasks.push(tokio::spawn(serve_http(listener, None, log.clone())));
        }
        if let Some((addr, tls)) = config.https.clone() {
            let listener = TcpListener::bind(addr).await?;
            server.https = Some(listener.local_addr()?);
            let acceptor = tokio_rustls::TlsAcceptor::from(tls);
            tasks.push(tokio::spawn(serve_http(
                listener,
                Some(acceptor),
                log.clone(),
            )));
        }
        if let Some(addr) = config.dns {
            let socket = UdpSocket::bind(addr).await?;
            server.dns = Some(socket.local_addr()?);
            tasks.push(tokio::spawn(serve_dns(
                socket,
                config.zone.clone(),
                config.addresses,
                log.clone(),
            )));
        }
        if let Some(addr) = config.smtp {
            let listener = TcpListener::bind(addr).await?;
            server.smtp = Some(listener.local_addr()?);
            tasks.push(tokio::spawn(serve_smtp(
                listener,
                config.zone.clone(),
                log.clone(),
            )));
        }
        server.tasks = tasks;
        Ok(server)
    }

    /// The interaction log.
    pub fn log(&self) -> &Arc<Log> {
        &self.log
    }

    /// Bound HTTP address.
    pub fn http_addr(&self) -> Option<SocketAddr> {
        self.http
    }

    /// Bound HTTPS address.
    pub fn https_addr(&self) -> Option<SocketAddr> {
        self.https
    }

    /// Bound DNS address.
    pub fn dns_addr(&self) -> Option<SocketAddr> {
        self.dns
    }

    /// Bound SMTP address.
    pub fn smtp_addr(&self) -> Option<SocketAddr> {
        self.smtp
    }
}

/// Commands accepted on one SMTP connection.
const SMTP_MAX_COMMANDS: usize = 64;
/// Recipients kept of one mail.
const SMTP_MAX_RECIPIENTS: usize = 20;
/// Longest SMTP command line.
const SMTP_MAX_LINE: usize = 2048;

/// Accepts mail and records it. It is a sink: nothing is relayed, stored
/// as mail or answered beyond the SMTP replies; no AUTH, no STARTTLS.
async fn serve_smtp(listener: TcpListener, zone: String, log: Arc<Log>) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let Some((stream, remote)) = accept(&listener).await else {
            continue;
        };
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            continue; // busy: drop the connection
        };
        let log = log.clone();
        let zone = zone.clone();
        tokio::spawn(async move {
            let _permit = permit;
            handle_smtp(stream, remote, &zone, &log).await;
        });
    }
}

/// One line (without its line break), or `None` at the end, on a timeout or
/// when the line is too long.
async fn smtp_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> Option<Vec<u8>> {
    use tokio::io::AsyncBufReadExt;
    let mut line = Vec::new();
    let read = tokio::time::timeout(
        READ_TIMEOUT,
        (&mut *reader)
            .take(limit as u64 + 2)
            .read_until(b'\n', &mut line),
    )
    .await;
    match read {
        Ok(Ok(n)) if n > 0 && line.ends_with(b"\n") => {
            line.pop();
            if line.ends_with(b"\r") {
                line.pop();
            }
            Some(line)
        }
        _ => None,
    }
}

/// The address between `<` and `>` of `MAIL FROM:<a>` / `RCPT TO:<b>`.
fn smtp_address(argument: &str) -> String {
    let inner = match (argument.find('<'), argument.find('>')) {
        (Some(a), Some(b)) if a < b => &argument[a + 1..b],
        _ => argument.split_whitespace().next().unwrap_or(""),
    };
    inner.chars().take(320).collect()
}

async fn handle_smtp<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    remote: SocketAddr,
    zone: &str,
    log: &Log,
) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(read);
    let say = async |write: &mut tokio::io::WriteHalf<S>, text: &str| {
        tokio::time::timeout(READ_TIMEOUT, write.write_all(text.as_bytes()))
            .await
            .is_ok_and(|r| r.is_ok())
    };
    if !say(
        &mut write,
        &format!("220 {zone} ChosOAST (mail is recorded, never delivered)\r\n"),
    )
    .await
    {
        return;
    }
    let mut from = String::new();
    let mut recipients: Vec<String> = Vec::new();
    // The envelope lines as received, for the record.
    let mut envelope: Vec<u8> = Vec::new();
    let record = |from: &str, recipients: &[String], envelope: &[u8], data: &[u8], cut: bool| {
        if from.is_empty() && recipients.is_empty() && data.is_empty() {
            return;
        }
        let mut raw = envelope.to_vec();
        raw.extend_from_slice(b"\r\n");
        raw.extend_from_slice(data);
        let ids = extract_ids(&raw);
        let summary = format!(
            "MAIL FROM:<{from}> RCPT TO:<{}>{}",
            recipients.join(">, <"),
            if cut { " (body cut)" } else { "" }
        );
        log.record(Protocol::Smtp, remote, summary, raw, ids);
    };
    for _ in 0..SMTP_MAX_COMMANDS {
        let Some(line) = smtp_line(&mut reader, SMTP_MAX_LINE).await else {
            break;
        };
        let text = String::from_utf8_lossy(&line).into_owned();
        let upper = text.to_ascii_uppercase();
        let reply = if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            format!("250 {zone}\r\n")
        } else if upper.starts_with("MAIL FROM:") {
            from = smtp_address(&text[10..]);
            envelope.extend_from_slice(&line);
            envelope.extend_from_slice(b"\r\n");
            "250 OK\r\n".to_string()
        } else if upper.starts_with("RCPT TO:") {
            if recipients.len() >= SMTP_MAX_RECIPIENTS {
                "452 too many recipients\r\n".to_string()
            } else {
                recipients.push(smtp_address(&text[8..]));
                envelope.extend_from_slice(&line);
                envelope.extend_from_slice(b"\r\n");
                "250 OK\r\n".to_string()
            }
        } else if upper == "DATA" {
            if recipients.is_empty() {
                "503 RCPT TO first\r\n".to_string()
            } else {
                if !say(&mut write, "354 end with a line with only a dot\r\n").await {
                    break;
                }
                // The mail up to the lone dot; kept up to the limit, read
                // (and dropped) up to ten times that, then the client is
                // let go.
                let mut data: Vec<u8> = Vec::new();
                let mut seen = 0usize;
                let mut complete = false;
                while seen <= MAX_RAW * 10 {
                    let Some(l) = smtp_line(&mut reader, MAX_RAW).await else {
                        break;
                    };
                    if l == b"." {
                        complete = true;
                        break;
                    }
                    seen += l.len() + 2;
                    if data.len() < MAX_RAW {
                        // A leading dot is doubled on the wire (RFC 5321).
                        let l = l.strip_prefix(b".").unwrap_or(&l);
                        data.extend_from_slice(l);
                        data.extend_from_slice(b"\r\n");
                    }
                }
                let cut = seen > MAX_RAW || !complete;
                record(&from, &recipients, &envelope, &data, cut);
                from.clear();
                recipients.clear();
                envelope.clear();
                if !complete {
                    break;
                }
                "250 OK: recorded, not delivered\r\n".to_string()
            }
        } else if upper == "RSET" {
            from.clear();
            recipients.clear();
            envelope.clear();
            "250 OK\r\n".to_string()
        } else if upper == "NOOP" {
            "250 OK\r\n".to_string()
        } else if upper == "QUIT" {
            let _ = say(&mut write, "221 bye\r\n").await;
            break;
        } else {
            // AUTH, STARTTLS, VRFY, EXPN and anything else.
            "502 not implemented\r\n".to_string()
        };
        if !say(&mut write, &reply).await {
            break;
        }
    }
    // An envelope without a mail still shows the server was contacted.
    record(&from, &recipients, &envelope, &[], false);
    let _ = write.shutdown().await;
}

/// The next connection; after a failed `accept` (typically no file
/// descriptors left) it waits a moment instead of spinning on the error.
pub(crate) async fn accept(listener: &TcpListener) -> Option<(tokio::net::TcpStream, SocketAddr)> {
    match listener.accept().await {
        Ok(x) => Some(x),
        Err(e) => {
            tracing::warn!(error = %e, "OAST accept failed");
            tokio::time::sleep(Duration::from_millis(100)).await;
            None
        }
    }
}

const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\n\
Content-Length: 9\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nchosoast\n";

async fn serve_http(listener: TcpListener, tls: Option<tokio_rustls::TlsAcceptor>, log: Arc<Log>) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let Some((stream, remote)) = accept(&listener).await else {
            continue;
        };
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            continue; // busy: drop the connection
        };
        let log = log.clone();
        let tls = tls.clone();
        tokio::spawn(async move {
            let _permit = permit;
            match tls {
                None => handle_http(stream, remote, Protocol::Http, &log).await,
                Some(acceptor) => {
                    if let Ok(Ok(s)) =
                        tokio::time::timeout(READ_TIMEOUT, acceptor.accept(stream)).await
                    {
                        handle_http(s, remote, Protocol::Https, &log).await;
                    }
                }
            }
        });
    }
}

/// Reads one request (head plus any body up to [`MAX_RAW`]) and records it.
async fn handle_http<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    remote: SocketAddr,
    protocol: Protocol,
    log: &Log,
) {
    let deadline = Instant::now() + READ_TIMEOUT;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut wanted: Option<usize> = None;
    loop {
        if let Some(total) = wanted
            && buf.len() >= total
        {
            break;
        }
        if buf.len() >= MAX_RAW {
            break;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        let read = match tokio::time::timeout(left, stream.read(&mut chunk)).await {
            Ok(Ok(0)) | Err(_) | Ok(Err(_)) => break,
            Ok(Ok(n)) => n,
        };
        buf.extend_from_slice(&chunk[..read]);
        if wanted.is_none()
            && let Some(end) = chosproxy_http::head::find_head_end(&buf)
        {
            wanted = Some(end + content_length(&buf[..end]).min(MAX_RAW));
        }
    }
    if buf.is_empty() {
        return;
    }
    let summary = buf
        .split(|b| *b == b'\n')
        .next()
        .map(|l| String::from_utf8_lossy(l).trim().to_string())
        .unwrap_or_default();
    let ids = extract_ids(&buf);
    log.record(protocol, remote, summary, buf, ids);
    let _ = tokio::time::timeout(READ_TIMEOUT, stream.write_all(RESPONSE)).await;
    let _ = stream.shutdown().await;
}

fn content_length(head: &[u8]) -> usize {
    String::from_utf8_lossy(head)
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(n, _)| n.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0)
}

async fn serve_dns(socket: UdpSocket, zone: String, addresses: ZoneAddresses, log: Arc<Log>) {
    let mut buf = [0u8; dns::MAX_PACKET];
    let mut budget: HashMap<IpAddr, (Instant, u32)> = HashMap::new();
    loop {
        let Ok((n, remote)) = socket.recv_from(&mut buf).await else {
            continue;
        };
        let packet = &buf[..n];
        let Ok(query) = dns::parse_query(packet) else {
            continue;
        };
        // Per-source budget: a spoofed flood cannot be reflected at volume.
        let now = Instant::now();
        if budget.len() > 10_000 {
            budget.retain(|_, (since, _)| now.duration_since(*since) < Duration::from_secs(1));
        }
        let entry = budget.entry(remote.ip()).or_insert((now, 0));
        if now.duration_since(entry.0) >= Duration::from_secs(1) {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 > DNS_RATE {
            continue; // over budget: neither recorded nor answered
        }
        let inside = dns::in_zone(&query.name, &zone);
        if inside {
            let ids = extract_ids(query.name.as_bytes());
            let kind = match query.qtype {
                dns::TYPE_A => "A".to_string(),
                dns::TYPE_AAAA => "AAAA".to_string(),
                t => format!("type {t}"),
            };
            log.record(
                Protocol::Dns,
                remote,
                format!("{kind} {}", query.name),
                packet.to_vec(),
                ids,
            );
        }
        let answer = dns::respond(&query, &zone, addresses);
        let _ = socket.send_to(&answer, remote).await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::id::new_id;
    use std::net::Ipv4Addr;

    fn local() -> SocketAddr {
        "127.0.0.1:0".parse().unwrap()
    }

    #[tokio::test]
    async fn records_http_and_dns_interactions() {
        let server = OastServer::start(ServerConfig {
            zone: "oast.test".into(),
            addresses: ZoneAddresses {
                ipv4: Some(Ipv4Addr::LOCALHOST),
                ipv6: None,
            },
            http: Some(local()),
            https: None,
            dns: Some(local()),
            smtp: None,
            keep: 100,
        })
        .await
        .unwrap();
        let mut events = server.log().subscribe();
        let id = new_id().unwrap();

        let mut s = tokio::net::TcpStream::connect(server.http_addr().unwrap())
            .await
            .unwrap();
        s.write_all(
            format!(
                "POST /{id}?x=1 HTTP/1.1\r\nHost: {id}.oast.test\r\nContent-Length: 5\r\n\r\nhello"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut reply = Vec::new();
        s.read_to_end(&mut reply).await.unwrap();
        assert!(reply.ends_with(b"chosoast\n"));
        let http = events.recv().await.unwrap();
        assert_eq!(http.protocol, Protocol::Http);
        assert_eq!(http.ids, vec![id.clone()]);
        assert!(http.summary.starts_with("POST /"));
        assert!(http.raw.ends_with(b"hello"));

        let client = UdpSocket::bind(local()).await.unwrap();
        let name = format!("{}.oast.test", id.to_uppercase());
        client
            .send_to(
                &dns::build_query(9, &name, dns::TYPE_A),
                server.dns_addr().unwrap(),
            )
            .await
            .unwrap();
        let mut answer = [0u8; 512];
        let (n, _) = client.recv_from(&mut answer).await.unwrap();
        assert_eq!(&answer[n - 4..n], &[127, 0, 0, 1]);
        let lookup = events.recv().await.unwrap();
        assert_eq!(lookup.protocol, Protocol::Dns);
        assert_eq!(lookup.ids, vec![id.clone()]);
        assert_eq!(lookup.summary, format!("A {}.oast.test", id));

        // Lookups outside the zone are refused and not recorded.
        client
            .send_to(
                &dns::build_query(10, "example.com", dns::TYPE_A),
                server.dns_addr().unwrap(),
            )
            .await
            .unwrap();
        let (n, _) = client.recv_from(&mut answer).await.unwrap();
        assert_eq!(answer[3] & 0x0f, 5);
        assert!(n < 64);
        assert_eq!(server.log().latest(), 2);
        assert_eq!(server.log().after(1, 10).len(), 1);
    }

    #[tokio::test]
    async fn keeps_only_the_newest() {
        let log = Log::new(3);
        for i in 0..5 {
            log.record(Protocol::Http, local(), format!("r{i}"), vec![], vec![]);
        }
        let all = log.after(0, 10);
        assert_eq!(all.iter().map(|i| i.seq).collect::<Vec<_>>(), vec![3, 4, 5]);
    }

    #[tokio::test]
    async fn smtp_records_mail_and_relays_nothing() {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let server = OastServer::start(ServerConfig {
            zone: "oast.test".into(),
            addresses: ZoneAddresses::default(),
            http: None,
            https: None,
            dns: None,
            smtp: Some("127.0.0.1:0".parse().unwrap()),
            keep: 10,
        })
        .await
        .unwrap();
        let id = crate::new_id().unwrap();
        let stream = tokio::net::TcpStream::connect(server.smtp_addr().unwrap())
            .await
            .unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        type Lines = tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>;
        async fn step(
            lines: &mut Lines,
            write: &mut tokio::net::tcp::OwnedWriteHalf,
            send: &str,
            code: &str,
        ) {
            if !send.is_empty() {
                write.write_all(send.as_bytes()).await.unwrap();
            }
            let line = lines.next_line().await.unwrap().unwrap();
            assert!(line.starts_with(code), "{send:?} -> {line}");
        }
        step(&mut lines, &mut write, "", "220").await;
        step(&mut lines, &mut write, "EHLO client.test\r\n", "250").await;
        step(&mut lines, &mut write, "STARTTLS\r\n", "502").await;
        step(&mut lines, &mut write, "AUTH PLAIN AAAA\r\n", "502").await;
        step(&mut lines, &mut write, "DATA\r\n", "503").await;
        step(
            &mut lines,
            &mut write,
            "MAIL FROM:<app@target.test>\r\n",
            "250",
        )
        .await;
        step(
            &mut lines,
            &mut write,
            &format!("RCPT TO:<{id}@oast.test>\r\n"),
            "250",
        )
        .await;
        step(&mut lines, &mut write, "DATA\r\n", "354").await;
        step(
            &mut lines,
            &mut write,
            "Subject: reset\r\n\r\nHello\r\n..dot line\r\n.\r\n",
            "250",
        )
        .await;
        step(&mut lines, &mut write, "QUIT\r\n", "221").await;
        let seen = server.log().after(0, 10);
        assert_eq!(seen.len(), 1, "{seen:?}");
        let mail = &seen[0];
        assert_eq!(mail.protocol, Protocol::Smtp);
        assert_eq!(mail.ids, vec![id.clone()]);
        assert_eq!(
            mail.summary,
            format!("MAIL FROM:<app@target.test> RCPT TO:<{id}@oast.test>")
        );
        let raw = String::from_utf8_lossy(&mail.raw);
        assert!(
            raw.contains("Subject: reset") && raw.contains("\r\n.dot line\r\n"),
            "{raw}"
        );

        // A mail larger than the limit is cut, and still acknowledged.
        let stream = tokio::net::TcpStream::connect(server.smtp_addr().unwrap())
            .await
            .unwrap();
        let (read, mut write) = stream.into_split();
        lines = BufReader::new(read).lines();
        step(&mut lines, &mut write, "", "220").await;
        step(&mut lines, &mut write, "MAIL FROM:<a@b>\r\n", "250").await;
        step(&mut lines, &mut write, "RCPT TO:<c@d>\r\n", "250").await;
        step(&mut lines, &mut write, "DATA\r\n", "354").await;
        let big = format!("{}\r\n", "x".repeat(1000)).repeat(MAX_RAW / 1000 + 20);
        step(&mut lines, &mut write, &format!("{big}.\r\n"), "250").await;
        let seen = server.log().after(1, 10);
        assert_eq!(seen.len(), 1);
        assert!(
            seen[0].summary.ends_with("(body cut)"),
            "{}",
            seen[0].summary
        );
        assert!(seen[0].raw.len() <= MAX_RAW);
        assert_eq!(Protocol::parse("smtp"), Some(Protocol::Smtp));
    }
}
