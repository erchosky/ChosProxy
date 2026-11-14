//! Preparing stored messages for display: de-chunking, decompressing and
//! text detection. Never alters stored data.

use crate::types::MessageView;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chosproxy_http::content::{dechunk, decode_content, is_probably_text, split_message};
use chosproxy_http::head::Header;

/// Maximum raw bytes sent to the UI for one message.
pub(crate) const DISPLAY_LIMIT: usize = 4 * 1024 * 1024;
/// Maximum decoded body size (decompression bomb guard).
const DECODE_LIMIT: usize = 32 * 1024 * 1024;

fn headers_of(head: &[u8]) -> Vec<Header> {
    // Parse loosely line by line: the stored head may be truncated or odd.
    String::from_utf8_lossy(head)
        .lines()
        .skip(1)
        .filter_map(|l| {
            let (name, value) = l.split_once(':')?;
            Some(Header {
                name: name.trim().to_string(),
                value: value.trim().as_bytes().to_vec(),
            })
        })
        .collect()
}

fn header<'a>(headers: &'a [Header], name: &str) -> Option<&'a [u8]> {
    headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_slice())
}

/// A message body with transfer and content codings removed.
pub(crate) struct DecodedBody {
    /// The decoded bytes (the raw body when decoding failed).
    pub data: Vec<u8>,
    /// Codings removed, in order (`chunked`, `gzip`, …).
    pub decoded_from: Vec<String>,
    /// Why decoding was incomplete, if it was.
    pub note: Option<String>,
}

/// Removes chunked framing and content codings from a stored body.
pub(crate) fn decode_body(head: &[u8], body: &[u8]) -> DecodedBody {
    let headers = headers_of(head);
    let mut decoded_from = Vec::new();
    let mut note = None;
    let mut decoded = body.to_vec();

    let te = header(&headers, "transfer-encoding")
        .map(|v| String::from_utf8_lossy(v).to_ascii_lowercase());
    if te
        .as_deref()
        .is_some_and(|t| t.trim_end().ends_with("chunked"))
    {
        match dechunk(body) {
            Ok((data, complete)) => {
                decoded = data;
                decoded_from.push("chunked".to_string());
                if !complete {
                    note = Some(
                        "chunked body is incomplete (capture truncated or connection closed)"
                            .into(),
                    );
                }
            }
            Err(e) => note = Some(e.to_string()),
        }
    }
    if let Some(enc) =
        header(&headers, "content-encoding").map(|v| String::from_utf8_lossy(v).into_owned())
        && !enc.trim().is_empty()
        && !enc.trim().eq_ignore_ascii_case("identity")
    {
        match decode_content(&decoded, &enc, DECODE_LIMIT) {
            Ok(data) => {
                decoded = data;
                decoded_from.extend(enc.split(',').map(|c| c.trim().to_ascii_lowercase()));
            }
            Err(e) => {
                note.get_or_insert_with(|| format!("{e}; showing encoded bytes"));
            }
        }
    }
    DecodedBody {
        data: decoded,
        decoded_from,
        note,
    }
}

/// Builds the display view of a stored raw message.
pub(crate) fn message_view(
    raw: &[u8],
    wire_size: Option<u64>,
    capture_truncated: bool,
) -> MessageView {
    let (head, body) = split_message(raw);
    let DecodedBody {
        data: mut decoded,
        decoded_from,
        note,
    } = decode_body(head, body);

    let body_size = decoded.len() as u64;
    let body_text = (is_probably_text(&decoded) && decoded.len() <= DISPLAY_LIMIT)
        .then(|| String::from_utf8_lossy(&decoded).into_owned());
    decoded.truncate(DISPLAY_LIMIT);
    let shown = &raw[..raw.len().min(DISPLAY_LIMIT)];
    MessageView {
        raw_base64: STANDARD.encode(shown),
        raw_size: raw.len() as u64,
        raw_display_truncated: raw.len() > DISPLAY_LIMIT,
        capture_truncated,
        wire_size,
        head: String::from_utf8_lossy(head).into_owned(),
        body_base64: STANDARD.encode(&decoded),
        body_size,
        body_text,
        decoded_from,
        decode_note: note,
        content_type: header(&headers_of(head), "content-type")
            .map(|v| String::from_utf8_lossy(v).into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn decodes_chunked_gzip_for_display() {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"{\"a\":1}").unwrap();
        let mut gz = enc.finish().unwrap();
        let mut raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        raw.extend(format!("{:x}\r\n", gz.len()).as_bytes());
        raw.append(&mut gz);
        raw.extend(b"\r\n0\r\n\r\n");
        let v = message_view(&raw, Some(raw.len() as u64), false);
        assert_eq!(v.body_text.as_deref(), Some("{\"a\":1}"));
        assert_eq!(v.decoded_from, vec!["chunked", "gzip"]);
        assert!(v.decode_note.is_none());
        assert_eq!(v.content_type.as_deref(), Some("application/json"));
        assert_eq!(STANDARD.decode(&v.raw_base64).unwrap(), raw);
    }

    #[test]
    fn binary_and_broken_bodies() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n\x00\x01not gzip";
        let v = message_view(raw, None, true);
        assert!(v.decode_note.is_some());
        assert!(v.body_text.is_none());
        assert!(v.capture_truncated);
        let req = message_view(b"GET / HTTP/1.1\r\nHost: a\r\n\r\n", Some(27), false);
        assert_eq!(req.body_size, 0);
        assert_eq!(req.head, "GET / HTTP/1.1\r\nHost: a\r\n\r\n");
    }
}
