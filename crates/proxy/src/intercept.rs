//! What happens after an intercepted CONNECT is accepted: look at the first
//! bytes the client sends and decide between TLS interception, plain HTTP
//! inside the tunnel, or an opaque tunnel for anything else.

use crate::conn::{Client, ConnCtx, Origin};
use chosproxy_http::Scheme;
use chosproxy_request::Upstream;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_rustls::LazyConfigAcceptor;

/// How long to wait for the client to speak first after CONNECT. Protocols
/// where the server speaks first fall back to an opaque tunnel.
const FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(3);
/// Time allowed for the client-side TLS handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// TLS record type of a handshake message (ClientHello).
const TLS_HANDSHAKE_RECORD: u8 = 0x16;

pub(crate) enum Upgraded {
    /// Continue the session over the (possibly decrypted) tunnel.
    Session(Client, Origin),
    /// Relay bytes without inspection.
    Splice(Client, Upstream),
    /// The client chose HTTP/2 (ALPN `h2`) on the decrypted stream.
    Http2(chosproxy_request::io::BoxIo, Origin),
    /// The connection is finished.
    Done,
}

pub(crate) async fn upgrade(ctx: &ConnCtx, client: Client, host: String, port: u16) -> Upgraded {
    let mut reader = BufReader::new(client.into_io());
    let first = match tokio::time::timeout(FIRST_BYTE_TIMEOUT, reader.fill_buf()).await {
        Ok(Ok([])) | Ok(Err(_)) => return Upgraded::Done,
        Ok(Ok(bytes)) => Some(bytes[0]),
        Err(_) => None,
    };
    match first {
        Some(TLS_HANDSHAKE_RECORD) => tls(ctx, reader, host, port).await,
        Some(b) if b.is_ascii_uppercase() => {
            // Plain HTTP sent through CONNECT (e.g. ws:// via proxy): capture it.
            Upgraded::Session(
                Client::new(Box::new(reader)),
                Origin {
                    scheme: Scheme::Http,
                    tls_name: host.clone(),
                    host,
                    port,
                },
            )
        }
        _ => opaque(ctx, Client::new(Box::new(reader)), &host, port).await,
    }
}

async fn opaque(ctx: &ConnCtx, client: Client, host: &str, port: u16) -> Upgraded {
    match Upstream::tunnel(host, port).await {
        Ok(up) => {
            ctx.shared.counters.tunnels.fetch_add(1, Ordering::Relaxed);
            Upgraded::Splice(client, up)
        }
        Err(e) => {
            tracing::debug!(%host, error = %e, "opaque tunnel failed");
            ctx.shared.counters.errors.fetch_add(1, Ordering::Relaxed);
            Upgraded::Done
        }
    }
}

async fn tls(
    ctx: &ConnCtx,
    reader: BufReader<chosproxy_request::io::BoxIo>,
    host: String,
    port: u16,
) -> Upgraded {
    let Some(interceptor) = ctx.shared.interceptor(&ctx.listener_id) else {
        return opaque(ctx, Client::new(Box::new(reader)), &host, port).await;
    };
    let handshake = async {
        let start = LazyConfigAcceptor::new(rustls::server::Acceptor::default(), reader)
            .await
            .map_err(|e| (host.clone(), format!("invalid TLS ClientHello: {e}")))?;
        let sni = start.client_hello().server_name().map(str::to_string);
        let cert_host = sni.clone().unwrap_or_else(|| host.clone());
        let config = interceptor
            .server_config_with(&cert_host, ctx.http2)
            .map_err(|e| (cert_host.clone(), e.to_string()))?;
        let stream = start
            .into_stream(config)
            .await
            .map_err(|e| (cert_host.clone(), describe_client_error(&e)))?;
        Ok::<_, (String, String)>((stream, sni))
    };
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake).await {
        Ok(Ok((stream, sni))) => {
            ctx.shared
                .counters
                .intercepted
                .fetch_add(1, Ordering::Relaxed);
            let h2 = stream.get_ref().1.alpn_protocol() == Some(b"h2");
            let origin = Origin {
                scheme: Scheme::Https,
                tls_name: sni.unwrap_or_else(|| host.clone()),
                host,
                port,
            };
            if h2 {
                Upgraded::Http2(Box::new(stream), origin)
            } else {
                Upgraded::Session(Client::new(Box::new(stream)), origin)
            }
        }
        Ok(Err((name, error))) => {
            ctx.shared.tls_client_failure(&name, &error);
            Upgraded::Done
        }
        Err(_) => {
            ctx.shared
                .tls_client_failure(&host, "TLS handshake with the client timed out");
            Upgraded::Done
        }
    }
}

/// Explains the most common failure: the browser does not trust our CA.
fn describe_client_error(e: &std::io::Error) -> String {
    let text = e.to_string();
    let untrusted = [
        "UnknownCA",
        "unknown ca",
        "BadCertificate",
        "CertificateUnknown",
        "bad certificate",
    ];
    if untrusted.iter().any(|m| text.contains(m)) {
        format!(
            "the client rejected the interception certificate ({text}); install or trust the \
             local CA (Proxy → HTTPS) or use the lab browser"
        )
    } else {
        format!("client TLS handshake failed: {text}")
    }
}
