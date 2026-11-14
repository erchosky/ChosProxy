//! Events the engine broadcasts to user interfaces.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Lifecycle state of a proxy listener.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum ListenerState {
    /// Not running.
    Stopped,
    /// Accepting connections.
    Running {
        /// Actual bound address (resolves port 0).
        address: String,
    },
    /// Failed to start or crashed.
    Failed {
        /// Human-readable reason.
        error: String,
    },
}

/// Severity of a user-facing notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum NoticeLevel {
    /// Informational.
    Info,
    /// Something the user should look at.
    Warning,
    /// An operation failed.
    Error,
}

/// Event emitted by the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum EngineEvent {
    /// A listener changed state.
    ListenerChanged {
        /// Listener id.
        listener_id: String,
        /// New state.
        state: ListenerState,
    },
    /// New exchanges were stored in the history.
    HistoryAppended {
        /// Highest exchange id stored so far.
        #[ts(type = "number")]
        last_id: i64,
        /// Number of exchanges added in this batch.
        added: u32,
    },
    /// The open project changed (opened, closed).
    ProjectChanged,
    /// The local CA was generated, regenerated or failed to load.
    CaChanged,
    /// The lab browser started or exited.
    BrowserChanged,
    /// The scope rules changed.
    ScopeChanged,
    /// WebSocket connections or messages were stored.
    WebSocketsChanged,
    /// Findings were added or changed by an automatic source (passive
    /// analysis).
    FindingsChanged {
        /// New findings.
        created: u32,
    },
    /// A task was created, changed state or made progress.
    TasksChanged,
    /// The AI activity log grew, or an agent's plan, approvals or queued
    /// messages changed.
    AgentChanged {
        /// The agent task (none for one-shot ChosAI questions).
        #[ts(type = "number | null")]
        task_id: Option<i64>,
    },
    /// A plugin started, stopped, crashed or logged something.
    PluginsChanged,
    /// An OAST interaction arrived, a payload was created or the OAST
    /// server started, stopped or failed.
    OastChanged,
    /// The DOM Inspector started, stopped or observed something.
    DomChanged,
    /// Intercept was switched or the queue of paused messages changed.
    InterceptChanged {
        /// Messages currently paused.
        pending: u32,
    },
    /// A message for the user (errors that are not tied to a request).
    Notice {
        /// Severity.
        level: NoticeLevel,
        /// Text.
        message: String,
    },
}
