//! One client connection: read requests, dispatch them, keep the session.
//!
//! A connection starts in proxy mode (requests in absolute form, CONNECT).
//! An intercepted CONNECT switches the same connection to tunnel mode, where
//! requests are in origin form and go to the tunnel's fixed origin.

use crate::auth::{AuthGate, AuthResult};
use crate::intercept::{self, Upgraded};
use crate::relay::relay;
use crate::responses;
use crate::service::Shared;
use crate::sni::{self, Sniffed};
use crate::websocket::WsInfo;
use chosproxy_http::framing::request_framing;
use chosproxy_http::head::{Headers, read_head};
use chosproxy_http::target::parse_authority;
use chosproxy_http::{BodyKind, HttpError, MAX_HEAD_BYTES, RequestHead, RequestTarget, Scheme};
use chosproxy_request::Upstream;
use chosproxy_request::io::{BoxIo, Prefixed};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

/// How long an idle keep-alive client connection is kept open.
const CLIENT_IDLE: Duration = Duration::from_secs(120);

/// Host name that always addresses the proxy itself (`http://chosproxy/`).
pub(crate) const MAGIC_HOST: &str = "chosproxy";

/// Per-connection context.
#[derive(Clone)]
pub(crate) struct ConnCtx {
    pub(crate) shared: Arc<Shared>,
    pub(crate) listener_id: Arc<str>,
    pub(crate) peer: SocketAddr,
    /// Present when the listener requires proxy authentication.
    pub(crate) auth: Option<Arc<AuthGate>>,
    /// The listener also serves clients that do not know about the proxy;
    /// their TLS goes to this port.
    pub(crate) invisible: Option<u16>,
    /// Offer HTTP/2 to clients of intercepted HTTPS.
    pub(crate) http2: bool,
    /// Invisible traffic goes here instead of its Host / server name.
    pub(crate) redirect: Option<(String, u16)>,
}

/// The client side of a connection (TCP, or TLS after interception).
pub(crate) struct Client {
    pub(crate) reader: BufReader<ReadHalf<BoxIo>>,
    pub(crate) writer: WriteHalf<BoxIo>,
}

impl Client {
    pub(crate) fn new(io: BoxIo) -> Client {
        let (r, w) = tokio::io::split(io);
        Client {
            reader: BufReader::with_capacity(32 * 1024, r),
            writer: w,
        }
    }

    /// Writes bytes to the client; false if the client is gone.
    pub(crate) async fn send(&mut self, bytes: &[u8]) -> bool {
        self.writer.write_all(bytes).await.is_ok() && self.writer.flush().await.is_ok()
    }

    /// Reassembles the underlying stream, keeping bytes already buffered.
    pub(crate) fn into_io(self) -> BoxIo {
        let leftover = self.reader.buffer().to_vec();
        let io = self.reader.into_inner().unsplit(self.writer);
        Box::new(Prefixed::new(leftover, io))
    }
}

/// The fixed destination of a tunnel.
#[derive(Debug, Clone)]
pub(crate) struct Origin {
    pub(crate) scheme: Scheme,
    pub(crate) host: String,
    pub(crate) port: u16,
    /// TLS server name for the upstream (client SNI when known).
    pub(crate) tls_name: String,
}

enum Mode {
    Proxy,
    Tunnel(Origin),
}

pub(crate) enum Outcome {
    KeepAlive,
    Close,
    /// Switch to raw byte relaying (opaque tunnel or protocol upgrade).
    Splice(Upstream),
    /// Relay a WebSocket connection frame by frame (capture, interception).
    WebSocket(Upstream, Box<WsInfo>),
    /// The client asked to CONNECT and the tunnel is to be intercepted.
    Intercept {
        host: String,
        port: u16,
    },
}

struct ActiveGuard(Arc<Shared>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.counters.active.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Serves one client connection until it closes or `cancel` fires.
pub(crate) async fn serve(ctx: ConnCtx, stream: TcpStream, cancel: CancellationToken) {
    ctx.shared.counters.active.fetch_add(1, Ordering::Relaxed);
    let _guard = ActiveGuard(ctx.shared.clone());
    let _ = stream.set_nodelay(true);
    let mut stream = stream;
    let mut first: Option<Outcome> = None;
    let mut client = if let Some(tls_port) = ctx.invisible {
        let (bytes, sniffed) = tokio::select! {
            _ = cancel.cancelled() => return,
            s = sni::sniff(&mut stream) => s,
        };
        let client = Client::new(Box::new(Prefixed::new(bytes, stream)));
        match sniffed {
            None => return,
            Some(Sniffed::Tls(None)) => {
                ctx.shared.tls_client_failure(
                    "(no server name)",
                    "a client spoke TLS to the invisible listener without a server name (SNI)",
                );
                return;
            }
            Some(Sniffed::Tls(Some(host))) => {
                first = Some(invisible_tls(&ctx, host, tls_port).await);
            }
            Some(Sniffed::Other) => {}
        }
        client
    } else {
        Client::new(Box::new(stream))
    };
    let mut mode = Mode::Proxy;
    let mut upstream: Option<Upstream> = None;
    loop {
        let outcome = match first.take() {
            Some(o) => o,
            None => tokio::select! {
                _ = cancel.cancelled() => return,
                o = next_exchange(&ctx, &mut client, &mode, &mut upstream) => o,
            },
        };
        match outcome {
            Outcome::KeepAlive => continue,
            Outcome::Close => break,
            Outcome::Splice(up) => {
                tokio::select! {
                    _ = cancel.cancelled() => {}
                    _ = splice(client, up) => {}
                }
                return;
            }
            Outcome::WebSocket(up, info) => {
                tokio::select! {
                    _ = cancel.cancelled() => {}
                    _ = crate::websocket::relay(&ctx, client, up, *info) => {}
                }
                return;
            }
            Outcome::Intercept { host, port } => {
                let upgraded = tokio::select! {
                    _ = cancel.cancelled() => return,
                    u = intercept::upgrade(&ctx, client, host, port) => u,
                };
                match upgraded {
                    Upgraded::Session(new_client, origin) => {
                        client = new_client;
                        mode = Mode::Tunnel(origin);
                        upstream = None;
                    }
                    Upgraded::Splice(c, up) => {
                        tokio::select! {
                            _ = cancel.cancelled() => {}
                            _ = splice(c, up) => {}
                        }
                        return;
                    }
                    Upgraded::Http2(io, origin) => {
                        tokio::select! {
                            _ = cancel.cancelled() => {}
                            _ = crate::h2_relay::serve(&ctx, io, origin) => {}
                        }
                        return;
                    }
                    Upgraded::Done => return,
                }
            }
        }
    }
    let _ = client.writer.shutdown().await;
}

/// Relays bytes in both directions until both sides close.
pub(crate) async fn splice(client: Client, up: Upstream) {
    let Client {
        mut reader,
        mut writer,
    } = client;
    let Upstream {
        reader: mut up_reader,
        writer: mut up_writer,
        ..
    } = up;
    // BufReader yields any already-buffered bytes first, so nothing is lost.
    let client_to_upstream = async {
        let _ = tokio::io::copy(&mut reader, &mut up_writer).await;
        let _ = up_writer.shutdown().await;
    };
    let upstream_to_client = async {
        let _ = tokio::io::copy(&mut up_reader, &mut writer).await;
        let _ = writer.shutdown().await;
    };
    tokio::join!(client_to_upstream, upstream_to_client);
}

async fn next_exchange(
    ctx: &ConnCtx,
    client: &mut Client,
    mode: &Mode,
    upstream: &mut Option<Upstream>,
) -> Outcome {
    let raw = match tokio::time::timeout(CLIENT_IDLE, read_head(&mut client.reader, MAX_HEAD_BYTES))
        .await
    {
        Err(_) | Ok(Ok(None)) => return Outcome::Close,
        Ok(Ok(Some(raw))) => raw,
        Ok(Err(HttpError::HeadTooLarge(n))) => {
            let msg = format!("request head larger than {n} bytes");
            client
                .send(&responses::error(
                    431,
                    "Request Header Fields Too Large",
                    &msg,
                    true,
                ))
                .await;
            return Outcome::Close;
        }
        Ok(Err(HttpError::Io(_) | HttpError::UnexpectedEof)) => return Outcome::Close,
        Ok(Err(e)) => return bad_request(client, &e.to_string()).await,
    };
    let head = match RequestHead::parse(raw) {
        Ok(h) => h,
        Err(e) => return bad_request(client, &e.to_string()).await,
    };
    let target = match RequestTarget::parse(&head.method, &head.target) {
        Ok(t) => t,
        Err(e) => return bad_request(client, &e.to_string()).await,
    };
    if let (Mode::Proxy, Some(gate)) = (mode, &ctx.auth) {
        // Requests inside an accepted CONNECT tunnel are already authorized.
        let gate = gate.clone();
        let header = head.header("proxy-authorization").map(<[u8]>::to_vec);
        let result = tokio::task::spawn_blocking(move || gate.check_blocking(header.as_deref()))
            .await
            .unwrap_or(AuthResult::Rejected);
        if result != AuthResult::Ok {
            if result == AuthResult::Rejected {
                ctx.shared.auth_failure(&ctx.listener_id, ctx.peer);
            }
            client.send(&responses::proxy_auth_required()).await;
            return Outcome::Close;
        }
    }
    match mode {
        Mode::Proxy => proxy_request(ctx, client, upstream, head, target).await,
        Mode::Tunnel(origin) => tunnel_request(ctx, client, upstream, head, target, origin).await,
    }
}

async fn proxy_request(
    ctx: &ConnCtx,
    client: &mut Client,
    upstream: &mut Option<Upstream>,
    head: RequestHead,
    target: RequestTarget,
) -> Outcome {
    match target {
        RequestTarget::Authority { host, port } => connect(ctx, client, host, port).await,
        RequestTarget::Absolute {
            scheme: Scheme::Http,
            host,
            port,
            path,
        } => {
            if host == MAGIC_HOST || ctx.shared.is_self(&host, port) {
                serve_local(ctx, client, &head, &path).await
            } else {
                let origin = Origin {
                    scheme: Scheme::Http,
                    tls_name: host.clone(),
                    host,
                    port,
                };
                relay(ctx, client, upstream, head, &origin, path).await
            }
        }
        RequestTarget::Absolute {
            scheme: Scheme::Https,
            ..
        } => {
            let msg = "absolute-form https:// requests are not supported; \
                       clients use CONNECT for HTTPS";
            client
                .send(&responses::error(501, "Not Implemented", msg, true))
                .await;
            Outcome::Close
        }
        RequestTarget::Origin(path) => {
            let to_us = match head.header_str("host") {
                None => true,
                Some(h) => match parse_authority(&h, Some(80)) {
                    Ok((host, port)) => host == MAGIC_HOST || ctx.shared.is_self(&host, port),
                    Err(_) => false,
                },
            };
            if to_us {
                serve_local(ctx, client, &head, &path).await
            } else if ctx.invisible.is_some() {
                let authority = head.header_str("host").unwrap_or_default();
                let target = match &ctx.redirect {
                    Some((h, p)) => Ok((h.clone(), *p)),
                    None => parse_authority(&authority, Some(80)),
                };
                match target {
                    Ok((host, port)) => {
                        let origin = Origin {
                            scheme: Scheme::Http,
                            tls_name: host.clone(),
                            host,
                            port,
                        };
                        relay(ctx, client, upstream, head, &origin, path).await
                    }
                    Err(e) => bad_request(client, &format!("invalid Host header: {e}")).await,
                }
            } else {
                bad_request(
                    client,
                    "this listener is an explicit HTTP proxy; configure the client to use it \
                     as its proxy, or turn on \"invisible\" for the listener",
                )
                .await
            }
        }
        RequestTarget::Asterisk => bad_request(client, "OPTIONS * is not proxied").await,
    }
}

async fn tunnel_request(
    ctx: &ConnCtx,
    client: &mut Client,
    upstream: &mut Option<Upstream>,
    head: RequestHead,
    target: RequestTarget,
    origin: &Origin,
) -> Outcome {
    match target {
        RequestTarget::Origin(path) => relay(ctx, client, upstream, head, origin, path).await,
        RequestTarget::Absolute {
            host, port, path, ..
        } if host == origin.host && port == origin.port => {
            relay(ctx, client, upstream, head, origin, path).await
        }
        RequestTarget::Absolute { .. } => {
            bad_request(client, "request inside the tunnel targets a different host").await
        }
        RequestTarget::Authority { .. } => {
            bad_request(client, "CONNECT is not allowed inside a tunnel").await
        }
        RequestTarget::Asterisk => bad_request(client, "OPTIONS * is not proxied").await,
    }
}

async fn bad_request(client: &mut Client, detail: &str) -> Outcome {
    client
        .send(&responses::error(400, "Bad Request", detail, true))
        .await;
    Outcome::Close
}

/// Pages served by the proxy itself: info page and CA certificate download.
async fn serve_local(
    ctx: &ConnCtx,
    client: &mut Client,
    head: &RequestHead,
    path: &str,
) -> Outcome {
    let has_body = request_framing(head).map_or(true, |f| f.kind != BodyKind::Empty);
    let close = has_body || !head.keep_alive();
    let ca = ctx.shared.ca_certificate(&ctx.listener_id);
    let route = path.split('?').next().unwrap_or("/");
    let response = match (route, &ca) {
        ("/", _) => responses::info_page(ca.is_some(), close),
        ("/cert" | "/cert.crt" | "/cert.der", Some((der, _))) => {
            responses::certificate(der, false, close)
        }
        ("/cert.pem", Some((_, pem))) => responses::certificate(pem.as_bytes(), true, close),
        ("/cert" | "/cert.crt" | "/cert.der" | "/cert.pem", None) => responses::error(
            503,
            "Service Unavailable",
            "the local CA is not available; check Proxy → HTTPS in the application",
            close,
        ),
        _ => responses::error(404, "Not Found", "no such page on the proxy", close),
    };
    if client.send(&response).await && !close {
        Outcome::KeepAlive
    } else {
        Outcome::Close
    }
}

/// A client of an invisible listener started TLS for `host`: intercept it
/// as if it had sent `CONNECT host:port`, or relay it untouched.
async fn invisible_tls(ctx: &ConnCtx, host: String, port: u16) -> Outcome {
    let (host, port) = ctx.redirect.clone().unwrap_or((host, port));
    if host == MAGIC_HOST || ctx.shared.is_self(&host, port) {
        return Outcome::Close;
    }
    let policy = ctx.shared.tls_policy();
    if policy.intercept && ctx.shared.can_intercept() && !policy.is_passthrough_at(&host, port) {
        return Outcome::Intercept { host, port };
    }
    match Upstream::tunnel(&host, port).await {
        Ok(up) => {
            ctx.shared.counters.tunnels.fetch_add(1, Ordering::Relaxed);
            Outcome::Splice(up)
        }
        Err(msg) => {
            tracing::debug!(%host, error = %msg, "invisible tunnel failed");
            ctx.shared.counters.errors.fetch_add(1, Ordering::Relaxed);
            Outcome::Close
        }
    }
}

/// Handles CONNECT: intercept (when enabled and not excluded) or tunnel.
async fn connect(ctx: &ConnCtx, client: &mut Client, host: String, port: u16) -> Outcome {
    if host == MAGIC_HOST || ctx.shared.is_self(&host, port) {
        return bad_request(client, "refusing to tunnel to the proxy itself").await;
    }
    let policy = ctx.shared.tls_policy();
    if policy.intercept && ctx.shared.can_intercept() && !policy.is_passthrough_at(&host, port) {
        // Upstream is connected lazily, after the client's first request, so
        // connection errors can be reported to the browser as HTTP errors.
        if !client
            .send(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
        {
            return Outcome::Close;
        }
        return Outcome::Intercept { host, port };
    }
    match Upstream::tunnel(&host, port).await {
        Ok(up) => {
            if !client
                .send(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
            {
                return Outcome::Close;
            }
            ctx.shared.counters.tunnels.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(%host, port, "CONNECT tunnel established (not decrypted)");
            Outcome::Splice(up)
        }
        Err(msg) => {
            ctx.shared.counters.errors.fetch_add(1, Ordering::Relaxed);
            client
                .send(&responses::error(502, "Bad Gateway", &msg, true))
                .await;
            Outcome::Close
        }
    }
}
