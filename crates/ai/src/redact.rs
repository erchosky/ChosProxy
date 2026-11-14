//! Redaction: credentials and secrets are replaced by `[REDACTED:kind]`
//! before any text reaches a model, and the tester sees what was hidden.

use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;
use ts_rs::TS;

/// How many values of one kind were hidden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AiRedaction")]
pub struct Redaction {
    /// What was hidden (`authorization`, `cookie`, `jwt`, …).
    pub kind: String,
    /// How many times.
    pub count: u32,
}

/// What to hide besides the built-in kinds.
#[derive(Debug, Clone, Default)]
pub struct RedactRules {
    /// The tester's own patterns (every match is hidden).
    pub custom: Vec<Regex>,
}

impl RedactRules {
    /// Rules from pattern strings; invalid ones are reported.
    pub fn new(patterns: &[String]) -> Result<RedactRules, String> {
        let custom = patterns
            .iter()
            .filter(|p| !p.trim().is_empty())
            .map(|p| Regex::new(p).map_err(|e| format!("redaction pattern {p:?}: {e}")))
            .collect::<Result<_, _>>()?;
        Ok(RedactRules { custom })
    }
}

#[allow(clippy::unwrap_used)] // literal patterns, checked by the tests
static AUTH_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)^((?:proxy-)?authorization|x-api-key|api-key|x-auth-token|x-access-token|x-csrf-token|x-xsrf-token)(\s*:\s*)(?:([A-Za-z]+)\s+)?([^\r\n]+)",
    )
    .unwrap()
});
#[allow(clippy::unwrap_used)]
static COOKIE_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?im)^(cookie\s*:\s*)([^\r\n]*)").unwrap());
#[allow(clippy::unwrap_used)]
static SET_COOKIE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?im)^(set-cookie\s*:\s*[^=;\r\n]+=)([^;\r\n]*)").unwrap());
#[allow(clippy::unwrap_used)]
static JWT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]*").unwrap()
});
const SECRET_NAMES: &str = r"(?:password|passwd|pwd|pass|passphrase|secret|client_secret|api_?key|apikey|access_?token|refresh_?token|id_token|auth_?token|token|session_?id|sessionid|otp|pin|ssn|credit_?card|card_?number|cvv)";
#[allow(clippy::unwrap_used)]
static PARAM_SECRET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)([?&\s;]|^){SECRET_NAMES}=[^&\s;]*")).unwrap());
#[allow(clippy::unwrap_used)]
static JSON_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?i)("{SECRET_NAMES}"\s*:\s*)("(?:[^"\\]|\\.)*"|[0-9]+)"#
    ))
    .unwrap()
});
#[allow(clippy::unwrap_used)]
static KNOWN_KEYS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        ("aws-key", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
        ("google-key", r"\bAIza[0-9A-Za-z_\-]{35}\b"),
        ("github-token", r"\bgh[pousr]_[0-9A-Za-z]{36,}\b"),
        ("slack-token", r"\bxox[abposr]-[0-9A-Za-z\-]{10,}\b"),
        ("stripe-key", r"\b[sr]k_(?:live|test)_[0-9A-Za-z]{16,}\b"),
        (
            "private-key",
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
        ),
    ]
    .into_iter()
    .map(|(k, r)| (k, Regex::new(r).unwrap()))
    .collect()
});

/// Hides credentials and secrets in `text`; returns the redacted text and
/// what was hidden.
pub fn redact(text: &str, rules: &RedactRules) -> (String, Vec<Redaction>) {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut bump = |kind: &str| *counts.entry(kind.to_string()).or_insert(0) += 1;

    let mut out = AUTH_HEADER
        .replace_all(text, |c: &regex::Captures<'_>| {
            let name = c[1].to_ascii_lowercase();
            bump(&name);
            let scheme = c
                .get(3)
                .map(|s| format!("{} ", s.as_str()))
                .unwrap_or_default();
            format!("{}{}{scheme}[REDACTED:{name}]", &c[1], &c[2])
        })
        .into_owned();
    out = COOKIE_HEADER
        .replace_all(&out, |c: &regex::Captures<'_>| {
            let values: Vec<String> = c[2]
                .split(';')
                .map(|pair| match pair.split_once('=') {
                    Some((name, _)) => {
                        bump("cookie");
                        format!("{name}=[REDACTED:cookie]")
                    }
                    None => pair.to_string(),
                })
                .collect();
            format!("{}{}", &c[1], values.join(";"))
        })
        .into_owned();
    out = SET_COOKIE
        .replace_all(&out, |c: &regex::Captures<'_>| {
            bump("cookie");
            format!("{}[REDACTED:cookie]", &c[1])
        })
        .into_owned();
    for (kind, re) in KNOWN_KEYS.iter() {
        out = re
            .replace_all(&out, |_: &regex::Captures<'_>| {
                bump(kind);
                format!("[REDACTED:{kind}]")
            })
            .into_owned();
    }
    out = JWT
        .replace_all(&out, |_: &regex::Captures<'_>| {
            bump("jwt");
            "[REDACTED:jwt]".to_string()
        })
        .into_owned();
    out = PARAM_SECRET
        .replace_all(&out, |c: &regex::Captures<'_>| {
            let whole = c.get(0).map_or("", |m| m.as_str());
            if whole.contains("[REDACTED") {
                return whole.to_string();
            }
            bump("secret-parameter");
            let (name, _) = whole.split_once('=').unwrap_or((whole, ""));
            format!("{name}=[REDACTED:secret]")
        })
        .into_owned();
    out = JSON_SECRET
        .replace_all(&out, |c: &regex::Captures<'_>| {
            if c[2].contains("[REDACTED") {
                return c[0].to_string();
            }
            bump("secret-field");
            format!("{}\"[REDACTED:secret]\"", &c[1])
        })
        .into_owned();
    for re in &rules.custom {
        out = re
            .replace_all(&out, |_: &regex::Captures<'_>| {
                bump("custom");
                "[REDACTED:custom]".to_string()
            })
            .into_owned();
    }
    let report = counts
        .into_iter()
        .map(|(kind, count)| Redaction { kind, count })
        .collect();
    (out, report)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn hides_credentials_in_headers_bodies_and_known_formats() {
        let text = "POST /login?next=/a&token=abc123 HTTP/1.1\r\nHost: shop.test\r\n\
                    Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln\r\n\
                    Cookie: sid=s3cr3t; theme=dark\r\nX-Api-Key: k-123\r\n\r\n\
                    {\"user\":\"bob\",\"password\":\"hunter2\",\"key\":\"AKIAIOSFODNN7EXAMPLE\"}";
        let (out, report) = redact(text, &RedactRules::default());
        for secret in [
            "hunter2",
            "s3cr3t",
            "dark",
            "k-123",
            "abc123",
            "AKIAIOSFODNN7EXAMPLE",
            "eyJhbGci",
        ] {
            assert!(!out.contains(secret), "{secret} leaked: {out}");
        }
        assert!(
            out.contains("Authorization: Bearer [REDACTED:authorization]"),
            "{out}"
        );
        assert!(out.contains("Cookie: sid=[REDACTED:cookie]; theme=[REDACTED:cookie]"));
        assert!(out.contains("next=/a"), "other parameters stay");
        assert!(out.contains("\"user\":\"bob\""));
        let kinds: Vec<&str> = report.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(
            kinds,
            [
                "authorization",
                "aws-key",
                "cookie",
                "secret-field",
                "secret-parameter",
                "x-api-key"
            ]
        );
        assert_eq!(report.iter().find(|r| r.kind == "cookie").unwrap().count, 2);
    }

    #[test]
    fn hides_set_cookie_values_jwts_keys_and_custom_patterns() {
        let text = "HTTP/1.1 200 OK\nSet-Cookie: session=abcdef; Path=/; HttpOnly\n\n\
                    token in body eyJhbGciOiJub25lIn0.eyJzdWIiOiJ4In0. and \
                    -----BEGIN RSA PRIVATE KEY-----\nMIIB\n-----END RSA PRIVATE KEY----- employee 4711";
        let rules = RedactRules::new(&[r"employee \d+".into()]).unwrap();
        let (out, report) = redact(text, &rules);
        assert!(
            out.contains("Set-Cookie: session=[REDACTED:cookie]; Path=/; HttpOnly"),
            "{out}"
        );
        assert!(out.contains("[REDACTED:jwt]") && !out.contains("eyJzdWIi"));
        assert!(out.contains("[REDACTED:private-key]") && !out.contains("MIIB"));
        assert!(out.contains("[REDACTED:custom]") && !out.contains("4711"));
        assert_eq!(report.len(), 4);
        assert!(RedactRules::new(&["(".into()]).is_err());
        let (same, none) = redact("GET / HTTP/1.1\nHost: a\n\n", &RedactRules::default());
        assert_eq!(same, "GET / HTTP/1.1\nHost: a\n\n");
        assert!(none.is_empty());
    }
}
