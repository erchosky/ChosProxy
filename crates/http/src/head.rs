//! Message heads: reading them off the wire and tokenizing them.

use crate::{HttpError, MAX_HEADERS};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// Protocol version of a message head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// HTTP/1.0
    Http10,
    /// HTTP/1.1
    Http11,
    /// HTTP/2, written as text in the same form as HTTP/1.1 with the
    /// version token `HTTP/2` (ADR-0030).
    Http2,
}

impl Version {
    fn from_minor(minor: Option<u8>) -> Result<Version, HttpError> {
        match minor {
            Some(0) => Ok(Version::Http10),
            Some(1) => Ok(Version::Http11),
            _ => Err(HttpError::Malformed("unsupported HTTP version".into())),
        }
    }

    /// Canonical token, e.g. `HTTP/1.1`.
    pub fn as_str(self) -> &'static str {
        match self {
            Version::Http10 => "HTTP/1.0",
            Version::Http11 => "HTTP/1.1",
            Version::Http2 => "HTTP/2",
        }
    }
}

/// One header field, with the name exactly as sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Field name (original case).
    pub name: String,
    /// Field value bytes (may be non-UTF-8).
    pub value: Vec<u8>,
}

/// Header list helpers shared by requests and responses.
pub trait Headers {
    /// All header fields in order.
    fn headers(&self) -> &[Header];

    /// First value of a header (case-insensitive name).
    fn header(&self, name: &str) -> Option<&[u8]> {
        self.headers()
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_slice())
    }

    /// First value of a header as lossy UTF-8 text.
    fn header_str(&self, name: &str) -> Option<String> {
        self.header(name)
            .map(|v| String::from_utf8_lossy(v).trim().to_string())
    }

    /// All comma-separated, lowercased tokens of every instance of a header.
    fn header_tokens(&self, name: &str) -> Vec<String> {
        self.headers()
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case(name))
            .flat_map(|h| {
                String::from_utf8_lossy(&h.value)
                    .split(',')
                    .map(|t| t.trim().to_ascii_lowercase())
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// True if the `Connection` header lists `token`.
    fn connection_has(&self, token: &str) -> bool {
        self.header_tokens("connection")
            .iter()
            .any(|t| t.eq_ignore_ascii_case(token))
    }
}

/// A parsed request head plus its exact raw bytes.
#[derive(Debug, Clone)]
pub struct RequestHead {
    /// Method token as sent.
    pub method: String,
    /// Request target as sent.
    pub target: String,
    /// Protocol version.
    pub version: Version,
    /// Header fields.
    pub headers: Vec<Header>,
    /// Exact bytes of the head, including the terminating empty line.
    pub raw: Vec<u8>,
}

impl Headers for RequestHead {
    fn headers(&self) -> &[Header] {
        &self.headers
    }
}

impl RequestHead {
    /// Parses a complete request head.
    pub fn parse(raw: Vec<u8>) -> Result<RequestHead, HttpError> {
        let h2 = request_is_h2(&raw);
        let patched = if h2 {
            Some(h1_request_copy(&raw))
        } else {
            None
        };
        let mut storage = vec![httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut req = httparse::Request::new(&mut storage);
        let status = httparse::ParserConfig::default()
            .allow_multiple_spaces_in_request_line_delimiters(true)
            .parse_request(&mut req, patched.as_deref().unwrap_or(&raw))
            .map_err(|e| HttpError::Malformed(e.to_string()))?;
        if status.is_partial() {
            return Err(HttpError::Malformed("incomplete request head".into()));
        }
        let method = req.method.unwrap_or_default().to_string();
        let target = req.path.unwrap_or_default().to_string();
        let version = if h2 {
            Version::Http2
        } else {
            Version::from_minor(req.version)?
        };
        let headers = collect_headers(req.headers);
        Ok(RequestHead {
            method,
            target,
            version,
            headers,
            raw,
        })
    }

    /// Whether the client allows the connection to be reused afterwards.
    pub fn keep_alive(&self) -> bool {
        match self.version {
            Version::Http11 | Version::Http2 => !self.connection_has("close"),
            Version::Http10 => self.connection_has("keep-alive"),
        }
    }

    /// The version token exactly as written in the request line.
    pub fn version_token(&self) -> &[u8] {
        let line_end = memchr::memchr(b'\n', &self.raw).unwrap_or(self.raw.len());
        let line = trim_eol(&self.raw[..line_end]);
        match memchr::memrchr(b' ', line) {
            Some(i) => &line[i + 1..],
            None => self.version.as_str().as_bytes(),
        }
    }
}

/// A parsed response head plus its exact raw bytes.
#[derive(Debug, Clone)]
pub struct ResponseHead {
    /// Protocol version.
    pub version: Version,
    /// Status code.
    pub status: u16,
    /// Reason phrase as sent (may be empty).
    pub reason: String,
    /// Header fields.
    pub headers: Vec<Header>,
    /// Exact bytes of the head, including the terminating empty line.
    pub raw: Vec<u8>,
}

impl Headers for ResponseHead {
    fn headers(&self) -> &[Header] {
        &self.headers
    }
}

impl ResponseHead {
    /// Parses a complete response head (lenient about common server quirks).
    pub fn parse(raw: Vec<u8>) -> Result<ResponseHead, HttpError> {
        let h2 = raw.starts_with(b"HTTP/2 ");
        let patched = h2.then(|| {
            let mut v = b"HTTP/1.1 ".to_vec();
            v.extend_from_slice(&raw[7..]);
            v
        });
        let mut storage = vec![httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut res = httparse::Response::new(&mut storage);
        let status = httparse::ParserConfig::default()
            .allow_spaces_after_header_name_in_responses(true)
            .allow_obsolete_multiline_headers_in_responses(true)
            .allow_multiple_spaces_in_response_status_delimiters(true)
            .parse_response(&mut res, patched.as_deref().unwrap_or(&raw))
            .map_err(|e| HttpError::Malformed(e.to_string()))?;
        if status.is_partial() {
            return Err(HttpError::Malformed("incomplete response head".into()));
        }
        let version = if h2 {
            Version::Http2
        } else {
            Version::from_minor(res.version)?
        };
        let code = res.code.unwrap_or_default();
        let reason = res.reason.unwrap_or_default().to_string();
        let headers = collect_headers(res.headers);
        Ok(ResponseHead {
            version,
            status: code,
            reason,
            headers,
            raw,
        })
    }

    /// Whether the server allows the connection to be reused afterwards
    /// (ignoring body framing, which the caller must also consider).
    pub fn keep_alive(&self) -> bool {
        match self.version {
            Version::Http11 | Version::Http2 => !self.connection_has("close"),
            Version::Http10 => self.connection_has("keep-alive"),
        }
    }

    /// True for 1xx responses other than 101 (they precede the final one).
    pub fn is_interim(&self) -> bool {
        (100..200).contains(&self.status) && self.status != 101
    }
}

/// True if the request line of `raw` ends with the version `HTTP/2`.
pub fn request_is_h2(raw: &[u8]) -> bool {
    let line_end = memchr::memchr(b'\n', raw).unwrap_or(raw.len());
    trim_eol(&raw[..line_end]).ends_with(b" HTTP/2")
}

/// `raw` with its request line's `HTTP/2` replaced by `HTTP/1.1`, so the
/// HTTP/1 tokenizer can read the rest.
fn h1_request_copy(raw: &[u8]) -> Vec<u8> {
    let line_end = memchr::memchr(b'\n', raw).unwrap_or(raw.len());
    let line = trim_eol(&raw[..line_end]);
    let mut v = Vec::with_capacity(raw.len() + 2);
    v.extend_from_slice(&line[..line.len() - "HTTP/2".len()]);
    v.extend_from_slice(b"HTTP/1.1");
    v.extend_from_slice(&raw[line.len()..]);
    v
}

fn collect_headers(parsed: &[httparse::Header<'_>]) -> Vec<Header> {
    parsed
        .iter()
        .take_while(|h| !h.name.is_empty())
        .map(|h| Header {
            name: h.name.to_string(),
            value: h.value.to_vec(),
        })
        .collect()
}

pub(crate) fn trim_eol(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Returns the index just past the empty line that ends a head, if present.
/// Accepts both CRLF and bare LF line endings.
pub fn find_head_end(buf: &[u8]) -> Option<usize> {
    let mut search = 0;
    while let Some(off) = memchr::memchr(b'\n', &buf[search..]) {
        let i = search + off;
        match buf.get(i + 1) {
            Some(b'\n') => return Some(i + 2),
            Some(b'\r') if buf.get(i + 2) == Some(&b'\n') => return Some(i + 3),
            _ => {}
        }
        search = i + 1;
    }
    None
}

/// Reads one message head from `r`.
///
/// Returns `Ok(None)` if the stream ends cleanly before any byte of a new
/// message (normal end of a keep-alive connection). Leading empty lines are
/// skipped, as RFC 9112 §2.2 recommends. Bytes after the head stay buffered
/// in `r` for the body reader.
pub async fn read_head<R>(r: &mut R, max: usize) -> Result<Option<Vec<u8>>, HttpError>
where
    R: AsyncBufRead + Unpin,
{
    // Skip stray CRLFs between messages (bounded).
    let mut skipped = 0usize;
    loop {
        let avail = r.fill_buf().await?;
        if avail.is_empty() {
            return Ok(None);
        }
        let n = avail
            .iter()
            .take_while(|b| **b == b'\r' || **b == b'\n')
            .count();
        if n == 0 {
            break;
        }
        r.consume(n);
        skipped += n;
        if skipped > 1024 {
            return Err(HttpError::Malformed("too many empty lines".into()));
        }
    }

    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    loop {
        let avail = r.fill_buf().await?;
        if avail.is_empty() {
            return Err(HttpError::UnexpectedEof);
        }
        let n = avail.len();
        let prev_len = buf.len();
        let search_from = prev_len.saturating_sub(2);
        buf.extend_from_slice(avail);
        if let Some(end) = find_head_end(&buf[search_from..]).map(|e| e + search_from) {
            r.consume(end - prev_len);
            buf.truncate(end);
            if end > max {
                return Err(HttpError::HeadTooLarge(max));
            }
            return Ok(Some(buf));
        }
        r.consume(n);
        if buf.len() > max {
            return Err(HttpError::HeadTooLarge(max));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, BufReader};

    #[test]
    fn finds_head_end_variants() {
        assert_eq!(find_head_end(b"GET / HTTP/1.1\r\n\r\nbody"), Some(18));
        assert_eq!(find_head_end(b"GET / HTTP/1.1\n\nbody"), Some(16));
        assert_eq!(find_head_end(b"GET / HTTP/1.1\r\nA: b\r\n"), None);
    }

    #[test]
    fn parses_request_preserving_case_and_order() {
        let raw = b"POST /a?b=1 HTTP/1.1\r\nHost: x\r\nX-CaSe: 1\r\nx-case: 2\r\n\r\n".to_vec();
        let h = RequestHead::parse(raw.clone()).unwrap();
        assert_eq!(h.method, "POST");
        assert_eq!(h.target, "/a?b=1");
        assert_eq!(h.version, Version::Http11);
        assert_eq!(h.headers[1].name, "X-CaSe");
        assert_eq!(h.header("x-case"), Some(&b"1"[..]));
        assert_eq!(h.raw, raw);
        assert_eq!(h.version_token(), b"HTTP/1.1");
    }

    #[test]
    fn keep_alive_semantics() {
        let p = |s: &str| RequestHead::parse(s.as_bytes().to_vec()).unwrap();
        assert!(p("GET / HTTP/1.1\r\n\r\n").keep_alive());
        assert!(!p("GET / HTTP/1.1\r\nConnection: close\r\n\r\n").keep_alive());
        assert!(!p("GET / HTTP/1.0\r\n\r\n").keep_alive());
        assert!(p("GET / HTTP/1.0\r\nConnection: Keep-Alive\r\n\r\n").keep_alive());
        assert!(!p("GET / HTTP/1.1\r\nConnection: upgrade, close\r\n\r\n").keep_alive());
    }

    #[test]
    fn rejects_garbage() {
        assert!(RequestHead::parse(b"\x00\x01\x02\r\n\r\n".to_vec()).is_err());
        assert!(RequestHead::parse(b"GET / HTTP/2.0\r\n\r\n".to_vec()).is_err());
        let h2 = RequestHead::parse(b"GET /a HTTP/2\r\nHost: x\r\n\r\n".to_vec()).unwrap();
        assert_eq!(h2.version, Version::Http2);
        assert_eq!(h2.target, "/a");
        assert_eq!(h2.version_token(), b"HTTP/2");
        assert_eq!(h2.raw, b"GET /a HTTP/2\r\nHost: x\r\n\r\n");
        let res =
            ResponseHead::parse(b"HTTP/2 404\r\ncontent-type: text/html\r\n\r\n".to_vec()).unwrap();
        assert_eq!((res.version, res.status), (Version::Http2, 404));
        assert_eq!(res.header_str("content-type").as_deref(), Some("text/html"));
    }

    #[test]
    fn parses_response_with_quirks() {
        let raw = b"HTTP/1.1 200 OK\r\nServer : x\r\nX-Long: a\r\n b\r\n\r\n".to_vec();
        let h = ResponseHead::parse(raw).unwrap();
        assert_eq!(h.status, 200);
        assert_eq!(h.reason, "OK");
        assert_eq!(h.header_str("server").as_deref(), Some("x"));
        let empty_reason = ResponseHead::parse(b"HTTP/1.1 204\r\n\r\n".to_vec()).unwrap();
        assert_eq!(empty_reason.reason, "");
        assert!(
            ResponseHead::parse(b"HTTP/1.1 100 Continue\r\n\r\n".to_vec())
                .unwrap()
                .is_interim()
        );
    }

    #[tokio::test]
    async fn reads_head_byte_by_byte_and_leaves_body() {
        let data = b"\r\nGET / HTTP/1.1\r\nHost: a\r\n\r\nBODY".to_vec();
        let mut r = BufReader::with_capacity(1, &data[..]);
        let head = read_head(&mut r, 1024).await.unwrap().unwrap();
        assert_eq!(head, b"GET / HTTP/1.1\r\nHost: a\r\n\r\n");
        let mut rest = String::new();
        r.read_to_string(&mut rest).await.unwrap();
        assert_eq!(rest, "BODY");
    }

    #[tokio::test]
    async fn reads_pipelined_heads() {
        let data = b"GET /1 HTTP/1.1\r\n\r\nGET /2 HTTP/1.1\r\n\r\n".to_vec();
        let mut r = BufReader::new(&data[..]);
        let a = read_head(&mut r, 1024).await.unwrap().unwrap();
        let b = read_head(&mut r, 1024).await.unwrap().unwrap();
        assert!(a.starts_with(b"GET /1"));
        assert!(b.starts_with(b"GET /2"));
        assert!(read_head(&mut r, 1024).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn head_limits() {
        let mut data = b"GET / HTTP/1.1\r\n".to_vec();
        data.extend(std::iter::repeat_n(b'a', 200));
        data.extend_from_slice(b"\r\n\r\n");
        let mut r = BufReader::with_capacity(16, &data[..]);
        assert!(matches!(
            read_head(&mut r, 64).await,
            Err(HttpError::HeadTooLarge(64))
        ));
        let mut r = BufReader::new(&b"GET / HTTP/1.1\r\nHost"[..]);
        assert!(matches!(
            read_head(&mut r, 64).await,
            Err(HttpError::UnexpectedEof)
        ));
        let mut r = BufReader::new(&b""[..]);
        assert!(read_head(&mut r, 64).await.unwrap().is_none());
    }
}
