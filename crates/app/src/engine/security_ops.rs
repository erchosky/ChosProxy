//! Security Center (M31 F, spec §120): local-only security and privacy
//! posture overview across the engine, CA, listeners, Automation API,
//! plugins, ChosAI/ChosAgent, network/upstream proxies, ChosOAST, TLS
//! verification exceptions, project encryption, and temporary files.
//!
//! Never performs network I/O and never returns private keys, passwords,
//! tokens, or stored secrets.

use super::Engine;
use crate::keyring::KeyringProtection;
use crate::types::ProjectKind;
use chosproxy_ai::agent::ToolKind;
use chosproxy_core::config::{AgentPermission, AiMode, AiProvider, OastMode, UpstreamKind};
use chosproxy_plugin::Permission;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::Path;
use ts_rs::TS;

/// Status of one Security Center check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SecurityCheckStatus {
    /// Hardened / default-safe state.
    Ok,
    /// Worth reviewing or increases exposure / trust surface.
    Warn,
    /// Informational (explicitly configured capability with controls active).
    Info,
    /// Not applicable right now (e.g., no project open).
    Unavailable,
}

/// One row in the Security Center overview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SecurityCheck {
    /// Stable identifier (e.g. `"ca_certificate"`).
    pub id: String,
    /// Short title shown in the UI.
    pub title: String,
    /// Check status.
    pub status: SecurityCheckStatus,
    /// One-line summary of the current state.
    pub summary: String,
    /// Additional context (paths, counts, or recommendations; never secrets).
    pub detail: String,
    /// View id to navigate to (`"https"`, `"listeners"`, `"plugins"`, `"oast"`, `"project"`, `"settings"`).
    pub target_view: String,
    /// Optional section anchor inside the target view.
    pub target_section: Option<String>,
}

/// Full Security Center snapshot returned by [`Engine::security_overview`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SecurityOverview {
    /// Ordered list of security checks.
    pub checks: Vec<SecurityCheck>,
    /// Count of `Ok` checks.
    pub ok_count: u32,
    /// Count of `Warn` checks.
    pub warn_count: u32,
    /// Count of `Info` checks.
    pub info_count: u32,
    /// Count of `Unavailable` checks.
    pub unavailable_count: u32,
}

fn is_loopback_addr(addr: &str) -> bool {
    let trimmed = addr.trim().trim_start_matches('[').trim_end_matches(']');
    if trimmed.eq_ignore_ascii_case("localhost") {
        return true;
    }
    trimmed.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn key_file_is_private(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| (m.permissions().mode() & 0o077) == 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

impl Engine {
    /// Computes the local Security Center overview (M31 F, spec §120).
    /// Works both with and without an open project.
    pub async fn security_overview(&self) -> SecurityOverview {
        let mut checks = Vec::with_capacity(13);

        // 1. Root CA certificate & private key
        let ca = self.ca_status();
        checks.push(match ca.info {
            Some(info) if ca.available => {
                let key_private = key_file_is_private(Path::new(&info.key_path));
                let now = chosproxy_core::time::now_ms();
                let expired = info.not_after <= now;
                let (status, summary) = if expired {
                    (
                        SecurityCheckStatus::Warn,
                        "Local Root CA certificate has expired".into(),
                    )
                } else if !key_private {
                    (
                        SecurityCheckStatus::Warn,
                        "Local Root CA private key permissions are broader than 0600".into(),
                    )
                } else {
                    (
                        SecurityCheckStatus::Ok,
                        format!("Per-installation Root CA ready ({})", info.subject),
                    )
                };
                SecurityCheck {
                    id: "ca_certificate".into(),
                    title: "Root CA & private key".into(),
                    status,
                    summary,
                    detail: format!(
                        "SHA-256 {} · Key path: {} (private key material is never exported)",
                        info.fingerprint_sha256, info.key_path
                    ),
                    target_view: "https".into(),
                    target_section: None,
                }
            }
            _ => SecurityCheck {
                id: "ca_certificate".into(),
                title: "Root CA & private key".into(),
                status: SecurityCheckStatus::Warn,
                summary: "Local Root CA is unavailable".into(),
                detail: ca
                    .error
                    .unwrap_or_else(|| "Could not initialize local CA.".into()),
                target_view: "https".into(),
                target_section: None,
            },
        });

        // 2. Proxy listeners & non-loopback exposure
        let proxy_cfg = self.proxy_config().await;
        let mut remote_unauth = Vec::new();
        let mut remote_auth = Vec::new();
        for l in &proxy_cfg.listeners {
            if l.allow_remote || !l.bind_address.is_loopback() {
                let label = format!("{}:{}", l.bind_address, l.port);
                if l.auth.is_some() {
                    remote_auth.push(label);
                } else {
                    remote_unauth.push(label);
                }
            }
        }
        let listener_check = if !remote_unauth.is_empty() {
            SecurityCheck {
                id: "proxy_listeners".into(),
                title: "Proxy listeners & bind exposure".into(),
                status: SecurityCheckStatus::Warn,
                summary: format!(
                    "{} listener(s) bound beyond loopback without authentication",
                    remote_unauth.len()
                ),
                detail: format!(
                    "Unauthenticated non-loopback listeners: {}. Restrict to 127.0.0.1 or configure listener authentication.",
                    remote_unauth.join(", ")
                ),
                target_view: "listeners".into(),
                target_section: None,
            }
        } else if !remote_auth.is_empty() {
            SecurityCheck {
                id: "proxy_listeners".into(),
                title: "Proxy listeners & bind exposure".into(),
                status: SecurityCheckStatus::Info,
                summary: format!(
                    "{} non-loopback listener(s) protected by authentication",
                    remote_auth.len()
                ),
                detail: format!(
                    "Authenticated remote listeners: {}.",
                    remote_auth.join(", ")
                ),
                target_view: "listeners".into(),
                target_section: None,
            }
        } else {
            SecurityCheck {
                id: "proxy_listeners".into(),
                title: "Proxy listeners & bind exposure".into(),
                status: SecurityCheckStatus::Ok,
                summary: format!(
                    "All {} listener(s) bound to loopback only",
                    proxy_cfg.listeners.len()
                ),
                detail: "Only processes on this machine can connect to the proxy listeners.".into(),
                target_view: "listeners".into(),
                target_section: None,
            }
        };
        checks.push(listener_check);

        // 3. Automation API
        let api_cfg = self.api_config();
        let token_file = self
            .paths()
            .data_dir
            .join("api.token")
            .display()
            .to_string();
        let api_check = if !api_cfg.enabled {
            SecurityCheck {
                id: "automation_api".into(),
                title: "Automation API".into(),
                status: SecurityCheckStatus::Ok,
                summary: "Automation API is disabled".into(),
                detail: "No HTTP automation listener is active.".into(),
                target_view: "settings".into(),
                target_section: Some("automation".into()),
            }
        } else {
            SecurityCheck {
                id: "automation_api".into(),
                title: "Automation API".into(),
                status: SecurityCheckStatus::Info,
                summary: format!(
                    "Automation API enabled on loopback (127.0.0.1:{}) with bearer token",
                    api_cfg.port
                ),
                detail: format!(
                    "Token file: {token_file} (0600; token value is never exported here)."
                ),
                target_view: "settings".into(),
                target_section: Some("automation".into()),
            }
        };
        checks.push(api_check);

        // 4. Installed plugins & granted permissions
        let plugins = self.plugins().await.unwrap_or_default();
        let enabled_plugins: Vec<_> = plugins.iter().filter(|p| p.enabled).collect();
        let active_perms: Vec<String> = enabled_plugins
            .iter()
            .filter(|p| {
                p.granted
                    .iter()
                    .any(|g| matches!(g, Permission::NetworkSend | Permission::ProxyIntercept))
            })
            .map(|p| p.name.clone())
            .collect();
        let plugins_check = if enabled_plugins.is_empty() {
            SecurityCheck {
                id: "plugins_permissions".into(),
                title: "Plugins & permissions".into(),
                status: SecurityCheckStatus::Ok,
                summary: format!("No plugins enabled ({} installed)", plugins.len()),
                detail:
                    "Plugins run in separate processes and receive zero permissions by default."
                        .into(),
                target_view: "plugins".into(),
                target_section: None,
            }
        } else if !active_perms.is_empty() {
            SecurityCheck {
                id: "plugins_permissions".into(),
                title: "Plugins & permissions".into(),
                status: SecurityCheckStatus::Warn,
                summary: format!(
                    "{} enabled plugin(s) hold network.send or proxy.intercept permissions",
                    active_perms.len()
                ),
                detail: format!("Active traffic/hook plugins: {}.", active_perms.join(", ")),
                target_view: "plugins".into(),
                target_section: None,
            }
        } else {
            SecurityCheck {
                id: "plugins_permissions".into(),
                title: "Plugins & permissions".into(),
                status: SecurityCheckStatus::Info,
                summary: format!(
                    "{} plugin(s) enabled with read/UI permissions only",
                    enabled_plugins.len()
                ),
                detail: enabled_plugins
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                target_view: "plugins".into(),
                target_section: None,
            }
        };
        checks.push(plugins_check);

        // 5. Plugin signature policy
        let trust_cfg = self.plugins_trust_settings();
        let unsigned_enabled = enabled_plugins
            .iter()
            .any(|p| p.signature.state != "trusted");
        let signing_check = if trust_cfg.require_signed {
            SecurityCheck {
                id: "plugins_signing".into(),
                title: "Plugin signature verification".into(),
                status: SecurityCheckStatus::Ok,
                summary: format!(
                    "Signed-only policy enabled ({} trusted publisher key(s))",
                    trust_cfg.trusted.len()
                ),
                detail: "Unsigned or untrusted plugins cannot be enabled or run.".into(),
                target_view: "plugins".into(),
                target_section: None,
            }
        } else if unsigned_enabled {
            SecurityCheck {
                id: "plugins_signing".into(),
                title: "Plugin signature verification".into(),
                status: SecurityCheckStatus::Warn,
                summary: "Unsigned or untrusted plugin is enabled while signed-only policy is off".into(),
                detail: "Enable 'Require signed plugins' in Plugins to enforce Ed25519 publisher verification.".into(),
                target_view: "plugins".into(),
                target_section: None,
            }
        } else {
            SecurityCheck {
                id: "plugins_signing".into(),
                title: "Plugin signature verification".into(),
                status: SecurityCheckStatus::Info,
                summary: format!(
                    "Signed-only policy is off ({} trusted publisher key(s), no unsigned plugins enabled)",
                    trust_cfg.trusted.len()
                ),
                detail: "You can require Ed25519 signatures for all plugins in the Plugins view."
                    .into(),
                target_view: "plugins".into(),
                target_section: None,
            }
        };
        checks.push(signing_check);

        // 6. ChosAI mode & provider
        let ai_cfg = self.ai_config();
        let provider_label = match ai_cfg.provider {
            AiProvider::Ollama => "Ollama",
            AiProvider::OpenAiCompatible => "OpenAI-compatible",
        };
        let ai_check = match ai_cfg.mode {
            AiMode::Disabled => SecurityCheck {
                id: "chos_ai".into(),
                title: "ChosAI model & data boundary".into(),
                status: SecurityCheckStatus::Ok,
                summary: "ChosAI is disabled (no model calls)".into(),
                detail: "No traffic or project context is sent to any language model.".into(),
                target_view: "settings".into(),
                target_section: Some("ai".into()),
            },
            AiMode::Local => SecurityCheck {
                id: "chos_ai".into(),
                title: "ChosAI model & data boundary".into(),
                status: SecurityCheckStatus::Ok,
                summary: format!("Local loopback model ({provider_label}: {})", ai_cfg.model),
                detail: format!(
                    "Endpoint {} stays on this machine; secrets are redacted before prompting.",
                    ai_cfg.base_url
                ),
                target_view: "settings".into(),
                target_section: Some("ai".into()),
            },
            AiMode::External => SecurityCheck {
                id: "chos_ai".into(),
                title: "ChosAI model & data boundary".into(),
                status: SecurityCheckStatus::Warn,
                summary: format!(
                    "External model provider enabled ({provider_label}: {})",
                    ai_cfg.model
                ),
                detail: format!(
                    "Endpoint: {}. Every external prompt requires explicit preview and redacts secrets.",
                    ai_cfg.base_url
                ),
                target_view: "settings".into(),
                target_section: Some("ai".into()),
            },
        };
        checks.push(ai_check);

        // 7. ChosAgent tool permissions
        let agent_tools = self.agent_tools();
        let allow_mutating: Vec<&str> = agent_tools
            .iter()
            .filter(|t| {
                t.permission == AgentPermission::Allow
                    && matches!(t.kind, ToolKind::Project | ToolKind::Send)
            })
            .map(|t| t.name.as_str())
            .collect();
        let allow_read: Vec<&str> = agent_tools
            .iter()
            .filter(|t| t.permission == AgentPermission::Allow)
            .map(|t| t.name.as_str())
            .collect();
        let agent_check = if !allow_mutating.is_empty() {
            SecurityCheck {
                id: "chos_agent".into(),
                title: "ChosAgent tool permissions".into(),
                status: SecurityCheckStatus::Warn,
                summary: format!(
                    "{} project-modifying tool(s) set to Allow without per-call prompt",
                    allow_mutating.len()
                ),
                detail: format!(
                    "Auto-allowed mutating tools: {}. Sending tools always require Ask.",
                    allow_mutating.join(", ")
                ),
                target_view: "settings".into(),
                target_section: Some("agent".into()),
            }
        } else {
            SecurityCheck {
                id: "chos_agent".into(),
                title: "ChosAgent tool permissions".into(),
                status: SecurityCheckStatus::Ok,
                summary: "All traffic-sending and project-modifying tools require Ask or Deny"
                    .into(),
                detail: format!(
                    "{} read/plan tool(s) set to Allow ({})",
                    allow_read.len(),
                    allow_read.join(", ")
                ),
                target_view: "settings".into(),
                target_section: Some("agent".into()),
            }
        };
        checks.push(agent_check);

        // 8. Upstream proxies & host overrides
        let net_cfg = self.network_config();
        let active_upstreams: Vec<String> = net_cfg
            .upstream
            .iter()
            .filter(|u| u.enabled && u.kind != UpstreamKind::Direct)
            .map(|u| format!("{} -> {}:{}", u.host, u.proxy_host, u.proxy_port))
            .collect();
        let active_hosts: Vec<String> = net_cfg
            .hosts
            .iter()
            .filter(|h| h.enabled)
            .map(|h| format!("{} -> {}", h.host, h.address))
            .collect();
        let upstream_check = if active_upstreams.is_empty() && active_hosts.is_empty() {
            SecurityCheck {
                id: "upstream_proxies".into(),
                title: "Upstream proxies & host overrides".into(),
                status: SecurityCheckStatus::Ok,
                summary: "Direct connections (no upstream proxy or DNS override active)".into(),
                detail: "Outgoing requests connect directly to target hosts.".into(),
                target_view: "settings".into(),
                target_section: Some("network".into()),
            }
        } else {
            SecurityCheck {
                id: "upstream_proxies".into(),
                title: "Upstream proxies & host overrides".into(),
                status: SecurityCheckStatus::Info,
                summary: format!(
                    "{} upstream proxy rule(s) and {} host override(s) active",
                    active_upstreams.len(),
                    active_hosts.len()
                ),
                detail: [active_upstreams, active_hosts].concat().join("; "),
                target_view: "settings".into(),
                target_section: Some("network".into()),
            }
        };
        checks.push(upstream_check);

        // 9. ChosOAST out-of-band interactions
        let oast_cfg = self.oast_config();
        let oast_check = match oast_cfg.mode {
            OastMode::Off => SecurityCheck {
                id: "oast".into(),
                title: "ChosOAST out-of-band service".into(),
                status: SecurityCheckStatus::Ok,
                summary: "ChosOAST is off (no listeners or external polling)".into(),
                detail: "No out-of-band callbacks or external OAST servers are contacted.".into(),
                target_view: "oast".into(),
                target_section: None,
            },
            OastMode::Local => {
                let loopback = is_loopback_addr(&oast_cfg.local.bind);
                SecurityCheck {
                    id: "oast".into(),
                    title: "ChosOAST out-of-band service".into(),
                    status: if loopback {
                        SecurityCheckStatus::Info
                    } else {
                        SecurityCheckStatus::Warn
                    },
                    summary: format!(
                        "Local ChosOAST listener on {}:{}",
                        oast_cfg.local.bind, oast_cfg.local.http_port
                    ),
                    detail: format!(
                        "DNS: {}, SMTP: {}.",
                        if oast_cfg.local.dns_enabled {
                            "on"
                        } else {
                            "off"
                        },
                        if oast_cfg.local.smtp_enabled {
                            "on"
                        } else {
                            "off"
                        }
                    ),
                    target_view: "oast".into(),
                    target_section: None,
                }
            }
            OastMode::SelfHosted => SecurityCheck {
                id: "oast".into(),
                title: "ChosOAST out-of-band service".into(),
                status: SecurityCheckStatus::Warn,
                summary: format!(
                    "Self-hosted OAST server polled ({}:{})",
                    oast_cfg.self_hosted.api_host, oast_cfg.self_hosted.api_port
                ),
                detail: format!(
                    "Zone: {} · Poll interval: {}s (access token is kept private).",
                    oast_cfg.self_hosted.zone, oast_cfg.poll_seconds
                ),
                target_view: "oast".into(),
                target_section: None,
            },
        };
        checks.push(oast_check);

        // 10. Upstream TLS verification exceptions
        let non_loopback_skip: Vec<String> = proxy_cfg
            .tls
            .skip_verification_hosts
            .iter()
            .filter(|h| !is_loopback_addr(h))
            .cloned()
            .collect();
        let tls_check = if non_loopback_skip.is_empty() {
            SecurityCheck {
                id: "tls_verification".into(),
                title: "Upstream TLS verification".into(),
                status: SecurityCheckStatus::Ok,
                summary: "Strict TLS verification for all non-loopback hosts".into(),
                detail: "Only loopback lab hosts allow self-signed upstream certificates.".into(),
                target_view: "https".into(),
                target_section: None,
            }
        } else {
            SecurityCheck {
                id: "tls_verification".into(),
                title: "Upstream TLS verification".into(),
                status: SecurityCheckStatus::Warn,
                summary: format!(
                    "TLS certificate verification skipped for {} non-loopback host pattern(s)",
                    non_loopback_skip.len()
                ),
                detail: format!("Exceptions: {}.", non_loopback_skip.join(", ")),
                target_view: "https".into(),
                target_section: None,
            }
        };
        checks.push(tls_check);

        // 11. Project encryption & read-only status
        let project_check = match self.project() {
            None => SecurityCheck {
                id: "project_encryption".into(),
                title: "Open project encryption".into(),
                status: SecurityCheckStatus::Unavailable,
                summary: "No project is currently open".into(),
                detail: "Open or create an encrypted project to protect stored traffic at rest with SQLCipher.".into(),
                target_view: "project".into(),
                target_section: None,
            },
            Some(proj) => {
                let ro_suffix = if proj.read_only { " (read-only)" } else { "" };
                if proj.encrypted {
                    SecurityCheck {
                        id: "project_encryption".into(),
                        title: "Open project encryption".into(),
                        status: SecurityCheckStatus::Ok,
                        summary: format!(
                            "Project '{}' is encrypted with SQLCipher{}",
                            proj.name, ro_suffix
                        ),
                        detail: format!("Path: {}", proj.path),
                        target_view: "project".into(),
                        target_section: None,
                    }
                } else if proj.kind == ProjectKind::Temporary {
                    SecurityCheck {
                        id: "project_encryption".into(),
                        title: "Open project encryption".into(),
                        status: SecurityCheckStatus::Info,
                        summary: format!(
                            "Temporary project '{}' is unencrypted on disk{}",
                            proj.name, ro_suffix
                        ),
                        detail: "Use 'Encrypted temporary project' on the welcome screen to encrypt temp projects with an ephemeral key.".into(),
                        target_view: "project".into(),
                        target_section: None,
                    }
                } else {
                    SecurityCheck {
                        id: "project_encryption".into(),
                        title: "Open project encryption".into(),
                        status: SecurityCheckStatus::Warn,
                        summary: format!(
                            "Persistent project '{}' is stored unencrypted{}",
                            proj.name, ro_suffix
                        ),
                        detail: "You can encrypt this project in the Project view with 'Encrypt with password…'.".into(),
                        target_view: "project".into(),
                        target_section: None,
                    }
                }
            }
        };
        checks.push(project_check);

        // 12. Temporary files & crash recovery state
        let temp_st = self.temp_status().await.ok();
        let rec_st = self.recovery_status();
        let temp_dir = temp_st
            .as_ref()
            .map(|t| t.dir.clone())
            .unwrap_or_else(|| self.paths().temp_dir.display().to_string());
        let total_entries = temp_st.as_ref().map(|t| t.entries.len()).unwrap_or(0);
        let removable_entries = temp_st
            .as_ref()
            .map(|t| t.entries.iter().filter(|e| e.removable).count())
            .unwrap_or(0);
        let temp_check = if rec_st.is_some() {
            SecurityCheck {
                id: "temp_files".into(),
                title: "Temporary files & session recovery".into(),
                status: SecurityCheckStatus::Warn,
                summary: "An unrecovered session marker is waiting for review".into(),
                detail: format!("Temp directory: {temp_dir}"),
                target_view: "settings".into(),
                target_section: Some("storage".into()),
            }
        } else if removable_entries > 0 {
            SecurityCheck {
                id: "temp_files".into(),
                title: "Temporary files & session recovery".into(),
                status: SecurityCheckStatus::Warn,
                summary: format!(
                    "{} leftover temporary item(s) can be cleaned up",
                    removable_entries
                ),
                detail: format!("Temp directory: {temp_dir}"),
                target_view: "settings".into(),
                target_section: Some("storage".into()),
            }
        } else {
            SecurityCheck {
                id: "temp_files".into(),
                title: "Temporary files & session recovery".into(),
                status: SecurityCheckStatus::Ok,
                summary: format!("No orphaned temporary files ({total_entries} active item(s))"),
                detail: format!("Temp directory: {temp_dir}"),
                target_view: "settings".into(),
                target_section: Some("storage".into()),
            }
        };
        checks.push(temp_check);

        // 13. Credential keyring protection
        let kr = self.keyring_status().await.ok();
        let creds_count = self.credentials().await.map(|c| c.len()).unwrap_or(0);
        let protection = kr
            .as_ref()
            .map(|k| k.protection)
            .unwrap_or(KeyringProtection::File);
        let locked = kr.as_ref().is_some_and(|k| k.locked);
        let kr_check = match protection {
            KeyringProtection::Password => SecurityCheck {
                id: "keyring_protection".into(),
                title: "Credential keyring protection".into(),
                status: SecurityCheckStatus::Ok,
                summary: format!(
                    "Keyring protected by master password ({} credential(s), {})",
                    creds_count,
                    if locked { "locked" } else { "unlocked" }
                ),
                detail: "AES-256-GCM key is derived via PBKDF2-HMAC-SHA256 from your master password.".into(),
                target_view: "settings".into(),
                target_section: Some("credentials".into()),
            },
            KeyringProtection::OsKeychain => SecurityCheck {
                id: "keyring_protection".into(),
                title: "Credential keyring protection".into(),
                status: SecurityCheckStatus::Ok,
                summary: format!(
                    "Keyring key stored in OS keychain ({} credential(s))",
                    creds_count
                ),
                detail: "AES-256-GCM key is managed by the operating system keychain.".into(),
                target_view: "settings".into(),
                target_section: Some("credentials".into()),
            },
            KeyringProtection::File => SecurityCheck {
                id: "keyring_protection".into(),
                title: "Credential keyring protection".into(),
                status: if creds_count > 0 {
                    SecurityCheckStatus::Warn
                } else {
                    SecurityCheckStatus::Info
                },
                summary: format!(
                    "Keyring uses a local key file (0600) with {} stored credential(s)",
                    creds_count
                ),
                detail: "Set a master password or enable OS keychain protection in Settings → Credentials.".into(),
                target_view: "settings".into(),
                target_section: Some("credentials".into()),
            },
        };
        checks.push(kr_check);

        let mut ok_count = 0;
        let mut warn_count = 0;
        let mut info_count = 0;
        let mut unavailable_count = 0;
        for c in &checks {
            match c.status {
                SecurityCheckStatus::Ok => ok_count += 1,
                SecurityCheckStatus::Warn => warn_count += 1,
                SecurityCheckStatus::Info => info_count += 1,
                SecurityCheckStatus::Unavailable => unavailable_count += 1,
            }
        }

        SecurityOverview {
            checks,
            ok_count,
            warn_count,
            info_count,
            unavailable_count,
        }
    }
}
