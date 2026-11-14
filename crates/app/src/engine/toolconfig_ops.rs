//! The configuration library for tool settings (M31b, matrix
//! SETTINGS-003): named settings of the crawler, content discovery and the
//! Fuzzer, saved once and loaded into the tool's form later, with JSON
//! import and export — like the scan configurations of M25.
//!
//! A saved configuration is settings only (limits, rate, options, payload
//! sets): no target, no start URL, no request. Loading one changes a form;
//! nothing starts by itself.

use super::Engine;
use crate::AppError;
use chosproxy_core::config::ConfigSection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Tools with a settings library.
pub const CONFIG_TOOLS: [&str; 3] = ["crawler", "discovery", "fuzzer"];
/// Most saved configurations per tool.
const MAX_PER_TOOL: usize = 50;
/// Largest configuration (JSON text).
const MAX_JSON: usize = 256 * 1024;
const FILE_FORMAT: &str = "chosproxy-tool-configurations";

/// A named configuration of a tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ToolConfig {
    /// `crawler`, `discovery` or `fuzzer`.
    pub tool: String,
    /// Name (1-80 characters), unique per tool.
    pub name: String,
    /// The settings: a JSON object, as the tool's form reads it.
    pub json: String,
}

impl ToolConfig {
    fn validate(&self) -> Result<(), String> {
        if !CONFIG_TOOLS.contains(&self.tool.as_str()) {
            return Err(format!("no configuration library for \"{}\"", self.tool));
        }
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err("the name must be 1-80 characters".into());
        }
        if self.json.len() > MAX_JSON {
            return Err("the configuration is too large".into());
        }
        match serde_json::from_str::<serde_json::Value>(&self.json) {
            Ok(serde_json::Value::Object(_)) => Ok(()),
            _ => Err("a configuration is a JSON object".into()),
        }
    }
}

/// The library file (`toolconfigs.toml`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolConfigs {
    /// Saved configurations.
    pub configs: Vec<ToolConfig>,
}

impl ConfigSection for ToolConfigs {
    const FILE_NAME: &'static str = "toolconfigs.toml";

    fn validate(&self) -> Result<(), String> {
        for tool in CONFIG_TOOLS {
            if self.configs.iter().filter(|c| c.tool == tool).count() > MAX_PER_TOOL {
                return Err(format!("at most {MAX_PER_TOOL} configurations for {tool}"));
            }
        }
        for c in &self.configs {
            c.validate().map_err(|e| format!("{}: {e}", c.name))?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct ToolConfigFile {
    format: String,
    version: u32,
    configs: Vec<ToolConfig>,
}

fn same(a: &ToolConfig, tool: &str, name: &str) -> bool {
    a.tool == tool && a.name.eq_ignore_ascii_case(name)
}

impl Engine {
    /// Saved configurations of a tool, by name.
    pub fn tool_configs(&self, tool: &str) -> Vec<ToolConfig> {
        let mut list: Vec<ToolConfig> = self
            .inner
            .store
            .load::<ToolConfigs>()
            .value
            .configs
            .into_iter()
            .filter(|c| c.tool == tool)
            .collect();
        list.sort_by_key(|c| c.name.to_lowercase());
        list
    }

    /// Saves a configuration (replacing the one with the same name).
    pub fn save_tool_config(&self, mut config: ToolConfig) -> Result<Vec<ToolConfig>, AppError> {
        config.name = config.name.trim().to_string();
        config.validate().map_err(AppError::Invalid)?;
        let mut stored = self.inner.store.load::<ToolConfigs>().value;
        match stored
            .configs
            .iter_mut()
            .find(|c| same(c, &config.tool, &config.name))
        {
            Some(existing) => *existing = config.clone(),
            None => stored.configs.push(config.clone()),
        }
        stored.validate().map_err(AppError::Invalid)?;
        self.inner.store.save(&stored)?;
        Ok(self.tool_configs(&config.tool))
    }

    /// Deletes a saved configuration.
    pub fn delete_tool_config(&self, tool: &str, name: &str) -> Result<Vec<ToolConfig>, AppError> {
        let mut stored = self.inner.store.load::<ToolConfigs>().value;
        let before = stored.configs.len();
        stored.configs.retain(|c| !same(c, tool, name));
        if stored.configs.len() == before {
            return Err(AppError::Invalid(format!(
                "no saved configuration named \"{name}\""
            )));
        }
        self.inner.store.save(&stored)?;
        Ok(self.tool_configs(tool))
    }

    /// Writes a tool's saved configurations to a JSON file to share.
    /// Returns how many were written.
    pub fn export_tool_configs(
        &self,
        path: &std::path::Path,
        tool: &str,
    ) -> Result<usize, AppError> {
        let file = ToolConfigFile {
            format: FILE_FORMAT.into(),
            version: 1,
            configs: self.tool_configs(tool),
        };
        super::write_shared_file(path, &file, "configurations")?;
        Ok(file.configs.len())
    }

    /// Adds the configurations of a shared file for `tool` to the library
    /// (names in use get a suffix; configurations of other tools in the
    /// file are left out). Nothing starts on import.
    pub fn import_tool_configs(
        &self,
        path: &std::path::Path,
        tool: &str,
    ) -> Result<Vec<ToolConfig>, AppError> {
        let file: ToolConfigFile = super::read_shared_file(path, 4, "tool configurations")?;
        if file.format != FILE_FORMAT {
            return Err(AppError::Invalid(
                "not a ChosProxy tool configurations file".into(),
            ));
        }
        let mut stored = self.inner.store.load::<ToolConfigs>().value;
        let mut added = 0;
        for mut config in file.configs.into_iter().filter(|c| c.tool == tool) {
            config.name = config.name.trim().to_string();
            config.validate().map_err(AppError::Invalid)?;
            let base = config.name.clone();
            let mut n = 2;
            while stored.configs.iter().any(|c| same(c, tool, &config.name)) {
                config.name = format!("{base} ({n})");
                n += 1;
            }
            stored.configs.push(config);
            added += 1;
        }
        if added == 0 {
            return Err(AppError::Invalid(format!(
                "the file has no {tool} configurations"
            )));
        }
        stored.validate().map_err(AppError::Invalid)?;
        self.inner.store.save(&stored)?;
        Ok(self.tool_configs(tool))
    }
}
