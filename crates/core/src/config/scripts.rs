//! `scripts.toml`: the tester's ChosScripts library (ADR-0032). Scripts are
//! kept with the application settings so they work in every project.

use super::ConfigSection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

/// What a script is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ScriptKind {
    /// Keeps or hides history rows: `fn filter(msg)`.
    Filter,
    /// A value for a history column: `fn column(msg)`.
    Column,
    /// Changes proxied messages through a Match & Replace rule:
    /// `fn request(text)`, `fn response(text)` and/or, for WebSocket text
    /// messages, `fn websocket(text, to_server)`.
    Rewrite,
    /// Runs on a message on demand and shows its output: `fn action(msg)`.
    Action,
    /// Reports findings during passive or active analysis:
    /// `fn check(msg)` and/or `fn active_probes(value)` + `fn active_check(baseline, probe, payload)`.
    Check,
    /// Processes each Fuzzer payload: `fn payload(p)`.
    Payload,
}

impl ScriptKind {
    /// The functions a script of this kind may define (at least one).
    pub fn entry_points(self) -> &'static [&'static str] {
        match self {
            ScriptKind::Filter => &["filter"],
            ScriptKind::Column => &["column"],
            ScriptKind::Rewrite => &["request", "response", "websocket"],
            ScriptKind::Action => &["action"],
            ScriptKind::Check => &["check", "active_check"],
            ScriptKind::Payload => &["payload"],
        }
    }
}

/// One script of the library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct ScriptEntry {
    /// Stable identifier (`[a-z0-9-]{1,48}`), used by rules that refer to it.
    pub id: String,
    /// Name shown in menus.
    pub name: String,
    /// What it is for.
    pub kind: ScriptKind,
    /// Checks run during passive analysis and filters, columns and actions
    /// appear in menus only when enabled; rewrite rules skip disabled ones.
    pub enabled: bool,
    /// What it does.
    #[serde(default)]
    pub description: String,
    /// Rhai source.
    pub source: String,
}

/// The script library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct ScriptsConfig {
    /// Schema version of this file.
    pub version: u32,
    /// Scripts, in the order shown.
    pub scripts: Vec<ScriptEntry>,
}

impl Default for ScriptsConfig {
    fn default() -> Self {
        ScriptsConfig {
            version: 1,
            scripts: Vec::new(),
        }
    }
}

impl ScriptsConfig {
    /// Most scripts in the library.
    pub const MAX_SCRIPTS: usize = 200;
    /// Longest source.
    pub const MAX_SOURCE: usize = 64 * 1024;

    /// A script by id.
    pub fn get(&self, id: &str) -> Option<&ScriptEntry> {
        self.scripts.iter().find(|s| s.id == id)
    }
}

/// True for a valid script id.
pub fn valid_script_id(id: &str) -> bool {
    (1..=48).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl ConfigSection for ScriptsConfig {
    const FILE_NAME: &'static str = "scripts.toml";

    fn validate(&self) -> Result<(), String> {
        if self.scripts.len() > Self::MAX_SCRIPTS {
            return Err(format!("at most {} scripts", Self::MAX_SCRIPTS));
        }
        let mut ids = HashSet::new();
        for s in &self.scripts {
            if !valid_script_id(&s.id) {
                return Err(format!(
                    "script id {:?} must be 1-48 characters of a-z, 0-9 or '-'",
                    s.id
                ));
            }
            if !ids.insert(s.id.as_str()) {
                return Err(format!("two scripts have the id {:?}", s.id));
            }
            let name_len = s.name.trim().chars().count();
            if !(1..=80).contains(&name_len) {
                return Err(format!(
                    "script {:?}: the name must be 1-80 characters",
                    s.id
                ));
            }
            if s.source.len() > Self::MAX_SOURCE {
                return Err(format!(
                    "script {:?}: the source is limited to {} KB",
                    s.id,
                    Self::MAX_SOURCE / 1024
                ));
            }
            if s.description.chars().count() > 2000 {
                return Err(format!("script {:?}: the description is too long", s.id));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> ScriptEntry {
        ScriptEntry {
            id: id.into(),
            name: "A script".into(),
            kind: ScriptKind::Filter,
            enabled: true,
            description: String::new(),
            source: "fn filter(msg) { true }".into(),
        }
    }

    #[test]
    fn validation_and_round_trip() {
        let mut c = ScriptsConfig {
            scripts: vec![entry("only-json"), entry("other")],
            ..ScriptsConfig::default()
        };
        assert!(c.validate().is_ok());
        assert!(c.get("other").is_some());
        let s = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<ScriptsConfig>(&s).unwrap(), c);
        c.scripts[1].id = "only-json".into();
        assert!(c.validate().unwrap_err().contains("two scripts"));
        c.scripts[1].id = "Bad Id".into();
        assert!(c.validate().is_err());
        c.scripts[1].id = "ok".into();
        c.scripts[1].name = " ".into();
        assert!(c.validate().is_err());
        assert_eq!(
            ScriptKind::Rewrite.entry_points(),
            &["request", "response", "websocket"]
        );
    }
}
