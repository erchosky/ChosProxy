//! Content helpers used for *display and classification only*. Nothing here
//! alters what is forwarded or stored.

use crate::body::parse_chunk_size;
use crate::head::{find_head_end, trim_eol};
use std::io::Read;

/// Splits a stored raw message into head and body.
pub fn split_message(raw: &[u8]) -> (&[u8], &[u8]) {
    match find_head_end(raw) {
        Some(end) => raw.split_at(end),
        None => (raw, &[]),
    }
}

/// Errors decoding a body for display.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    /// Chunked framing is invalid or incomplete.
    #[error("invalid chunked body: {0}")]
    Chunked(String),
    /// A content coding could not be decoded.
    #[error("cannot decode {coding}: {message}")]
    Coding {
        /// The coding that failed.
        coding: String,
        /// Decoder message.
        message: String,
    },
    /// Content coding is not supported.
    #[error("unsupported content coding {0:?}")]
    Unsupported(String),
    /// Decoded output exceeds the limit (decompression-bomb guard).
    #[error("decoded body exceeds {0} bytes")]
    TooLarge(usize),
}

/// Removes chunked framing. Tolerates a missing final chunk (truncated
/// capture) by returning the data decoded so far with `complete = false`.
pub fn dechunk(raw: &[u8]) -> Result<(Vec<u8>, bool), DecodeError> {
    let mut out = Vec::with_capacity(raw.len());
    let mut rest = raw;
    loop {
        let Some(nl) = memchr::memchr(b'\n', rest) else {
            return Ok((out, false));
        };
        let size =
            parse_chunk_size(&rest[..=nl]).map_err(|e| DecodeError::Chunked(e.to_string()))?;
        rest = &rest[nl + 1..];
        if size == 0 {
            return Ok((out, true));
        }
        let size =
            usize::try_from(size).map_err(|_| DecodeError::Chunked("chunk too large".into()))?;
        if rest.len() < size {
            out.extend_from_slice(rest);
            return Ok((out, false));
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size..];
        match memchr::memchr(b'\n', rest) {
            Some(i) if trim_eol(&rest[..=i]).is_empty() => rest = &rest[i + 1..],
            Some(_) => return Err(DecodeError::Chunked("missing CRLF after chunk".into())),
            None => return Ok((out, false)),
        }
    }
}

/// Decodes `Content-Encoding` codings (applied in listed order, so removed in
/// reverse). Supports gzip, deflate (zlib or raw), br and identity.
pub fn decode_content(
    body: &[u8],
    content_encoding: &str,
    limit: usize,
) -> Result<Vec<u8>, DecodeError> {
    let codings: Vec<String> = content_encoding
        .split(',')
        .map(|c| c.trim().to_ascii_lowercase())
        .filter(|c| !c.is_empty() && c != "identity")
        .collect();
    let mut data = body.to_vec();
    for coding in codings.iter().rev() {
        data = match coding.as_str() {
            "gzip" | "x-gzip" => {
                read_limited(flate2::read::MultiGzDecoder::new(&data[..]), coding, limit)?
            }
            "deflate" => read_limited(flate2::read::ZlibDecoder::new(&data[..]), coding, limit)
                .or_else(|_| {
                    read_limited(flate2::read::DeflateDecoder::new(&data[..]), coding, limit)
                })?,
            "br" => read_limited(brotli::Decompressor::new(&data[..], 4096), coding, limit)?,
            other => return Err(DecodeError::Unsupported(other.to_string())),
        };
    }
    Ok(data)
}

fn read_limited(reader: impl Read, coding: &str, limit: usize) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    reader
        .take(cap)
        .read_to_end(&mut out)
        .map_err(|e| DecodeError::Coding {
            coding: coding.to_string(),
            message: e.to_string(),
        })?;
    if out.len() > limit {
        return Err(DecodeError::TooLarge(limit));
    }
    Ok(out)
}

/// The body of a response as text: dechunked and decompressed as its
/// headers say, cut at `limit` bytes. `None` when it is empty, cannot be
/// decoded, or looks binary (a NUL byte near the start).
pub fn body_text(head: &crate::ResponseHead, raw_body: &[u8], limit: usize) -> Option<String> {
    use crate::head::Headers;
    if raw_body.is_empty() {
        return None;
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
    if let Some(encoding) = head.header_str("content-encoding") {
        body = decode_content(&body, &encoding, limit).ok()?;
    }
    body.truncate(limit);
    if body[..body.len().min(1024)].contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&body).into_owned())
}

/// Lowercased file extension of a request path (query and fragment ignored).
pub fn extension_of(path: &str) -> Option<String> {
    let path = path.split(['?', '#']).next().unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("");
    let (_, ext) = last.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 10 || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Heuristic: does this look like human-readable text?
pub fn is_probably_text(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(4096)];
    if sample.is_empty() {
        return true;
    }
    if sample.contains(&0) {
        return false;
    }
    let text = match std::str::from_utf8(sample) {
        Ok(s) => s,
        // A multi-byte sequence may be cut at the sample boundary.
        Err(e) if e.valid_up_to() + 4 >= sample.len() => {
            std::str::from_utf8(&sample[..e.valid_up_to()]).unwrap_or_default()
        }
        Err(_) => return false,
    };
    let control = text
        .chars()
        .filter(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        .count();
    control * 20 < text.chars().count().max(1)
}

/// Coarse MIME category shown in history ("HTML", "JSON", "script", …).
pub fn classify_mime(content_type: Option<&str>, path: &str, body: &[u8]) -> &'static str {
    if let Some(ct) = content_type {
        let essence = ct
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let by_type = match essence.as_str() {
            "text/html" | "application/xhtml+xml" => Some("HTML"),
            "application/json" | "text/json" | "application/problem+json" => Some("JSON"),
            "application/javascript"
            | "text/javascript"
            | "application/x-javascript"
            | "application/ecmascript"
            | "text/ecmascript" => Some("script"),
            "text/css" => Some("CSS"),
            "application/xml" | "text/xml" => Some("XML"),
            "application/x-www-form-urlencoded" => Some("form"),
            "multipart/form-data" => Some("multipart"),
            "image/svg+xml" => Some("SVG"),
            _ if essence.ends_with("+json") => Some("JSON"),
            _ if essence.ends_with("+xml") => Some("XML"),
            _ if essence.starts_with("text/") => Some("text"),
            _ if essence.starts_with("image/") => Some("image"),
            _ if essence.starts_with("font/") || essence.contains("font") => Some("font"),
            _ if essence.starts_with("video/") => Some("video"),
            _ if essence.starts_with("audio/") => Some("audio"),
            _ => None,
        };
        if let Some(kind) = by_type {
            return kind;
        }
    }
    if let Some(ext) = extension_of(path) {
        let by_ext = match ext.as_str() {
            "html" | "htm" => Some("HTML"),
            "js" | "mjs" => Some("script"),
            "css" => Some("CSS"),
            "json" => Some("JSON"),
            "xml" => Some("XML"),
            "svg" => Some("SVG"),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" | "avif" => Some("image"),
            "woff" | "woff2" | "ttf" | "otf" | "eot" => Some("font"),
            _ => None,
        };
        if let Some(kind) = by_ext {
            return kind;
        }
    }
    let trimmed = body.trim_ascii_start();
    let head = &trimmed[..trimmed.len().min(64)];
    let lower = head.to_ascii_lowercase();
    if lower.starts_with(b"<!doctype html") || lower.starts_with(b"<html") {
        "HTML"
    } else if lower.starts_with(b"<?xml") {
        "XML"
    } else if matches!(head.first(), Some(b'{' | b'[')) && is_probably_text(body) {
        "JSON"
    } else if body.is_empty() {
        ""
    } else if is_probably_text(body) {
        "text"
    } else {
        "binary"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn splits_messages() {
        let (h, b) = split_message(b"HTTP/1.1 200 OK\r\n\r\nbody");
        assert_eq!(h, b"HTTP/1.1 200 OK\r\n\r\n");
        assert_eq!(b, b"body");
        let (h, b) = split_message(b"partial");
        assert_eq!((h, b), (&b"partial"[..], &b""[..]));
    }

    #[test]
    fn dechunks() {
        assert_eq!(
            dechunk(b"3\r\nabc\r\n2;x\r\nde\r\n0\r\n\r\n").unwrap(),
            (b"abcde".to_vec(), true)
        );
        assert_eq!(
            dechunk(b"3\r\nabc\r\n5\r\nde").unwrap(),
            (b"abcde".to_vec(), false)
        );
        assert!(dechunk(b"3\r\nabcXX\r\n").is_err());
    }

    #[test]
    fn decodes_gzip_deflate_br() {
        let plain = b"hello hello hello".to_vec();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&plain).unwrap();
        let gz = gz.finish().unwrap();
        assert_eq!(decode_content(&gz, "gzip", 1024).unwrap(), plain);

        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&plain).unwrap();
        assert_eq!(
            decode_content(&z.finish().unwrap(), "deflate", 1024).unwrap(),
            plain
        );

        let mut raw =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        raw.write_all(&plain).unwrap();
        assert_eq!(
            decode_content(&raw.finish().unwrap(), "deflate", 1024).unwrap(),
            plain
        );

        let mut br = Vec::new();
        {
            let mut w = brotli::CompressorWriter::new(&mut br, 4096, 5, 22);
            w.write_all(&plain).unwrap();
        }
        assert_eq!(decode_content(&br, "br", 1024).unwrap(), plain);
        assert_eq!(decode_content(&plain, "identity", 1024).unwrap(), plain);
        assert!(matches!(
            decode_content(&plain, "zstd", 1024),
            Err(DecodeError::Unsupported(_))
        ));
    }

    #[test]
    fn decompression_bomb_guard() {
        let zeros = vec![0u8; 100_000];
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        gz.write_all(&zeros).unwrap();
        let gz = gz.finish().unwrap();
        assert_eq!(
            decode_content(&gz, "gzip", 1000),
            Err(DecodeError::TooLarge(1000))
        );
    }

    #[test]
    fn classifies() {
        assert_eq!(
            classify_mime(Some("text/html; charset=utf-8"), "/", b""),
            "HTML"
        );
        assert_eq!(
            classify_mime(Some("application/vnd.api+json"), "/", b""),
            "JSON"
        );
        assert_eq!(classify_mime(None, "/a/app.JS?v=1", b""), "script");
        assert_eq!(classify_mime(None, "/", b"  <!DOCTYPE html><p>"), "HTML");
        assert_eq!(classify_mime(None, "/", b"{\"a\":1}"), "JSON");
        assert_eq!(classify_mime(None, "/", &[0, 159, 146, 150]), "binary");
        assert_eq!(
            classify_mime(Some("application/octet-stream"), "/", b"plain words"),
            "text"
        );
    }

    #[test]
    fn extensions() {
        assert_eq!(extension_of("/a/b.min.JS?x=y.z").as_deref(), Some("js"));
        assert_eq!(extension_of("/a.b/c"), None);
        assert_eq!(extension_of("/"), None);
    }
}
