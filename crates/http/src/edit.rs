//! Converting a raw message to editable text and back without altering any
//! byte the user did not change.
//!
//! Text editors normalize line endings to `\n`. HTTP heads use CRLF and bodies
//! may use either (multipart uses CRLF). We therefore record the line ending
//! convention of the head and of the body separately and restore it when the
//! text is converted back. Messages whose bytes cannot survive this round
//! trip (invalid UTF-8, mixed or lone-CR line endings) are reported as not
//! editable as text.

use crate::content::{DecodeError, dechunk, decode_content, split_message};
use crate::head::find_head_end;

/// A line-ending convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    /// `\n`
    Lf,
    /// `\r\n`
    CrLf,
}

impl Eol {
    fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
        }
    }
}

/// An editable text view of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextForm {
    /// Message with `\n` line endings; head and body separated by an empty line.
    pub text: String,
    /// Line endings of the head.
    pub head_eol: Eol,
    /// Line endings of the body.
    pub body_eol: Eol,
}

/// Why a message cannot be edited as text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotEditable {
    /// The message contains bytes that are not valid UTF-8 (binary or
    /// compressed body).
    #[error("the message contains binary data")]
    Binary,
    /// Line endings are mixed or contain a lone carriage return, which a text
    /// editor cannot preserve.
    #[error("the message mixes line endings")]
    MixedLineEndings,
    /// No complete head was found.
    #[error("the message has no complete head")]
    Incomplete,
}

/// Uniform line ending of `s`; `default` if it contains no line breaks.
fn uniform_eol(s: &str, default: Eol) -> Result<Eol, NotEditable> {
    let bytes = s.as_bytes();
    let mut seen: Option<Eol> = None;
    for (i, b) in bytes.iter().enumerate() {
        let eol = match b {
            b'\n' if i > 0 && bytes[i - 1] == b'\r' => Eol::CrLf,
            b'\n' => Eol::Lf,
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => continue,
            b'\r' => return Err(NotEditable::MixedLineEndings),
            _ => continue,
        };
        match seen {
            None => seen = Some(eol),
            Some(prev) if prev != eol => return Err(NotEditable::MixedLineEndings),
            Some(_) => {}
        }
    }
    Ok(seen.unwrap_or(default))
}

/// Builds the editable text form of a raw message.
pub fn to_text(raw: &[u8]) -> Result<TextForm, NotEditable> {
    if find_head_end(raw).is_none() {
        return Err(NotEditable::Incomplete);
    }
    let (head, body) = split_message(raw);
    let head = std::str::from_utf8(head).map_err(|_| NotEditable::Binary)?;
    let body = std::str::from_utf8(body).map_err(|_| NotEditable::Binary)?;
    let head_eol = uniform_eol(head, Eol::CrLf)?;
    let body_eol = uniform_eol(body, head_eol)?;
    let mut text = head.replace("\r\n", "\n");
    text.push_str(&body.replace("\r\n", "\n"));
    Ok(TextForm {
        text,
        head_eol,
        body_eol,
    })
}

/// Rebuilds raw bytes from (possibly edited) text, restoring line endings.
/// Everything up to the first empty line is the head.
pub fn from_text(text: &str, head_eol: Eol, body_eol: Eol) -> Vec<u8> {
    let text = text.replace("\r\n", "\n");
    let (head, body) = match text.find("\n\n") {
        Some(i) => (&text[..i + 2], &text[i + 2..]),
        // No empty line: the user removed it; treat everything as head and
        // terminate it so the message stays well-formed.
        None => (text.as_str(), ""),
    };
    let mut out = head.replace('\n', head_eol.as_str());
    if !head.ends_with("\n\n") {
        if !head.ends_with('\n') {
            out.push_str(head_eol.as_str());
        }
        out.push_str(head_eol.as_str());
    }
    out.push_str(&body.replace('\n', body_eol.as_str()));
    out.into_bytes()
}

/// Sets every `Content-Length` header to the actual body length, unless the
/// message uses `Transfer-Encoding` (then the length is defined by chunks).
/// Messages without `Content-Length` and a non-empty body get none added for
/// GET-like requests; callers decide (`add_if_missing`).
pub fn update_content_length(raw: &[u8], add_if_missing: bool) -> Vec<u8> {
    let Some(end) = find_head_end(raw) else {
        return raw.to_vec();
    };
    let (head, body) = raw.split_at(end);
    let lines: Vec<&[u8]> = head.split_inclusive(|b| *b == b'\n').collect();
    let header_name = |line: &[u8]| -> Vec<u8> {
        line.split(|b| *b == b':')
            .next()
            .unwrap_or_default()
            .trim_ascii()
            .to_ascii_lowercase()
    };
    if lines
        .iter()
        .skip(1)
        .any(|l| header_name(l) == b"transfer-encoding")
    {
        return raw.to_vec();
    }
    let eol: &[u8] = if head.ends_with(b"\r\n\r\n") {
        b"\r\n"
    } else {
        b"\n"
    };
    let mut out = Vec::with_capacity(raw.len() + 24);
    let mut found = false;
    for (i, line) in lines.iter().enumerate() {
        let is_terminator = line.trim_ascii().is_empty();
        if i > 0 && !is_terminator && header_name(line) == b"content-length" {
            let name_len = line.iter().position(|b| *b == b':').unwrap_or(line.len());
            out.extend_from_slice(&line[..name_len]);
            out.extend_from_slice(format!(": {}", body.len()).as_bytes());
            out.extend_from_slice(eol);
            found = true;
            continue;
        }
        if is_terminator && !found && add_if_missing && !body.is_empty() {
            out.extend_from_slice(format!("Content-Length: {}", body.len()).as_bytes());
            out.extend_from_slice(eol);
        }
        out.extend_from_slice(line);
    }
    out.extend_from_slice(body);
    out
}

/// Whether a message has a transfer or content coding that [`unpack`] can
/// remove (chunked framing or compression).
pub fn is_packed(raw: &[u8]) -> bool {
    let (head, _) = split_message(raw);
    String::from_utf8_lossy(head).lines().skip(1).any(|l| {
        let name = l.split(':').next().unwrap_or("").trim();
        let value = l.split_once(':').map_or("", |(_, v)| v).trim();
        (name.eq_ignore_ascii_case("transfer-encoding") && !value.is_empty())
            || (name.eq_ignore_ascii_case("content-encoding")
                && !value.is_empty()
                && !value.eq_ignore_ascii_case("identity"))
    })
}

/// Removes chunked framing and content codings so the body can be edited
/// as plain data: the body is decoded, `Transfer-Encoding` and
/// `Content-Encoding` headers are removed and `Content-Length` is set.
/// Everything else in the head is kept byte for byte.
pub fn unpack(raw: &[u8], limit: usize) -> Result<Vec<u8>, DecodeError> {
    let (head, body) = split_message(raw);
    let head_text = String::from_utf8_lossy(head);
    let header = |name: &str| -> Option<String> {
        head_text.lines().skip(1).find_map(|l| {
            let (n, v) = l.split_once(':')?;
            n.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_string())
        })
    };
    let mut data = body.to_vec();
    if header("transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().ends_with("chunked")) {
        let (decoded, complete) = dechunk(body)?;
        if !complete {
            return Err(DecodeError::Chunked("incomplete chunked body".into()));
        }
        data = decoded;
    }
    if let Some(enc) = header("content-encoding") {
        data = decode_content(&data, &enc, limit)?;
    }
    let new_head =
        crate::rewrite::rewrite_head(head, None, &["Transfer-Encoding", "Content-Encoding"]);
    let mut out = new_head;
    out.extend_from_slice(&data);
    Ok(update_content_length(&out, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(raw: &[u8]) {
        let form = to_text(raw).unwrap();
        assert!(!form.text.contains('\r'));
        assert_eq!(from_text(&form.text, form.head_eol, form.body_eol), raw);
    }

    #[test]
    fn unedited_text_round_trips_byte_exact() {
        round_trip(b"GET / HTTP/1.1\r\nHost: a\r\n\r\n");
        round_trip(b"POST /x HTTP/1.1\r\nHost: a\r\nContent-Length: 7\r\n\r\na=1&b=2");
        round_trip(b"POST /j HTTP/1.1\r\nContent-Length: 9\r\n\r\n{\n \"a\":1\n}");
        round_trip(b"POST /m HTTP/1.1\r\nContent-Type: multipart/form-data\r\n\r\n--x\r\nContent-Disposition: form-data\r\n\r\nv\r\n--x--\r\n");
        round_trip(b"HTTP/1.0 200 OK\nServer: old\n\nline1\nline2");
    }

    #[test]
    fn binary_and_mixed_messages_are_not_editable() {
        assert_eq!(
            to_text(b"HTTP/1.1 200 OK\r\n\r\n\x1f\x8b\x08\x00\xff"),
            Err(NotEditable::Binary)
        );
        assert_eq!(
            to_text(b"POST / HTTP/1.1\r\n\r\na\r\nb\nc"),
            Err(NotEditable::MixedLineEndings)
        );
        assert_eq!(
            to_text(b"GET / HTTP/1.1\r\nHost"),
            Err(NotEditable::Incomplete)
        );
    }

    #[test]
    fn edits_keep_crlf_head() {
        let form = to_text(b"GET / HTTP/1.1\r\nHost: a\r\n\r\n").unwrap();
        let edited = form.text.replace("GET /", "GET /admin");
        let raw = from_text(&edited, form.head_eol, form.body_eol);
        assert_eq!(raw, b"GET /admin HTTP/1.1\r\nHost: a\r\n\r\n");
        // Removing the blank line still yields a terminated head.
        let raw = from_text("GET / HTTP/1.1\nHost: a", Eol::CrLf, Eol::CrLf);
        assert_eq!(raw, b"GET / HTTP/1.1\r\nHost: a\r\n\r\n");
    }

    #[test]
    fn unpacks_chunked_gzip_bodies() {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"{\"a\":1}").unwrap();
        let gz = enc.finish().unwrap();
        let mut raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        raw.extend(format!("{:x}\r\n", gz.len()).as_bytes());
        raw.extend(&gz);
        raw.extend(b"\r\n0\r\n\r\n");
        assert!(is_packed(&raw));
        let out = unpack(&raw, 1 << 20).unwrap();
        assert_eq!(
            out,
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\n{\"a\":1}"
        );
        assert!(!is_packed(&out));
        assert!(to_text(&out).is_ok(), "editable after unpacking");
        assert!(
            unpack(
                b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\nnot gzip",
                1024
            )
            .is_err()
        );
    }

    #[test]
    fn content_length_is_updated_preserving_name_case() {
        let raw = b"POST / HTTP/1.1\r\ncontent-LENGTH: 3\r\nX: y\r\n\r\nhello";
        assert_eq!(
            update_content_length(raw, false),
            b"POST / HTTP/1.1\r\ncontent-LENGTH: 5\r\nX: y\r\n\r\nhello"
        );
        let chunked =
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 1\r\n\r\n0\r\n\r\n";
        assert_eq!(update_content_length(chunked, true), chunked);
        assert_eq!(
            update_content_length(b"POST / HTTP/1.1\r\nHost: a\r\n\r\nbody", true),
            b"POST / HTTP/1.1\r\nHost: a\r\nContent-Length: 4\r\n\r\nbody"
        );
        let get = b"GET / HTTP/1.1\r\nHost: a\r\n\r\n";
        assert_eq!(update_content_length(get, true), get);
    }
}
