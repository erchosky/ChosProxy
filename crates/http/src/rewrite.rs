//! Minimal, explicit edits of raw message heads.
//!
//! Only the request line and whole header lines can be changed; every other
//! byte (header case, order, spacing, line endings) is preserved.

use crate::head::trim_eol;

/// Returns `raw_head` with the first line replaced (when `first_line` is
/// given) and every header named in `drop` removed, including any obsolete
/// folded continuation lines that belong to it. `first_line` must not include
/// a line terminator; CRLF is appended.
pub fn rewrite_head(raw_head: &[u8], first_line: Option<&[u8]>, drop: &[&str]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw_head.len());
    let mut lines = split_lines(raw_head);
    if let Some(first) = lines.next() {
        match first_line {
            Some(line) => {
                out.extend_from_slice(line);
                out.extend_from_slice(b"\r\n");
            }
            None => out.extend_from_slice(first),
        }
    }
    let mut dropping = false;
    for line in lines {
        let content = trim_eol(line);
        if content.is_empty() {
            out.extend_from_slice(line);
            continue;
        }
        if matches!(content[0], b' ' | b'\t') {
            if !dropping {
                out.extend_from_slice(line);
            }
            continue;
        }
        let name = content
            .split(|b| *b == b':')
            .next()
            .unwrap_or_default()
            .trim_ascii();
        dropping = drop.iter().any(|d| d.as_bytes().eq_ignore_ascii_case(name));
        if !dropping {
            out.extend_from_slice(line);
        }
    }
    out
}

/// Splits into lines, each including its terminator.
fn split_lines(buf: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = buf;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = memchr::memchr(b'\n', rest).map_or(rest.len(), |i| i + 1);
        let (line, tail) = rest.split_at(end);
        rest = tail;
        Some(line)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_request_line_and_drops_headers() {
        let raw = b"GET http://h/x HTTP/1.1\r\nHost: h\r\nProxy-Connection: keep-alive\r\nX-Folded: a\r\n b\r\nproxy-connection: again\r\nAccept: */*\r\n\r\n";
        let out = rewrite_head(
            raw,
            Some(b"GET /x HTTP/1.1"),
            &["Proxy-Connection", "x-folded"],
        );
        assert_eq!(out, b"GET /x HTTP/1.1\r\nHost: h\r\nAccept: */*\r\n\r\n");
    }

    #[test]
    fn untouched_when_nothing_to_do() {
        let raw = b"GET / HTTP/1.1\nHost:  h \n\n";
        assert_eq!(rewrite_head(raw, None, &[]), raw);
    }
}
