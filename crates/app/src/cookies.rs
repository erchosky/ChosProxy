//! The cookie jar seen from traffic: cookies set by responses become jar
//! entries, and jar cookies are added to Repeater requests that ask for it.

use chosproxy_http::ResponseHead;
use chosproxy_http::cookies::{SetCookie, domain_matches, path_matches, scope_of};
use chosproxy_http::head::find_head_end;
use chosproxy_storage::{JarCookie, JarEntry};

/// Jar entries for the `Set-Cookie` fields of a raw response to `host` and
/// `request_path`. Cheap when the response sets no cookie.
pub(crate) fn entries_from_response(
    response: &[u8],
    host: &str,
    request_path: &str,
    now_ms: i64,
) -> Vec<JarEntry> {
    let Some(end) = find_head_end(response) else {
        return Vec::new();
    };
    let head = &response[..end];
    if !head
        .windows(11)
        .any(|w| w.eq_ignore_ascii_case(b"set-cookie:"))
    {
        return Vec::new();
    }
    let Ok(head) = ResponseHead::parse(head.to_vec()) else {
        return Vec::new();
    };
    head.headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("set-cookie"))
        .filter_map(|h| SetCookie::parse(&String::from_utf8_lossy(&h.value), now_ms))
        .filter_map(|cookie| {
            let scope = scope_of(&cookie, host, request_path)?;
            Some(JarEntry {
                expired: cookie.is_expired(now_ms),
                domain: scope.domain,
                host_only: scope.host_only,
                path: scope.path,
                name: cookie.name,
                value: cookie.value,
                expires_at: cookie.expires_at,
                secure: cookie.secure,
                http_only: cookie.http_only,
                same_site: cookie.same_site,
            })
        })
        .collect()
}

/// The jar cookies a request to `host`/`path` carries, in the order a
/// browser sends them (longer paths first, then older first).
pub(crate) fn cookies_for<'a>(
    jar: &'a [JarCookie],
    host: &str,
    path: &str,
    https: bool,
    now_ms: i64,
) -> Vec<&'a JarCookie> {
    let mut matching: Vec<&JarCookie> = jar
        .iter()
        .filter(|c| c.expires_at.is_none_or(|t| t > now_ms))
        .filter(|c| https || !c.secure)
        .filter(|c| domain_matches(host, &c.domain, c.host_only))
        .filter(|c| path_matches(path, &c.path))
        .collect();
    matching.sort_by(|a, b| {
        b.path
            .len()
            .cmp(&a.path.len())
            .then(a.created_at.cmp(&b.created_at))
    });
    // One value per name: the most specific wins.
    let mut seen = std::collections::HashSet::new();
    matching.retain(|c| seen.insert(c.name.clone()));
    matching
}

/// Sets `cookies` in the `Cookie` header of a raw request: values of
/// cookies already there are replaced, the others are added (a `Cookie`
/// header is created if needed). Untouched headers keep their bytes.
/// Returns the request and the names of the cookies that changed it.
pub(crate) fn apply_cookies(request: &[u8], cookies: &[&JarCookie]) -> (Vec<u8>, Vec<String>) {
    let Some(end) = find_head_end(request) else {
        return (request.to_vec(), Vec::new());
    };
    if cookies.is_empty() {
        return (request.to_vec(), Vec::new());
    }
    let (head, body) = request.split_at(end);
    let crlf = head.windows(2).any(|w| w == b"\r\n");
    let eol: &[u8] = if crlf { b"\r\n" } else { b"\n" };
    // Lines of the head without the empty line that ends it.
    let text = String::from_utf8_lossy(head).into_owned();
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }

    let mut pending: Vec<&JarCookie> = cookies.to_vec();
    let mut changed: Vec<String> = Vec::new();
    let mut first_cookie_line = None;
    for (i, line) in lines.iter_mut().enumerate().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("cookie") {
            continue;
        }
        first_cookie_line.get_or_insert(i);
        let mut pairs: Vec<(String, String)> = value
            .split(';')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| match p.split_once('=') {
                Some((n, v)) => (n.trim().to_string(), v.to_string()),
                None => (String::new(), p.to_string()),
            })
            .collect();
        let mut touched = false;
        for (n, v) in &mut pairs {
            if let Some(at) = pending.iter().position(|c| c.name == *n) {
                let cookie = pending.remove(at);
                if *v != cookie.value {
                    v.clone_from(&cookie.value);
                    touched = true;
                    changed.push(cookie.name.clone());
                }
            }
        }
        if touched {
            *line = format!("{}: {}", name.trim_end(), join(&pairs));
        }
    }
    if !pending.is_empty() {
        let added: Vec<(String, String)> = pending
            .iter()
            .map(|c| (c.name.clone(), c.value.clone()))
            .collect();
        changed.extend(pending.iter().map(|c| c.name.clone()));
        match first_cookie_line {
            Some(i) => {
                if let Some(line) = lines.get_mut(i) {
                    let sep = if line.trim_end().ends_with(':') {
                        " "
                    } else {
                        "; "
                    };
                    line.push_str(sep);
                    line.push_str(&join(&added));
                }
            }
            None => lines.push(format!("Cookie: {}", join(&added))),
        }
    }
    if changed.is_empty() {
        return (request.to_vec(), changed);
    }
    let mut out = Vec::with_capacity(request.len() + 64);
    for line in &lines {
        out.extend_from_slice(line.as_bytes());
        out.extend_from_slice(eol);
    }
    out.extend_from_slice(eol);
    out.extend_from_slice(body);
    (out, changed)
}

fn join(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(n, v)| {
            if n.is_empty() {
                v.clone()
            } else {
                format!("{n}={v}")
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    fn jar(name: &str, value: &str, path: &str, created_at: i64) -> JarCookie {
        JarCookie {
            id: created_at,
            domain: "lab.test".into(),
            host_only: false,
            path: path.into(),
            name: name.into(),
            value: value.into(),
            expires_at: None,
            secure: false,
            http_only: false,
            same_site: None,
            source: "proxy".into(),
            created_at,
            updated_at: created_at,
        }
    }

    #[test]
    fn entries_from_set_cookie_fields() {
        let response = b"HTTP/1.1 200 OK\r\nSet-Cookie: sid=1; Path=/\r\n\
            set-cookie: gone=; Max-Age=0\r\nSet-Cookie: evil=1; Domain=other.test\r\n\r\nbody";
        let entries = entries_from_response(response, "www.lab.test", "/login", NOW);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            (entries[0].name.as_str(), entries[0].path.as_str()),
            ("sid", "/")
        );
        assert!(entries[0].host_only && !entries[0].expired);
        assert!(entries[1].expired);
        assert!(entries_from_response(b"HTTP/1.1 200 OK\r\n\r\n", "x", "/", NOW).is_empty());
    }

    #[test]
    fn picks_matching_cookies_most_specific_first() {
        let mut secure = jar("s", "1", "/", 1);
        secure.secure = true;
        let mut expired = jar("e", "1", "/", 2);
        expired.expires_at = Some(NOW - 1);
        let all = vec![
            jar("sid", "root", "/", 3),
            jar("sid", "app", "/app", 4),
            jar("other", "x", "/elsewhere", 5),
            secure,
            expired,
        ];
        let names = |v: Vec<&JarCookie>| {
            v.iter()
                .map(|c| format!("{}={}", c.name, c.value))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(cookies_for(&all, "api.lab.test", "/app/x", false, NOW)),
            vec!["sid=app"]
        );
        assert_eq!(
            names(cookies_for(&all, "lab.test", "/", true, NOW)),
            vec!["s=1", "sid=root"]
        );
        assert!(cookies_for(&all, "other.test", "/", true, NOW).is_empty());
    }

    #[test]
    fn replaces_and_adds_cookies() {
        let sid = jar("sid", "new", "/", 1);
        let csrf = jar("csrf", "t", "/", 2);
        let request = b"POST /a HTTP/1.1\r\nHost: lab.test\r\nCookie: theme=dark; sid=old\r\nContent-Length: 2\r\n\r\nhi";
        let (out, changed) = apply_cookies(request, &[&sid, &csrf]);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "POST /a HTTP/1.1\r\nHost: lab.test\r\nCookie: theme=dark; sid=new; csrf=t\r\n\
             Content-Length: 2\r\n\r\nhi"
        );
        assert_eq!(changed, vec!["sid", "csrf"]);

        // No Cookie header: one is added; LF line endings are kept.
        let (out, _) = apply_cookies(b"GET / HTTP/1.1\nHost: lab.test\n\n", &[&sid]);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "GET / HTTP/1.1\nHost: lab.test\nCookie: sid=new\n\n"
        );

        // Same values: the request is untouched, byte for byte.
        let same = b"GET / HTTP/1.1\r\nCookie:sid=new\r\n\r\n";
        let (out, changed) = apply_cookies(same, &[&sid]);
        assert_eq!(out, same);
        assert!(changed.is_empty());
    }
}
