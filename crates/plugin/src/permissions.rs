//! The permission vocabulary (spec §36, ADR-0010). A plugin declares what
//! it needs; the tester grants each permission; nothing is granted
//! automatically, and every action is checked by the host.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One capability a plugin can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export, rename = "PluginPermission")]
pub enum Permission {
    /// Receive the requests and responses stored in the project.
    #[serde(rename = "network.read")]
    NetworkRead,
    /// Add informational and passive findings.
    #[serde(rename = "scanner.passive")]
    ScannerPassive,
    /// Annotate history entries (highlight and note).
    #[serde(rename = "project.write")]
    ProjectWrite,
    /// Send requests through ChosProxy (API v2).
    #[serde(rename = "network.send")]
    NetworkSend,
    /// Read stored traffic and findings on request (API v2).
    #[serde(rename = "project.read")]
    ProjectRead,
    /// Add actions to request menus (API v2).
    #[serde(rename = "ui.extend")]
    UiExtend,
    /// See in-scope proxy requests before they are sent and let them go,
    /// change, block or answer them (API v2).
    #[serde(rename = "proxy.intercept")]
    ProxyIntercept,
}

impl Permission {
    /// Every permission, in display order.
    pub const ALL: [Permission; 7] = [
        Permission::NetworkRead,
        Permission::ScannerPassive,
        Permission::ProjectWrite,
        Permission::NetworkSend,
        Permission::ProjectRead,
        Permission::UiExtend,
        Permission::ProxyIntercept,
    ];

    /// The name used in manifests and grants.
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::NetworkRead => "network.read",
            Permission::ScannerPassive => "scanner.passive",
            Permission::ProjectWrite => "project.write",
            Permission::NetworkSend => "network.send",
            Permission::ProjectRead => "project.read",
            Permission::UiExtend => "ui.extend",
            Permission::ProxyIntercept => "proxy.intercept",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(s: &str) -> Option<Permission> {
        Permission::ALL.into_iter().find(|p| p.as_str() == s)
    }

    /// Whether plugin API v1 implements it.
    pub fn supported(self) -> bool {
        self.supported_in(1)
    }

    /// Whether plugin API `api` implements it.
    pub fn supported_in(self, api: u32) -> bool {
        match api {
            1 => matches!(
                self,
                Permission::NetworkRead | Permission::ScannerPassive | Permission::ProjectWrite
            ),
            2 => true,
            _ => false,
        }
    }

    /// What granting it allows, for the tester deciding.
    pub fn description(self) -> &'static str {
        match self {
            Permission::NetworkRead => {
                "Receive every request and response stored in the open project (headers and \
                 bodies, including cookies and credentials they contain)."
            }
            Permission::ScannerPassive => {
                "Add findings to the project, linked to the request they were seen in."
            }
            Permission::ProjectWrite => "Highlight history entries and set their notes.",
            Permission::NetworkSend => {
                "Send requests to servers through ChosProxy (stored in the history, refused out of \
                 scope when a scope is defined, rate-limited)."
            }
            Permission::ProjectRead => {
                "Read stored requests, responses and findings of the open project when it asks."
            }
            Permission::UiExtend => {
                "Add actions to request menus; running one gives the plugin that request."
            }
            Permission::ProxyIntercept => {
                "See in-scope requests going through the proxy before they are sent, and let \
                 them go, change them, block them or answer them itself. Only with a scope \
                 defined; a plugin that does not answer within 2 seconds is skipped."
            }
        }
    }
}
