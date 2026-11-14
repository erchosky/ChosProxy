//! The engine: configuration, project lifecycle, listeners and queries.

use crate::AppError;
use crate::browser::{self, BrowserInfo};
use crate::browser_session::{BrowserSessions, BrowserStatus};
use crate::ca_manager::{CaManager, CaStatus, CertFormat};
use crate::decode_view::{self, DecodeStep, SmartDecode};
use crate::detail::message_view;
use crate::event_log::{self, EventLog, LogCategory, LogEntry};
use crate::intercept_view::{self, HeldDetail, HeldEdit, InterceptState};
use crate::search::{self, SearchQuery, SearchResults};
use crate::sink::{CurrentDb, ProjectSink};
use crate::types::{
    AppInfo, DashboardStats, ExchangeDetail, ListenerStatus, OrganizerDetail, ProjectInfo,
    ProjectKind,
};
use crate::ws_view::{self, WsMessageDetail};
use chosproxy_core::config::{
    AppSettings, ConfigStore, ListenerConfig, NetworkConfig, ProxyAuth, ProxyConfig, ScriptKind,
    ScriptsConfig,
};
use chosproxy_core::event::{EngineEvent, ListenerState, NoticeLevel};
use chosproxy_core::paths::AppPaths;
use chosproxy_core::scope::{ScopeProtocol, ScopeRule, UrlParts};
use chosproxy_core::{Scope, ScopeConfig, ScopeDecision, brand};
use chosproxy_decoder::Operation;
use chosproxy_proxy::{CaptureLimits, ProxyObserver, ProxyService};
use chosproxy_storage::{
    Highlight, HistoryPage, HistoryQuery, InsertBatch, OpenOptions, OrganizerId, OrganizerItem,
    OrganizerUpdate, ProjectDb, ProjectKey, SiteMap, SiteMark, WsConnectionQuery, WsConnectionRow,
    WsMessagePage, WsMessageQuery,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, broadcast};

mod agent_ops;
mod ai_ops;
mod api_ops;
mod automation_ops;
mod browser_crawl_ops;
mod crawl_graph_ops;
mod discovery_ops;
mod dom_ops;
mod findings_ops;
mod fuzz_ops;
mod graphql_ops;
mod keyring_ops;
mod live_ops;
mod network_ops;
mod oast_ops;
mod passive_ops;
mod perf_ops;
mod plugin_hooks;
mod plugin_ops;
mod pool_ops;
mod project_ops;
mod recovery_ops;
mod repeater_ops;
mod report_ops;
mod scan_ops;
mod schedule_ops;
mod script_ops;
mod security_ops;
mod sequencer_ops;
mod session_ops;
mod share_ops;
mod sitenode_ops;
mod task_ops;
mod temp_ops;
mod toolconfig_ops;

pub use agent_ops::{AgentApproval, AgentApprovalKind, AgentPlanStep, AgentStart, AgentState};
pub use ai_ops::{AiAnswer, AiAttachment, AiPreview, AiRequest, LoginRecording};
pub use api_ops::{ApiDefinitionDetail, ApiRequest, ApiSendConfig};
pub use automation_ops::{ApiStatus, AutomationSent};
pub use browser_crawl_ops::BrowserCrawlConfig;
pub use crawl_graph_ops::{CrawlEdge, CrawlGraph, CrawlNode};
pub use dom_ops::DomStatus;
pub use findings_ops::{EvidenceView, FindingDetail};
pub use fuzz_ops::FuzzPreview;
pub use live_ops::{LIVE_SOURCES, LiveMode, LiveTaskConfig};
pub use oast_ops::{OastInteractionDetail, OastStatus};
pub use passive_ops::PassiveStatus;
pub use perf_ops::{PerformanceSnapshot, ProcessUsage};
pub use plugin_ops::{PermissionInfo, PluginAction, PluginInfo, PluginLogLine, PluginStatus};
pub use pool_ops::{POOL_TOOLS, PoolConfig, PoolStatus, PoolTool, PoolsConfig, PoolsView};
pub use project_ops::ProjectEncryption;
pub use recovery_ops::{
    RecoveryStatus, RecoveryTask, SESSION_MARKER_FILE, SESSION_MARKER_VERSION, SessionMarker,
    current_process_start_secs, process_start_secs,
};
pub use repeater_ops::{
    RepeaterGroupItem, RepeaterGroupSendResult, RepeaterSendResult, RepeaterSnapshot,
};
pub use report_ops::{ReportFormat, ReportOptions};
pub use scan_ops::{
    PhaseState, ScanConfig, ScanLaunch, ScanPhase, ScanSummary, ScansConfig, builtin_scan_configs,
};
pub use script_ops::{ScriptCell, ScriptLibrary, ScriptRun, ScriptedHistoryPage, ScriptedSiteMap};
pub use security_ops::{SecurityCheck, SecurityCheckStatus, SecurityOverview};
pub use sequencer_ops::SequencerPrepared;
pub use session_ops::{SessionExtraction, SessionHeader, SessionRule};
pub use share_ops::OrganizerShare;
pub use sitenode_ops::{NodeParam, SiteNodeDetails};
pub use task_ops::TaskDetail;
pub use temp_ops::{TempCleanup, TempEntry, TempKind, TempStatus};
pub use toolconfig_ops::{CONFIG_TOOLS, ToolConfig};

/// File extension of project databases.
pub const PROJECT_EXTENSION: &str = "chosproxy";

/// Capacity of the event broadcast channel. Slow subscribers lag and skip
/// events rather than slowing the engine down.
const EVENT_CAPACITY: usize = 1024;

/// The ChosProxy engine. Cheap to clone.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
    /// Held only by caller-owned `Engine` handles (never by internal
    /// background tasks). When the last caller handle is dropped without
    /// `shutdown()`, `Drop` marks the session as exited and aborts background
    /// tasks while preserving any crash recovery marker on disk.
    #[allow(dead_code)]
    handle: Option<Arc<SessionHandle>>,
}

struct SessionHandle {
    session_id: String,
    inner: std::sync::Weak<Inner>,
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        recovery_ops::unregister_session(&self.session_id);
        if let Some(inner) = self.inner.upgrade() {
            if let Some(t) = lock(&inner.backup_task).take() {
                t.abort();
            }
            if let Some(t) = lock(&inner.passive_task).take() {
                t.abort();
            }
            if let Some(t) = lock(&inner.scheduler_task).take() {
                t.abort();
            }
            if let Some(t) = lock(&inner.dom_task).take() {
                t.abort();
            }
            let running: Vec<_> = lock(&inner.tasks_running).drain().collect();
            for (_, task) in running {
                task.abort();
            }
            if let Ok(mut g) = inner.db.write() {
                let _ = g.take();
            }
        }
    }
}

impl Engine {
    fn from_inner(inner: Arc<Inner>) -> Engine {
        Engine {
            inner,
            handle: None,
        }
    }

    pub(crate) fn internal_clone(&self) -> Engine {
        Engine {
            inner: self.inner.clone(),
            handle: None,
        }
    }
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

struct Inner {
    paths: AppPaths,
    store: ConfigStore,
    config_warnings: Vec<String>,
    settings: RwLock<AppSettings>,
    proxy_config: Mutex<ProxyConfig>,
    scope_config: RwLock<ScopeConfig>,
    /// The open project's own scope (overrides `scope_config`).
    project_scope: RwLock<Option<ScopeConfig>>,
    scope: RwLock<Scope>,
    /// Scope proxy traffic must be in to be stored (`None`: store all).
    store_filter: crate::sink::StoreFilter,
    backup_state: std::sync::Mutex<project_ops::BackupState>,
    backup_task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    search_generation: AtomicU64,
    project: RwLock<Option<ProjectInfo>>,
    project_lock: Mutex<()>,
    db: CurrentDb,
    proxy: ProxyService,
    listener_states: Arc<RwLock<HashMap<String, ListenerState>>>,
    events: broadcast::Sender<EngineEvent>,
    dropped: Arc<AtomicU64>,
    ca: CaManager,
    browsers: BrowserSessions,
    log: Arc<EventLog>,
    /// Running Repeater sends by tab.
    repeater_inflight: std::sync::Mutex<HashMap<i64, tokio::task::AbortHandle>>,
    /// Running Repeater group sends by group.
    repeater_group_inflight: std::sync::Mutex<HashMap<i64, tokio::task::AbortHandle>>,
    /// Connections kept open for Repeater tabs that reuse them.
    repeater_connections: std::sync::Mutex<HashMap<i64, repeater_ops::KeptConnection>>,
    /// Upstream TLS for Repeater when the local CA is not loaded.
    fallback_tls: std::sync::Mutex<Option<chosproxy_tls::UpstreamTls>>,
    /// The passive analyzer task.
    passive_task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Wakes the passive analyzer early (re-analysis requested).
    passive_wake: tokio::sync::Notify,
    /// What the analyzer's cross-request checks remember.
    passive_cross: std::sync::Mutex<chosproxy_scanner::CrossAnalyzer>,
    /// Tasks running in this session.
    tasks_running: std::sync::Mutex<HashMap<i64, task_ops::RunningTask>>,
    /// Plugin state in this session (status, logs, stop handles).
    plugins: std::sync::Mutex<HashMap<String, plugin_ops::Runtime>>,
    /// ChosOAST for the open project (ADR-0026).
    oast: std::sync::Mutex<Option<oast_ops::Runtime>>,
    oast_status: std::sync::Mutex<oast_ops::OastStatus>,
    /// DOM Inspector session (ADR-0027).
    dom_status: std::sync::Mutex<dom_ops::DomStatus>,
    dom_task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    dom_navigate: std::sync::Mutex<Option<tokio::sync::mpsc::Sender<String>>>,
    /// Starts scheduled tasks when due.
    scheduler_task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// The compiled script library (ADR-0032).
    scripts: RwLock<Arc<script_ops::Library>>,
    /// The last scripted history filter result.
    script_filter_cache: std::sync::Mutex<Option<script_ops::FilterCache>>,
    /// Session rule variables, by rule name (M24).
    session_vars: std::sync::Mutex<HashMap<String, HashMap<String, String>>>,
    /// One login macro at a time.
    session_macro: tokio::sync::Mutex<()>,
    /// Resource pools in force (M26).
    pools: RwLock<Arc<pool_ops::PoolRuntime>>,
    /// Agent tasks running in this session (M30).
    agents: std::sync::Mutex<HashMap<i64, agent_ops::AgentRuntime>>,
    /// The keyring key a master password gave in this session (M31b).
    keyring_key: std::sync::Mutex<Option<[u8; 32]>>,
    /// The operating system's keychain (replaced by a fake in tests).
    os_keychain: RwLock<Arc<dyn crate::keyring::OsKeychain>>,
    /// ChosAI answers being streamed, by the caller's id (M31b).
    ai_streams: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Notify>>>,
    /// The history id a login recording started after (M31b, AI-004).
    login_recording: std::sync::Mutex<Option<i64>>,
    /// Reads this process's memory and CPU (M31 Performance view).
    perf: std::sync::Mutex<sysinfo::System>,
    /// Exchanges stored since the project opened.
    stored: Arc<AtomicU64>,
    /// Unique identifier of this engine session (M31a crash recovery).
    session_id: String,
    /// When the open writable project was opened (ms since epoch).
    session_started_at: std::sync::atomic::AtomicI64,
    /// Path of the session marker written by this engine session.
    session_marker_path: std::sync::Mutex<Option<PathBuf>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        recovery_ops::unregister_session(&self.session_id);
    }
}

/// Minimum interval between notices about failing TLS clients for one host.
const TLS_NOTICE_INTERVAL: Duration = Duration::from_secs(60);

struct StateObserver {
    states: Arc<RwLock<HashMap<String, ListenerState>>>,
    events: broadcast::Sender<EngineEvent>,
    tls_notices: std::sync::Mutex<HashMap<String, Instant>>,
}

/// True at most once per [`TLS_NOTICE_INTERVAL`] for each key.
fn notice_due(seen: &std::sync::Mutex<HashMap<String, Instant>>, key: &str) -> bool {
    let now = Instant::now();
    match seen.lock() {
        Ok(mut seen) => {
            if seen.len() > 1000 {
                seen.clear();
            }
            let due = seen
                .get(key)
                .is_none_or(|t| now.duration_since(*t) >= TLS_NOTICE_INTERVAL);
            if due {
                seen.insert(key.to_string(), now);
            }
            due
        }
        Err(_) => true,
    }
}

impl ProxyObserver for StateObserver {
    fn listener_changed(&self, listener_id: &str, state: &ListenerState) {
        if let Ok(mut s) = self.states.write() {
            s.insert(listener_id.to_string(), state.clone());
        }
        let _ = self.events.send(EngineEvent::ListenerChanged {
            listener_id: listener_id.to_string(),
            state: state.clone(),
        });
    }

    fn intercept_changed(&self, pending: usize) {
        let _ = self.events.send(EngineEvent::InterceptChanged {
            pending: u32::try_from(pending).unwrap_or(u32::MAX),
        });
    }

    fn auth_failure(&self, listener_id: &str, peer: std::net::SocketAddr) {
        if notice_due(&self.tls_notices, &format!("auth:{}", peer.ip())) {
            let _ = self.events.send(EngineEvent::Notice {
                level: NoticeLevel::Warning,
                message: format!(
                    "Listener {listener_id}: {} sent wrong proxy credentials",
                    peer.ip()
                ),
            });
        }
    }

    fn tls_client_failure(&self, host: &str, error: &str) {
        if notice_due(&self.tls_notices, host) {
            let _ = self.events.send(EngineEvent::Notice {
                level: NoticeLevel::Warning,
                message: format!("HTTPS to {host}: {error}"),
            });
        }
    }
}

/// Locks a mutex; a poisoned one (a holder panicked) is used as it is: the
/// engine's shared state stays usable.
fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Reads a file shared between testers (scripts, configurations): at most
/// `max_mb` megabytes of JSON that parses as `T`. `what` names the kind of
/// file in the error ("scripts", "scan configurations").
fn read_shared_file<T: serde::de::DeserializeOwned>(
    path: &std::path::Path,
    max_mb: u64,
    what: &str,
) -> Result<T, AppError> {
    let meta = std::fs::metadata(path)
        .map_err(|e| AppError::Invalid(format!("{}: {e}", path.display())))?;
    if meta.len() > max_mb * 1024 * 1024 {
        return Err(AppError::Invalid(format!(
            "the file is larger than {max_mb} MB"
        )));
    }
    let bytes =
        std::fs::read(path).map_err(|e| AppError::Invalid(format!("{}: {e}", path.display())))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Invalid(format!("not a ChosProxy {what} file: {e}")))
}

/// Writes a file to share as pretty JSON.
fn write_shared_file<T: serde::Serialize>(
    path: &std::path::Path,
    file: &T,
    what: &str,
) -> Result<(), AppError> {
    let json = serde_json::to_vec_pretty(file)
        .map_err(|e| AppError::Invalid(format!("cannot write {what}: {e}")))?;
    std::fs::write(path, json).map_err(|e| AppError::Invalid(format!("{}: {e}", path.display())))
}

fn read<T: Clone>(lock: &RwLock<T>) -> T {
    match lock.read() {
        Ok(g) => g.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

fn write<T>(lock: &RwLock<T>, value: T) {
    match lock.write() {
        Ok(mut g) => *g = value,
        Err(poisoned) => *poisoned.into_inner() = value,
    }
}

fn limits_of(settings: &AppSettings) -> CaptureLimits {
    CaptureLimits {
        max_message_bytes: usize::try_from(settings.capture.max_message_bytes)
            .unwrap_or(usize::MAX),
    }
}

impl Engine {
    /// Creates the engine: prepares directories and loads configuration.
    /// No project is open and no listener runs until a project is opened.
    pub fn new(paths: AppPaths) -> Result<Engine, AppError> {
        paths.ensure()?;
        let store = ConfigStore::new(&paths.config_dir);
        let settings = store.load::<AppSettings>();
        let proxy_config = store.load::<ProxyConfig>();
        let scope_config = store.load::<ScopeConfig>();
        let network = store.load::<NetworkConfig>();
        let scripts = store.load::<ScriptsConfig>();
        let library = Arc::new(script_ops::Library::build(scripts.value));
        chosproxy_request::set_network(network.value);
        // `load` only returns configurations that validate, so this compiles.
        let scope = Scope::compile(&scope_config.value).unwrap_or_default();
        let store_filter: crate::sink::StoreFilter = Arc::new(RwLock::new(
            (scope_config.value.skip_out_of_scope_proxy && !scope.is_empty())
                .then(|| scope.clone()),
        ));
        let config_warnings: Vec<String> = [
            settings.warning,
            proxy_config.warning,
            scope_config.warning,
            network.warning,
            scripts.warning,
        ]
        .into_iter()
        .flatten()
        .collect();

        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let log = Arc::new(EventLog::persistent(
            &paths.log_dir.join(event_log::EVENT_LOG_FILE),
        ));
        event_log::spawn_recorder(log.clone(), events.subscribe());
        let db: CurrentDb = Arc::default();
        let dropped = Arc::new(AtomicU64::new(0));
        let listener_states: Arc<RwLock<HashMap<String, ListenerState>>> = Arc::default();
        let ca = CaManager::init(&paths.data_dir.join("ca"));
        let mut config_warnings = config_warnings;
        if let Some(e) = ca.status().error {
            config_warnings.push(format!(
                "HTTPS interception is disabled: {e}. Regenerate the CA in Proxy → HTTPS."
            ));
        }
        for w in &config_warnings {
            log.record(NoticeLevel::Warning, LogCategory::System, w.clone());
        }
        let browsers = BrowserSessions::new(paths.temp_dir.clone(), events.clone());
        let proxy = ProxyService::new(
            Arc::new(ProjectSink {
                db: db.clone(),
                dropped: dropped.clone(),
                events: events.clone(),
                filter: store_filter.clone(),
                skipped_ws: std::sync::Mutex::new(std::collections::HashSet::new()),
            }),
            Arc::new(StateObserver {
                states: listener_states.clone(),
                events: events.clone(),
                tls_notices: std::sync::Mutex::new(HashMap::new()),
            }),
            limits_of(&settings.value),
        );
        proxy.set_tls(ca.tls_context());
        proxy.set_tls_policy(proxy_config.value.tls.clone());
        let resolver = |id: &str| library.usable(id, ScriptKind::Rewrite);
        if let Err(e) = proxy.set_match_replace_with(&proxy_config.value.match_replace, &resolver) {
            log.record(
                NoticeLevel::Warning,
                LogCategory::System,
                format!("Match & Replace rules not loaded: {e}"),
            );
        }
        proxy
            .intercept()
            .set_rules(proxy_config.value.intercept.clone());
        proxy.intercept().set_scope(scope.clone());
        let pools =
            pool_ops::PoolRuntime::build(&store.load::<pool_ops::PoolsConfig>().value, None);
        let session_id = recovery_ops::register_session();
        let inner = Arc::new(Inner {
            paths,
            store,
            config_warnings,
            settings: RwLock::new(settings.value),
            proxy_config: Mutex::new(proxy_config.value),
            scope_config: RwLock::new(scope_config.value),
            project_scope: RwLock::new(None),
            scope: RwLock::new(scope),
            store_filter,
            backup_state: std::sync::Mutex::default(),
            backup_task: std::sync::Mutex::new(None),
            search_generation: AtomicU64::new(0),
            project: RwLock::new(None),
            project_lock: Mutex::new(()),
            db,
            proxy,
            listener_states,
            events,
            dropped,
            ca,
            browsers,
            log,
            repeater_inflight: std::sync::Mutex::default(),
            repeater_group_inflight: std::sync::Mutex::default(),
            repeater_connections: std::sync::Mutex::default(),
            fallback_tls: std::sync::Mutex::new(None),
            passive_task: std::sync::Mutex::new(None),
            passive_wake: tokio::sync::Notify::new(),
            passive_cross: std::sync::Mutex::default(),
            tasks_running: std::sync::Mutex::default(),
            plugins: std::sync::Mutex::default(),
            oast: std::sync::Mutex::new(None),
            oast_status: std::sync::Mutex::default(),
            dom_status: std::sync::Mutex::default(),
            dom_task: std::sync::Mutex::new(None),
            dom_navigate: std::sync::Mutex::new(None),
            scheduler_task: std::sync::Mutex::new(None),
            scripts: RwLock::new(library),
            script_filter_cache: std::sync::Mutex::new(None),
            session_vars: std::sync::Mutex::new(HashMap::new()),
            session_macro: tokio::sync::Mutex::new(()),
            pools: RwLock::new(Arc::new(pools)),
            agents: std::sync::Mutex::new(HashMap::new()),
            keyring_key: std::sync::Mutex::new(None),
            os_keychain: RwLock::new(Arc::new(crate::keyring::SystemKeychain)),
            ai_streams: std::sync::Mutex::new(HashMap::new()),
            login_recording: std::sync::Mutex::new(None),
            perf: std::sync::Mutex::new(sysinfo::System::new()),
            stored: Arc::new(AtomicU64::new(0)),
            session_id: session_id.clone(),
            session_started_at: std::sync::atomic::AtomicI64::new(0),
            session_marker_path: std::sync::Mutex::new(None),
        });
        let handle = Arc::new(SessionHandle {
            session_id,
            inner: Arc::downgrade(&inner),
        });
        let engine = Engine {
            inner,
            handle: Some(handle),
        };
        engine.reload_platform_auth();
        engine
            .inner
            .proxy
            .set_request_hook(Some(Arc::new(engine.plugin_hooks())));
        Ok(engine)
    }

    /// Subscribes to engine events.
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.events.subscribe()
    }

    fn notice(&self, level: NoticeLevel, message: String) {
        let _ = self
            .inner
            .events
            .send(EngineEvent::Notice { level, message });
    }

    /// Static application information.
    pub fn info(&self) -> AppInfo {
        let p = &self.inner.paths;
        AppInfo {
            product_name: brand().product_name.clone(),
            tagline: brand().tagline.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            config_dir: p.config_dir.display().to_string(),
            data_dir: p.data_dir.display().to_string(),
            log_dir: p.log_dir.display().to_string(),
            config_warnings: self.inner.config_warnings.clone(),
        }
    }

    // ----------------------------------------------------------------------
    // Settings

    /// Current application settings.
    pub fn settings(&self) -> AppSettings {
        read(&self.inner.settings)
    }

    /// Validates, persists and applies application settings.
    pub fn save_settings(&self, settings: AppSettings) -> Result<AppSettings, AppError> {
        self.inner.store.save(&settings)?;
        self.inner.proxy.set_limits(limits_of(&settings));
        write(&self.inner.settings, settings.clone());
        Ok(settings)
    }

    /// Current proxy configuration.
    pub async fn proxy_config(&self) -> ProxyConfig {
        self.inner.proxy_config.lock().await.clone()
    }

    /// Validates and persists the proxy configuration, then reconciles running
    /// listeners: removed ones stop, changed ones restart with new settings.
    pub async fn save_proxy_config(
        &self,
        config: ProxyConfig,
    ) -> Result<Vec<ListenerStatus>, AppError> {
        let mut current = self.inner.proxy_config.lock().await;
        self.inner.store.save(&config)?;
        self.inner.proxy.set_tls_policy(config.tls.clone());
        let library = self.library();
        let resolver = |id: &str| library.usable(id, ScriptKind::Rewrite);
        self.inner
            .proxy
            .set_match_replace_with(&config.match_replace, &resolver)
            .map_err(AppError::Invalid)?;
        self.inner
            .proxy
            .intercept()
            .set_rules(config.intercept.clone());
        let running = self.inner.proxy.running().await;
        for (id, _) in running {
            let old = current.listener(&id).cloned();
            match config.listener(&id) {
                None => {
                    self.inner.proxy.stop(&id).await;
                    if let Ok(mut s) = self.inner.listener_states.write() {
                        s.remove(&id);
                    }
                }
                Some(new) if Some(new) != old.as_ref() => {
                    self.inner.proxy.stop(&id).await;
                    if let Err(e) = self.start_proxy_listener(new).await {
                        self.notice(NoticeLevel::Error, format!("Listener {id}: {e}"));
                    }
                }
                Some(_) => {}
            }
        }
        *current = config;
        drop(current);
        self.audit(
            "proxy.config.changed",
            "listeners, HTTPS policy or Intercept rules saved",
        );
        Ok(self.listeners().await)
    }

    /// Requires (`Some((user, password))`) or stops requiring proxy
    /// authentication on a listener. The password is stored only as a salted
    /// hash; a running listener restarts with the new setting.
    pub async fn set_listener_auth(
        &self,
        id: &str,
        credentials: Option<(String, String)>,
    ) -> Result<Vec<ListenerStatus>, AppError> {
        let auth = match credentials {
            Some((user, password)) => Some(
                tokio::task::spawn_blocking(move || ProxyAuth::new(user.trim(), &password))
                    .await?
                    .map_err(AppError::Invalid)?,
            ),
            None => None,
        };
        let mut config = self.proxy_config().await;
        let listener = config
            .listeners
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or_else(|| AppError::UnknownListener(id.to_string()))?;
        let detail = match &auth {
            Some(a) => format!("listener {id} requires user {}", a.username),
            None => format!("listener {id} no longer requires credentials"),
        };
        listener.auth = auth;
        let result = self.save_proxy_config(config).await;
        if result.is_ok() {
            self.audit("listener.auth.changed", &detail);
        }
        result
    }

    // ----------------------------------------------------------------------
    // Listeners

    /// Configured listeners with their live state.
    pub async fn listeners(&self) -> Vec<ListenerStatus> {
        let config = self.inner.proxy_config.lock().await.clone();
        let states = read(&self.inner.listener_states);
        config
            .listeners
            .into_iter()
            .map(|c| ListenerStatus {
                state: states.get(&c.id).cloned().unwrap_or(ListenerState::Stopped),
                config: c,
            })
            .collect()
    }

    async fn listener_config(&self, id: &str) -> Result<ListenerConfig, AppError> {
        self.inner
            .proxy_config
            .lock()
            .await
            .listener(id)
            .cloned()
            .ok_or_else(|| AppError::UnknownListener(id.to_string()))
    }

    /// Starts a configured listener. Requires an open writable project, so
    /// captured traffic always has somewhere to go.
    pub async fn start_listener(&self, id: &str) -> Result<(), AppError> {
        self.ensure_writable()?;
        let cfg = self.listener_config(id).await?;
        self.start_proxy_listener(&cfg).await
    }

    /// Starts a listener with the certificate its settings choose. A
    /// certificate that cannot be loaded stops the listener from starting
    /// (it would otherwise present a certificate the tester did not pick).
    async fn start_proxy_listener(
        &self,
        cfg: &chosproxy_core::config::ListenerConfig,
    ) -> Result<(), AppError> {
        let certificate = cfg.certificate.clone();
        let id = cfg.id.clone();
        let engine = self.clone();
        let interceptor = tokio::task::spawn_blocking(move || {
            engine.inner.ca.listener_interceptor(&id, &certificate)
        })
        .await?
        .map_err(|e| AppError::Invalid(format!("the listener's certificate: {e}")))?;
        self.inner.proxy.set_listener_tls(&cfg.id, interceptor);
        self.inner.proxy.start(cfg).await?;
        Ok(())
    }

    /// Stops a listener (no-op if it is not running).
    pub async fn stop_listener(&self, id: &str) -> Result<(), AppError> {
        self.listener_config(id).await?;
        self.inner.proxy.stop(id).await;
        Ok(())
    }

    /// Stops then starts a listener.
    pub async fn restart_listener(&self, id: &str) -> Result<(), AppError> {
        self.stop_listener(id).await?;
        self.start_listener(id).await
    }

    // ----------------------------------------------------------------------
    // Projects

    /// The open project, if any.
    pub fn project(&self) -> Option<ProjectInfo> {
        read(&self.inner.project)
    }

    /// Creates and opens a temporary project (deleted when closed).
    pub async fn new_temporary_project(&self) -> Result<ProjectInfo, AppError> {
        self.new_temporary_project_with(false).await
    }

    /// Creates and opens a temporary project; with `encrypt`, it is
    /// encrypted with a random key kept only in memory (M31).
    pub async fn new_temporary_project_with(&self, encrypt: bool) -> Result<ProjectInfo, AppError> {
        let key = if encrypt {
            Some(ProjectKey::random()?)
        } else {
            None
        };
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "temp-{}-{}-{}.{PROJECT_EXTENSION}",
            std::process::id(),
            chosproxy_core::time::now_ms(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = self.inner.paths.temp_dir.join(name);
        self.activate(
            path,
            ProjectKind::Temporary,
            "Temporary project".into(),
            key,
        )
        .await
    }

    /// Creates a new persistent project file and opens it.
    pub async fn create_project(&self, path: &Path) -> Result<ProjectInfo, AppError> {
        self.create_project_with(path, None).await
    }

    /// Creates a new project file, encrypted with `password` when given
    /// (M31). The password is not stored anywhere.
    pub async fn create_project_with(
        &self,
        path: &Path,
        password: Option<&str>,
    ) -> Result<ProjectInfo, AppError> {
        let key = password.map(ProjectKey::password).transpose()?;
        let path = with_extension(path);
        if path.exists() {
            return Err(AppError::Invalid(format!(
                "{} already exists",
                path.display()
            )));
        }
        match path.parent() {
            Some(dir) if dir.as_os_str().is_empty() || dir.is_dir() => {}
            _ => {
                return Err(AppError::Invalid(format!(
                    "folder of {} does not exist",
                    path.display()
                )));
            }
        }
        let name = display_name(&path);
        let encrypted = key.is_some();
        let info = self
            .activate(path, ProjectKind::Persistent, name, key)
            .await?;
        if encrypted {
            self.audit("project.encrypted", "created encrypted");
        }
        Ok(info)
    }

    /// Opens an existing persistent project file.
    pub async fn open_project(&self, path: &Path) -> Result<ProjectInfo, AppError> {
        self.open_project_with(path, None).await
    }

    /// Opens a project file; an encrypted one needs its password.
    pub async fn open_project_with(
        &self,
        path: &Path,
        password: Option<&str>,
    ) -> Result<ProjectInfo, AppError> {
        let key = password
            .filter(|p| !p.is_empty())
            .map(ProjectKey::password)
            .transpose()?;
        if !path.is_file() {
            return Err(AppError::Invalid(format!(
                "{} is not a file",
                path.display()
            )));
        }
        let name = display_name(path);
        self.activate(path.to_path_buf(), ProjectKind::Persistent, name, key)
            .await
    }

    pub(crate) async fn activate(
        &self,
        path: PathBuf,
        kind: ProjectKind,
        name: String,
        key: Option<ProjectKey>,
    ) -> Result<ProjectInfo, AppError> {
        self.activate_with(path, kind, name, key, false, false)
            .await
    }

    pub(crate) async fn activate_with(
        &self,
        path: PathBuf,
        kind: ProjectKind,
        name: String,
        key: Option<ProjectKey>,
        read_only: bool,
        recovering: bool,
    ) -> Result<ProjectInfo, AppError> {
        // A wrong or missing password or damaged file is found before the open
        // project is closed, so a typo or corrupt file does not close the
        // tester's work or discard a crash recovery marker.
        let mut key = key;
        if path.is_file() {
            let (check, k) = (path.clone(), key.clone());
            let encrypted = tokio::task::spawn_blocking(move || -> Result<bool, AppError> {
                let encrypted = chosproxy_storage::is_encrypted_file(&check)?;
                if encrypted && k.is_none() {
                    return Err(chosproxy_storage::StorageError::Encrypted.into());
                }
                let problems = chosproxy_storage::ProjectDb::quick_check(
                    &check,
                    if encrypted { k.as_ref() } else { None },
                )?;
                if !problems.is_empty() {
                    return Err(chosproxy_storage::StorageError::InvalidQuery(format!(
                        "the project file failed integrity check: {}",
                        problems.join("; ")
                    ))
                    .into());
                }
                if read_only || recovering {
                    let _ = chosproxy_storage::inspect_project_file_with(
                        &check,
                        if encrypted { k.as_ref() } else { None },
                    )?;
                }
                Ok(encrypted)
            })
            .await??;
            // A password typed for a file that is not encrypted is ignored.
            if !encrypted {
                key = None;
            }
        }
        let encrypted = key.is_some();
        let _guard = self.inner.project_lock.lock().await;
        self.close_locked().await?;

        let events = self.inner.events.clone();
        let stored = self.inner.stored.clone();
        stored.store(0, Ordering::Relaxed);
        // A login recording belongs to the project it started in.
        *lock(&self.inner.login_recording) = None;
        let hook = Arc::new(move |b: InsertBatch| {
            stored.fetch_add(u64::from(b.count), Ordering::Relaxed);
            if b.count > 0 {
                let _ = events.send(EngineEvent::HistoryAppended {
                    last_id: b.last_id,
                    added: b.count,
                });
            }
            if b.ws_events > 0 {
                let _ = events.send(EngineEvent::WebSocketsChanged);
            }
        });
        let open_path = path.clone();
        let db = tokio::task::spawn_blocking(move || {
            ProjectDb::open(
                &open_path,
                OpenOptions {
                    on_insert: (!read_only).then_some(hook),
                    read_only,
                    key,
                    ..OpenOptions::default()
                },
            )
        })
        .await??;
        if !read_only {
            // Tasks left unfinished by an earlier session cannot continue.
            let db = db.clone();
            let marked = tokio::task::spawn_blocking(move || {
                if recovering {
                    db.tasks_interrupted_crash()
                } else {
                    db.tasks_interrupted()
                }
            })
            .await?;
            if let Err(e) = marked {
                tracing::warn!(error = %e, "could not mark interrupted tasks");
            }
        }
        write(&self.inner.db, Some(db.clone()));
        self.load_project_layer(&db).await;
        self.passive_reset_cross();
        if !read_only {
            self.resume_live_tasks(&db).await;
            self.ensure_passive_task();
            self.ensure_scheduler();
            self.start_plugins().await;
            self.start_oast().await;
        }
        let info = ProjectInfo {
            name,
            path: path.display().to_string(),
            kind,
            encrypted,
            read_only,
        };
        write(&self.inner.project, Some(info.clone()));
        if !read_only {
            self.inner
                .session_started_at
                .store(chosproxy_core::time::now_ms(), Ordering::Relaxed);
            self.write_session_marker();
        }
        self.inner.dropped.store(0, Ordering::Relaxed);
        tracing::info!(path = %path.display(), ?kind, read_only, "project opened");
        self.inner.log.record(
            NoticeLevel::Info,
            LogCategory::Project,
            format!(
                "Opened {} ({}){}",
                info.name,
                info.path,
                if read_only { " [read-only]" } else { "" }
            ),
        );
        let _ = self.inner.events.send(EngineEvent::ProjectChanged);

        if !read_only {
            let config = self.inner.proxy_config.lock().await.clone();
            for l in config.listeners.iter().filter(|l| l.start_on_open) {
                if let Err(e) = self.start_proxy_listener(l).await {
                    self.notice(NoticeLevel::Error, format!("Listener {}: {e}", l.id));
                }
            }
            if kind == ProjectKind::Persistent {
                self.start_backups(&db).await;
            }
        }
        Ok(info)
    }

    /// Closes the open project: stops listeners, flushes pending writes and
    /// deletes the file if the project was temporary (and not opened read-only).
    pub async fn close_project(&self) -> Result<(), AppError> {
        let _guard = self.inner.project_lock.lock().await;
        self.close_locked().await?;
        let _ = self.inner.events.send(EngineEvent::ProjectChanged);
        Ok(())
    }

    async fn close_locked(&self) -> Result<(), AppError> {
        self.stop_backups();
        self.repeater_cancel_all();
        if let Some(db) = read(&self.inner.db) {
            self.tasks_stop_all(&db).await;
        }
        self.stop_plugins().await;
        self.stop_oast();
        // Release anything paused and start the next project with Intercept off.
        self.inner.proxy.intercept().set_enabled(false);
        self.inner.proxy.stop_all().await;
        self.remove_own_session_marker();
        self.inner.session_started_at.store(0, Ordering::Relaxed);
        let db = match self.inner.db.write() {
            Ok(mut g) => g.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        let info = match self.inner.project.write() {
            Ok(mut g) => g.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(db) = db {
            tokio::task::spawn_blocking(move || {
                let flushed = db.flush();
                drop(db);
                flushed
            })
            .await??;
        }
        if let Some(info) = info {
            tracing::info!(path = %info.path, "project closed");
            self.inner.log.record(
                NoticeLevel::Info,
                LogCategory::Project,
                format!("Closed {}", info.name),
            );
            if info.kind == ProjectKind::Temporary && !info.read_only {
                for suffix in ["", "-wal", "-shm"] {
                    let p = PathBuf::from(format!("{}{suffix}", info.path));
                    if let Err(e) = std::fs::remove_file(&p)
                        && e.kind() != std::io::ErrorKind::NotFound
                    {
                        tracing::warn!(path = %p.display(), error = %e, "could not delete temporary project file");
                    }
                }
            }
        }
        if read(&self.inner.project_scope).is_some() {
            write(&self.inner.project_scope, None);
            self.apply_scope();
        }
        Ok(())
    }

    // ----------------------------------------------------------------------
    // Intercept

    /// Intercept switch and paused messages.
    pub fn intercept_state(&self) -> InterceptState {
        intercept_view::state(&self.inner.proxy.intercept())
    }

    /// Switches Intercept. Turning it off forwards every paused message
    /// unmodified. In read-only project mode, Intercept stays off.
    pub fn set_intercept(&self, enabled: bool) -> InterceptState {
        let queue = self.inner.proxy.intercept();
        let allow = enabled && !read(&self.inner.project).is_some_and(|p| p.read_only);
        queue.set_enabled(allow);
        intercept_view::state(&queue)
    }

    /// A paused message prepared for editing.
    pub fn held_message(&self, id: u64) -> Result<HeldDetail, AppError> {
        intercept_view::get(&self.inner.proxy.intercept(), id)
    }

    /// Decodes a paused message's chunked/compressed body so it can be edited.
    pub fn unpack_held(&self, id: u64) -> Result<HeldDetail, AppError> {
        intercept_view::unpack(&self.inner.proxy.intercept(), id)
    }

    /// Forwards a paused message, optionally edited. For requests,
    /// `intercept_response` also pauses the matching response.
    pub fn forward_held(
        &self,
        id: u64,
        edit: Option<HeldEdit>,
        intercept_response: bool,
    ) -> Result<(), AppError> {
        intercept_view::forward(&self.inner.proxy.intercept(), id, edit, intercept_response)
    }

    /// Drops a paused message.
    pub fn drop_held(&self, id: u64) -> Result<(), AppError> {
        intercept_view::drop_message(&self.inner.proxy.intercept(), id)
    }

    /// Forwards every paused message as currently shown.
    pub fn forward_all_held(&self) {
        self.inner.proxy.intercept().forward_all();
    }

    // ----------------------------------------------------------------------
    // HTTPS: local CA

    /// State of the local CA.
    pub fn ca_status(&self) -> CaStatus {
        self.inner.ca.status()
    }

    /// Writes the public CA certificate to `path`; returns the final path.
    pub fn export_ca(&self, path: &Path, format: CertFormat) -> Result<PathBuf, AppError> {
        self.inner.ca.export(path, format)
    }

    /// Replaces the local CA with an imported certificate and PKCS#8 key
    /// (PEM files); browsers must trust the imported certificate.
    pub fn import_ca(&self, cert: &Path, key: &Path) -> Result<CaStatus, AppError> {
        self.inner.ca.import(cert, key)?;
        self.inner.proxy.set_tls(self.inner.ca.tls_context());
        let _ = self.inner.events.send(EngineEvent::CaChanged);
        self.audit(
            "ca.imported",
            "the local CA was replaced by an imported one",
        );
        self.notice(
            NoticeLevel::Info,
            "The imported CA now signs intercepted HTTPS. The previous one is kept as \
             ca-cert.previous.pem / ca-key.previous.pem."
                .into(),
        );
        Ok(self.inner.ca.status())
    }

    /// Generates a new CA. Previously trusting browsers must import the new
    /// certificate; the old one stops being used immediately.
    pub fn regenerate_ca(&self) -> Result<CaStatus, AppError> {
        self.inner.ca.regenerate()?;
        self.inner.proxy.set_tls(self.inner.ca.tls_context());
        let _ = self.inner.events.send(EngineEvent::CaChanged);
        self.notice(
            NoticeLevel::Info,
            "A new local CA was generated. Re-import it in any browser that trusted the old one."
                .into(),
        );
        Ok(self.inner.ca.status())
    }

    // ----------------------------------------------------------------------
    // Lab browser

    /// Installed browsers usable as lab browser.
    pub fn browsers(&self) -> Vec<BrowserInfo> {
        browser::detect()
    }

    /// State of the lab browser.
    pub async fn browser_status(&self) -> BrowserStatus {
        self.inner.browsers.status().await
    }

    /// Starts an isolated lab browser through the first running listener.
    /// `browser_id` selects one of [`Engine::browsers`] (default: the first).
    pub async fn launch_browser(
        &self,
        browser_id: Option<String>,
        url: Option<String>,
    ) -> Result<BrowserStatus, AppError> {
        self.launch_browser_with(browser_id, url, false).await
    }

    /// Launches the lab browser, with the DOM Inspector when asked
    /// (Chromium-family browsers only, ADR-0027).
    pub async fn launch_browser_with(
        &self,
        browser_id: Option<String>,
        url: Option<String>,
        dom_inspector: bool,
    ) -> Result<BrowserStatus, AppError> {
        let status = self
            .launch_lab_browser(browser_id, url.clone(), dom_inspector)
            .await?;
        if dom_inspector && let Some(profile) = status.profile_dir.clone() {
            let url = url
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| "http://chosproxy/".to_string());
            self.start_dom_inspector(std::path::PathBuf::from(profile), url);
        }
        Ok(status)
    }

    /// Launches the lab browser through the first running listener, with
    /// a loopback DevTools endpoint when `devtools` (Chromium family).
    pub(crate) async fn launch_lab_browser(
        &self,
        browser_id: Option<String>,
        url: Option<String>,
        devtools: bool,
    ) -> Result<BrowserStatus, AppError> {
        self.ensure_writable()?;
        let mut proxy_addr = self
            .inner
            .proxy
            .running()
            .await
            .into_iter()
            .map(|(_, addr)| addr)
            .next()
            .ok_or_else(|| AppError::Invalid("start a proxy listener first".into()))?;
        if proxy_addr.ip().is_unspecified() {
            proxy_addr.set_ip(std::net::Ipv4Addr::LOCALHOST.into());
        }
        let available = browser::detect();
        let chosen = match &browser_id {
            Some(id) => available.into_iter().find(|b| &b.id == id),
            None => available.into_iter().next(),
        }
        .ok_or_else(|| {
            AppError::Invalid(
                "no supported browser found; install Chromium (sudo apt install chromium) or                  Firefox"
                    .into(),
            )
        })?;
        let (spki_pin, ca_pem) = match self.inner.ca.interceptor() {
            Some(i) => (
                i.leaf_spki_pin().to_string(),
                i.authority().cert_pem().to_string(),
            ),
            None => (String::new(), String::new()),
        };
        let url = url
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| "http://chosproxy/".to_string());
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(AppError::Invalid(
                "the start URL must begin with http:// or https://".into(),
            ));
        }
        let mut status = self
            .inner
            .browsers
            .launch(
                chosen,
                proxy_addr,
                spki_pin.clone(),
                ca_pem,
                url.clone(),
                devtools,
            )
            .await?;
        if spki_pin.is_empty() {
            status.warning.get_or_insert_with(|| {
                "The local CA is unavailable: HTTPS pages will fail in the lab browser.".into()
            });
        }
        Ok(status)
    }

    /// Closes the lab browser and deletes its temporary profile.
    pub async fn close_browser(&self) {
        self.inner.browsers.close().await;
    }

    /// Closes the project and stops everything. Call before exiting.
    pub async fn shutdown(&self) {
        self.inner.browsers.close().await;
        if let Err(e) = self.close_project().await {
            tracing::error!(error = %e, "error while shutting down");
        }
        self.remove_own_session_marker();
        recovery_ops::unregister_session(&self.inner.session_id);
        self.temp_cleanup_on_exit();
    }

    // ----------------------------------------------------------------------
    // Queries

    fn db(&self) -> Result<ProjectDb, AppError> {
        read(&self.inner.db).ok_or(AppError::NoProject)
    }

    /// Saves the current site map as a named snapshot.
    pub async fn snapshot_save(
        &self,
        name: String,
    ) -> Result<chosproxy_storage::SiteSnapshotInfo, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || {
            let map = db.site_map(&HistoryQuery::default())?;
            db.snapshot_save(name, map.entries)
        })
        .await??)
    }

    /// Saved snapshots, newest first.
    pub async fn snapshots(&self) -> Result<Vec<chosproxy_storage::SiteSnapshotInfo>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.snapshots()).await??)
    }

    /// Deletes a snapshot.
    pub async fn snapshot_delete(&self, id: i64) -> Result<(), AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.snapshot_delete(id)).await??)
    }

    /// Differences from snapshot `older` to snapshot `newer` (the current
    /// site map when `newer` is none).
    pub async fn snapshot_compare(
        &self,
        older: i64,
        newer: Option<i64>,
    ) -> Result<Vec<chosproxy_storage::SiteDiffEntry>, AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || {
            let missing = |id| AppError::Invalid(format!("no snapshot {id}"));
            let before = db.snapshot_entries(older)?.ok_or_else(|| missing(older))?;
            let after = match newer {
                Some(id) => db.snapshot_entries(id)?.ok_or_else(|| missing(id))?,
                None => db.site_map(&HistoryQuery::default())?.entries,
            };
            Ok(chosproxy_storage::diff_site_maps(&before, &after))
        })
        .await?
    }

    /// What one site exposes, from its stored traffic (target analyzer).
    pub async fn target_analysis(
        &self,
        site: chosproxy_storage::SiteFilter,
    ) -> Result<crate::analysis::TargetAnalysis, AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || {
            let query = HistoryQuery {
                site: Some(site),
                ..HistoryQuery::default()
            };
            let ids = db.matching_ids(&query, crate::analysis::MAX_ANALYZED)?;
            let mut exchanges = Vec::with_capacity(ids.len());
            for chunk in ids.chunks(256) {
                exchanges.extend(db.exchanges_bulk(chunk)?);
            }
            Ok(crate::analysis::analyze(&exchanges))
        })
        .await?
    }

    /// A page of HTTP history.
    pub async fn history_page(&self, query: HistoryQuery) -> Result<HistoryPage, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.history_page(&query)).await??)
    }

    /// Full exchange prepared for display.
    pub async fn exchange_detail(&self, id: i64) -> Result<ExchangeDetail, AppError> {
        let db = self.db()?;
        let stored = tokio::task::spawn_blocking(move || db.exchange(id))
            .await??
            .ok_or(AppError::UnknownExchange(id))?;
        let row = stored.row;
        let request = message_view(
            &stored.request,
            Some(row.request_len),
            row.request_truncated,
        );
        let response = stored
            .response
            .as_deref()
            .map(|r| message_view(r, row.response_len, row.response_truncated));
        let original_request = stored
            .original_request
            .as_deref()
            .map(|r| message_view(r, Some(r.len() as u64), false));
        let original_response = stored
            .original_response
            .as_deref()
            .map(|r| message_view(r, Some(r.len() as u64), false));
        Ok(ExchangeDetail {
            row,
            client_addr: stored.client_addr,
            listener: stored.listener,
            reason: stored.reason,
            request,
            response,
            original_request,
            original_response,
        })
    }

    /// Events of this session, oldest first.
    pub fn event_log(&self) -> Vec<LogEntry> {
        self.inner.log.entries()
    }

    /// Empties the event log.
    pub fn clear_event_log(&self) {
        self.inner.log.clear();
    }

    // ----------------------------------------------------------------------
    // Target: scope, site map, annotations, search

    /// Current scope rules.
    pub fn scope_config(&self) -> ScopeConfig {
        self.effective_scope()
    }

    /// Validates, persists and applies scope rules everywhere (history
    /// columns and filters, Intercept, and every later scope check).
    /// The rules go to the open project when it has its own scope.
    pub fn save_scope_config(&self, config: ScopeConfig) -> Result<ScopeConfig, AppError> {
        self.store_scope(config.clone())?;
        Ok(config)
    }

    /// Whether an absolute URL is in scope, and which rules decided it.
    pub fn check_scope(&self, url: &str) -> Result<ScopeDecision, AppError> {
        read(&self.inner.scope)
            .check_url(url)
            .map_err(AppError::Invalid)
    }

    /// Adds a rule for the URL's protocol, host and port to the include
    /// (`exclude = false`) or exclude list. `path` keeps the URL's path as a
    /// prefix; otherwise the whole origin is covered.
    pub fn add_url_to_scope(
        &self,
        url: &str,
        exclude: bool,
        path: bool,
    ) -> Result<ScopeConfig, AppError> {
        let parts = UrlParts::parse(url).map_err(AppError::Invalid)?;
        let rule = ScopeRule {
            protocol: if parts.scheme == "https" {
                ScopeProtocol::Https
            } else {
                ScopeProtocol::Http
            },
            host: parts.host.clone(),
            port: Some(parts.port),
            path: if path {
                parts
                    .path
                    .split(['?', '#'])
                    .next()
                    .unwrap_or("/")
                    .to_string()
            } else {
                String::new()
            },
            ..ScopeRule::default()
        };
        let mut config = self.scope_config();
        let list = if exclude {
            &mut config.exclude
        } else {
            &mut config.include
        };
        if !list.contains(&rule) {
            list.push(rule);
        }
        self.save_scope_config(config)
    }

    /// The site map built from stored traffic matching `query`'s filter.
    pub async fn site_map(&self, query: HistoryQuery) -> Result<SiteMap, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.site_map(&query)).await??)
    }

    /// Sets (or clears, with `None`) the mark on a site map node.
    pub async fn set_site_mark(
        &self,
        scheme: String,
        host: String,
        port: u16,
        path: String,
        mark: Option<SiteMark>,
    ) -> Result<(), AppError> {
        let db = self.db()?;
        Ok(
            tokio::task::spawn_blocking(move || db.set_site_mark(scheme, host, port, path, mark))
                .await??,
        )
    }

    /// Replaces the note and highlight of an exchange.
    pub async fn annotate_exchange(
        &self,
        id: i64,
        notes: Option<String>,
        highlight: Option<Highlight>,
    ) -> Result<(), AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.annotate(id, notes, highlight))
            .await?
            .map_err(|e| match e {
                chosproxy_storage::StorageError::NotFound(id) => AppError::UnknownExchange(id),
                e => e.into(),
            })
    }

    /// Searches stored traffic. Starting a search stops the previous one.
    pub async fn search(&self, query: SearchQuery) -> Result<SearchResults, AppError> {
        let db = self.db()?;
        let mine = self.inner.search_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let engine = self.clone();
        tokio::task::spawn_blocking(move || {
            search::run(&db, &query, &engine.inner.search_generation, mine)
        })
        .await?
    }

    // ----------------------------------------------------------------------
    // Utilities: Decoder, Organizer

    /// Runs a Decoder chain on Base64 input (no project needed).
    pub fn decode(
        &self,
        input_base64: &str,
        ops: &[Operation],
    ) -> Result<Vec<DecodeStep>, AppError> {
        decode_view::decode(input_base64, ops)
    }

    /// Proposes and runs a decoding chain for Base64 input.
    pub fn smart_decode(&self, input_base64: &str) -> Result<SmartDecode, AppError> {
        decode_view::smart(input_base64)
    }

    /// Copies a history entry into the Organizer.
    pub async fn organizer_add(&self, exchange_id: i64) -> Result<OrganizerId, AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.organizer_add(exchange_id))
            .await?
            .map_err(|e| match e {
                chosproxy_storage::StorageError::NotFound(id) => AppError::UnknownExchange(id),
                e => e.into(),
            })
    }

    /// Organizer items, newest first.
    pub async fn organizer_list(&self) -> Result<Vec<OrganizerItem>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.organizer_list()).await??)
    }

    /// One Organizer item with its messages.
    pub async fn organizer_detail(&self, id: OrganizerId) -> Result<OrganizerDetail, AppError> {
        let db = self.db()?;
        let stored = tokio::task::spawn_blocking(move || db.organizer_item(id))
            .await??
            .ok_or_else(|| AppError::Invalid(format!("organizer item {id} not found")))?;
        Ok(OrganizerDetail {
            request: message_view(&stored.request, Some(stored.request.len() as u64), false),
            response: stored
                .response
                .as_deref()
                .map(|r| message_view(r, Some(r.len() as u64), false)),
            item: stored.item,
        })
    }

    /// Saves an Organizer item's name, notes, tags, status and severity.
    pub async fn organizer_update(
        &self,
        id: OrganizerId,
        update: OrganizerUpdate,
    ) -> Result<(), AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.organizer_update(id, update)).await??)
    }

    /// Deletes an Organizer item (the history entry is kept).
    pub async fn organizer_delete(&self, id: OrganizerId) -> Result<(), AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.organizer_delete(id)).await??;
        self.audit("organizer.item.deleted", &format!("item {id}"));
        Ok(())
    }

    // ----------------------------------------------------------------------
    // WebSockets

    /// WebSocket connections, newest first.
    pub async fn ws_connections(
        &self,
        query: WsConnectionQuery,
    ) -> Result<Vec<WsConnectionRow>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.ws_connections(&query)).await??)
    }

    /// A page of one connection's messages.
    pub async fn ws_messages(&self, query: WsMessageQuery) -> Result<WsMessagePage, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.ws_messages(&query)).await??)
    }

    /// One message with its payload.
    pub async fn ws_message(&self, id: i64) -> Result<WsMessageDetail, AppError> {
        let db = self.db()?;
        let stored = tokio::task::spawn_blocking(move || db.ws_message(id))
            .await??
            .ok_or_else(|| AppError::Invalid(format!("WebSocket message {id} not found")))?;
        Ok(ws_view::detail(stored))
    }

    /// Writes a connection and all its messages to `path` as JSON.
    pub async fn ws_export(&self, connection_id: i64, path: &Path) -> Result<PathBuf, AppError> {
        let db = self.db()?;
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let connection = db.ws_connection(connection_id)?.ok_or_else(|| {
                AppError::Invalid(format!("WebSocket connection {connection_id} not found"))
            })?;
            let messages = db.ws_all_messages(connection_id)?;
            let json = ws_view::export_json(&connection, &messages)?;
            std::fs::write(&path, json)?;
            Ok::<_, AppError>(path)
        })
        .await?
    }

    /// Dashboard counters.
    pub async fn dashboard(&self) -> Result<DashboardStats, AppError> {
        let db = self.db()?;
        let (project, findings) = tokio::task::spawn_blocking(move || {
            Ok::<_, AppError>((db.stats()?, db.finding_counts()?))
        })
        .await??;
        Ok(DashboardStats {
            project,
            findings,
            proxy: self.inner.proxy.stats(),
            captures_dropped: self.inner.dropped.load(Ordering::Relaxed),
        })
    }
}

fn with_extension(path: &Path) -> PathBuf {
    if path.extension().is_some_and(|e| e == PROJECT_EXTENSION) {
        path.to_path_buf()
    } else {
        let mut s = path.as_os_str().to_os_string();
        s.push(format!(".{PROJECT_EXTENSION}"));
        PathBuf::from(s)
    }
}

fn display_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Project".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_paths() {
        assert_eq!(
            with_extension(Path::new("/a/b")),
            PathBuf::from("/a/b.chosproxy")
        );
        assert_eq!(
            with_extension(Path::new("/a/b.chosproxy")),
            PathBuf::from("/a/b.chosproxy")
        );
        assert_eq!(
            display_name(Path::new("/a/juice-shop.chosproxy")),
            "juice-shop"
        );
    }
}
