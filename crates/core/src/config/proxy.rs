use super::{ConfigSection, ProxyAuth};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use ts_rs::TS;

/// Proxy listener configuration (`proxy.toml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct ProxyConfig {
    /// Schema version of this file.
    pub version: u32,
    /// Configured listeners.
    pub listeners: Vec<ListenerConfig>,
    /// HTTPS interception policy.
    pub tls: TlsConfig,
    /// Which messages are paused when Intercept is on.
    pub intercept: InterceptRules,
    /// Rules that change proxied messages automatically, in order.
    pub match_replace: Vec<MatchReplaceRule>,
}

impl ProxyConfig {
    /// Maximum number of listeners.
    pub const MAX_LISTENERS: usize = 16;

    /// Finds a listener by id.
    pub fn listener(&self, id: &str) -> Option<&ListenerConfig> {
        self.listeners.iter().find(|l| l.id == id)
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        ProxyConfig {
            version: 1,
            listeners: vec![ListenerConfig::default()],
            tls: TlsConfig::default(),
            intercept: InterceptRules::default(),
            match_replace: Vec::new(),
        }
    }
}

/// The part of a message a Match & Replace rule works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MatchTarget {
    /// The request line (`GET /path HTTP/1.1`).
    RequestFirstLine,
    /// Each request header line; an empty match adds the replacement as a
    /// new header, an empty result removes the line.
    RequestHeader,
    /// The request body (decoded first when compressed or chunked).
    RequestBody,
    /// The status line.
    ResponseFirstLine,
    /// Each response header line (same rules as request headers).
    ResponseHeader,
    /// The response body (decoded first when compressed or chunked).
    ResponseBody,
    /// The whole request, through `fn request(text)` of a rewrite script
    /// (`match` holds the script id).
    RequestScript,
    /// The whole response, through `fn response(text)` of a rewrite script
    /// (`match` holds the script id).
    ResponseScript,
    /// Text WebSocket messages sent by the client.
    WsToServer,
    /// Text WebSocket messages sent by the server.
    WsToClient,
    /// Text WebSocket messages in both directions, through
    /// `fn websocket(text, to_server)` of a rewrite script (M31b; `match`
    /// holds the script id).
    WsScript,
}

impl MatchTarget {
    /// True for request targets.
    pub fn is_request(self) -> bool {
        matches!(
            self,
            MatchTarget::RequestFirstLine
                | MatchTarget::RequestHeader
                | MatchTarget::RequestBody
                | MatchTarget::RequestScript
        )
    }

    /// True for body targets.
    pub fn is_body(self) -> bool {
        matches!(self, MatchTarget::RequestBody | MatchTarget::ResponseBody)
    }

    /// True for WebSocket message targets.
    pub fn is_websocket(self) -> bool {
        matches!(
            self,
            MatchTarget::WsToServer | MatchTarget::WsToClient | MatchTarget::WsScript
        )
    }

    /// True for script targets (they need the whole message).
    pub fn is_script(self) -> bool {
        matches!(
            self,
            MatchTarget::RequestScript | MatchTarget::ResponseScript | MatchTarget::WsScript
        )
    }
}

/// One Match & Replace rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct MatchReplaceRule {
    /// Whether the rule is applied.
    pub enabled: bool,
    /// Where it applies.
    pub target: MatchTarget,
    /// Text (or regular expression) to find.
    #[serde(rename = "match")]
    #[ts(rename = "match")]
    pub pattern: String,
    /// Replacement; with `regex`, `$1` / `${name}` insert groups.
    pub replace: String,
    /// `pattern` is a regular expression.
    #[serde(default)]
    pub regex: bool,
    /// What the rule is for (shown in the history notes).
    #[serde(default)]
    pub comment: String,
}

impl MatchReplaceRule {
    /// Maximum rules.
    pub const MAX_RULES: usize = 256;
    /// Compiled size limit of a regular expression.
    pub const REGEX_SIZE_LIMIT: usize = 1 << 20;

    /// Checks one rule (`n` is its position, for messages).
    pub fn validate(&self, n: usize) -> Result<(), String> {
        if self.target.is_script() {
            return if super::valid_script_id(&self.pattern) {
                Ok(())
            } else {
                Err(format!("rule {n}: choose a rewrite script"))
            };
        }
        let header = matches!(
            self.target,
            MatchTarget::RequestHeader | MatchTarget::ResponseHeader
        );
        if self.pattern.is_empty() && !header {
            return Err(format!("rule {n}: the text to find is empty"));
        }
        if self.pattern.is_empty() && self.replace.is_empty() {
            return Err(format!("rule {n}: give a header to add or a text to find"));
        }
        if self.pattern.len() > 4096 || self.replace.len() > 64 * 1024 {
            return Err(format!("rule {n}: text too long"));
        }
        if !self.target.is_body()
            && !self.target.is_websocket()
            && self.replace.contains(['\r', '\n'])
        {
            return Err(format!(
                "rule {n}: line breaks are only allowed in body replacements"
            ));
        }
        if self.regex {
            regex::bytes::RegexBuilder::new(&self.pattern)
                .size_limit(Self::REGEX_SIZE_LIMIT)
                .build()
                .map_err(|e| format!("rule {n}: invalid regular expression: {e}"))?;
        }
        Ok(())
    }
}

/// File extensions treated as static assets by interception rules.
pub const STATIC_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "avif", "ico", "bmp", "svg", "woff", "woff2", "ttf",
    "otf", "eot", "css", "map", "mp4", "webm", "mp3",
];

/// Rules deciding which messages Intercept pauses (Intercept itself is
/// switched on and off at runtime and always starts off).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct InterceptRules {
    /// Pause requests.
    pub requests: bool,
    /// Pause responses.
    pub responses: bool,
    /// Only requests that carry parameters (query string or body).
    pub only_with_parameters: bool,
    /// Skip images, fonts, stylesheets and media (by extension).
    pub ignore_static_assets: bool,
    /// Only targets in the current scope.
    pub only_in_scope: bool,
    /// Pause WebSocket messages sent by the client (host and scope rules apply).
    pub websocket_to_server: bool,
    /// Pause WebSocket messages sent by the server (host and scope rules apply).
    pub websocket_to_client: bool,
    /// Only these hosts (patterns as in [`TlsConfig`]); empty = all hosts.
    pub include_hosts: Vec<String>,
    /// Never these hosts.
    pub exclude_hosts: Vec<String>,
    /// Only these methods (upper case); empty = all methods.
    pub methods: Vec<String>,
    /// Only responses whose MIME category is listed (e.g. `HTML`, `JSON`);
    /// empty = all.
    pub response_mime: Vec<String>,
    /// Only URLs matching this regular expression (case-insensitive);
    /// empty = all.
    #[serde(default)]
    pub url_regex: String,
}

impl Default for InterceptRules {
    fn default() -> Self {
        InterceptRules {
            requests: true,
            responses: false,
            only_with_parameters: false,
            ignore_static_assets: true,
            only_in_scope: false,
            websocket_to_server: false,
            websocket_to_client: false,
            include_hosts: Vec::new(),
            exclude_hosts: Vec::new(),
            methods: Vec::new(),
            response_mime: Vec::new(),
            url_regex: String::new(),
        }
    }
}

/// What interception rules need to know about a request.
#[derive(Debug, Clone, Copy)]
pub struct RequestFacts<'a> {
    /// Target host.
    pub host: &'a str,
    /// Method.
    pub method: &'a str,
    /// Lowercase extension of the path, if any.
    pub extension: Option<&'a str>,
    /// The URL has a query string.
    pub has_query: bool,
    /// The request has a body.
    pub has_body: bool,
    /// The target is in the current scope.
    pub in_scope: bool,
    /// Full URL.
    pub url: &'a str,
}

thread_local! {
    /// The last regular expressions compiled for rules, by pattern.
    static RULE_REGEX: std::cell::RefCell<Vec<(String, Option<regex::Regex>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// True if `text` matches `pattern` (case-insensitive). An invalid pattern
/// matches nothing (patterns are validated when saved).
pub fn rule_regex_matches(pattern: &str, text: &str) -> bool {
    RULE_REGEX.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((_, re)) = cache.iter().find(|(p, _)| p == pattern) {
            return re.as_ref().is_some_and(|r| r.is_match(text));
        }
        let re = compile_rule_regex(pattern).ok();
        let hit = re.as_ref().is_some_and(|r| r.is_match(text));
        if cache.len() >= 16 {
            cache.remove(0);
        }
        cache.push((pattern.to_string(), re));
        hit
    })
}

fn compile_rule_regex(pattern: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| e.to_string())
}

impl InterceptRules {
    fn host_allowed(&self, host: &str) -> bool {
        (self.include_hosts.is_empty() || self.include_hosts.iter().any(|p| host_matches(p, host)))
            && !self.exclude_hosts.iter().any(|p| host_matches(p, host))
    }

    fn request_in_rules(&self, r: &RequestFacts<'_>) -> bool {
        self.host_allowed(r.host)
            && (self.methods.is_empty()
                || self
                    .methods
                    .iter()
                    .any(|m| m.eq_ignore_ascii_case(r.method)))
            && !(self.ignore_static_assets
                && r.extension.is_some_and(|e| STATIC_EXTENSIONS.contains(&e)))
            && (!self.only_with_parameters || r.has_query || r.has_body)
            && (!self.only_in_scope || r.in_scope)
            && (self.url_regex.is_empty() || rule_regex_matches(&self.url_regex, r.url))
    }

    /// Whether a WebSocket message on a connection to `host` should be
    /// paused (`to_server`: sent by the client).
    pub fn intercepts_websocket(&self, host: &str, in_scope: bool, to_server: bool) -> bool {
        (if to_server {
            self.websocket_to_server
        } else {
            self.websocket_to_client
        }) && self.host_allowed(host)
            && (!self.only_in_scope || in_scope)
    }

    /// Whether this request should be paused.
    pub fn intercepts_request(&self, r: &RequestFacts<'_>) -> bool {
        self.requests && self.request_in_rules(r)
    }

    /// Whether the response to this request (with MIME category `mime`)
    /// should be paused. `forced` = the user asked for this response.
    pub fn intercepts_response(&self, r: &RequestFacts<'_>, mime: &str, forced: bool) -> bool {
        forced
            || (self.responses
                && self.request_in_rules(r)
                && (self.response_mime.is_empty()
                    || self
                        .response_mime
                        .iter()
                        .any(|m| m.eq_ignore_ascii_case(mime))))
    }

    fn validate(&self) -> Result<(), String> {
        for (name, list) in [
            ("include", &self.include_hosts),
            ("exclude", &self.exclude_hosts),
        ] {
            if list.len() > TlsConfig::MAX_PATTERNS {
                return Err(format!("at most {} {name} hosts", TlsConfig::MAX_PATTERNS));
            }
            if let Some(bad) = list.iter().find(|p| !valid_host_pattern(p)) {
                return Err(format!(
                    "invalid {name} host {bad:?}: use example.com or *.example.com"
                ));
            }
        }
        if !self.url_regex.is_empty() {
            compile_rule_regex(&self.url_regex)
                .map_err(|e| format!("invalid URL regular expression: {e}"))?;
        }
        if let Some(bad) = self
            .methods
            .iter()
            .find(|m| m.is_empty() || m.len() > 32 || !m.bytes().all(|b| b.is_ascii_alphabetic()))
        {
            return Err(format!("invalid method {bad:?}"));
        }
        Ok(())
    }
}

/// HTTPS interception policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct TlsConfig {
    /// Decrypt HTTPS with the local CA. When off, every CONNECT is tunnelled.
    pub intercept: bool,
    /// Hosts never decrypted (pinned apps, sensitive sites). Patterns:
    /// `example.com` (exact) or `*.example.com` (any subdomain).
    pub passthrough_hosts: Vec<String>,
    /// Hosts whose upstream certificate is not verified, for labs with
    /// self-signed certificates. Same pattern syntax.
    pub skip_verification_hosts: Vec<String>,
    /// Also never decrypt `host:port` matching this regular expression
    /// (case-insensitive); empty = off.
    #[serde(default)]
    pub passthrough_regex: String,
}

impl Default for TlsConfig {
    fn default() -> Self {
        TlsConfig {
            intercept: true,
            passthrough_hosts: Vec::new(),
            // Local labs commonly use self-signed certificates; a loopback
            // upstream cannot be intercepted by a third party.
            skip_verification_hosts: vec!["localhost".into(), "127.0.0.1".into(), "::1".into()],
            passthrough_regex: String::new(),
        }
    }
}

impl TlsConfig {
    /// Maximum patterns per list.
    pub const MAX_PATTERNS: usize = 512;

    /// True if `host` must not be decrypted (patterns without a port).
    pub fn is_passthrough(&self, host: &str) -> bool {
        self.passthrough_hosts.iter().any(|p| host_matches(p, host))
    }

    /// True if `host:port` must not be decrypted: `example.com` and
    /// `*.example.com` match any port, `example.com:8443` only that port.
    pub fn is_passthrough_at(&self, host: &str, port: u16) -> bool {
        if !self.passthrough_regex.is_empty()
            && rule_regex_matches(&self.passthrough_regex, &format!("{host}:{port}"))
        {
            return true;
        }
        self.passthrough_hosts.iter().any(|p| {
            match p.rsplit_once(':') {
                // One colon and a numeric suffix: a port-specific pattern
                // (IPv6 literals have several colons).
                Some((h, ps)) if !h.contains(':') && ps.parse::<u16>().is_ok() => {
                    ps.parse::<u16>().ok() == Some(port) && host_matches(h, host)
                }
                _ => host_matches(p, host),
            }
        })
    }

    /// True if the upstream certificate of `host` is not verified.
    pub fn skips_verification(&self, host: &str) -> bool {
        self.skip_verification_hosts
            .iter()
            .any(|p| host_matches(p, host))
    }

    fn validate(&self) -> Result<(), String> {
        if !self.passthrough_regex.is_empty() {
            compile_rule_regex(&self.passthrough_regex)
                .map_err(|e| format!("invalid passthrough regular expression: {e}"))?;
        }
        for (name, list) in [
            ("passthrough", &self.passthrough_hosts),
            ("skip-verification", &self.skip_verification_hosts),
        ] {
            if list.len() > Self::MAX_PATTERNS {
                return Err(format!("at most {} {name} hosts", Self::MAX_PATTERNS));
            }
            if let Some(bad) = list.iter().find(|p| !valid_host_pattern(p)) {
                return Err(format!(
                    "invalid {name} host {bad:?}: use example.com or *.example.com"
                ));
            }
        }
        Ok(())
    }
}

/// `example.com` or `*.example.com` (host names, IPs, `host:port`).
pub(crate) fn valid_host_pattern(p: &str) -> bool {
    let host = p.strip_prefix("*.").unwrap_or(p);
    !host.is_empty()
        && host.len() <= 253
        && !host.contains('*')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':'))
}

/// Matches `host` against `example.com` (exact, case-insensitive) or
/// `*.example.com` (strict subdomains only).
pub fn host_matches(pattern: &str, host: &str) -> bool {
    let host = host.trim_end_matches('.');
    match pattern.strip_prefix("*.") {
        Some(suffix) => {
            host.len() > suffix.len() + 1
                && host[host.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
                && host.as_bytes()[host.len() - suffix.len() - 1] == b'.'
        }
        None => pattern.eq_ignore_ascii_case(host),
    }
}

/// One proxy listener.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ListenerConfig {
    /// Stable identifier (`[a-z0-9-]{1,32}`).
    pub id: String,
    /// Interface address to bind.
    #[ts(type = "string")]
    pub bind_address: IpAddr,
    /// TCP port (0 = pick a free port; useful for tests).
    pub port: u16,
    /// Start automatically when a project is opened.
    pub start_on_open: bool,
    /// Accept connections from non-loopback clients. Required for binding a
    /// non-loopback interface; off by default.
    pub allow_remote: bool,
    /// Require clients to authenticate (`Proxy-Authorization: Basic`).
    #[serde(default)]
    pub auth: Option<ProxyAuth>,
    /// Also accept clients that do not know they use a proxy: plain HTTP is
    /// sent where its Host header says, TLS where its server name (SNI)
    /// says, on port 443.
    #[serde(default)]
    pub invisible: bool,
    /// Destination port of TLS from invisible clients.
    #[serde(default = "https_port")]
    pub invisible_tls_port: u16,
    /// Offer HTTP/2 to clients of intercepted HTTPS (ALPN `h2`); off: they
    /// use HTTP/1.1.
    #[serde(default)]
    pub http2: bool,
    /// Invisible clients only: send everything to this `host:port` instead
    /// of where the Host header or server name says (empty = off).
    #[serde(default)]
    pub redirect: String,
    /// The certificate shown to clients of intercepted HTTPS (M31b).
    #[serde(default)]
    pub certificate: ListenerCertificate,
}

/// Which certificate a listener presents to clients of intercepted HTTPS.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum ListenerCertificate {
    /// A certificate per host, signed by ChosProxy's CA (the default).
    #[default]
    Shared,
    /// A certificate per host, signed by a CA made for this listener only
    /// (kept next to ChosProxy's CA; clients of this listener trust that
    /// CA instead).
    OwnCa,
    /// One certificate, chosen by the tester, for every host.
    File {
        /// PEM file with the certificate (and its chain).
        cert: String,
        /// PEM file with its private key.
        key: String,
    },
}

fn https_port() -> u16 {
    443
}

/// `host:port` (IPv6 in brackets) → host and port.
pub fn split_host_port(s: &str) -> Option<(String, u16)> {
    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        let (h, p) = rest.split_once("]:")?;
        (h, p)
    } else {
        let (h, p) = s.rsplit_once(':')?;
        if h.contains(':') {
            return None;
        }
        (h, p)
    };
    let port: u16 = port.parse().ok().filter(|p| *p > 0)?;
    (!host.is_empty() && valid_host_pattern(host) && !host.starts_with("*."))
        .then(|| (host.to_ascii_lowercase(), port))
}

impl ListenerConfig {
    /// Where invisible traffic is redirected, if anywhere.
    pub fn redirect_target(&self) -> Option<(String, u16)> {
        split_host_port(self.redirect.trim())
    }

    /// Socket address this listener binds.
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_address, self.port)
    }
}

impl Default for ListenerConfig {
    fn default() -> Self {
        ListenerConfig {
            id: "default".into(),
            bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8080,
            start_on_open: true,
            allow_remote: false,
            auth: None,
            invisible: false,
            invisible_tls_port: 443,
            http2: false,
            redirect: String::new(),
            certificate: ListenerCertificate::Shared,
        }
    }
}

fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

impl ConfigSection for ProxyConfig {
    const FILE_NAME: &'static str = "proxy.toml";

    fn validate(&self) -> Result<(), String> {
        self.tls.validate()?;
        self.intercept.validate()?;
        if self.match_replace.len() > MatchReplaceRule::MAX_RULES {
            return Err(format!(
                "at most {} Match & Replace rules",
                MatchReplaceRule::MAX_RULES
            ));
        }
        for (i, r) in self.match_replace.iter().enumerate() {
            r.validate(i + 1)?;
        }
        if self.listeners.len() > Self::MAX_LISTENERS {
            return Err(format!(
                "at most {} listeners are allowed",
                Self::MAX_LISTENERS
            ));
        }
        let mut ids = HashSet::new();
        let mut addrs = HashSet::new();
        for l in &self.listeners {
            if !valid_id(&l.id) {
                return Err(format!(
                    "listener id {:?} must be 1-32 chars of a-z, 0-9 or '-'",
                    l.id
                ));
            }
            if !ids.insert(l.id.as_str()) {
                return Err(format!("duplicate listener id {:?}", l.id));
            }
            if !l.redirect.trim().is_empty() && l.redirect_target().is_none() {
                return Err(format!(
                    "listener {:?}: redirect {:?} must be host:port",
                    l.id, l.redirect
                ));
            }
            if l.invisible_tls_port == 0 {
                return Err(format!("listener {:?}: the TLS port cannot be 0", l.id));
            }
            if l.port != 0 && !addrs.insert(l.socket_addr()) {
                return Err(format!("two listeners use {}", l.socket_addr()));
            }
            if let Some(auth) = &l.auth {
                auth.validate()
                    .map_err(|e| format!("listener {:?}: {e}", l.id))?;
                if l.invisible {
                    return Err(format!(
                        "listener {:?}: an invisible listener cannot ask for proxy \
                         credentials (its clients do not know they use a proxy)",
                        l.id
                    ));
                }
            }
            if !l.bind_address.is_loopback() && !l.allow_remote {
                return Err(format!(
                    "listener {:?} binds {} which is reachable from the network; \
                     enable \"allow remote connections\" to confirm this is intended",
                    l.id, l.bind_address
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_loopback_8080() {
        let c = ProxyConfig::default();
        assert!(c.validate().is_ok());
        assert_eq!(c.listeners[0].socket_addr().to_string(), "127.0.0.1:8080");
    }

    #[test]
    fn non_loopback_requires_explicit_opt_in() {
        let mut c = ProxyConfig::default();
        c.listeners[0].bind_address = "0.0.0.0".parse().unwrap();
        assert!(c.validate().is_err());
        c.listeners[0].allow_remote = true;
        assert!(c.validate().is_ok());
    }

    #[test]
    fn duplicates_rejected() {
        let mut c = ProxyConfig::default();
        c.listeners.push(ListenerConfig {
            id: "b".into(),
            ..ListenerConfig::default()
        });
        assert!(c.validate().unwrap_err().contains("two listeners"));
        c.listeners[1].port = 8081;
        assert!(c.validate().is_ok());
        c.listeners[1].id = "default".into();
        assert!(c.validate().unwrap_err().contains("duplicate"));
    }

    #[test]
    fn bad_ids_rejected() {
        let mut c = ProxyConfig::default();
        c.listeners[0].id = "../x".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn host_patterns() {
        assert!(host_matches("example.com", "EXAMPLE.com"));
        assert!(!host_matches("example.com", "a.example.com"));
        assert!(host_matches("*.example.com", "a.b.example.com"));
        assert!(!host_matches("*.example.com", "example.com"));
        assert!(!host_matches("*.example.com", "badexample.com"));
        let mut c = ProxyConfig::default();
        c.tls.passthrough_hosts = vec!["*.bank.test".into()];
        assert!(c.validate().is_ok());
        assert!(c.tls.is_passthrough("www.bank.test"));
        c.tls.passthrough_hosts = vec!["api.test:8443".into(), "::1".into()];
        assert!(c.validate().is_ok());
        assert!(c.tls.is_passthrough_at("api.test", 8443));
        assert!(!c.tls.is_passthrough_at("api.test", 443));
        assert!(c.tls.is_passthrough_at("::1", 443));
        assert_eq!(
            split_host_port("Lab.test:8080"),
            Some(("lab.test".into(), 8080))
        );
        assert_eq!(split_host_port("[::1]:443"), Some(("::1".into(), 443)));
        assert_eq!(split_host_port("lab.test"), None);
        assert_eq!(split_host_port("lab.test:0"), None);
        let mut l = ListenerConfig {
            redirect: "nope".into(),
            ..ListenerConfig::default()
        };
        let mut pc = ProxyConfig {
            listeners: vec![l.clone()],
            ..ProxyConfig::default()
        };
        assert!(pc.validate().is_err());
        l.redirect = "10.0.0.5:443".into();
        pc.listeners = vec![l];
        assert!(pc.validate().is_ok());
        assert!(c.tls.skips_verification("127.0.0.1"));
        for bad in ["", "*", "a*.b", "has space", "*."] {
            c.tls.passthrough_hosts = vec![bad.into()];
            assert!(c.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn intercept_rules() {
        let r = InterceptRules::default();
        let get = RequestFacts {
            host: "lab.test",
            method: "GET",
            extension: None,
            has_query: false,
            has_body: false,
            in_scope: false,
            url: "https://lab.test/a",
        };
        assert!(r.intercepts_request(&get));
        let only_api = InterceptRules {
            url_regex: "/API/".into(),
            ..InterceptRules::default()
        };
        assert!(!only_api.intercepts_request(&get));
        assert!(only_api.intercepts_request(&RequestFacts {
            url: "https://lab.test/api/x",
            ..get
        }));
        let mut c = ProxyConfig::default();
        c.intercept.url_regex = "(".into();
        assert!(c.validate().is_err());
        c.intercept.url_regex = String::new();
        c.tls.passthrough_regex = r"^pay\..*:443$".into();
        assert!(c.validate().is_ok());
        assert!(c.tls.is_passthrough_at("pay.bank.test", 443));
        assert!(!c.tls.is_passthrough_at("pay.bank.test", 8443));
        let ws = InterceptRules {
            websocket_to_server: true,
            exclude_hosts: vec!["cdn.test".into()],
            ..InterceptRules::default()
        };
        assert!(ws.intercepts_websocket("lab.test", false, true));
        assert!(!ws.intercepts_websocket("lab.test", false, false));
        assert!(!ws.intercepts_websocket("cdn.test", false, true));
        assert!(!InterceptRules::default().intercepts_websocket("lab.test", true, true));
        let scoped = InterceptRules {
            only_in_scope: true,
            ..InterceptRules::default()
        };
        assert!(!scoped.intercepts_request(&get));
        assert!(scoped.intercepts_request(&RequestFacts {
            in_scope: true,
            ..get
        }));
        assert!(!r.intercepts_request(&RequestFacts {
            extension: Some("png"),
            ..get
        }));
        assert!(
            !r.intercepts_response(&get, "HTML", false),
            "responses off by default"
        );
        assert!(
            r.intercepts_response(&get, "HTML", true),
            "forced by the user"
        );

        let r = InterceptRules {
            responses: true,
            only_with_parameters: true,
            include_hosts: vec!["*.lab.test".into()],
            exclude_hosts: vec!["cdn.lab.test".into()],
            methods: vec!["post".into()],
            response_mime: vec!["JSON".into()],
            ..InterceptRules::default()
        };
        let post = RequestFacts {
            host: "api.lab.test",
            method: "POST",
            has_body: true,
            ..get
        };
        assert!(r.intercepts_request(&post));
        assert!(!r.intercepts_request(&RequestFacts {
            host: "cdn.lab.test",
            ..post
        }));
        assert!(!r.intercepts_request(&RequestFacts {
            host: "other.test",
            ..post
        }));
        assert!(!r.intercepts_request(&RequestFacts {
            method: "GET",
            ..post
        }));
        assert!(!r.intercepts_request(&RequestFacts {
            has_body: false,
            ..post
        }));
        assert!(r.intercepts_response(&post, "json", false));
        assert!(!r.intercepts_response(&post, "HTML", false));

        let mut c = ProxyConfig::default();
        c.intercept.methods = vec!["G E T".into()];
        assert!(c.validate().is_err());
    }

    #[test]
    fn files_without_tls_section_get_defaults() {
        let c: ProxyConfig = toml::from_str("version = 1\n").unwrap();
        assert!(c.tls.intercept);
    }

    #[test]
    fn toml_round_trip() {
        let c = ProxyConfig::default();
        let s = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<ProxyConfig>(&s).unwrap(), c);
    }
}
