//! Preparing captured messages for a report: lossy text, credentials
//! redacted, size-limited.

use crate::ReportMessage;

/// Headers whose values are replaced when redaction is on.
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
    "x-csrf-token",
    "x-xsrf-token",
];

const REDACTED: &str = "[redacted]";

/// Replaces the values of sensitive headers in the head of `text`. For
/// cookies the names are kept (`session=[redacted]`). Returns the text and
/// whether anything was replaced.
pub fn redact_message(text: &str) -> (String, bool) {
    // The head ends at the first blank line, whichever line ending it uses
    // (a body may contain the other one).
    let end = match (text.find("\r\n\r\n"), text.find("\n\n")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let (head, body) = match end {
        Some(i) => text.split_at(i),
        None => (text, ""),
    };
    let mut changed = false;
    // The previous header was sensitive: its folded continuation lines
    // (starting with a space or tab) are part of the secret value.
    let mut in_secret = false;
    let mut out = String::with_capacity(text.len());
    for (i, line) in head.split_inclusive('\n').enumerate() {
        if i > 0 && line.starts_with([' ', '\t']) {
            if !in_secret {
                out.push_str(line);
            }
            continue;
        }
        in_secret = false;
        let Some((name, value)) = line.split_once(':').filter(|_| i > 0) else {
            out.push_str(line);
            continue;
        };
        let lower = name.trim().to_ascii_lowercase();
        if !SENSITIVE_HEADERS.contains(&lower.as_str()) {
            out.push_str(line);
            continue;
        }
        in_secret = true;
        let eol = if line.ends_with("\r\n") {
            "\r\n"
        } else if line.ends_with('\n') {
            "\n"
        } else {
            ""
        };
        let value = value.trim();
        let replaced = match lower.as_str() {
            "cookie" => value
                .split(';')
                .map(|c| match c.trim().split_once('=') {
                    Some((n, _)) => format!("{n}={REDACTED}"),
                    None => REDACTED.to_string(),
                })
                .collect::<Vec<_>>()
                .join("; "),
            "set-cookie" => {
                let (pair, attrs) = value.split_once(';').unwrap_or((value, ""));
                let name = pair.split_once('=').map_or(pair, |(n, _)| n).trim();
                if attrs.is_empty() {
                    format!("{name}={REDACTED}")
                } else {
                    format!("{name}={REDACTED};{attrs}")
                }
            }
            "authorization" | "proxy-authorization" => match value.split_once(' ') {
                Some((scheme, _)) => format!("{scheme} {REDACTED}"),
                None => REDACTED.to_string(),
            },
            _ => REDACTED.to_string(),
        };
        out.push_str(name);
        out.push_str(": ");
        out.push_str(&replaced);
        out.push_str(eol);
        changed = true;
    }
    out.push_str(body);
    (out, changed)
}

/// A captured message as report text: lossy UTF-8, optionally redacted,
/// cut to `max_bytes` on a character boundary.
pub fn prepare_message(raw: &[u8], redact: bool, max_bytes: usize) -> ReportMessage {
    let text = String::from_utf8_lossy(raw).into_owned();
    let (text, redacted) = if redact {
        redact_message(&text)
    } else {
        (text, false)
    };
    let truncated = text.len() > max_bytes;
    let text = if truncated {
        let mut end = max_bytes;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text[..end].to_string()
    } else {
        text
    };
    ReportMessage {
        text,
        redacted,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials_keeping_structure() {
        let raw = "GET / HTTP/1.1\r\nHost: lab.test\r\nAuthorization: Bearer abc.def\r\n\
                   Cookie: session=123; theme=dark\r\nX-Api-Key: k\r\n\r\nAuthorization: in body stays";
        let (out, changed) = redact_message(raw);
        assert!(changed);
        assert!(out.contains("Authorization: Bearer [redacted]\r\n"));
        assert!(out.contains("Cookie: session=[redacted]; theme=[redacted]\r\n"));
        assert!(out.contains("X-Api-Key: [redacted]\r\n"));
        assert!(out.contains("Host: lab.test\r\n"));
        assert!(
            out.ends_with("\r\n\r\nAuthorization: in body stays"),
            "body untouched"
        );
        assert!(!out.contains("abc.def") && !out.contains("123"));

        let (set, _) = redact_message("HTTP/1.1 200 OK\nSet-Cookie: sid=xyz; Path=/; HttpOnly\n\n");
        assert!(set.contains("Set-Cookie: sid=[redacted]; Path=/; HttpOnly\n"));
        let (none, changed) = redact_message("GET / HTTP/1.1\r\nHost: a\r\n\r\n");
        assert!(!changed && none == "GET / HTTP/1.1\r\nHost: a\r\n\r\n");
    }

    #[test]
    fn head_ends_at_the_first_blank_line() {
        // LF head, CRLF blank line only inside the body.
        let raw = "POST / HTTP/1.1\nCookie: a=1\n\nline\r\n\r\nCookie: b=2\r\n";
        let (out, changed) = redact_message(raw);
        assert!(changed);
        assert_eq!(
            out,
            "POST / HTTP/1.1\nCookie: a=[redacted]\n\nline\r\n\r\nCookie: b=2\r\n"
        );
    }

    #[test]
    fn folded_secret_values_are_removed() {
        let raw = "GET / HTTP/1.1\r\nAuthorization: Bearer\r\n abc.def\r\n\tmore\r\n\
                   X-Note: a\r\n folded\r\nHost: a\r\n\r\n";
        let (out, _) = redact_message(raw);
        assert_eq!(
            out,
            "GET / HTTP/1.1\r\nAuthorization: [redacted]\r\nX-Note: a\r\n folded\r\nHost: a\r\n\r\n"
        );
    }

    #[test]
    fn truncates_on_char_boundaries() {
        let m = prepare_message("héllo wörld".as_bytes(), false, 3);
        assert!(m.truncated);
        assert_eq!(m.text, "hé");
        let m = prepare_message(b"\xff\xfeok", false, 100);
        assert!(!m.truncated && m.text.ends_with("ok"));
    }
}
