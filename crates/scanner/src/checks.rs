//! The passive checks. Each one reads a [`Message`] and adds [`Issue`]s; the
//! texts explain the problem for someone learning web security.

use crate::jwt;
use crate::message::Message;
use crate::{CheckInfo, Confidence, Issue, Severity};
use chosproxy_http::cookies::SetCookie;
use regex::Regex;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::LazyLock;

type Run = fn(&Message, &mut Vec<Issue>);

macro_rules! check {
    ($name:ident, $id:literal, $kind:literal, $severity:ident, $summary:literal,
     $description:literal, $remediation:literal) => {
        static $name: CheckInfo = CheckInfo {
            id: $id,
            kind: $kind,
            summary: $summary,
            severity: Severity::$severity,
            description: $description,
            remediation: $remediation,
        };
    };
}

check!(
    COOKIE_SECURE,
    "cookie-secure",
    "Cookie without Secure",
    Low,
    "HTTPS responses that set a cookie without the Secure attribute.",
    "A cookie without the Secure attribute is also sent over plain HTTP. If the user ever \
     reaches the site over HTTP (a typed address, an old link, an attacker on the network \
     forcing a downgrade), the cookie travels unencrypted and can be read by anyone on the \
     path. For session cookies that means the session can be stolen.",
    "Add the Secure attribute to every cookie set over HTTPS, and redirect HTTP to HTTPS with \
     HSTS so browsers never use HTTP for the site."
);
check!(
    COOKIE_HTTPONLY,
    "cookie-httponly",
    "Cookie without HttpOnly",
    Low,
    "Session-like cookies (session, sid, token, auth…) set without HttpOnly.",
    "Without HttpOnly, JavaScript running in the page can read the cookie through \
     document.cookie. A single cross-site scripting (XSS) bug then lets an attacker copy the \
     session cookie and take over the account. The check guesses which cookies are sessions \
     from their names, so confirm what the cookie is used for.",
    "Set HttpOnly on session and authentication cookies. Only cookies that scripts must read \
     (for example a CSRF double-submit cookie) should go without it."
);
check!(
    COOKIE_SAMESITE,
    "cookie-samesite",
    "Cookie SameSite policy",
    Low,
    "Cookies with SameSite=None but no Secure, or session-like cookies without SameSite.",
    "SameSite tells the browser whether to send a cookie with requests started by other \
     sites. With SameSite=None the cookie goes with every cross-site request, which is what \
     cross-site request forgery (CSRF) relies on; browsers also reject SameSite=None without \
     Secure. Without the attribute, browsers apply their own default (usually Lax), which \
     differs between browsers and versions.",
    "Set SameSite=Lax (or Strict) on session cookies explicitly. Use SameSite=None only when \
     cross-site use is required, always together with Secure, and protect state-changing \
     requests with CSRF tokens."
);
check!(
    HSTS,
    "hsts",
    "Strict Transport Security",
    Low,
    "HTTPS responses without Strict-Transport-Security, or with a short max-age.",
    "HTTP Strict Transport Security (HSTS) makes the browser use HTTPS for the site for the \
     time given in max-age, even if the user types http:// or follows an old link. Without \
     it, the first request can go over HTTP, where an attacker on the network can intercept \
     it and keep the user on an unencrypted copy of the site (SSL stripping).",
    "Send Strict-Transport-Security: max-age=31536000; includeSubDomains on every HTTPS \
     response (at least 180 days), after making sure every subdomain works over HTTPS."
);
check!(
    NOSNIFF,
    "nosniff",
    "Missing X-Content-Type-Options",
    Info,
    "Text, script and data responses without X-Content-Type-Options: nosniff.",
    "Browsers may guess (sniff) the type of a response instead of trusting Content-Type. A \
     file uploaded as an image or a JSON answer could then be run as HTML or a script, which \
     can turn a harmless upload or API response into cross-site scripting.",
    "Send X-Content-Type-Options: nosniff on every response and set an accurate Content-Type."
);
check!(
    CLICKJACKING,
    "clickjacking",
    "Frameable page (clickjacking)",
    Info,
    "HTML pages without X-Frame-Options or a CSP frame-ancestors directive.",
    "A page that any site may show in a frame can be loaded invisibly under an attacker's \
     page, so the user's clicks land on buttons they cannot see (clickjacking): changing \
     settings, confirming payments, granting permissions.",
    "Send Content-Security-Policy: frame-ancestors 'self' (or 'none'), or X-Frame-Options: \
     DENY / SAMEORIGIN for older browsers."
);
check!(
    CSP,
    "csp",
    "Content Security Policy",
    Info,
    "HTML pages without a Content-Security-Policy, or whose policy allows inline scripts.",
    "A Content Security Policy limits where scripts, styles and other resources may come \
     from. It does not fix cross-site scripting, but a strict policy stops most injected \
     scripts from running. 'unsafe-inline' or 'unsafe-eval' in script-src removes most of that \
     protection.",
    "Define a policy such as default-src 'self'; script-src 'self' with nonces or hashes for \
     the inline scripts you need, and remove 'unsafe-inline' and 'unsafe-eval'. Start with \
     Content-Security-Policy-Report-Only to find what would break."
);
check!(
    SOFTWARE_VERSION,
    "software-version",
    "Software version disclosed",
    Info,
    "Server, X-Powered-By and similar headers (or a generator tag) naming software versions.",
    "Headers such as Server: Apache/2.4.41 or X-Powered-By: PHP/7.4.3 tell anyone which \
     software and version runs the site. That alone is not a vulnerability, but it lets an \
     attacker look up known vulnerabilities for that exact version without probing.",
    "Remove version numbers from Server, X-Powered-By and similar headers (and generator \
     tags), and keep the software up to date: hiding versions only slows an attacker down."
);
check!(
    CORS,
    "cors",
    "Permissive CORS",
    Medium,
    "Access-Control-Allow-Origin that reflects the request's Origin or allows null.",
    "Cross-Origin Resource Sharing (CORS) decides which other sites may read responses with \
     JavaScript. If the server copies any Origin it receives into Access-Control-Allow-Origin \
     and also sends Access-Control-Allow-Credentials: true, any website the user visits can \
     read this response with the user's cookies — personal data, API keys, CSRF tokens. The \
     null origin is used by sandboxed frames and local files, so allowing it is just as \
     risky. Confirm by resending the request in Repeater with Origin: https://example.org.",
    "Allow only a fixed list of trusted origins, compare the Origin header exactly (not with \
     prefixes or suffixes), never allow null, and send Vary: Origin."
);
check!(
    MIXED_CONTENT,
    "mixed-content",
    "Mixed content",
    Low,
    "HTTPS pages that load scripts, frames, styles or images over plain HTTP.",
    "An HTTPS page that loads a script, frame or stylesheet over HTTP gives an attacker on \
     the network a way to replace it and take control of the whole page, defeating HTTPS. \
     Browsers block most of this (active mixed content) and warn about images and media \
     (passive mixed content), which can still leak what the user is viewing.",
    "Load every resource over HTTPS (or with relative URLs), and consider \
     Content-Security-Policy: upgrade-insecure-requests."
);
check!(
    CLEARTEXT_PASSWORD,
    "cleartext-password",
    "Password sent in cleartext",
    Medium,
    "Password fields on pages served over HTTP, or forms that submit them to http:// URLs.",
    "A password typed into a page served over HTTP, or submitted to an http:// address, \
     crosses the network unencrypted. Anyone on the same Wi-Fi, a compromised router or the \
     network provider can read it.",
    "Serve login and registration pages over HTTPS only, make forms submit to https:// URLs, \
     and enable HSTS."
);
check!(
    SENSITIVE_URL,
    "sensitive-url-parameter",
    "Sensitive data in the URL",
    Low,
    "URLs with parameters such as password, token, api_key or session.",
    "URLs are stored in browser history, server and proxy logs, analytics tools and are sent \
     to other sites in the Referer header. Passwords, tokens or session identifiers in the \
     query string leak to all of them.",
    "Send secrets in request bodies (POST) or headers such as Authorization, never in the \
     URL. Rotate any secret that has been sent this way."
);
check!(
    JWT,
    "jwt",
    "JSON Web Token weaknesses",
    Medium,
    "JWTs that are unsigned (alg none), never expire or carry secrets in their claims.",
    "A JSON Web Token's header and claims are only Base64-encoded: anyone holding the token \
     can read them. A token with alg none has no signature, so if the server accepts it, \
     anyone can forge one with any identity. A token without an exp claim stays valid \
     forever once stolen. Secrets in the claims are visible to anyone who sees the token.",
    "Always verify signatures with an explicit list of allowed algorithms (never none), give \
     tokens a short expiry and refresh them, and keep secrets out of the claims. Try a \
     forged alg none token in Repeater to see whether the server accepts it."
);
check!(
    ERROR_MESSAGES,
    "error-messages",
    "Verbose error messages",
    Medium,
    "Stack traces, framework debug pages and database error messages in responses.",
    "Detailed error messages reveal file paths, framework versions, code and query structure \
     — a map for an attacker. A database error triggered by a request is also a classic sign \
     of SQL injection: the input reached a query without being handled safely. Database \
     errors are reported as tentative; confirm them manually.",
    "Show users a generic error page, log the details on the server, turn off debug modes in \
     production, and use parameterised queries for every database access."
);
check!(
    PRIVATE_IP,
    "private-ip",
    "Private IP address disclosed",
    Info,
    "Internal (RFC 1918) IP addresses in responses from public hosts.",
    "Internal addresses such as 10.0.0.12 or 192.168.1.20 reveal how the internal network is \
     laid out (load balancers, application servers). They help an attacker plan further steps, \
     for example server-side request forgery against internal hosts. Version numbers and \
     other text can look like addresses, so confirm the match.",
    "Remove internal addresses from headers, error pages and API responses, for example by \
     configuring the reverse proxy to rewrite them."
);
check!(
    DIRECTORY_LISTING,
    "directory-listing",
    "Directory listing",
    Low,
    "Pages that list the files of a directory (Index of /…).",
    "When the web server lists a directory's contents, anyone can browse files that were \
     never meant to be linked: backups, configuration files, old versions, uploads.",
    "Disable directory listing in the web server (for example Options -Indexes in Apache, \
     autoindex off in nginx) and remove files that should not be public."
);
check!(
    CACHE_CONTROL,
    "cache-control",
    "Cacheable authenticated response",
    Info,
    "Authenticated HTTPS responses without Cache-Control: no-store or private.",
    "A response to an authenticated request (with a session cookie or an Authorization \
     header) may contain personal data. Without Cache-Control: no-store or private, shared \
     caches (proxies, CDNs) or the browser's disk cache may keep it, where other users of the \
     same computer or cache could see it.",
    "Send Cache-Control: no-store on responses with personal or sensitive data, and private \
     on other authenticated responses."
);
check!(
    INTERESTING_PATH,
    "interesting-path",
    "Interesting or exposed resource",
    Low,
    "Reachable admin consoles, API documentation, debug endpoints and sensitive files.",
    "Some paths are worth a closer look: administration and debug consoles, API \
     documentation (it lists every endpoint), version-control folders (.git), environment \
     files (.env), backups and phpinfo pages, which can expose source code, credentials or \
     configuration. Sensitive files are reported as Low; other paths as information.",
    "Remove debug endpoints and sensitive files from production, keep version-control folders \
     out of the web root, and protect administration interfaces with authentication and \
     network restrictions."
);

/// Every passive check, in the order shown in the settings.
pub static CHECKS: &[&CheckInfo] = &[
    &COOKIE_SECURE,
    &COOKIE_HTTPONLY,
    &COOKIE_SAMESITE,
    &HSTS,
    &NOSNIFF,
    &CLICKJACKING,
    &CSP,
    &SOFTWARE_VERSION,
    &CORS,
    &MIXED_CONTENT,
    &CLEARTEXT_PASSWORD,
    &SENSITIVE_URL,
    &JWT,
    &ERROR_MESSAGES,
    &PRIVATE_IP,
    &DIRECTORY_LISTING,
    &CACHE_CONTROL,
    &INTERESTING_PATH,
];

static RUNNERS: &[(&CheckInfo, Run)] = &[
    (&COOKIE_SECURE, cookie_secure),
    (&COOKIE_HTTPONLY, cookie_httponly),
    (&COOKIE_SAMESITE, cookie_samesite),
    (&HSTS, hsts),
    (&NOSNIFF, nosniff),
    (&CLICKJACKING, clickjacking),
    (&CSP, csp),
    (&SOFTWARE_VERSION, software_version),
    (&CORS, cors),
    (&MIXED_CONTENT, mixed_content),
    (&CLEARTEXT_PASSWORD, cleartext_password),
    (&SENSITIVE_URL, sensitive_url),
    (&JWT, jwts),
    (&ERROR_MESSAGES, error_messages),
    (&PRIVATE_IP, private_ip),
    (&DIRECTORY_LISTING, directory_listing),
    (&CACHE_CONTROL, cache_control),
    (&INTERESTING_PATH, interesting_path),
];

/// Runs the checks `enabled` accepts (by id) on one message.
pub fn analyze(message: &Message, enabled: impl Fn(&str) -> bool) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (info, run) in RUNNERS {
        if enabled(info.id) {
            run(message, &mut issues);
        }
    }
    issues
}

fn issue(
    check: &'static CheckInfo,
    title: impl Into<String>,
    severity: Severity,
    confidence: Confidence,
    evidence: impl Into<String>,
    dedup: impl Into<String>,
) -> Issue {
    Issue {
        check,
        title: title.into(),
        severity,
        confidence,
        evidence: shorten(&evidence.into(), 300),
        dedup: dedup.into(),
    }
}

/// At most `max` characters, with an ellipsis when cut.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

fn regex(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

/// Cookie names that look like sessions or credentials.
pub(crate) fn session_like(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.contains("csrf") || n.contains("xsrf") {
        return false;
    }
    ["sess", "sid", "token", "auth", "jwt", "login", "remember"]
        .iter()
        .any(|k| n.contains(k))
}

/// The cookies set by the response, with the raw header (for evidence).
fn set_cookies(m: &Message) -> Vec<(SetCookie, String)> {
    m.response_headers("set-cookie")
        .into_iter()
        .filter_map(|raw| Some((SetCookie::parse(&raw, 0)?, raw)))
        // A cookie being deleted needs no protection.
        .filter(|(c, raw)| {
            let lower = raw.to_ascii_lowercase();
            !(c.value.is_empty()
                || lower.contains("max-age=0")
                || lower.contains("expires=thu, 01 jan 1970"))
        })
        .collect()
}

/// `name=value…` with the value shortened (evidence without the secret).
fn cookie_evidence(raw: &str) -> String {
    match raw.split_once('=') {
        Some((name, rest)) => {
            let (value, attrs) = rest.split_once(';').unwrap_or((rest, ""));
            let shown: String = value.chars().take(6).collect();
            let more = if value.chars().count() > 6 { "…" } else { "" };
            format!("Set-Cookie: {name}={shown}{more};{attrs}")
        }
        None => format!("Set-Cookie: {raw}"),
    }
}

fn cookie_secure(m: &Message, out: &mut Vec<Issue>) {
    if !m.https() {
        return;
    }
    for (c, raw) in set_cookies(m) {
        if !c.secure {
            out.push(issue(
                &COOKIE_SECURE,
                format!("Cookie without Secure: {}", c.name),
                if session_like(&c.name) {
                    Severity::Low
                } else {
                    Severity::Info
                },
                Confidence::Certain,
                cookie_evidence(&raw),
                format!("{}|{}", m.origin(), c.name),
            ));
        }
    }
}

fn cookie_httponly(m: &Message, out: &mut Vec<Issue>) {
    for (c, raw) in set_cookies(m) {
        if !c.http_only && session_like(&c.name) {
            out.push(issue(
                &COOKIE_HTTPONLY,
                format!("Session cookie without HttpOnly: {}", c.name),
                Severity::Low,
                Confidence::Firm,
                cookie_evidence(&raw),
                format!("{}|{}", m.origin(), c.name),
            ));
        }
    }
}

fn cookie_samesite(m: &Message, out: &mut Vec<Issue>) {
    for (c, raw) in set_cookies(m) {
        let same_site = c.same_site.as_deref().map(str::to_ascii_lowercase);
        if same_site.as_deref() == Some("none") && !c.secure {
            out.push(issue(
                &COOKIE_SAMESITE,
                format!("SameSite=None without Secure: {}", c.name),
                Severity::Low,
                Confidence::Certain,
                cookie_evidence(&raw),
                format!("{}|{}|none", m.origin(), c.name),
            ));
        } else if same_site.is_none() && session_like(&c.name) {
            out.push(issue(
                &COOKIE_SAMESITE,
                format!("Session cookie without SameSite: {}", c.name),
                Severity::Info,
                Confidence::Firm,
                cookie_evidence(&raw),
                format!("{}|{}|missing", m.origin(), c.name),
            ));
        }
    }
}

/// Six months: the shortest max-age considered long enough.
const HSTS_MIN_AGE: u64 = 15_552_000;

fn hsts(m: &Message, out: &mut Vec<Issue>) {
    if !m.https() || !(200..400).contains(&m.status()) {
        return;
    }
    match m.response_header("strict-transport-security") {
        None => out.push(issue(
            &HSTS,
            format!("Strict-Transport-Security missing on {}", m.host),
            Severity::Low,
            Confidence::Certain,
            format!("HTTP/1.1 {} without Strict-Transport-Security", m.status()),
            m.origin(),
        )),
        Some(value) => {
            let age = value.split(';').find_map(|d| {
                let (k, v) = d.split_once('=')?;
                k.trim()
                    .eq_ignore_ascii_case("max-age")
                    .then(|| v.trim().trim_matches('"').parse::<u64>().ok())
                    .flatten()
            });
            if let Some(age) = age.filter(|a| *a < HSTS_MIN_AGE) {
                out.push(issue(
                    &HSTS,
                    format!("Short HSTS max-age ({age} s) on {}", m.host),
                    Severity::Info,
                    Confidence::Certain,
                    format!("Strict-Transport-Security: {value}"),
                    format!("{}|short", m.origin()),
                ));
            }
        }
    }
}

fn nosniff(m: &Message, out: &mut Vec<Issue>) {
    if !(200..300).contains(&m.status()) || m.body.is_empty() {
        return;
    }
    let t = m.media_type();
    let sniffable = t.starts_with("text/")
        || t.contains("javascript")
        || t.contains("json")
        || t.contains("xml");
    let set = m
        .response_header("x-content-type-options")
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("nosniff"));
    if sniffable && !set {
        out.push(issue(
            &NOSNIFF,
            format!("X-Content-Type-Options missing on {}", m.host),
            Severity::Info,
            Confidence::Certain,
            format!("Content-Type: {t} without X-Content-Type-Options: nosniff"),
            m.origin(),
        ));
    }
}

fn csp_header(m: &Message) -> Option<String> {
    m.response_header("content-security-policy")
}

fn clickjacking(m: &Message, out: &mut Vec<Issue>) {
    if !m.is_html() || m.status() != 200 {
        return;
    }
    let xfo = m.response_header("x-frame-options").is_some();
    let ancestors =
        csp_header(m).is_some_and(|c| c.to_ascii_lowercase().contains("frame-ancestors"));
    if !xfo && !ancestors {
        out.push(issue(
            &CLICKJACKING,
            format!("Pages on {} can be framed", m.host),
            Severity::Info,
            Confidence::Firm,
            "No X-Frame-Options and no CSP frame-ancestors",
            m.origin(),
        ));
    }
}

fn csp(m: &Message, out: &mut Vec<Issue>) {
    if !m.is_html() || m.status() != 200 {
        return;
    }
    let Some(policy) = csp_header(m) else {
        out.push(issue(
            &CSP,
            format!("No Content-Security-Policy on {}", m.host),
            Severity::Info,
            Confidence::Certain,
            "HTML page without a Content-Security-Policy header",
            format!("{}|missing", m.origin()),
        ));
        return;
    };
    let lower = policy.to_ascii_lowercase();
    let directive = |name: &str| {
        lower
            .split(';')
            .map(str::trim)
            .find(|d| d.split_whitespace().next() == Some(name))
            .map(str::to_string)
    };
    let Some(scripts) = directive("script-src").or_else(|| directive("default-src")) else {
        return;
    };
    let weak = ["'unsafe-inline'", "'unsafe-eval'"]
        .iter()
        .filter(|w| scripts.contains(*w))
        .copied()
        .collect::<Vec<_>>();
    // 'unsafe-inline' is ignored by browsers when a nonce or hash is present.
    let has_nonce = scripts.contains("'nonce-") || scripts.contains("'sha");
    let weak: Vec<&str> = weak
        .into_iter()
        .filter(|w| !(has_nonce && *w == "'unsafe-inline'"))
        .collect();
    if !weak.is_empty() {
        out.push(issue(
            &CSP,
            format!(
                "Content-Security-Policy on {} allows {}",
                m.host,
                weak.join(" and ")
            ),
            Severity::Info,
            Confidence::Firm,
            format!("Content-Security-Policy: {policy}"),
            format!("{}|weak", m.origin()),
        ));
    }
}

static VERSION_LIKE: LazyLock<Option<Regex>> = LazyLock::new(|| regex(r"\d+\.\d+"));
static GENERATOR: LazyLock<Option<Regex>> = LazyLock::new(|| {
    regex(r#"(?i)<meta\s+name\s*=\s*["']generator["']\s+content\s*=\s*["']([^"']+)["']"#)
});

fn software_version(m: &Message, out: &mut Vec<Issue>) {
    let Some(version) = VERSION_LIKE.as_ref() else {
        return;
    };
    for header in [
        "server",
        "x-powered-by",
        "x-aspnet-version",
        "x-aspnetmvc-version",
        "x-generator",
        "x-runtime-version",
    ] {
        if let Some(value) = m.response_header(header)
            && version.is_match(&value)
        {
            let value = shorten(&value, 80);
            out.push(issue(
                &SOFTWARE_VERSION,
                format!("Software version in {header}: {value}"),
                Severity::Info,
                Confidence::Firm,
                format!("{header}: {value}"),
                format!("{}|{header}|{value}", m.origin()),
            ));
        }
    }
    if m.is_html()
        && let Some(captures) = GENERATOR.as_ref().and_then(|r| r.captures(&m.body))
        && let Some(content) = captures.get(1)
        && version.is_match(content.as_str())
    {
        let value = shorten(content.as_str(), 80);
        out.push(issue(
            &SOFTWARE_VERSION,
            format!("Software version in a generator tag: {value}"),
            Severity::Info,
            Confidence::Firm,
            shorten(captures.get(0).map_or("", |c| c.as_str()), 200),
            format!("{}|generator|{value}", m.origin()),
        ));
    }
}

/// Host part of an origin such as `https://a.example:8443`.
fn origin_host(origin: &str) -> String {
    let rest = origin.split_once("://").map_or(origin, |(_, r)| r);
    let host = rest.split(['/', ':']).next().unwrap_or_default();
    host.to_ascii_lowercase()
}

fn cors(m: &Message, out: &mut Vec<Issue>) {
    let Some(allowed) = m.response_header("access-control-allow-origin") else {
        return;
    };
    let allowed = allowed.trim().to_string();
    let credentials = m
        .response_header("access-control-allow-credentials")
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"));
    let evidence = format!(
        "Access-Control-Allow-Origin: {allowed}{}",
        if credentials {
            "\nAccess-Control-Allow-Credentials: true"
        } else {
            ""
        }
    );
    let dedup = format!("{}{}", m.origin(), m.path_only());
    if allowed.eq_ignore_ascii_case("null") {
        out.push(issue(
            &CORS,
            format!("CORS allows the null origin on {}", m.path_only()),
            if credentials {
                Severity::Medium
            } else {
                Severity::Low
            },
            Confidence::Firm,
            evidence,
            format!("{dedup}|null"),
        ));
        return;
    }
    let Some(origin) = m.request_header("origin") else {
        return;
    };
    let cross = origin_host(&origin) != m.host;
    if cross && allowed == origin.trim() {
        out.push(issue(
            &CORS,
            format!("CORS trusts the request's origin on {}", m.path_only()),
            if credentials {
                Severity::Medium
            } else {
                Severity::Low
            },
            Confidence::Tentative,
            format!("Origin: {origin}\n{evidence}"),
            format!("{dedup}|reflected"),
        ));
    }
}

static MIXED: LazyLock<Option<Regex>> = LazyLock::new(|| {
    regex(
        r#"(?i)<(script|iframe|frame|link|object|embed|form|img|audio|video|source)\b[^>]*?\b(?:src|href|action|data)\s*=\s*["']?(http://[^"'\s>]+)"#,
    )
});

fn mixed_content(m: &Message, out: &mut Vec<Issue>) {
    if !m.https() || !m.is_html() {
        return;
    }
    let Some(re) = MIXED.as_ref() else {
        return;
    };
    let mut seen_active = false;
    let mut seen_passive = false;
    for c in re.captures_iter(&m.body).take(200) {
        let tag = c.get(1).map_or("", |t| t.as_str()).to_ascii_lowercase();
        let url = c.get(2).map_or("", |u| u.as_str());
        let active = !matches!(tag.as_str(), "img" | "audio" | "video" | "source");
        if (active && seen_active) || (!active && seen_passive) {
            continue;
        }
        if active {
            seen_active = true;
        } else {
            seen_passive = true;
        }
        out.push(issue(
            &MIXED_CONTENT,
            format!(
                "{} loaded over HTTP on {}",
                if active {
                    "Active content"
                } else {
                    "Images or media"
                },
                m.path_only()
            ),
            if active {
                Severity::Low
            } else {
                Severity::Info
            },
            Confidence::Certain,
            format!("<{tag}> {url}"),
            format!(
                "{}{}|{}",
                m.origin(),
                m.path_only(),
                if active { "active" } else { "passive" }
            ),
        ));
    }
}

static PASSWORD_INPUT: LazyLock<Option<Regex>> =
    LazyLock::new(|| regex(r#"(?i)<input\b[^>]*\btype\s*=\s*["']?password"#));
static HTTP_FORM: LazyLock<Option<Regex>> =
    LazyLock::new(|| regex(r#"(?i)<form\b[^>]*\baction\s*=\s*["']?(http://[^"'\s>]+)"#));

fn cleartext_password(m: &Message, out: &mut Vec<Issue>) {
    if !m.is_html() {
        return;
    }
    let Some(field) = PASSWORD_INPUT.as_ref().and_then(|r| r.find(&m.body)) else {
        return;
    };
    let dedup = format!("{}{}", m.origin(), m.path_only());
    if !m.https() {
        out.push(issue(
            &CLEARTEXT_PASSWORD,
            format!("Password field served over HTTP on {}", m.path_only()),
            Severity::Medium,
            Confidence::Certain,
            field.as_str(),
            dedup,
        ));
    } else if let Some(action) = HTTP_FORM
        .as_ref()
        .and_then(|r| r.captures(&m.body))
        .and_then(|c| c.get(1))
    {
        out.push(issue(
            &CLEARTEXT_PASSWORD,
            format!("Password form submits over HTTP on {}", m.path_only()),
            Severity::Medium,
            Confidence::Firm,
            format!("form action {}", action.as_str()),
            dedup,
        ));
    }
}

const SENSITIVE_PARAMS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "pass",
    "secret",
    "token",
    "access_token",
    "auth_token",
    "id_token",
    "refresh_token",
    "api_key",
    "apikey",
    "session",
    "sessionid",
    "session_id",
    "sid",
    "jsessionid",
    "phpsessid",
];

fn sensitive_url(m: &Message, out: &mut Vec<Issue>) {
    let Some(query) = m.query() else {
        return;
    };
    let mut seen = HashSet::new();
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = name.to_ascii_lowercase().replace("%5f", "_");
        if value.is_empty()
            || !SENSITIVE_PARAMS.contains(&name.as_str())
            || !seen.insert(name.clone())
        {
            continue;
        }
        let shown: String = value.chars().take(4).collect();
        out.push(issue(
            &SENSITIVE_URL,
            format!("Parameter “{name}” in the URL of {}", m.path_only()),
            Severity::Low,
            Confidence::Firm,
            format!("?{name}={shown}… in the request URL"),
            format!("{}{}|{name}", m.origin(), m.path_only()),
        ));
    }
}

fn jwts(m: &Message, out: &mut Vec<Issue>) {
    let mut places: Vec<(&str, String)> = Vec::new();
    if let Some(a) = m.request_header("authorization") {
        places.push(("the Authorization header", a));
    }
    if let Some(c) = m.request_header("cookie") {
        places.push(("a request cookie", c));
    }
    for c in m.response_headers("set-cookie") {
        places.push(("a Set-Cookie header", c));
    }
    if !m.body.is_empty() {
        let end = m.body.len().min(256 * 1024);
        let end = (0..=end)
            .rev()
            .find(|i| m.body.is_char_boundary(*i))
            .unwrap_or(0);
        places.push(("the response body", m.body[..end].to_string()));
    }
    let mut seen = HashSet::new();
    for (place, text) in places {
        for token in jwt::find(&text) {
            if !seen.insert(token.token.clone()) {
                continue;
            }
            let alg = token.alg();
            let evidence = format!("JWT in {place}: {} (alg {alg})", token.short());
            if alg.eq_ignore_ascii_case("none") || token.unsigned {
                out.push(issue(
                    &JWT,
                    format!("Unsigned JWT (alg {alg}) in {place}"),
                    Severity::Medium,
                    Confidence::Firm,
                    evidence.clone(),
                    format!("{}|unsigned", m.origin()),
                ));
            }
            if !token.claims.contains_key("exp") {
                out.push(issue(
                    &JWT,
                    format!("JWT without expiry in {place}"),
                    Severity::Info,
                    Confidence::Certain,
                    evidence.clone(),
                    format!("{}|no-exp", m.origin()),
                ));
            }
            let secret = token.claims.keys().find(|k| {
                let k = k.to_ascii_lowercase();
                k.contains("password") || k == "pwd" || k.contains("secret")
            });
            if let Some(key) = secret {
                out.push(issue(
                    &JWT,
                    format!("JWT claims include “{key}”"),
                    Severity::Low,
                    Confidence::Firm,
                    evidence,
                    format!("{}|secret-claim", m.origin()),
                ));
            }
        }
    }
}

/// An error signature: pattern, what it reveals, whether it is a database
/// error.
struct Signature {
    pattern: &'static str,
    what: &'static str,
    database: bool,
}

const SIGNATURES: &[Signature] = &[
    Signature {
        pattern: r"Traceback \(most recent call last\)",
        what: "Python stack trace",
        database: false,
    },
    Signature {
        pattern: r"\bat [a-zA-Z_$][\w$]*(?:\.[\w$<>]+)+\([\w$]+\.java:\d+\)",
        what: "Java stack trace",
        database: false,
    },
    Signature {
        pattern: r"Server Error in '[^']*' Application|\bat [\w.<>`]+\(.*\) in [^\n]+:line \d+",
        what: ".NET error page",
        database: false,
    },
    Signature {
        pattern: r"(?:Fatal error|Warning|Notice|Parse error)</b>:.{0,300} on line <b>\d+|PHP (?:Fatal error|Warning|Parse error):",
        what: "PHP error message",
        database: false,
    },
    Signature {
        pattern: r"Whitelabel Error Page",
        what: "Spring Boot error page",
        database: false,
    },
    Signature {
        pattern: r"You're seeing this error because you have <code>DEBUG = True</code>",
        what: "Django debug page",
        database: false,
    },
    Signature {
        pattern: r"\bat (?:[\w.<>]+ )?\(?(?:/|[A-Z]:\\)[^\s()]+\.js:\d+:\d+\)?",
        what: "Node.js stack trace",
        database: false,
    },
    Signature {
        pattern: r"ActionController::RoutingError|Rails\.root:",
        what: "Ruby on Rails error page",
        database: false,
    },
    Signature {
        pattern: r"You have an error in your SQL syntax|mysqli?_[a-z_]+\(\)|SQLSTATE\[\w+\]",
        what: "MySQL/PDO error",
        database: true,
    },
    Signature {
        pattern: r"\bORA-\d{5}\b",
        what: "Oracle error",
        database: true,
    },
    Signature {
        pattern: r"Unclosed quotation mark after the character string|Microsoft OLE DB Provider for SQL Server|\[SQL Server\]",
        what: "SQL Server error",
        database: true,
    },
    Signature {
        pattern: r"PG::SyntaxError|pg_query\(\)|ERROR:\s+syntax error at or near",
        what: "PostgreSQL error",
        database: true,
    },
    Signature {
        pattern: r"SQLite3::SQLException|sqlite3\.OperationalError|SQLITE_ERROR",
        what: "SQLite error",
        database: true,
    },
];

static SIGNATURE_RES: LazyLock<Vec<(Regex, &'static Signature)>> = LazyLock::new(|| {
    SIGNATURES
        .iter()
        .filter_map(|s| Some((regex(s.pattern)?, s)))
        .collect()
});

fn error_messages(m: &Message, out: &mut Vec<Issue>) {
    if m.body.is_empty() {
        return;
    }
    let end = m.body.len().min(512 * 1024);
    let end = (0..=end)
        .rev()
        .find(|i| m.body.is_char_boundary(*i))
        .unwrap_or(0);
    let body = &m.body[..end];
    for (re, sig) in SIGNATURE_RES.iter() {
        let Some(found) = re.find(body) else {
            continue;
        };
        let (title, severity, confidence) = if sig.database {
            (
                format!("{} on {} (possible SQL injection)", sig.what, m.path_only()),
                Severity::Medium,
                Confidence::Tentative,
            )
        } else {
            (
                format!("{} on {}", sig.what, m.path_only()),
                Severity::Low,
                Confidence::Firm,
            )
        };
        out.push(issue(
            &ERROR_MESSAGES,
            title,
            severity,
            confidence,
            found.as_str(),
            format!("{}{}|{}", m.origin(), m.path_only(), sig.what),
        ));
    }
}

static PRIVATE_V4: LazyLock<Option<Regex>> = LazyLock::new(|| {
    regex(
        r"\b(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3})\b",
    )
});

fn is_internal_host(host: &str) -> bool {
    match host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => host == "localhost" || host.ends_with(".local") || host.ends_with(".internal"),
    }
}

fn private_ip(m: &Message, out: &mut Vec<Issue>) {
    let (Some(re), Some(response)) = (PRIVATE_V4.as_ref(), &m.response) else {
        return;
    };
    if is_internal_host(&m.host) {
        return;
    }
    let head = String::from_utf8_lossy(&response.raw);
    let end = m.body.len().min(512 * 1024);
    let end = (0..=end)
        .rev()
        .find(|i| m.body.is_char_boundary(*i))
        .unwrap_or(0);
    let mut seen = HashSet::new();
    for text in [head.as_ref(), &m.body[..end]] {
        for found in re.find_iter(text) {
            let ip = found.as_str();
            // Each octet must be a real one (not 10.300.1.1 from a version).
            if ip.parse::<Ipv4Addr>().is_err() || !seen.insert(ip.to_string()) || seen.len() > 3 {
                continue;
            }
            let start = found.start().saturating_sub(40);
            let start = (0..=start)
                .rev()
                .find(|i| text.is_char_boundary(*i))
                .unwrap_or(0);
            let stop = (found.end() + 40).min(text.len());
            let stop = (stop..=text.len())
                .find(|i| text.is_char_boundary(*i))
                .unwrap_or(text.len());
            out.push(issue(
                &PRIVATE_IP,
                format!("Private IP address {ip} disclosed by {}", m.host),
                Severity::Info,
                Confidence::Tentative,
                format!("…{}…", text[start..stop].replace(['\r', '\n'], " ")),
                format!("{}|{ip}", m.origin()),
            ));
        }
    }
}

static LISTING: LazyLock<Option<Regex>> =
    LazyLock::new(|| regex(r"(?i)<title>\s*(?:Index of /|Directory listing for /)[^<]*</title>"));

fn directory_listing(m: &Message, out: &mut Vec<Issue>) {
    if !m.is_html() || m.status() != 200 {
        return;
    }
    if let Some(found) = LISTING.as_ref().and_then(|r| r.find(&m.body)) {
        out.push(issue(
            &DIRECTORY_LISTING,
            format!("Directory listing at {}", m.path_only()),
            Severity::Low,
            Confidence::Firm,
            found.as_str(),
            format!("{}{}", m.origin(), m.path_only()),
        ));
    }
}

fn cache_control(m: &Message, out: &mut Vec<Issue>) {
    if !m.https() || m.status() != 200 {
        return;
    }
    let authorization = m.request_header("authorization").is_some();
    let session = set_cookies(m).iter().any(|(c, _)| session_like(&c.name))
        || m.request_header("cookie").is_some_and(|c| {
            c.split(';')
                .any(|p| session_like(p.split('=').next().unwrap_or_default().trim()))
        });
    if !authorization && !session {
        return;
    }
    let cache = m
        .response_header("cache-control")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if cache.contains("no-store") || cache.contains("private") {
        return;
    }
    out.push(issue(
        &CACHE_CONTROL,
        format!("Authenticated response may be cached: {}", m.path_only()),
        Severity::Info,
        Confidence::Tentative,
        if cache.is_empty() {
            "No Cache-Control header on an authenticated response".to_string()
        } else {
            format!("Cache-Control: {cache}")
        },
        format!("{}{}", m.origin(), m.path_only()),
    ));
}

/// Path pieces, whether they point at a sensitive file, and a label.
const PATHS: &[(&str, bool, &str)] = &[
    ("/.git/", true, "Git repository"),
    ("/.git/config", true, "Git repository"),
    ("/.svn/", true, "Subversion repository"),
    ("/.env", true, "environment file"),
    ("/.htpasswd", true, "password file"),
    ("/.ds_store", true, "macOS folder metadata"),
    ("/web.config", true, "IIS configuration"),
    ("/phpinfo.php", true, "phpinfo page"),
    ("/server-status", true, "Apache server status"),
    ("/elmah.axd", true, ".NET error log"),
    ("/trace.axd", true, ".NET trace"),
    ("/actuator", false, "Spring Boot actuator"),
    ("/swagger", false, "API documentation"),
    ("/api-docs", false, "API documentation"),
    ("/openapi", false, "API documentation"),
    ("/graphql", false, "GraphQL endpoint"),
    ("/graphiql", false, "GraphQL console"),
    ("/wp-admin", false, "WordPress administration"),
    ("/admin", false, "administration area"),
    ("/console", false, "console"),
    ("/debug", false, "debug endpoint"),
];

const SENSITIVE_EXTENSIONS: &[&str] = &[".bak", ".old", ".orig", ".sql", ".swp", "~"];

fn interesting_path(m: &Message, out: &mut Vec<Issue>) {
    if !(200..300).contains(&m.status()) {
        return;
    }
    let path = m.path_only().to_ascii_lowercase();
    let dedup = format!("{}{}", m.origin(), m.path_only());
    let found = PATHS
        .iter()
        .find(|(piece, _, _)| {
            path == *piece
                || path.starts_with(piece)
                    && (piece.ends_with('/')
                        || path[piece.len()..].starts_with(['/', '.', '?'])
                        || path.len() == piece.len())
        })
        .map(|(_, sensitive, label)| (*sensitive, label.to_string()))
        .or_else(|| {
            SENSITIVE_EXTENSIONS
                .iter()
                .find(|e| path.ends_with(*e))
                .map(|e| (true, format!("backup or source file ({e})")))
        })
        .or_else(|| {
            m.body
                .contains("<title>phpinfo()</title>")
                .then(|| (true, "phpinfo page".to_string()))
        });
    if let Some((sensitive, label)) = found {
        out.push(issue(
            &INTERESTING_PATH,
            format!("Reachable {label}: {}", m.path_only()),
            if sensitive {
                Severity::Low
            } else {
                Severity::Info
            },
            if sensitive {
                Confidence::Firm
            } else {
                Confidence::Tentative
            },
            format!("HTTP {} for {}", m.status(), m.path_only()),
            dedup,
        ));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    fn msg(scheme: &str, path: &str, request_headers: &str, response: &str) -> Message {
        let request = format!("GET {path} HTTP/1.1\r\nHost: shop.example\r\n{request_headers}\r\n");
        Message::new(
            scheme,
            "shop.example",
            if scheme == "https" { 443 } else { 80 },
            path,
            request.as_bytes(),
            Some(response.as_bytes()),
        )
    }

    fn run(m: &Message, id: &str) -> Vec<Issue> {
        analyze(m, |c| c == id)
    }

    const HTML: &str = "Content-Type: text/html\r\n";

    #[test]
    fn every_check_runs_and_has_texts() {
        assert_eq!(CHECKS.len(), RUNNERS.len());
        let ids: HashSet<&str> = CHECKS.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), CHECKS.len(), "ids are unique");
        for c in CHECKS {
            assert!(
                c.description.len() > 80 && c.remediation.len() > 40,
                "{}",
                c.id
            );
        }
    }

    #[test]
    fn cookies() {
        let m = msg(
            "https",
            "/login",
            "",
            "HTTP/1.1 200 OK\r\nSet-Cookie: SESSIONID=abcdef123456; Path=/\r\n\
             Set-Cookie: theme=dark; Secure\r\nSet-Cookie: tracking=1; SameSite=None\r\n\
             Set-Cookie: auth=gone; Max-Age=0\r\n\r\n",
        );
        let secure = run(&m, "cookie-secure");
        let titles: Vec<&str> = secure.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "Cookie without Secure: SESSIONID",
                "Cookie without Secure: tracking"
            ]
        );
        assert_eq!(secure[0].severity, Severity::Low);
        assert_eq!(secure[1].severity, Severity::Info);
        assert!(
            secure[0]
                .evidence
                .starts_with("Set-Cookie: SESSIONID=abcdef…;")
        );
        assert!(
            !secure[0].evidence.contains("123456"),
            "values are not copied"
        );
        assert_eq!(secure[0].dedup, "https://shop.example:443|SESSIONID");

        assert_eq!(run(&m, "cookie-httponly").len(), 1);
        let samesite = run(&m, "cookie-samesite");
        assert_eq!(samesite.len(), 2);
        assert!(
            samesite
                .iter()
                .any(|i| i.title.contains("SameSite=None without Secure"))
        );

        // Over HTTP the Secure check does not apply.
        let plain = msg(
            "http",
            "/",
            "",
            "HTTP/1.1 200 OK\r\nSet-Cookie: sid=1\r\n\r\n",
        );
        assert!(run(&plain, "cookie-secure").is_empty());
    }

    #[test]
    fn transport_and_framing_headers() {
        let bare = msg(
            "https",
            "/",
            "",
            &format!("HTTP/1.1 200 OK\r\n{HTML}\r\n<p>hi</p>"),
        );
        assert_eq!(run(&bare, "hsts").len(), 1);
        assert_eq!(run(&bare, "nosniff").len(), 1);
        assert_eq!(run(&bare, "clickjacking").len(), 1);
        assert!(
            run(&bare, "csp")[0]
                .title
                .starts_with("No Content-Security-Policy")
        );

        let hardened = msg(
            "https",
            "/",
            "",
            &format!(
                "HTTP/1.1 200 OK\r\n{HTML}Strict-Transport-Security: max-age=31536000\r\n\
                 X-Content-Type-Options: nosniff\r\n\
                 Content-Security-Policy: default-src 'self'; frame-ancestors 'none'\r\n\r\n<p>hi</p>"
            ),
        );
        for id in ["hsts", "nosniff", "clickjacking", "csp"] {
            assert!(run(&hardened, id).is_empty(), "{id}");
        }

        let weak = msg(
            "https",
            "/",
            "",
            &format!(
                "HTTP/1.1 200 OK\r\n{HTML}Strict-Transport-Security: max-age=600\r\n\
                 Content-Security-Policy: script-src 'self' 'unsafe-inline' 'unsafe-eval'\r\n\r\nx"
            ),
        );
        assert!(run(&weak, "hsts")[0].title.contains("600 s"));
        assert!(
            run(&weak, "csp")[0]
                .title
                .contains("'unsafe-inline' and 'unsafe-eval'")
        );
        let nonce = msg(
            "https",
            "/",
            "",
            &format!(
                "HTTP/1.1 200 OK\r\n{HTML}Content-Security-Policy: script-src 'nonce-abc' 'unsafe-inline'\r\n\r\nx"
            ),
        );
        assert!(
            run(&nonce, "csp").is_empty(),
            "unsafe-inline is ignored with a nonce"
        );
    }

    #[test]
    fn versions_and_cors() {
        let m = msg(
            "http",
            "/api/me",
            "Origin: https://evil.example\r\n",
            "HTTP/1.1 200 OK\r\nServer: Apache/2.4.41 (Ubuntu)\r\nX-Powered-By: Express\r\n\
             Access-Control-Allow-Origin: https://evil.example\r\n\
             Access-Control-Allow-Credentials: true\r\n\r\n{}",
        );
        let versions = run(&m, "software-version");
        assert_eq!(versions.len(), 1, "Express has no version");
        assert!(versions[0].title.contains("Apache/2.4.41"));
        let cors = run(&m, "cors");
        assert_eq!(cors.len(), 1);
        assert_eq!(
            (cors[0].severity, cors[0].confidence),
            (Severity::Medium, Confidence::Tentative)
        );

        let same_origin = msg(
            "http",
            "/",
            "Origin: http://shop.example\r\n",
            "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: http://shop.example\r\n\r\n",
        );
        assert!(run(&same_origin, "cors").is_empty());
        let null = msg(
            "http",
            "/",
            "",
            "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: null\r\n\r\n",
        );
        assert_eq!(run(&null, "cors")[0].severity, Severity::Low);
    }

    #[test]
    fn page_content_checks() {
        let page = msg(
            "https",
            "/login",
            "",
            &format!(
                "HTTP/1.1 200 OK\r\n{HTML}\r\n<html><script src=\"http://cdn.example/a.js\"></script>\
                 <img src='http://img.example/x.png'><img src='http://img.example/y.png'>\
                 <form action=\"http://shop.example/login\"><input type=\"password\" name=p></form></html>"
            ),
        );
        let mixed = run(&page, "mixed-content");
        assert_eq!(mixed.len(), 2, "one active, one passive");
        assert_eq!(mixed[0].severity, Severity::Low);
        assert!(mixed[0].evidence.contains("http://cdn.example/a.js"));
        assert!(
            run(&page, "cleartext-password")[0]
                .title
                .contains("submits over HTTP")
        );

        let http_login = msg(
            "http",
            "/login",
            "",
            &format!("HTTP/1.1 200 OK\r\n{HTML}\r\n<input name=pw type=password>"),
        );
        assert_eq!(
            run(&http_login, "cleartext-password")[0].confidence,
            Confidence::Certain
        );

        let listing = msg(
            "http",
            "/files/",
            "",
            &format!("HTTP/1.1 200 OK\r\n{HTML}\r\n<html><title>Index of /files</title>"),
        );
        assert_eq!(run(&listing, "directory-listing").len(), 1);
    }

    #[test]
    fn urls_and_tokens() {
        let m = msg(
            "https",
            "/reset?email=a@b.c&token=abcdef&Password=x&token=again",
            "",
            "HTTP/1.1 200 OK\r\n\r\n",
        );
        let found = run(&m, "sensitive-url-parameter");
        let names: Vec<&str> = found
            .iter()
            .map(|i| i.dedup.rsplit('|').next().unwrap())
            .collect();
        assert_eq!(names, vec!["token", "password"]);
        assert!(!found[0].evidence.contains("abcdef"));

        let enc = |s: &str| URL_SAFE_NO_PAD.encode(s);
        let none = format!(
            "{}.{}.",
            enc(r#"{"alg":"none"}"#),
            enc(r#"{"sub":"1","password":"x"}"#)
        );
        let m = msg(
            "https",
            "/api",
            &format!("Authorization: Bearer {none}\r\n"),
            "HTTP/1.1 200 OK\r\n\r\n",
        );
        let jwt = run(&m, "jwt");
        let titles: Vec<&str> = jwt.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "Unsigned JWT (alg none) in the Authorization header",
                "JWT without expiry in the Authorization header",
                "JWT claims include “password”"
            ]
        );
        let signed = format!("{}.{}.c2ln", enc(r#"{"alg":"HS256"}"#), enc(r#"{"exp":1}"#));
        let ok = msg(
            "https",
            "/",
            &format!("Cookie: jwt={signed}\r\n"),
            "HTTP/1.1 200 OK\r\n\r\n",
        );
        assert!(run(&ok, "jwt").is_empty());
    }

    #[test]
    fn errors_and_addresses() {
        let sql = msg(
            "http",
            "/item",
            "",
            "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/html\r\n\r\n\
             <b>Warning</b>: mysqli_query(): You have an error in your SQL syntax near ''1''",
        );
        let errors = run(&sql, "error-messages");
        assert_eq!(errors.len(), 1);
        assert_eq!(
            (errors[0].severity, errors[0].confidence),
            (Severity::Medium, Confidence::Tentative)
        );

        let java = msg(
            "http",
            "/x",
            "",
            "HTTP/1.1 500 Error\r\nContent-Type: text/plain\r\n\r\njava.lang.NullPointerException\n\
             \tat com.shop.web.CartController.add(CartController.java:42)",
        );
        assert_eq!(
            run(&java, "error-messages")[0].title,
            "Java stack trace on /x"
        );

        let leak = msg(
            "https",
            "/",
            "",
            "HTTP/1.1 200 OK\r\nX-Backend: 10.0.12.7\r\nContent-Type: text/plain\r\n\r\n\
             upstream 192.168.1.20 and version 10.300.1.1",
        );
        let ips: Vec<String> = run(&leak, "private-ip")
            .iter()
            .map(|i| i.dedup.clone())
            .collect();
        assert_eq!(
            ips,
            vec![
                "https://shop.example:443|10.0.12.7",
                "https://shop.example:443|192.168.1.20"
            ]
        );
        // Labs on private addresses are not reported.
        let lab = Message::new(
            "http",
            "192.168.1.5",
            80,
            "/",
            b"GET / HTTP/1.1\r\n\r\n",
            Some(b"HTTP/1.1 200 OK\r\n\r\n10.0.0.1"),
        );
        assert!(run(&lab, "private-ip").is_empty());
    }

    #[test]
    fn caching_and_paths() {
        let m = msg(
            "https",
            "/account",
            "Cookie: PHPSESSID=abc\r\n",
            "HTTP/1.1 200 OK\r\nCache-Control: max-age=600\r\n\r\n",
        );
        assert_eq!(run(&m, "cache-control").len(), 1);
        let private = msg(
            "https",
            "/account",
            "Authorization: Basic x\r\n",
            "HTTP/1.1 200 OK\r\nCache-Control: private, max-age=0\r\n\r\n",
        );
        assert!(run(&private, "cache-control").is_empty());

        let path = |p: &str| {
            run(
                &msg("http", p, "", "HTTP/1.1 200 OK\r\n\r\n"),
                "interesting-path",
            )
        };
        assert_eq!(path("/.git/config")[0].severity, Severity::Low);
        assert_eq!(path("/admin/users")[0].severity, Severity::Info);
        assert_eq!(
            path("/db.sql")[0].title,
            "Reachable backup or source file (.sql): /db.sql"
        );
        assert!(path("/administrator-guide").is_empty());
        assert!(path("/products").is_empty());
        let missing = msg("http", "/.env", "", "HTTP/1.1 404 Not Found\r\n\r\n");
        assert!(run(&missing, "interesting-path").is_empty());
    }
}
