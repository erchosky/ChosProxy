//! Repeater helpers that need no engine state: turning the editor text into
//! the bytes to send, describing a send as a captured exchange, and building
//! the request that follows a redirect.

use crate::AppError;
use chosproxy_core::Source;
use chosproxy_core::exchange::{CapturedExchange, flags};
use chosproxy_http::Scheme;
use chosproxy_http::content::{
    classify_mime, dechunk, decode_content, extension_of, split_message,
};
use chosproxy_http::edit::{self, Eol, NotEditable};
use chosproxy_http::head::Headers;
use chosproxy_http::target::{format_url, parse_authority};
use chosproxy_request::{SendError, Sent, request_method};
use chosproxy_storage::{RepeaterTarget, request_path};
use serde::Serialize;
use ts_rs::TS;

/// Largest body decoded when a binary (compressed or chunked) request is
/// opened in Repeater.
const UNPACK_LIMIT: usize = 8 * 1024 * 1024;

/// A request ready for the Repeater editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditorText {
    pub(crate) text: String,
    pub(crate) head_crlf: bool,
    pub(crate) body_crlf: bool,
}

/// The request that follows a redirect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeaterRedirect {
    /// Where to send it.
    pub target: RepeaterTarget,
    /// Request text.
    pub request: String,
    /// What changed, for the user (e.g. "POST became GET").
    pub notes: Vec<String>,
}

fn eol(crlf: bool) -> Eol {
    if crlf { Eol::CrLf } else { Eol::Lf }
}

pub(crate) fn scheme_of(target: &RepeaterTarget) -> Scheme {
    if target.scheme == "https" {
        Scheme::Https
    } else {
        Scheme::Http
    }
}

/// Editable text of raw request bytes. Compressed or chunked bodies are
/// unpacked first; other binary requests cannot be edited.
pub(crate) fn editor_text(raw: &[u8]) -> Result<EditorText, AppError> {
    let form = match edit::to_text(raw) {
        Ok(form) => form,
        Err(NotEditable::Binary) if edit::is_packed(raw) => {
            let unpacked = edit::unpack(raw, UNPACK_LIMIT)
                .map_err(|e| AppError::Invalid(format!("cannot unpack the body: {e}")))?;
            edit::to_text(&unpacked).map_err(|e| {
                AppError::Invalid(format!("this request cannot be edited as text: {e}"))
            })?
        }
        Err(e) => {
            return Err(AppError::Invalid(format!(
                "this request cannot be edited as text: {e}"
            )));
        }
    };
    Ok(EditorText {
        text: form.text,
        head_crlf: form.head_eol == Eol::CrLf,
        body_crlf: form.body_eol == Eol::CrLf,
    })
}

/// The bytes sent for the editor text.
pub(crate) fn request_bytes(
    text: &str,
    head_crlf: bool,
    body_crlf: bool,
    update_content_length: bool,
) -> Result<Vec<u8>, AppError> {
    if text.trim().is_empty() {
        return Err(AppError::Invalid("the request is empty".into()));
    }
    let raw = edit::from_text(text, eol(head_crlf), eol(body_crlf));
    Ok(if update_content_length {
        edit::update_content_length(&raw, true)
    } else {
        raw
    })
}

/// MIME category; sniffs the body only when the server sent no Content-Type.
fn mime_of(sent: &Sent, path: &str) -> &'static str {
    let content_type = sent.head.header_str("content-type");
    if content_type.is_some() {
        return classify_mime(content_type.as_deref(), path, &[]);
    }
    let raw_body = split_message(&sent.response).1;
    let chunked = sent
        .head
        .header_tokens("transfer-encoding")
        .last()
        .is_some_and(|t| t == "chunked");
    let mut body = if chunked {
        dechunk(raw_body).map(|(b, _)| b).unwrap_or_default()
    } else {
        raw_body.to_vec()
    };
    if let Some(enc) = sent.head.header_str("content-encoding")
        && let Ok(decoded) = decode_content(&body, &enc, 1024 * 1024)
    {
        body = decoded;
    }
    classify_mime(None, path, &body)
}

/// The version token of a request line (`HTTP/1.1` if missing).
fn version_of(request: &[u8]) -> String {
    let line_end = request
        .iter()
        .position(|b| *b == b'\n')
        .unwrap_or(request.len());
    String::from_utf8_lossy(&request[..line_end])
        .split_whitespace()
        .nth(2)
        .map_or_else(|| "HTTP/1.1".to_string(), str::to_string)
}

/// A send as an exchange for the project history (`source` is the tool
/// that sent it: Repeater, the crawler…).
pub(crate) fn captured(
    source: Source,
    target: &RepeaterTarget,
    request: Vec<u8>,
    started_at: i64,
    outcome: &Result<Sent, SendError>,
) -> CapturedExchange {
    let scheme = scheme_of(target);
    let path = request_path(&String::from_utf8_lossy(&request));
    let mut exchange = CapturedExchange {
        started_at,
        duration_ms: None,
        source,
        listener: None,
        client_addr: None,
        scheme: scheme.as_str().to_string(),
        method: request_method(&request),
        host: target.host.clone(),
        port: target.port,
        extension: extension_of(&path),
        url: format_url(scheme, &target.host, target.port, &path),
        path,
        http_version: version_of(&request),
        status: None,
        reason: None,
        mime: None,
        request_len: request.len() as u64,
        request,
        response: None,
        response_len: None,
        original_request: None,
        original_response: None,
        server_ip: None,
        tls: scheme == Scheme::Https,
        error: None,
        notes: None,
        flags: 0,
        ws_key: None,
    };
    match outcome {
        Ok(sent) => {
            let mime = mime_of(sent, &exchange.path);
            exchange.duration_ms = i64::try_from(sent.timing.total_ms).ok();
            exchange.status = Some(sent.head.status);
            exchange.reason = Some(sent.head.reason.clone());
            exchange.mime = (!mime.is_empty()).then(|| mime.to_string());
            exchange.response = Some(sent.response.clone());
            exchange.response_len = Some(sent.response_len);
            exchange.server_ip = Some(sent.peer.ip().to_string());
            exchange.error = sent.incomplete.clone();
            if sent.truncated {
                exchange.flags |= flags::RESPONSE_TRUNCATED;
            }
            if sent.interim > 0 {
                exchange.notes = Some(format!(
                    "{} interim (1xx) response(s) received before the final one",
                    sent.interim
                ));
            }
        }
        Err(e) => {
            exchange.duration_ms = i64::try_from(e.timing.total_ms).ok();
            exchange.server_ip = e.peer.map(|p| p.ip().to_string());
            exchange.error = Some(e.message.clone());
        }
    }
    if let Ok(sent) = outcome {
        // Store what actually went out (credentials, session cookies…).
        if let Some(sent_request) = &sent.sent_request {
            exchange.request_len = sent_request.len() as u64;
            exchange.request = sent_request.clone();
        }
        if !sent.notes.is_empty() {
            let note = sent.notes.join(" ");
            exchange.notes = Some(match exchange.notes.take() {
                Some(n) => format!("{n} {note}"),
                None => note,
            });
        }
    }
    if chosproxy_http::head::request_is_h2(&exchange.request) {
        let destination = chosproxy_request::Destination {
            host: target.host.clone(),
            port: target.port,
            tls_name: target.host.clone(),
        };
        let mut notes = vec!["Sent over HTTP/2.".to_string()];
        notes.extend(chosproxy_request::translation_notes(
            &exchange.request,
            scheme == Scheme::Https,
            &destination,
        ));
        if let Some(n) = exchange.notes.take() {
            notes.insert(0, n);
        }
        exchange.notes = Some(notes.join(" "));
    }
    exchange
}

/// `host[:port]` for a Host header (port omitted when it is the default).
fn authority(scheme: Scheme, host: &str, port: u16) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    if port == scheme.default_port() {
        host
    } else {
        format!("{host}:{port}")
    }
}

/// Resolves a `Location` value against the URL that was requested.
/// Returns the new target and origin-form path.
fn resolve_location(
    from: &RepeaterTarget,
    from_path: &str,
    location: &str,
) -> Result<(RepeaterTarget, String), String> {
    let location = location.trim();
    let location = location.split('#').next().unwrap_or_default();
    if location.is_empty() {
        return Err("the Location header is empty".into());
    }
    let split_authority = |rest: &str| -> (String, String) {
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let path = &rest[end..];
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        (rest[..end].to_string(), path)
    };
    let lower = location.to_ascii_lowercase();
    let (scheme, authority_text, path) =
        if lower.starts_with("http://") || lower.starts_with("https://") {
            let scheme = if lower.starts_with("https") {
                "https"
            } else {
                "http"
            };
            let (a, p) = split_authority(&location[scheme.len() + 3..]);
            (scheme.to_string(), Some(a), p)
        } else if let Some(rest) = location.strip_prefix("//") {
            let (a, p) = split_authority(rest);
            (from.scheme.clone(), Some(a), p)
        } else if location.contains("://") {
            return Err(format!("unsupported redirect target {location:?}"));
        } else if location.starts_with('/') {
            (from.scheme.clone(), None, location.to_string())
        } else {
            let base = from_path.split('?').next().unwrap_or("/");
            let dir = &base[..base.rfind('/').map_or(0, |i| i + 1)];
            let dir = if dir.is_empty() { "/" } else { dir };
            (from.scheme.clone(), None, format!("{dir}{location}"))
        };
    let target = match authority_text {
        Some(a) => {
            let default = if scheme == "https" { 443 } else { 80 };
            let (host, port) = parse_authority(&a, Some(default))
                .map_err(|_| format!("invalid redirect host {a:?}"))?;
            RepeaterTarget { scheme, host, port }
        }
        None => from.clone(),
    };
    Ok((target, path))
}

/// The request a client would send next for a redirect response: the
/// `Location` resolved against the previous URL, the method changed as
/// browsers do (303, and 301/302 after POST, become GET without a body),
/// `Host` updated, and credentials (`Authorization`, `Cookie`) removed when
/// the redirect leaves the origin.
pub(crate) fn follow_redirect(
    request: &str,
    from: &RepeaterTarget,
    status: u16,
    location: &str,
) -> Result<RepeaterRedirect, String> {
    if !matches!(status, 301 | 302 | 303 | 307 | 308) {
        return Err(format!(
            "status {status} is not a redirect that can be followed"
        ));
    }
    let (head, body) = match request.find("\n\n") {
        Some(i) => (&request[..i], &request[i + 2..]),
        None => (request.trim_end_matches('\n'), ""),
    };
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut words = request_line.split_whitespace();
    let method = words.next().unwrap_or("GET").to_string();
    let old_target = words.next().unwrap_or("/");
    let version = words.next().unwrap_or("HTTP/1.1");
    let from_path = request_path(&format!("GET {old_target} HTTP/1.1"));
    let (target, path) = resolve_location(from, &from_path, location)?;

    let mut notes = Vec::new();
    let to_get = !method.eq_ignore_ascii_case("HEAD")
        && (status == 303 || (matches!(status, 301 | 302) && method.eq_ignore_ascii_case("POST")));
    let new_method = if to_get {
        if !method.eq_ignore_ascii_case("GET") {
            notes.push(format!("{method} became GET without a body ({status})"));
        }
        "GET".to_string()
    } else {
        method
    };
    let cross_origin = target != *from;
    let mut dropped: Vec<String> = Vec::new();
    let mut headers: Vec<String> = Vec::new();
    let mut host_set = false;
    for line in lines {
        let name = line.split(':').next().unwrap_or_default().trim();
        let lower = name.to_ascii_lowercase();
        if to_get
            && matches!(
                lower.as_str(),
                "content-length" | "content-type" | "transfer-encoding"
            )
        {
            continue;
        }
        if cross_origin && matches!(lower.as_str(), "authorization" | "cookie") {
            dropped.push(name.to_string());
            continue;
        }
        if lower == "host" {
            if !host_set {
                headers.push(format!(
                    "{name}: {}",
                    authority(scheme_of(&target), &target.host, target.port)
                ));
                host_set = true;
            }
            continue;
        }
        headers.push(line.to_string());
    }
    if !host_set {
        headers.insert(
            0,
            format!(
                "Host: {}",
                authority(scheme_of(&target), &target.host, target.port)
            ),
        );
    }
    if !dropped.is_empty() {
        notes.push(format!(
            "removed {} (the redirect leaves the origin)",
            dropped.join(", ")
        ));
    }
    let mut text = format!("{new_method} {path} {version}\n");
    for h in headers {
        text.push_str(&h);
        text.push('\n');
    }
    text.push('\n');
    if !to_get {
        text.push_str(body);
    }
    Ok(RepeaterRedirect {
        target,
        request: text,
        notes,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use chosproxy_http::ResponseHead;
    use chosproxy_request::SendTiming;

    fn lab() -> RepeaterTarget {
        RepeaterTarget {
            scheme: "http".into(),
            host: "lab.test".into(),
            port: 8080,
        }
    }

    #[test]
    fn editor_text_round_trip_and_unpacking() {
        let raw = b"POST /a HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\na\nb";
        let form = editor_text(raw).unwrap();
        assert!(form.head_crlf && !form.body_crlf);
        assert_eq!(
            request_bytes(&form.text, form.head_crlf, form.body_crlf, false).unwrap(),
            raw
        );

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, b"{\"a\":1}").unwrap();
        let mut packed =
            b"POST / HTTP/1.1\r\nContent-Encoding: gzip\r\nContent-Length: 0\r\n\r\n".to_vec();
        packed.extend(gz.finish().unwrap());
        let unpacked = editor_text(&packed).unwrap();
        assert!(unpacked.text.ends_with("{\"a\":1}"));
        assert!(!unpacked.text.contains("gzip"));

        assert!(editor_text(b"POST / HTTP/1.1\r\n\r\n\xff\xfe").is_err());
    }

    #[test]
    fn content_length_is_updated_and_added() {
        let sent = request_bytes(
            "POST /a HTTP/1.1\nContent-Length: 1\n\nhello",
            true,
            true,
            true,
        )
        .unwrap();
        assert!(
            String::from_utf8(sent)
                .unwrap()
                .contains("Content-Length: 5\r\n")
        );
        let added = request_bytes("POST /a HTTP/1.1\nHost: x\n\nab", true, true, true).unwrap();
        assert!(
            String::from_utf8(added)
                .unwrap()
                .contains("Content-Length: 2\r\n")
        );
        let get = request_bytes("GET / HTTP/1.1\nHost: x\n\n", true, true, true).unwrap();
        assert_eq!(get, b"GET / HTTP/1.1\r\nHost: x\r\n\r\n");
        let kept = request_bytes(
            "POST / HTTP/1.1\nContent-Length: 99\n\nab",
            true,
            true,
            false,
        )
        .unwrap();
        assert!(
            String::from_utf8(kept)
                .unwrap()
                .contains("Content-Length: 99")
        );
        assert!(request_bytes(" \n ", true, true, true).is_err());
    }

    #[test]
    fn captured_success_and_failure() {
        let head = ResponseHead::parse(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n"
                .to_vec(),
        )
        .unwrap();
        let mut response = head.raw.clone();
        response.extend_from_slice(b"{}");
        let sent = Sent {
            peer: "127.0.0.1:8080".parse().unwrap(),
            response_len: response.len() as u64,
            response,
            truncated: false,
            head,
            interim: 1,
            incomplete: None,
            timing: SendTiming {
                total_ms: 9,
                ..SendTiming::default()
            },
            reused: false,
            keep_alive: true,
            sent_request: None,
            notes: Vec::new(),
        };
        let ex = captured(
            Source::Repeater,
            &lab(),
            b"GET /api/x.json?q=1 HTTP/1.1\r\n\r\n".to_vec(),
            5,
            &Ok(sent),
        );
        assert_eq!(ex.source, Source::Repeater);
        assert_eq!(ex.url, "http://lab.test:8080/api/x.json?q=1");
        assert_eq!(ex.status, Some(200));
        assert_eq!(ex.duration_ms, Some(9));
        assert_eq!(ex.mime.as_deref(), Some("JSON"));
        assert_eq!(ex.server_ip.as_deref(), Some("127.0.0.1"));
        assert!(ex.notes.unwrap().contains("interim"));

        let failed = captured(
            Source::Repeater,
            &lab(),
            b"BREW /pot HTTP/1.1\r\n\r\n".to_vec(),
            5,
            &Err(SendError {
                message: "refused".into(),
                timing: SendTiming::default(),
                peer: None,
            }),
        );
        assert_eq!(failed.method, "BREW");
        assert_eq!(failed.error.as_deref(), Some("refused"));
        assert!(failed.response.is_none());
    }

    #[test]
    fn redirect_after_post_becomes_get() {
        let req = "POST /login HTTP/1.1\nHost: lab.test:8080\nContent-Type: application/x-www-form-urlencoded\nContent-Length: 7\nCookie: s=1\n\nuser=me";
        let r = follow_redirect(req, &lab(), 302, "/home?x=1#top").unwrap();
        assert_eq!(r.target, lab());
        assert_eq!(
            r.request,
            "GET /home?x=1 HTTP/1.1\nHost: lab.test:8080\nCookie: s=1\n\n"
        );
        assert_eq!(r.notes.len(), 1);
    }

    #[test]
    fn redirect_307_keeps_method_and_body() {
        let req = "PUT /a/b HTTP/1.1\nHost: lab.test:8080\nContent-Length: 2\n\nhi";
        let r = follow_redirect(req, &lab(), 307, "c").unwrap();
        assert_eq!(
            r.request,
            "PUT /a/c HTTP/1.1\nHost: lab.test:8080\nContent-Length: 2\n\nhi"
        );
        assert!(r.notes.is_empty());
    }

    #[test]
    fn cross_origin_redirect_drops_credentials() {
        let req = "GET / HTTP/1.1\nHost: lab.test:8080\nAuthorization: Bearer t\nCookie: a=b\nAccept: */*\n\n";
        let r = follow_redirect(req, &lab(), 301, "https://Other.test/x").unwrap();
        assert_eq!(
            r.target,
            RepeaterTarget {
                scheme: "https".into(),
                host: "other.test".into(),
                port: 443
            }
        );
        assert_eq!(
            r.request,
            "GET /x HTTP/1.1\nHost: other.test\nAccept: */*\n\n"
        );
        assert!(r.notes[0].contains("Authorization, Cookie"));

        let scheme_relative = follow_redirect(req, &lab(), 302, "//cdn.test:81").unwrap();
        assert_eq!(scheme_relative.target.port, 81);
        assert!(
            scheme_relative
                .request
                .starts_with("GET / HTTP/1.1\nHost: cdn.test:81\n")
        );
    }

    #[test]
    fn redirect_errors() {
        let req = "GET / HTTP/1.1\nHost: x\n\n";
        assert!(follow_redirect(req, &lab(), 200, "/x").is_err());
        assert!(follow_redirect(req, &lab(), 302, "  ").is_err());
        assert!(follow_redirect(req, &lab(), 302, "ftp://x/").is_err());
        assert!(follow_redirect(req, &lab(), 302, "http://bad host/").is_err());
    }
}
