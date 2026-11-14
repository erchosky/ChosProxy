//! The ChosProxy engine: a UI-agnostic composition root.
//!
//! [`Engine`] owns configuration, the open project, the proxy listeners and
//! the event bus. The desktop app is a thin adapter over this API; a future
//! CLI/headless mode can drive the same methods.

pub mod analysis;
pub mod browser;
mod browser_session;
mod ca_manager;
mod cookies;
mod decode_view;
mod detail;
mod engine;
mod error;
mod event_log;
mod intercept_view;
mod keyring;
pub mod logging;
mod projects;
mod repeater;
mod search;
mod sink;
mod types;
mod ws_view;

pub use browser::{BrowserInfo, BrowserKind};
pub use browser_session::BrowserStatus;
pub use ca_manager::{CaStatus, CertFormat};
pub use chosproxy_ai::agent::AgentToolInfo;
pub use chosproxy_crawler::CrawlConfig;
pub use chosproxy_decoder::Operation as DecodeOperation;
pub use chosproxy_discovery::DiscoveryConfig;
pub use chosproxy_fuzzer::{BuiltinList as FuzzBuiltinList, FuzzConfig};
pub use chosproxy_plugin::Permission as PluginPermission;
pub use chosproxy_scanner::ActiveCheckInfo as ActiveCheck;
pub use chosproxy_scanner::CheckInfo as PassiveCheck;
pub use chosproxy_scanner::{InsertionPoint, PointKind};
pub use chosproxy_sequencer::{CaptureConfig, TokenAnalysis};
pub use decode_view::{DecodeStep, SmartDecode};
pub use engine::BrowserCrawlConfig;
pub use engine::ScriptedSiteMap;
pub use engine::{AgentApproval, AgentApprovalKind, AgentPlanStep, AgentStart, AgentState};
pub use engine::{AiAnswer, AiAttachment, AiPreview, AiRequest, LoginRecording};
pub use engine::{
    ApiDefinitionDetail, ApiRequest, ApiSendConfig, ApiStatus, AutomationSent, NodeParam,
    SiteNodeDetails,
};
pub use engine::{CONFIG_TOOLS, ToolConfig};
pub use engine::{CrawlEdge, CrawlGraph, CrawlNode};
pub use engine::{
    DomStatus, Engine, EvidenceView, FindingDetail, FuzzPreview, OastInteractionDetail, OastStatus,
    PassiveStatus, PermissionInfo, PluginAction, PluginInfo, PluginLogLine, PluginStatus,
    RepeaterGroupItem, RepeaterGroupSendResult, RepeaterSendResult, RepeaterSnapshot, ReportFormat,
    ReportOptions, SequencerPrepared, TaskDetail,
};
pub use engine::{LIVE_SOURCES, LiveMode, LiveTaskConfig};
pub use engine::{OrganizerShare, ProjectEncryption};
pub use engine::{POOL_TOOLS, PoolConfig, PoolStatus, PoolTool, PoolsConfig, PoolsView};
pub use engine::{PerformanceSnapshot, ProcessUsage};
pub use engine::{
    PhaseState, ScanConfig, ScanLaunch, ScanPhase, ScanSummary, ScansConfig, builtin_scan_configs,
};
pub use engine::{
    RecoveryStatus, RecoveryTask, SESSION_MARKER_FILE, SESSION_MARKER_VERSION, SessionMarker,
    current_process_start_secs, process_start_secs,
};
pub use engine::{
    ScriptCell, ScriptLibrary, ScriptRun, ScriptedHistoryPage, SessionExtraction, SessionHeader,
    SessionRule,
};
pub use engine::{SecurityCheck, SecurityCheckStatus, SecurityOverview};
pub use engine::{TempCleanup, TempEntry, TempKind, TempStatus};
pub use error::{AppError, ErrorPayload};
pub use event_log::{EVENT_LOG_CAPACITY, LogCategory, LogEntry};
pub use intercept_view::{HeldDetail, HeldEdit, InterceptState};
pub use keyring::{CredentialInfo, KeyringProtection, KeyringStatus};
pub use projects::{
    BackupInfo, BackupSettings, ProjectSettings, SectionPreview, SettingsImportPreview,
    SettingsSection,
};
pub use repeater::RepeaterRedirect;
pub use search::{SearchHit, SearchLocation, SearchQuery, SearchResults};
pub use types::{
    AppInfo, DashboardStats, ExchangeDetail, ListenerStatus, MessageView, OrganizerDetail,
    ProjectInfo, ProjectKind,
};
pub use ws_view::WsMessageDetail;
