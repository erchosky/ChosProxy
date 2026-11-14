//! Outgoing connections (`network.toml`): upstream proxies, host name
//! overrides and timeouts. Every sender uses them: the proxy's upstream side,
//! Repeater, the Fuzzer, the crawler and the other tools (ADR-0029).

use super::{ConfigSection, host_matches, valid_host_pattern};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use ts_rs::TS;

/// Outgoing connection settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct NetworkConfig {
    /// Schema version of this file.
    pub version: u32,
    /// Upstream proxy rules; the first enabled rule whose host matches is
    /// used, no match means a direct connection.
    pub upstream: Vec<UpstreamRule>,
    /// Host names resolved to fixed addresses instead of using DNS.
    pub hosts: Vec<HostOverride>,
    /// Timeouts.
    pub timeouts: Timeouts,
    /// Platform authentication: credentials (from the keyring) used to
    /// answer 401 challenges of matching hosts.
    pub auth: Vec<PlatformAuthRule>,
}

/// A platform authentication scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AuthScheme {
    /// HTTP Basic (RFC 7617).
    Basic,
    /// HTTP Digest (RFC 7616: MD5 or SHA-256, qop=auth).
    Digest,
    /// NTLMv2 over HTTP (M31b; the user name may be `DOMAIN\user`).
    Ntlm,
}

/// Answer 401 challenges of `host` with a keyring credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct PlatformAuthRule {
    /// Whether the rule is used.
    pub enabled: bool,
    /// `example.com` or `*.example.com`.
    pub host: String,
    /// Scheme to answer.
    pub scheme: AuthScheme,
    /// Keyring credential id.
    pub credential: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            version: 1,
            upstream: Vec::new(),
            hosts: Vec::new(),
            timeouts: Timeouts::default(),
            auth: Vec::new(),
        }
    }
}

/// How a connection is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum UpstreamKind {
    /// Straight to the destination (useful to exempt hosts from a later rule).
    Direct,
    /// Through an HTTP proxy (CONNECT for HTTPS, absolute-form for HTTP).
    Http,
    /// Through a SOCKS5 proxy.
    Socks5,
}

/// One upstream proxy rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct UpstreamRule {
    /// Whether the rule is used.
    pub enabled: bool,
    /// Destination hosts: `*` (all), `example.com` or `*.example.com`.
    pub host: String,
    /// Direct, HTTP proxy or SOCKS5 proxy.
    pub kind: UpstreamKind,
    /// Proxy host (empty for `Direct`).
    #[serde(default)]
    pub proxy_host: String,
    /// Proxy port.
    #[serde(default)]
    pub proxy_port: u16,
    /// User name for the proxy (empty = none).
    #[serde(default)]
    pub username: String,
    /// Password for the proxy. Stored in this file, which only your user
    /// can read; never logged.
    #[serde(default)]
    pub password: String,
    /// SOCKS5 only: let the proxy resolve host names (otherwise they are
    /// resolved here and the proxy receives an address).
    #[serde(default = "yes")]
    pub remote_dns: bool,
}

fn yes() -> bool {
    true
}

impl UpstreamRule {
    /// True if the rule applies to connections to `host`.
    pub fn matches(&self, host: &str) -> bool {
        self.enabled && (self.host == "*" || host_matches(&self.host, host))
    }
}

/// A host name resolved to a fixed address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct HostOverride {
    /// Whether the override is used.
    pub enabled: bool,
    /// `example.com` or `*.example.com`.
    pub host: String,
    /// Address used instead of a DNS lookup.
    #[ts(type = "string")]
    pub address: IpAddr,
}

/// Timeouts of outgoing connections, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct Timeouts {
    /// DNS lookup plus TCP connect (and the proxy handshake, if any).
    pub connect_secs: u32,
    /// From the request being sent to the response head.
    pub response_secs: u32,
    /// Longest silence while a body is being transferred.
    pub idle_secs: u32,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            connect_secs: 10,
            response_secs: 120,
            idle_secs: 120,
        }
    }
}

impl NetworkConfig {
    /// Maximum rules per list.
    pub const MAX_RULES: usize = 256;
    /// Longest accepted timeout (one hour).
    pub const MAX_TIMEOUT_SECS: u32 = 3600;

    /// The rule used for connections to `host`, if any.
    pub fn route(&self, host: &str) -> Option<&UpstreamRule> {
        self.upstream.iter().find(|r| r.matches(host))
    }

    /// The fixed address of `host`, if it has one.
    pub fn resolve(&self, host: &str) -> Option<IpAddr> {
        self.hosts
            .iter()
            .find(|h| h.enabled && host_matches(&h.host, host))
            .map(|h| h.address)
    }
}

fn valid_proxy_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':'))
}

impl ConfigSection for NetworkConfig {
    const FILE_NAME: &'static str = "network.toml";

    fn validate(&self) -> Result<(), String> {
        if self.upstream.len() > Self::MAX_RULES || self.hosts.len() > Self::MAX_RULES {
            return Err(format!("at most {} rules per list", Self::MAX_RULES));
        }
        for (i, r) in self.upstream.iter().enumerate() {
            let n = i + 1;
            if r.host != "*" && !valid_host_pattern(&r.host) {
                return Err(format!(
                    "upstream rule {n}: invalid host {:?}; use *, example.com or *.example.com",
                    r.host
                ));
            }
            if r.kind != UpstreamKind::Direct {
                if !valid_proxy_host(&r.proxy_host) {
                    return Err(format!(
                        "upstream rule {n}: invalid proxy host {:?}",
                        r.proxy_host
                    ));
                }
                if r.proxy_port == 0 {
                    return Err(format!("upstream rule {n}: the proxy port is missing"));
                }
            }
            if r.username.len() > 255 || r.password.len() > 255 {
                return Err(format!(
                    "upstream rule {n}: user name and password are limited to 255 bytes"
                ));
            }
            if r.username.contains(':') && r.kind == UpstreamKind::Http {
                return Err(format!(
                    "upstream rule {n}: the user name cannot contain ':'"
                ));
            }
            if r.username.contains(['\r', '\n']) || r.password.contains(['\r', '\n']) {
                return Err(format!("upstream rule {n}: line breaks are not allowed"));
            }
        }
        if self.auth.len() > Self::MAX_RULES {
            return Err(format!("at most {} authentication rules", Self::MAX_RULES));
        }
        for (i, a) in self.auth.iter().enumerate() {
            if !valid_host_pattern(&a.host) {
                return Err(format!(
                    "authentication rule {}: invalid host {:?}",
                    i + 1,
                    a.host
                ));
            }
            if !super::valid_script_id(&a.credential) {
                return Err(format!(
                    "authentication rule {}: choose a credential",
                    i + 1
                ));
            }
        }
        for (i, h) in self.hosts.iter().enumerate() {
            if !valid_host_pattern(&h.host) {
                return Err(format!(
                    "host override {}: invalid host {:?}",
                    i + 1,
                    h.host
                ));
            }
        }
        let t = &self.timeouts;
        for (name, v) in [
            ("connect", t.connect_secs),
            ("response", t.response_secs),
            ("idle", t.idle_secs),
        ] {
            if v == 0 || v > Self::MAX_TIMEOUT_SECS {
                return Err(format!(
                    "the {name} timeout must be 1–{} seconds",
                    Self::MAX_TIMEOUT_SECS
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(host: &str, kind: UpstreamKind) -> UpstreamRule {
        UpstreamRule {
            enabled: true,
            host: host.into(),
            kind,
            proxy_host: "127.0.0.1".into(),
            proxy_port: 3128,
            username: String::new(),
            password: String::new(),
            remote_dns: true,
        }
    }

    #[test]
    fn first_matching_rule_wins() {
        let c = NetworkConfig {
            upstream: vec![
                rule("intranet.test", UpstreamKind::Direct),
                rule("*.corp.test", UpstreamKind::Socks5),
                rule("*", UpstreamKind::Http),
            ],
            ..NetworkConfig::default()
        };
        assert!(c.validate().is_ok());
        assert_eq!(
            c.route("intranet.test").map(|r| r.kind),
            Some(UpstreamKind::Direct)
        );
        assert_eq!(
            c.route("a.corp.test").map(|r| r.kind),
            Some(UpstreamKind::Socks5)
        );
        assert_eq!(c.route("x.test").map(|r| r.kind), Some(UpstreamKind::Http));
        let mut off = c.clone();
        off.upstream[2].enabled = false;
        assert!(off.route("x.test").is_none());
    }

    #[test]
    fn host_overrides() {
        let c = NetworkConfig {
            hosts: vec![HostOverride {
                enabled: true,
                host: "*.lab.test".into(),
                address: "10.0.0.5".parse().unwrap(),
            }],
            ..NetworkConfig::default()
        };
        assert_eq!(c.resolve("a.lab.test"), Some("10.0.0.5".parse().unwrap()));
        assert_eq!(c.resolve("lab.test"), None);
    }

    #[test]
    fn validation() {
        assert!(NetworkConfig::default().validate().is_ok());
        let mut c = NetworkConfig {
            upstream: vec![rule("*", UpstreamKind::Http)],
            ..NetworkConfig::default()
        };
        c.upstream[0].proxy_port = 0;
        assert!(c.validate().unwrap_err().contains("port"));
        c.upstream[0].proxy_port = 8080;
        c.upstream[0].proxy_host = "bad host".into();
        assert!(c.validate().is_err());
        c.upstream[0].proxy_host = "proxy.corp".into();
        c.upstream[0].password = "a\r\nb".into();
        assert!(c.validate().is_err());
        c.upstream[0].password = "pw".into();
        c.timeouts.connect_secs = 0;
        assert!(c.validate().is_err());
        c.timeouts.connect_secs = 5;
        assert!(c.validate().is_ok());
        let direct = rule("x.test", UpstreamKind::Direct);
        let c = NetworkConfig {
            upstream: vec![UpstreamRule {
                proxy_host: String::new(),
                proxy_port: 0,
                ..direct
            }],
            ..NetworkConfig::default()
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn toml_round_trip() {
        let c = NetworkConfig {
            upstream: vec![rule("*", UpstreamKind::Socks5)],
            ..NetworkConfig::default()
        };
        let s = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<NetworkConfig>(&s).unwrap(), c);
        let empty: NetworkConfig = toml::from_str("version = 1\n").unwrap();
        assert_eq!(empty.timeouts.response_secs, 120);
    }
}
