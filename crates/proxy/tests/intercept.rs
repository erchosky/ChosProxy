//! Intercept end to end: requests and responses paused, edited, forwarded,
//! dropped and released, with real TCP clients and the lab server.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_core::config::{InterceptRules, ListenerConfig};
use chosproxy_core::event::ListenerState;
use chosproxy_core::exchange::{CapturedExchange, flags};
use chosproxy_http::content::split_message;
use chosproxy_http::edit::update_content_length;
use chosproxy_http::framing::response_framing;
use chosproxy_http::head::read_head;
use chosproxy_http::{BodyReader, ResponseHead};
use chosproxy_lab::LabServer;
use chosproxy_proxy::{
    CaptureLimits, CaptureSink, Decision, HeldKind, InterceptQueue, ProxyObserver, ProxyService,
};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[derive(Default)]
struct Recorder(Mutex<Vec<CapturedExchange>>);

impl CaptureSink for Recorder {
    fn record(&self, exchange: CapturedExchange) {
        self.0.lock().unwrap().push(exchange);
    }
}

impl ProxyObserver for Recorder {
    fn listener_changed(&self, _: &str, _: &ListenerState) {}
}

impl Recorder {
    async fn wait_for(&self, n: usize) -> Vec<CapturedExchange> {
        for _ in 0..300 {
            {
                let v = self.0.lock().unwrap();
                if v.len() >= n {
                    return v.clone();
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("expected {n} exchanges");
    }
}

struct Harness {
    _proxy: ProxyService,
    addr: SocketAddr,
    rec: Arc<Recorder>,
    queue: Arc<InterceptQueue>,
    lab: LabServer,
}

async fn harness(rules: InterceptRules) -> Harness {
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let rec = Arc::new(Recorder::default());
    let proxy = ProxyService::new(
        rec.clone(),
        rec.clone(),
        CaptureLimits {
            max_message_bytes: 1 << 20,
        },
    );
    let queue = proxy.intercept();
    queue.set_rules(rules);
    queue.set_enabled(true);
    let addr = proxy
        .start(&ListenerConfig {
            port: 0,
            ..ListenerConfig::default()
        })
        .await
        .unwrap();
    Harness {
        _proxy: proxy,
        addr,
        rec,
        queue,
        lab,
    }
}

async fn wait_pending(q: &InterceptQueue, n: usize) -> Vec<chosproxy_proxy::HeldSummary> {
    for _ in 0..300 {
        let p = q.pending();
        if p.len() >= n {
            return p;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("expected {n} paused messages");
}

async fn send(proxy: SocketAddr, raw: String) -> BufReader<TcpStream> {
    let mut s = BufReader::new(TcpStream::connect(proxy).await.unwrap());
    s.get_mut().write_all(raw.as_bytes()).await.unwrap();
    s
}

async fn response(s: &mut BufReader<TcpStream>, method: &str) -> (ResponseHead, Vec<u8>) {
    let raw = tokio::time::timeout(Duration::from_secs(10), read_head(s, 65536))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let head = ResponseHead::parse(raw).unwrap();
    let mut body = BodyReader::new(response_framing(method, &head).unwrap().kind);
    let mut out = Vec::new();
    while let Some(p) = body.next_chunk(s).await.unwrap() {
        out.extend(p);
    }
    (head, out)
}

fn replace(raw: &[u8], from: &str, to: &str) -> Vec<u8> {
    String::from_utf8(raw.to_vec())
        .unwrap()
        .replace(from, to)
        .into_bytes()
}

#[tokio::test]
async fn paused_request_is_edited_and_forwarded() {
    let h = harness(InterceptRules::default()).await;
    let mut c = send(
        h.addr,
        format!(
            "POST {}/echo HTTP/1.1\r\nHost: lab\r\nContent-Length: 3\r\n\r\na=1",
            h.lab.url()
        ),
    )
    .await;
    let held = wait_pending(&h.queue, 1).await;
    assert_eq!(held[0].kind, HeldKind::Request);
    assert_eq!(held[0].method, "POST");
    let msg = h.queue.get(held[0].id).unwrap();
    assert!(
        msg.raw.starts_with(b"POST /echo HTTP/1.1\r\n"),
        "origin-form"
    );
    let edited = update_content_length(&replace(&msg.raw, "a=1", "a=12345"), false);
    assert!(h.queue.decide(
        held[0].id,
        Decision::Forward {
            raw: edited,
            intercept_response: false
        }
    ));
    let (head, body) = response(&mut c, "POST").await;
    assert_eq!(head.status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"body\":\"a=12345\""));

    let ex = h.rec.wait_for(1).await.remove(0);
    assert_ne!(ex.flags & flags::EDITED_REQUEST, 0);
    assert!(ex.request.ends_with(b"a=12345"));
    assert!(ex.original_request.unwrap().ends_with(b"a=1"));
    assert_eq!(ex.status, Some(200));
}

#[tokio::test]
async fn dropped_request_never_reaches_the_server() {
    let h = harness(InterceptRules::default()).await;
    let mut c = send(
        h.addr,
        format!("GET {}/json HTTP/1.1\r\nHost: lab\r\n\r\n", h.lab.url()),
    )
    .await;
    let held = wait_pending(&h.queue, 1).await;
    h.queue.decide(held[0].id, Decision::Drop);
    let (head, body) = response(&mut c, "GET").await;
    assert_eq!(head.status, 502);
    assert!(String::from_utf8_lossy(&body).contains("dropped by the user"));
    let ex = h.rec.wait_for(1).await.remove(0);
    assert_ne!(ex.flags & flags::DROPPED, 0);
    assert!(ex.server_ip.is_none(), "no upstream connection was made");
    assert!(ex.status.is_none());
}

#[tokio::test]
async fn responses_can_be_paused_edited_and_kept_alive() {
    let h = harness(InterceptRules {
        requests: false,
        responses: true,
        ..InterceptRules::default()
    })
    .await;
    let mut c = send(
        h.addr,
        format!("GET {}/json HTTP/1.1\r\nHost: lab\r\n\r\n", h.lab.url()),
    )
    .await;
    let held = wait_pending(&h.queue, 1).await;
    assert_eq!(held[0].kind, HeldKind::Response);
    assert_eq!(held[0].status, Some(200));
    let msg = h.queue.get(held[0].id).unwrap();
    assert!(msg.request.unwrap().starts_with(b"GET /json"));
    let edited = update_content_length(&replace(&msg.raw, "\"lab\"", "\"edited\""), false);
    h.queue.decide(
        held[0].id,
        Decision::Forward {
            raw: edited,
            intercept_response: false,
        },
    );
    let (_, body) = response(&mut c, "GET").await;
    assert!(String::from_utf8_lossy(&body).contains("\"name\":\"edited\""));

    // The same client connection keeps working.
    c.get_mut()
        .write_all(b"GET /status/404 HTTP/1.1\r\nHost: lab\r\n\r\n")
        .await
        .ok();
    let ex = h.rec.wait_for(1).await.remove(0);
    assert_ne!(ex.flags & flags::EDITED_RESPONSE, 0);
    let (_, original_body) = split_message(ex.original_response.as_deref().unwrap());
    assert!(String::from_utf8_lossy(original_body).contains("\"lab\""));
}

#[tokio::test]
async fn forward_and_intercept_response_pauses_the_response_too() {
    let h = harness(InterceptRules::default()).await;
    let mut c = send(
        h.addr,
        format!("GET {}/json HTTP/1.1\r\nHost: lab\r\n\r\n", h.lab.url()),
    )
    .await;
    let held = wait_pending(&h.queue, 1).await;
    let raw = h.queue.get(held[0].id).unwrap().raw;
    h.queue.decide(
        held[0].id,
        Decision::Forward {
            raw,
            intercept_response: true,
        },
    );
    let held = wait_pending(&h.queue, 1).await;
    assert_eq!(held[0].kind, HeldKind::Response);
    h.queue.decide(held[0].id, Decision::Drop);
    let (head, body) = response(&mut c, "GET").await;
    assert_eq!(head.status, 502);
    assert!(String::from_utf8_lossy(&body).contains("response was dropped"));
    let ex = h.rec.wait_for(1).await.remove(0);
    assert_eq!(ex.status, Some(200), "the real response is recorded");
    assert_ne!(ex.flags & flags::DROPPED, 0);
}

#[tokio::test]
async fn turning_intercept_off_releases_paused_messages() {
    let h = harness(InterceptRules::default()).await;
    let mut c = send(
        h.addr,
        format!("GET {}/json HTTP/1.1\r\nHost: lab\r\n\r\n", h.lab.url()),
    )
    .await;
    wait_pending(&h.queue, 1).await;
    h.queue.set_enabled(false);
    let (head, _) = response(&mut c, "GET").await;
    assert_eq!(head.status, 200);
    let ex = h.rec.wait_for(1).await.remove(0);
    assert_eq!(ex.flags & flags::EDITED_REQUEST, 0);
}

#[tokio::test]
async fn rules_let_static_assets_through() {
    let h = harness(InterceptRules::default()).await;
    let mut c = send(
        h.addr,
        format!("GET {}/logo.png HTTP/1.1\r\nHost: lab\r\n\r\n", h.lab.url()),
    )
    .await;
    let (head, _) = response(&mut c, "GET").await;
    assert_eq!(head.status, 404, "reached the lab without pausing");
    assert!(h.queue.pending().is_empty());
}

#[tokio::test]
async fn client_disconnect_removes_the_paused_request() {
    let h = harness(InterceptRules::default()).await;
    let c = send(
        h.addr,
        format!("GET {}/json HTTP/1.1\r\nHost: lab\r\n\r\n", h.lab.url()),
    )
    .await;
    wait_pending(&h.queue, 1).await;
    drop(c);
    for _ in 0..300 {
        if h.queue.pending().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(h.queue.pending().is_empty());
    let ex = h.rec.wait_for(1).await.remove(0);
    assert!(ex.error.unwrap().contains("client disconnected"));
}
