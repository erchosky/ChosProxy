//! Redaction of secrets before anything is written to application logs.
//!
//! Captured traffic is stored in the project database on purpose; *logs* are
//! a different matter and must never contain credentials by default.

use std::borrow::Cow;

/// Header names whose values are secrets.
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

/// Returns true if values of this header must be redacted in logs.
pub fn is_sensitive_header(name: &str) -> bool {
    SENSITIVE_HEADERS
        .iter()
        .any(|h| h.eq_ignore_ascii_case(name))
}

/// Returns the value to log for a header: unchanged, or a placeholder that
/// only reveals the length.
pub fn header_value_for_log<'a>(name: &str, value: &'a str) -> Cow<'a, str> {
    if is_sensitive_header(name) {
        Cow::Owned(format!("[redacted {} bytes]", value.len()))
    } else {
        Cow::Borrowed(value)
    }
}

/// Redacts the query string of a URL (often carries tokens) for logging.
pub fn url_for_log(url: &str) -> Cow<'_, str> {
    match url.find('?') {
        Some(i) => Cow::Owned(format!("{}?[redacted]", &url[..i])),
        None => Cow::Borrowed(url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_case_insensitively() {
        assert!(is_sensitive_header("Authorization"));
        assert!(is_sensitive_header("COOKIE"));
        assert!(!is_sensitive_header("Content-Type"));
        assert_eq!(header_value_for_log("Cookie", "a=b"), "[redacted 3 bytes]");
        assert_eq!(header_value_for_log("Accept", "*/*"), "*/*");
    }

    #[test]
    fn redacts_query() {
        assert_eq!(url_for_log("http://a/b?token=x"), "http://a/b?[redacted]");
        assert_eq!(url_for_log("http://a/b"), "http://a/b");
    }
}
