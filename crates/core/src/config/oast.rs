//! `oast.toml`: how ChosOAST runs (ADR-0026). Off by default: nothing
//! listens and nothing is polled until the tester chooses a mode.

use super::ConfigSection;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use ts_rs::TS;

/// Where interactions are received.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "OastMode")]
pub enum OastMode {
    /// No OAST server.
    #[default]
    Off,
    /// A server inside ChosProxy (labs reachable from this machine).
    Local,
    /// A `chosoast` server the tester runs on a machine they control.
    SelfHosted,
}

/// The server inside ChosProxy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export, rename = "OastLocalConfig")]
pub struct LocalOast {
    /// Address the listeners bind to (`127.0.0.1` keeps them on this
    /// machine; a LAN address lets lab VMs reach them).
    pub bind: String,
    /// HTTP port.
    pub http_port: u16,
    /// Also answer DNS for `zone`.
    pub dns_enabled: bool,
    /// DNS (UDP) port.
    pub dns_port: u16,
    /// Also accept mail (SMTP) and record it (M31b); off by default.
    #[serde(default)]
    pub smtp_enabled: bool,
    /// SMTP port.
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    /// Zone the DNS listener answers for.
    pub zone: String,
    /// Host or IP put in payload URLs (how targets reach this machine).
    pub advertised_host: String,
}

impl Default for LocalOast {
    fn default() -> Self {
        LocalOast {
            bind: "127.0.0.1".into(),
            http_port: 8899,
            dns_enabled: false,
            dns_port: 5353,
            smtp_enabled: false,
            smtp_port: default_smtp_port(),
            zone: "oast.chosproxy.test".into(),
            advertised_host: "127.0.0.1".into(),
        }
    }
}

/// A self-hosted `chosoast` server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export, rename = "OastSelfHostedConfig")]
pub struct SelfHostedOast {
    /// Zone delegated to the server (payloads are `<id>.<zone>`).
    pub zone: String,
    /// API host.
    pub api_host: String,
    /// API port.
    pub api_port: u16,
    /// Access token printed by `chosoast` (kept in this file, mode 0600,
    /// never exported with the settings).
    pub token: String,
    /// SHA-256 fingerprint of the server certificate.
    pub fingerprint: String,
}

impl Default for SelfHostedOast {
    fn default() -> Self {
        SelfHostedOast {
            zone: String::new(),
            api_host: String::new(),
            api_port: 8443,
            token: String::new(),
            fingerprint: String::new(),
        }
    }
}

/// The OAST configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct OastConfig {
    /// Mode.
    pub mode: OastMode,
    /// Local server settings.
    pub local: LocalOast,
    /// Self-hosted server settings.
    pub self_hosted: SelfHostedOast,
    /// Seconds between polls of a self-hosted server.
    pub poll_seconds: u32,
}

impl Default for OastConfig {
    fn default() -> Self {
        OastConfig {
            mode: OastMode::Off,
            local: LocalOast::default(),
            self_hosted: SelfHostedOast::default(),
            poll_seconds: 5,
        }
    }
}

fn valid_zone(zone: &str) -> bool {
    let zone = zone.trim_end_matches('.');
    zone.len() <= 200
        && zone.contains('.')
        && zone.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        })
}

fn valid_host(host: &str) -> bool {
    host.parse::<IpAddr>().is_ok() || valid_zone(host) || host == "localhost"
}

impl ConfigSection for OastConfig {
    const FILE_NAME: &'static str = "oast.toml";

    fn validate(&self) -> Result<(), String> {
        if !(1..=3600).contains(&self.poll_seconds) {
            return Err("poll interval must be 1–3600 seconds".into());
        }
        let l = &self.local;
        if l.bind.parse::<IpAddr>().is_err() {
            return Err(format!("invalid bind address {:?}", l.bind));
        }
        if l.smtp_enabled && l.smtp_port == 0 {
            return Err("the SMTP port must not be 0".into());
        }
        if l.http_port == 0 || (l.dns_enabled && l.dns_port == 0) {
            return Err("ports must be 1–65535".into());
        }
        if !valid_zone(&l.zone) || !valid_host(&l.advertised_host) {
            return Err("invalid local zone or advertised host".into());
        }
        let s = &self.self_hosted;
        if self.mode == OastMode::SelfHosted {
            if !valid_zone(&s.zone) || !valid_host(&s.api_host) || s.api_port == 0 {
                return Err("enter the server's zone, API host and port".into());
            }
            let hex = s
                .fingerprint
                .chars()
                .filter(char::is_ascii_hexdigit)
                .count();
            if hex != 64 {
                return Err("the certificate fingerprint must be a SHA-256 (64 hex digits)".into());
            }
            if s.token.len() < 32
                || s.token.len() > 256
                || !s.token.chars().all(|c| c.is_ascii_graphic())
            {
                return Err("enter the access token printed by chosoast".into());
            }
        }
        Ok(())
    }
}

fn default_smtp_port() -> u16 {
    2525
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_and_valid() {
        let c = OastConfig::default();
        assert_eq!(c.mode, OastMode::Off);
        assert!(c.validate().is_ok());
        let mut s = c.clone();
        s.mode = OastMode::SelfHosted;
        assert!(s.validate().is_err(), "self-hosted needs its server");
        s.self_hosted = SelfHostedOast {
            zone: "oast.example.com".into(),
            api_host: "203.0.113.5".into(),
            api_port: 8443,
            token: "a".repeat(64),
            fingerprint: "ab:".repeat(31) + "ab",
        };
        assert!(s.validate().is_ok());
        let mut bad = c;
        bad.local.zone = "bad zone".into();
        assert!(bad.validate().is_err());
    }
}
