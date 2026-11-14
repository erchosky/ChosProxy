//! SQLite project storage (see ADR-0004).
//!
//! A project is one SQLite file. Writes go through a dedicated writer thread
//! that batches inserts into transactions; reads use a small pool of
//! read-only connections. All methods are synchronous; async callers should
//! use `spawn_blocking`.

mod ai_activity;
mod apidefs;
mod blob;
mod cookie_jar;
mod db;
mod dom;
mod error;
mod findings;
mod fuzz;
mod key;
mod macros;
mod migrations;
pub mod model;
mod oast;
mod organizer;
mod passive;
mod project;
mod repeater;
mod schedules;
pub mod sealed;
mod snapshots;
mod suppressions;
mod tasks;
mod websocket;

pub use ai_activity::{AiActivity, MAX_ACTIVITY_PAGE, NewAiActivity};
pub use apidefs::{ApiDefinitionInfo, MAX_API_DEFINITIONS, NewApiDefinition};
pub use cookie_jar::{JarCookie, JarEntry, MAX_JAR_COOKIES};
pub use db::{InsertBatch, OpenOptions, ProjectDb};
pub use dom::{DomEventRow, MAX_DOM_EVENTS, NewDomEvent, without_fragment};
pub use error::StorageError;
pub use findings::{
    Confidence, EvidenceRow, FindingCounts, FindingId, FindingQuery, FindingRow, FindingSeverity,
    FindingStatus, FindingUpdate, NewFinding, StoredEvidence, StoredFinding,
};
pub use fuzz::{FuzzResultQuery, FuzzResultRow, FuzzResultsPage, FuzzSort, NewFuzzResult};
pub use key::{ProjectKey, is_encrypted_file};
pub use model::{
    ExchangeId, Highlight, HistoryOrder, HistoryPage, HistoryQuery, HistoryRow, HistorySort,
    NewExchange, ProjectStats, SiteFilter, SiteMap, SiteMapEntry, SiteMark, SiteMarkEntry,
    StoredExchange,
};
pub use oast::{
    MAX_OAST_PAYLOADS, NewOastInteraction, OastInteractionRow, OastPayloadRow,
    StoredOastInteraction,
};
pub use organizer::{
    NewOrganizerItem, OrganizerId, OrganizerItem, OrganizerStatus, OrganizerUpdate, Severity,
    StoredOrganizerItem, normalize_collection,
};
pub use passive::{MAX_PASSIVE_EVIDENCE, PassiveIssue, PassiveRecorded};
pub use project::{
    AuditEntry, AuditQuery, CopyKey, MAX_SETTING_BYTES, ProjectFileInfo, inspect_project_file,
    inspect_project_file_with,
};
pub use repeater::{
    GroupSendMode, MAX_REPEATER_GROUPS, MAX_REPEATER_REQUEST_BYTES, MAX_REPEATER_TABS,
    NewRepeaterTab, RepeaterGroup, RepeaterGroupId, RepeaterGroupUpdate, RepeaterSendRow,
    RepeaterTab, RepeaterTabId, RepeaterTabUpdate, RepeaterTarget, SendDetails, SendTimingRow,
    request_path, validate_target,
};
pub use schedules::{MAX_SCHEDULES, MIN_REPEAT_MINUTES, NewSchedule, ScheduleRow};
pub use snapshots::{MAX_SNAPSHOTS, SiteChange, SiteDiffEntry, SiteSnapshotInfo, diff_site_maps};
pub use suppressions::{MAX_SUPPRESSIONS, NewSuppression, SuppressionRule};
pub use tasks::{
    DiscoveryHit, MAX_TASK_LOG, TaskId, TaskLogEntry, TaskProgress, TaskRow, TaskStatus,
};
pub use websocket::{
    StoredWsMessage, WsConnectionQuery, WsConnectionRow, WsMessagePage, WsMessageQuery,
    WsMessageRow,
};
