//! Message body length determination (RFC 9112 §6).

use crate::HttpError;
use crate::head::{Headers, RequestHead, ResponseHead};

/// How the body of a message is delimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// No body.
    Empty,
    /// Exactly this many bytes.
    Length(u64),
    /// Chunked transfer coding.
    Chunked,
    /// Until the connection closes (responses only).
    UntilClose,
}

/// Body framing plus anomalies worth surfacing to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Framing {
    /// How to read the body.
    pub kind: BodyKind,
    /// Both `Transfer-Encoding` and `Content-Length` were present
    /// (`Transfer-Encoding` wins; classic request-smuggling ambiguity).
    pub cl_te_conflict: bool,
}

fn transfer_chunked(h: &impl Headers) -> Option<bool> {
    let codings = h.header_tokens("transfer-encoding");
    if codings.is_empty() {
        return None;
    }
    Some(codings.last().is_some_and(|c| c == "chunked"))
}

fn content_length(h: &impl Headers) -> Result<Option<u64>, HttpError> {
    let mut found: Option<u64> = None;
    for v in h.header_tokens("content-length") {
        if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
            return Err(HttpError::BadFraming(format!(
                "invalid Content-Length {v:?}"
            )));
        }
        let n: u64 = v
            .parse()
            .map_err(|_| HttpError::BadFraming(format!("Content-Length {v:?} overflows")))?;
        match found {
            Some(prev) if prev != n => {
                return Err(HttpError::BadFraming(
                    "conflicting Content-Length values".into(),
                ));
            }
            _ => found = Some(n),
        }
    }
    Ok(found)
}

/// Framing of a request body.
pub fn request_framing(h: &RequestHead) -> Result<Framing, HttpError> {
    let cl = content_length(h);
    match transfer_chunked(h) {
        Some(true) => Ok(Framing {
            kind: BodyKind::Chunked,
            cl_te_conflict: h.header("content-length").is_some(),
        }),
        Some(false) => Err(HttpError::BadFraming(
            "request Transfer-Encoding does not end with chunked".into(),
        )),
        None => Ok(Framing {
            kind: match cl? {
                Some(0) | None => BodyKind::Empty,
                Some(n) => BodyKind::Length(n),
            },
            cl_te_conflict: false,
        }),
    }
}

/// Framing of a response body, given the method of the request it answers.
/// The caller handles successful `CONNECT` and `101` upgrades separately.
pub fn response_framing(request_method: &str, h: &ResponseHead) -> Result<Framing, HttpError> {
    if request_method.eq_ignore_ascii_case("HEAD")
        || (100..200).contains(&h.status)
        || h.status == 204
        || h.status == 304
    {
        return Ok(Framing {
            kind: BodyKind::Empty,
            cl_te_conflict: false,
        });
    }
    match transfer_chunked(h) {
        Some(chunked) => Ok(Framing {
            kind: if chunked {
                BodyKind::Chunked
            } else {
                BodyKind::UntilClose
            },
            cl_te_conflict: h.header("content-length").is_some(),
        }),
        None => Ok(Framing {
            kind: match content_length(h)? {
                Some(0) => BodyKind::Empty,
                Some(n) => BodyKind::Length(n),
                None => BodyKind::UntilClose,
            },
            cl_te_conflict: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(s: &str) -> RequestHead {
        RequestHead::parse(s.as_bytes().to_vec()).unwrap()
    }
    fn res(s: &str) -> ResponseHead {
        ResponseHead::parse(s.as_bytes().to_vec()).unwrap()
    }

    #[test]
    fn request_rules() {
        assert_eq!(
            request_framing(&req("GET / HTTP/1.1\r\n\r\n"))
                .unwrap()
                .kind,
            BodyKind::Empty
        );
        assert_eq!(
            request_framing(&req("POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\n"))
                .unwrap()
                .kind,
            BodyKind::Length(5)
        );
        assert_eq!(
            request_framing(&req("POST / HTTP/1.1\r\nContent-Length: 5, 5\r\n\r\n"))
                .unwrap()
                .kind,
            BodyKind::Length(5)
        );
        let f = request_framing(&req(
            "POST / HTTP/1.1\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n",
        ))
        .unwrap();
        assert_eq!(f.kind, BodyKind::Chunked);
        assert!(f.cl_te_conflict);
        assert!(
            request_framing(&req("POST / HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n")).is_err()
        );
        assert!(
            request_framing(&req(
                "POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n"
            ))
            .is_err()
        );
        assert!(request_framing(&req("POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n")).is_err());
        assert!(
            request_framing(&req(
                "POST / HTTP/1.1\r\nContent-Length: 99999999999999999999999\r\n\r\n"
            ))
            .is_err()
        );
    }

    #[test]
    fn response_rules() {
        let ok = res("HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n");
        assert_eq!(
            response_framing("GET", &ok).unwrap().kind,
            BodyKind::Length(3)
        );
        assert_eq!(response_framing("HEAD", &ok).unwrap().kind, BodyKind::Empty);
        for s in ["204 No Content", "304 Not Modified", "100 Continue"] {
            let h = res(&format!("HTTP/1.1 {s}\r\nContent-Length: 3\r\n\r\n"));
            assert_eq!(response_framing("GET", &h).unwrap().kind, BodyKind::Empty);
        }
        assert_eq!(
            response_framing("GET", &res("HTTP/1.1 200 OK\r\n\r\n"))
                .unwrap()
                .kind,
            BodyKind::UntilClose
        );
        assert_eq!(
            response_framing(
                "GET",
                &res("HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n")
            )
            .unwrap()
            .kind,
            BodyKind::Chunked
        );
        assert_eq!(
            response_framing(
                "GET",
                &res("HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n")
            )
            .unwrap()
            .kind,
            BodyKind::UntilClose
        );
    }
}
