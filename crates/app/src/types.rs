//! Serializable types exposed to user interfaces.

use chosproxy_core::config::ListenerConfig;
use chosproxy_core::event::ListenerState;
use chosproxy_proxy::ProxyStats;
use chosproxy_storage::{FindingCounts, HistoryRow, OrganizerItem, ProjectStats};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Static information about the running application.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfo {
    /// Product name from `branding.json`.
    pub product_name: String,
    /// Tagline.
    pub tagline: String,
    /// Application version.
    pub version: String,
    /// Global configuration directory.
    pub config_dir: String,
    /// Default directory for persistent projects.
    pub data_dir: String,
    /// Log directory.
    pub log_dir: String,
    /// Warnings produced while loading configuration.
    pub config_warnings: Vec<String>,
}

/// Temporary projects are deleted on close; persistent ones are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ProjectKind {
    /// Deleted when closed.
    Temporary,
    /// Saved to a file chosen by the user.
    Persistent,
}

/// The open project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectInfo {
    /// Display name (file stem).
    pub name: String,
    /// Database file path.
    pub path: String,
    /// Temporary or persistent.
    pub kind: ProjectKind,
    /// The project file is encrypted (M31).
    pub encrypted: bool,
    /// Opened in read-only mode (M31a, spec §118).
    pub read_only: bool,
}

/// A configured listener and its live state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ListenerStatus {
    /// Configuration.
    pub config: ListenerConfig,
    /// Live state.
    pub state: ListenerState,
}

/// Dashboard counters.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DashboardStats {
    /// Project contents.
    pub project: ProjectStats,
    /// Open and confirmed findings per severity.
    pub findings: FindingCounts,
    /// Live proxy counters.
    pub proxy: ProxyStats,
    /// Captures dropped because storage could not keep up.
    #[ts(type = "number")]
    pub captures_dropped: u64,
}

/// One message (request or response) prepared for display.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MessageView {
    /// The stored raw message (head + body exactly as on the wire), base64,
    /// limited to the display cap.
    pub raw_base64: String,
    /// Size of the stored raw message.
    #[ts(type = "number")]
    pub raw_size: u64,
    /// `raw_base64` was cut at the display cap.
    pub raw_display_truncated: bool,
    /// The stored copy was cut at the capture cap (wire size is larger).
    pub capture_truncated: bool,
    /// Bytes on the wire.
    #[ts(type = "number | null")]
    pub wire_size: Option<u64>,
    /// Message head as text (lossy UTF-8).
    pub head: String,
    /// Body after removing transfer and content codings, base64.
    pub body_base64: String,
    /// Size of the decoded body.
    #[ts(type = "number")]
    pub body_size: u64,
    /// Decoded body as text, when it looks like text.
    pub body_text: Option<String>,
    /// Codings that were removed to produce the body (e.g. `chunked`, `gzip`).
    pub decoded_from: Vec<String>,
    /// Why decoding was incomplete, if it was.
    pub decode_note: Option<String>,
    /// Content-Type header value.
    pub content_type: Option<String>,
}

/// Full exchange prepared for display.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExchangeDetail {
    /// Metadata.
    pub row: HistoryRow,
    /// Client socket address.
    pub client_addr: Option<String>,
    /// Listener id.
    pub listener: Option<String>,
    /// Response reason phrase.
    pub reason: Option<String>,
    /// Request as sent upstream.
    pub request: MessageView,
    /// Response as delivered (absent if none was received).
    pub response: Option<MessageView>,
    /// Request before it was edited in Intercept.
    pub original_request: Option<MessageView>,
    /// Response before it was edited in Intercept.
    pub original_response: Option<MessageView>,
}

/// An Organizer item with its messages prepared for display.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OrganizerDetail {
    /// Metadata.
    pub item: OrganizerItem,
    /// Request.
    pub request: MessageView,
    /// Response, if one was received.
    pub response: Option<MessageView>,
}
