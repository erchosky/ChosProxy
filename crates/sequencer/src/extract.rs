//! Where a token is in a response, and finding candidates.

use chosproxy_http::ResponseHead;
use chosproxy_http::content::{body_text, split_message};
use chosproxy_http::cookies::SetCookie;
use regex::Regex;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Longest token kept.
pub const MAX_TOKEN_CHARS: usize = 1024;

/// Where to read the token in each response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "TokenLocation")]
pub enum TokenLocation {
    /// The value of a cookie set by the response.
    Cookie {
        /// Cookie name.
        name: String,
    },
    /// The value of a response header.
    Header {
        /// Header name.
        name: String,
    },
    /// The first group (or the whole match) of a regular expression on the
    /// response (head and decoded body).
    Regex {
        /// Pattern.
        pattern: String,
    },
}

impl TokenLocation {
    /// Checks the location (e.g. that a regex compiles).
    pub fn validate(&self) -> Result<(), String> {
        match self {
            TokenLocation::Cookie { name } | TokenLocation::Header { name } => {
                if name.trim().is_empty() {
                    Err("enter the cookie or header name".into())
                } else {
                    Ok(())
                }
            }
            TokenLocation::Regex { pattern } => Regex::new(pattern)
                .map(|_| ())
                .map_err(|e| format!("invalid pattern: {e}")),
        }
    }

    /// The token in a raw response, if present.
    pub fn extract(&self, response: &[u8]) -> Option<String> {
        let (head_bytes, raw_body) = split_message(response);
        let head = ResponseHead::parse(head_bytes.to_vec()).ok()?;
        let token = match self {
            TokenLocation::Cookie { name } => head
                .headers
                .iter()
                .filter(|h| h.name.eq_ignore_ascii_case("set-cookie"))
                .filter_map(|h| SetCookie::parse(&String::from_utf8_lossy(&h.value), 0))
                .find(|c| c.name == name.trim())
                .map(|c| c.value.trim_matches('"').to_string()),
            TokenLocation::Header { name } => head
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name.trim()))
                .map(|h| String::from_utf8_lossy(&h.value).trim().to_string()),
            TokenLocation::Regex { pattern } => {
                let re = Regex::new(pattern).ok()?;
                let mut text = String::from_utf8_lossy(head_bytes).into_owned();
                text.push_str(&body_text(&head, raw_body, 4 * 1024 * 1024).unwrap_or_default());
                re.captures(&text)
                    .and_then(|c| c.get(1).or_else(|| c.get(0)))
                    .map(|m| m.as_str().to_string())
            }
        }?;
        if token.is_empty() {
            return None;
        }
        Some(token.chars().take(MAX_TOKEN_CHARS).collect())
    }
}

/// A token candidate found in a sample response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TokenCandidate {
    /// Where it is.
    pub location: TokenLocation,
    /// Its value in the sample.
    pub sample: String,
}

/// Values that look like tokens: long enough and mostly letters and digits.
fn token_like(v: &str) -> bool {
    v.len() >= 8
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_=+/.%:".contains(c))
}

/// Cookies set by a response and token-like headers and hidden fields,
/// in that order.
pub fn suggest(response: &[u8]) -> Vec<TokenCandidate> {
    let (head_bytes, raw_body) = split_message(response);
    let Ok(head) = ResponseHead::parse(head_bytes.to_vec()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for h in &head.headers {
        let value = String::from_utf8_lossy(&h.value).into_owned();
        if h.name.eq_ignore_ascii_case("set-cookie") {
            if let Some(c) = SetCookie::parse(&value, 0)
                && !c.value.is_empty()
            {
                out.push(TokenCandidate {
                    location: TokenLocation::Cookie { name: c.name },
                    sample: c.value,
                });
            }
        } else if token_like(value.trim())
            && ![
                "date",
                "etag",
                "content-type",
                "content-length",
                "server",
                "last-modified",
            ]
            .contains(&h.name.to_ascii_lowercase().as_str())
        {
            out.push(TokenCandidate {
                location: TokenLocation::Header {
                    name: h.name.clone(),
                },
                sample: value.trim().to_string(),
            });
        }
    }
    let body = body_text(&head, raw_body, 1024 * 1024).unwrap_or_default();
    if let Ok(hidden) = Regex::new(
        r#"(?i)<input\b[^>]*\bname\s*=\s*["']([^"']+)["'][^>]*\bvalue\s*=\s*["']([^"']{8,})["']"#,
    ) {
        for c in hidden.captures_iter(&body).take(10) {
            let (Some(name), Some(value)) = (c.get(1), c.get(2)) else {
                continue;
            };
            if token_like(value.as_str()) {
                out.push(TokenCandidate {
                    location: TokenLocation::Regex {
                        pattern: format!(
                            r#"name=["']{}["'][^>]*value=["']([^"']+)"#,
                            regex::escape(name.as_str())
                        ),
                    },
                    sample: value.as_str().to_string(),
                });
            }
        }
    }
    out.truncate(20);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nSet-Cookie: SID=\"a1b2c3d4e5\"; Path=/\r\n\
        X-Request-Token: QWERTY123456\r\nDate: Wed, 11 Nov 2026 00:00:00 GMT\r\n\
        Content-Type: text/html\r\n\r\n<form><input type=hidden name=\"csrf\" value=\"Zx9Yw8Vu7Ts6\"></form>";

    #[test]
    fn extracts_from_cookies_headers_and_regexes() {
        let cookie = TokenLocation::Cookie { name: "SID".into() };
        assert_eq!(cookie.extract(RESPONSE).as_deref(), Some("a1b2c3d4e5"));
        let header = TokenLocation::Header {
            name: "x-request-token".into(),
        };
        assert_eq!(header.extract(RESPONSE).as_deref(), Some("QWERTY123456"));
        let regex = TokenLocation::Regex {
            pattern: r#"value="(\w+)""#.into(),
        };
        assert_eq!(regex.extract(RESPONSE).as_deref(), Some("Zx9Yw8Vu7Ts6"));
        assert!(
            TokenLocation::Cookie { name: "x".into() }
                .extract(RESPONSE)
                .is_none()
        );
        assert!(
            TokenLocation::Regex {
                pattern: "(".into()
            }
            .validate()
            .is_err()
        );
        assert!(
            TokenLocation::Header { name: " ".into() }
                .validate()
                .is_err()
        );
    }

    #[test]
    fn suggests_candidates() {
        let found = suggest(RESPONSE);
        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(
            found[0].location,
            TokenLocation::Cookie { name: "SID".into() }
        );
        assert_eq!(found[1].sample, "QWERTY123456");
        // The suggested pattern reads the hidden field back.
        assert_eq!(
            found[2].location.extract(RESPONSE).as_deref(),
            Some("Zx9Yw8Vu7Ts6")
        );
    }
}
