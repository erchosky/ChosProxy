//! Finding JSON Web Tokens in text and reading their header and claims
//! (without verifying anything: the checks only look at what is visible).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

static JWT: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"\beyJ[A-Za-z0-9_-]{4,}\.eyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]*").ok()
});

/// A token found in a message.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Jwt {
    /// The token text.
    pub(crate) token: String,
    /// Decoded header.
    pub(crate) header: Map<String, Value>,
    /// Decoded claims.
    pub(crate) claims: Map<String, Value>,
    /// The signature part is empty.
    pub(crate) unsigned: bool,
}

impl Jwt {
    /// The `alg` header value.
    pub(crate) fn alg(&self) -> String {
        self.header
            .get("alg")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    /// A short form for evidence: the first characters and the length.
    pub(crate) fn short(&self) -> String {
        let head: String = self.token.chars().take(24).collect();
        format!("{head}… ({} characters)", self.token.len())
    }
}

fn decode_part(part: &str) -> Option<Map<String, Value>> {
    let bytes = URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).ok()?;
    match serde_json::from_slice(&bytes).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// Every well-formed token in `text` (at most 10).
pub(crate) fn find(text: &str) -> Vec<Jwt> {
    let Some(re) = JWT.as_ref() else {
        return Vec::new();
    };
    re.find_iter(text)
        .filter_map(|m| {
            let token = m.as_str();
            let mut parts = token.split('.');
            let header = decode_part(parts.next()?)?;
            let claims = decode_part(parts.next()?)?;
            let signature = parts.next().unwrap_or_default();
            Some(Jwt {
                token: token.to_string(),
                header,
                claims,
                unsigned: signature.is_empty(),
            })
        })
        .take(10)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    pub(crate) fn make(header: &str, claims: &str, signature: &str) -> String {
        format!(
            "{}.{}.{signature}",
            URL_SAFE_NO_PAD.encode(header),
            URL_SAFE_NO_PAD.encode(claims)
        )
    }

    #[test]
    fn finds_and_decodes_tokens() {
        let token = make(r#"{"alg":"none","typ":"JWT"}"#, r#"{"sub":"1"}"#, "");
        let text = format!("Authorization: Bearer {token}\r\nX: eyJnot.a.token");
        let found = find(&text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].alg(), "none");
        assert!(found[0].unsigned);
        assert_eq!(found[0].claims.get("sub"), Some(&Value::from("1")));
        assert!(found[0].short().ends_with("characters)"));
    }
}
