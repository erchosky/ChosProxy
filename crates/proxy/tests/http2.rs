//! HTTP/2 between a client and the proxy (M21c): a real HTTP/2 client goes
//! through CONNECT, negotiates `h2` with the interception certificate, and
//! the proxy relays to HTTP/2 and HTTP/1.1-only servers.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use bytes::Bytes;
use chosproxy_core::config::{ListenerConfig, MatchReplaceRule, MatchTarget};
use chosproxy_core::event::ListenerState;
use chosproxy_core::exchange::CapturedExchange;
use chosproxy_http::head::read_head;
use chosproxy_lab::LabServer;
use chosproxy_proxy::{
    CaptureLimits, CaptureSink, HookFuture, HookOutcome, HookTarget, HookVerdict, ProxyObserver,
    ProxyService, RequestHook, TlsContext,
};
use chosproxy_tls::{CaStore, Interceptor, UpstreamTls};
use rustls::pki_types::{CertificateDer, ServerName};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[derive(Default)]
struct Recorder {
    exchanges: Mutex<Vec<CapturedExchange>>,
}

impl CaptureSink for Recorder {
    fn record(&self, exchange: CapturedExchange) {
        self.exchanges.lock().unwrap().push(exchange);
    }
}

impl ProxyObserver for Recorder {
    fn listener_changed(&self, _: &str, _: &ListenerState) {}
}

impl Recorder {
    async fn wait_for(&self, n: usize) -> Vec<CapturedExchange> {
        for _ in 0..500 {
            {
                let v = self.exchanges.lock().unwrap();
                if v.len() >= n {
                    return v.clone();
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("expected {n} exchanges");
    }
}

struct Proxy {
    service: ProxyService,
    addr: SocketAddr,
    rec: Arc<Recorder>,
    ca_der: CertificateDer<'static>,
    _dir: tempfile::TempDir,
}

async fn proxy(http2: bool) -> Proxy {
    let dir = tempfile::tempdir().unwrap();
    let (ca, _) = CaStore::new(dir.path()).load_or_create().unwrap();
    let ca_der = ca.cert_der().clone();
    let rec = Arc::new(Recorder::default());
    let service = ProxyService::new(
        rec.clone(),
        rec.clone(),
        CaptureLimits {
            max_message_bytes: 1 << 20,
        },
    );
    service.set_tls(Some(TlsContext {
        interceptor: Arc::new(Interceptor::new(Arc::new(ca)).unwrap()),
        upstream: UpstreamTls::new(&[]).unwrap(),
    }));
    let addr = service
        .start(&ListenerConfig {
            port: 0,
            http2,
            ..ListenerConfig::default()
        })
        .await
        .unwrap();
    Proxy {
        service,
        addr,
        rec,
        ca_der,
        _dir: dir,
    }
}

/// CONNECT through the proxy, TLS offering h2 and http/1.1; returns the
/// stream and the protocol the proxy picked.
async fn tunnel(
    p: &Proxy,
    lab: &LabServer,
) -> (tokio_rustls::client::TlsStream<TcpStream>, Option<Vec<u8>>) {
    tunnel_trusting(p, lab, p.ca_der.clone()).await.unwrap()
}

/// The same, trusting only `ca`; an error when the handshake fails.
async fn tunnel_trusting(
    p: &Proxy,
    lab: &LabServer,
    ca: CertificateDer<'static>,
) -> std::io::Result<(tokio_rustls::client::TlsStream<TcpStream>, Option<Vec<u8>>)> {
    let authority = lab.addr().to_string();
    let mut s = BufReader::new(TcpStream::connect(p.addr).await.unwrap());
    s.get_mut()
        .write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let ok = read_head(&mut s, 4096).await.unwrap().unwrap();
    assert!(ok.starts_with(b"HTTP/1.1 200"));
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca).unwrap();
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let name = ServerName::try_from("127.0.0.1".to_string()).unwrap();
    let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, s.into_inner())
        .await?;
    let alpn = tls.get_ref().1.alpn_protocol().map(<[u8]>::to_vec);
    Ok((tls, alpn))
}

async fn h2_get(
    tls: tokio_rustls::client::TlsStream<TcpStream>,
    paths: &[&str],
) -> Vec<(u16, String)> {
    let (sender, connection) = h2::client::handshake(tls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let mut out = Vec::new();
    for path in paths {
        let mut sender = sender.clone().ready().await.unwrap();
        let request = http::Request::builder()
            .method("GET")
            .uri(format!("https://127.0.0.1{path}"))
            .header("cookie", "a=1")
            .header("cookie", "b=2")
            .body(())
            .unwrap();
        let (future, _) = sender.send_request(request, true).unwrap();
        let response = future.await.unwrap();
        let status = response.status().as_u16();
        let mut body = response.into_body();
        let mut text = Vec::new();
        while let Some(chunk) = body.data().await {
            let chunk: Bytes = chunk.unwrap();
            let _ = body.flow_control().release_capacity(chunk.len());
            text.extend_from_slice(&chunk);
        }
        out.push((status, String::from_utf8_lossy(&text).to_string()));
    }
    out
}

#[tokio::test]
async fn http2_clients_reach_http2_and_http1_servers() {
    let h2_lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h1_lab = LabServer::start_tls_http1("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let p = proxy(true).await;
    p.service
        .set_match_replace(&[MatchReplaceRule {
            enabled: true,
            target: MatchTarget::RequestHeader,
            pattern: String::new(),
            replace: "x-chosproxy: h2".into(),
            regex: false,
            comment: "mark".into(),
        }])
        .unwrap();

    // Server with HTTP/2: two streams on one client connection.
    let (tls, alpn) = tunnel(&p, &h2_lab).await;
    assert_eq!(alpn.as_deref(), Some(&b"h2"[..]));
    let answers = h2_get(tls, &["/version", "/headers"]).await;
    assert_eq!(answers[0].0, 200, "{}", answers[0].1);
    assert!(
        answers[0].1.contains(r#""version":"HTTP/2""#),
        "{}",
        answers[0].1
    );
    let headers = answers[1].1.to_ascii_lowercase();
    assert!(headers.contains(r#""x-chosproxy":"h2""#), "{headers}");
    assert!(headers.contains("a=1; b=2"), "{headers}");

    // Server with HTTP/1.1 only: the request is sent to it as HTTP/1.1.
    let (tls, _) = tunnel(&p, &h1_lab).await;
    let answers = h2_get(tls, &["/version"]).await;
    assert_eq!(answers[0].0, 200, "{}", answers[0].1);
    assert!(
        answers[0].1.contains(r#""version":"HTTP/1.1""#),
        "{}",
        answers[0].1
    );

    let all = p.rec.wait_for(3).await;
    let first = all
        .iter()
        .find(|e| e.path == "/version" && e.port == h2_lab.addr().port())
        .unwrap();
    assert_eq!(first.http_version, "HTTP/2");
    assert!(String::from_utf8_lossy(&first.request).starts_with("GET /version HTTP/2\r\n"));
    assert!(String::from_utf8_lossy(first.response.as_deref().unwrap()).starts_with("HTTP/2 200"));
    let notes = first.notes.clone().unwrap_or_default();
    assert!(notes.contains("Match & Replace: mark"), "{notes}");
    let fallback = all.iter().find(|e| e.port == h1_lab.addr().port()).unwrap();
    assert!(
        fallback
            .notes
            .clone()
            .unwrap_or_default()
            .contains("as HTTP/1.1")
    );
    p.service.stop_all().await;
}

#[tokio::test]
async fn listeners_without_http2_keep_http1() {
    let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let p = proxy(false).await;
    let (_tls, alpn) = tunnel(&p, &lab).await;
    assert_eq!(alpn.as_deref(), Some(&b"http/1.1"[..]));
    p.service.stop_all().await;
}

/// A hook that decides by path, as a plugin would.
struct PathHook;

impl RequestHook for PathHook {
    fn wants(&self, _target: &HookTarget<'_>) -> bool {
        true
    }

    fn on_request(&self, target: &HookTarget<'_>, request: Vec<u8>) -> HookFuture {
        let path = target.path.to_string();
        Box::pin(async move {
            let text = String::from_utf8_lossy(&request).into_owned();
            let verdict = match path.as_str() {
                "/blocked" => HookVerdict::Block("not on my watch".into()),
                "/answered" => HookVerdict::Respond(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 4\r\n\r\nhook"
                        .to_vec(),
                ),
                "/json" => {
                    HookVerdict::Replace(text.replacen("GET /json", "GET /version", 1).into_bytes())
                }
                _ => HookVerdict::Continue,
            };
            HookOutcome {
                verdict,
                notes: vec![format!("hook saw the request {path}")],
            }
        })
    }

    fn wants_response(&self, target: &HookTarget<'_>) -> bool {
        target.path == "/chunked" || target.path.starts_with("/status/")
    }

    fn on_response(
        &self,
        target: &HookTarget<'_>,
        request: Vec<u8>,
        response: Vec<u8>,
    ) -> HookFuture {
        let path = target.path.to_string();
        Box::pin(async move {
            assert!(String::from_utf8_lossy(&request).contains(" HTTP/2\r\n"));
            let verdict = if path == "/chunked" {
                assert!(String::from_utf8_lossy(&response).contains("first,second,third"));
                HookVerdict::Replace(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nrewritten".to_vec(),
                )
            } else {
                HookVerdict::Block("no tea".into())
            };
            HookOutcome {
                verdict,
                notes: vec![format!("hook saw the response of {path}")],
            }
        })
    }
}

#[tokio::test]
async fn hooks_see_http2_requests_and_responses() {
    let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let p = proxy(true).await;
    p.service.set_request_hook(Some(Arc::new(PathHook)));
    let (tls, alpn) = tunnel(&p, &lab).await;
    assert_eq!(alpn.as_deref(), Some(&b"h2"[..]));
    let answers = h2_get(
        tls,
        &[
            "/blocked",
            "/answered",
            "/json",
            "/chunked",
            "/status/418",
            "/headers",
        ],
    )
    .await;
    assert_eq!(answers[0].0, 403);
    assert!(answers[0].1.contains("not on my watch"), "{}", answers[0].1);
    assert_eq!(answers[1], (200, "hook".to_string()));
    // Replaced: the server was asked for /version instead.
    assert!(
        answers[2].1.contains(r#""version":"HTTP/2""#),
        "{}",
        answers[2].1
    );
    assert_eq!(answers[3], (200, "rewritten".to_string()));
    assert_eq!(answers[4].0, 502);
    assert!(answers[4].1.contains("no tea"), "{}", answers[4].1);
    assert_eq!(answers[5].0, 200);

    let all = p.rec.wait_for(6).await;
    let by = |path: &str| all.iter().find(|e| e.path == path).unwrap();
    let notes = |path: &str| by(path).notes.clone().unwrap_or_default();
    use chosproxy_core::exchange::flags;
    assert!(by("/blocked").flags & flags::DROPPED != 0);
    assert!(by("/blocked").response.is_none());
    assert!(notes("/answered").contains("hook saw the request /answered"));
    // The replaced request is stored as sent, with the original kept.
    let replaced = by("/version");
    assert!(replaced.flags & flags::EDITED_REQUEST != 0);
    assert!(
        String::from_utf8_lossy(replaced.original_request.as_deref().unwrap())
            .starts_with("GET /json HTTP/2")
    );
    let chunked = by("/chunked");
    assert!(chunked.flags & flags::EDITED_RESPONSE != 0);
    assert!(
        String::from_utf8_lossy(chunked.original_response.as_deref().unwrap())
            .contains("first,second,third")
    );
    assert!(notes("/chunked").contains("hook saw the response of /chunked"));
    assert!(by("/status/418").flags & flags::DROPPED != 0);
    assert!(!notes("/headers").contains("response"));
    p.service.stop_all().await;
}

#[tokio::test]
async fn a_listener_presents_its_own_ca_when_given_one() {
    let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let p = proxy(true).await;
    let other = tempfile::tempdir().unwrap();
    let (own, _) = CaStore::new(other.path()).load_or_create().unwrap();
    let own_der = own.cert_der().clone();
    assert_ne!(own_der, p.ca_der);

    // The shared CA at first.
    assert!(tunnel_trusting(&p, &lab, p.ca_der.clone()).await.is_ok());
    assert!(tunnel_trusting(&p, &lab, own_der.clone()).await.is_err());

    // Its own CA: clients that trust only the shared one are refused.
    p.service.set_listener_tls(
        "default",
        Some(Arc::new(Interceptor::new(Arc::new(own)).unwrap())),
    );
    let (tls, _) = tunnel_trusting(&p, &lab, own_der.clone()).await.unwrap();
    assert_eq!(h2_get(tls, &["/version"]).await[0].0, 200);
    assert!(tunnel_trusting(&p, &lab, p.ca_der.clone()).await.is_err());
    // The download page of that listener gives its CA.
    let mut s = TcpStream::connect(p.addr).await.unwrap();
    s.write_all(
        b"GET http://chosproxy/cert.der HTTP/1.1\r\nHost: chosproxy\r\nConnection: close\r\n\r\n",
    )
    .await
    .unwrap();
    let mut page = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut s, &mut page)
        .await
        .unwrap();
    assert!(
        page.ends_with(own_der.as_ref()),
        "the page gives another CA"
    );

    // Another listener id is not affected, and it can be taken back.
    p.service.set_listener_tls("default", None);
    assert!(tunnel_trusting(&p, &lab, p.ca_der.clone()).await.is_ok());
    p.service.stop_all().await;
}
