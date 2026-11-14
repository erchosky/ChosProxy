//! Scans (M25): a configuration from the library, run as one task in
//! phases — crawl, passive audit, JavaScript analysis — with an event log
//! that says why each item was audited or skipped.
//!
//! A scan never sends attack payloads: the crawl only follows links (GET,
//! scope-bound, rate-limited, words to avoid), and the audit phases read the
//! stored traffic. Active checks are not part of M25 (see ADR-0035).

use super::Engine;
use super::passive_ops::{active_issue_pair, cross_issues, issues_in, message_of, passive_issue};
use super::task_ops::Control;
use crate::AppError;
use chosproxy_core::Source;
use chosproxy_core::config::{ConfigSection, OastMode};
use chosproxy_core::event::EngineEvent;
use chosproxy_core::scope::UrlParts;
use chosproxy_core::time::now_ms;
use chosproxy_crawler::CrawlConfig;
use chosproxy_request::{Destination, Limiter, SendLimits};
use chosproxy_scanner::CrossAnalyzer;
use chosproxy_scanner::active::{
    ACTIVE_CHECKS, ActiveFinding, PlannedProbe, ProbeObservation, active_check,
    evaluate_point_probes, evaluate_request_probes, plan_point_probes, plan_request_probes,
};
use chosproxy_scanner::fingerprint::request_fingerprint;
use chosproxy_scanner::insertion::{PointKind, insertion_points, replace};
use chosproxy_scanner::javascript::{analyze_javascript, is_javascript};
use chosproxy_storage::{
    PassiveIssue, ProjectDb, RepeaterTarget, StoredExchange, TaskId, TaskProgress, TaskRow,
    TaskStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use tokio::sync::watch;
use ts_rs::TS;

/// Exchanges read per round of the audit.
const BATCH: u32 = 200;
/// Audit decisions written to the event log one by one; more are counted.
const LOGGED_DECISIONS: u64 = 300;
/// Most configurations in the library.
const MAX_CONFIGS: usize = 100;
/// The format tag of a shared configurations file.
const FILE_FORMAT: &str = "chosproxy-scan-configs";

/// A named scan configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct ScanConfig {
    /// Unique name in the library.
    pub name: String,
    /// What it is for.
    pub description: String,
    /// Comes with ChosProxy (read-only; save a copy to change it).
    pub builtin: bool,
    /// Phase 1: crawl from the start URLs. Off: audit the traffic already
    /// stored under the start URLs.
    pub crawl: bool,
    /// Crawl limits (its start URLs are those of the launch).
    pub crawl_config: CrawlConfig,
    /// Phase 2: run the passive checks (and check scripts) on each item.
    pub passive: bool,
    /// Passive and active checks turned off for this scan (check ids).
    pub disabled_checks: Vec<String>,
    /// Phase 3: analyze JavaScript responses.
    pub javascript: bool,
    /// Unique items audited at most.
    pub max_items: u32,
    /// Phase 4: run active verification probes on audited in-scope items (off by default).
    pub active: bool,
    /// Explicit per-launch confirmation that the tester is authorized to send active probes.
    /// Never persisted as `true` in saved configurations.
    pub active_authorized: bool,
    /// Insertion point kinds probed during active audit.
    pub active_point_kinds: Vec<PointKind>,
    /// Also probe non-idempotent methods (`POST`, `PUT`, `PATCH`, `DELETE`). Off by default.
    pub active_allow_unsafe_methods: bool,
    /// Maximum probes sent per insertion point (1..=50).
    pub max_requests_per_point: u32,
    /// Maximum active probe requests across the whole scan (1..=50_000).
    pub max_active_requests: u32,
    /// Maximum active probe requests in flight (1..=16).
    pub active_concurrency: u32,
    /// Maximum active probe requests started per second (1..=100).
    pub active_requests_per_second: u32,
    /// Per-probe response timeout in seconds (1..=120).
    pub active_timeout_secs: u32,
}

impl Default for ScanConfig {
    fn default() -> ScanConfig {
        ScanConfig {
            name: String::new(),
            description: String::new(),
            builtin: false,
            crawl: true,
            crawl_config: CrawlConfig::default(),
            passive: true,
            disabled_checks: Vec::new(),
            javascript: true,
            max_items: 5000,
            active: false,
            active_authorized: false,
            active_point_kinds: PointKind::DEFAULT.to_vec(),
            active_allow_unsafe_methods: false,
            max_requests_per_point: 10,
            max_active_requests: 500,
            active_concurrency: 2,
            active_requests_per_second: 10,
            active_timeout_secs: 15,
        }
    }
}

impl ScanConfig {
    /// Checks names and limits.
    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err("a configuration needs a name of at most 80 characters".into());
        }
        if self.description.len() > 2000 {
            return Err("the description is too long".into());
        }
        if !(1..=100_000).contains(&self.max_items) {
            return Err("unique items must be between 1 and 100000".into());
        }
        if !self.crawl && !self.passive && !self.javascript && !self.active {
            return Err("turn on at least one phase".into());
        }
        if !(1..=50).contains(&self.max_requests_per_point) {
            return Err("requests per insertion point must be between 1 and 50".into());
        }
        if !(1..=50_000).contains(&self.max_active_requests) {
            return Err("active request budget must be between 1 and 50000".into());
        }
        if !(1..=16).contains(&self.active_concurrency) {
            return Err("active concurrency must be between 1 and 16".into());
        }
        if !(1..=100).contains(&self.active_requests_per_second) {
            return Err("active requests per second must be between 1 and 100".into());
        }
        if !(1..=120).contains(&self.active_timeout_secs) {
            return Err("active timeout must be between 1 and 120 seconds".into());
        }
        // The crawl limits are checked with a placeholder start URL: the
        // real ones come with each launch.
        let mut crawl = self.crawl_config.clone();
        crawl.start_urls = vec!["http://localhost/".into()];
        crawl.validate().map(|_| ())
    }

    fn phases(&self) -> Vec<&'static str> {
        let mut phases = Vec::new();
        if self.crawl {
            phases.push("crawl");
        }
        if self.passive {
            phases.push("passive audit");
        }
        if self.javascript {
            phases.push("JavaScript analysis");
        }
        if self.active {
            phases.push("active audit");
        }
        phases
    }
}

/// The configurations that come with ChosProxy. None is aggressive: the
/// crawls are rate-limited and avoid state-changing links.
pub fn builtin_scan_configs() -> Vec<ScanConfig> {
    let light = CrawlConfig {
        max_depth: 3,
        max_requests: 200,
        concurrency: 2,
        requests_per_second: 5,
        max_similar: 5,
        ..CrawlConfig::default()
    };
    vec![
        ScanConfig {
            name: "Crawl and audit (light)".into(),
            description: "A gentle crawl (depth 3, 200 requests, 5 per second, 5 URLs per \
                          pattern), then the passive checks and JavaScript analysis on what \
                          it found. Good first scan of a shared or fragile target."
                .into(),
            builtin: true,
            crawl_config: light,
            ..ScanConfig::default()
        },
        ScanConfig {
            name: "Crawl and audit (balanced)".into(),
            description: "The default crawl (depth 5, 500 requests, 10 per second, 20 URLs \
                          per pattern), then the passive checks and JavaScript analysis."
                .into(),
            builtin: true,
            ..ScanConfig::default()
        },
        ScanConfig {
            name: "Audit stored traffic".into(),
            description: "Sends nothing: runs the passive checks and JavaScript analysis on \
                          the traffic already in the project under the start URLs (for \
                          example what you browsed through the proxy)."
                .into(),
            builtin: true,
            crawl: false,
            ..ScanConfig::default()
        },
        ScanConfig {
            name: "Crawl only".into(),
            description: "Maps the application (default crawl limits) without auditing; the \
                          background passive analysis still sees the traffic if it is on."
                .into(),
            builtin: true,
            passive: false,
            javascript: false,
            ..ScanConfig::default()
        },
    ]
}

/// The tester's own configurations (`scans.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScansConfig {
    /// Saved configurations.
    pub configs: Vec<ScanConfig>,
}

impl ConfigSection for ScansConfig {
    const FILE_NAME: &'static str = "scans.toml";

    fn validate(&self) -> Result<(), String> {
        if self.configs.len() > MAX_CONFIGS {
            return Err(format!("at most {MAX_CONFIGS} scan configurations"));
        }
        for c in &self.configs {
            c.validate().map_err(|e| format!("{}: {e}", c.name))?;
        }
        Ok(())
    }
}

/// What to scan, and how.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScanLaunch {
    /// Start URLs (absolute http/https). Without a crawl, the stored
    /// traffic under them is audited.
    pub start_urls: Vec<String>,
    /// The configuration to run (from the library, possibly edited).
    pub config: ScanConfig,
}

/// Where a scan phase is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PhaseState {
    /// Not started yet.
    Pending,
    /// Working.
    Running,
    /// Finished.
    Done,
    /// Stopped before the end (paused scans stay running).
    Stopped,
}

/// One phase of a scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScanPhase {
    /// `crawl`, `passive audit` or `JavaScript analysis`.
    pub name: String,
    /// Where it is.
    pub state: PhaseState,
}

/// What a scan did so far, for its dashboard (saved with the task).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct ScanSummary {
    /// The configuration's name.
    pub config: String,
    /// The phases in order.
    pub phases: Vec<ScanPhase>,
    /// Crawl requests sent.
    #[ts(type = "number")]
    pub crawl_requests: u64,
    /// Crawl requests that failed.
    #[ts(type = "number")]
    pub crawl_errors: u64,
    /// URLs the crawl found.
    #[ts(type = "number")]
    pub crawl_found: u64,
    /// Items read by the audit.
    #[ts(type = "number")]
    pub items_read: u64,
    /// Unique items audited.
    #[ts(type = "number")]
    pub audited: u64,
    /// Items skipped as structural duplicates.
    #[ts(type = "number")]
    pub duplicates: u64,
    /// Items skipped without a response.
    #[ts(type = "number")]
    pub no_response: u64,
    /// Items skipped out of scope.
    #[ts(type = "number")]
    pub out_of_scope: u64,
    /// Scripts analyzed.
    #[ts(type = "number")]
    pub scripts: u64,
    /// Active probe requests sent (M13).
    #[ts(type = "number")]
    pub active_requests: u64,
    /// Active probe requests that failed (M13).
    #[ts(type = "number")]
    pub active_errors: u64,
    /// Insertion points audited actively (M13).
    #[ts(type = "number")]
    pub active_points: u64,
    /// Items or points skipped during active audit (M13).
    #[ts(type = "number")]
    pub active_skipped: u64,
    /// Findings created.
    pub findings_created: u32,
    /// Findings that got new evidence.
    pub findings_updated: u32,
    /// Distinct issues reported, by severity (`info`…`critical`).
    pub issues_by_severity: BTreeMap<String, u32>,
}

impl ScanSummary {
    fn new(config: &ScanConfig) -> ScanSummary {
        ScanSummary {
            config: config.name.clone(),
            phases: config
                .phases()
                .into_iter()
                .map(|name| ScanPhase {
                    name: name.into(),
                    state: PhaseState::Pending,
                })
                .collect(),
            ..ScanSummary::default()
        }
    }

    /// Sets the state of the phases named.
    fn set(&mut self, names: &[&str], state: PhaseState) {
        for phase in &mut self.phases {
            if names.contains(&phase.name.as_str()) {
                phase.state = state;
            }
        }
    }

    /// Marks phases still pending or running as stopped.
    fn stop_rest(&mut self) {
        for phase in &mut self.phases {
            if matches!(phase.state, PhaseState::Pending | PhaseState::Running) {
                phase.state = PhaseState::Stopped;
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ScanConfigFile {
    format: String,
    version: u32,
    configs: Vec<ScanConfig>,
}

/// Counters of the audit phases.
#[derive(Debug, Default)]
struct AuditCounts {
    read: u64,
    audited: u64,
    duplicates: u64,
    no_response: u64,
    outside: u64,
    logged: u64,
}

impl Engine {
    /// The library: built-in configurations first, then the tester's.
    pub fn scan_configs(&self) -> Vec<ScanConfig> {
        let mut all = builtin_scan_configs();
        all.extend(
            self.inner
                .store
                .load::<ScansConfig>()
                .value
                .configs
                .into_iter()
                .map(|mut c| {
                    c.builtin = false;
                    c
                }),
        );
        all
    }

    /// Adds or replaces (same name) a configuration of the tester.
    pub fn save_scan_config(&self, mut config: ScanConfig) -> Result<Vec<ScanConfig>, AppError> {
        config.name = config.name.trim().to_string();
        config.builtin = false;
        config.active_authorized = false;
        if builtin_scan_configs()
            .iter()
            .any(|b| b.name.eq_ignore_ascii_case(&config.name))
        {
            return Err(AppError::Invalid(format!(
                "\"{}\" is a built-in configuration; save it under another name",
                config.name
            )));
        }
        config.validate().map_err(AppError::Invalid)?;
        let mut stored = self.inner.store.load::<ScansConfig>().value;
        match stored
            .configs
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(&config.name))
        {
            Some(existing) => *existing = config,
            None => stored.configs.push(config),
        }
        stored.validate().map_err(AppError::Invalid)?;
        self.inner.store.save(&stored)?;
        Ok(self.scan_configs())
    }

    /// Removes a configuration of the tester.
    pub fn delete_scan_config(&self, name: &str) -> Result<Vec<ScanConfig>, AppError> {
        let mut stored = self.inner.store.load::<ScansConfig>().value;
        let before = stored.configs.len();
        stored
            .configs
            .retain(|c| !c.name.eq_ignore_ascii_case(name));
        if stored.configs.len() == before {
            return Err(AppError::Invalid(format!(
                "no saved configuration named \"{name}\""
            )));
        }
        self.inner.store.save(&stored)?;
        Ok(self.scan_configs())
    }

    /// Writes configurations (all when `names` is empty) to a JSON file to
    /// share. Returns how many were written.
    pub fn export_scan_configs(
        &self,
        path: &std::path::Path,
        names: &[String],
    ) -> Result<usize, AppError> {
        let configs: Vec<ScanConfig> = self
            .scan_configs()
            .into_iter()
            .filter(|c| names.is_empty() || names.iter().any(|n| n.eq_ignore_ascii_case(&c.name)))
            .collect();
        let file = ScanConfigFile {
            format: FILE_FORMAT.into(),
            version: 1,
            configs,
        };
        super::write_shared_file(path, &file, "configurations")?;
        Ok(file.configs.len())
    }

    /// Adds the configurations of a shared file to the library (names in
    /// use get a suffix; nothing starts on import).
    pub fn import_scan_configs(&self, path: &std::path::Path) -> Result<Vec<ScanConfig>, AppError> {
        let file: ScanConfigFile = super::read_shared_file(path, 1, "scan configurations")?;
        if file.format != FILE_FORMAT {
            return Err(AppError::Invalid(
                "not a ChosProxy scan configurations file".into(),
            ));
        }
        let mut stored = self.inner.store.load::<ScansConfig>().value;
        let taken = |stored: &ScansConfig, name: &str| {
            builtin_scan_configs()
                .iter()
                .any(|b| b.name.eq_ignore_ascii_case(name))
                || stored
                    .configs
                    .iter()
                    .any(|c| c.name.eq_ignore_ascii_case(name))
        };
        for mut config in file.configs {
            config.builtin = false;
            config.active_authorized = false;
            let base: String = config.name.trim().chars().take(70).collect();
            config.name = base.clone();
            let mut n = 2;
            while taken(&stored, &config.name) {
                config.name = format!("{base} ({n})");
                n += 1;
            }
            config
                .validate()
                .map_err(|e| AppError::Invalid(format!("{}: {e}", config.name)))?;
            stored.configs.push(config);
        }
        stored.validate().map_err(AppError::Invalid)?;
        self.inner.store.save(&stored)?;
        Ok(self.scan_configs())
    }

    /// Validates a scan launch (including active audit preconditions: target,
    /// non-empty scope, in-scope start URLs, explicit authorization, and budgets).
    pub(crate) fn validate_scan_launch(
        &self,
        launch: &ScanLaunch,
    ) -> Result<(CrawlConfig, Vec<url::Url>), AppError> {
        launch.config.validate().map_err(AppError::Invalid)?;
        let mut crawl = launch.config.crawl_config.clone();
        crawl.start_urls = launch.start_urls.clone();
        let start = crawl.validate().map_err(AppError::Invalid)?;
        if launch.config.active {
            if !launch.config.active_authorized {
                return Err(AppError::Invalid(
                    "active audit requires explicit target authorization confirmation".into(),
                ));
            }
            let scope = super::read(&self.inner.scope);
            if scope.is_empty() {
                return Err(AppError::Invalid(
                    "active audit requires a non-empty project scope; add the target to Scope first"
                        .into(),
                ));
            }
            for u in &start {
                let Some(host) = u.host_str() else {
                    return Err(AppError::Invalid(format!("invalid start URL: {u}")));
                };
                let port = u.port_or_known_default().unwrap_or(80);
                let mut path = u.path().to_string();
                if let Some(q) = u.query() {
                    path.push('?');
                    path.push_str(q);
                }
                let parts = UrlParts {
                    scheme: u.scheme().to_string(),
                    host: host.to_string(),
                    port,
                    path,
                };
                if !scope.contains(&parts.target()) {
                    return Err(AppError::Invalid(format!(
                        "{u} is outside the project scope (active audit requires in-scope targets)"
                    )));
                }
            }
        }
        Ok((crawl, start))
    }

    /// Starts a scan task.
    pub async fn scan_start(&self, launch: ScanLaunch) -> Result<TaskRow, AppError> {
        let config = launch.config.clone();
        let (crawl, start) = self.validate_scan_launch(&launch)?;
        let first = start
            .first()
            .and_then(|u| u.host_str())
            .unwrap_or("target")
            .to_string();
        let name = format!("Scan {first} ({})", config.name);
        let json = serde_json::to_string(&launch)
            .map_err(|e| AppError::Invalid(format!("invalid scan settings: {e}")))?;
        self.launch_task(
            "scan",
            "Scan",
            name,
            json,
            move |engine, db, id, rx| async move {
                engine.run_scan(&db, id, config, crawl, start, rx).await
            },
        )
        .await
    }

    async fn run_scan(
        &self,
        db: &ProjectDb,
        id: TaskId,
        config: ScanConfig,
        crawl: CrawlConfig,
        start: Vec<url::Url>,
        control: watch::Receiver<Control>,
    ) -> Result<TaskStatus, AppError> {
        let phases = config.phases();
        self.task_log_lines(
            db,
            id,
            vec![(
                "info".into(),
                if config.active {
                    format!(
                        "Scan with \"{}\": {} (active budget: {} requests, {}/point, {} req/s).",
                        config.name,
                        phases.join(" → "),
                        config.max_active_requests,
                        config.max_requests_per_point,
                        config.active_requests_per_second
                    )
                } else {
                    format!(
                        "Scan with \"{}\": {}. Nothing but GET requests from links is sent; \
                         the audit reads the stored traffic.",
                        config.name,
                        phases.join(" → ")
                    )
                },
            )],
        )
        .await;
        let first_id = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || db.last_exchange_id()).await??
        };
        let mut summary = ScanSummary::new(&config);
        if config.crawl {
            summary.set(&["crawl"], PhaseState::Running);
            self.save_summary(db, id, &summary).await?;
            self.task_log_lines(db, id, vec![("info".into(), "Phase: crawl.".into())])
                .await;
            let status = self
                .run_crawl(db, id, crawl.clone(), start.clone(), control.clone())
                .await?;
            let row = {
                let db = db.clone();
                tokio::task::spawn_blocking(move || db.task(id)).await??
            };
            if let Some(row) = row {
                summary.crawl_requests = row.requests;
                summary.crawl_errors = row.errors;
                summary.crawl_found = row.discovered;
            }
            if status != TaskStatus::Completed {
                summary.stop_rest();
                self.save_summary(db, id, &summary).await?;
                return Ok(status);
            }
            summary.set(&["crawl"], PhaseState::Done);
        }
        if !config.passive && !config.javascript && !config.active {
            self.save_summary(db, id, &summary).await?;
            return Ok(TaskStatus::Completed);
        }
        summary.set(
            &["passive audit", "JavaScript analysis"],
            PhaseState::Running,
        );
        self.save_summary(db, id, &summary).await?;
        self.task_log_lines(
            db,
            id,
            vec![(
                "info".into(),
                if config.crawl {
                    "Phase: audit of the crawled items (each structural pattern once).".into()
                } else {
                    "Phase: audit of the stored traffic under the start URLs (each \
                     structural pattern once)."
                        .into()
                },
            )],
        )
        .await;
        let (status, audited_ids, mut severities) = self
            .run_audit(
                db,
                id,
                &config,
                &start,
                first_id,
                control.clone(),
                &mut summary,
            )
            .await?;
        if status == TaskStatus::Completed {
            summary.set(&["passive audit", "JavaScript analysis"], PhaseState::Done);
        } else {
            summary.stop_rest();
            self.save_summary(db, id, &summary).await?;
            return Ok(status);
        }
        if config.active {
            summary.set(&["active audit"], PhaseState::Running);
            self.save_summary(db, id, &summary).await?;
            self.audit(
                "scanner.active.started",
                &format!(
                    "task {id}, {} item(s), budget {} requests",
                    audited_ids.len(),
                    config.max_active_requests
                ),
            );
            self.task_log_lines(
                db,
                id,
                vec![(
                    "info".into(),
                    format!(
                        "Phase: active audit on {} unique item(s) (budget: {} requests, {}/point).",
                        audited_ids.len(),
                        config.max_active_requests,
                        config.max_requests_per_point
                    ),
                )],
            )
            .await;
            let active_status = self
                .run_active_audit(
                    db,
                    id,
                    &config,
                    &audited_ids,
                    control,
                    &mut summary,
                    &mut severities,
                )
                .await?;
            if active_status == TaskStatus::Completed {
                summary.set(&["active audit"], PhaseState::Done);
                self.audit(
                    "scanner.active.completed",
                    &format!(
                        "task {id}, {} probe(s) sent, {} point(s) tested",
                        summary.active_requests, summary.active_points
                    ),
                );
            } else {
                summary.stop_rest();
            }
            self.save_summary(db, id, &summary).await?;
            return Ok(active_status);
        }
        self.save_summary(db, id, &summary).await?;
        Ok(status)
    }

    /// A scan's dashboard data (`None` for other tasks and scans from
    /// before M26).
    pub async fn scan_summary(&self, id: TaskId) -> Result<Option<ScanSummary>, AppError> {
        let db = self.db()?;
        let json = tokio::task::spawn_blocking(move || db.task_summary(id)).await??;
        Ok(serde_json::from_str(&json).ok())
    }

    async fn save_summary(
        &self,
        db: &ProjectDb,
        id: TaskId,
        summary: &ScanSummary,
    ) -> Result<(), AppError> {
        let json = serde_json::to_string(summary)
            .map_err(|e| AppError::Invalid(format!("cannot save the scan summary: {e}")))?;
        let db = db.clone();
        tokio::task::spawn_blocking(move || db.task_set_summary(id, json)).await??;
        Ok(())
    }

    /// Reads the items to audit in id order and audits each structural
    /// pattern once.
    #[allow(clippy::too_many_arguments)]
    async fn run_audit(
        &self,
        db: &ProjectDb,
        id: TaskId,
        config: &ScanConfig,
        start: &[url::Url],
        first_id: i64,
        control: watch::Receiver<Control>,
        summary: &mut ScanSummary,
    ) -> Result<
        (
            TaskStatus,
            Vec<i64>,
            HashMap<String, chosproxy_storage::FindingSeverity>,
        ),
        AppError,
    > {
        let mut severities: HashMap<String, chosproxy_storage::FindingSeverity> = HashMap::new();
        let mut audited_ids: Vec<i64> = Vec::new();
        let crawled_only = config.crawl;
        let after = if crawled_only { first_id } else { 0 };
        let prefixes: Vec<String> = start.iter().map(|u| prefix_of(u.as_str())).collect();
        let scope = super::read(&self.inner.scope);
        let checks = self.library().checks();
        let mut seen: HashMap<String, i64> = HashMap::new();
        // The cross-request checks see every item, duplicates included.
        let mut cross = CrossAnalyzer::new();
        let mut counts = AuditCounts::default();
        let mut cursor = after;
        let mut created = 0u32;
        let mut updated = 0u32;
        let mut status = TaskStatus::Completed;
        loop {
            if *control.borrow() == Control::Cancel {
                status = TaskStatus::Cancelled;
                break;
            }
            let ids = {
                let db = db.clone();
                tokio::task::spawn_blocking(move || db.exchange_ids_after(cursor, BATCH)).await??
            };
            let Some(&last) = ids.last() else {
                break;
            };
            cursor = last;
            let exchanges = {
                let db = db.clone();
                tokio::task::spawn_blocking(move || db.exchanges_bulk(&ids)).await??
            };
            let mut log = Vec::new();
            let mut issues: Vec<PassiveIssue> = Vec::new();
            for ex in &exchanges {
                let row = &ex.row;
                let wanted = if crawled_only {
                    row.source == Source::Crawler
                } else {
                    prefixes.iter().any(|p| row.url.starts_with(p.as_str()))
                };
                if !wanted {
                    continue;
                }
                counts.read += 1;
                if !scope.is_empty() {
                    let parts = UrlParts {
                        scheme: row.scheme.clone(),
                        host: row.host.clone(),
                        port: row.port,
                        path: row.path.clone(),
                    };
                    if !scope.contains(&parts.target()) {
                        counts.outside += 1;
                        continue;
                    }
                }
                if ex.response.is_none() {
                    counts.no_response += 1;
                    continue;
                }
                if config.passive {
                    issues.extend(cross_issues(&mut cross, ex, &config.disabled_checks));
                }
                let fingerprint = request_fingerprint(&String::from_utf8_lossy(&ex.request));
                if let Some(first) = seen.get(&fingerprint) {
                    counts.duplicates += 1;
                    decision(
                        &mut log,
                        &mut counts,
                        format!(
                            "Skipped #{} {} {}: same pattern as #{first}.",
                            row.id, row.method, row.url
                        ),
                    );
                    continue;
                }
                if counts.audited >= u64::from(config.max_items) {
                    break;
                }
                seen.insert(fingerprint.clone(), row.id);
                counts.audited += 1;
                audited_ids.push(row.id);
                decision(
                    &mut log,
                    &mut counts,
                    format!(
                        "Audited #{} {} {}: first item of pattern {fingerprint}.",
                        row.id, row.method, row.url
                    ),
                );
                if config.passive {
                    issues.extend(issues_in(ex, &config.disabled_checks));
                    issues.extend(super::script_ops::script_issues(ex, &checks));
                }
                if config.javascript {
                    if is_javascript(&message_of(ex)) {
                        summary.scripts += 1;
                    }
                    issues.extend(javascript_issues(ex, &config.disabled_checks));
                }
            }
            for issue in &issues {
                severities
                    .entry(issue.dedup_key.clone())
                    .or_insert(issue.severity);
            }
            if !issues.is_empty() {
                let db = db.clone();
                let recorded =
                    tokio::task::spawn_blocking(move || db.findings_record_auto(issues)).await??;
                created += recorded.created;
                updated += recorded.updated;
            }
            self.task_log_lines(db, id, log).await;
            self.save_audit_progress(db, id, &counts).await?;
            summary.items_read = counts.read;
            summary.audited = counts.audited;
            summary.duplicates = counts.duplicates;
            summary.no_response = counts.no_response;
            summary.out_of_scope = counts.outside;
            summary.findings_created = created;
            summary.findings_updated = updated;
            summary.issues_by_severity = BTreeMap::new();
            for severity in severities.values() {
                *summary
                    .issues_by_severity
                    .entry(severity.as_str().to_string())
                    .or_insert(0) += 1;
            }
            self.save_summary(db, id, summary).await?;
            if counts.audited >= u64::from(config.max_items) {
                self.task_log_lines(
                    db,
                    id,
                    vec![(
                        "warning".into(),
                        format!(
                            "Stopped at {} unique items (the configuration's limit).",
                            config.max_items
                        ),
                    )],
                )
                .await;
                break;
            }
        }
        if created + updated > 0 {
            let _ = self
                .inner
                .events
                .send(EngineEvent::FindingsChanged { created });
        }
        let mut summary_log = vec![(
            "info".into(),
            format!(
                "{} after reading {} item(s): {} audited, {} skipped as duplicates, {} without \
                 a response, {} out of scope; {created} finding(s) created, {updated} updated.",
                if status == TaskStatus::Cancelled {
                    "Stopped"
                } else {
                    "Audit finished"
                },
                counts.read,
                counts.audited,
                counts.duplicates,
                counts.no_response,
                counts.outside
            ),
        )];
        if counts.read == 0 {
            summary_log.push((
                "warning".into(),
                if crawled_only {
                    "The crawl stored no items to audit.".into()
                } else {
                    "No stored traffic under the start URLs: browse the application through \
                     the proxy first, or use a configuration with a crawl."
                        .into()
                },
            ));
        }
        self.task_log_lines(db, id, summary_log).await;
        Ok((status, audited_ids, severities))
    }

    /// Runs the active audit phase (M13, spec §23) on the deduplicated
    /// in-scope baseline exchanges collected during `run_audit`.
    #[allow(clippy::too_many_arguments)]
    async fn run_active_audit(
        &self,
        db: &ProjectDb,
        id: TaskId,
        config: &ScanConfig,
        audited_ids: &[i64],
        mut control: watch::Receiver<Control>,
        summary: &mut ScanSummary,
        severities: &mut HashMap<String, chosproxy_storage::FindingSeverity>,
    ) -> Result<TaskStatus, AppError> {
        let limiter = Limiter::new(
            config.active_concurrency as usize,
            config.active_requests_per_second,
        );
        let timeout = Duration::from_secs(u64::from(config.active_timeout_secs));
        let limits = SendLimits {
            response_timeout: timeout,
            body_idle_timeout: timeout,
            max_response_bytes: super::limits_of(&self.settings())
                .max_message_bytes
                .max(64 * 1024),
        };
        let oast_enabled = self.oast_config().mode != OastMode::Off;
        let enabled_checks: Vec<&'static chosproxy_scanner::ActiveCheckInfo> = ACTIVE_CHECKS
            .iter()
            .filter(|c| !config.disabled_checks.iter().any(|d| d == c.id))
            .filter(|c| !c.requires_oast || oast_enabled)
            .collect();
        let active_scripts: Vec<_> = self
            .library()
            .checks()
            .into_iter()
            .filter(|(entry, script)| {
                script.defines("active_check")
                    && !config.disabled_checks.iter().any(|d| d == &entry.id)
            })
            .collect();

        let mut paused = false;
        let mut status = TaskStatus::Completed;
        let mut logged_decisions = 0u64;

        for (item_idx, &baseline_id) in audited_ids.iter().enumerate() {
            let mut log: Vec<(String, String)> = Vec::new();
            if !self
                .wait_active_control(db, id, &mut control, &mut paused, &mut log)
                .await?
            {
                status = TaskStatus::Cancelled;
                self.task_log_lines(db, id, log).await;
                break;
            }
            if summary.active_requests + summary.active_errors
                >= u64::from(config.max_active_requests)
            {
                log.push((
                    "warning".into(),
                    format!(
                        "Stopped active audit at {} requests (the configuration's active request budget).",
                        config.max_active_requests
                    ),
                ));
                self.task_log_lines(db, id, log).await;
                break;
            }

            let ex = {
                let db = db.clone();
                let mut rows =
                    tokio::task::spawn_blocking(move || db.exchanges_bulk(&[baseline_id]))
                        .await??;
                rows.pop()
            };
            let Some(ex) = ex else {
                continue;
            };
            let row = &ex.row;
            if !config.active_allow_unsafe_methods
                && !matches!(row.method.as_str(), "GET" | "HEAD" | "OPTIONS")
            {
                summary.active_skipped += 1;
                active_note(
                    &mut log,
                    &mut logged_decisions,
                    "info",
                    format!(
                        "Skipped active audit of #{} {} {}: non-idempotent method (unsafe methods are off).",
                        row.id, row.method, row.url
                    ),
                );
                self.task_log_lines(db, id, log).await;
                continue;
            }
            let target = RepeaterTarget {
                scheme: row.scheme.clone(),
                host: row.host.clone(),
                port: row.port,
            };
            if !self.is_target_in_active_scope(&target, &row.path) {
                summary.out_of_scope += 1;
                summary.active_skipped += 1;
                active_note(
                    &mut log,
                    &mut logged_decisions,
                    "info",
                    format!(
                        "Skipped active audit of #{} {} {}: target is out of scope.",
                        row.id, row.method, row.url
                    ),
                );
                self.task_log_lines(db, id, log).await;
                continue;
            }

            let baseline_msg = message_of(&ex);
            let baseline_script_msg = super::script_ops::message_of(&ex);
            let base_text = String::from_utf8_lossy(&ex.request).into_owned();
            let mut batch_issues: Vec<PassiveIssue> = Vec::new();
            let mut item_aborted_scope = false;

            // 1. Request-level active checks (e.g. active-cors).
            for &check_info in enabled_checks.iter().filter(|c| c.point_kinds.is_empty()) {
                let planned =
                    plan_request_probes(check_info.id, &base_text, config.max_requests_per_point);
                let mut obs_owned: Vec<(&'static str, String, i64, chosproxy_scanner::Message)> =
                    Vec::new();
                for probe in planned {
                    if !self
                        .wait_active_control(db, id, &mut control, &mut paused, &mut log)
                        .await?
                    {
                        status = TaskStatus::Cancelled;
                        break;
                    }
                    if summary.active_requests + summary.active_errors
                        >= u64::from(config.max_active_requests)
                    {
                        break;
                    }
                    if !self.is_target_in_active_scope(&target, &row.path) {
                        summary.out_of_scope += 1;
                        summary.active_skipped += 1;
                        item_aborted_scope = true;
                        break;
                    }
                    if let Some((probe_id, probe_ex)) = self
                        .send_active_probe(
                            db,
                            id,
                            &limiter,
                            &limits,
                            &target,
                            &ex,
                            &probe,
                            "request headers",
                            item_idx,
                            audited_ids.len(),
                            summary,
                            &mut log,
                            &mut logged_decisions,
                        )
                        .await?
                    {
                        obs_owned.push((
                            probe.label,
                            probe.payload,
                            probe_id,
                            message_of(&probe_ex),
                        ));
                    }
                }
                if status == TaskStatus::Cancelled || item_aborted_scope {
                    break;
                }
                let obs_refs: Vec<ProbeObservation<'_>> = obs_owned
                    .iter()
                    .map(|(label, payload, _, msg)| ProbeObservation {
                        label,
                        payload: payload.as_str(),
                        message: msg,
                    })
                    .collect();
                if let Some(finding) =
                    evaluate_request_probes(check_info.id, &baseline_msg, &obs_refs)
                    && let Some((_, _, probe_id, _)) = obs_owned.get(finding.probe_index)
                {
                    batch_issues.extend(active_issue_pair(row.id, *probe_id, &finding));
                }
            }

            if status == TaskStatus::Cancelled {
                self.task_log_lines(db, id, log).await;
                break;
            }
            if item_aborted_scope {
                self.task_log_lines(db, id, log).await;
                continue;
            }

            // 2. Insertion-point active checks and active check scripts.
            let points = insertion_points(&base_text, &config.active_point_kinds);
            for point in &points {
                if !self
                    .wait_active_control(db, id, &mut control, &mut paused, &mut log)
                    .await?
                {
                    status = TaskStatus::Cancelled;
                    break;
                }
                if summary.active_requests + summary.active_errors
                    >= u64::from(config.max_active_requests)
                {
                    break;
                }
                if !self.is_target_in_active_scope(&target, &row.path) {
                    summary.out_of_scope += 1;
                    summary.active_skipped += 1;
                    active_note(
                        &mut log,
                        &mut logged_decisions,
                        "info",
                        format!(
                            "Stopped probing #{} {} {}: target removed from scope.",
                            row.id, row.method, row.url
                        ),
                    );
                    break;
                }
                summary.active_points += 1;
                let mut point_sent = 0u32;

                for &check_info in enabled_checks.iter().filter(|c| !c.point_kinds.is_empty()) {
                    if point_sent >= config.max_requests_per_point {
                        break;
                    }
                    let remaining_point = config.max_requests_per_point - point_sent;
                    let remaining_total = config
                        .max_active_requests
                        .saturating_sub((summary.active_requests + summary.active_errors) as u32);
                    let budget = remaining_point.min(remaining_total);
                    if budget == 0 {
                        break;
                    }

                    let mut oast_token: Option<String> = None;
                    let mut oast_url: Option<String> = None;
                    if check_info.id == "active-oast" {
                        if let Ok(payload_row) = self
                            .oast_new_payload(format!(
                                "Active scan on #{} {} ({})",
                                row.id, row.url, point.name
                            ))
                            .await
                        {
                            oast_token = Some(payload_row.correlation_id);
                            oast_url = Some(payload_row.url);
                        } else {
                            continue;
                        }
                    }

                    let planned = plan_point_probes(
                        check_info.id,
                        &base_text,
                        point,
                        budget,
                        oast_url.as_deref(),
                    );
                    if planned.is_empty() {
                        continue;
                    }

                    let mut obs_owned: Vec<(
                        &'static str,
                        String,
                        i64,
                        chosproxy_scanner::Message,
                    )> = Vec::new();
                    for probe in planned {
                        if !self
                            .wait_active_control(db, id, &mut control, &mut paused, &mut log)
                            .await?
                        {
                            status = TaskStatus::Cancelled;
                            break;
                        }
                        if summary.active_requests + summary.active_errors
                            >= u64::from(config.max_active_requests)
                        {
                            break;
                        }
                        if !self.is_target_in_active_scope(&target, &row.path) {
                            summary.out_of_scope += 1;
                            summary.active_skipped += 1;
                            item_aborted_scope = true;
                            break;
                        }
                        point_sent += 1;
                        if let Some((probe_id, probe_ex)) = self
                            .send_active_probe(
                                db,
                                id,
                                &limiter,
                                &limits,
                                &target,
                                &ex,
                                &probe,
                                &point.name,
                                item_idx,
                                audited_ids.len(),
                                summary,
                                &mut log,
                                &mut logged_decisions,
                            )
                            .await?
                        {
                            obs_owned.push((
                                probe.label,
                                probe.payload,
                                probe_id,
                                message_of(&probe_ex),
                            ));
                        }
                    }
                    if status == TaskStatus::Cancelled || item_aborted_scope {
                        break;
                    }

                    if check_info.id == "active-oast" {
                        if let Some(token) = oast_token
                            && let Some((_, payload_sent, probe_id, _)) = obs_owned.first()
                        {
                            let mut interactions = Vec::new();
                            for _ in 0..6 {
                                tokio::time::sleep(Duration::from_millis(25)).await;
                                let db_check = db.clone();
                                let token_clone = token.clone();
                                interactions = tokio::task::spawn_blocking(move || {
                                    db_check.oast_interactions(Some(token_clone), 10)
                                })
                                .await??;
                                if !interactions.is_empty() {
                                    break;
                                }
                            }
                            if let Some(first_ix) = interactions.first()
                                && let Some(oast_info) = active_check("active-oast")
                            {
                                let finding = ActiveFinding {
                                    check: oast_info,
                                    title: format!(
                                        "Out-of-band ({}) interaction via '{}' on {}",
                                        first_ix.protocol.to_ascii_uppercase(),
                                        point.name,
                                        baseline_msg.path_only()
                                    ),
                                    severity: chosproxy_scanner::Severity::High,
                                    confidence: chosproxy_scanner::Confidence::Certain,
                                    evidence: format!(
                                        "Injecting OAST payload '{}' into '{}' triggered {} interaction(s) for token {} from {} ({}).",
                                        payload_sent,
                                        point.name,
                                        interactions.len(),
                                        token,
                                        first_ix.remote,
                                        first_ix.summary
                                    ),
                                    dedup: format!(
                                        "active-oast|{}{}|{}",
                                        baseline_msg.origin(),
                                        baseline_msg.path_only(),
                                        point.name
                                    ),
                                    probe_index: 0,
                                };
                                batch_issues.extend(active_issue_pair(row.id, *probe_id, &finding));
                            }
                        }
                    } else {
                        let obs_refs: Vec<ProbeObservation<'_>> = obs_owned
                            .iter()
                            .map(|(label, payload, _, msg)| ProbeObservation {
                                label,
                                payload: payload.as_str(),
                                message: msg,
                            })
                            .collect();
                        if let Some(finding) =
                            evaluate_point_probes(check_info.id, &baseline_msg, point, &obs_refs)
                            && let Some((_, _, probe_id, _)) = obs_owned.get(finding.probe_index)
                        {
                            batch_issues.extend(active_issue_pair(row.id, *probe_id, &finding));
                        }
                    }
                }

                // Active check scripts (SCANNER-013)
                if status != TaskStatus::Cancelled && !item_aborted_scope {
                    for (entry, script) in &active_scripts {
                        if point_sent >= config.max_requests_per_point {
                            break;
                        }
                        let probes = match script.active_probes(&point.value) {
                            Ok(p) => p,
                            Err(e) => {
                                tracing::debug!(script = %entry.id, error = %e, "active_probes failed");
                                continue;
                            }
                        };
                        for payload in probes {
                            if point_sent >= config.max_requests_per_point
                                || summary.active_requests + summary.active_errors
                                    >= u64::from(config.max_active_requests)
                            {
                                break;
                            }
                            if !self
                                .wait_active_control(db, id, &mut control, &mut paused, &mut log)
                                .await?
                            {
                                status = TaskStatus::Cancelled;
                                break;
                            }
                            if !self.is_target_in_active_scope(&target, &row.path) {
                                summary.out_of_scope += 1;
                                summary.active_skipped += 1;
                                item_aborted_scope = true;
                                break;
                            }
                            point_sent += 1;
                            let probe = PlannedProbe {
                                check_id: "script",
                                label: "script-probe",
                                request_text: replace(&base_text, point, &payload),
                                payload: payload.clone(),
                            };
                            if let Some((probe_id, probe_ex)) = self
                                .send_active_probe(
                                    db,
                                    id,
                                    &limiter,
                                    &limits,
                                    &target,
                                    &ex,
                                    &probe,
                                    &point.name,
                                    item_idx,
                                    audited_ids.len(),
                                    summary,
                                    &mut log,
                                    &mut logged_decisions,
                                )
                                .await?
                            {
                                let probe_script_msg = super::script_ops::message_of(&probe_ex);
                                if let Ok(run) = script.active_check(
                                    &baseline_script_msg,
                                    &probe_script_msg,
                                    &payload,
                                ) {
                                    for f in run.value {
                                        let mut base_issue = super::script_ops::script_active_issue(
                                            entry,
                                            row.id,
                                            f.clone(),
                                        );
                                        base_issue.evidence_note = format!(
                                            "Baseline exchange #{} before active script probe.",
                                            row.id
                                        );
                                        let probe_issue = super::script_ops::script_active_issue(
                                            entry, probe_id, f,
                                        );
                                        batch_issues.push(base_issue);
                                        batch_issues.push(probe_issue);
                                    }
                                }
                            }
                        }
                        if status == TaskStatus::Cancelled || item_aborted_scope {
                            break;
                        }
                    }
                }

                if status == TaskStatus::Cancelled || item_aborted_scope {
                    break;
                }
            }

            for issue in &batch_issues {
                severities
                    .entry(issue.dedup_key.clone())
                    .or_insert(issue.severity);
            }
            if !batch_issues.is_empty() {
                let db_rec = db.clone();
                let recorded =
                    tokio::task::spawn_blocking(move || db_rec.findings_record_auto(batch_issues))
                        .await??;
                summary.findings_created += recorded.created;
                if recorded.created == 0 {
                    summary.findings_updated += recorded.updated;
                }
                if recorded.created + recorded.updated > 0 {
                    let _ = self.inner.events.send(EngineEvent::FindingsChanged {
                        created: recorded.created,
                    });
                }
            }
            summary.issues_by_severity = BTreeMap::new();
            for severity in severities.values() {
                *summary
                    .issues_by_severity
                    .entry(severity.as_str().to_string())
                    .or_insert(0) += 1;
            }
            self.save_summary(db, id, summary).await?;
            self.task_log_lines(db, id, log).await;

            if status == TaskStatus::Cancelled {
                break;
            }
        }

        self.task_log_lines(
            db,
            id,
            vec![(
                "info".into(),
                format!(
                    "{} active audit: {} probe request(s) sent, {} error(s), {} point(s) tested, {} skipped.",
                    if status == TaskStatus::Cancelled {
                        "Stopped"
                    } else {
                        "Finished"
                    },
                    summary.active_requests,
                    summary.active_errors,
                    summary.active_points,
                    summary.active_skipped
                ),
            )],
        )
        .await;
        Ok(status)
    }

    fn is_target_in_active_scope(&self, target: &RepeaterTarget, path: &str) -> bool {
        let scope = super::read(&self.inner.scope);
        if scope.is_empty() {
            return false;
        }
        let parts = UrlParts {
            scheme: target.scheme.clone(),
            host: target.host.clone(),
            port: target.port,
            path: path.to_string(),
        };
        scope.contains(&parts.target())
    }

    async fn wait_active_control(
        &self,
        db: &ProjectDb,
        id: TaskId,
        control: &mut watch::Receiver<Control>,
        paused: &mut bool,
        log: &mut Vec<(String, String)>,
    ) -> Result<bool, AppError> {
        loop {
            let wanted = *control.borrow_and_update();
            match wanted {
                Control::Cancel | Control::Suspend => {
                    log.push(("info".into(), "Active audit stopped.".into()));
                    return Ok(false);
                }
                Control::Pause | Control::Run => {}
            }
            if (wanted == Control::Pause) != *paused {
                *paused = wanted == Control::Pause;
                self.set_status(
                    db,
                    id,
                    if *paused {
                        TaskStatus::Paused
                    } else {
                        TaskStatus::Running
                    },
                )
                .await?;
                log.push((
                    "info".into(),
                    if *paused {
                        "Active audit paused."
                    } else {
                        "Active audit resumed."
                    }
                    .into(),
                ));
            }
            if !*paused {
                return Ok(true);
            }
            if control.changed().await.is_err() {
                return Ok(false);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_active_probe(
        &self,
        db: &ProjectDb,
        id: TaskId,
        limiter: &Limiter,
        limits: &SendLimits,
        target: &RepeaterTarget,
        baseline: &StoredExchange,
        probe: &PlannedProbe,
        point_name: &str,
        item_idx: usize,
        total_items: usize,
        summary: &mut ScanSummary,
        log: &mut Vec<(String, String)>,
        logged_decisions: &mut u64,
    ) -> Result<Option<(i64, StoredExchange)>, AppError> {
        let req_bytes = match crate::repeater::request_bytes(&probe.request_text, true, true, true)
        {
            Ok(b) => b,
            Err(e) => {
                summary.active_skipped += 1;
                active_note(
                    log,
                    logged_decisions,
                    "warning",
                    format!("Skipped probe {} on #{}: {e}", probe.label, baseline.row.id),
                );
                return Ok(None);
            }
        };
        let https = target.scheme == "https";
        let security = match self.security_for_host(&target.host, https).await {
            Ok(s) => s,
            Err(e) => {
                summary.active_errors += 1;
                active_note(
                    log,
                    logged_decisions,
                    "warning",
                    format!("Active probe {} on #{}: {e}", probe.label, baseline.row.id),
                );
                return Ok(None);
            }
        };
        let _permit = limiter.acquire().await;
        if !self.is_target_in_active_scope(target, &baseline.row.path) {
            summary.out_of_scope += 1;
            summary.active_skipped += 1;
            return Ok(None);
        }
        let destination = Destination {
            host: target.host.clone(),
            port: target.port,
            tls_name: target.host.clone(),
        };
        let started_at = now_ms();
        let outcome = self
            .session_send(
                "crawler",
                Source::Scanner,
                &destination,
                &security,
                &req_bytes,
                limits,
            )
            .await;
        let mut exchange =
            crate::repeater::captured(Source::Scanner, target, req_bytes, started_at, &outcome);
        let probe_note = format!(
            "Active scan ({}): {} on '{}' (baseline #{}).",
            probe.check_id, probe.label, point_name, baseline.row.id
        );
        exchange.notes = Some(match exchange.notes.take() {
            Some(n) => format!("{probe_note} {n}"),
            None => probe_note,
        });
        match &outcome {
            Ok(s) => {
                summary.active_requests += 1;
                active_note(
                    log,
                    logged_decisions,
                    "info",
                    format!(
                        "Probe {} ({}) on #{} '{}' → {}",
                        probe.check_id, probe.label, baseline.row.id, point_name, s.head.status
                    ),
                );
            }
            Err(e) => {
                summary.active_errors += 1;
                active_note(
                    log,
                    logged_decisions,
                    "warning",
                    format!(
                        "Probe {} ({}) on #{} '{}' failed: {}",
                        probe.check_id, probe.label, baseline.row.id, point_name, e.message
                    ),
                );
            }
        }
        let probe_id = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || db.insert(exchange)).await??
        };
        let progress = TaskProgress {
            requests: summary.crawl_requests + summary.active_requests + summary.active_errors,
            errors: summary.crawl_errors + summary.active_errors,
            discovered: summary.items_read,
            queued: total_items.saturating_sub(item_idx + 1) as u64,
            progress: format!(
                "active: {} sent ({} of {total_items} items)",
                summary.active_requests,
                item_idx + 1
            ),
        };
        {
            let db = db.clone();
            tokio::task::spawn_blocking(move || db.task_progress(id, progress)).await??;
        }
        if outcome.is_err() {
            return Ok(None);
        }
        let stored = {
            let db = db.clone();
            let mut rows =
                tokio::task::spawn_blocking(move || db.exchanges_bulk(&[probe_id])).await??;
            rows.pop()
        };
        Ok(stored.map(|ex| (probe_id, ex)))
    }

    async fn save_audit_progress(
        &self,
        db: &ProjectDb,
        id: TaskId,
        counts: &AuditCounts,
    ) -> Result<(), AppError> {
        let db = db.clone();
        let discovered = counts.read;
        let progress = format!(
            "audit: {} audited, {} duplicates",
            counts.audited, counts.duplicates
        );
        tokio::task::spawn_blocking(move || db.task_progress_note(id, discovered, progress))
            .await??;
        Ok(())
    }
}

fn active_note(log: &mut Vec<(String, String)>, logged: &mut u64, level: &str, message: String) {
    *logged += 1;
    if *logged <= LOGGED_DECISIONS {
        log.push((level.to_string(), message));
    } else if *logged == LOGGED_DECISIONS + 1 {
        log.push((
            "info".into(),
            "More active probe decisions are only counted.".into(),
        ));
    }
}

/// Logs an audit decision (the first ones; later ones are only counted).
fn decision(log: &mut Vec<(String, String)>, counts: &mut AuditCounts, message: String) {
    counts.logged += 1;
    if counts.logged <= LOGGED_DECISIONS {
        log.push(("info".into(), message));
    } else if counts.logged == LOGGED_DECISIONS + 1 {
        log.push((
            "info".into(),
            "More audit decisions are only counted.".into(),
        ));
    }
}

/// `scheme://host:port/path` without query, as a prefix to match stored
/// URLs (an origin without a path matches everything under it).
fn prefix_of(url: &str) -> String {
    let cut = url.find(['?', '#']).unwrap_or(url.len());
    url[..cut].to_string()
}

/// Findings of the JavaScript analysis for one exchange (phase 3).
pub(crate) fn javascript_issues(ex: &StoredExchange, disabled: &[String]) -> Vec<PassiveIssue> {
    let message = message_of(ex);
    analyze_javascript(&message, |id| !disabled.iter().any(|d| d == id))
        .into_iter()
        .map(|issue| passive_issue(ex.row.id, issue))
        .collect()
}
