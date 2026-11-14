//! Request-target forms (RFC 9112 §3.2) and authority parsing.

use crate::HttpError;

/// URI scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scheme {
    /// `http`
    Http,
    /// `https`
    Https,
}

impl Scheme {
    /// Lowercase scheme name.
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }

    /// Default TCP port.
    pub fn default_port(self) -> u16 {
        match self {
            Scheme::Http => 80,
            Scheme::Https => 443,
        }
    }
}

/// The four request-target forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestTarget {
    /// `/path?query` — sent to origin servers.
    Origin(String),
    /// `http://host:port/path?query` — sent to proxies.
    Absolute {
        /// Scheme.
        scheme: Scheme,
        /// Lowercased host (IPv6 without brackets).
        host: String,
        /// Port (default for the scheme when absent).
        port: u16,
        /// Origin-form equivalent (`/path?query`).
        path: String,
    },
    /// `host:port` — only for `CONNECT`.
    Authority {
        /// Lowercased host.
        host: String,
        /// Port.
        port: u16,
    },
    /// `*` — only for `OPTIONS`.
    Asterisk,
}

impl RequestTarget {
    /// Classifies the target of a request.
    pub fn parse(method: &str, target: &str) -> Result<RequestTarget, HttpError> {
        if method.eq_ignore_ascii_case("CONNECT") {
            let (host, port) = parse_authority(target, None)?;
            return Ok(RequestTarget::Authority { host, port });
        }
        if target == "*" {
            return Ok(RequestTarget::Asterisk);
        }
        if target.starts_with('/') {
            return Ok(RequestTarget::Origin(target.to_string()));
        }
        let lower = target.get(..8).unwrap_or(target).to_ascii_lowercase();
        let (scheme, rest) = if lower.starts_with("http://") {
            (Scheme::Http, &target[7..])
        } else if lower.starts_with("https://") {
            (Scheme::Https, &target[8..])
        } else {
            return Err(HttpError::BadTarget(format!(
                "unsupported target {target:?}"
            )));
        };
        let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..auth_end];
        // Drop userinfo; it must never be forwarded in the request line.
        let authority = authority.rsplit_once('@').map_or(authority, |(_, a)| a);
        let (host, port) = parse_authority(authority, Some(scheme.default_port()))?;
        let remainder = &rest[auth_end..];
        let remainder = remainder.split('#').next().unwrap_or("");
        let path = if remainder.starts_with('/') {
            remainder.to_string()
        } else {
            format!("/{remainder}")
        };
        Ok(RequestTarget::Absolute {
            scheme,
            host,
            port,
            path,
        })
    }
}

/// Parses `host[:port]`, supporting bracketed IPv6 literals.
/// `default_port` of `None` makes the port mandatory.
pub fn parse_authority(s: &str, default_port: Option<u16>) -> Result<(String, u16), HttpError> {
    let bad = || HttpError::BadTarget(format!("invalid authority {s:?}"));
    let (host, port_str) = if let Some(rest) = s.strip_prefix('[') {
        let close = rest.find(']').ok_or_else(bad)?;
        let host = &rest[..close];
        let after = &rest[close + 1..];
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p),
            None if after.is_empty() => None,
            None => return Err(bad()),
        };
        if host.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(bad());
        }
        (host, port)
    } else {
        match s.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (s, None),
        }
    };
    if host.is_empty()
        || host.len() > 253
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':' | b'%'))
    {
        return Err(bad());
    }
    let port = match port_str {
        Some(p) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => {
            p.parse::<u16>().map_err(|_| bad())?
        }
        Some(_) => return Err(bad()),
        None => default_port.ok_or_else(bad)?,
    };
    if port == 0 {
        return Err(bad());
    }
    Ok((host.to_ascii_lowercase(), port))
}

/// Formats a URL, omitting the port when it is the scheme default.
pub fn format_url(scheme: Scheme, host: &str, port: u16, path: &str) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    if port == scheme.default_port() {
        format!("{}://{host}{path}", scheme.as_str())
    } else {
        format!("{}://{host}:{port}{path}", scheme.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(t: &str) -> (Scheme, String, u16, String) {
        match RequestTarget::parse("GET", t).unwrap() {
            RequestTarget::Absolute {
                scheme,
                host,
                port,
                path,
            } => (scheme, host, port, path),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn absolute_forms() {
        assert_eq!(
            abs("http://Example.com/a?b"),
            (Scheme::Http, "example.com".into(), 80, "/a?b".into())
        );
        assert_eq!(
            abs("HTTP://h:8000"),
            (Scheme::Http, "h".into(), 8000, "/".into())
        );
        assert_eq!(abs("http://h?x=1").3, "/?x=1");
        assert_eq!(
            abs("http://u:p@h/x#frag"),
            (Scheme::Http, "h".into(), 80, "/x".into())
        );
        assert_eq!(abs("https://[::1]:8443/").1, "::1");
        assert_eq!(abs("https://[::1]/").2, 443);
    }

    #[test]
    fn other_forms() {
        assert_eq!(
            RequestTarget::parse("GET", "/x").unwrap(),
            RequestTarget::Origin("/x".into())
        );
        assert_eq!(
            RequestTarget::parse("OPTIONS", "*").unwrap(),
            RequestTarget::Asterisk
        );
        assert_eq!(
            RequestTarget::parse("CONNECT", "a.b:443").unwrap(),
            RequestTarget::Authority {
                host: "a.b".into(),
                port: 443
            }
        );
    }

    #[test]
    fn rejects_bad_targets() {
        for (m, t) in [
            ("CONNECT", "a.b"),
            ("CONNECT", "a.b:0"),
            ("CONNECT", "a.b:99999"),
            ("GET", "ftp://x/"),
            ("GET", "http:///x"),
            ("GET", "http://a b/"),
            ("GET", "http://[zz]/"),
            ("GET", "http://h:x/"),
        ] {
            assert!(RequestTarget::parse(m, t).is_err(), "{m} {t}");
        }
    }

    #[test]
    fn formats_urls() {
        assert_eq!(format_url(Scheme::Http, "a", 80, "/"), "http://a/");
        assert_eq!(format_url(Scheme::Http, "a", 81, "/x"), "http://a:81/x");
        assert_eq!(format_url(Scheme::Https, "::1", 443, "/"), "https://[::1]/");
    }
}
