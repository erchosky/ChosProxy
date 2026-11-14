//! Live tasks (M26, matrix SCANNER-009): tasks that follow the traffic as it
//! is stored, until they are stopped.
//!
//! - **Live passive audit** runs the passive, JavaScript, cross-request and
//!   script checks on matching traffic, with its own selection of checks.
//! - **Live passive crawl** reads the links, forms and scripts in matching
//!   responses and lists the URLs nobody has requested yet (and marks them
//!   once someone does).
//!
//! Both only read stored traffic: a live task never sends a request. They
//! keep where they got to in their configuration, so a live task running
//! when the project closes carries on when it is opened again.

use super::passive_ops::{cross_issues, issues_in, message_of};
use super::task_ops::{Control, PROGRESS_EVERY, target_of};
use super::{Engine, read};
use crate::AppError;
use chosproxy_core::event::EngineEvent;
use chosproxy_core::scope::UrlParts;
use chosproxy_core::time::now_ms;
use chosproxy_crawler::{FoundKind, discover};
use chosproxy_scanner::CrossAnalyzer;
use chosproxy_storage::{
    DiscoveryHit, PassiveIssue, ProjectDb, StoredExchange, TaskId, TaskProgress, TaskRow,
    TaskStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use ts_rs::TS;
use url::Url;

/// Exchanges read per round.
const BATCH: u32 = 200;
/// How often an idle live task looks for new traffic.
const POLL: Duration = Duration::from_millis(1000);
/// URLs of the project read to know what was requested already.
const MAX_KNOWN_URLS: u32 = 500_000;
/// Unrequested URLs listed at most per live crawl.
const MAX_HITS: u64 = 20_000;
/// Sources a live task can follow.
pub const LIVE_SOURCES: [&str; 8] = [
    "proxy",
    "repeater",
    "fuzzer",
    "crawler",
    "scanner",
    "sequencer",
    "plugin",
    "discovery",
];

/// What a live task does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LiveMode {
    /// Run the passive checks on new traffic.
    #[default]
    Audit,
    /// List the URLs that new responses link to and nobody requested.
    Crawl,
}

/// The settings of a live task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct LiveTaskConfig {
    /// Name shown in the task list.
    pub name: String,
    /// Audit or crawl.
    pub mode: LiveMode,
    /// Traffic sources to follow (`proxy`, `repeater`…).
    pub sources: Vec<String>,
    /// Only in-scope traffic (when a scope is defined).
    pub in_scope_only: bool,
    /// Only URLs containing this text (case-insensitive; empty = all).
    pub url_contains: String,
    /// Checks turned off for this task (audit).
    pub disabled_checks: Vec<String>,
    /// Start with the traffic already stored instead of only new traffic.
    pub include_existing: bool,
    /// Where the task got to (last exchange id read); kept by the task.
    #[ts(type = "number")]
    pub resume_after: i64,
}

impl Default for LiveTaskConfig {
    fn default() -> LiveTaskConfig {
        LiveTaskConfig {
            name: String::new(),
            mode: LiveMode::Audit,
            sources: vec!["proxy".into()],
            in_scope_only: true,
            url_contains: String::new(),
            disabled_checks: Vec::new(),
            include_existing: false,
            resume_after: 0,
        }
    }
}

impl LiveTaskConfig {
    /// Checks the settings.
    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.chars().count() > 80 {
            return Err("the name is at most 80 characters".into());
        }
        if self.sources.is_empty() {
            return Err("choose at least one traffic source".into());
        }
        if let Some(bad) = self
            .sources
            .iter()
            .find(|s| !LIVE_SOURCES.contains(&s.as_str()))
        {
            return Err(format!("unknown traffic source \"{bad}\""));
        }
        if self.url_contains.len() > 500 {
            return Err("the URL filter is at most 500 characters".into());
        }
        if self.disabled_checks.len() > 500 {
            return Err("too many checks turned off".into());
        }
        Ok(())
    }

    fn default_name(&self) -> String {
        let what = match self.mode {
            LiveMode::Audit => "Live passive audit",
            LiveMode::Crawl => "Live passive crawl",
        };
        format!("{what} ({})", self.sources.join(", "))
    }

    fn describe(&self, scope_defined: bool) -> String {
        let mut parts = vec![format!("following {} traffic", self.sources.join(", "))];
        if self.in_scope_only && scope_defined {
            parts.push("in scope only".into());
        }
        if !self.url_contains.trim().is_empty() {
            parts.push(format!("URLs containing “{}”", self.url_contains.trim()));
        }
        if self.mode == LiveMode::Audit && !self.disabled_checks.is_empty() {
            parts.push(format!("{} check(s) off", self.disabled_checks.len()));
        }
        parts.join(", ")
    }
}

/// The URL as compared between links and requests (no fragment, default
/// port dropped).
fn normalize(url: &str) -> Option<String> {
    let mut u = Url::parse(url).ok()?;
    u.set_fragment(None);
    Some(u.to_string())
}

/// What a live crawl knows.
struct CrawlState {
    /// URLs requested (as normalized).
    requested: HashSet<String>,
    /// URLs listed as not requested yet.
    listed: HashSet<String>,
    hits: u64,
}

/// Counters of a live task.
#[derive(Default)]
struct Counts {
    read: u64,
    matched: u64,
    found: u64,
    requested_later: u64,
}

impl Engine {
    /// Starts a live task.
    pub async fn live_task_start(&self, mut config: LiveTaskConfig) -> Result<TaskRow, AppError> {
        config.validate().map_err(AppError::Invalid)?;
        config.name = config.name.trim().to_string();
        if config.name.is_empty() {
            config.name = config.default_name();
        }
        let db = self.db()?;
        config.resume_after = if config.include_existing {
            0
        } else {
            tokio::task::spawn_blocking(move || db.last_exchange_id()).await??
        };
        let json = serde_json::to_string(&config)
            .map_err(|e| AppError::Invalid(format!("invalid live task settings: {e}")))?;
        let name = config.name.clone();
        self.launch_task("live", "Live", name, json, move |engine, db, id, rx| async move {
            engine.run_live(&db, id, config, rx, false).await
        })
        .await
    }

    /// Starts again the live tasks a previous session left running or
    /// paused (called when a project is opened).
    pub(crate) async fn resume_live_tasks(&self, db: &ProjectDb) {
        let rows = {
            let db = db.clone();
            match tokio::task::spawn_blocking(move || db.tasks_live_unfinished()).await {
                Ok(Ok(rows)) => rows,
                _ => return,
            }
        };
        for row in rows {
            let Ok(config) = serde_json::from_str::<LiveTaskConfig>(&row.config) else {
                let db = db.clone();
                let id = row.id;
                let _ = tokio::task::spawn_blocking(move || {
                    db.task_set_status(id, TaskStatus::Failed, Some("unreadable settings".into()))
                })
                .await;
                continue;
            };
            let initial = if row.status == TaskStatus::Paused {
                Control::Pause
            } else {
                Control::Run
            };
            self.run_task(
                db.clone(),
                row,
                "live",
                "Live",
                initial,
                move |engine, db, id, rx| async move {
                    engine.run_live(&db, id, config, rx, true).await
                },
            );
        }
    }

    async fn run_live(
        &self,
        db: &ProjectDb,
        id: TaskId,
        mut config: LiveTaskConfig,
        mut control: watch::Receiver<Control>,
        resumed: bool,
    ) -> Result<TaskStatus, AppError> {
        let scope_defined = !read(&self.inner.scope).is_empty();
        let mut log = vec![(
            "info".into(),
            if resumed {
                format!(
                    "Resumed after the project was opened, from exchange #{}.",
                    config.resume_after
                )
            } else {
                format!(
                    "{}: {}; {}. Nothing is sent.",
                    match config.mode {
                        LiveMode::Audit => "Live passive audit",
                        LiveMode::Crawl => "Live passive crawl",
                    },
                    config.describe(scope_defined),
                    if config.include_existing {
                        "starting with the traffic already stored"
                    } else {
                        "new traffic only"
                    }
                )
            },
        )];
        let mut crawl = if config.mode == LiveMode::Crawl {
            Some(self.crawl_state(db, id, config.resume_after).await?)
        } else {
            None
        };
        let mut cross = CrossAnalyzer::new();
        let mut counts = Counts::default();
        let mut paused = false;
        let mut first = true;
        let mut last_progress = Instant::now();
        let mut dirty = false;
        let status = loop {
            let wanted = *control.borrow_and_update();
            match wanted {
                Control::Cancel => {
                    log.push(("info".into(), "Stopped.".into()));
                    break TaskStatus::Cancelled;
                }
                Control::Suspend => {
                    break if paused {
                        TaskStatus::Paused
                    } else {
                        TaskStatus::Running
                    };
                }
                Control::Pause | Control::Run => {}
            }
            let want_pause = wanted == Control::Pause;
            if want_pause != paused || first {
                if !first {
                    log.push((
                        "info".into(),
                        if want_pause {
                            "Paused.".into()
                        } else {
                            "Resumed.".into()
                        },
                    ));
                }
                first = false;
                paused = want_pause;
                self.set_status(
                    db,
                    id,
                    if paused {
                        TaskStatus::Paused
                    } else {
                        TaskStatus::Running
                    },
                )
                .await?;
            }
            if paused {
                self.save_live(db, id, &config, &counts, &mut log).await?;
                if control.changed().await.is_err() {
                    break TaskStatus::Cancelled;
                }
                continue;
            }
            let ids = {
                let db = db.clone();
                let after = config.resume_after;
                tokio::task::spawn_blocking(move || db.exchange_ids_after(after, BATCH)).await??
            };
            if let Some(&last) = ids.last() {
                let exchanges = {
                    let db = db.clone();
                    tokio::task::spawn_blocking(move || db.exchanges_bulk(&ids)).await??
                };
                self.live_batch(
                    db,
                    id,
                    &config,
                    &exchanges,
                    crawl.as_mut(),
                    &mut cross,
                    &mut counts,
                    &mut log,
                )
                .await?;
                config.resume_after = last;
                dirty = true;
                if last_progress.elapsed() >= PROGRESS_EVERY {
                    last_progress = Instant::now();
                    self.save_live(db, id, &config, &counts, &mut log).await?;
                    dirty = false;
                }
                continue;
            }
            if dirty {
                self.save_live(db, id, &config, &counts, &mut log).await?;
                dirty = false;
            }
            // Caught up: wait for new traffic (polled) or a control change.
            tokio::select! {
                () = tokio::time::sleep(POLL) => {}
                changed = control.changed() => {
                    if changed.is_err() {
                        break TaskStatus::Cancelled;
                    }
                }
            }
        };
        if status == TaskStatus::Cancelled {
            log.push((
                "info".into(),
                format!(
                    "Read {} item(s), {} matched; {}.",
                    counts.read,
                    counts.matched,
                    match config.mode {
                        LiveMode::Audit => format!("{} finding(s) created", counts.found),
                        LiveMode::Crawl => format!(
                            "{} URL(s) found not requested, {} of them requested later",
                            counts.found, counts.requested_later
                        ),
                    }
                ),
            ));
        }
        self.save_live(db, id, &config, &counts, &mut log).await?;
        Ok(status)
    }

    /// What a live crawl needs to know at its start: the URLs requested
    /// up to where it starts, and the ones it listed before (resumed).
    async fn crawl_state(
        &self,
        db: &ProjectDb,
        id: TaskId,
        up_to: i64,
    ) -> Result<CrawlState, AppError> {
        let db = db.clone();
        Ok(tokio::task::spawn_blocking(move || {
            let requested = db
                .exchange_urls(up_to, MAX_KNOWN_URLS)?
                .iter()
                .filter_map(|u| normalize(u))
                .collect();
            let hits = db.discovery_results(id)?;
            let listed: HashSet<String> = hits.iter().map(|h| h.url.clone()).collect();
            Ok::<_, chosproxy_storage::StorageError>(CrawlState {
                requested,
                hits: listed.len() as u64,
                listed,
            })
        })
        .await??)
    }

    #[allow(clippy::too_many_arguments)]
    async fn live_batch(
        &self,
        db: &ProjectDb,
        id: TaskId,
        config: &LiveTaskConfig,
        exchanges: &[StoredExchange],
        crawl: Option<&mut CrawlState>,
        cross: &mut CrossAnalyzer,
        counts: &mut Counts,
        log: &mut Vec<(String, String)>,
    ) -> Result<(), AppError> {
        let scope = read(&self.inner.scope);
        let filter = config.url_contains.trim().to_lowercase();
        let matching: Vec<&StoredExchange> = exchanges
            .iter()
            .filter(|ex| {
                counts.read += 1;
                let row = &ex.row;
                if !config.sources.iter().any(|s| s == row.source.as_str()) {
                    return false;
                }
                if !filter.is_empty() && !row.url.to_lowercase().contains(&filter) {
                    return false;
                }
                if config.in_scope_only && !scope.is_empty() {
                    let parts = UrlParts {
                        scheme: row.scheme.clone(),
                        host: row.host.clone(),
                        port: row.port,
                        path: row.path.clone(),
                    };
                    if !scope.contains(&parts.target()) {
                        return false;
                    }
                }
                true
            })
            .collect();
        counts.matched += matching.len() as u64;
        match crawl {
            None => {
                let checks = self.library().checks();
                let mut issues: Vec<PassiveIssue> = Vec::new();
                for ex in matching.iter().filter(|ex| ex.response.is_some()) {
                    issues.extend(issues_in(ex, &config.disabled_checks));
                    issues.extend(super::scan_ops::javascript_issues(
                        ex,
                        &config.disabled_checks,
                    ));
                    issues.extend(cross_issues(cross, ex, &config.disabled_checks));
                    issues.extend(super::script_ops::script_issues(ex, &checks));
                }
                if !issues.is_empty() {
                    let db = db.clone();
                    let recorded =
                        tokio::task::spawn_blocking(move || db.findings_record_auto(issues))
                            .await??;
                    counts.found += u64::from(recorded.created);
                    if recorded.created + recorded.updated > 0 {
                        let _ = self.inner.events.send(EngineEvent::FindingsChanged {
                            created: recorded.created,
                        });
                    }
                }
            }
            Some(state) => {
                self.live_crawl_batch(db, id, &scope, &matching, state, counts, log)
                    .await?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn live_crawl_batch(
        &self,
        db: &ProjectDb,
        id: TaskId,
        scope: &chosproxy_core::Scope,
        matching: &[&StoredExchange],
        state: &mut CrawlState,
        counts: &mut Counts,
        log: &mut Vec<(String, String)>,
    ) -> Result<(), AppError> {
        let mut hits = Vec::new();
        let mut requested_now = Vec::new();
        for ex in matching {
            let Some(own) = normalize(&ex.row.url) else {
                continue;
            };
            if state.requested.insert(own.clone()) && state.listed.contains(&own) {
                requested_now.push((own.clone(), ex));
            }
            let message = message_of(ex);
            let (Some(head), Ok(page)) = (message.response.as_ref(), Url::parse(&own)) else {
                continue;
            };
            let mut found_here = 0u32;
            for found in discover(&page, head, &message.body, false) {
                if found.kind == FoundKind::Resource {
                    continue;
                }
                let allowed = if scope.is_empty() {
                    found.url.host_str() == page.host_str()
                } else {
                    let t = target_of(&found.url);
                    let parts = UrlParts {
                        scheme: t.scheme,
                        host: t.host,
                        port: t.port,
                        path: super::task_ops::request_target(&found.url),
                    };
                    scope.contains(&parts.target())
                };
                let url = found.url.to_string();
                if !allowed
                    || state.requested.contains(&url)
                    || state.listed.contains(&url)
                    || state.hits >= MAX_HITS
                {
                    continue;
                }
                state.listed.insert(url.clone());
                state.hits += 1;
                found_here += 1;
                let kind = if found.kind == FoundKind::Form && found.method != "GET" {
                    format!("form ({})", found.method)
                } else {
                    found.kind.label().to_string()
                };
                hits.push(DiscoveryHit {
                    id: 0,
                    found_at: now_ms(),
                    url,
                    kind,
                    status: 0,
                    length: 0,
                    exchange_id: Some(ex.row.id),
                });
                if state.hits == MAX_HITS {
                    log.push((
                        "warning".into(),
                        format!("Listed {MAX_HITS} URLs; more are not recorded."),
                    ));
                }
            }
            if found_here > 0 {
                counts.found += u64::from(found_here);
            }
        }
        let db = db.clone();
        let marks: Vec<(String, u16, u64)> = requested_now
            .iter()
            .map(|(url, ex)| {
                (
                    url.clone(),
                    ex.row.status.unwrap_or(0),
                    ex.response.as_ref().map_or(0, |r| r.len() as u64),
                )
            })
            .collect();
        let marked = tokio::task::spawn_blocking(move || {
            if !hits.is_empty() {
                db.discovery_record(id, hits)?;
            }
            let mut marked = 0u64;
            for (url, status, length) in marks {
                // A request without a response keeps status 0 as "not
                // answered"; one with a response is marked.
                if status > 0 && db.discovery_mark_requested(id, url, status, length)? {
                    marked += 1;
                }
            }
            Ok::<_, chosproxy_storage::StorageError>(marked)
        })
        .await??;
        counts.requested_later += marked;
        Ok(())
    }

    async fn save_live(
        &self,
        db: &ProjectDb,
        id: TaskId,
        config: &LiveTaskConfig,
        counts: &Counts,
        log: &mut Vec<(String, String)>,
    ) -> Result<(), AppError> {
        let json = serde_json::to_string(config)
            .map_err(|e| AppError::Invalid(format!("invalid live task settings: {e}")))?;
        let progress = TaskProgress {
            requests: 0,
            errors: 0,
            discovered: counts.found,
            queued: 0,
            progress: format!(
                "{} read, {} matched, at #{}",
                counts.read, counts.matched, config.resume_after
            ),
        };
        let lines = std::mem::take(log);
        let db = db.clone();
        tokio::task::spawn_blocking(move || {
            db.task_set_config(id, json)?;
            db.task_progress(id, progress)?;
            db.task_log(id, lines)
        })
        .await??;
        let _ = self.inner.events.send(EngineEvent::TasksChanged);
        Ok(())
    }
}
