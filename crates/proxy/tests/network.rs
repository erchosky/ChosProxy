//! Network handling end to end (M21): invisible listeners, chaining to an
//! upstream HTTP proxy, host name overrides and the loop guard. The network
//! settings are process-wide, so the tests here take turns.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_core::config::{
    HostOverride, ListenerConfig, MatchReplaceRule, MatchTarget, NetworkConfig, TlsConfig,
    UpstreamKind, UpstreamRule,
};
use chosproxy_core::event::ListenerState;
use chosproxy_core::exchange::CapturedExchange;
use chosproxy_http::framing::response_framing;
use chosproxy_http::head::{Headers, read_head};
use chosproxy_http::{BodyReader, ResponseHead};
use chosproxy_lab::LabServer;
use chosproxy_proxy::{CaptureLimits, CaptureSink, ProxyObserver, ProxyService, TlsContext};
use chosproxy_request::set_network;
use chosproxy_tls::{CaStore, Interceptor, UpstreamTls};
use rustls::pki_types::{CertificateDer, ServerName};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

async fn proxy(listener: ListenerConfig) -> Proxy {
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
    service.set_tls_policy(TlsConfig {
        skip_verification_hosts: vec!["127.0.0.1".into(), "*.test".into()],
        ..TlsConfig::default()
    });
    let addr = service
        .start(&ListenerConfig {
            port: 0,
            ..listener
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

async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut BufReader<S>,
    request: &str,
) -> (ResponseHead, Vec<u8>) {
    s.get_mut().write_all(request.as_bytes()).await.unwrap();
    let raw = tokio::time::timeout(Duration::from_secs(10), read_head(s, 65536))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let head = ResponseHead::parse(raw).unwrap();
    let mut body = BodyReader::new(response_framing("GET", &head).unwrap().kind);
    let mut out = Vec::new();
    while let Some(p) = body.next_chunk(s).await.unwrap() {
        out.extend(p);
    }
    (head, out)
}

fn tls_client(root: &CertificateDer<'static>) -> tokio_rustls::TlsConnector {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(root.clone()).unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    tokio_rustls::TlsConnector::from(Arc::new(config))
}

fn override_host(host: &str) -> HostOverride {
    HostOverride {
        enabled: true,
        host: host.into(),
        address: "127.0.0.1".parse().unwrap(),
    }
}

#[tokio::test]
async fn invisible_listener_serves_plain_http_and_tls() {
    let _turn = TURN.lock().await;
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let tls_lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    set_network(NetworkConfig {
        hosts: vec![override_host("app.lab.test")],
        ..NetworkConfig::default()
    });
    let p = proxy(ListenerConfig {
        invisible: true,
        invisible_tls_port: tls_lab.addr().port(),
        ..ListenerConfig::default()
    })
    .await;

    // Plain HTTP in origin form: sent where the Host header says.
    let mut s = BufReader::new(TcpStream::connect(p.addr).await.unwrap());
    let host = lab.addr().to_string();
    let (head, body) = exchange(
        &mut s,
        &format!("GET /json HTTP/1.1\r\nHost: {host}\r\n\r\n"),
    )
    .await;
    assert_eq!(head.status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"name\":\"lab\""));

    // TLS straight to the listener: the server name picks the destination,
    // resolved through the host override.
    let tcp = TcpStream::connect(p.addr).await.unwrap();
    let name = ServerName::try_from("app.lab.test".to_string()).unwrap();
    let tls = tls_client(&p.ca_der).connect(name, tcp).await.unwrap();
    let mut tls = BufReader::new(tls);
    let (head, _) = exchange(
        &mut tls,
        "GET /status/204 HTTP/1.1\r\nHost: app.lab.test\r\n\r\n",
    )
    .await;
    assert_eq!(head.status, 204);

    let all = p.rec.wait_for(2).await;
    assert_eq!(all[0].url, format!("http://{host}/json"));
    assert_eq!(
        all[1].url,
        format!("https://app.lab.test:{}/status/204", tls_lab.addr().port())
    );
    set_network(NetworkConfig::default());
    p.service.stop_all().await;
}

/// A minimal upstream HTTP proxy: CONNECT tunnels and absolute-form
/// requests, spliced to the target; records each request line.
async fn mini_proxy() -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let lines = Arc::new(Mutex::new(Vec::new()));
    let seen = lines.clone();
    tokio::spawn(async move {
        loop {
            let (s, _) = l.accept().await.unwrap();
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut r = BufReader::new(s);
                let head = read_head(&mut r, 65536).await.unwrap().unwrap();
                let text = String::from_utf8_lossy(&head).to_string();
                let first = text.lines().next().unwrap().to_string();
                seen.lock().unwrap().push(text.clone());
                let target = first.split(' ').nth(1).unwrap().to_string();
                let connect = first.starts_with("CONNECT ");
                let authority = if connect {
                    target
                } else {
                    let rest = target.strip_prefix("http://").unwrap();
                    rest.split('/').next().unwrap().to_string()
                };
                let mut up = TcpStream::connect(authority).await.unwrap();
                if connect {
                    r.get_mut()
                        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                        .await
                        .unwrap();
                } else {
                    up.write_all(&head).await.unwrap();
                }
                let mut client = r.into_inner();
                let _ = tokio::io::copy_bidirectional(&mut client, &mut up).await;
            });
        }
    });
    (addr, lines)
}

#[tokio::test]
async fn chains_through_an_upstream_http_proxy() {
    let _turn = TURN.lock().await;
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let tls_lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let (upstream, seen) = mini_proxy().await;
    set_network(NetworkConfig {
        upstream: vec![UpstreamRule {
            enabled: true,
            host: "*".into(),
            kind: UpstreamKind::Http,
            proxy_host: "127.0.0.1".into(),
            proxy_port: upstream.port(),
            username: "tester".into(),
            password: "secret".into(),
            remote_dns: true,
        }],
        ..NetworkConfig::default()
    });
    let a = proxy(ListenerConfig::default()).await;

    // Plain HTTP: sent to the upstream proxy in absolute form.
    let mut s = BufReader::new(TcpStream::connect(a.addr).await.unwrap());
    let url = format!("{}/json", lab.url());
    let (head, _) = exchange(
        &mut s,
        &format!("GET {url} HTTP/1.1\r\nHost: {}\r\n\r\n", lab.addr()),
    )
    .await;
    assert_eq!(head.status, 200);

    // HTTPS: decrypted here, then tunnelled to the lab with CONNECT.
    let mut c = BufReader::new(TcpStream::connect(a.addr).await.unwrap());
    let authority = tls_lab.addr().to_string();
    c.get_mut()
        .write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let ok = read_head(&mut c, 4096).await.unwrap().unwrap();
    assert!(ok.starts_with(b"HTTP/1.1 200"));
    let name = ServerName::try_from("127.0.0.1".to_string()).unwrap();
    let tls = tls_client(&a.ca_der).connect(name, c).await.unwrap();
    let mut tls = BufReader::new(tls);
    let (head, _) = exchange(&mut tls, "GET /status/418 HTTP/1.1\r\nHost: lab\r\n\r\n").await;
    assert_eq!(head.status, 418);

    let all = a.rec.wait_for(2).await;
    let note = all[0].notes.clone().unwrap_or_default();
    assert!(note.contains("upstream HTTP proxy"), "{note}");
    assert_eq!(all[0].url, url);
    assert!(
        String::from_utf8_lossy(&all[0].request).starts_with("GET /json HTTP/1.1"),
        "history keeps the request as the client sent it"
    );
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "{seen:?}");
    // base64("tester:secret")
    let auth = "Proxy-Authorization: Basic dGVzdGVyOnNlY3JldA==";
    assert!(
        seen[0].starts_with(&format!("GET {url} HTTP/1.1\r\n")),
        "{}",
        seen[0]
    );
    assert!(seen[0].contains(auth));
    assert!(seen[1].starts_with(&format!("CONNECT {authority} HTTP/1.1\r\n")));
    assert!(seen[1].contains(auth));
    set_network(NetworkConfig::default());
    a.service.stop_all().await;
}

#[tokio::test]
async fn refuses_to_loop_back_into_itself() {
    let _turn = TURN.lock().await;
    set_network(NetworkConfig {
        hosts: vec![override_host("loop.test")],
        ..NetworkConfig::default()
    });
    let p = proxy(ListenerConfig::default()).await;
    let mut s = BufReader::new(TcpStream::connect(p.addr).await.unwrap());
    let port = p.addr.port();
    let (head, body) = exchange(
        &mut s,
        &format!("GET http://loop.test:{port}/ HTTP/1.1\r\nHost: loop.test:{port}\r\n\r\n"),
    )
    .await;
    assert_eq!(head.status, 508);
    assert!(String::from_utf8_lossy(&body).contains("Host name overrides"));
    set_network(NetworkConfig::default());
    p.service.stop_all().await;
}

fn mr(target: MatchTarget, pattern: &str, replace: &str, regex: bool) -> MatchReplaceRule {
    MatchReplaceRule {
        enabled: true,
        target,
        pattern: pattern.into(),
        replace: replace.into(),
        regex,
        comment: String::new(),
    }
}

#[tokio::test]
async fn match_and_replace_changes_requests_and_responses() {
    let _turn = TURN.lock().await;
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let p = proxy(ListenerConfig::default()).await;
    p.service
        .set_match_replace(&[
            MatchReplaceRule {
                comment: "add header".into(),
                ..mr(MatchTarget::RequestHeader, "", "X-Added: yes", false)
            },
            mr(MatchTarget::RequestBody, "alice", "bob", false),
            mr(
                MatchTarget::ResponseBody,
                r#""compressed":true"#,
                r#""compressed":"no""#,
                false,
            ),
            mr(
                MatchTarget::ResponseHeader,
                r"(?i)^content-type:.*$",
                "Content-Type: text/plain",
                true,
            ),
        ])
        .unwrap();
    let mut s = BufReader::new(TcpStream::connect(p.addr).await.unwrap());
    let host = lab.addr();
    let (head, body) = exchange(
        &mut s,
        &format!(
            "POST http://{host}/echo HTTP/1.1\r\nHost: {host}\r\nContent-Length: 10\r\n\r\nuser=alice"
        ),
    )
    .await;
    assert_eq!(head.status, 200);
    assert_eq!(
        head.header_str("content-type").as_deref(),
        Some("text/plain")
    );
    let echoed = String::from_utf8_lossy(&body).to_string();
    assert!(echoed.contains("\"body\":\"user=bob\""), "{echoed}");
    assert!(
        echoed.to_ascii_lowercase().contains("\"x-added\":\"yes\""),
        "{echoed}"
    );

    // A compressed response is decoded, changed and sent with a new length.
    let (head, body) = exchange(
        &mut s,
        &format!("GET http://{host}/gzip HTTP/1.1\r\nHost: {host}\r\n\r\n"),
    )
    .await;
    assert_eq!(head.status, 200);
    assert!(head.header("content-encoding").is_none());
    assert!(String::from_utf8_lossy(&body).contains(r#""compressed":"no""#));

    let all = p.rec.wait_for(2).await;
    let notes = all[0].notes.clone().unwrap_or_default();
    assert!(
        notes.contains("Request changed by Match & Replace: add header, rule 2"),
        "{notes}"
    );
    assert!(
        String::from_utf8_lossy(all[0].original_request.as_deref().unwrap())
            .ends_with("user=alice")
    );
    assert!(String::from_utf8_lossy(&all[0].request).ends_with("user=bob"));
    assert!(all[1].original_response.is_some());
    p.service.stop_all().await;
}

#[tokio::test]
async fn invisible_listener_can_redirect_everything() {
    let _turn = TURN.lock().await;
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let p = proxy(ListenerConfig {
        invisible: true,
        redirect: lab.addr().to_string(),
        ..ListenerConfig::default()
    })
    .await;
    let mut s = BufReader::new(TcpStream::connect(p.addr).await.unwrap());
    let (head, _) = exchange(
        &mut s,
        "GET /json HTTP/1.1\r\nHost: thick-client.example\r\n\r\n",
    )
    .await;
    assert_eq!(head.status, 200);
    let all = p.rec.wait_for(1).await;
    assert_eq!(all[0].url, format!("{}/json", lab.url()));
    p.service.stop_all().await;
}
