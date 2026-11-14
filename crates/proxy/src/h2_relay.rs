//! HTTP/2 between a client and the proxy (ADR-0031).
//!
//! When a listener offers HTTP/2 and the client picks it (ALPN `h2`) on an
//! intercepted TLS connection, every stream is handled here: the request is
//! read whole and turned into the text form (`GET /path HTTP/2`), then
//! changed by Match & Replace and paused by Intercept like any other
//! request. It goes to the server over one shared HTTP/2 connection, or over
//! HTTP/1.1 when the server does not offer HTTP/2. The response comes back
//! the same way, and both are stored in the history.

use crate::conn::{ConnCtx, Origin};
use crate::hold::{Decision, HeldKind, HeldMessage, HeldSummary};
use crate::hook::{HookTarget, HookVerdict};
use crate::match_replace::Side;
use crate::relay::{HOLD_LIMIT, mime_of, security_for};
use bytes::Bytes;
use chosproxy_core::config::RequestFacts;
use chosproxy_core::exchange::{CapturedExchange, flags};
use chosproxy_core::time::now_ms;
use chosproxy_core::{ScopeTarget, Source};
use chosproxy_http::content::{dechunk, extension_of, split_message};
use chosproxy_http::head::{Headers, find_head_end};
use chosproxy_http::target::format_url;
use chosproxy_http::{ResponseHead, Scheme};
use chosproxy_request::io::BoxIo;
use chosproxy_request::{Destination, H2Connection, SendLimits, Sent};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Largest response relayed over HTTP/2 to the client (it is read whole).
const MAX_RESPONSE: usize = 64 * 1024 * 1024;
/// Headers not sent to an HTTP/2 client.
const HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-connection",
    "transfer-encoding",
    "upgrade",
];

/// How requests reach the server.
enum Upstream {
    Unopened,
    Http2(Arc<H2Connection>),
    /// The server does not offer HTTP/2: each request goes as HTTP/1.1.
    Http1,
}

struct Shared {
    ctx: ConnCtx,
    origin: Origin,
    upstream: Mutex<Upstream>,
}

/// Serves an HTTP/2 client connection until it closes.
pub(crate) async fn serve(ctx: &ConnCtx, io: BoxIo, origin: Origin) {
    let mut connection = match h2::server::Builder::new()
        // Bounds the memory one client can hold (each stream is read whole).
        .max_concurrent_streams(32)
        .handshake::<_, Bytes>(io)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(error = %e, "HTTP/2 handshake with the client failed");
            return;
        }
    };
    let shared = Arc::new(Shared {
        ctx: ctx.clone(),
        origin,
        upstream: Mutex::new(Upstream::Unopened),
    });
    while let Some(next) = connection.accept().await {
        match next {
            Ok((request, respond)) => {
                let shared = shared.clone();
                tokio::spawn(async move { stream(&shared, request, respond).await });
            }
            Err(e) => {
                tracing::debug!(error = %e, "HTTP/2 client connection ended");
                break;
            }
        }
    }
}

/// Bytes kept for the history, up to `max`.
fn kept(bytes: &[u8], max: usize) -> (Vec<u8>, bool) {
    if bytes.len() > max {
        (bytes[..max].to_vec(), true)
    } else {
        (bytes.to_vec(), false)
    }
}

struct Record {
    started: Instant,
    started_at: i64,
    method: String,
    path: String,
    request: Vec<u8>,
    original_request: Option<Vec<u8>>,
    response: Option<Vec<u8>>,
    original_response: Option<Vec<u8>>,
    response_len: Option<u64>,
    server_ip: Option<String>,
    notes: Vec<String>,
    flags: i64,
}

impl Record {
    fn finish(self, s: &Shared, error: Option<String>) -> CapturedExchange {
        let max = s.ctx.shared.limits().max_message_bytes;
        let mut flags = self.flags;
        let (request, cut) = kept(&self.request, max);
        if cut {
            flags |= flags::REQUEST_TRUNCATED;
        }
        let head = self.response.as_ref().and_then(|r| {
            let end = find_head_end(r)?;
            ResponseHead::parse(r[..end].to_vec()).ok()
        });
        let (status, mime) = match (&head, &self.response) {
            (Some(h), Some(r)) => (
                Some(h.status),
                Some(mime_of(h, &self.path, split_message(r).1).to_string()),
            ),
            _ => (None, None),
        };
        let response = self.response.map(|r| {
            let (bytes, cut) = kept(&r, max);
            if cut {
                flags |= flags::RESPONSE_TRUNCATED;
            }
            bytes
        });
        let o = &s.origin;
        CapturedExchange {
            started_at: self.started_at,
            duration_ms: i64::try_from(self.started.elapsed().as_millis()).ok(),
            source: Source::Proxy,
            listener: Some(s.ctx.listener_id.to_string()),
            client_addr: Some(s.ctx.peer.to_string()),
            scheme: "https".into(),
            method: self.method,
            host: o.host.clone(),
            port: o.port,
            extension: extension_of(&self.path),
            url: format_url(Scheme::Https, &o.host, o.port, &self.path),
            path: self.path,
            http_version: "HTTP/2".into(),
            status,
            reason: head.as_ref().map(|h| h.reason.clone()),
            mime: mime.filter(|m| !m.is_empty()),
            request_len: self.request.len() as u64,
            request,
            response,
            response_len: self.response_len,
            original_request: self.original_request,
            original_response: self.original_response,
            server_ip: self.server_ip,
            tls: true,
            error,
            notes: (!self.notes.is_empty()).then(|| self.notes.join("\n")),
            flags,
            ws_key: None,
        }
    }
}

/// The request as text (`… HTTP/2`), with `host` from `:authority`.
fn request_text(parts: &http::request::Parts, origin: &Origin, body: &[u8]) -> Vec<u8> {
    let path = parts
        .uri
        .path_and_query()
        .map_or("/", |p| p.as_str())
        .to_string();
    let authority = parts.uri.authority().map_or_else(
        || {
            if origin.port == 443 {
                origin.host.clone()
            } else {
                format!("{}:{}", origin.host, origin.port)
            }
        },
        |a| a.as_str().to_string(),
    );
    let mut out = format!("{} {path} HTTP/2\r\nhost: {authority}\r\n", parts.method).into_bytes();
    let mut cookies = Vec::new();
    for (name, value) in &parts.headers {
        if name == http::header::COOKIE {
            cookies.push(String::from_utf8_lossy(value.as_bytes()).to_string());
            continue;
        }
        out.extend_from_slice(name.as_str().as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    if !cookies.is_empty() {
        // Separate cookie fields are joined for an HTTP/1 view (RFC 9113 §8.2.3).
        out.extend_from_slice(format!("cookie: {}\r\n", cookies.join("; ")).as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

/// The request line's method and target of a text request.
fn method_and_path(raw: &[u8]) -> (String, String) {
    let line = raw.split(|b| *b == b'\n').next().unwrap_or_default();
    let text = String::from_utf8_lossy(line);
    let mut parts = text.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    (method, path)
}

/// HTTP/1.1 text of an `HTTP/2` text request, for servers without HTTP/2.
fn as_http1(raw: &[u8]) -> Vec<u8> {
    let line_end = raw.iter().position(|b| *b == b'\n').unwrap_or(raw.len());
    let line = String::from_utf8_lossy(&raw[..line_end]);
    let line = line.trim_end_matches('\r');
    let fixed = match line.strip_suffix("HTTP/2") {
        Some(start) => format!("{start}HTTP/1.1\r"),
        None => format!("{line}\r"),
    };
    let mut out = fixed.into_bytes();
    out.extend_from_slice(&raw[line_end..]);
    chosproxy_http::edit::update_content_length(&out, true)
}

/// A response as an HTTP/2 response head and body (chunked framing
/// removed, hop-by-hop headers dropped).
fn response_parts(raw: &[u8]) -> Result<(http::Response<()>, Vec<u8>), String> {
    let end = find_head_end(raw).ok_or("incomplete response head")?;
    let head = ResponseHead::parse(raw[..end].to_vec()).map_err(|e| e.to_string())?;
    let mut body = raw[end..].to_vec();
    if head
        .header_tokens("transfer-encoding")
        .last()
        .is_some_and(|t| t == "chunked")
    {
        body = dechunk(&body).map_err(|e| e.to_string())?.0;
    }
    let mut builder = http::Response::builder().status(head.status);
    for h in &head.headers {
        let lower = h.name.to_ascii_lowercase();
        if HOP_HEADERS.contains(&lower.as_str()) {
            continue;
        }
        let (Ok(name), Ok(value)) = (
            http::HeaderName::from_bytes(lower.as_bytes()),
            http::HeaderValue::from_bytes(&h.value),
        ) else {
            continue;
        };
        builder = builder.header(name, value);
    }
    let response = builder.body(()).map_err(|e| e.to_string())?;
    Ok((response, body))
}

fn plain_response(status: u16, message: &str) -> (http::Response<()>, Vec<u8>) {
    let mut response = http::Response::new(());
    *response.status_mut() =
        http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::BAD_GATEWAY);
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    (response, format!("ChosProxy: {message}\n").into_bytes())
}

async fn reply(
    respond: &mut h2::server::SendResponse<Bytes>,
    (response, body): (http::Response<()>, Vec<u8>),
) {
    match respond.send_response(response, body.is_empty()) {
        Ok(mut stream) if !body.is_empty() => {
            let _ = stream.send_data(Bytes::from(body), true);
        }
        Ok(_) => {}
        Err(e) => tracing::debug!(error = %e, "could not answer the HTTP/2 client"),
    }
}

enum Wait {
    Decided(Decision),
    Pass,
    ClientGone,
}

/// Pauses `message` in Intercept until the user decides, or the client
/// cancels the stream.
async fn wait_for_user(
    s: &Shared,
    respond: &mut h2::server::SendResponse<Bytes>,
    message: HeldMessage,
) -> Wait {
    let queue = s.ctx.shared.intercept();
    let Some((id, rx)) = queue.hold(message) else {
        return Wait::Pass;
    };
    let reset = std::future::poll_fn(|cx| respond.poll_reset(cx));
    tokio::select! {
        decision = rx => match decision {
            Ok(d) => Wait::Decided(d),
            Err(_) => Wait::Pass,
        },
        _ = reset => {
            queue.abandon(id);
            Wait::ClientGone
        }
    }
}

fn summary(
    s: &Shared,
    kind: HeldKind,
    rec: &Record,
    status: Option<u16>,
    size: usize,
) -> HeldSummary {
    HeldSummary {
        id: 0,
        kind,
        held_at: now_ms(),
        method: rec.method.clone(),
        url: format_url(Scheme::Https, &s.origin.host, s.origin.port, &rec.path),
        host: s.origin.host.clone(),
        status,
        size: size as u64,
        ws_opcode: None,
    }
}

async fn read_body(body: &mut h2::RecvStream, limit: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(120), body.data()).await {
            Err(_) => return Err("the client stopped sending the body".into()),
            Ok(None) => return Ok(out),
            Ok(Some(Err(e))) => return Err(format!("request body: {e}")),
            Ok(Some(Ok(chunk))) => {
                let _ = body.flow_control().release_capacity(chunk.len());
                out.extend_from_slice(&chunk);
                if out.len() > limit {
                    return Err(format!(
                        "request bodies over {limit} bytes are not relayed over HTTP/2"
                    ));
                }
            }
        }
    }
}

/// Sends the request to the server: HTTP/2 when it offers it, else HTTP/1.1.
async fn forward(s: &Shared, request: &[u8], rec: &mut Record) -> Result<Sent, String> {
    let limits = SendLimits {
        max_response_bytes: MAX_RESPONSE,
        ..SendLimits::default()
    };
    let destination = Destination {
        host: s.origin.host.clone(),
        port: s.origin.port,
        tls_name: s.origin.tls_name.clone(),
    };
    let security = security_for(&s.ctx, &s.origin)?;
    let connection = {
        let mut up = s.upstream.lock().await;
        if matches!(*up, Upstream::Unopened) {
            *up = match H2Connection::open(&destination, &security).await? {
                Some(c) => Upstream::Http2(Arc::new(c)),
                None => Upstream::Http1,
            };
        }
        match &*up {
            Upstream::Http2(c) => Some(c.clone()),
            _ => None,
        }
    };
    let sent = match connection {
        Some(c) => {
            let result = c.send(request, &limits).await;
            if result.is_err() {
                // A broken connection is replaced for the next request.
                *s.upstream.lock().await = Upstream::Unopened;
            }
            result
        }
        None => {
            rec.notes
                .push("The server does not offer HTTP/2: sent to it as HTTP/1.1.".into());
            chosproxy_request::send(&destination, &security, &as_http1(request), &limits).await
        }
    };
    sent.map_err(|e| e.message)
}

async fn stream(
    s: &Shared,
    request: http::Request<h2::RecvStream>,
    mut respond: h2::server::SendResponse<Bytes>,
) {
    let ctx = &s.ctx;
    let (parts, mut body) = request.into_parts();
    let started = Instant::now();
    let path = parts
        .uri
        .path_and_query()
        .map_or("/", |p| p.as_str())
        .to_string();
    let mut rec = Record {
        started,
        started_at: now_ms(),
        method: parts.method.to_string(),
        path: path.clone(),
        request: Vec::new(),
        original_request: None,
        response: None,
        original_response: None,
        response_len: None,
        server_ip: None,
        notes: vec!["HTTP/2 between the client and ChosProxy.".into()],
        flags: 0,
    };
    let body = match read_body(&mut body, HOLD_LIMIT).await {
        Ok(b) => b,
        Err(e) => {
            rec.request = request_text(&parts, &s.origin, &[]);
            reply(&mut respond, plain_response(413, &e)).await;
            ctx.shared.record(rec.finish(s, Some(e)));
            return;
        }
    };
    let mut current = request_text(&parts, &s.origin, &body);
    rec.request = current.clone();

    // Match & Replace, then Intercept.
    let mr = ctx.shared.match_replace();
    if let Some(applied) = mr.apply(Side::Request, &current, HOLD_LIMIT) {
        if let Some(why) = &applied.skipped {
            rec.notes.push(format!("Match & Replace: {why}."));
        }
        if !applied.rules.is_empty() {
            rec.original_request = Some(current.clone());
            rec.flags |= flags::EDITED_REQUEST;
            rec.notes.push(format!(
                "Request changed by Match & Replace: {}.",
                applied.rules.join(", ")
            ));
            current = applied.message;
            rec.request = current.clone();
        }
    }
    // Request hooks (plugins), like the HTTP/1 relay: after Match & Replace
    // and before Intercept.
    let hook = ctx.shared.request_hook();
    let hook_target = HookTarget {
        scheme: "https",
        host: &s.origin.host,
        port: s.origin.port,
        path: &path,
    };
    if let Some(hook) = hook.as_ref().filter(|h| h.wants(&hook_target)) {
        let outcome = hook.on_request(&hook_target, current.clone()).await;
        rec.notes.extend(outcome.notes);
        match outcome.verdict {
            HookVerdict::Continue => {}
            HookVerdict::Replace(raw) => {
                if raw != current {
                    if rec.original_request.is_none() {
                        rec.original_request = Some(current.clone());
                    }
                    rec.flags |= flags::EDITED_REQUEST;
                    current = raw;
                    rec.request = current.clone();
                    let (method, path) = method_and_path(&current);
                    rec.method = method;
                    rec.path = path;
                }
            }
            HookVerdict::Block(reason) => {
                rec.flags |= flags::DROPPED;
                reply(&mut respond, plain_response(403, &reason)).await;
                ctx.shared.record(rec.finish(s, Some(reason)));
                return;
            }
            HookVerdict::Respond(response) => {
                match response_parts(&response) {
                    Ok(parts) => reply(&mut respond, parts).await,
                    Err(e) => {
                        rec.notes.push(format!(
                            "The plugin's response cannot be sent over HTTP/2: {e}."
                        ));
                        reply(&mut respond, plain_response(502, &e)).await;
                    }
                }
                rec.response = Some(response);
                ctx.shared.record(rec.finish(s, None));
                return;
            }
        }
    }
    let queue = ctx.shared.intercept();
    let rules = queue.rules();
    let in_scope = rules.only_in_scope
        && queue.scope().contains(&ScopeTarget {
            scheme: "https",
            host: &s.origin.host,
            port: s.origin.port,
            path: &path,
        });
    let extension = extension_of(&path);
    let original_method = rec.method.clone();
    let url_for_rules = format_url(Scheme::Https, &s.origin.host, s.origin.port, &path);
    let facts = RequestFacts {
        host: &s.origin.host,
        method: &original_method,
        extension: extension.as_deref(),
        has_query: path.contains('?'),
        has_body: !body.is_empty(),
        in_scope,
        url: &url_for_rules,
    };
    let mut force_response = false;
    if queue.is_enabled() && rules.intercepts_request(&facts) {
        let message = HeldMessage {
            summary: summary(s, HeldKind::Request, &rec, None, current.len()),
            raw: current.clone(),
            request: None,
        };
        match wait_for_user(s, &mut respond, message).await {
            Wait::ClientGone => {
                ctx.shared.record(rec.finish(
                    s,
                    Some("the client cancelled the request while it was paused".into()),
                ));
                return;
            }
            Wait::Decided(Decision::Drop) => {
                rec.flags |= flags::DROPPED;
                reply(
                    &mut respond,
                    plain_response(502, "the request was dropped by the user in Intercept"),
                )
                .await;
                ctx.shared
                    .record(rec.finish(s, Some("request dropped by the user".into())));
                return;
            }
            Wait::Decided(Decision::Forward {
                raw,
                intercept_response,
            }) => {
                force_response = intercept_response;
                if raw != current {
                    if rec.original_request.is_none() {
                        rec.original_request = Some(current.clone());
                    }
                    rec.flags |= flags::EDITED_REQUEST;
                    current = raw;
                    rec.request = current.clone();
                    let (method, path) = method_and_path(&current);
                    rec.method = method;
                    rec.path = path;
                }
            }
            Wait::Pass => {}
        }
    }

    // To the server.
    let sent = match forward(s, &current, &mut rec).await {
        Ok(sent) => sent,
        Err(e) => {
            ctx.shared.counters.errors.fetch_add(1, Ordering::Relaxed);
            reply(&mut respond, plain_response(502, &e)).await;
            ctx.shared.record(rec.finish(s, Some(e)));
            return;
        }
    };
    rec.server_ip = Some(sent.peer.ip().to_string());
    rec.response_len = Some(sent.response_len);
    if sent.truncated {
        let msg = format!("responses over {MAX_RESPONSE} bytes are not relayed over HTTP/2");
        rec.response = Some(sent.response);
        reply(&mut respond, plain_response(502, &msg)).await;
        ctx.shared.record(rec.finish(s, Some(msg)));
        return;
    }
    let mut response = sent.response;

    // Match & Replace and Intercept on the response.
    if let Some(applied) = mr.apply(Side::Response, &response, MAX_RESPONSE) {
        if let Some(why) = &applied.skipped {
            rec.notes.push(format!("Match & Replace: {why}."));
        }
        if !applied.rules.is_empty() {
            rec.original_response = Some(response.clone());
            rec.flags |= flags::EDITED_RESPONSE;
            rec.notes.push(format!(
                "Response changed by Match & Replace: {}.",
                applied.rules.join(", ")
            ));
            response = applied.message;
        }
    }
    if let Some(hook) = hook.as_ref().filter(|h| h.wants_response(&hook_target))
        && response.len() <= HOLD_LIMIT
    {
        let outcome = hook
            .on_response(&hook_target, rec.request.clone(), response.clone())
            .await;
        rec.notes.extend(outcome.notes);
        match outcome.verdict {
            HookVerdict::Continue => {}
            HookVerdict::Replace(raw) | HookVerdict::Respond(raw) => {
                if raw != response {
                    if rec.original_response.is_none() {
                        rec.original_response = Some(response.clone());
                    }
                    rec.flags |= flags::EDITED_RESPONSE;
                    response = raw;
                }
            }
            HookVerdict::Block(reason) => {
                rec.flags |= flags::DROPPED;
                rec.response = Some(response);
                reply(&mut respond, plain_response(502, &reason)).await;
                ctx.shared.record(rec.finish(s, Some(reason)));
                return;
            }
        }
    }
    let mime = mime_of(&sent.head, &rec.path, split_message(&response).1);
    if queue.is_enabled()
        && rules.intercepts_response(&facts, mime, force_response)
        && response.len() <= HOLD_LIMIT
    {
        let message = HeldMessage {
            summary: summary(
                s,
                HeldKind::Response,
                &rec,
                Some(sent.head.status),
                response.len(),
            ),
            raw: response.clone(),
            request: Some(rec.request.clone()),
        };
        match wait_for_user(s, &mut respond, message).await {
            Wait::ClientGone => {
                rec.response = Some(response);
                ctx.shared.record(rec.finish(
                    s,
                    Some("the client cancelled the request while the response was paused".into()),
                ));
                return;
            }
            Wait::Decided(Decision::Drop) => {
                rec.flags |= flags::DROPPED;
                rec.response = Some(response);
                reply(
                    &mut respond,
                    plain_response(502, "the response was dropped by the user in Intercept"),
                )
                .await;
                ctx.shared
                    .record(rec.finish(s, Some("response dropped by the user".into())));
                return;
            }
            Wait::Decided(Decision::Forward { raw, .. }) => {
                if raw != response {
                    if rec.original_response.is_none() {
                        rec.original_response = Some(response.clone());
                    }
                    rec.flags |= flags::EDITED_RESPONSE;
                    response = raw;
                }
            }
            Wait::Pass => {}
        }
    }

    // To the client.
    match response_parts(&response) {
        Ok(parts) => {
            let error = if rec.method == "HEAD" {
                None
            } else {
                sent.incomplete.clone()
            };
            reply(&mut respond, parts).await;
            rec.response = Some(response);
            ctx.shared.record(rec.finish(s, error));
        }
        Err(e) => {
            let msg = format!("the response cannot be sent over HTTP/2: {e}");
            reply(&mut respond, plain_response(502, &msg)).await;
            rec.response = Some(response);
            ctx.shared.record(rec.finish(s, Some(msg)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_become_text_and_back() {
        let req = http::Request::builder()
            .method("POST")
            .uri("https://lab.test/a?b=1")
            .header("cookie", "a=1")
            .header("cookie", "b=2")
            .header("x-test", "y")
            .body(())
            .unwrap();
        let (parts, ()) = req.into_parts();
        let origin = Origin {
            scheme: Scheme::Https,
            host: "lab.test".into(),
            port: 443,
            tls_name: "lab.test".into(),
        };
        let text = request_text(&parts, &origin, b"hi");
        assert_eq!(
            String::from_utf8_lossy(&text),
            "POST /a?b=1 HTTP/2\r\nhost: lab.test\r\nx-test: y\r\ncookie: a=1; b=2\r\n\r\nhi"
        );
        assert_eq!(method_and_path(&text), ("POST".into(), "/a?b=1".into()));
        assert_eq!(
            String::from_utf8_lossy(&as_http1(&text)),
            "POST /a?b=1 HTTP/1.1\r\nhost: lab.test\r\nx-test: y\r\ncookie: a=1; b=2\r\nContent-Length: 2\r\n\r\nhi"
        );
    }

    #[test]
    fn responses_lose_hop_headers_and_chunking() {
        let raw = b"HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nTransfer-Encoding: chunked\r\nX-A: 1\r\n\r\n2\r\nok\r\n0\r\n\r\n";
        let (res, body) = response_parts(raw).unwrap();
        assert_eq!(res.status(), 200);
        assert!(res.headers().get("connection").is_none());
        assert!(res.headers().get("transfer-encoding").is_none());
        assert_eq!(res.headers()["x-a"], "1");
        assert_eq!(body, b"ok");
    }
}
