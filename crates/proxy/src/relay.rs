//! Relaying one HTTP exchange upstream and capturing it.

use crate::CaptureLimits;
use crate::capture::Capture;
use crate::conn::{Client, ConnCtx, Origin, Outcome};
use crate::hold::{Decision, HeldKind, HeldMessage, HeldSummary};
use crate::hook::{HookTarget, HookVerdict};
use crate::match_replace::{Applied, Side};
use crate::responses;
use crate::websocket::WsInfo;
use chosproxy_core::ScopeTarget;
use chosproxy_core::Source;
use chosproxy_core::config::RequestFacts;
use chosproxy_core::exchange::{CapturedExchange, flags};
use chosproxy_core::time::now_ms;
use chosproxy_http::content::{
    classify_mime, dechunk, decode_content, extension_of, split_message,
};
use chosproxy_http::framing::{request_framing, response_framing};
use chosproxy_http::head::{Headers, read_head};
use chosproxy_http::rewrite::rewrite_head;
use chosproxy_http::target::format_url;
use chosproxy_http::{
    BodyKind, BodyReader, HttpError, MAX_HEAD_BYTES, RequestHead, ResponseHead, Scheme,
};
use chosproxy_request::{Upstream, UpstreamSecurity};
use chosproxy_tls::UpstreamVerification;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

/// How long to wait for the upstream response head (network settings).
fn response_timeout() -> Duration {
    Duration::from_secs(u64::from(
        chosproxy_request::network().timeouts.response_secs,
    ))
}
/// Longest silence tolerated while a body is being transferred.
fn body_idle() -> Duration {
    Duration::from_secs(u64::from(chosproxy_request::network().timeouts.idle_secs))
}
/// Headers addressed to the proxy itself; never forwarded.
const PROXY_HEADERS: &[&str] = &["Proxy-Connection", "Proxy-Authorization"];

/// Everything known about an exchange while it is in flight.
struct Recording {
    started: Instant,
    started_at: i64,
    listener: Arc<str>,
    client_addr: SocketAddr,
    scheme: Scheme,
    method: String,
    host: String,
    port: u16,
    path: String,
    version: String,
    request: Capture,
    response: Option<Capture>,
    response_head: Option<ResponseHead>,
    original_request: Option<Vec<u8>>,
    original_response: Option<Vec<u8>>,
    server_ip: Option<String>,
    notes: Vec<String>,
    flags: i64,
    ws_key: Option<i64>,
}

impl Recording {
    fn finish(self, error: Option<String>) -> CapturedExchange {
        let mut flags = self.flags;
        if self.request.truncated() {
            flags |= flags::REQUEST_TRUNCATED;
        }
        let (status, reason, mime) = match &self.response_head {
            Some(h) => {
                let body = self
                    .response
                    .as_ref()
                    .map(|c| split_message(c.bytes()).1)
                    .unwrap_or_default();
                let mime = mime_of(h, &self.path, body);
                (
                    Some(h.status),
                    Some(h.reason.clone()),
                    Some(mime.to_string()),
                )
            }
            None => (None, None, None),
        };
        let (response, response_len) = match self.response {
            Some(c) => {
                if c.truncated() {
                    flags |= flags::RESPONSE_TRUNCATED;
                }
                let len = c.total();
                (Some(c.into_bytes()), Some(len))
            }
            None => (None, None),
        };
        let url = format_url(self.scheme, &self.host, self.port, &self.path);
        CapturedExchange {
            started_at: self.started_at,
            duration_ms: Some(
                i64::try_from(self.started.elapsed().as_millis()).unwrap_or(i64::MAX),
            ),
            source: Source::Proxy,
            listener: Some(self.listener.to_string()),
            client_addr: Some(self.client_addr.to_string()),
            scheme: self.scheme.as_str().to_string(),
            extension: extension_of(&self.path),
            method: self.method,
            host: self.host,
            port: self.port,
            path: self.path,
            url,
            http_version: self.version,
            status,
            reason,
            mime: mime.filter(|m| !m.is_empty()),
            request_len: self.request.total(),
            request: self.request.into_bytes(),
            response,
            response_len,
            original_request: self.original_request,
            original_response: self.original_response,
            server_ip: self.server_ip,
            tls: self.scheme == Scheme::Https,
            error,
            notes: (!self.notes.is_empty()).then(|| self.notes.join("\n")),
            flags,
            ws_key: self.ws_key,
        }
    }
}

/// MIME category; sniffs the body only when the server sent no Content-Type.
pub(crate) fn mime_of(head: &ResponseHead, path: &str, raw_body: &[u8]) -> &'static str {
    let content_type = head.header_str("content-type");
    if content_type.is_some() {
        return classify_mime(content_type.as_deref(), path, &[]);
    }
    let chunked = head
        .header_tokens("transfer-encoding")
        .last()
        .is_some_and(|t| t == "chunked");
    let mut body = if chunked {
        dechunk(raw_body).map(|(b, _)| b).unwrap_or_default()
    } else {
        raw_body.to_vec()
    };
    if let Some(enc) = head.header_str("content-encoding")
        && let Ok(decoded) = decode_content(&body, &enc, 1024 * 1024)
    {
        body = decoded;
    }
    classify_mime(None, path, &body)
}

enum HeadError {
    /// The server closed (or reset) before sending any response byte.
    ClosedEarly,
    Timeout,
    Invalid(String),
}

async fn read_response_head(up: &mut Upstream) -> Result<ResponseHead, HeadError> {
    match tokio::time::timeout(
        response_timeout(),
        read_head(&mut up.reader, MAX_HEAD_BYTES),
    )
    .await
    {
        Err(_) => Err(HeadError::Timeout),
        Ok(Ok(None)) | Ok(Err(HttpError::UnexpectedEof)) => Err(HeadError::ClosedEarly),
        Ok(Err(HttpError::Io(e)))
            if matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
            ) =>
        {
            Err(HeadError::ClosedEarly)
        }
        Ok(Err(e)) => Err(HeadError::Invalid(e.to_string())),
        Ok(Ok(Some(raw))) => {
            ResponseHead::parse(raw).map_err(|e| HeadError::Invalid(e.to_string()))
        }
    }
}

impl HeadError {
    fn describe(&self) -> String {
        match self {
            HeadError::ClosedEarly => "upstream closed the connection without a response".into(),
            HeadError::Timeout => format!(
                "upstream did not respond within {}s",
                response_timeout().as_secs()
            ),
            HeadError::Invalid(e) => format!("invalid upstream response: {e}"),
        }
    }

    fn status(&self) -> (u16, &'static str) {
        match self {
            HeadError::Timeout => (504, "Gateway Timeout"),
            _ => (502, "Bad Gateway"),
        }
    }
}

/// Sends a proxy-generated error to the client and records the failed exchange.
async fn fail(
    ctx: &ConnCtx,
    client: &mut Client,
    rec: Recording,
    (status, reason): (u16, &str),
    detail: String,
    close: bool,
) -> Outcome {
    ctx.shared.counters.errors.fetch_add(1, Ordering::Relaxed);
    tracing::debug!(host = %rec.host, error = %detail, "exchange failed");
    let sent = client
        .send(&responses::error(status, reason, &detail, close))
        .await;
    ctx.shared.record(rec.finish(Some(detail)));
    if sent && !close {
        Outcome::KeepAlive
    } else {
        Outcome::Close
    }
}

/// Upstream security for an origin, per the current TLS policy.
pub(crate) fn security_for(ctx: &ConnCtx, origin: &Origin) -> Result<UpstreamSecurity, String> {
    match origin.scheme {
        Scheme::Http => Ok(UpstreamSecurity::Plain),
        Scheme::Https => {
            let tls = ctx.shared.upstream_tls().ok_or_else(|| {
                "HTTPS interception is unavailable (local CA not loaded)".to_string()
            })?;
            let verification = if ctx.shared.tls_policy().skips_verification(&origin.host) {
                UpstreamVerification::NotVerified
            } else {
                UpstreamVerification::Verified
            };
            Ok(UpstreamSecurity::Tls { tls, verification })
        }
    }
}

/// Largest message that can be paused for editing; larger ones are
/// forwarded unmodified (with a note) so memory stays bounded.
pub(crate) const HOLD_LIMIT: usize = 8 * 1024 * 1024;

/// What is sent upstream.
enum Payload {
    /// Head, then `prefix` (body bytes already read), then the rest of the
    /// body streamed from the client.
    Stream { prefix: Vec<u8> },
    /// The complete request (head and body), e.g. after interception.
    Buffered(Vec<u8>),
}

fn held_summary(
    kind: HeldKind,
    rec: &Recording,
    method: &str,
    status: Option<u16>,
    size: usize,
) -> HeldSummary {
    HeldSummary {
        id: 0,
        kind,
        held_at: now_ms(),
        method: method.to_string(),
        url: format_url(rec.scheme, &rec.host, rec.port, &rec.path),
        host: rec.host.clone(),
        status,
        size: size as u64,
        ws_opcode: None,
    }
}

enum Wait {
    /// The user decided.
    Decided(Decision),
    /// Not paused (Intercept off or queue full) or released by the queue.
    Pass,
    /// The client closed its connection while the message was paused.
    ClientGone,
}

/// Resolves when the client closes its connection. Data sent meanwhile
/// (pipelining) is left untouched and simply ends the watch.
async fn client_closed(client: &mut Client) {
    use tokio::io::AsyncBufReadExt;
    match client.reader.fill_buf().await {
        Ok([]) | Err(_) => {}
        Ok(_) => std::future::pending::<()>().await,
    }
}

/// Pauses `message` and waits for the user's decision.
async fn await_decision(ctx: &ConnCtx, client: &mut Client, message: HeldMessage) -> Wait {
    let queue = ctx.shared.intercept();
    let Some((id, rx)) = queue.hold(message) else {
        return Wait::Pass;
    };
    tokio::select! {
        decision = rx => match decision {
            Ok(d) => Wait::Decided(d),
            Err(_) => Wait::Pass,
        },
        _ = client_closed(client) => {
            queue.abandon(id);
            Wait::ClientGone
        }
    }
}

/// Updates the recorded method/path after the user edited the request line.
fn adopt_edited_request_line(rec: &mut Recording, raw: &[u8]) -> Option<String> {
    let end = chosproxy_http::head::find_head_end(raw)?;
    let head = RequestHead::parse(raw[..end].to_vec()).ok()?;
    if head.target.starts_with('/') {
        rec.path = head.target.clone();
    }
    rec.method = head.method.clone();
    Some(head.method)
}

/// Reads a whole body (up to `limit` bytes). Returns the bytes and whether
/// the body ended within the limit.
async fn read_body_limited<R>(
    body: &mut BodyReader,
    reader: &mut R,
    limit: usize,
) -> Result<(Vec<u8>, bool), String>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut buf = Vec::new();
    loop {
        match tokio::time::timeout(body_idle(), body.next_chunk(reader)).await {
            Err(_) => return Err("the body stopped arriving".into()),
            Ok(Err(e)) => return Err(e.to_string()),
            Ok(Ok(None)) => return Ok((buf, true)),
            Ok(Ok(Some(piece))) => {
                buf.extend_from_slice(&piece);
                if buf.len() > limit {
                    return Ok((buf, false));
                }
            }
        }
    }
}

/// Relays one request to `origin` and the response back, capturing both.
/// When Intercept is on and the rules match, the request and/or response is
/// paused for the user to forward (possibly edited) or drop.
pub(crate) async fn relay(
    ctx: &ConnCtx,
    client: &mut Client,
    upstream: &mut Option<Upstream>,
    head: RequestHead,
    origin: &Origin,
    path: String,
) -> Outcome {
    let (host, port) = (origin.host.clone(), origin.port);
    let limits = ctx.shared.limits();
    let mut rec = Recording {
        started: Instant::now(),
        started_at: now_ms(),
        listener: ctx.listener_id.clone(),
        client_addr: ctx.peer,
        scheme: origin.scheme,
        method: head.method.clone(),
        host: host.clone(),
        port,
        path: path.clone(),
        version: String::from_utf8_lossy(head.version_token()).into_owned(),
        request: Capture::new(limits.max_message_bytes),
        response: None,
        response_head: None,
        original_request: None,
        original_response: None,
        server_ip: None,
        notes: Vec::new(),
        flags: 0,
        ws_key: None,
    };

    let framing = match request_framing(&head) {
        Ok(f) => f,
        Err(e) => {
            rec.request.push(&head.raw);
            return fail(ctx, client, rec, (400, "Bad Request"), e.to_string(), true).await;
        }
    };
    if framing.cl_te_conflict {
        rec.flags |= flags::CL_TE_CONFLICT;
        rec.notes.push(
            "Request had both Content-Length and Transfer-Encoding; Transfer-Encoding was used."
                .into(),
        );
    }
    let has_body = framing.kind != BodyKind::Empty;
    let expect_continue = has_body
        && head
            .header_tokens("expect")
            .iter()
            .any(|t| t == "100-continue");

    // Forwarded head: origin-form request line, proxy-only headers removed.
    let mut request_line = Vec::with_capacity(head.method.len() + path.len() + 12);
    request_line.extend_from_slice(head.method.as_bytes());
    request_line.push(b' ');
    request_line.extend_from_slice(path.as_bytes());
    request_line.push(b' ');
    request_line.extend_from_slice(head.version_token());
    let mut drop_headers: Vec<&str> = PROXY_HEADERS.to_vec();
    if expect_continue {
        // We answer 100-continue ourselves; the body follows immediately.
        drop_headers.push("Expect");
    }
    let websocket = head
        .header_tokens("upgrade")
        .iter()
        .any(|t| t == "websocket");
    if websocket && head.header("sec-websocket-extensions").is_some() {
        // Without extensions (compression) messages stay readable and editable.
        drop_headers.push("Sec-WebSocket-Extensions");
        rec.notes.push(
            "WebSocket extensions (e.g. compression) were removed from the handshake.".into(),
        );
    }
    let mut forwarded_head = rewrite_head(&head.raw, Some(&request_line), &drop_headers);
    rec.request.push(&forwarded_head);

    let extension = extension_of(&path);
    let queue = ctx.shared.intercept();
    let rules = queue.rules();
    let in_scope = rules.only_in_scope
        && queue.scope().contains(&ScopeTarget {
            scheme: origin.scheme.as_str(),
            host: &host,
            port,
            path: &path,
        });
    let url_for_rules = format_url(origin.scheme, &host, port, &path);
    let facts = RequestFacts {
        host: &host,
        method: &head.method,
        extension: extension.as_deref(),
        has_query: path.contains('?'),
        has_body,
        in_scope,
        url: &url_for_rules,
    };
    let client_close = has_body || !head.keep_alive();
    let mut body = BodyReader::new(framing.kind);
    let mut continue_sent = false;
    let mut response_method = head.method.clone();
    let mut force_response_hold = false;
    let mut payload = Payload::Stream { prefix: Vec::new() };

    // ---- Match & Replace on the request ---------------------------------
    let mr = ctx.shared.match_replace();
    // The complete request after the rules, when its body had to be read.
    let mut prepared: Option<Vec<u8>> = None;
    if mr.has(Side::Request) {
        let limit = HOLD_LIMIT.min(limits.max_message_bytes);
        if mr.has_body(Side::Request) && has_body {
            if expect_continue {
                if !client.send(b"HTTP/1.1 100 Continue\r\n\r\n").await {
                    return Outcome::Close;
                }
                continue_sent = true;
            }
            let (body_bytes, complete) =
                match read_body_limited(&mut body, &mut client.reader, limit).await {
                    Ok(r) => r,
                    Err(e) => {
                        let msg = format!("invalid request body: {e}");
                        return fail(ctx, client, rec, (400, "Bad Request"), msg, true).await;
                    }
                };
            rec.request.push(&body_bytes);
            if complete {
                let mut whole = forwarded_head.clone();
                whole.extend_from_slice(&body_bytes);
                if let Some(applied) = mr.apply(Side::Request, &whole, limit) {
                    if let Some(m) = rules_changed_request(&mut rec, &applied, limits) {
                        response_method = m;
                    }
                    whole = applied.message;
                }
                prepared = Some(whole);
            } else {
                rec.notes.push(format!(
                    "Request larger than {limit} bytes: Match & Replace body rules not applied."
                ));
                if let Some(applied) = mr.apply_head(Side::Request, &forwarded_head) {
                    if let Some(m) = rules_changed_request(&mut rec, &applied, limits) {
                        response_method = m;
                    }
                    forwarded_head = applied.message;
                }
                payload = Payload::Stream { prefix: body_bytes };
            }
        } else if let Some(applied) = mr.apply_head(Side::Request, &forwarded_head) {
            if let Some(m) = rules_changed_request(&mut rec, &applied, limits) {
                response_method = m;
            }
            forwarded_head = applied.message;
        }
    }

    // ---- Request hooks (plugins) ------------------------------------------
    if let Some(hook) = ctx.shared.request_hook() {
        let target = HookTarget {
            scheme: origin.scheme.as_str(),
            host: &host,
            port,
            path: &path,
        };
        let streaming = matches!(&payload, Payload::Stream { prefix } if !prefix.is_empty());
        if !websocket && !streaming && hook.wants(&target) {
            if prepared.is_none() {
                if expect_continue && !continue_sent {
                    if !client.send(b"HTTP/1.1 100 Continue\r\n\r\n").await {
                        return Outcome::Close;
                    }
                    continue_sent = true;
                }
                let limit = HOLD_LIMIT.min(limits.max_message_bytes);
                let (body_bytes, complete) =
                    match read_body_limited(&mut body, &mut client.reader, limit).await {
                        Ok(r) => r,
                        Err(e) => {
                            let msg = format!("invalid request body: {e}");
                            return fail(ctx, client, rec, (400, "Bad Request"), msg, true).await;
                        }
                    };
                rec.request.push(&body_bytes);
                if complete {
                    let mut whole = forwarded_head.clone();
                    whole.extend_from_slice(&body_bytes);
                    prepared = Some(whole);
                } else {
                    rec.notes.push(format!(
                        "Request larger than {limit} bytes: plugin hooks not applied."
                    ));
                    payload = Payload::Stream { prefix: body_bytes };
                }
            }
            if let Some(whole) = prepared.take() {
                let outcome = hook.on_request(&target, whole.clone()).await;
                rec.notes.extend(outcome.notes);
                match outcome.verdict {
                    HookVerdict::Continue => prepared = Some(whole),
                    HookVerdict::Replace(raw) => {
                        if raw != whole {
                            let mut edited = Capture::new(limits.max_message_bytes);
                            edited.push(&raw);
                            let before = std::mem::replace(&mut rec.request, edited);
                            if rec.original_request.is_none() {
                                rec.original_request = Some(before.into_bytes());
                            }
                            rec.flags |= flags::EDITED_REQUEST;
                            if let Some(m) = adopt_edited_request_line(&mut rec, &raw) {
                                response_method = m;
                            }
                        }
                        prepared = Some(raw);
                    }
                    HookVerdict::Block(reason) => {
                        rec.flags |= flags::DROPPED;
                        ctx.shared.record(rec.finish(Some(reason.clone())));
                        let close = !head.keep_alive();
                        let sent = client
                            .send(&responses::error(403, "Forbidden", &reason, close))
                            .await;
                        return if sent && !close {
                            Outcome::KeepAlive
                        } else {
                            Outcome::Close
                        };
                    }
                    HookVerdict::Respond(response) => {
                        let end = chosproxy_http::head::find_head_end(&response);
                        rec.response_head =
                            end.and_then(|end| ResponseHead::parse(response[..end].to_vec()).ok());
                        let mut captured = Capture::new(limits.max_message_bytes);
                        captured.push(&response);
                        rec.response = Some(captured);
                        let _ = client.send(&response).await;
                        ctx.shared.record(rec.finish(None));
                        return Outcome::Close;
                    }
                }
            }
        }
    }

    // ---- Intercept the request ------------------------------------------
    let streaming = matches!(&payload, Payload::Stream { prefix } if !prefix.is_empty());
    if queue.is_enabled() && rules.intercepts_request(&facts) && !streaming {
        if expect_continue && !continue_sent {
            if !client.send(b"HTTP/1.1 100 Continue\r\n\r\n").await {
                return Outcome::Close;
            }
            continue_sent = true;
        }
        let limit = HOLD_LIMIT.min(limits.max_message_bytes);
        let (body_bytes, complete) = if prepared.is_some() {
            (Vec::new(), true)
        } else {
            match read_body_limited(&mut body, &mut client.reader, limit).await {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!("invalid request body: {e}");
                    return fail(ctx, client, rec, (400, "Bad Request"), msg, true).await;
                }
            }
        };
        if prepared.is_none() {
            rec.request.push(&body_bytes);
        }
        if complete {
            let original = match prepared.take() {
                Some(p) => p,
                None => {
                    let mut o = forwarded_head.clone();
                    o.extend_from_slice(&body_bytes);
                    o
                }
            };
            let summary = held_summary(HeldKind::Request, &rec, &head.method, None, original.len());
            let message = HeldMessage {
                summary,
                raw: original.clone(),
                request: None,
            };
            match await_decision(ctx, client, message).await {
                Wait::ClientGone => {
                    ctx.shared.record(rec.finish(Some(
                        "client disconnected while the request was paused".into(),
                    )));
                    return Outcome::Close;
                }
                Wait::Decided(Decision::Drop) => {
                    rec.flags |= flags::DROPPED;
                    ctx.shared
                        .record(rec.finish(Some("request dropped by the user".into())));
                    let close = !head.keep_alive();
                    let sent = client
                        .send(&responses::error(
                            502,
                            "Bad Gateway",
                            "the request was dropped by the user in Intercept",
                            close,
                        ))
                        .await;
                    return if sent && !close {
                        Outcome::KeepAlive
                    } else {
                        Outcome::Close
                    };
                }
                Wait::Decided(Decision::Forward {
                    raw,
                    intercept_response,
                }) => {
                    force_response_hold = intercept_response;
                    if raw != original {
                        let mut edited = Capture::new(limits.max_message_bytes);
                        edited.push(&raw);
                        let before = std::mem::replace(&mut rec.request, edited);
                        if rec.original_request.is_none() {
                            rec.original_request = Some(before.into_bytes());
                        }
                        rec.flags |= flags::EDITED_REQUEST;
                        if let Some(m) = adopt_edited_request_line(&mut rec, &raw) {
                            response_method = m;
                        }
                    }
                    payload = Payload::Buffered(raw);
                }
                Wait::Pass => payload = Payload::Buffered(original),
            }
        } else {
            rec.notes.push(format!(
                "Request larger than {limit} bytes: not paused, forwarded unmodified."
            ));
            payload = Payload::Stream { prefix: body_bytes };
        }
    }
    if let Some(p) = prepared.take() {
        payload = Payload::Buffered(p);
    }

    // ---- Connect upstream ------------------------------------------------
    let security = match security_for(ctx, origin) {
        Ok(s) => s,
        Err(msg) => {
            let close = client_close && !matches!(payload, Payload::Buffered(_));
            return fail(ctx, client, rec, (502, "Bad Gateway"), msg, close).await;
        }
    };
    let body_consumed = matches!(payload, Payload::Buffered(_)) || !has_body;
    let mut idle = upstream.take();
    if let Some(u) = idle.as_mut()
        && !u.reusable_for(origin.scheme, &host, port).await
    {
        idle = None;
    }
    let (mut up, reused) = match idle {
        Some(u) => (u, true),
        None => match Upstream::connect(&host, port, &origin.tls_name, &security).await {
            Ok(u) => (u, false),
            Err(msg) => {
                return fail(ctx, client, rec, (502, "Bad Gateway"), msg, !body_consumed).await;
            }
        },
    };
    rec.server_ip = Some(up.peer.ip().to_string());
    if ctx
        .shared
        .is_self(&up.peer.ip().to_string(), up.peer.port())
    {
        let msg = format!(
            "{host} resolves to this proxy, which would loop; give it its real address in \
             Settings → Network → Host name overrides"
        );
        return fail(
            ctx,
            client,
            rec,
            (508, "Loop Detected"),
            msg,
            !body_consumed,
        )
        .await;
    }
    if let Some(via) = &up.via {
        rec.notes.push(format!("Sent through the upstream {via}."));
    }
    if up.verification == Some(UpstreamVerification::NotVerified) {
        rec.notes.push(
            "Upstream TLS certificate was not verified (host is in the verification exceptions)."
                .into(),
        );
    }

    if expect_continue && !continue_sent && !client.send(b"HTTP/1.1 100 Continue\r\n\r\n").await {
        return Outcome::Close;
    }

    // ---- Send the request ------------------------------------------------
    // Bytes that can be re-sent on a fresh connection if a reused one turns
    // out to be dead (only when nothing more must be read from the client).
    let replay: Option<Vec<u8>> = match &payload {
        Payload::Buffered(b) => Some(b.clone()),
        Payload::Stream { .. } if !has_body => Some(forwarded_head.clone()),
        Payload::Stream { .. } => None,
    };
    let first = match &payload {
        Payload::Buffered(b) => b.clone(),
        Payload::Stream { prefix } => {
            let mut v = forwarded_head.clone();
            v.extend_from_slice(prefix);
            v
        }
    };
    let first = up.outgoing(&first).into_owned();
    if let Err(e) = up.writer.write_all(&first).await {
        let msg = format!("cannot send request upstream: {e}");
        return fail(ctx, client, rec, (502, "Bad Gateway"), msg, !body_consumed).await;
    }
    if let Payload::Stream { .. } = payload {
        loop {
            let piece = match tokio::time::timeout(body_idle(), body.next_chunk(&mut client.reader))
                .await
            {
                Err(_) => {
                    let msg = "client stopped sending the request body".to_string();
                    return fail(ctx, client, rec, (408, "Request Timeout"), msg, true).await;
                }
                Ok(Err(e)) => {
                    let msg = format!("invalid request body: {e}");
                    return fail(ctx, client, rec, (400, "Bad Request"), msg, true).await;
                }
                Ok(Ok(None)) => break,
                Ok(Ok(Some(p))) => p,
            };
            rec.request.push(&piece);
            if let Err(e) = up.writer.write_all(&piece).await {
                let msg = format!("cannot send request body upstream: {e}");
                return fail(ctx, client, rec, (502, "Bad Gateway"), msg, true).await;
            }
        }
    }
    if let Err(e) = up.writer.flush().await {
        let msg = format!("cannot send request upstream: {e}");
        return fail(ctx, client, rec, (502, "Bad Gateway"), msg, true).await;
    }

    // ---- Response head (interim 1xx forwarded) ---------------------------
    let mut retried = false;
    let res_head = loop {
        match read_response_head(&mut up).await {
            Ok(h) if h.is_interim() => {
                if !client.send(&h.raw).await {
                    return Outcome::Close;
                }
            }
            Ok(h) => break h,
            // A reused keep-alive connection may have been closed by the
            // server just before we used it; re-send when that is safe.
            Err(HeadError::ClosedEarly) if reused && !retried && replay.is_some() => {
                retried = true;
                up = match Upstream::connect(&host, port, &origin.tls_name, &security).await {
                    Ok(u) => u,
                    Err(msg) => {
                        return fail(ctx, client, rec, (502, "Bad Gateway"), msg, false).await;
                    }
                };
                rec.server_ip = Some(up.peer.ip().to_string());
                let bytes = up
                    .outgoing(replay.as_deref().unwrap_or_default())
                    .into_owned();
                if let Err(e) = up.writer.write_all(&bytes).await {
                    let msg = format!("cannot send request upstream: {e}");
                    return fail(ctx, client, rec, (502, "Bad Gateway"), msg, false).await;
                }
            }
            Err(e) => {
                return fail(ctx, client, rec, e.status(), e.describe(), true).await;
            }
        }
    };

    let mut res_capture = Capture::new(limits.max_message_bytes);
    res_capture.push(&res_head.raw);

    if res_head.status == 101 {
        let sent = client.send(&res_head.raw).await;
        rec.flags |= flags::UPGRADED;
        let is_websocket = websocket
            && res_head
                .header_tokens("upgrade")
                .iter()
                .any(|t| t == "websocket");
        let ws = is_websocket.then(|| WsInfo {
            key: crate::websocket::new_key(),
            scheme: rec.scheme,
            host: rec.host.clone(),
            port: rec.port,
            path: rec.path.clone(),
            url: format_url(rec.scheme, &rec.host, rec.port, &rec.path),
        });
        if let Some(ws) = &ws {
            rec.ws_key = Some(ws.key);
            rec.notes
                .push("WebSocket connection: messages are captured in Proxy → WebSockets.".into());
        } else {
            rec.notes.push(
                "Connection upgraded to a non-WebSocket protocol; subsequent traffic is relayed \
                 but not captured."
                    .into(),
            );
        }
        rec.response = Some(res_capture);
        rec.response_head = Some(res_head);
        ctx.shared.record(rec.finish(None));
        return match (sent, ws) {
            (false, _) => Outcome::Close,
            (true, Some(info)) => Outcome::WebSocket(up, Box::new(info)),
            (true, None) => Outcome::Splice(up),
        };
    }

    let res_framing = match response_framing(&response_method, &res_head) {
        Ok(f) => f,
        Err(e) => {
            rec.response = Some(res_capture);
            let msg = format!("invalid upstream response: {e}");
            return fail(ctx, client, rec, (502, "Bad Gateway"), msg, true).await;
        }
    };
    let mut res_body = BodyReader::new(res_framing.kind);
    let mut client_keep = head.keep_alive() && res_head.keep_alive();

    // ---- Intercept the response ------------------------------------------
    let mime = classify_mime(res_head.header_str("content-type").as_deref(), &path, &[]);
    let mut already_sent = false;
    let mut error: Option<String> = None;
    let hold = queue.is_enabled() && rules.intercepts_response(&facts, mime, force_response_hold);
    // Plugins that asked for the responses of this target (in scope).
    let response_hook = ctx.shared.request_hook().filter(|hook| {
        hook.wants_response(&HookTarget {
            scheme: origin.scheme.as_str(),
            host: &host,
            port,
            path: &path,
        })
    });
    if hold || response_hook.is_some() || mr.has_body(Side::Response) {
        let limit = HOLD_LIMIT.min(limits.max_message_bytes);
        let (body_bytes, complete) =
            match read_body_limited(&mut res_body, &mut up.reader, limit).await {
                Ok(r) => r,
                Err(e) => {
                    rec.response = Some(res_capture);
                    let msg = format!("upstream response body: {e}");
                    return fail(ctx, client, rec, (502, "Bad Gateway"), msg, true).await;
                }
            };
        res_capture.push(&body_bytes);
        let mut original = res_head.raw.clone();
        original.extend_from_slice(&body_bytes);
        if complete {
            // Match & Replace first, so Intercept shows the result.
            let mut current = original.clone();
            if let Some(applied) = mr.apply(Side::Response, &original, limit) {
                if rules_changed_response(&mut rec, &mut res_capture, &applied, limits) {
                    client_keep = client_keep
                        && edited_response_keeps_alive(&applied.message, &response_method);
                }
                current = applied.message;
            }
            // What the capture holds now; later changes are compared to it.
            let base = current.clone();
            let mut blocked: Option<String> = None;
            if let Some(hook) = &response_hook {
                let target = HookTarget {
                    scheme: origin.scheme.as_str(),
                    host: &host,
                    port,
                    path: &path,
                };
                let outcome = hook
                    .on_response(&target, rec.request.bytes().to_vec(), current.clone())
                    .await;
                rec.notes.extend(outcome.notes);
                match outcome.verdict {
                    HookVerdict::Continue => {}
                    HookVerdict::Replace(raw) | HookVerdict::Respond(raw) => current = raw,
                    HookVerdict::Block(reason) => blocked = Some(reason),
                }
            }
            let summary = held_summary(
                HeldKind::Response,
                &rec,
                &rec.method,
                Some(res_head.status),
                current.len(),
            );
            let message = HeldMessage {
                summary,
                raw: current.clone(),
                request: Some(rec.request.bytes().to_vec()),
            };
            let decision = if blocked.is_some() {
                Wait::Decided(Decision::Drop)
            } else if hold {
                await_decision(ctx, client, message).await
            } else {
                Wait::Pass
            };
            let deliver = match decision {
                Wait::ClientGone => {
                    rec.response = Some(res_capture);
                    rec.response_head = Some(res_head);
                    ctx.shared.record(rec.finish(Some(
                        "client disconnected while the response was paused".into(),
                    )));
                    return Outcome::Close;
                }
                Wait::Decided(Decision::Drop) => {
                    // The body was read completely, so the upstream
                    // connection stays usable if the server allows it.
                    let keep_up = res_head.keep_alive() && res_framing.kind != BodyKind::UntilClose;
                    rec.flags |= flags::DROPPED;
                    rec.response = Some(res_capture);
                    rec.response_head = Some(res_head);
                    ctx.shared.record(
                        rec.finish(Some(
                            blocked
                                .clone()
                                .unwrap_or_else(|| "response dropped by the user".into()),
                        )),
                    );
                    let close = !client_keep;
                    let sent = client
                        .send(&responses::error(
                            502,
                            "Bad Gateway",
                            blocked
                                .as_deref()
                                .unwrap_or("the response was dropped by the user in Intercept"),
                            close,
                        ))
                        .await;
                    if keep_up {
                        *upstream = Some(up);
                    }
                    return if sent && !close {
                        Outcome::KeepAlive
                    } else {
                        Outcome::Close
                    };
                }
                Wait::Decided(Decision::Forward { raw, .. }) => raw,
                Wait::Pass => current.clone(),
            };
            if deliver != base {
                let mut edited = Capture::new(limits.max_message_bytes);
                edited.push(&deliver);
                let before = std::mem::replace(&mut res_capture, edited);
                if rec.original_response.is_none() {
                    rec.original_response = Some(before.into_bytes());
                }
                rec.flags |= flags::EDITED_RESPONSE;
                // The client frames the edited message by its own headers.
                client_keep =
                    client_keep && edited_response_keeps_alive(&deliver, &response_method);
            }
            if !client.send(&deliver).await {
                error = Some("client disconnected before the response was delivered".into());
            }
            already_sent = true;
        } else {
            rec.notes.push(format!(
                "Response larger than {limit} bytes: not paused or changed by rules, forwarded \
                 unmodified."
            ));
            if !client.send(&original).await {
                error = Some("client disconnected during the response".into());
            }
        }
    } else {
        // Head rules only: the body streams through unchanged.
        let mut head_bytes = res_head.raw.clone();
        if let Some(applied) = mr.apply_head(Side::Response, &res_head.raw)
            && rules_changed_response(&mut rec, &mut res_capture, &applied, limits)
        {
            head_bytes = applied.message;
        }
        if !client.send(&head_bytes).await {
            error = Some("client disconnected before the response was delivered".into());
        }
    }

    // ---- Stream the rest of the response ---------------------------------
    while error.is_none() && !already_sent {
        match tokio::time::timeout(body_idle(), res_body.next_chunk(&mut up.reader)).await {
            Err(_) => error = Some("upstream stopped sending the response body".into()),
            Ok(Err(e)) => error = Some(format!("upstream response body: {e}")),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(piece))) => {
                res_capture.push(&piece);
                if client.writer.write_all(&piece).await.is_err() {
                    error = Some("client disconnected during the response".into());
                }
            }
        }
    }
    if error.is_none() && client.writer.flush().await.is_err() {
        error = Some("client disconnected during the response".into());
    }

    let upstream_keep = error.is_none()
        && res_head.keep_alive()
        && res_framing.kind != BodyKind::UntilClose
        && res_body.is_done();
    let keep = upstream_keep && client_keep;
    rec.response = Some(res_capture);
    rec.response_head = Some(res_head);
    if error.is_some() {
        ctx.shared.counters.errors.fetch_add(1, Ordering::Relaxed);
    }
    ctx.shared.record(rec.finish(error));
    if upstream_keep {
        *upstream = Some(up);
    }
    if keep {
        Outcome::KeepAlive
    } else {
        Outcome::Close
    }
}

/// Records that Match & Replace changed the request (`applied`); returns
/// the new method when the request line changed.
fn rules_changed_request(
    rec: &mut Recording,
    applied: &Applied,
    limits: CaptureLimits,
) -> Option<String> {
    if let Some(why) = &applied.skipped {
        rec.notes.push(format!("Match & Replace: {why}."));
    }
    if applied.rules.is_empty() {
        return None;
    }
    let mut edited = Capture::new(limits.max_message_bytes);
    edited.push(&applied.message);
    let before = std::mem::replace(&mut rec.request, edited);
    if rec.original_request.is_none() {
        rec.original_request = Some(before.into_bytes());
    }
    rec.flags |= flags::EDITED_REQUEST;
    rec.notes.push(format!(
        "Request changed by Match & Replace: {}.",
        applied.rules.join(", ")
    ));
    adopt_edited_request_line(rec, &applied.message)
}

/// Records that Match & Replace changed the response; true if it did.
fn rules_changed_response(
    rec: &mut Recording,
    capture: &mut Capture,
    applied: &Applied,
    limits: CaptureLimits,
) -> bool {
    if let Some(why) = &applied.skipped {
        rec.notes.push(format!("Match & Replace: {why}."));
    }
    if applied.rules.is_empty() {
        return false;
    }
    let mut edited = Capture::new(limits.max_message_bytes);
    edited.push(&applied.message);
    let before = std::mem::replace(capture, edited);
    if rec.original_response.is_none() {
        rec.original_response = Some(before.into_bytes());
    }
    rec.flags |= flags::EDITED_RESPONSE;
    rec.notes.push(format!(
        "Response changed by Match & Replace: {}.",
        applied.rules.join(", ")
    ));
    true
}

/// Whether the client can keep its connection after an edited response:
/// the edited head must parse and delimit its body.
fn edited_response_keeps_alive(raw: &[u8], method: &str) -> bool {
    let Some(end) = chosproxy_http::head::find_head_end(raw) else {
        return false;
    };
    match ResponseHead::parse(raw[..end].to_vec()) {
        Ok(h) => {
            h.keep_alive()
                && response_framing(method, &h).is_ok_and(|f| f.kind != BodyKind::UntilClose)
        }
        Err(_) => false,
    }
}
