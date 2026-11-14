//! End-to-end tests: real TCP clients → proxy → real local servers.
//!
//! Upstreams are either the hyper-based lab server (independent HTTP
//! implementation) or tiny scripted servers for edge cases.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_core::config::ListenerConfig;
use chosproxy_core::event::ListenerState;
use chosproxy_core::exchange::CapturedExchange;
use chosproxy_http::content::{dechunk, split_message};
use chosproxy_http::framing::response_framing;
use chosproxy_http::head::{Headers, read_head};
use chosproxy_http::{BodyReader, ResponseHead};
use chosproxy_lab::LabServer;
use chosproxy_proxy::{CaptureLimits, CaptureSink, ProxyObserver, ProxyService};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

#[derive(Default)]
struct MemorySink(Mutex<Vec<CapturedExchange>>);

impl CaptureSink for MemorySink {
    fn record(&self, exchange: CapturedExchange) {
        self.0.lock().unwrap().push(exchange);
    }
}

impl MemorySink {
    async fn wait_for(&self, n: usize) -> Vec<CapturedExchange> {
        for _ in 0..200 {
            {
                let v = self.0.lock().unwrap();
                if v.len() >= n {
                    return v.clone();
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "expected {n} exchanges, got {}",
            self.0.lock().unwrap().len()
        );
    }

    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

struct NoObserver;
impl ProxyObserver for NoObserver {
    fn listener_changed(&self, _: &str, _: &ListenerState) {}
}

struct Harness {
    proxy: ProxyService,
    addr: SocketAddr,
    sink: Arc<MemorySink>,
}

async fn harness_with_limit(max_message_bytes: usize) -> Harness {
    let sink = Arc::new(MemorySink::default());
    let proxy = ProxyService::new(
        sink.clone(),
        Arc::new(NoObserver),
        CaptureLimits { max_message_bytes },
    );
    let addr = proxy
        .start(&ListenerConfig {
            port: 0,
            ..ListenerConfig::default()
        })
        .await
        .unwrap();
    Harness { proxy, addr, sink }
}

async fn harness() -> Harness {
    harness_with_limit(16 * 1024 * 1024).await
}

struct Client {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl Client {
    async fn connect(addr: SocketAddr) -> Client {
        let (r, w) = TcpStream::connect(addr).await.unwrap().into_split();
        Client {
            reader: BufReader::new(r),
            writer: w,
        }
    }

    async fn send(&mut self, raw: &[u8]) {
        self.writer.write_all(raw).await.unwrap();
    }

    /// Reads one response; returns the head and the de-framed body bytes.
    async fn response(&mut self, method: &str) -> (ResponseHead, Vec<u8>) {
        let raw = tokio::time::timeout(Duration::from_secs(10), read_head(&mut self.reader, 65536))
            .await
            .expect("response head in time")
            .unwrap()
            .expect("a response");
        let head = ResponseHead::parse(raw).unwrap();
        let framing = response_framing(method, &head).unwrap();
        let mut body = BodyReader::new(framing.kind);
        let mut raw_body = Vec::new();
        while let Some(p) = body.next_chunk(&mut self.reader).await.unwrap() {
            raw_body.extend(p);
        }
        let is_chunked = head
            .header_tokens("transfer-encoding")
            .last()
            .is_some_and(|t| t == "chunked");
        let body = if is_chunked {
            dechunk(&raw_body).unwrap().0
        } else {
            raw_body
        };
        (head, body)
    }

    async fn is_closed(&mut self) -> bool {
        let mut buf = [0u8; 1];
        matches!(
            tokio::time::timeout(Duration::from_secs(5), self.reader.read(&mut buf)).await,
            Ok(Ok(0)) | Ok(Err(_))
        )
    }
}

fn get(url: &str) -> Vec<u8> {
    let host = url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap()
        .to_string();
    format!("GET {url} HTTP/1.1\r\nHost: {host}\r\nProxy-Connection: keep-alive\r\nUser-Agent: test\r\n\r\n")
        .into_bytes()
}

#[tokio::test]
async fn relays_and_captures_a_get() {
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    c.send(&get(&format!("{}/json", lab.url()))).await;
    let (head, body) = c.response("GET").await;
    assert_eq!(head.status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"name\":\"lab\""));

    let ex = h.sink.wait_for(1).await.remove(0);
    assert_eq!(ex.method, "GET");
    assert_eq!(ex.status, Some(200));
    assert_eq!(ex.mime.as_deref(), Some("JSON"));
    assert_eq!(ex.host, "127.0.0.1");
    assert_eq!(ex.port, lab.addr().port());
    assert_eq!(ex.url, format!("{}/json", lab.url()));
    assert_eq!(ex.server_ip.as_deref(), Some("127.0.0.1"));
    assert!(ex.error.is_none());
    let req = String::from_utf8(ex.request.clone()).unwrap();
    assert!(
        req.starts_with("GET /json HTTP/1.1\r\n"),
        "origin-form upstream: {req}"
    );
    assert!(!req.contains("Proxy-Connection"), "proxy headers stripped");
    assert!(req.contains("User-Agent: test\r\n"));
    let (res_head, res_body) = split_message(ex.response.as_deref().unwrap());
    assert!(res_head.starts_with(b"HTTP/1.1 200 OK"));
    assert_eq!(res_body, body.as_slice());
    assert_eq!(
        ex.response_len,
        Some(ex.response.as_ref().unwrap().len() as u64)
    );
}

#[tokio::test]
async fn keeps_client_connection_alive_across_requests() {
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    for path in ["/json", "/headers", "/status/404"] {
        c.send(&get(&format!("{}{path}", lab.url()))).await;
        let (head, _) = c.response("GET").await;
        assert!(head.status == 200 || head.status == 404);
    }
    let all = h.sink.wait_for(3).await;
    assert_eq!(all[2].status, Some(404));
    assert_eq!(h.proxy.stats().total_connections, 1);
}

#[tokio::test]
async fn relays_request_bodies_length_and_chunked() {
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h = harness().await;
    let url = format!("{}/echo", lab.url());
    let mut c = Client::connect(h.addr).await;
    c.send(
        format!("POST {url} HTTP/1.1\r\nHost: x\r\nContent-Length: 11\r\n\r\nhello world")
            .as_bytes(),
    )
    .await;
    let (_, body) = c.response("POST").await;
    assert!(String::from_utf8_lossy(&body).contains("\"length\":11"));

    c.send(
        format!("POST {url} HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n")
            .as_bytes(),
    )
    .await;
    let (_, body) = c.response("POST").await;
    assert!(String::from_utf8_lossy(&body).contains("\"body\":\"hello world\""));

    let all = h.sink.wait_for(2).await;
    assert!(
        all[1]
            .request
            .ends_with(b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"),
        "raw chunked framing kept"
    );
}

#[tokio::test]
async fn answers_expect_100_continue() {
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    c.send(
        format!(
            "POST {}/echo HTTP/1.1\r\nHost: x\r\nExpect: 100-continue\r\nContent-Length: 4\r\n\r\n",
            lab.url()
        )
        .as_bytes(),
    )
    .await;
    let (interim, _) = c.response("POST").await;
    assert_eq!(interim.status, 100);
    c.send(b"data").await;
    let (head, body) = c.response("POST").await;
    assert_eq!(head.status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"length\":4"));
    let ex = h.sink.wait_for(1).await.remove(0);
    assert!(!String::from_utf8_lossy(&ex.request).contains("Expect:"));
}

#[tokio::test]
async fn chunked_and_gzip_responses_are_relayed_byte_exact() {
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    c.send(&get(&format!("{}/chunked", lab.url()))).await;
    let (head, body) = c.response("GET").await;
    assert_eq!(
        head.header_str("transfer-encoding").as_deref(),
        Some("chunked")
    );
    assert_eq!(body, b"first,second,third");
    c.send(&get(&format!("{}/gzip", lab.url()))).await;
    let (head, body) = c.response("GET").await;
    assert_eq!(head.header_str("content-encoding").as_deref(), Some("gzip"));
    assert_eq!(&body[..2], &[0x1f, 0x8b], "still compressed for the client");

    let all = h.sink.wait_for(2).await;
    let (_, raw_chunked) = split_message(all[0].response.as_deref().unwrap());
    assert!(
        raw_chunked.ends_with(b"0\r\n\r\n"),
        "stored with chunk framing"
    );
    assert_eq!(dechunk(raw_chunked).unwrap().0, b"first,second,third");
    assert_eq!(all[1].mime.as_deref(), Some("JSON"));
}

#[tokio::test]
async fn large_bodies_stream_fully_but_capture_is_capped() {
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h = harness_with_limit(4096).await;
    let mut c = Client::connect(h.addr).await;
    c.send(&get(&format!("{}/large?bytes=300000", lab.url())))
        .await;
    let (_, body) = c.response("GET").await;
    assert_eq!(body.len(), 300_000);
    let ex = h.sink.wait_for(1).await.remove(0);
    assert_eq!(ex.response.as_ref().unwrap().len(), 4096);
    assert!(ex.response_len.unwrap() > 300_000);
    assert_ne!(
        ex.flags & chosproxy_core::exchange::flags::RESPONSE_TRUNCATED,
        0
    );
}

#[tokio::test]
async fn unreachable_upstream_gets_502_and_is_recorded() {
    // Bind then drop to obtain a port with nothing listening.
    let dead = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    c.send(&get(&format!("http://{dead}/x"))).await;
    let (head, body) = c.response("GET").await;
    assert_eq!(head.status, 502);
    assert!(String::from_utf8_lossy(&body).starts_with(&chosproxy_core::brand().product_name));
    // Bodyless request: the client connection stays usable.
    c.send(&get(&format!("http://{dead}/y"))).await;
    assert_eq!(c.response("GET").await.0.status, 502);
    let ex = h.sink.wait_for(2).await.remove(0);
    assert!(ex.status.is_none());
    assert!(ex.error.as_deref().unwrap().contains("cannot connect"));
    assert_eq!(h.proxy.stats().errors, 2);
}

#[tokio::test]
async fn requests_to_the_proxy_itself_get_the_info_page() {
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    c.send(&get(&format!("http://{}/", h.addr))).await;
    let (head, body) = c.response("GET").await;
    assert_eq!(head.status, 200);
    assert!(String::from_utf8_lossy(&body).contains("is running"));
    // Origin-form, as when the proxy URL is opened without proxy settings.
    c.send(format!("GET / HTTP/1.1\r\nHost: {}\r\n\r\n", h.addr).as_bytes())
        .await;
    assert_eq!(c.response("GET").await.0.status, 200);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(h.sink.len(), 0, "self requests are not captured");
}

#[tokio::test]
async fn connect_tunnels_bytes_opaquely() {
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    let authority = lab.addr().to_string();
    c.send(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await;
    let raw = read_head(&mut c.reader, 4096).await.unwrap().unwrap();
    assert!(raw.starts_with(b"HTTP/1.1 200"));
    // Speak plain HTTP inside the tunnel to prove bytes flow both ways.
    c.send(b"GET /json HTTP/1.1\r\nHost: lab\r\n\r\n").await;
    let (head, _) = c.response("GET").await;
    assert_eq!(head.status, 200);
    assert_eq!(h.proxy.stats().tunnels, 1);
    assert_eq!(h.sink.len(), 0);
}

#[tokio::test]
async fn malformed_client_requests_get_400() {
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    c.send(b"THIS IS NOT HTTP\r\n\r\n").await;
    let (head, _) = c.response("GET").await;
    assert_eq!(head.status, 400);
    assert!(c.is_closed().await);
}

#[tokio::test]
async fn stopping_a_listener_closes_it() {
    let h = harness().await;
    assert!(h.proxy.stop("default").await);
    assert!(TcpStream::connect(h.addr).await.is_err());
}

// ---------------------------------------------------------------------------
// Scripted upstreams for edge cases.

type Seen = Arc<Mutex<Vec<Vec<u8>>>>;

/// Serves each connection by reading request heads and answering with the
/// next scripted response. `None` closes the connection without answering;
/// HTTP/1.0 responses and `Connection: close` responses close after writing.
async fn scripted(responses: Vec<Option<&'static [u8]>>) -> (SocketAddr, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Seen = Arc::default();
    let seen2 = seen.clone();
    tokio::spawn(async move {
        let script = Arc::new(Mutex::new(responses.into_iter()));
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let script = script.clone();
            let seen = seen2.clone();
            tokio::spawn(async move {
                let (r, mut w) = stream.into_split();
                let mut r = BufReader::new(r);
                while let Ok(Some(head)) = read_head(&mut r, 65536).await {
                    seen.lock().unwrap().push(head);
                    let next = script.lock().unwrap().next().flatten();
                    let Some(resp) = next else { return };
                    if w.write_all(resp).await.is_err() {
                        return;
                    }
                    let closes = resp.starts_with(b"HTTP/1.0")
                        || resp
                            .windows(17)
                            .any(|win| win.eq_ignore_ascii_case(b"connection: close"));
                    if closes {
                        let _ = w.shutdown().await;
                        return;
                    }
                }
            });
        }
    });
    (addr, seen)
}

#[tokio::test]
async fn preserves_header_case_order_and_spacing_upstream() {
    let (addr, seen) = scripted(vec![Some(b"HTTP/1.1 204 No Content\r\n\r\n")]).await;
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    c.send(
        format!("GET http://{addr}/p?q=1 HTTP/1.1\r\nhost: {addr}\r\nX-MiXeD:   spaced  \r\nproxy-connection: x\r\nA: 1\r\n\r\n")
            .as_bytes(),
    )
    .await;
    assert_eq!(c.response("GET").await.0.status, 204);
    let got = seen.lock().unwrap()[0].clone();
    assert_eq!(
        String::from_utf8(got).unwrap(),
        format!("GET /p?q=1 HTTP/1.1\r\nhost: {addr}\r\nX-MiXeD:   spaced  \r\nA: 1\r\n\r\n")
    );
}

#[tokio::test]
async fn close_delimited_response_is_relayed_and_connection_closed() {
    let (addr, _) = scripted(vec![Some(
        b"HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\nuntil close",
    )])
    .await;
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    c.send(&get(&format!("http://{addr}/"))).await;
    // Read head, then body until the proxy closes the connection.
    let raw = read_head(&mut c.reader, 4096).await.unwrap().unwrap();
    assert!(raw.starts_with(b"HTTP/1.0 200"));
    let mut rest = Vec::new();
    let _ = tokio::time::timeout(Duration::from_millis(300), c.reader.read_to_end(&mut rest)).await;
    assert!(rest.starts_with(b"until close"));
}

#[tokio::test]
async fn interim_responses_are_forwarded() {
    let (addr, _) = scripted(vec![Some(
        b"HTTP/1.1 103 Early Hints\r\nLink: </a.css>; rel=preload\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    )])
    .await;
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    c.send(&get(&format!("http://{addr}/"))).await;
    assert_eq!(c.response("GET").await.0.status, 103);
    let (head, body) = c.response("GET").await;
    assert_eq!((head.status, body.as_slice()), (200, &b"ok"[..]));
    assert_eq!(h.sink.wait_for(1).await[0].status, Some(200));
}

#[tokio::test]
async fn head_and_304_have_no_body_and_keep_alive() {
    let (addr, _) = scripted(vec![
        Some(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n"),
        Some(b"HTTP/1.1 304 Not Modified\r\nContent-Length: 1000\r\n\r\n"),
        Some(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nend"),
    ])
    .await;
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    c.send(format!("HEAD http://{addr}/ HTTP/1.1\r\nHost: a\r\n\r\n").as_bytes())
        .await;
    assert_eq!(c.response("HEAD").await.1.len(), 0);
    c.send(&get(&format!("http://{addr}/"))).await;
    assert_eq!(c.response("GET").await.0.status, 304);
    c.send(&get(&format!("http://{addr}/"))).await;
    assert_eq!(c.response("GET").await.1, b"end");
}

#[tokio::test]
async fn upstream_failures_are_reported() {
    let (garbage, _) = scripted(vec![Some(b"NOT HTTP AT ALL\r\n\r\n")]).await;
    let (silent, _) = scripted(vec![None]).await;
    let h = harness().await;
    for addr in [garbage, silent] {
        let mut c = Client::connect(h.addr).await;
        c.send(&get(&format!("http://{addr}/"))).await;
        assert_eq!(c.response("GET").await.0.status, 502);
    }
    let all = h.sink.wait_for(2).await;
    assert!(
        all[0]
            .error
            .as_deref()
            .unwrap()
            .contains("invalid upstream response")
    );
    assert!(
        all[1]
            .error
            .as_deref()
            .unwrap()
            .contains("without a response")
    );
}

#[tokio::test]
async fn reconnects_when_upstream_closed_idle_connection() {
    // First connection answers once and then closes (keep-alive implied, so
    // the proxy keeps it idle); the next request must succeed regardless.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let (r, mut w) = stream.into_split();
                let mut r = BufReader::new(r);
                if read_head(&mut r, 65536).await.ok().flatten().is_some() {
                    let _ = w
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi")
                        .await;
                }
                // Drop: server closes the "keep-alive" connection.
            });
        }
    });
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    for _ in 0..3 {
        c.send(&get(&format!("http://{addr}/"))).await;
        let (head, body) = c.response("GET").await;
        assert_eq!((head.status, body.as_slice()), (200, &b"hi"[..]));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn cl_te_conflict_is_flagged() {
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h = harness().await;
    let mut c = Client::connect(h.addr).await;
    c.send(
        format!(
            "POST {}/echo HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n0\r\n\r\n",
            lab.url()
        )
        .as_bytes(),
    )
    .await;
    let _ = c.response("POST").await;
    let ex = h.sink.wait_for(1).await.remove(0);
    assert_ne!(
        ex.flags & chosproxy_core::exchange::flags::CL_TE_CONFLICT,
        0
    );
    assert!(ex.notes.unwrap().contains("Transfer-Encoding was used"));
}

#[tokio::test]
async fn listener_authentication_is_enforced_and_not_forwarded() {
    use base64::Engine as _;
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let sink = Arc::new(MemorySink::default());
    let proxy = ProxyService::new(
        sink.clone(),
        Arc::new(NoObserver),
        CaptureLimits {
            max_message_bytes: 1 << 20,
        },
    );
    let addr = proxy
        .start(&ListenerConfig {
            port: 0,
            auth: Some(chosproxy_core::config::ProxyAuth::new("lab", "pw").unwrap()),
            ..ListenerConfig::default()
        })
        .await
        .unwrap();
    let url = format!("{}/headers", lab.url());
    let basic = |c: &str| base64::engine::general_purpose::STANDARD.encode(c);

    for auth in [
        String::new(),
        format!("Proxy-Authorization: Basic {}\r\n", basic("lab:no")),
    ] {
        let mut c = Client::connect(addr).await;
        c.send(format!("GET {url} HTTP/1.1\r\nHost: lab\r\n{auth}\r\n").as_bytes())
            .await;
        let (head, _) = c.response("GET").await;
        assert_eq!(head.status, 407);
        assert!(
            head.header_str("proxy-authenticate")
                .unwrap()
                .starts_with("Basic realm=")
        );
    }
    let ok = format!("Proxy-Authorization: Basic {}\r\n", basic("lab:pw"));
    let mut c = Client::connect(addr).await;
    for _ in 0..2 {
        c.send(format!("GET {url} HTTP/1.1\r\nHost: lab\r\n{ok}\r\n").as_bytes())
            .await;
        let (head, body) = c.response("GET").await;
        assert_eq!(head.status, 200);
        let body = String::from_utf8_lossy(&body).to_ascii_lowercase();
        assert!(
            !body.contains("proxy-authorization"),
            "credentials forwarded: {body}"
        );
    }
    let captured = sink.wait_for(2).await;
    assert_eq!(captured.len(), 2, "rejected requests are not captured");
    assert_eq!(
        proxy.stats().auth_failures,
        1,
        "only wrong credentials count"
    );
}

// ---------------------------------------------------------------------------
// WebSockets

#[derive(Default)]
struct WsSink {
    exchanges: Mutex<Vec<CapturedExchange>>,
    events: Mutex<Vec<chosproxy_core::websocket::WsEvent>>,
}

impl CaptureSink for WsSink {
    fn record(&self, exchange: CapturedExchange) {
        self.exchanges.lock().unwrap().push(exchange);
    }
    fn ws_event(&self, event: chosproxy_core::websocket::WsEvent) {
        self.events.lock().unwrap().push(event);
    }
}

async fn ws_harness() -> (ProxyService, SocketAddr, Arc<WsSink>, LabServer) {
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let sink = Arc::new(WsSink::default());
    let proxy = ProxyService::new(
        sink.clone(),
        Arc::new(NoObserver),
        CaptureLimits {
            max_message_bytes: 1 << 20,
        },
    );
    let addr = proxy
        .start(&ListenerConfig {
            port: 0,
            ..ListenerConfig::default()
        })
        .await
        .unwrap();
    (proxy, addr, sink, lab)
}

/// Opens a WebSocket through the proxy; returns the client and the 101 head.
async fn ws_connect(addr: SocketAddr, lab: &LabServer, extensions: bool) -> (Client, ResponseHead) {
    let mut c = Client::connect(addr).await;
    let ext = if extensions {
        "Sec-WebSocket-Extensions: permessage-deflate; client_max_window_bits\r\n"
    } else {
        ""
    };
    c.send(
        format!(
            "GET {}/ws HTTP/1.1\r\nHost: lab\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{ext}\r\n",
            lab.url()
        )
        .as_bytes(),
    )
    .await;
    let raw = read_head(&mut c.reader, 65536).await.unwrap().unwrap();
    (c, ResponseHead::parse(raw).unwrap())
}

async fn ws_read(c: &mut Client) -> chosproxy_websocket::Frame {
    tokio::time::timeout(
        Duration::from_secs(10),
        chosproxy_websocket::read_frame(&mut c.reader),
    )
    .await
    .expect("frame in time")
    .unwrap()
    .expect("a frame")
}

fn ws_text(text: &str) -> Vec<u8> {
    chosproxy_websocket::encode_frame(
        chosproxy_websocket::Opcode::Text,
        text.as_bytes(),
        Some(chosproxy_websocket::random_mask()),
    )
}

#[tokio::test]
async fn websocket_messages_are_relayed_and_captured() {
    use chosproxy_core::websocket::{WsDirection, WsEvent};
    use chosproxy_websocket::{Opcode, encode_frame, random_mask};
    let (_proxy, addr, sink, lab) = ws_harness().await;
    let (mut c, head) = ws_connect(addr, &lab, true).await;
    assert_eq!(head.status, 101);
    assert_eq!(
        head.header_str("x-lab-extensions-offered").as_deref(),
        Some("no"),
        "extensions are stripped from the handshake"
    );
    assert_eq!(ws_read(&mut c).await.payload, b"welcome");

    c.send(&ws_text("hello")).await;
    assert_eq!(ws_read(&mut c).await.payload, b"echo: hello");
    // A fragmented message with a ping in between.
    let mut fragmented = vec![0x01, 0x83];
    let mask = random_mask();
    fragmented.extend_from_slice(&mask);
    fragmented.extend(b"Hel".iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    fragmented.extend(encode_frame(Opcode::Ping, b"p", Some(random_mask())));
    let mut last = encode_frame(Opcode::Continuation, b"lo", Some(random_mask()));
    last[0] |= 0x80;
    fragmented.extend(last);
    c.send(&fragmented).await;
    let pong = ws_read(&mut c).await;
    assert_eq!(
        (pong.opcode, pong.payload.as_slice()),
        (Opcode::Pong, &b"p"[..])
    );
    assert_eq!(ws_read(&mut c).await.payload, b"echo: Hello");
    c.send(&encode_frame(
        Opcode::Close,
        &[0x03, 0xe8, b'b', b'y', b'e'],
        Some(random_mask()),
    ))
    .await;
    assert_eq!(ws_read(&mut c).await.opcode, Opcode::Close);

    let mut events = Vec::new();
    for _ in 0..300 {
        events = sink.events.lock().unwrap().clone();
        if events.iter().any(|e| matches!(e, WsEvent::Closed { .. })) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let key = match &events[0] {
        WsEvent::Opened { key, url, .. } => {
            assert!(url.ends_with("/ws"));
            *key
        }
        other => panic!("expected Opened, got {other:?}"),
    };
    let handshake = sink.exchanges.lock().unwrap()[0].clone();
    assert_eq!(handshake.ws_key, Some(key));
    assert!(!String::from_utf8_lossy(&handshake.request).contains("Sec-WebSocket-Extensions"));
    let messages: Vec<(WsDirection, u8, String)> = events
        .iter()
        .filter_map(|e| match e {
            WsEvent::Message(m) => Some((
                m.direction,
                m.opcode,
                String::from_utf8_lossy(&m.payload).into_owned(),
            )),
            _ => None,
        })
        .collect();
    let to_server: Vec<_> = messages
        .iter()
        .filter(|m| m.0 == WsDirection::ToServer)
        .collect();
    assert_eq!(to_server[0].2, "hello");
    assert!(
        to_server.iter().any(|m| m.1 == 1 && m.2 == "Hello"),
        "fragments joined"
    );
    assert!(to_server.iter().any(|m| m.1 == 9), "ping captured");
    assert!(
        messages
            .iter()
            .any(|m| m.0 == WsDirection::ToClient && m.2 == "welcome")
    );
    match events.last().unwrap() {
        WsEvent::Closed { code, reason, .. } => {
            assert_eq!((*code, reason.as_str()), (Some(1000), "bye"));
        }
        other => panic!("expected Closed, got {other:?}"),
    }
}

#[tokio::test]
async fn websocket_messages_can_be_intercepted_edited_and_dropped() {
    use chosproxy_core::config::InterceptRules;
    use chosproxy_core::websocket::{WsEvent, ws_flags};
    use chosproxy_proxy::{Decision, HeldKind};
    let (proxy, addr, sink, lab) = ws_harness().await;
    let queue = proxy.intercept();
    queue.set_rules(InterceptRules {
        requests: false,
        websocket_to_server: true,
        ..InterceptRules::default()
    });
    queue.set_enabled(true);
    let (mut c, head) = ws_connect(addr, &lab, false).await;
    assert_eq!(head.status, 101);
    assert_eq!(
        ws_read(&mut c).await.payload,
        b"welcome",
        "server messages pass"
    );

    let wait_held = || async {
        for _ in 0..300 {
            if let Some(s) = queue.pending().first() {
                return s.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("nothing paused");
    };
    c.send(&ws_text("original")).await;
    let held = wait_held().await;
    assert_eq!(held.kind, HeldKind::WsToServer);
    assert_eq!(held.ws_opcode.as_deref(), Some("text"));
    assert_eq!(queue.get(held.id).unwrap().raw, b"original");
    queue.decide(
        held.id,
        Decision::Forward {
            raw: b"edited by tester".to_vec(),
            intercept_response: false,
        },
    );
    assert_eq!(ws_read(&mut c).await.payload, b"echo: edited by tester");

    c.send(&ws_text("drop me")).await;
    let held = wait_held().await;
    queue.decide(held.id, Decision::Drop);
    c.send(&ws_text("after")).await;
    let held = wait_held().await;
    queue.decide(
        held.id,
        Decision::Forward {
            raw: b"after".to_vec(),
            intercept_response: false,
        },
    );
    assert_eq!(
        ws_read(&mut c).await.payload,
        b"echo: after",
        "the dropped message never reached the server"
    );

    let events = sink.events.lock().unwrap().clone();
    let flags: Vec<(String, i64, Option<Vec<u8>>)> = events
        .iter()
        .filter_map(|e| match e {
            WsEvent::Message(m)
                if m.direction == chosproxy_core::websocket::WsDirection::ToServer =>
            {
                Some((
                    String::from_utf8_lossy(&m.payload).into_owned(),
                    m.flags,
                    m.original.clone(),
                ))
            }
            _ => None,
        })
        .collect();
    assert_eq!(flags[0].0, "edited by tester");
    assert!(flags[0].1 & ws_flags::EDITED != 0);
    assert_eq!(flags[0].2.as_deref(), Some(&b"original"[..]));
    assert!(flags[1].1 & ws_flags::DROPPED != 0);
    assert_eq!(flags[2].1, 0, "forwarded unchanged");
}

#[tokio::test]
async fn websocket_messages_follow_match_and_replace() {
    use chosproxy_core::config::{MatchReplaceRule, MatchTarget};
    let (proxy, addr, _sink, lab) = ws_harness().await;
    let rule = |target, pattern: &str, replace: &str| MatchReplaceRule {
        enabled: true,
        target,
        pattern: pattern.into(),
        replace: replace.into(),
        regex: false,
        comment: String::new(),
    };
    proxy
        .set_match_replace(&[
            rule(MatchTarget::WsToServer, "user", "admin"),
            rule(MatchTarget::WsToClient, "echo: ", "ECHO> "),
        ])
        .unwrap();
    let (mut c, head) = ws_connect(addr, &lab, false).await;
    assert_eq!(head.status, 101);
    assert_eq!(ws_read(&mut c).await.payload, b"welcome");
    c.send(&ws_text("I am user")).await;
    // Changed on the way to the server and again on the way back.
    assert_eq!(ws_read(&mut c).await.payload, b"ECHO> I am admin");
}
