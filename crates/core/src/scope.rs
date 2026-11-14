//! The scope engine: which targets the user is working on.
//!
//! A target is in scope when at least one enabled include rule matches it and
//! no enabled exclude rule does. With no include rules nothing is in scope.
//! The configuration lives in `scope.toml`; [`Scope`] is its compiled form,
//! a pure function from a target to a [`ScopeDecision`].

use crate::config::ConfigSection;
use crate::config::{host_matches, valid_host_pattern};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Protocols a rule applies to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ScopeProtocol {
    /// HTTP and HTTPS.
    #[default]
    Any,
    /// Plain HTTP only.
    Http,
    /// HTTPS only.
    Https,
}

/// One include or exclude rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct ScopeRule {
    /// Disabled rules are kept but ignored.
    pub enabled: bool,
    /// Protocols the rule applies to.
    pub protocol: ScopeProtocol,
    /// Host: `example.com`, or a regular expression matching the whole host
    /// when `regex` is set. Empty = any host.
    pub host: String,
    /// Also match subdomains of `host` (not used with `regex`).
    pub include_subdomains: bool,
    /// Port; `None` = any port.
    pub port: Option<u16>,
    /// Path (without query): a prefix such as `/api/`, or a regular
    /// expression searched in the path when `regex` is set. Empty = any path.
    pub path: String,
    /// `host` and `path` are regular expressions.
    pub regex: bool,
    /// Free text shown next to the rule.
    pub comment: String,
}

impl Default for ScopeRule {
    fn default() -> Self {
        ScopeRule {
            enabled: true,
            protocol: ScopeProtocol::Any,
            host: String::new(),
            include_subdomains: false,
            port: None,
            path: String::new(),
            regex: false,
            comment: String::new(),
        }
    }
}

/// Scope configuration (`scope.toml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct ScopeConfig {
    /// Schema version of this file.
    pub version: u32,
    /// A target must match one of these…
    pub include: Vec<ScopeRule>,
    /// …and none of these.
    pub exclude: Vec<ScopeRule>,
    /// Proxy traffic to out-of-scope targets is forwarded but not stored
    /// (history, WebSockets, cookie jar). Ignored while no scope is defined.
    pub skip_out_of_scope_proxy: bool,
}

impl Default for ScopeConfig {
    fn default() -> Self {
        ScopeConfig {
            version: 1,
            include: Vec::new(),
            exclude: Vec::new(),
            skip_out_of_scope_proxy: false,
        }
    }
}

impl ScopeConfig {
    /// Most rules per list.
    pub const MAX_RULES: usize = 500;
}

impl ConfigSection for ScopeConfig {
    const FILE_NAME: &'static str = "scope.toml";

    fn validate(&self) -> Result<(), String> {
        Scope::compile(self).map(|_| ())
    }
}

/// What a scope check needs to know about a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeTarget<'a> {
    /// `http` or `https`.
    pub scheme: &'a str,
    /// Host name or IP (IPv6 without brackets).
    pub host: &'a str,
    /// Port.
    pub port: u16,
    /// Path; anything from `?` on is ignored.
    pub path: &'a str,
}

/// The result of a scope check, with the rules that decided it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScopeDecision {
    /// The target is in scope.
    pub in_scope: bool,
    /// Index of the first matching include rule.
    pub include_rule: Option<u32>,
    /// Index of the first matching exclude rule.
    pub exclude_rule: Option<u32>,
}

#[derive(Debug, Clone)]
enum HostMatcher {
    Any,
    Exact { host: String, subdomains: bool },
    Regex(Regex),
}

#[derive(Debug, Clone)]
enum PathMatcher {
    Any,
    Prefix(String),
    Regex(Regex),
}

#[derive(Debug, Clone)]
struct CompiledRule {
    protocol: ScopeProtocol,
    host: HostMatcher,
    port: Option<u16>,
    path: PathMatcher,
}

/// Upper bound for one compiled regular expression.
const REGEX_SIZE_LIMIT: usize = 1 << 20;

fn build_regex(pattern: &str, anchored: bool, case_insensitive: bool) -> Result<Regex, String> {
    let source = if anchored {
        format!("^(?:{pattern})$")
    } else {
        pattern.to_string()
    };
    RegexBuilder::new(&source)
        .case_insensitive(case_insensitive)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|e| format!("invalid regular expression {pattern:?}: {e}"))
}

impl CompiledRule {
    fn compile(rule: &ScopeRule) -> Result<CompiledRule, String> {
        let host = rule.host.trim();
        let path = rule.path.trim();
        let host = if host.is_empty() {
            HostMatcher::Any
        } else if rule.regex {
            HostMatcher::Regex(build_regex(host, true, true)?)
        } else {
            if host.starts_with("*.") || !valid_host_pattern(host) {
                return Err(format!(
                    "invalid host {host:?}: use example.com (tick \"include subdomains\" for \
                     *.example.com) or switch the rule to regex"
                ));
            }
            HostMatcher::Exact {
                host: host.trim_end_matches('.').to_ascii_lowercase(),
                subdomains: rule.include_subdomains,
            }
        };
        let path = if path.is_empty() {
            PathMatcher::Any
        } else if rule.regex {
            PathMatcher::Regex(build_regex(path, false, false)?)
        } else {
            if !path.starts_with('/') {
                return Err(format!("invalid path {path:?}: it must start with /"));
            }
            PathMatcher::Prefix(path.to_string())
        };
        if rule.port == Some(0) {
            return Err("port 0 is not a valid port".into());
        }
        Ok(CompiledRule {
            protocol: rule.protocol,
            host,
            port: rule.port,
            path,
        })
    }

    fn matches(&self, t: &ScopeTarget<'_>) -> bool {
        let protocol_ok = match self.protocol {
            ScopeProtocol::Any => true,
            ScopeProtocol::Http => t.scheme.eq_ignore_ascii_case("http"),
            ScopeProtocol::Https => t.scheme.eq_ignore_ascii_case("https"),
        };
        if !protocol_ok || self.port.is_some_and(|p| p != t.port) {
            return false;
        }
        let host = t.host.trim_end_matches('.');
        let host_ok = match &self.host {
            HostMatcher::Any => true,
            HostMatcher::Exact {
                host: h,
                subdomains,
            } => {
                h.eq_ignore_ascii_case(host)
                    || (*subdomains && host_matches(&format!("*.{h}"), host))
            }
            HostMatcher::Regex(re) => re.is_match(host),
        };
        if !host_ok {
            return false;
        }
        let path = t.path.split(['?', '#']).next().unwrap_or("");
        let path = if path.is_empty() { "/" } else { path };
        match &self.path {
            PathMatcher::Any => true,
            PathMatcher::Prefix(p) => path.starts_with(p.as_str()),
            PathMatcher::Regex(re) => re.is_match(path),
        }
    }
}

/// Compiled scope. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    include: Vec<Option<CompiledRule>>,
    exclude: Vec<Option<CompiledRule>>,
}

fn compile_list(list: &[ScopeRule], name: &str) -> Result<Vec<Option<CompiledRule>>, String> {
    if list.len() > ScopeConfig::MAX_RULES {
        return Err(format!("at most {} {name} rules", ScopeConfig::MAX_RULES));
    }
    list.iter()
        .enumerate()
        .map(|(i, r)| {
            // Disabled rules are validated too, so enabling one never fails.
            let compiled =
                CompiledRule::compile(r).map_err(|e| format!("{name} rule {}: {e}", i + 1))?;
            Ok(r.enabled.then_some(compiled))
        })
        .collect()
}

fn first_match(rules: &[Option<CompiledRule>], t: &ScopeTarget<'_>) -> Option<u32> {
    rules
        .iter()
        .position(|r| r.as_ref().is_some_and(|r| r.matches(t)))
        .and_then(|i| u32::try_from(i).ok())
}

impl Scope {
    /// Compiles and validates a configuration.
    pub fn compile(config: &ScopeConfig) -> Result<Scope, String> {
        Ok(Scope {
            include: compile_list(&config.include, "include")?,
            exclude: compile_list(&config.exclude, "exclude")?,
        })
    }

    /// True when no enabled include rule exists (nothing is in scope).
    pub fn is_empty(&self) -> bool {
        self.include.iter().all(Option::is_none)
    }

    /// Decides whether `target` is in scope.
    pub fn check(&self, target: &ScopeTarget<'_>) -> ScopeDecision {
        let include_rule = first_match(&self.include, target);
        let exclude_rule = include_rule.and_then(|_| first_match(&self.exclude, target));
        ScopeDecision {
            in_scope: include_rule.is_some() && exclude_rule.is_none(),
            include_rule,
            exclude_rule,
        }
    }

    /// Shorthand for `check(..).in_scope`.
    pub fn contains(&self, target: &ScopeTarget<'_>) -> bool {
        self.check(target).in_scope
    }

    /// Checks an absolute `http://` or `https://` URL.
    pub fn check_url(&self, url: &str) -> Result<ScopeDecision, String> {
        let parts = UrlParts::parse(url)?;
        Ok(self.check(&parts.target()))
    }
}

/// The pieces of an absolute URL that scope rules look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlParts {
    /// `http` or `https` (lower case).
    pub scheme: String,
    /// Host (IPv6 without brackets).
    pub host: String,
    /// Explicit or default port.
    pub port: u16,
    /// Path and query (`/` when absent).
    pub path: String,
}

impl UrlParts {
    /// Parses `scheme://host[:port][/path]`.
    pub fn parse(url: &str) -> Result<UrlParts, String> {
        let url = url.trim();
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| "enter an absolute URL such as https://example.com/".to_string())?;
        let scheme = scheme.to_ascii_lowercase();
        let default_port = match scheme.as_str() {
            "http" => 80,
            "https" => 443,
            _ => return Err(format!("unsupported scheme {scheme:?}: use http or https")),
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, path) = rest.split_at(end);
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
            let (h, after) = v6
                .split_once(']')
                .ok_or_else(|| "unterminated IPv6 address".to_string())?;
            (h, after.strip_prefix(':'))
        } else {
            match authority.rsplit_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (authority, None),
            }
        };
        if host.is_empty() {
            return Err("the URL has no host".into());
        }
        let port = match port {
            None | Some("") => default_port,
            Some(p) => p
                .parse::<u16>()
                .ok()
                .filter(|p| *p != 0)
                .ok_or_else(|| format!("invalid port {p:?}"))?,
        };
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        Ok(UrlParts {
            scheme,
            host: host.to_ascii_lowercase(),
            port,
            path,
        })
    }

    /// As a scope target.
    pub fn target(&self) -> ScopeTarget<'_> {
        ScopeTarget {
            scheme: &self.scheme,
            host: &self.host,
            port: self.port,
            path: &self.path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(host: &str) -> ScopeRule {
        ScopeRule {
            host: host.into(),
            ..ScopeRule::default()
        }
    }

    fn scope(include: Vec<ScopeRule>, exclude: Vec<ScopeRule>) -> Scope {
        Scope::compile(&ScopeConfig {
            include,
            exclude,
            ..ScopeConfig::default()
        })
        .unwrap()
    }

    fn url(s: &Scope, u: &str) -> bool {
        s.check_url(u).unwrap().in_scope
    }

    #[test]
    fn empty_scope_contains_nothing() {
        let s = scope(vec![], vec![]);
        assert!(s.is_empty());
        assert!(!url(&s, "http://example.com/"));
    }

    #[test]
    fn host_rules_with_and_without_subdomains() {
        let s = scope(vec![rule("lab.test")], vec![]);
        assert!(url(&s, "http://LAB.test/x"));
        assert!(url(&s, "https://lab.test./"));
        assert!(!url(&s, "http://a.lab.test/"));
        assert!(!url(&s, "http://evillab.test/"));
        let s = scope(
            vec![ScopeRule {
                include_subdomains: true,
                ..rule("lab.test")
            }],
            vec![],
        );
        assert!(url(&s, "http://lab.test/"));
        assert!(url(&s, "http://a.b.lab.test/"));
        assert!(!url(&s, "http://badlab.test/"));
    }

    #[test]
    fn protocol_port_and_path_prefix() {
        let s = scope(
            vec![ScopeRule {
                protocol: ScopeProtocol::Https,
                port: Some(8443),
                path: "/api/".into(),
                ..rule("lab.test")
            }],
            vec![],
        );
        assert!(url(&s, "https://lab.test:8443/api/users?id=1"));
        assert!(!url(&s, "http://lab.test:8443/api/users"));
        assert!(!url(&s, "https://lab.test/api/users"));
        assert!(!url(&s, "https://lab.test:8443/apix"));
        assert!(!url(&s, "https://lab.test:8443/static/api/"));
    }

    #[test]
    fn exclude_wins_and_reports_rules() {
        let s = scope(
            vec![rule("other.test"), rule("lab.test")],
            vec![ScopeRule {
                path: "/logout".into(),
                ..rule("")
            }],
        );
        let d = s.check_url("http://lab.test/logout").unwrap();
        assert_eq!(
            d,
            ScopeDecision {
                in_scope: false,
                include_rule: Some(1),
                exclude_rule: Some(0)
            }
        );
        assert!(url(&s, "http://lab.test/login"));
    }

    #[test]
    fn regex_rules() {
        let s = scope(
            vec![ScopeRule {
                regex: true,
                host: r"(dev|qa)\d*\.lab\.test".into(),
                path: r"\.php$".into(),
                ..rule("")
            }],
            vec![],
        );
        assert!(url(&s, "http://dev1.lab.test/a/index.php?x=1"));
        assert!(url(&s, "http://QA.lab.test/b.php"));
        assert!(
            !url(&s, "http://xdev1.lab.test/a.php"),
            "host regex is anchored"
        );
        assert!(!url(&s, "http://dev1.lab.test/a.php.bak"));
    }

    #[test]
    fn disabled_rules_are_ignored_but_validated() {
        let s = scope(
            vec![ScopeRule {
                enabled: false,
                ..rule("lab.test")
            }],
            vec![],
        );
        assert!(s.is_empty());
        let bad = ScopeConfig {
            include: vec![ScopeRule {
                enabled: false,
                regex: true,
                host: "(".into(),
                ..rule("")
            }],
            ..ScopeConfig::default()
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn invalid_rules_rejected() {
        for r in [
            rule("*.lab.test"),
            rule("bad host"),
            ScopeRule {
                path: "api".into(),
                ..rule("lab.test")
            },
            ScopeRule {
                port: Some(0),
                ..rule("lab.test")
            },
        ] {
            let c = ScopeConfig {
                include: vec![r.clone()],
                ..ScopeConfig::default()
            };
            assert!(c.validate().is_err(), "{r:?}");
        }
    }

    #[test]
    fn parses_urls() {
        let p = UrlParts::parse("HTTPS://user@[::1]:8443/a?b").unwrap();
        assert_eq!(
            (p.scheme.as_str(), p.host.as_str(), p.port),
            ("https", "::1", 8443)
        );
        assert_eq!(p.path, "/a?b");
        let p = UrlParts::parse("http://lab.test").unwrap();
        assert_eq!((p.port, p.path.as_str()), (80, "/"));
        assert!(UrlParts::parse("ftp://x/").is_err());
        assert!(UrlParts::parse("lab.test/").is_err());
        assert!(UrlParts::parse("http://lab.test:0/").is_err());
    }

    #[test]
    fn toml_round_trip() {
        let c = ScopeConfig {
            include: vec![ScopeRule {
                port: Some(3000),
                comment: "juice shop".into(),
                ..rule("localhost")
            }],
            ..ScopeConfig::default()
        };
        let text = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<ScopeConfig>(&text).unwrap(), c);
    }
}
