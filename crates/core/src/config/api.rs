//! `api.toml`: the automation API (M28, ADR-0038). Off by default: nothing
//! listens until the tester turns it on. It only ever binds to loopback.

use super::ConfigSection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The automation API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct ApiConfig {
    /// Listen on `127.0.0.1:port` while ChosProxy runs.
    pub enabled: bool,
    /// TCP port on 127.0.0.1.
    pub port: u16,
}

impl ApiConfig {
    /// The default port.
    pub const DEFAULT_PORT: u16 = 8790;
}

impl Default for ApiConfig {
    fn default() -> ApiConfig {
        ApiConfig {
            enabled: false,
            port: ApiConfig::DEFAULT_PORT,
        }
    }
}

impl ConfigSection for ApiConfig {
    const FILE_NAME: &'static str = "api.toml";

    fn validate(&self) -> Result<(), String> {
        if self.port < 1024 {
            return Err("the API port must be 1024 or higher".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_by_default_and_ports_checked() {
        let c = ApiConfig::default();
        assert!(!c.enabled);
        assert!(c.validate().is_ok());
        assert!(ApiConfig { port: 80, ..c }.validate().is_err());
    }
}
