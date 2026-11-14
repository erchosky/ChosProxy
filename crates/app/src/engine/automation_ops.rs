//! What the automation API (M28, `chosproxy-api`) needs from the engine
//! besides the operations the UI already uses: its configuration, the
//! application paths (for its token file), and a Repeater-like send of one
//! request (scope-checked, stored in the history with a note).

use super::Engine;
use crate::AppError;
use chosproxy_core::Source;
use chosproxy_core::config::{ApiConfig, ConfigSection};
use chosproxy_core::paths::AppPaths;
use chosproxy_core::time::now_ms;
use chosproxy_request::{Destination, SendLimits};
use chosproxy_storage::RepeaterTarget;
use serde::Serialize;
use std::time::Duration;
use ts_rs::TS;

/// Largest response kept for an automation send.
const MAX_RESPONSE: usize = 8 * 1024 * 1024;

/// The outcome of an automation send.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AutomationSent {
    /// History id of the stored exchange.
    #[ts(type = "number")]
    pub exchange_id: i64,
    /// Response status (none when the send failed).
    pub status: Option<u16>,
    /// Why the send failed.
    pub error: Option<String>,
}

/// The automation API as the desktop shows it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiStatus {
    /// Saved settings.
    pub config: ApiConfig,
    /// Listening now.
    pub running: bool,
    /// `127.0.0.1:port` while running.
    pub address: Option<String>,
    /// The token clients must send (empty until the API has run once).
    pub token: String,
    /// Where the token is kept.
    pub token_file: String,
    /// Why it is not running although it is enabled.
    pub error: Option<String>,
}

impl Engine {
    /// The automation API settings.
    pub fn api_config(&self) -> ApiConfig {
        self.inner.store.load::<ApiConfig>().value
    }

    /// Saves the automation API settings (the caller restarts the server).
    pub fn save_api_config(&self, config: ApiConfig) -> Result<ApiConfig, AppError> {
        config.validate().map_err(AppError::Invalid)?;
        self.inner.store.save(&config)?;
        self.audit(
            "api.settings",
            &format!(
                "automation API {} on 127.0.0.1:{}",
                if config.enabled { "on" } else { "off" },
                config.port
            ),
        );
        Ok(config)
    }

    /// Records an automation API call that changed something in the open
    /// project's audit log (no project: not recorded).
    pub fn audit_automation(&self, action: &str, detail: &str) {
        self.audit(&format!("api.{action}"), detail);
    }

    /// Where configuration and data live.
    pub fn paths(&self) -> &AppPaths {
        &self.inner.paths
    }

    /// Sends one request (editor text form) to `target` like Repeater:
    /// refused out of scope when a scope exists, through the Repeater's
    /// resource pool and session rules, stored in the history.
    pub async fn automation_send(
        &self,
        target: RepeaterTarget,
        text: &str,
    ) -> Result<AutomationSent, AppError> {
        let target = chosproxy_storage::validate_target(&target)?;
        let path = chosproxy_storage::request_path(text);
        self.ensure_in_scope(&target, &path, "automation API")?;
        let request = crate::repeater::request_bytes(text, true, true, true)
            .map_err(|e| AppError::Invalid(e.to_string()))?;
        let security = self
            .security_for_host(&target.host, target.scheme == "https")
            .await
            .map_err(AppError::Invalid)?;
        let destination = Destination {
            host: target.host.clone(),
            port: target.port,
            tls_name: target.host.clone(),
        };
        let limits = SendLimits {
            response_timeout: Duration::from_secs(30),
            body_idle_timeout: Duration::from_secs(30),
            max_response_bytes: MAX_RESPONSE,
        };
        let started_at = now_ms();
        let outcome = self
            .session_send(
                "repeater",
                Source::Repeater,
                &destination,
                &security,
                &request,
                &limits,
            )
            .await;
        let mut exchange =
            crate::repeater::captured(Source::Repeater, &target, request, started_at, &outcome);
        exchange.notes = Some(match exchange.notes.take() {
            Some(n) => format!("Sent through the automation API. {n}"),
            None => "Sent through the automation API.".into(),
        });
        let db = self.db()?;
        let exchange_id = tokio::task::spawn_blocking(move || db.insert(exchange)).await??;
        Ok(match outcome {
            Ok(sent) => AutomationSent {
                exchange_id,
                status: Some(sent.head.status),
                error: None,
            },
            Err(e) => AutomationSent {
                exchange_id,
                status: None,
                error: Some(e.message),
            },
        })
    }
}
