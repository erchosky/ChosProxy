//! Connections to origin servers (plain TCP or TLS).

use crate::io::BoxIo;
use crate::network::{self, Forward, Purpose};
use chosproxy_http::Scheme;
use chosproxy_tls::{UpstreamTls, UpstreamVerification};
use rustls::pki_types::ServerName;
use std::borrow::Cow;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader, ReadHalf, WriteHalf};

/// Default time allowed for DNS resolution plus TCP connect (the current
/// value is in `network.toml`).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Start of the error returned when a TLS server does not agree to HTTP/2.
pub(crate) const NO_H2_ALPN: &str = "the server did not agree to HTTP/2 (ALPN)";

/// Time allowed for the upstream TLS handshake.
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

/// How to secure an upstream connection.
#[derive(Clone, Debug)]
pub enum UpstreamSecurity {
    /// Plain TCP.
    Plain,
    /// TLS with the given client configuration and verification policy.
    Tls {
        /// Client TLS configurations.
        tls: UpstreamTls,
        /// Whether the server certificate is verified.
        verification: UpstreamVerification,
    },
}

/// How long establishing a connection took.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConnectTiming {
    /// DNS resolution, in ms.
    pub dns_ms: u64,
    /// TCP connect (after DNS), in ms.
    pub connect_ms: u64,
    /// TLS handshake, in ms (0 for plain connections).
    pub tls_ms: u64,
}

/// An open connection to an origin server.
pub struct Upstream {
    /// `http` or `https`.
    pub scheme: Scheme,
    /// Host it was opened for.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Address actually connected to.
    pub peer: SocketAddr,
    /// TLS verification applied (TLS connections only).
    pub verification: Option<UpstreamVerification>,
    /// How long opening it took.
    pub timing: ConnectTiming,
    /// The upstream proxy it goes through ("HTTP proxy host:port"), if any.
    pub via: Option<String>,
    /// Plain HTTP through an HTTP proxy: requests are sent in absolute form.
    forward: Option<Forward>,
    /// Buffered read side.
    pub reader: BufReader<ReadHalf<BoxIo>>,
    /// Write side.
    pub writer: WriteHalf<BoxIo>,
}

impl std::fmt::Debug for Upstream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Upstream")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("peer", &self.peer)
            .finish_non_exhaustive()
    }
}

fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

impl Upstream {
    /// Resolves, connects and (for TLS) completes the handshake, through
    /// the upstream proxy the network settings choose for `host`.
    /// `tls_name` is the TLS server name (the client's SNI when
    /// intercepting, otherwise the host).
    pub async fn connect(
        host: &str,
        port: u16,
        tls_name: &str,
        security: &UpstreamSecurity,
    ) -> Result<Upstream, String> {
        let purpose = match security {
            UpstreamSecurity::Plain => Purpose::PlainHttp,
            UpstreamSecurity::Tls { .. } => Purpose::Tunnel,
        };
        Self::open(host, port, tls_name, security, purpose, false).await
    }

    /// Opens a connection for HTTP/2: TLS must agree on `h2` through ALPN;
    /// plain connections speak HTTP/2 with prior knowledge (h2c).
    pub async fn connect_h2(
        host: &str,
        port: u16,
        tls_name: &str,
        security: &UpstreamSecurity,
    ) -> Result<Upstream, String> {
        Self::open(host, port, tls_name, security, Purpose::Tunnel, true).await
    }

    /// Opens a connection for relaying raw bytes (an opaque CONNECT tunnel):
    /// through an HTTP proxy this is always a tunnel.
    pub async fn tunnel(host: &str, port: u16) -> Result<Upstream, String> {
        Self::open(
            host,
            port,
            host,
            &UpstreamSecurity::Plain,
            Purpose::Tunnel,
            false,
        )
        .await
    }

    async fn open(
        host: &str,
        port: u16,
        tls_name: &str,
        security: &UpstreamSecurity,
        purpose: Purpose,
        http2: bool,
    ) -> Result<Upstream, String> {
        let started = Instant::now();
        let opened = network::open(host, port, purpose).await?;
        let dns_ms = opened.dns_ms;
        let connect_ms = elapsed_ms(started).saturating_sub(dns_ms);
        let tls_started = Instant::now();
        let (io, scheme, verification): (BoxIo, Scheme, Option<UpstreamVerification>) =
            match security {
                UpstreamSecurity::Plain => (opened.io, Scheme::Http, None),
                UpstreamSecurity::Tls { tls, verification } => {
                    let name = ServerName::try_from(tls_name.to_string())
                        .map_err(|_| format!("invalid TLS server name {tls_name:?}"))?;
                    let connector = if http2 {
                        tls.connector_h2(*verification)
                    } else {
                        tls.connector(*verification)
                    };
                    let handshake = connector.connect(name, opened.io);
                    let stream = tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, handshake)
                        .await
                        .map_err(|_| format!("TLS handshake with {host}:{port} timed out"))?
                        .map_err(|e| describe_tls_error(host, &e))?;
                    if http2 && stream.get_ref().1.alpn_protocol() != Some(b"h2") {
                        return Err(format!(
                            "{NO_H2_ALPN}: {host}:{port}; send the request as HTTP/1.1 instead"
                        ));
                    }
                    (Box::new(stream), Scheme::Https, Some(*verification))
                }
            };
        let tls_ms = if scheme == Scheme::Https {
            elapsed_ms(tls_started)
        } else {
            0
        };
        let (r, w) = tokio::io::split(io);
        Ok(Upstream {
            timing: ConnectTiming {
                dns_ms,
                connect_ms,
                tls_ms,
            },
            scheme,
            host: host.to_string(),
            port,
            peer: opened.peer,
            verification,
            via: opened.via,
            forward: opened.forward,
            reader: BufReader::with_capacity(32 * 1024, r),
            writer: w,
        })
    }

    /// The whole stream again (bytes already buffered are kept), e.g. to
    /// hand it to an HTTP/2 client.
    pub fn into_io(self) -> BoxIo {
        let leftover = self.reader.buffer().to_vec();
        let io = self.reader.into_inner().unsplit(self.writer);
        Box::new(crate::io::Prefixed::new(leftover, io))
    }

    /// The bytes to write for a request that starts with `bytes`: unchanged,
    /// or in absolute form with proxy credentials when plain HTTP goes
    /// through an HTTP proxy.
    pub fn outgoing<'a>(&self, bytes: &'a [u8]) -> Cow<'a, [u8]> {
        match &self.forward {
            None => Cow::Borrowed(bytes),
            Some(f) => Cow::Owned(network::absolute_form(bytes, f)),
        }
    }

    /// True if this idle connection can carry another request to the same
    /// origin: not closed by the server and no stray bytes pending.
    pub async fn reusable_for(&mut self, scheme: Scheme, host: &str, port: u16) -> bool {
        if self.scheme != scheme || self.host != host || self.port != port {
            return false;
        }
        // Poll once without waiting: still pending means idle and healthy;
        // ready means EOF, an error, or unexpected data — not reusable.
        tokio::time::timeout(Duration::ZERO, self.reader.fill_buf())
            .await
            .is_err()
    }
}

fn describe_tls_error(host: &str, e: &std::io::Error) -> String {
    let text = e.to_string();
    if text.contains("invalid peer certificate") || text.contains("UnknownIssuer") {
        format!(
            "upstream TLS: {text}. If {host} is a lab with a self-signed certificate, add it to \
             \"Skip certificate verification\" in Proxy → HTTPS"
        )
    } else {
        format!("upstream TLS handshake with {host} failed: {text}")
    }
}
