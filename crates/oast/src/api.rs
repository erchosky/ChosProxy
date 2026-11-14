//! The polling API of a self-hosted ChosOAST server.
//!
//! `GET /chosoast/v1/interactions?after=<seq>` with `Authorization: Bearer
//! <token>` returns the interactions recorded after `seq`. It is served
//! over TLS with the server's own certificate; the client pins that
//! certificate's SHA-256 instead of trusting a public CA, so no domain
//! certificate is needed and nobody in between can read the token.

use crate::server::{Interaction, Log};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use subtle::ConstantTimeEq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Interactions returned per request.
pub const PAGE: usize = 500;
/// Largest API response the client reads.
const MAX_RESPONSE: usize = 64 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);

/// One page of interactions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionPage {
    /// Interactions after the requested sequence number, oldest first.
    pub interactions: Vec<Interaction>,
    /// The server's newest sequence number.
    pub latest: u64,
    /// The server's DNS zone.
    pub zone: String,
}

/// Why polling failed.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Network or TLS failure.
    #[error("cannot reach the server: {0}")]
    Io(#[from] std::io::Error),
    /// The certificate did not match the pinned fingerprint.
    #[error("the server certificate does not match the pinned fingerprint")]
    Pin,
    /// The server refused the token.
    #[error("the server refused the access token")]
    Unauthorized,
    /// Anything else unexpected in the answer.
    #[error("unexpected answer from the server: {0}")]
    Protocol(String),
    /// The request timed out.
    #[error("the server did not answer in time")]
    Timeout,
}

/// Lowercase hex SHA-256 of a DER certificate, bytes separated by `:`.
pub fn fingerprint(cert_der: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, cert_der)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn normalize_pin(pin: &str) -> String {
    pin.chars()
        .filter(char::is_ascii_hexdigit)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

// ---------------------------------------------------------------- server

/// Serves the API until the listener fails or the task is aborted.
pub async fn serve(
    listener: TcpListener,
    tls: Arc<rustls::ServerConfig>,
    token: String,
    zone: String,
    log: Arc<Log>,
) {
    let acceptor = tokio_rustls::TlsAcceptor::from(tls);
    let token = Arc::new(token);
    let zone = Arc::new(zone);
    loop {
        let Some((stream, _)) = crate::server::accept(&listener).await else {
            continue;
        };
        let (acceptor, token, zone, log) =
            (acceptor.clone(), token.clone(), zone.clone(), log.clone());
        tokio::spawn(async move {
            let Ok(Ok(mut s)) = tokio::time::timeout(TIMEOUT, acceptor.accept(stream)).await else {
                return;
            };
            let mut head = Vec::new();
            let mut chunk = [0u8; 4096];
            while chosproxy_http::head::find_head_end(&head).is_none() && head.len() < 16 * 1024 {
                match tokio::time::timeout(TIMEOUT, s.read(&mut chunk)).await {
                    Ok(Ok(n)) if n > 0 => head.extend_from_slice(&chunk[..n]),
                    _ => return,
                }
            }
            let (status, body) = answer(&head, &token, &zone, &log);
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Cache-Control: no-store\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(reply.as_bytes()).await;
            let _ = s.write_all(body.as_bytes()).await;
            let _ = s.shutdown().await;
        });
    }
}

fn answer(head: &[u8], token: &str, zone: &str, log: &Log) -> (&'static str, String) {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let presented = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(n, _)| n.trim().eq_ignore_ascii_case("authorization"))
        .and_then(|(_, v)| v.trim().strip_prefix("Bearer ").map(str::to_string))
        .unwrap_or_default();
    if !bool::from(presented.as_bytes().ct_eq(token.as_bytes())) || token.is_empty() {
        return ("401 Unauthorized", "{\"error\":\"unauthorized\"}".into());
    }
    let mut parts = request_line.split(' ');
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if method != "GET" || path != "/chosoast/v1/interactions" {
        return ("404 Not Found", "{\"error\":\"not found\"}".into());
    }
    let after = query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == "after")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let page = InteractionPage {
        interactions: log.after(after, PAGE),
        latest: log.latest(),
        zone: zone.to_string(),
    };
    match serde_json::to_string(&page) {
        Ok(json) => ("200 OK", json),
        Err(_) => ("500 Internal Server Error", "{}".into()),
    }
}

// ---------------------------------------------------------------- client

/// Accepts exactly one certificate, identified by its SHA-256.
#[derive(Debug)]
struct PinnedCert {
    pin: String,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for PinnedCert {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let got = normalize_pin(&fingerprint(end_entity));
        if bool::from(got.as_bytes().ct_eq(self.pin.as_bytes())) {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("certificate pin mismatch".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Where and how to poll a self-hosted server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    /// API host name or IP.
    pub host: String,
    /// API port.
    pub port: u16,
    /// Access token.
    pub token: String,
    /// SHA-256 fingerprint of the server certificate (hex, `:` optional).
    pub pin: String,
}

/// Fetches the interactions after `after`.
pub async fn fetch(remote: &Remote, after: u64) -> Result<InteractionPage, ApiError> {
    tokio::time::timeout(TIMEOUT, fetch_inner(remote, after))
        .await
        .map_err(|_| ApiError::Timeout)?
}

async fn fetch_inner(remote: &Remote, after: u64) -> Result<InteractionPage, ApiError> {
    let pin = normalize_pin(&remote.pin);
    if pin.len() != 64 {
        return Err(ApiError::Protocol(
            "the certificate fingerprint must be a SHA-256 (64 hex digits)".into(),
        ));
    }
    if remote.token.is_empty() || remote.token.contains(['\r', '\n']) {
        return Err(ApiError::Protocol("invalid access token".into()));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| ApiError::Protocol(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedCert { pin, provider }))
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(remote.host.clone())
        .map_err(|_| ApiError::Protocol(format!("invalid host {:?}", remote.host)))?;
    let tcp = TcpStream::connect((remote.host.as_str(), remote.port)).await?;
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .map_err(|e| {
            if e.to_string().contains("pin mismatch") {
                ApiError::Pin
            } else {
                ApiError::Io(e)
            }
        })?;
    let request = format!(
        "GET /chosoast/v1/interactions?after={after} HTTP/1.1\r\nHost: {}\r\n\
         Authorization: Bearer {}\r\nConnection: close\r\n\r\n",
        remote.host, remote.token
    );
    tls.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    (&mut tls)
        .take(MAX_RESPONSE as u64)
        .read_to_end(&mut response)
        .await?;
    let end = chosproxy_http::head::find_head_end(&response)
        .ok_or_else(|| ApiError::Protocol("no HTTP response".into()))?;
    let status_line = String::from_utf8_lossy(&response[..end]);
    let status = status_line.split(' ').nth(1).unwrap_or("");
    match status {
        "200" => {
            serde_json::from_slice(&response[end..]).map_err(|e| ApiError::Protocol(e.to_string()))
        }
        "401" => Err(ApiError::Unauthorized),
        other => Err(ApiError::Protocol(format!("status {other}"))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::cert::SelfSigned;
    use crate::server::{OastServer, ServerConfig};

    #[tokio::test]
    async fn polls_with_token_and_pin() {
        let cert = SelfSigned::generate("localhost").unwrap();
        let server = OastServer::start(ServerConfig {
            zone: "oast.test".into(),
            addresses: Default::default(),
            http: Some("127.0.0.1:0".parse().unwrap()),
            https: None,
            dns: None,
            smtp: None,
            keep: 100,
        })
        .await
        .unwrap();
        let api = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = api.local_addr().unwrap().port();
        let task = tokio::spawn(serve(
            api,
            cert.server_config().unwrap(),
            "s3cret-token".into(),
            "oast.test".into(),
            server.log().clone(),
        ));

        let id = crate::id::new_id().unwrap();
        let mut s = TcpStream::connect(server.http_addr().unwrap())
            .await
            .unwrap();
        s.write_all(format!("GET /{id} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut sink = Vec::new();
        s.read_to_end(&mut sink).await.unwrap();

        let remote = Remote {
            host: "localhost".into(),
            port,
            token: "s3cret-token".into(),
            pin: cert.fingerprint.to_uppercase(),
        };
        let page = fetch(&remote, 0).await.unwrap();
        assert_eq!((page.latest, page.zone.as_str()), (1, "oast.test"));
        assert_eq!(page.interactions[0].ids, vec![id]);
        assert!(fetch(&remote, 1).await.unwrap().interactions.is_empty());

        let wrong_token = Remote {
            token: "nope".into(),
            ..remote.clone()
        };
        assert!(matches!(
            fetch(&wrong_token, 0).await,
            Err(ApiError::Unauthorized)
        ));
        let wrong_pin = Remote {
            pin: "00".repeat(32),
            ..remote
        };
        assert!(matches!(fetch(&wrong_pin, 0).await, Err(ApiError::Pin)));
        task.abort();
    }
}
