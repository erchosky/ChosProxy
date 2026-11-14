//! Tasks (spec §32) and the crawler (spec §13, M10).
//!
//! A task runs as a tokio task controlled through a watch channel (run,
//! pause, cancel); its status, counters and log are kept in the project. The
//! crawler fetches pages breadth first with GET only, within the scope (or,
//! while no scope is defined, the start URLs' origins), never follows links
//! that look state-changing (log out, delete…) and never submits POST forms.

use super::{Engine, limits_of, lock, read};
use crate::AppError;
use crate::cookies;
use crate::repeater;
use chosproxy_core::Source;
use chosproxy_core::event::{EngineEvent, NoticeLevel};
use chosproxy_core::scope::UrlParts;
use chosproxy_core::time::now_ms;
use chosproxy_crawler::{
    CrawlConfig, Entry, FoundKind, Frontier, Push, discover, robots_and_sitemap_paths,
};
use chosproxy_http::ResponseHead;
use chosproxy_http::content::{body_text, split_message};
use chosproxy_request::{Destination, Limiter, SendError, SendLimits, Sent};
use chosproxy_storage::{
    ProjectDb, RepeaterTarget, StorageError, TaskId, TaskLogEntry, TaskProgress, TaskRow,
    TaskStatus,
};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use ts_rs::TS;
use url::Url;

/// What the user asked a running task to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Control {
    Run,
    Pause,
    Cancel,
    /// The project is closing: a live task stops here and resumes when the
    /// project is opened again (only sent to live tasks).
    Suspend,
}

/// A task running in this session.
pub(crate) struct RunningTask {
    control: watch::Sender<Control>,
    pub(super) kind: &'static str,
    pub(super) name: String,
    /// Tells this run apart from a later run of the same task (a live task
    /// suspended and resumed).
    run: Arc<()>,
    /// A resource pool chosen for this task (M31b): its sends go through
    /// it instead of the pool assigned to the tool.
    pool: Option<String>,
    abort: tokio::task::AbortHandle,
}

impl RunningTask {
    pub(super) fn abort(&self) {
        self.abort.abort();
    }
}

tokio::task_local! {
    /// The task whose work the current future does.
    static CURRENT_TASK: TaskId;
    /// The resource pool chosen in a launch dialog for the task the current
    /// call starts (see [`Engine::with_launch_pool`]).
    static LAUNCH_POOL: String;
}

/// The task the current future works for, if any.
pub(super) fn current_task() -> Option<TaskId> {
    CURRENT_TASK.try_with(|id| *id).ok()
}

/// Runs `f` as part of `task` (for work a task spawns: spawned futures do
/// not inherit the task they came from).
pub(super) async fn in_task<F: std::future::Future>(task: Option<TaskId>, f: F) -> F::Output {
    match task {
        Some(id) => CURRENT_TASK.scope(id, f).await,
        None => f.await,
    }
}

/// A task with its log.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskDetail {
    /// The task.
    pub row: TaskRow,
    /// Its log, oldest first.
    pub logs: Vec<TaskLogEntry>,
    /// The resource pool chosen for it while it runs (none: the pool
    /// assigned to its tool, if any).
    pub pool: Option<String>,
}

/// How often a running crawl saves its counters.
pub(super) const PROGRESS_EVERY: Duration = Duration::from_millis(1000);
/// Errors logged one by one per crawl (later ones are only counted).
pub(super) const LOGGED_ERRORS: u64 = 50;
/// Decoded bytes of a response read for links.
const DISCOVER_LIMIT: usize = 4 * 1024 * 1024;

fn origin(url: &Url) -> (String, String, u16) {
    (
        url.scheme().to_string(),
        url.host_str().unwrap_or_default().to_ascii_lowercase(),
        url.port_or_known_default().unwrap_or(0),
    )
}

/// The target (scheme, host, port) of a URL.
pub(super) fn target_of(url: &Url) -> RepeaterTarget {
    let (scheme, host, port) = origin(url);
    RepeaterTarget {
        scheme,
        host: host.trim_matches(['[', ']']).to_string(),
        port,
    }
}

/// Path and query of a URL as sent in the request line.
pub(super) fn request_target(url: &Url) -> String {
    match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_string(),
    }
}

/// A browser-like GET request.
fn crawl_request(url: &Url, user_agent: &str, cookie: Option<&str>) -> Vec<u8> {
    let host = match (url.host_str(), url.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_string(),
        _ => String::new(),
    };
    let mut r = format!(
        "GET {} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {user_agent}\r\n\
         Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r\n\
         Accept-Encoding: gzip, deflate\r\nConnection: close\r\n",
        request_target(url)
    );
    if let Some(c) = cookie.filter(|c| !c.is_empty()) {
        r.push_str(&format!("Cookie: {c}\r\n"));
    }
    r.push_str("\r\n");
    r.into_bytes()
}

/// Why the crawler requested a URL, for the history notes.
pub(super) fn crawl_reason(entry: &Entry) -> String {
    match (entry.kind, entry.via) {
        (FoundKind::Robots, None) => "Crawler: robots.txt or sitemap of a start site.".to_string(),
        (_, None) => "Crawler: start URL.".to_string(),
        (FoundKind::Redirect, Some(page)) => {
            format!("Crawler: redirected from #{page} (depth {}).", entry.depth)
        }
        (kind, Some(page)) => {
            let how = match kind {
                FoundKind::Link => "a link",
                FoundKind::Script => "a script reference",
                FoundKind::Form => "a GET form",
                FoundKind::Redirect => "a redirect",
                FoundKind::JsEndpoint => "a URL in a script",
                FoundKind::Robots => "a robots.txt or sitemap entry",
                FoundKind::Resource => "a resource",
            };
            format!("Crawler: {how} on #{page} (depth {}).", entry.depth)
        }
    }
}

/// The outcome of one fetch.
pub(super) struct Fetched {
    pub(super) entry: Entry,
    pub(super) request: Vec<u8>,
    pub(super) started_at: i64,
    pub(super) outcome: Result<Sent, SendError>,
}

impl Engine {
    /// Tasks of the open project, newest first.
    pub async fn tasks(&self) -> Result<Vec<TaskRow>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.tasks()).await??)
    }

    /// A task with its log.
    pub async fn task_detail(&self, id: TaskId) -> Result<TaskDetail, AppError> {
        let db = self.db()?;
        let pool = self.task_pool(id);
        Ok(tokio::task::spawn_blocking(move || {
            let row = db.task(id)?.ok_or(StorageError::NotFound(id))?;
            Ok::<_, StorageError>(TaskDetail {
                row,
                logs: db.task_logs(id)?,
                pool,
            })
        })
        .await??)
    }

    /// The resource pool chosen for a running task.
    pub(super) fn task_pool(&self, id: TaskId) -> Option<String> {
        lock(&self.inner.tasks_running)
            .get(&id)
            .and_then(|t| t.pool.clone())
    }

    /// Chooses the resource pool a running task sends through, from its
    /// next request on (`None`: back to the pool assigned to its tool).
    /// The task's own limits still apply. Kept while the task runs.
    pub async fn task_set_pool(&self, id: TaskId, pool: Option<String>) -> Result<(), AppError> {
        let pool = self.known_pool(pool)?;
        {
            let mut running = lock(&self.inner.tasks_running);
            let task = running
                .get_mut(&id)
                .ok_or_else(|| AppError::Invalid("the task is not running".into()))?;
            task.pool = pool.clone();
        }
        let message = match &pool {
            Some(name) => format!("Resource pool chosen for this task: {name}."),
            None => "Resource pool: back to the tool's own assignment.".to_string(),
        };
        if let Ok(db) = self.db() {
            tokio::task::spawn_blocking(move || db.task_log(id, vec![("info".into(), message)]))
                .await??;
        }
        let _ = self.inner.events.send(EngineEvent::TasksChanged);
        Ok(())
    }

    /// A pool name as typed, trimmed, checked against the configured pools
    /// (empty: none).
    fn known_pool(&self, pool: Option<String>) -> Result<Option<String>, AppError> {
        let pool = pool.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        if let Some(name) = &pool
            && !self
                .pools_config()
                .pools
                .iter()
                .any(|p| p.name.trim().eq_ignore_ascii_case(name))
        {
            return Err(AppError::Invalid(format!(
                "no resource pool named {name:?}"
            )));
        }
        Ok(pool)
    }

    /// Runs `launch`, a call that starts a task (crawl, scan, discovery,
    /// Fuzzer, API send), so that the task sends through `pool` from its
    /// first request instead of the pool assigned to its tool. `None` or an
    /// empty name changes nothing; an unknown name is refused before
    /// anything starts.
    pub async fn with_launch_pool<F: std::future::Future>(
        &self,
        pool: Option<String>,
        launch: F,
    ) -> Result<F::Output, AppError> {
        Ok(match self.known_pool(pool)? {
            Some(name) => LAUNCH_POOL.scope(name, launch).await,
            None => launch.await,
        })
    }

    fn control(&self, id: TaskId, control: Control) -> Result<(), AppError> {
        let running = lock(&self.inner.tasks_running);
        let task = running
            .get(&id)
            .ok_or_else(|| AppError::Invalid("the task is not running".into()))?;
        let _ = task.control.send(control);
        Ok(())
    }

    /// Pauses a running task (requests in flight finish).
    pub fn task_pause(&self, id: TaskId) -> Result<(), AppError> {
        self.control(id, Control::Pause)
    }

    /// Resumes a paused task.
    pub fn task_resume(&self, id: TaskId) -> Result<(), AppError> {
        self.control(id, Control::Run)
    }

    /// Stops a task for good.
    pub fn task_cancel(&self, id: TaskId) -> Result<(), AppError> {
        self.control(id, Control::Cancel)
    }

    /// Deletes a finished task and its log (its traffic stays).
    pub async fn task_delete(&self, id: TaskId) -> Result<(), AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.task_delete(id)).await??;
        let _ = self.inner.events.send(EngineEvent::TasksChanged);
        Ok(())
    }

    /// Stops every running task (closing the project) and records them as
    /// cancelled.
    pub(crate) async fn tasks_stop_all(&self, db: &ProjectDb) {
        let running: Vec<(TaskId, RunningTask)> = lock(&self.inner.tasks_running).drain().collect();
        for (id, task) in running {
            if task.kind == "live" {
                let _ = task.control.send(Control::Suspend);
                self.task_log_lines(
                    db,
                    id,
                    vec![(
                        "info".into(),
                        "Suspended because the project was closed; it resumes when the project \
                         is opened again."
                            .into(),
                    )],
                )
                .await;
                continue;
            }
            let _ = task.control.send(Control::Cancel);
            let db = db.clone();
            let _ = tokio::task::spawn_blocking(move || {
                db.task_set_status(id, TaskStatus::Cancelled, None)?;
                db.task_log(
                    id,
                    vec![(
                        "warning".into(),
                        "Stopped because the project was closed.".into(),
                    )],
                )
            })
            .await;
        }
    }

    /// Default crawl settings.
    pub fn crawl_defaults(&self) -> CrawlConfig {
        CrawlConfig::default()
    }

    /// Starts a crawl task.
    pub async fn crawl_start(&self, config: CrawlConfig) -> Result<TaskRow, AppError> {
        let start = config.validate().map_err(AppError::Invalid)?;
        let first = start
            .first()
            .and_then(|u| u.host_str())
            .unwrap_or("target")
            .to_string();
        let name = if start.len() > 1 {
            format!("Crawl {first} and {} more", start.len() - 1)
        } else {
            format!("Crawl {first}")
        };
        let json = serde_json::to_string(&config)
            .map_err(|e| AppError::Invalid(format!("invalid crawl settings: {e}")))?;
        self.launch_task("crawl", "Crawl", name, json, move |engine, db, id, rx| async move {
            engine.run_crawl(&db, id, config, start, rx).await
        })
        .await
    }

    /// Creates a task and runs `run` for it in the background, recording how
    /// it ends (`label` names the kind in messages).
    pub(super) async fn launch_task<F, Fut>(
        &self,
        kind: &'static str,
        label: &'static str,
        name: String,
        config_json: String,
        run: F,
    ) -> Result<TaskRow, AppError>
    where
        F: FnOnce(Engine, ProjectDb, TaskId, watch::Receiver<Control>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<TaskStatus, AppError>> + Send + 'static,
    {
        self.ensure_writable()?;
        let db = self.db()?;
        let task = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || db.task_create(kind, &name, &config_json)).await??
        };
        let pool = LAUNCH_POOL.try_with(Clone::clone).ok();
        if let Some(name) = &pool {
            let db = db.clone();
            let line = format!("Resource pool chosen for this task: {name}.");
            let id = task.id;
            tokio::task::spawn_blocking(move || db.task_log(id, vec![("info".into(), line)]))
                .await??;
        }
        // `run_task` takes the launch pool before the task's first request.
        self.audit(
            &format!("task.{kind}.started"),
            &format!("task #{}: {}", task.id, task.name),
        );
        self.run_task(db, task.clone(), kind, label, Control::Run, run);
        Ok(task)
    }

    /// Runs `run` for an existing task in the background (`initial`: run or
    /// start paused), recording how it ends.
    pub(super) fn run_task<F, Fut>(
        &self,
        db: ProjectDb,
        task: TaskRow,
        kind: &'static str,
        label: &'static str,
        initial: Control,
        run: F,
    ) where
        F: FnOnce(Engine, ProjectDb, TaskId, watch::Receiver<Control>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<TaskStatus, AppError>> + Send + 'static,
    {
        let (tx, rx) = watch::channel(initial);
        let this_run = Arc::new(());
        let this_run_task = this_run.clone();
        let engine = self.internal_clone();
        let id = task.id;
        let task_name = task.name.clone();
        let join = tokio::spawn(async move {
            let result = in_task(Some(id), run(engine.clone(), db.clone(), id, rx)).await;
            {
                let mut running = lock(&engine.inner.tasks_running);
                if running
                    .get(&id)
                    .is_some_and(|t| Arc::ptr_eq(&t.run, &this_run_task))
                {
                    running.remove(&id);
                }
            }
            engine.write_session_marker();
            let (status, error) = match result {
                Ok(status) => (status, None),
                Err(e) => (TaskStatus::Failed, Some(e.to_string())),
            };
            let finished = {
                let db = db.clone();
                let error = error.clone();
                tokio::task::spawn_blocking(move || {
                    db.task_set_status(id, status, error.clone())?;
                    if let Some(e) = error {
                        db.task_log(id, vec![("error".into(), format!("The task failed: {e}"))])?;
                    }
                    Ok::<_, StorageError>(())
                })
                .await
            };
            if let Ok(Err(e)) = finished {
                tracing::warn!(error = %e, task = id, "could not record the end of a task");
            }
            let outcome = match status {
                TaskStatus::Completed => "completed",
                TaskStatus::Cancelled => "cancelled",
                TaskStatus::Failed => "failed",
                _ => "finished",
            };
            let detail = match &error {
                Some(e) => format!("task #{id} ({task_name}): {e}"),
                None => format!("task #{id} ({task_name})"),
            };
            engine.audit(&format!("task.{kind}.{outcome}"), &detail);
            if status == TaskStatus::Failed {
                engine.notice(
                    NoticeLevel::Warning,
                    format!(
                        "{label} task #{id} failed{}",
                        error.map(|e| format!(": {e}")).unwrap_or_default()
                    ),
                );
            }
            let _ = engine.inner.events.send(EngineEvent::TasksChanged);
        });
        lock(&self.inner.tasks_running).insert(
            task.id,
            RunningTask {
                control: tx,
                kind,
                name: task.name.clone(),
                run: this_run,
                pool: LAUNCH_POOL.try_with(Clone::clone).ok(),
                abort: join.abort_handle(),
            },
        );
        self.write_session_marker();
        let _ = self.inner.events.send(EngineEvent::TasksChanged);
    }

    /// Refuses a tool's target that the scope leaves out, when a scope is
    /// defined (with none, any target the tester typed is accepted, as in
    /// the Repeater). `path` is the request-line target.
    pub(super) fn ensure_in_scope(
        &self,
        target: &RepeaterTarget,
        path: &str,
        tool: &str,
    ) -> Result<(), AppError> {
        let scope = read(&self.inner.scope);
        if scope.is_empty() {
            return Ok(());
        }
        let path = if path.starts_with('/') { path } else { "/" };
        let in_scope = scope.contains(&chosproxy_core::scope::ScopeTarget {
            scheme: &target.scheme,
            host: &target.host,
            port: target.port,
            path,
        });
        if in_scope {
            Ok(())
        } else {
            Err(AppError::Invalid(format!(
                "{}://{}:{}{path} is not in scope; add it to the scope before running the {tool} \
                 against it.",
                target.scheme, target.host, target.port
            )))
        }
    }

    pub(super) async fn task_log_lines(
        &self,
        db: &ProjectDb,
        id: TaskId,
        lines: Vec<(String, String)>,
    ) {
        let db = db.clone();
        let _ = tokio::task::spawn_blocking(move || db.task_log(id, lines)).await;
    }

    pub(super) async fn set_status(
        &self,
        db: &ProjectDb,
        id: TaskId,
        status: TaskStatus,
    ) -> Result<(), AppError> {
        let db = db.clone();
        tokio::task::spawn_blocking(move || db.task_set_status(id, status, None)).await??;
        let _ = self.inner.events.send(EngineEvent::TasksChanged);
        Ok(())
    }

    /// Fetches one URL (waiting for the limiter first).
    pub(super) async fn fetch(
        &self,
        entry: Entry,
        limiter: Limiter,
        config: &CrawlConfig,
        jar: bool,
        db: ProjectDb,
    ) -> Fetched {
        let _permit = limiter.acquire().await;
        let target = target_of(&entry.url);
        let cookie = if jar {
            let cookies_db = db.clone();
            let jar_list = tokio::task::spawn_blocking(move || cookies_db.cookie_jar_list())
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            let https = target.scheme == "https";
            let path = request_target(&entry.url);
            let matching = cookies::cookies_for(&jar_list, &target.host, &path, https, now_ms());
            Some(
                matching
                    .iter()
                    .map(|c| format!("{}={}", c.name, c.value))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        } else {
            None
        };
        let request = crawl_request(&entry.url, &config.user_agent, cookie.as_deref());
        let started_at = now_ms();
        let security = match self
            .tls_security(&target.host, target.scheme == "https")
            .await
        {
            Ok(security) => security,
            Err(e) => {
                return Fetched {
                    entry,
                    request,
                    started_at,
                    outcome: Err(SendError {
                        message: format!("TLS is not available: {e}"),
                        timing: Default::default(),
                        peer: None,
                    }),
                };
            }
        };
        let timeout = Duration::from_secs(u64::from(config.timeout_secs));
        let limits = SendLimits {
            response_timeout: timeout,
            body_idle_timeout: timeout,
            max_response_bytes: limits_of(&self.settings()).max_message_bytes.max(64 * 1024),
        };
        let destination = Destination {
            host: target.host.clone(),
            port: target.port,
            tls_name: target.host.clone(),
        };
        let outcome = self
            .session_send(
                "crawler",
                chosproxy_core::Source::Crawler,
                &destination,
                &security,
                &request,
                &limits,
            )
            .await;
        Fetched {
            entry,
            request,
            started_at,
            outcome,
        }
    }

    /// The crawl loop. Returns how it ended.
    pub(super) async fn run_crawl(
        &self,
        db: &ProjectDb,
        id: TaskId,
        config: CrawlConfig,
        start: Vec<Url>,
        mut control: watch::Receiver<Control>,
    ) -> Result<TaskStatus, AppError> {
        let scope = read(&self.inner.scope);
        let seeds: HashSet<(String, String, u16)> = start.iter().map(origin).collect();
        let allowed = |url: &Url| {
            if scope.is_empty() {
                seeds.contains(&origin(url))
            } else {
                let t = target_of(url);
                let parts = UrlParts {
                    scheme: t.scheme,
                    host: t.host,
                    port: t.port,
                    path: request_target(url),
                };
                scope.contains(&parts.target())
            }
        };
        let mut log: Vec<(String, String)> = Vec::new();
        let mut frontier = Frontier::new(
            config.max_depth,
            config.max_variants_per_path,
            &config.avoid,
        )
        .with_max_similar(config.max_similar);
        for url in &start {
            if allowed(url) {
                frontier.seed(url.clone());
            } else {
                log.push((
                    "warning".into(),
                    format!("Skipped {url}: it is not in scope."),
                ));
            }
        }
        for seed in &start {
            if !allowed(seed) {
                continue;
            }
            for path in robots_and_sitemap_paths() {
                if let Ok(url) = seed.join(path) {
                    frontier.push(url, 1, FoundKind::Robots, allowed);
                }
            }
        }
        if frontier.queued() == 0 {
            self.task_log_lines(db, id, log).await;
            return Err(AppError::Invalid(
                "no start URL is in scope; add it to the scope or define none".into(),
            ));
        }
        log.push((
            "info".into(),
            format!(
                "Crawling {} start URL(s) {}: depth {}, at most {} requests, {} at a time, {}.",
                start.len(),
                if scope.is_empty() {
                    "within their own sites (no scope defined)"
                } else {
                    "within the scope"
                },
                config.max_depth,
                config.max_requests,
                config.concurrency,
                if config.requests_per_second == 0 {
                    "no rate limit".to_string()
                } else {
                    format!("{} per second", config.requests_per_second)
                }
            ),
        ));
        self.task_log_lines(db, id, std::mem::take(&mut log)).await;
        self.set_status(db, id, TaskStatus::Running).await?;

        let limiter = Limiter::new(config.concurrency as usize, config.requests_per_second);
        let mut inflight: tokio::task::JoinSet<Fetched> = tokio::task::JoinSet::new();
        let mut sent: u64 = 0;
        let mut errors: u64 = 0;
        let mut depth_reached: u32 = 0;
        let mut post_forms: HashSet<String> = HashSet::new();
        let mut avoided: u64 = 0;
        let mut similar: u64 = 0;
        let mut last_progress = Instant::now();
        let mut paused = false;

        let status = loop {
            let wanted = *control.borrow_and_update();
            if wanted == Control::Cancel {
                inflight.abort_all();
                log.push(("info".into(), "Cancelled.".into()));
                break TaskStatus::Cancelled;
            }
            if (wanted == Control::Pause) != paused {
                paused = wanted == Control::Pause;
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
                log.push((
                    "info".into(),
                    if paused {
                        "Paused.".into()
                    } else {
                        "Resumed.".into()
                    },
                ));
            }
            while !paused
                && inflight.len() < config.concurrency as usize
                && sent < u64::from(config.max_requests)
            {
                let Some(entry) = frontier.pop() else {
                    break;
                };
                sent += 1;
                depth_reached = depth_reached.max(entry.depth);
                let engine = self.clone();
                let limiter = limiter.clone();
                let config = config.clone();
                let db = db.clone();
                inflight.spawn(in_task(current_task(), async move {
                    engine
                        .fetch(entry, limiter, &config, config.use_cookie_jar, db)
                        .await
                }));
            }
            if inflight.is_empty() {
                if paused {
                    if control.changed().await.is_err() {
                        break TaskStatus::Cancelled;
                    }
                    continue;
                }
                if frontier.queued() > 0 {
                    log.push((
                        "info".into(),
                        format!(
                            "Reached the maximum of {} requests; {} URLs were left unvisited.",
                            config.max_requests,
                            frontier.queued()
                        ),
                    ));
                }
                break TaskStatus::Completed;
            }
            let joined = tokio::select! {
                joined = inflight.join_next() => joined,
                changed = control.changed() => {
                    if changed.is_err() {
                        inflight.abort_all();
                        break TaskStatus::Cancelled;
                    }
                    continue;
                }
            };
            let Some(Ok(fetched)) = joined else {
                continue;
            };
            let Fetched {
                entry,
                request,
                started_at,
                outcome,
            } = fetched;
            let target = target_of(&entry.url);
            let jar_entries = match &outcome {
                Ok(sent_ok) if config.use_cookie_jar => cookies::entries_from_response(
                    &sent_ok.response,
                    &target.host,
                    &request_target(&entry.url),
                    now_ms(),
                ),
                _ => Vec::new(),
            };
            let mut exchange =
                repeater::captured(Source::Crawler, &target, request, started_at, &outcome);
            let reason = crawl_reason(&entry);
            exchange.notes = Some(match exchange.notes.take() {
                Some(n) => format!("{reason} {n}"),
                None => reason,
            });
            // Stored first, so the links found on it can name it.
            let page_id = {
                let db = db.clone();
                tokio::task::spawn_blocking(move || {
                    let page_id = db.insert(exchange)?;
                    db.cookie_jar_store("crawler", jar_entries)?;
                    Ok::<_, StorageError>(page_id)
                })
                .await??
            };
            match &outcome {
                Ok(sent_ok) => {
                    let (_, raw_body) = split_message(&sent_ok.response);
                    let head: &ResponseHead = &sent_ok.head;
                    let body = body_text(head, raw_body, DISCOVER_LIMIT).unwrap_or_default();
                    for found in discover(&entry.url, head, &body, config.submit_get_forms) {
                        match found.kind {
                            FoundKind::Resource => {}
                            FoundKind::Form if found.method != "GET" => {
                                if allowed(&found.url) && post_forms.insert(found.url.to_string()) {
                                    log.push((
                                        "info".into(),
                                        format!(
                                            "Found a {} form to {} on {} (not submitted: the crawler \
                                             only sends GET requests).",
                                            found.method, found.url, entry.url
                                        ),
                                    ));
                                }
                            }
                            kind => {
                                let depth = if kind == FoundKind::Redirect {
                                    entry.depth
                                } else {
                                    entry.depth + 1
                                };
                                match frontier.push_via(
                                    found.url,
                                    depth,
                                    kind,
                                    Some(page_id),
                                    allowed,
                                ) {
                                    Push::Avoided => avoided += 1,
                                    Push::Similar => similar += 1,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    errors += 1;
                    if errors <= LOGGED_ERRORS {
                        log.push(("warning".into(), format!("{}: {}", entry.url, e.message)));
                    } else if errors == LOGGED_ERRORS + 1 {
                        log.push(("warning".into(), "More errors are only counted.".into()));
                    }
                }
            }
            if last_progress.elapsed() >= PROGRESS_EVERY {
                last_progress = Instant::now();
                self.save_progress(db, id, &frontier, sent, errors, depth_reached, &mut log)
                    .await?;
            }
        };
        if avoided > 0 {
            log.push((
                "info".into(),
                format!("{avoided} link(s) were not followed because they look like they change state (log out, delete…)."),
            ));
        }
        if similar > 0 {
            log.push((
                "info".into(),
                format!(
                    "{similar} link(s) were not followed because {} URL(s) of the same pattern \
                     (same path apart from ids, same parameter names) were fetched already.",
                    config.max_similar
                ),
            ));
        }
        log.push((
            "info".into(),
            format!(
                "{} after {sent} requests ({errors} failed); {} URLs found.",
                match status {
                    TaskStatus::Cancelled => "Stopped",
                    _ => "Finished",
                },
                frontier.known()
            ),
        ));
        self.save_progress(db, id, &frontier, sent, errors, depth_reached, &mut log)
            .await?;
        Ok(status)
    }

    #[allow(clippy::too_many_arguments)]
    async fn save_progress(
        &self,
        db: &ProjectDb,
        id: TaskId,
        frontier: &Frontier,
        sent: u64,
        errors: u64,
        depth: u32,
        log: &mut Vec<(String, String)>,
    ) -> Result<(), AppError> {
        let progress = TaskProgress {
            requests: sent,
            errors,
            discovered: frontier.known() as u64,
            queued: frontier.queued() as u64,
            progress: format!("depth {depth}"),
        };
        let lines = std::mem::take(log);
        let db = db.clone();
        tokio::task::spawn_blocking(move || {
            db.task_progress(id, progress)?;
            db.task_log(id, lines)
        })
        .await??;
        let _ = self.inner.events.send(EngineEvent::TasksChanged);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn crawl_reasons() {
        let url = Url::parse("http://lab.test/a").unwrap();
        let entry = |kind, via| Entry {
            url: url.clone(),
            depth: 2,
            kind,
            via,
        };
        assert_eq!(
            crawl_reason(&entry(FoundKind::Link, None)),
            "Crawler: start URL."
        );
        assert!(crawl_reason(&entry(FoundKind::Robots, None)).contains("robots.txt"));
        assert_eq!(
            crawl_reason(&entry(FoundKind::Redirect, Some(7))),
            "Crawler: redirected from #7 (depth 2)."
        );
    }

    #[test]
    fn crawl_requests_look_like_a_browser() {
        let url = Url::parse("http://lab.test:8080/a b?x=1").unwrap();
        let raw = String::from_utf8(crawl_request(&url, "UA", Some("sid=1"))).unwrap();
        assert!(
            raw.starts_with("GET /a%20b?x=1 HTTP/1.1\r\nHost: lab.test:8080\r\nUser-Agent: UA\r\n")
        );
        assert!(raw.ends_with("Cookie: sid=1\r\n\r\n"));
        let plain = String::from_utf8(crawl_request(&url, "UA", Some(""))).unwrap();
        assert!(!plain.contains("Cookie"));
    }
}
