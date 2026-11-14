//! JavaScript analysis (M25): what a script served by the application
//! tells a tester — endpoints it calls, an exposed source map, values that
//! look like secrets, and DOM sinks worth tracing. It reads the response
//! only; nothing is run or sent.

use crate::{CheckInfo, Confidence, Issue, Message, Severity};
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

/// Endpoints listed at most in one finding.
const MAX_ENDPOINTS: usize = 40;

/// Endpoints and URLs found in a script.
pub static JS_ENDPOINTS: CheckInfo = CheckInfo {
    id: "js-endpoints",
    kind: "Endpoints referenced in JavaScript",
    summary: "Lists the paths and URLs a script refers to.",
    severity: Severity::Info,
    description: "Scripts often name API endpoints and pages that no link points to. \
                  They are part of the attack surface: map them and test them like the rest.",
    remediation: "Nothing to fix by itself. Make sure every endpoint enforces \
                  authentication and authorization on the server.",
};

/// A `sourceMappingURL` comment.
pub static JS_SOURCE_MAP: CheckInfo = CheckInfo {
    id: "js-source-map",
    kind: "JavaScript source map referenced",
    summary: "Finds sourceMappingURL comments that point to the original sources.",
    severity: Severity::Low,
    description: "A source map rebuilds the original, readable code (with comments and file \
                  names) from minified JavaScript. Public source maps make it easier to find \
                  hidden features, internal endpoints and mistakes in client-side checks.",
    remediation: "Do not deploy source maps to production, or serve them only to your own \
                  staff.",
};

/// Values that look like credentials.
pub static JS_SECRETS: CheckInfo = CheckInfo {
    id: "js-secrets",
    kind: "Secret-looking value in JavaScript",
    summary: "Finds keys and tokens with known formats, and values assigned to names like apiKey.",
    severity: Severity::Medium,
    description: "Anything in a script is public. A cloud access key, a private key or an API \
                  token found there can be used by anyone who reads it. Some keys are meant to \
                  be public (browser map keys, for example); confirm what the value gives \
                  access to.",
    remediation: "Remove the secret from client code, revoke and replace it, and keep secrets \
                  on the server.",
};

/// DOM sinks in a script.
pub static JS_DOM_SINKS: CheckInfo = CheckInfo {
    id: "js-dom-sinks",
    kind: "DOM sinks in JavaScript",
    summary: "Lists uses of innerHTML, document.write, eval and similar sinks.",
    severity: Severity::Info,
    description: "These functions turn text into HTML or code. If data the user controls (the \
                  URL, a message, storage) reaches them, the page may be open to DOM-based \
                  cross-site scripting. Their presence alone is not a vulnerability.",
    remediation: "Prefer textContent and safe DOM APIs; if HTML is needed, sanitize it. Trace \
                  the data flowing into each sink (Tools → DOM Inspector).",
};

/// The JavaScript checks.
pub static JS_CHECKS: [&CheckInfo; 4] = [&JS_ENDPOINTS, &JS_SOURCE_MAP, &JS_SECRETS, &JS_DOM_SINKS];

#[allow(clippy::unwrap_used)] // literal patterns, checked by the tests
static STRING_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"["'`]((?:https?://[A-Za-z0-9.-]+(?::\d+)?)?/[A-Za-z0-9_\-./{}:]*[A-Za-z0-9_\-}](?:\?[A-Za-z0-9_\-=&%.{}]*)?)["'`]"#).unwrap()
});

#[allow(clippy::unwrap_used)]
static SOURCE_MAP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*//[#@]\s*sourceMappingURL=(\S+)\s*$").unwrap());

#[allow(clippy::unwrap_used)]
static KNOWN_SECRETS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        ("AWS access key id", r"\b(AKIA|ASIA)[0-9A-Z]{16}\b"),
        ("Google API key", r"\bAIza[0-9A-Za-z_\-]{35}\b"),
        ("Slack token", r"\bxox[abposr]-[0-9A-Za-z\-]{10,}\b"),
        ("GitHub token", r"\bgh[pousr]_[0-9A-Za-z]{36,}\b"),
        ("Stripe secret key", r"\b[sr]k_live_[0-9A-Za-z]{16,}\b"),
        (
            "Private key",
            r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |)PRIVATE KEY-----",
        ),
    ]
    .into_iter()
    .map(|(name, re)| (name, Regex::new(re).unwrap()))
    .collect()
});

#[allow(clippy::unwrap_used)]
static ASSIGNED_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b([A-Za-z_]*(?:api[_-]?key|secret|access[_-]?token|auth[_-]?token|client[_-]?secret|password))["']?\s*[:=]\s*["']([A-Za-z0-9_\-+/=.]{16,})["']"#).unwrap()
});

#[allow(clippy::unwrap_used)]
static SINKS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        ("innerHTML", r"\.innerHTML\s*\+?=[^=]"),
        ("outerHTML", r"\.outerHTML\s*\+?=[^=]"),
        ("insertAdjacentHTML", r"\.insertAdjacentHTML\s*\("),
        ("document.write", r"\bdocument\.write(?:ln)?\s*\("),
        ("eval", r"(?:^|[^.\w$])eval\s*\("),
        ("new Function", r"\bnew\s+Function\s*\("),
        (
            "setTimeout with a string",
            r#"\bset(?:Timeout|Interval)\s*\(\s*["'`]"#,
        ),
        ("jQuery html()", r"\.html\s*\(\s*[^)\s]"),
    ]
    .into_iter()
    .map(|(name, re)| (name, Regex::new(re).unwrap()))
    .collect()
});

/// Whether the response is a script.
pub fn is_javascript(msg: &Message) -> bool {
    let media = msg.media_type();
    if media.contains("javascript") || media.contains("ecmascript") {
        return true;
    }
    let path = msg.path_only().to_ascii_lowercase();
    (media.is_empty() || media == "text/plain" || media == "application/octet-stream")
        && (path.ends_with(".js") || path.ends_with(".mjs"))
}

/// The JavaScript issues of one exchange (none unless it is a script with a
/// successful response). `enabled` filters by check id.
pub fn analyze_javascript(msg: &Message, enabled: impl Fn(&str) -> bool) -> Vec<Issue> {
    let mut out = Vec::new();
    if !is_javascript(msg) || !(200..300).contains(&msg.status()) || msg.body.is_empty() {
        return out;
    }
    let url = format!("{}{}", msg.origin(), msg.path_only());
    let body = &msg.body;
    if enabled(JS_ENDPOINTS.id) {
        let found: BTreeSet<&str> = STRING_URL
            .captures_iter(body)
            .filter_map(|c| c.get(1).map(|m| m.as_str()))
            .filter(|s| looks_like_endpoint(s))
            .collect();
        if !found.is_empty() {
            let shown: Vec<&str> = found.iter().take(MAX_ENDPOINTS).copied().collect();
            let more = found.len().saturating_sub(shown.len());
            let mut evidence = shown.join("\n");
            if more > 0 {
                evidence.push_str(&format!("\n… and {more} more"));
            }
            out.push(Issue {
                check: &JS_ENDPOINTS,
                title: format!(
                    "{} endpoint(s) referenced in {}",
                    found.len(),
                    msg.path_only()
                ),
                severity: Severity::Info,
                confidence: Confidence::Firm,
                evidence,
                dedup: url.clone(),
            });
        }
    }
    if enabled(JS_SOURCE_MAP.id)
        && let Some(m) = SOURCE_MAP.captures(body).and_then(|c| c.get(1))
        && !m.as_str().starts_with("data:")
    {
        out.push(Issue {
            check: &JS_SOURCE_MAP,
            title: format!("Source map referenced by {}", msg.path_only()),
            severity: Severity::Low,
            confidence: Confidence::Certain,
            evidence: format!("sourceMappingURL={}", m.as_str()),
            dedup: url.clone(),
        });
    }
    if let Some(sourcemap) = msg
        .response_header("sourcemap")
        .or_else(|| msg.response_header("x-sourcemap"))
        && enabled(JS_SOURCE_MAP.id)
        && !out.iter().any(|i| i.check.id == JS_SOURCE_MAP.id)
    {
        out.push(Issue {
            check: &JS_SOURCE_MAP,
            title: format!("Source map referenced by {}", msg.path_only()),
            severity: Severity::Low,
            confidence: Confidence::Certain,
            evidence: format!("SourceMap: {sourcemap}"),
            dedup: url.clone(),
        });
    }
    if enabled(JS_SECRETS.id) {
        for (name, re) in KNOWN_SECRETS.iter() {
            if let Some(m) = re.find(body) {
                out.push(Issue {
                    check: &JS_SECRETS,
                    title: format!("{name} in {}", msg.path_only()),
                    severity: Severity::Medium,
                    confidence: Confidence::Firm,
                    evidence: mask(m.as_str()),
                    dedup: format!("{url}:{name}"),
                });
            }
        }
        for c in ASSIGNED_SECRET.captures_iter(body).take(5) {
            let (Some(name), Some(value)) = (c.get(1), c.get(2)) else {
                continue;
            };
            if looks_like_placeholder(value.as_str()) {
                continue;
            }
            out.push(Issue {
                check: &JS_SECRETS,
                title: format!("Value assigned to {} in {}", name.as_str(), msg.path_only()),
                severity: Severity::Low,
                confidence: Confidence::Tentative,
                evidence: format!("{} = {}", name.as_str(), mask(value.as_str())),
                dedup: format!("{url}:{}", name.as_str().to_ascii_lowercase()),
            });
        }
    }
    if enabled(JS_DOM_SINKS.id) {
        let found: Vec<String> = SINKS
            .iter()
            .filter_map(|(name, re)| {
                let count = re.find_iter(body).count();
                (count > 0).then(|| format!("{name} ×{count}"))
            })
            .collect();
        if !found.is_empty() {
            out.push(Issue {
                check: &JS_DOM_SINKS,
                title: format!("DOM sinks in {}", msg.path_only()),
                severity: Severity::Info,
                confidence: Confidence::Tentative,
                evidence: found.join(", "),
                dedup: url,
            });
        }
    }
    out
}

/// Paths that are endpoints rather than MIME types, regexes or comments.
fn looks_like_endpoint(s: &str) -> bool {
    let path = s
        .split_once("://")
        .map_or(s, |(_, rest)| rest.find('/').map_or("", |i| &rest[i..]));
    if s.starts_with("//") || path.len() < 2 || path.contains("//") {
        return false;
    }
    let first = path
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or_default();
    // `/text/html`-like MIME fragments and single-letter regex pieces.
    !matches!(
        first,
        "text" | "application" | "image" | "audio" | "video" | "font" | "multipart"
    ) && path.chars().filter(char::is_ascii_alphabetic).count() >= 2
}

fn looks_like_placeholder(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "xxxx",
        "your",
        "example",
        "change",
        "placeholder",
        "dummy",
        "test",
        "****",
    ]
    .iter()
    .any(|p| lower.contains(p))
        || value.chars().collect::<BTreeSet<_>>().len() < 5
}

/// The first and last characters of a secret, the middle hidden.
fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 12 {
        return format!("{}…", chars.iter().take(4).collect::<String>());
    }
    format!(
        "{}…{} ({} characters)",
        chars.iter().take(6).collect::<String>(),
        chars[chars.len() - 4..].iter().collect::<String>(),
        chars.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(body: &str) -> Message {
        script_typed(body, "application/javascript")
    }

    fn script_typed(body: &str, media: &str) -> Message {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {media}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        Message::new(
            "https",
            "app.test",
            443,
            "/static/app.js",
            b"GET /static/app.js HTTP/1.1\r\nHost: app.test\r\n\r\n",
            Some(response.as_bytes()),
        )
    }

    fn ids(issues: &[Issue]) -> Vec<&str> {
        issues.iter().map(|i| i.check.id).collect()
    }

    #[test]
    fn lists_endpoints_but_not_mime_types_or_comments() {
        let issues = analyze_javascript(
            &script(
                r#"fetch("/api/v1/users?page=1"); const u = 'https://api.app.test/v2/orders';
                   const t = "text/html"; // see http://x/ ; const r = "/"; x("//cdn.test/a.js")"#,
            ),
            |_| true,
        );
        let endpoints = issues
            .iter()
            .find(|i| i.check.id == "js-endpoints")
            .map(|i| i.evidence.clone());
        assert_eq!(
            endpoints.as_deref(),
            Some("/api/v1/users?page=1\nhttps://api.app.test/v2/orders")
        );
    }

    #[test]
    fn finds_source_maps_secrets_and_sinks() {
        let body = "var k='AKIAIOSFODNN7EXAMPLE';\nconst apiKey = \"9f8e7d6c5b4a39281706f5e4\";\n\
                    const password = 'changeme-changeme';\nel.innerHTML = location.hash;\n\
                    if (a == b) {}\neval(x); obj.eval(y);\n//# sourceMappingURL=app.js.map\n";
        let issues = analyze_javascript(&script(body), |_| true);
        assert_eq!(
            ids(&issues),
            ["js-source-map", "js-secrets", "js-secrets", "js-dom-sinks"]
        );
        let aws = &issues[1];
        assert_eq!(aws.title, "AWS access key id in /static/app.js");
        assert!(
            !aws.evidence.contains("IOSFODNN7EXAM"),
            "the secret is masked: {}",
            aws.evidence
        );
        assert_eq!(issues[2].confidence, Confidence::Tentative);
        assert!(issues[2].title.contains("apiKey"));
        assert_eq!(issues[3].evidence, "innerHTML ×1, eval ×1");
        // Checks can be turned off one by one.
        let only = analyze_javascript(&script(body), |id| id == "js-dom-sinks");
        assert_eq!(ids(&only), ["js-dom-sinks"]);
    }

    #[test]
    fn only_successful_scripts_are_read() {
        let html = script_typed("el.innerHTML = x;", "text/html");
        assert!(analyze_javascript(&html, |_| true).is_empty());
        let by_extension = script_typed("el.innerHTML = x;", "text/plain");
        assert_eq!(
            ids(&analyze_javascript(&by_extension, |_| true)),
            ["js-dom-sinks"]
        );
        let inline_map = script("//# sourceMappingURL=data:application/json;base64,e30=");
        assert!(analyze_javascript(&inline_map, |_| true).is_empty());
    }
}
