//! Cookies: reading `Set-Cookie` fields and deciding which requests a stored
//! cookie belongs to (RFC 6265 §5.1–§5.4).
//!
//! This is what a cookie jar needs; it is deliberately lenient when reading
//! (servers send all kinds of dates and spacing) and strict when matching.
//! Without a public-suffix list, a `Domain` attribute naming a single label
//! (such as `com`) is refused unless it is the host itself.

use std::net::IpAddr;
use std::time::UNIX_EPOCH;

/// A cookie as set by one `Set-Cookie` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetCookie {
    /// Cookie name.
    pub name: String,
    /// Cookie value (quotes, if any, kept).
    pub value: String,
    /// `Domain` attribute, lower case and without a leading dot.
    pub domain: Option<String>,
    /// `Path` attribute, when it is a valid absolute path.
    pub path: Option<String>,
    /// When it expires, Unix epoch ms (`Max-Age` wins over `Expires`);
    /// `None` for a session cookie. A time in the past deletes the cookie.
    pub expires_at: Option<i64>,
    /// `Secure`: only sent over HTTPS.
    pub secure: bool,
    /// `HttpOnly`.
    pub http_only: bool,
    /// `SameSite` value as written.
    pub same_site: Option<String>,
}

impl SetCookie {
    /// Reads one `Set-Cookie` field value. `None` if it sets nothing (no
    /// `=` or an empty name).
    pub fn parse(field: &str, now_ms: i64) -> Option<SetCookie> {
        let mut parts = field.split(';');
        let pair = parts.next()?;
        let (name, value) = pair.split_once('=')?;
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        let mut cookie = SetCookie {
            name: name.to_string(),
            value: value.trim().to_string(),
            domain: None,
            path: None,
            expires_at: None,
            secure: false,
            http_only: false,
            same_site: None,
        };
        let mut max_age: Option<i64> = None;
        let mut expires: Option<i64> = None;
        for attribute in parts {
            let (key, val) = match attribute.split_once('=') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => (attribute.trim(), ""),
            };
            match key.to_ascii_lowercase().as_str() {
                "expires" => expires = parse_date(val).or(expires),
                "max-age" => {
                    let valid = val.starts_with(|c: char| c.is_ascii_digit() || c == '-');
                    if let (true, Ok(seconds)) = (valid, val.parse::<i64>()) {
                        max_age = Some(if seconds <= 0 {
                            0
                        } else {
                            now_ms.saturating_add(seconds.saturating_mul(1000))
                        });
                    }
                }
                "domain" => {
                    let d = val.trim_start_matches('.').to_ascii_lowercase();
                    cookie.domain = (!d.is_empty()).then_some(d);
                }
                "path" => cookie.path = val.starts_with('/').then(|| val.to_string()),
                "secure" => cookie.secure = true,
                "httponly" => cookie.http_only = true,
                "samesite" => cookie.same_site = (!val.is_empty()).then(|| val.to_string()),
                _ => {}
            }
        }
        cookie.expires_at = max_age.or(expires);
        Some(cookie)
    }

    /// True if storing this cookie deletes it instead (expiry in the past).
    pub fn is_expired(&self, now_ms: i64) -> bool {
        self.expires_at.is_some_and(|t| t <= now_ms)
    }
}

/// Unix epoch ms of a cookie date: the HTTP date formats, also with dashes
/// (`Wed, 21-Oct-2015 07:28:00 GMT`). Dates before 1970 give 0.
fn parse_date(text: &str) -> Option<i64> {
    let parsed = httpdate::parse_http_date(text)
        .or_else(|_| httpdate::parse_http_date(&text.replace('-', " ")))
        .ok()?;
    Some(match parsed.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    })
}

/// Where a cookie set in a response is kept: its domain, whether it is sent
/// to that exact host only, and its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieScope {
    /// Domain (lower case).
    pub domain: String,
    /// Sent to `domain` only, not to its subdomains (no `Domain` attribute).
    pub host_only: bool,
    /// Path prefix.
    pub path: String,
}

/// The scope of a cookie set by a response to `host` for `request_path`,
/// or `None` if the host may not set it (a `Domain` it does not belong to).
pub fn scope_of(cookie: &SetCookie, host: &str, request_path: &str) -> Option<CookieScope> {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    let path = cookie
        .path
        .clone()
        .unwrap_or_else(|| default_path(request_path));
    match &cookie.domain {
        None => Some(CookieScope {
            domain: host,
            host_only: true,
            path,
        }),
        Some(domain) if *domain == host => Some(CookieScope {
            domain: host,
            host_only: false,
            path,
        }),
        Some(domain) => {
            let allowed = domain.contains('.')
                && host.ends_with(&format!(".{domain}"))
                && host.parse::<IpAddr>().is_err();
            allowed.then(|| CookieScope {
                domain: domain.clone(),
                host_only: false,
                path,
            })
        }
    }
}

/// The default cookie path for a request path (RFC 6265 §5.1.4): its
/// directory, or `/`.
pub fn default_path(request_path: &str) -> String {
    let path = request_path.split(['?', '#']).next().unwrap_or_default();
    if !path.starts_with('/') {
        return "/".into();
    }
    match path.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => path[..i].to_string(),
    }
}

/// True if a cookie for `domain` is sent to `host`.
pub fn domain_matches(host: &str, domain: &str, host_only: bool) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if host == domain {
        return true;
    }
    !host_only && host.ends_with(&format!(".{domain}")) && host.parse::<IpAddr>().is_err()
}

/// True if a cookie for `cookie_path` is sent with a request for
/// `request_path` (query ignored).
pub fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    let path = request_path.split(['?', '#']).next().unwrap_or_default();
    let path = if path.is_empty() { "/" } else { path };
    path == cookie_path
        || (path.starts_with(cookie_path)
            && (cookie_path.ends_with('/') || path[cookie_path.len()..].starts_with('/')))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    #[test]
    fn reads_names_values_and_attributes() {
        let c = SetCookie::parse(
            " sid = a=b ; Path=/app; Domain=.Example.com; Secure; HttpOnly; SameSite=Lax",
            NOW,
        )
        .unwrap();
        assert_eq!((c.name.as_str(), c.value.as_str()), ("sid", "a=b"));
        assert_eq!(c.domain.as_deref(), Some("example.com"));
        assert_eq!(c.path.as_deref(), Some("/app"));
        assert!(c.secure && c.http_only);
        assert_eq!(c.same_site.as_deref(), Some("Lax"));
        assert_eq!(c.expires_at, None);

        assert!(SetCookie::parse("novalue", NOW).is_none());
        assert!(SetCookie::parse("=x", NOW).is_none());
        let empty = SetCookie::parse("a=; path=relative", NOW).unwrap();
        assert_eq!(empty.value, "");
        assert_eq!(empty.path, None);
    }

    #[test]
    fn expiry_from_max_age_or_dates() {
        let c = SetCookie::parse(
            "a=1; Max-Age=60; Expires=Wed, 21 Oct 2015 07:28:00 GMT",
            NOW,
        )
        .unwrap();
        assert_eq!(c.expires_at, Some(NOW + 60_000), "Max-Age wins");
        let past = SetCookie::parse("a=1; Expires=Wed, 21-Oct-2015 07:28:00 GMT", NOW).unwrap();
        assert_eq!(past.expires_at, Some(1_445_412_480_000));
        assert!(past.is_expired(NOW));
        let gone = SetCookie::parse("a=1; Max-Age=0", NOW).unwrap();
        assert!(gone.is_expired(NOW));
        let bad = SetCookie::parse("a=1; Max-Age=soon; Expires=never", NOW).unwrap();
        assert_eq!(bad.expires_at, None);
    }

    #[test]
    fn scopes_follow_the_host_and_path() {
        let c = |s: &str| SetCookie::parse(s, NOW).unwrap();
        assert_eq!(
            scope_of(&c("a=1"), "WWW.Example.com", "/shop/cart?x=1"),
            Some(CookieScope {
                domain: "www.example.com".into(),
                host_only: true,
                path: "/shop".into()
            })
        );
        let parent = scope_of(&c("a=1; Domain=example.com"), "www.example.com", "/").unwrap();
        assert!(!parent.host_only && parent.domain == "example.com");
        assert!(scope_of(&c("a=1; Domain=other.com"), "www.example.com", "/").is_none());
        assert!(scope_of(&c("a=1; Domain=com"), "example.com", "/").is_none());
        assert!(scope_of(&c("a=1; Domain=0.1"), "127.0.0.1", "/").is_none());
        assert!(scope_of(&c("a=1; Domain=localhost"), "localhost", "/").is_some());
    }

    #[test]
    fn matching_domains_and_paths() {
        assert!(domain_matches("a.example.com", "example.com", false));
        assert!(!domain_matches("a.example.com", "example.com", true));
        assert!(domain_matches("Example.com", "example.com", true));
        assert!(!domain_matches("badexample.com", "example.com", false));

        assert_eq!(default_path("/"), "/");
        assert_eq!(default_path("/a"), "/");
        assert_eq!(default_path("/a/b/c?q=/x"), "/a/b");
        assert_eq!(default_path("*"), "/");

        assert!(path_matches("/shop/cart", "/shop"));
        assert!(path_matches("/shop?x", "/shop"));
        assert!(path_matches("/shop/", "/shop/"));
        assert!(!path_matches("/shopping", "/shop"));
        assert!(path_matches("", "/"));
    }
}
