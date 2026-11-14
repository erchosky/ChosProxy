//! Outgoing connection settings (matrix PROXY rows of M21, ADR-0029):
//! upstream proxies, host name overrides and timeouts, used by every sender.

use super::Engine;
use crate::AppError;
use chosproxy_core::config::{ConfigSection, NetworkConfig};

impl Engine {
    /// Current outgoing connection settings.
    pub fn network_config(&self) -> NetworkConfig {
        self.inner.store.load::<NetworkConfig>().value
    }

    /// Validates, saves and applies the settings; new connections use them.
    pub fn save_network_config(&self, config: NetworkConfig) -> Result<NetworkConfig, AppError> {
        config.validate().map_err(AppError::Invalid)?;
        self.inner.store.save(&config)?;
        let proxies = config.upstream.iter().filter(|r| r.enabled).count();
        let hosts = config.hosts.iter().filter(|h| h.enabled).count();
        chosproxy_request::set_network(config.clone());
        self.reload_platform_auth();
        // Credentials are never part of the audit message.
        self.audit(
            "network.config.changed",
            &format!(
                "{proxies} upstream rule(s), {hosts} host override(s), timeouts {}s/{}s/{}s",
                config.timeouts.connect_secs,
                config.timeouts.response_secs,
                config.timeouts.idle_secs
            ),
        );
        Ok(config)
    }
}
