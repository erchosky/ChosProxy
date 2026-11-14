//! Content discovery tasks (matrix DISCOVERY-001/002).
//!
//! Folder by folder: a few requests for names that cannot exist teach what
//! "not found" looks like there (soft 404), then candidate names are
//! requested through the limiter, every URL checked against the scope at
//! send time (or the base origin while no scope is defined). Only hits are
//! stored in the history; folders found are searched in turn down to the
//! configured depth.

use super::Engine;
use super::task_ops::{Control, Fetched, LOGGED_ERRORS, PROGRESS_EVERY, request_target, target_of};
use crate::{AppError, repeater};
use chosproxy_core::Source;
use chosproxy_core::scope::UrlParts;
use chosproxy_core::time::now_ms;
use chosproxy_crawler::{CrawlConfig, Entry, FoundKind};
use chosproxy_discovery::{
    Baseline, Candidate, DiscoveryConfig, Fingerprint, Kind, Verdict, candidates, probes,
    words_from_paths,
};
use chosproxy_http::content::{body_text, split_message};
use chosproxy_request::Limiter;
use chosproxy_storage::{
    DiscoveryHit, HistoryQuery, ProjectDb, TaskId, TaskProgress, TaskRow, TaskStatus,
};
use std::collections::{HashSet, VecDeque};
use std::time::Instant;
use tokio::sync::watch;
use url::Url;

impl Engine {
    /// The defaults for a new discovery.
    pub fn discovery_defaults(&self) -> DiscoveryConfig {
        DiscoveryConfig::default()
    }

    /// Starts content discovery under `config.base_url`.
    pub async fn discovery_start(&self, config: DiscoveryConfig) -> Result<TaskRow, AppError> {
        let base = config.validate().map_err(AppError::Invalid)?;
        let target = target_of(&base);
        self.ensure_in_scope(&target, base.path(), "content discovery")?;
        let name = format!("Discover content on {}{}", target.host, base.path());
        let json = serde_json::to_string(&config)
            .map_err(|e| AppError::Invalid(format!("invalid discovery settings: {e}")))?;
        self.launch_task(
            "discovery",
            "Content discovery",
            name,
            json,
            move |engine, db, id, rx| async move {
                engine.run_discovery(&db, id, config, base, rx).await
            },
        )
        .await
    }

    /// Hits of a discovery task.
    pub async fn discovery_results(&self, task: TaskId) -> Result<Vec<DiscoveryHit>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.discovery_results(task)).await??)
    }

    async fn run_discovery(
        &self,
        db: &ProjectDb,
        id: TaskId,
        config: DiscoveryConfig,
        base: Url,
        mut control: watch::Receiver<Control>,
    ) -> Result<TaskStatus, AppError> {
        let scope = super::read(&self.inner.scope);
        let base_origin = (
            base.scheme().to_string(),
            base.host_str().unwrap_or("").to_string(),
            base.port_or_known_default(),
        );
        let allowed = |url: &Url| {
            if scope.is_empty() {
                (
                    url.scheme().to_string(),
                    url.host_str().unwrap_or("").to_string(),
                    url.port_or_known_default(),
                ) == base_origin
            } else {
                let t = target_of(url);
                scope.contains(
                    &UrlParts {
                        scheme: t.scheme,
                        host: t.host,
                        port: t.port,
                        path: request_target(url),
                    }
                    .target(),
                )
            }
        };
        let site: Vec<String> = if config.site_words {
            let db = db.clone();
            let host = base.host_str().unwrap_or("").to_string();
            let map = tokio::task::spawn_blocking(move || db.site_map(&HistoryQuery::default()))
                .await??;
            words_from_paths(
                map.entries
                    .iter()
                    .filter(|e| e.host == host)
                    .map(|e| e.path.as_str()),
            )
        } else {
            Vec::new()
        };
        let words = config.words(&site);
        let fetch_config = CrawlConfig {
            timeout_secs: config.timeout_secs,
            user_agent: config.user_agent.clone(),
            use_cookie_jar: config.use_cookie_jar,
            ..CrawlConfig::default()
        };
        let limiter = Limiter::new(config.concurrency as usize, config.requests_per_second);
        let mut log: Vec<(String, String)> = vec![(
            "info".into(),
            format!(
                "Searching {base} with {} names{} and {} extension(s), {} at a time, {}.",
                words.len(),
                if site.is_empty() {
                    String::new()
                } else {
                    format!(" ({} from the site map)", site.len())
                },
                config.extensions.len(),
                config.concurrency,
                if config.requests_per_second == 0 {
                    "no rate limit".to_string()
                } else {
                    format!("{} per second", config.requests_per_second)
                }
            ),
        )];
        self.task_log_lines(db, id, std::mem::take(&mut log)).await;
        self.set_status(db, id, TaskStatus::Running).await?;

        let mut folders: VecDeque<(Url, u32)> = VecDeque::from([(base.clone(), 0)]);
        let mut seen: HashSet<String> = HashSet::from([base.to_string()]);
        let (mut sent, mut errors, mut hits, mut skipped) = (0u64, 0u64, 0u64, 0u64);
        let mut last_progress = Instant::now();
        let mut paused = false;
        let max = u64::from(config.max_requests);
        let fetch = |url: Url| {
            let engine = self.clone();
            let limiter = limiter.clone();
            let config = fetch_config.clone();
            let db = db.clone();
            async move {
                engine
                    .fetch(
                        Entry {
                            url,
                            depth: 0,
                            kind: FoundKind::Link,
                            via: None,
                        },
                        limiter,
                        &config,
                        config.use_cookie_jar,
                        db,
                    )
                    .await
            }
        };
        let fingerprint = |f: &Fetched| -> Fingerprint {
            match &f.outcome {
                Ok(sent) => {
                    let (_, raw) = split_message(&sent.response);
                    let body = body_text(&sent.head, raw, 1024 * 1024).unwrap_or_default();
                    let location = sent
                        .head
                        .headers
                        .iter()
                        .find(|h| h.name.eq_ignore_ascii_case("location"))
                        .map(|h| String::from_utf8_lossy(&h.value).into_owned());
                    Fingerprint::of(
                        &f.entry.url,
                        sent.head.status,
                        location.as_deref(),
                        body.as_bytes(),
                    )
                }
                Err(_) => Fingerprint::of(&f.entry.url, 0, None, b""),
            }
        };

        let status = 'folders: loop {
            let Some((folder, depth)) = folders.pop_front() else {
                break TaskStatus::Completed;
            };
            if !allowed(&folder) {
                skipped += 1;
                continue;
            }
            // What "not found" looks like in this folder.
            let mut baseline = Baseline::default();
            for p in probes(&folder, &config.extensions) {
                if sent >= max {
                    break;
                }
                sent += 1;
                let f = fetch(p.url.clone()).await;
                baseline.learn(p.kind, fingerprint(&f));
            }
            let mut queue: VecDeque<Candidate> = candidates(
                &folder,
                &words,
                &config.extensions,
                config.well_known_files,
                depth,
            )
            .into();
            let mut inflight: tokio::task::JoinSet<(Candidate, Fetched)> =
                tokio::task::JoinSet::new();
            loop {
                let wanted = *control.borrow_and_update();
                if wanted == Control::Cancel {
                    inflight.abort_all();
                    log.push(("info".into(), "Cancelled.".into()));
                    break 'folders TaskStatus::Cancelled;
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
                        if paused { "Paused." } else { "Resumed." }.into(),
                    ));
                }
                while !paused && inflight.len() < config.concurrency as usize && sent < max {
                    let Some(c) = queue.pop_front() else {
                        break;
                    };
                    if !allowed(&c.url) {
                        skipped += 1;
                        continue;
                    }
                    sent += 1;
                    let f = fetch(c.url.clone());
                    inflight.spawn(super::task_ops::in_task(
                        super::task_ops::current_task(),
                        async move { (c, f.await) },
                    ));
                }
                if inflight.is_empty() {
                    if paused {
                        if control.changed().await.is_err() {
                            break 'folders TaskStatus::Cancelled;
                        }
                        continue;
                    }
                    if sent >= max && (!queue.is_empty() || !folders.is_empty()) {
                        log.push((
                            "info".into(),
                            format!("Reached the maximum of {} requests.", config.max_requests),
                        ));
                        break 'folders TaskStatus::Completed;
                    }
                    break;
                }
                let joined = tokio::select! {
                    joined = inflight.join_next() => joined,
                    changed = control.changed() => {
                        if changed.is_err() {
                            inflight.abort_all();
                            break 'folders TaskStatus::Cancelled;
                        }
                        continue;
                    }
                };
                let Some(Ok((candidate, fetched))) = joined else {
                    continue;
                };
                if let Err(e) = &fetched.outcome {
                    errors += 1;
                    if errors <= LOGGED_ERRORS {
                        log.push((
                            "warning".into(),
                            format!("{}: {}", candidate.url, e.message),
                        ));
                    }
                    continue;
                }
                let print = fingerprint(&fetched);
                let verdict = baseline.judge(&candidate, &print);
                if verdict == Verdict::Missing {
                    continue;
                }
                hits += 1;
                let Fetched {
                    entry,
                    request,
                    started_at,
                    outcome,
                } = fetched;
                let target = target_of(&entry.url);
                let exchange =
                    repeater::captured(Source::Discovery, &target, request, started_at, &outcome);
                let exchange_id = {
                    let db = db.clone();
                    tokio::task::spawn_blocking(move || db.insert(exchange))
                        .await?
                        .ok()
                };
                let (kind, folder_url) = match (verdict, candidate.kind) {
                    (Verdict::FolderRedirect, _) => (
                        Kind::Folder,
                        Url::parse(&format!("{}/", entry.url.as_str().trim_end_matches('/'))).ok(),
                    ),
                    (_, Kind::Folder) => (Kind::Folder, Some(entry.url.clone())),
                    _ => (Kind::File, None),
                };
                let hit = DiscoveryHit {
                    id: 0,
                    found_at: now_ms(),
                    url: entry.url.to_string(),
                    kind: if kind == Kind::Folder {
                        "folder".into()
                    } else {
                        "file".into()
                    },
                    status: print.status,
                    length: print.length as u64,
                    exchange_id,
                };
                {
                    let db = db.clone();
                    tokio::task::spawn_blocking(move || db.discovery_record(id, vec![hit]))
                        .await??;
                }
                if let Some(next) = folder_url
                    && config.recursive
                    && depth < config.max_depth
                    && seen.insert(next.to_string())
                {
                    folders.push_back((next, depth + 1));
                }
                if last_progress.elapsed() >= PROGRESS_EVERY {
                    last_progress = Instant::now();
                    self.discovery_progress(
                        db,
                        id,
                        sent,
                        errors,
                        hits,
                        queue.len() + folders.len(),
                        &mut log,
                    )
                    .await?;
                }
            }
        };
        if skipped > 0 {
            log.push((
                "info".into(),
                format!("{skipped} URL(s) outside the scope were not requested."),
            ));
        }
        log.push((
            "info".into(),
            format!(
                "{} after {sent} requests ({errors} failed): {hits} found.",
                if status == TaskStatus::Cancelled {
                    "Stopped"
                } else {
                    "Finished"
                }
            ),
        ));
        self.discovery_progress(db, id, sent, errors, hits, folders.len(), &mut log)
            .await?;
        Ok(status)
    }

    #[allow(clippy::too_many_arguments)]
    async fn discovery_progress(
        &self,
        db: &ProjectDb,
        id: TaskId,
        sent: u64,
        errors: u64,
        hits: u64,
        queued: usize,
        log: &mut Vec<(String, String)>,
    ) -> Result<(), AppError> {
        let progress = TaskProgress {
            requests: sent,
            errors,
            discovered: hits,
            queued: queued as u64,
            progress: format!("{hits} found, {sent} requests"),
        };
        let lines = std::mem::take(log);
        let db2 = db.clone();
        tokio::task::spawn_blocking(move || db2.task_progress(id, progress)).await??;
        self.task_log_lines(db, id, lines).await;
        let _ = self
            .inner
            .events
            .send(chosproxy_core::event::EngineEvent::TasksChanged);
        Ok(())
    }
}
