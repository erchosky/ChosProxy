//! A request/response pair prepared for the checks: parsed heads and the
//! decoded response body as text.

use chosproxy_http::content::{body_text, split_message};
use chosproxy_http::head::Headers;
use chosproxy_http::{RequestHead, ResponseHead};

/// Largest decoded response body the checks read.
const BODY_LIMIT: usize = 2 * 1024 * 1024;

/// One captured exchange, as the checks see it.
#[derive(Debug)]
pub struct Message {
    /// `http` or `https`.
    pub scheme: String,
    /// Host (lower case).
    pub host: String,
    /// Port.
    pub port: u16,
    /// Path with query.
    pub path: String,
    /// Parsed request head, if well formed.
    pub request: Option<RequestHead>,
    /// Parsed response head, if there is a response.
    pub response: Option<ResponseHead>,
    /// Response body decoded (dechunked, decompressed) as text, at most
    /// 2 MiB; empty for binary or missing bodies.
    pub body: String,
}

impl Message {
    /// Prepares raw messages for analysis.
    pub fn new(
        scheme: &str,
        host: &str,
        port: u16,
        path: &str,
        request: &[u8],
        response: Option<&[u8]>,
    ) -> Message {
        let request = {
            let (head, _) = split_message(request);
            RequestHead::parse(head.to_vec()).ok()
        };
        let (response, body) = match response {
            Some(raw) => {
                let (head, body) = split_message(raw);
                match ResponseHead::parse(head.to_vec()) {
                    Ok(h) => {
                        let text = decoded_text(&h, body);
                        (Some(h), text)
                    }
                    Err(_) => (None, String::new()),
                }
            }
            None => (None, String::new()),
        };
        Message {
            scheme: scheme.to_ascii_lowercase(),
            host: host.to_ascii_lowercase(),
            port,
            path: path.to_string(),
            request,
            response,
            body,
        }
    }

    /// `scheme://host:port`, the key of origin-wide issues.
    pub fn origin(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }

    /// The path without query or fragment.
    pub fn path_only(&self) -> &str {
        self.path.split(['?', '#']).next().unwrap_or("/")
    }

    /// The query string, if any (without `?`).
    pub fn query(&self) -> Option<&str> {
        let q = self.path.split_once('?')?.1;
        Some(q.split('#').next().unwrap_or_default())
    }

    /// True over HTTPS.
    pub fn https(&self) -> bool {
        self.scheme == "https"
    }

    /// Response status (0 without a response).
    pub fn status(&self) -> u16 {
        self.response.as_ref().map_or(0, |r| r.status)
    }

    /// A response header (joined if repeated), lower-cased name.
    pub fn response_header(&self, name: &str) -> Option<String> {
        self.response.as_ref()?.header_str(name)
    }

    /// Every value of a response header (e.g. `set-cookie`).
    pub fn response_headers(&self, name: &str) -> Vec<String> {
        match &self.response {
            Some(r) => r
                .headers
                .iter()
                .filter(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| String::from_utf8_lossy(&h.value).into_owned())
                .collect(),
            None => Vec::new(),
        }
    }

    /// A request header.
    pub fn request_header(&self, name: &str) -> Option<String> {
        self.request.as_ref()?.header_str(name)
    }

    /// The response's media type (`text/html`…), lower case.
    pub fn media_type(&self) -> String {
        self.response_header("content-type")
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
    }

    /// The response is an HTML page.
    pub fn is_html(&self) -> bool {
        let t = self.media_type();
        t == "text/html" || t == "application/xhtml+xml"
    }
}

/// The body as text (empty when it is not text or cannot be decoded).
fn decoded_text(head: &ResponseHead, raw: &[u8]) -> String {
    body_text(head, raw, BODY_LIMIT).unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn decodes_chunked_and_gzip_bodies() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"<html>secret</html>").unwrap();
        let compressed = gz.finish().unwrap();
        let mut raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
            Content-Encoding: gzip\r\nTransfer-Encoding: chunked\r\n\r\n"
            .to_vec();
        raw.extend_from_slice(format!("{:x}\r\n", compressed.len()).as_bytes());
        raw.extend_from_slice(&compressed);
        raw.extend_from_slice(b"\r\n0\r\n\r\n");
        let m = Message::new(
            "HTTPS",
            "Lab.Test",
            443,
            "/a/b?x=1#f",
            b"GET /a/b?x=1 HTTP/1.1\r\nHost: lab.test\r\n\r\n",
            Some(&raw),
        );
        assert_eq!(m.body, "<html>secret</html>");
        assert!(m.is_html() && m.https());
        assert_eq!(m.origin(), "https://lab.test:443");
        assert_eq!((m.path_only(), m.query()), ("/a/b", Some("x=1")));
        assert_eq!(m.request_header("host").as_deref(), Some("lab.test"));
    }

    #[test]
    fn binary_bodies_are_skipped() {
        let m = Message::new(
            "http",
            "x",
            80,
            "/",
            b"GET / HTTP/1.1\r\n\r\n",
            Some(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n\r\n\x89PNG\0\0"),
        );
        assert!(m.body.is_empty());
        assert_eq!(m.status(), 200);
    }
}
