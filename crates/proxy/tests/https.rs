//! HTTPS interception end to end: a real TLS client goes through CONNECT to
//! the proxy, which presents a certificate from the local CA and relays to an
//! HTTPS lab server.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_core::config::{ListenerConfig, TlsConfig};
use chosproxy_core::event::ListenerState;
use chosproxy_core::exchange::CapturedExchange;
use chosproxy_http::content::split_message;
use chosproxy_http::framing::response_framing;
use chosproxy_http::head::read_head;
use chosproxy_http::{BodyReader, ResponseHead};
use chosproxy_lab::LabServer;
use chosproxy_proxy::{CaptureLimits, CaptureSink, ProxyObserver, ProxyService, TlsContext};
use chosproxy_tls::{CaStore, Interceptor, UpstreamTls};
use rustls::pki_types::{CertificateDer, ServerName};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[derive(Default)]
struct Recorder {
    exchanges: Mutex<Vec<CapturedExchange>>,
    tls_failures: Mutex<Vec<(String, String)>>,
}

impl CaptureSink for Recorder {
    fn record(&self, exchange: CapturedExchange) {
        self.exchanges.lock().unwrap().push(exchange);
    }
}

impl ProxyObserver for Recorder {
    fn listener_changed(&self, _: &str, _: &ListenerState) {}
    fn tls_client_failure(&self, host: &str, error: &str) {
        self.tls_failures
            .lock()
            .unwrap()
            .push((host.into(), error.into()));
    }
}

impl Recorder {
    async fn wait_for(&self, n: usize) -> Vec<CapturedExchange> {
        for _ in 0..300 {
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

struct Harness {
    proxy: ProxyService,
    addr: SocketAddr,
    rec: Arc<Recorder>,
    ca_der: CertificateDer<'static>,
    _dir: tempfile::TempDir,
}

/// `upstream_roots`: extra roots the proxy trusts upstream (e.g. the lab cert).
async fn harness(policy: TlsConfig, upstream_roots: &[CertificateDer<'static>]) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let (ca, _) = CaStore::new(dir.path()).load_or_create().unwrap();
    let ca_der = ca.cert_der().clone();
    let rec = Arc::new(Recorder::default());
    let proxy = ProxyService::new(
        rec.clone(),
        rec.clone(),
        CaptureLimits {
            max_message_bytes: 1 << 20,
        },
    );
    proxy.set_tls(Some(TlsContext {
        interceptor: Arc::new(Interceptor::new(Arc::new(ca)).unwrap()),
        upstream: UpstreamTls::new(upstream_roots).unwrap(),
    }));
    proxy.set_tls_policy(policy);
    let addr = proxy
        .start(&ListenerConfig {
            port: 0,
            ..ListenerConfig::default()
        })
        .await
        .unwrap();
    Harness {
        proxy,
        addr,
        rec,
        ca_der,
        _dir: dir,
    }
}

fn no_skip() -> TlsConfig {
    TlsConfig {
        skip_verification_hosts: vec![],
        ..TlsConfig::default()
    }
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

/// Opens CONNECT through the proxy and returns the raw tunnel.
async fn connect(proxy: SocketAddr, authority: &str) -> BufReader<TcpStream> {
    let mut s = BufReader::new(TcpStream::connect(proxy).await.unwrap());
    s.get_mut()
        .write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let head = read_head(&mut s, 4096).await.unwrap().unwrap();
    assert!(
        head.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&head)
    );
    s
}

async fn tls_over(
    tunnel: BufReader<TcpStream>,
    sni: &str,
    root: &CertificateDer<'static>,
) -> std::io::Result<BufReader<tokio_rustls::client::TlsStream<BufReader<TcpStream>>>> {
    let name = ServerName::try_from(sni.to_string()).unwrap();
    let stream = tls_client(root).connect(name, tunnel).await?;
    Ok(BufReader::new(stream))
}

async fn request<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut BufReader<S>,
    path: &str,
) -> (ResponseHead, Vec<u8>) {
    s.get_mut()
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: lab\r\n\r\n").as_bytes())
        .await
        .unwrap();
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

#[tokio::test]
async fn intercepts_https_and_captures_decrypted_traffic() {
    let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    // Default policy: loopback upstreams are not verified (self-signed labs).
    let h = harness(TlsConfig::default(), &[]).await;
    let authority = lab.addr().to_string();
    let tunnel = connect(h.addr, &authority).await;
    let mut tls = tls_over(tunnel, "127.0.0.1", &h.ca_der).await.unwrap();
    let (head, body) = request(&mut tls, "/json").await;
    assert_eq!(head.status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"name\":\"lab\""));
    // Keep-alive inside the tunnel.
    let (head, _) = request(&mut tls, "/status/404").await;
    assert_eq!(head.status, 404);

    let all = h.rec.wait_for(2).await;
    let ex = &all[0];
    assert_eq!(ex.scheme, "https");
    assert!(ex.tls);
    assert_eq!(ex.url, format!("https://{authority}/json"));
    assert!(String::from_utf8_lossy(&ex.request).starts_with("GET /json HTTP/1.1"));
    assert!(
        split_message(ex.response.as_deref().unwrap())
            .1
            .starts_with(b"{")
    );
    assert!(ex.notes.as_deref().unwrap().contains("not verified"));
    assert_eq!(all[1].status, Some(404));
    let stats = h.proxy.stats();
    assert_eq!(stats.intercepted, 1);
    assert_eq!(stats.tunnels, 0);
}

#[tokio::test]
async fn verified_upstream_uses_client_sni() {
    let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let lab_cert = lab.tls().unwrap().cert_der.clone();
    let h = harness(no_skip(), &[lab_cert]).await;
    // CONNECT by IP, SNI "localhost": the leaf and the upstream name follow SNI.
    let tunnel = connect(h.addr, &lab.addr().to_string()).await;
    let mut tls = tls_over(tunnel, "localhost", &h.ca_der).await.unwrap();
    let (head, _) = request(&mut tls, "/json").await;
    assert_eq!(head.status, 200);
    let ex = h.rec.wait_for(1).await.remove(0);
    assert!(ex.notes.is_none(), "verified upstream needs no note");
}

#[tokio::test]
async fn untrusted_upstream_certificate_is_a_clear_502() {
    let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h = harness(no_skip(), &[]).await;
    let tunnel = connect(h.addr, &lab.addr().to_string()).await;
    let mut tls = tls_over(tunnel, "localhost", &h.ca_der).await.unwrap();
    let (head, body) = request(&mut tls, "/json").await;
    assert_eq!(head.status, 502);
    assert!(String::from_utf8_lossy(&body).contains("Skip certificate verification"));
    let ex = h.rec.wait_for(1).await.remove(0);
    assert!(ex.error.unwrap().contains("upstream TLS"));
}

#[tokio::test]
async fn client_that_distrusts_the_ca_is_reported() {
    let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h = harness(TlsConfig::default(), &[]).await;
    let other_dir = tempfile::tempdir().unwrap();
    let (other_ca, _) = CaStore::new(other_dir.path()).load_or_create().unwrap();
    let tunnel = connect(h.addr, &lab.addr().to_string()).await;
    assert!(
        tls_over(tunnel, "localhost", other_ca.cert_der())
            .await
            .is_err()
    );
    for _ in 0..200 {
        if !h.rec.tls_failures.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let failures = h.rec.tls_failures.lock().unwrap().clone();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, "localhost");
    assert!(failures[0].1.contains("local CA"), "{}", failures[0].1);
    assert_eq!(h.proxy.stats().tls_client_failures, 1);
}

#[tokio::test]
async fn passthrough_hosts_and_disabled_interception_are_not_decrypted() {
    let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let lab_cert = lab.tls().unwrap().cert_der.clone();
    for policy in [
        TlsConfig {
            passthrough_hosts: vec!["127.0.0.1".into()],
            ..TlsConfig::default()
        },
        TlsConfig {
            intercept: false,
            ..TlsConfig::default()
        },
    ] {
        let h = harness(policy, &[]).await;
        // The client sees the lab's own certificate, end to end.
        let tunnel = connect(h.addr, &lab.addr().to_string()).await;
        let mut tls = tls_over(tunnel, "localhost", &lab_cert).await.unwrap();
        assert_eq!(request(&mut tls, "/json").await.0.status, 200);
        assert_eq!(h.proxy.stats().tunnels, 1);
        assert_eq!(h.rec.exchanges.lock().unwrap().len(), 0);
    }
}

#[tokio::test]
async fn plain_http_inside_connect_is_captured() {
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let h = harness(TlsConfig::default(), &[]).await;
    let mut tunnel = connect(h.addr, &lab.addr().to_string()).await;
    let (head, _) = request(&mut tunnel, "/json").await;
    assert_eq!(head.status, 200);
    let ex = h.rec.wait_for(1).await.remove(0);
    assert_eq!(ex.scheme, "http");
    assert!(!ex.tls);
}

#[tokio::test]
async fn ca_certificate_is_downloadable_from_the_proxy() {
    let h = harness(TlsConfig::default(), &[]).await;
    let mut s = BufReader::new(TcpStream::connect(h.addr).await.unwrap());
    let (head, body) = {
        s.get_mut()
            .write_all(b"GET http://chosproxy/cert HTTP/1.1\r\nHost: chosproxy\r\n\r\n")
            .await
            .unwrap();
        let raw = read_head(&mut s, 4096).await.unwrap().unwrap();
        let head = ResponseHead::parse(raw).unwrap();
        let mut body = BodyReader::new(response_framing("GET", &head).unwrap().kind);
        let mut out = Vec::new();
        while let Some(p) = body.next_chunk(&mut s).await.unwrap() {
            out.extend(p);
        }
        (head, out)
    };
    assert_eq!(head.status, 200);
    assert_eq!(body, h.ca_der.as_ref());
    let (head, pem) = request(&mut s, "http://chosproxy/cert.pem").await;
    assert_eq!(head.status, 200);
    assert!(String::from_utf8_lossy(&pem).starts_with("-----BEGIN CERTIFICATE-----"));
    assert!(!String::from_utf8_lossy(&pem).contains("PRIVATE"));
    let (head, _) = request(&mut s, "http://chosproxy/").await;
    assert_eq!(head.status, 200);
}
