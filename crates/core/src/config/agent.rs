//! `agent.toml`: ChosAgent (M30, ADR-0040). The model it uses is the one
//! of ChosAI (`ai.toml`); this file holds what the agent may do.
//!
//! Permissions are only a wish list here: the engine caps each tool at its
//! highest permission (tools that send traffic never go above ASK), and a
//! tool not named gets its default.

use super::ConfigSection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

/// What the agent may do with a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentPermission {
    /// Never: the call is refused and the model is told so.
    Deny,
    /// Only after the tester approves this exact call.
    Ask,
    /// Without asking (still recorded in the activity log).
    Allow,
}

/// ChosAgent settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct AgentConfig {
    /// Permission per tool name; a tool not listed has its default.
    pub permissions: BTreeMap<String, AgentPermission>,
    /// Most model calls in one agent task.
    pub max_steps: u32,
    /// Most requests one agent task may send (each one approved).
    pub max_sends: u32,
}

impl Default for AgentConfig {
    fn default() -> AgentConfig {
        AgentConfig {
            permissions: BTreeMap::new(),
            max_steps: 25,
            max_sends: 20,
        }
    }
}

impl ConfigSection for AgentConfig {
    const FILE_NAME: &'static str = "agent.toml";

    fn validate(&self) -> Result<(), String> {
        if !(1..=200).contains(&self.max_steps) {
            return Err("the step limit must be between 1 and 200".into());
        }
        if self.max_sends > 500 {
            return Err("the send limit must be at most 500".into());
        }
        if self.permissions.len() > 100
            || self
                .permissions
                .keys()
                .any(|k| k.is_empty() || k.len() > 64)
        {
            return Err("invalid tool names in the permissions".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_permissions_are_ordered() {
        assert!(AgentConfig::default().validate().is_ok());
        assert!(AgentPermission::Deny < AgentPermission::Ask);
        assert!(AgentPermission::Ask < AgentPermission::Allow);
        let bad = AgentConfig {
            max_steps: 0,
            ..AgentConfig::default()
        };
        assert!(bad.validate().is_err());
        let toml = "maxSteps = 5\n[permissions]\nsend_request = \"allow\"\n";
        let parsed: AgentConfig = toml::from_str(toml).unwrap_or_default();
        assert_eq!(parsed.max_steps, 5);
        assert_eq!(
            parsed.permissions.get("send_request"),
            Some(&AgentPermission::Allow)
        );
    }
}
