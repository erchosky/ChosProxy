//! Cross-request analysis (M26): passive checks that need more than one
//! request/response pair. A [`CrossAnalyzer`] is fed the stored traffic in
//! order and remembers a little about each origin (bounded):
//!
//! - a session cookie sent to a login form, not renewed by its response,
//!   and still accepted afterwards (session fixation risk);
//! - a URL carrying a token, sent in the `Referer` of a request to another
//!   site;
//! - a security header that some HTML pages of an origin send and others
//!   do not.
//!
//! Like the other passive checks it only reads traffic; nothing is sent.

use crate::checks::session_like;
use crate::{CheckInfo, Confidence, Issue, Message, Severity};
use chosproxy_http::cookies::SetCookie;
use regex::Regex;
use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;

/// Origins remembered at most (older state is forgotten past this).
const MAX_ORIGINS: usize = 1000;
/// Token-bearing pages remembered at most.
const MAX_TOKEN_PAGES: usize = 2000;
/// Pages without a header kept per origin until a page with it shows up.
const MAX_PENDING: usize = 5;
/// Shortest parameter value treated as a token.
const MIN_TOKEN_LEN: usize = 8;

/// A session kept across a login.
pub static CROSS_SESSION_NOT_RENEWED: CheckInfo = CheckInfo {
    id: "cross-session-not-renewed",
    kind: "Session token not renewed at login",
    summary: "A session cookie sent to a login is not replaced by it and still works after it.",
    severity: Severity::Medium,
    description: "The browser sent a session cookie with a successful login, the login response \
                  did not set a new value, and the same value was accepted afterwards. If an \
                  attacker can plant a known session id in a victim's browser before they log \
                  in (session fixation), the attacker shares the logged-in session. Confirm by \
                  logging in with a session id you chose.",
    remediation: "Issue a new session identifier when the user logs in (and when privileges \
                  change), and invalidate the old one on the server.",
};

/// A token leaked through the Referer header.
pub static CROSS_REFERER_LEAK: CheckInfo = CheckInfo {
    id: "cross-referer-leak",
    kind: "Token in URL sent to another site",
    summary: "A URL with a token or session parameter is sent as Referer to a different host.",
    severity: Severity::Medium,
    description: "Browsers send the address of the current page in the Referer header when it \
                  loads images, scripts or links from other sites. A token in that address \
                  (a session id, a reset code, an API key) is then known to the other site and \
                  ends up in its logs.",
    remediation: "Keep tokens out of URLs (use headers or POST bodies), and set \
                  `Referrer-Policy: strict-origin-when-cross-origin` or stricter so other sites \
                  receive the origin only.",
};

/// Security headers set on some pages only.
pub static CROSS_INCONSISTENT_HEADERS: CheckInfo = CheckInfo {
    id: "cross-inconsistent-headers",
    kind: "Security header missing on some pages",
    summary: "An origin sends HSTS, CSP or anti-framing headers on some HTML pages but not others.",
    severity: Severity::Low,
    description: "Some HTML pages of this origin send a security header and others do not. This \
                  usually means the header is added by one part of the application (a framework, \
                  a route, a proxy rule) and not by the rest; the pages without it lose that \
                  protection.",
    remediation: "Set the header for the whole origin, in one place (the web server, the \
                  framework's middleware or the reverse proxy).",
};

/// The cross-request checks.
pub static CROSS_CHECKS: [&CheckInfo; 3] = [
    &CROSS_SESSION_NOT_RENEWED,
    &CROSS_REFERER_LEAK,
    &CROSS_INCONSISTENT_HEADERS,
];

/// Query parameter names that carry tokens.
const TOKEN_PARAMS: &[&str] = &[
    "token",
    "access_token",
    "auth_token",
    "id_token",
    "refresh_token",
    "session",
    "sessionid",
    "session_id",
    "sid",
    "jsessionid",
    "phpsessid",
    "api_key",
    "apikey",
    "key",
    "secret",
    "password",
    "reset",
    "reset_token",
    "auth",
    "signature",
    "sig",
];

/// Whether a page sends a header (`None`: does not apply to it).
type Presence = fn(&Message) -> Option<bool>;

/// The security headers compared across pages: (label, present?).
const HEADERS: [(&str, Presence); 3] = [
    ("Strict-Transport-Security", hsts),
    ("Content-Security-Policy", csp),
    (
        "anti-framing (X-Frame-Options or frame-ancestors)",
        anti_framing,
    ),
];

fn hsts(m: &Message) -> Option<bool> {
    m.https()
        .then(|| m.response_header("strict-transport-security").is_some())
}

fn csp(m: &Message) -> Option<bool> {
    Some(m.response_header("content-security-policy").is_some())
}

fn anti_framing(m: &Message) -> Option<bool> {
    let frame_ancestors = m
        .response_header("content-security-policy")
        .is_some_and(|v| v.to_ascii_lowercase().contains("frame-ancestors"));
    Some(m.response_header("x-frame-options").is_some() || frame_ancestors)
}

#[allow(clippy::unwrap_used)] // literal pattern, checked by the tests
static PASSWORD_FIELD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:^|[&?{,\s"])(?:pass(?:word|wd)?|pwd|user_?pass)"?\s*[=:]"#).unwrap()
});

/// A session cookie value that went to a login without being renewed.
#[derive(Debug, Clone)]
struct KeptSession {
    name: String,
    value: String,
    login_id: i64,
    login_path: String,
}

/// A security header seen on an origin's HTML pages.
#[derive(Debug, Default, Clone)]
struct HeaderState {
    /// A page that sends it (path, exchange).
    with: Option<(String, i64)>,
    /// Pages without it seen before any page with it.
    pending: Vec<(String, i64)>,
}

#[derive(Debug, Default)]
struct OriginState {
    kept: Vec<KeptSession>,
    headers: [HeaderState; 3],
}

/// Follows traffic message by message and reports issues that need
/// several of them. Feed it in exchange id order.
#[derive(Debug, Default)]
pub struct CrossAnalyzer {
    origins: HashMap<String, OriginState>,
    /// Pages whose URL carried a token: URL without fragment → exchange.
    token_pages: HashMap<String, i64>,
    token_order: VecDeque<String>,
}

/// Parameters of a query string with token-like names and long values.
fn token_params(query: &str) -> Vec<String> {
    query
        .split('&')
        .filter_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            let name = name.to_ascii_lowercase();
            (TOKEN_PARAMS.contains(&name.as_str()) && value.len() >= MIN_TOKEN_LEN).then_some(name)
        })
        .collect()
}

/// Splits `scheme://host[:port]/path?query` into (host, rest) — enough for
/// a Referer. `None` for anything else.
fn split_url(url: &str) -> Option<(String, &str)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = if authority.starts_with('[') {
        authority.split(']').next().map(|h| format!("{h}]"))?
    } else {
        authority.split(':').next()?.to_string()
    };
    Some((host.to_ascii_lowercase(), &rest[end..]))
}

/// The cookies of a request's `Cookie` header.
fn request_cookies(m: &Message) -> Vec<(String, String)> {
    m.request_header("cookie")
        .map(|v| {
            v.split(';')
                .filter_map(|c| {
                    let (n, v) = c.trim().split_once('=')?;
                    Some((n.trim().to_string(), v.trim().to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn is_login(m: &Message, request_body: &[u8]) -> bool {
    let post = m.request.as_ref().is_some_and(|r| r.method == "POST");
    let ok = (200..400).contains(&m.status());
    post && ok && PASSWORD_FIELD.is_match(&String::from_utf8_lossy(request_body))
}

fn new_issue(
    check: &'static CheckInfo,
    title: String,
    confidence: Confidence,
    evidence: String,
    dedup: String,
) -> Issue {
    Issue {
        check,
        title,
        severity: check.severity,
        confidence,
        evidence,
        dedup,
    }
}

impl CrossAnalyzer {
    /// An analyzer that has seen nothing yet.
    pub fn new() -> CrossAnalyzer {
        CrossAnalyzer::default()
    }

    /// Looks at one exchange (`exchange_id`, its message and its request
    /// body) and returns the issues it completes, each with the exchange
    /// that shows it (usually this one, sometimes one seen earlier).
    pub fn observe(
        &mut self,
        exchange_id: i64,
        m: &Message,
        request_body: &[u8],
        enabled: impl Fn(&str) -> bool,
    ) -> Vec<(i64, Issue)> {
        let mut out = Vec::new();
        if m.response.is_none() {
            return out;
        }
        if self.origins.len() >= MAX_ORIGINS && !self.origins.contains_key(&m.origin()) {
            self.origins.clear();
        }
        if enabled(CROSS_SESSION_NOT_RENEWED.id) {
            self.sessions(exchange_id, m, request_body, &mut out);
        }
        if enabled(CROSS_REFERER_LEAK.id) {
            self.referer(exchange_id, m, &mut out);
        }
        if enabled(CROSS_INCONSISTENT_HEADERS.id) {
            self.headers(exchange_id, m, &mut out);
        }
        out
    }

    fn sessions(&mut self, id: i64, m: &Message, request_body: &[u8], out: &mut Vec<(i64, Issue)>) {
        let origin = m.origin();
        let cookies = request_cookies(m);
        let state = self.origins.entry(origin.clone()).or_default();
        // A later request still using a value kept across a login.
        if !state.kept.is_empty() && m.status() < 400 {
            let mut still = Vec::new();
            for kept in std::mem::take(&mut state.kept) {
                let used = cookies
                    .iter()
                    .any(|(n, v)| *n == kept.name && *v == kept.value);
                if used && kept.login_id != id {
                    out.push((
                        id,
                        new_issue(
                            &CROSS_SESSION_NOT_RENEWED,
                            format!("Session cookie {} not renewed at login", kept.name),
                            Confidence::Firm,
                            format!(
                                "{} was sent to the login {} (#{}), the login did not set a new \
                                 value, and the same value was accepted here.",
                                kept.name, kept.login_path, kept.login_id
                            ),
                            format!("{origin}:{}", kept.name),
                        ),
                    ));
                } else {
                    still.push(kept);
                }
            }
            state.kept = still;
        }
        if !is_login(m, request_body) {
            return;
        }
        let set: Vec<SetCookie> = m
            .response_headers("set-cookie")
            .iter()
            .filter_map(|f| SetCookie::parse(f, 0))
            .collect();
        for (name, value) in cookies.iter().filter(|(n, _)| session_like(n)) {
            let renewed = set.iter().any(|c| c.name == *name && c.value != *value);
            if !renewed {
                state.kept.retain(|k| k.name != *name);
                state.kept.push(KeptSession {
                    name: name.clone(),
                    value: value.clone(),
                    login_id: id,
                    login_path: m.path_only().to_string(),
                });
            }
        }
        state.kept.truncate(20);
    }

    fn referer(&mut self, id: i64, m: &Message, out: &mut Vec<(i64, Issue)>) {
        let page = format!(
            "{}://{}{}",
            m.scheme,
            m.host,
            m.path.split('#').next().unwrap_or_default()
        );
        if m.query().is_some_and(|q| !token_params(q).is_empty())
            && !self.token_pages.contains_key(&page)
        {
            if self.token_order.len() >= MAX_TOKEN_PAGES
                && let Some(old) = self.token_order.pop_front()
            {
                self.token_pages.remove(&old);
            }
            self.token_order.push_back(page.clone());
            self.token_pages.insert(page, id);
        }
        let Some(referer) = m.request_header("referer") else {
            return;
        };
        let Some((host, rest)) = split_url(&referer) else {
            return;
        };
        if host == m.host {
            return;
        }
        let Some(query) = rest
            .split_once('?')
            .map(|(_, q)| q.split('#').next().unwrap_or(q))
        else {
            return;
        };
        let names = token_params(query);
        if names.is_empty() {
            return;
        }
        let referer_page = referer.split('#').next().unwrap_or_default().to_string();
        let seen = self.token_pages.get(&referer_page).copied();
        let path = rest.split('?').next().unwrap_or("/");
        out.push((
            id,
            new_issue(
                &CROSS_REFERER_LEAK,
                format!(
                    "{} in the URL of {host} sent to {}",
                    names.join(", "),
                    m.host
                ),
                if seen.is_some() {
                    Confidence::Firm
                } else {
                    Confidence::Tentative
                },
                match seen {
                    Some(page_id) => format!(
                        "The page {host}{path} (#{page_id}) had {} in its URL; this request to \
                         {} carried that URL as Referer.",
                        names.join(", "),
                        m.host
                    ),
                    None => format!(
                        "This request to {} carried a Referer from {host}{path} with {} in it.",
                        m.host,
                        names.join(", ")
                    ),
                },
                format!("{host}{path}:{}", m.host),
            ),
        ));
    }

    fn headers(&mut self, id: i64, m: &Message, out: &mut Vec<(i64, Issue)>) {
        if !m.is_html() || !(200..300).contains(&m.status()) {
            return;
        }
        let origin = m.origin();
        let path = m.path_only().to_string();
        let state = self.origins.entry(origin.clone()).or_default();
        for (i, (label, present)) in HEADERS.iter().enumerate() {
            let Some(present) = present(m) else {
                continue;
            };
            let h = &mut state.headers[i];
            let report = |page: &str, without_id: i64, with: &(String, i64)| {
                (
                    without_id,
                    new_issue(
                        &CROSS_INCONSISTENT_HEADERS,
                        format!("{label} missing on some pages of {origin}"),
                        Confidence::Certain,
                        format!("{page} has no {label}; {} (#{}) sends it.", with.0, with.1),
                        format!("{origin}:{label}"),
                    ),
                )
            };
            if present {
                if h.with.is_none() {
                    let with = (path.clone(), id);
                    for (page, page_id) in std::mem::take(&mut h.pending) {
                        out.push(report(&page, page_id, &with));
                    }
                    h.with = Some(with);
                }
            } else if let Some(with) = &h.with {
                out.push(report(&path, id, with));
            } else if h.pending.len() < MAX_PENDING && !h.pending.iter().any(|(p, _)| *p == path) {
                h.pending.push((path.clone(), id));
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn msg(host: &str, request: &str, response: &str) -> Message {
        let path = request.split(' ').nth(1).unwrap_or("/").to_string();
        Message::new(
            "https",
            host,
            443,
            &path,
            request.as_bytes(),
            Some(response.as_bytes()),
        )
    }

    fn all(_: &str) -> bool {
        true
    }

    #[test]
    fn a_session_kept_across_a_login_is_reported_once_it_is_used_again() {
        let mut a = CrossAnalyzer::new();
        let login = msg(
            "app.test",
            "POST /login HTTP/1.1\r\nHost: app.test\r\nCookie: SESSIONID=abc123\r\n\r\n",
            "HTTP/1.1 302 Found\r\nLocation: /home\r\n\r\n",
        );
        assert!(a.observe(1, &login, b"user=a&password=b", all).is_empty());
        let home = msg(
            "app.test",
            "GET /home HTTP/1.1\r\nHost: app.test\r\nCookie: SESSIONID=abc123\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nhi",
        );
        let found = a.observe(2, &home, b"", all);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, 2);
        assert_eq!(found[0].1.check.id, "cross-session-not-renewed");
        assert!(found[0].1.evidence.contains("#1"));
        // Reported once.
        assert!(a.observe(3, &home, b"", all).is_empty());

        // A login that renews the cookie is fine.
        let mut b = CrossAnalyzer::new();
        let renewing = msg(
            "app.test",
            "POST /login HTTP/1.1\r\nHost: app.test\r\nCookie: SESSIONID=abc123\r\n\r\n",
            "HTTP/1.1 302 Found\r\nSet-Cookie: SESSIONID=new999; HttpOnly\r\n\r\n",
        );
        b.observe(1, &renewing, b"{\"password\":\"x\"}", all);
        assert!(b.observe(2, &home, b"", all).is_empty());
        // So is a failed login, and a POST without a password.
        let mut c = CrossAnalyzer::new();
        c.observe(1, &login, b"q=search", all);
        assert!(c.observe(2, &home, b"", all).is_empty());
        // And the check can be turned off.
        let mut d = CrossAnalyzer::new();
        d.observe(1, &login, b"password=b", |id| {
            id != "cross-session-not-renewed"
        });
        assert!(d.observe(2, &home, b"", all).is_empty());
    }

    #[test]
    fn tokens_in_referers_to_other_hosts() {
        let mut a = CrossAnalyzer::new();
        let page = msg(
            "app.test",
            "GET /reset?token=0123456789abcdef HTTP/1.1\r\nHost: app.test\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<p>x</p>",
        );
        assert!(a.observe(1, &page, b"", all).is_empty());
        let image = msg(
            "cdn.other",
            "GET /logo.png HTTP/1.1\r\nHost: cdn.other\r\nReferer: https://app.test/reset?token=0123456789abcdef\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n\r\n",
        );
        let found = a.observe(2, &image, b"", all);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1.confidence, Confidence::Firm);
        assert!(found[0].1.evidence.contains("#1"));
        // Same host, short values, or no Referer: nothing.
        let same = msg(
            "app.test",
            "GET /a HTTP/1.1\r\nHost: app.test\r\nReferer: https://app.test/reset?token=0123456789abcdef\r\n\r\n",
            "HTTP/1.1 200 OK\r\n\r\n",
        );
        assert!(a.observe(3, &same, b"", all).is_empty());
        let short = msg(
            "cdn.other",
            "GET /b HTTP/1.1\r\nHost: cdn.other\r\nReferer: https://app.test/?id=1&token=abc\r\n\r\n",
            "HTTP/1.1 200 OK\r\n\r\n",
        );
        assert!(a.observe(4, &short, b"", all).is_empty());
        assert_eq!(
            split_url("https://u:p@[::1]:8443/x?y").unwrap(),
            ("[::1]".to_string(), "/x?y")
        );
    }

    #[test]
    fn headers_present_on_some_pages_only() {
        let mut a = CrossAnalyzer::new();
        let page = |path: &str, headers: &str| {
            msg(
                "app.test",
                &format!("GET {path} HTTP/1.1\r\nHost: app.test\r\n\r\n"),
                &format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n{headers}\r\n<p>x</p>"),
            )
        };
        // A page without any header first: pending.
        assert!(a.observe(1, &page("/old", ""), b"", all).is_empty());
        // A page with HSTS and CSP: the earlier page is reported for both.
        let found = a.observe(
            2,
            &page(
                "/new",
                "Strict-Transport-Security: max-age=1\r\nContent-Security-Policy: default-src 'self'\r\n",
            ),
            b"",
            all,
        );
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|(id, _)| *id == 1));
        // A later page without them: reported on itself.
        let later = a.observe(3, &page("/other", ""), b"", all);
        assert_eq!(later.len(), 2);
        assert!(
            later
                .iter()
                .all(|(id, i)| *id == 3 && i.evidence.contains("/new (#2)"))
        );
        // No anti-framing anywhere: never reported.
        assert!(
            found
                .iter()
                .chain(&later)
                .all(|(_, i)| !i.title.contains("anti-framing"))
        );
    }

    #[test]
    fn the_password_pattern() {
        for body in [
            "password=x",
            "a=1&pwd=2",
            "{\"password\": \"x\"}",
            "user_pass=1",
        ] {
            assert!(PASSWORD_FIELD.is_match(body), "{body}");
        }
        for body in ["passport=1", "q=password", "compass=1"] {
            assert!(!PASSWORD_FIELD.is_match(body), "{body}");
        }
    }
}
