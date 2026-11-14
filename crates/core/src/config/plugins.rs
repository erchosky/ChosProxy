//! `plugins.toml`: which plugins are enabled, what each was granted and the
//! fingerprint of the files that were approved (ADR-0025).

use super::ConfigSection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The tester's decisions about installed plugins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct PluginsConfig {
    /// One entry per plugin the tester has decided on.
    pub plugin: Vec<PluginGrant>,
    /// Publishers whose signatures the tester trusts (ADR-0033).
    pub trusted: Vec<TrustedPublisher>,
    /// Only run plugins signed by a trusted publisher.
    pub require_signed: bool,
}

/// A publisher key the tester trusts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct TrustedPublisher {
    /// Name, as the tester calls it.
    pub name: String,
    /// Ed25519 public key, Base64.
    pub key: String,
}

/// What one plugin may do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct PluginGrant {
    /// Plugin id.
    pub id: String,
    /// Run it while a project is open.
    pub enabled: bool,
    /// Permissions granted (a subset of what it asks for).
    pub granted: Vec<String>,
    /// SHA-256 of the plugin files when it was enabled; any change needs a
    /// new approval.
    pub approved_hash: String,
}

impl PluginsConfig {
    /// The entry of a plugin.
    pub fn get(&self, id: &str) -> Option<&PluginGrant> {
        self.plugin.iter().find(|p| p.id == id)
    }
}

impl ConfigSection for PluginsConfig {
    const FILE_NAME: &'static str = "plugins.toml";

    fn validate(&self) -> Result<(), String> {
        if self.trusted.len() > 100
            || self
                .trusted
                .iter()
                .any(|t| t.key.len() != 44 || t.name.chars().count() > 100)
        {
            return Err("invalid trusted publisher list".into());
        }
        if self.plugin.len() > 500 {
            return Err("too many plugin entries".into());
        }
        let mut seen = std::collections::HashSet::new();
        for p in &self.plugin {
            if p.id.is_empty() || p.id.len() > 64 || !seen.insert(p.id.as_str()) {
                return Err(format!("invalid or repeated plugin id {:?}", p.id));
            }
            if p.granted.len() > 20 || p.approved_hash.len() > 128 {
                return Err(format!("invalid entry for plugin {}", p.id));
            }
        }
        Ok(())
    }
}
