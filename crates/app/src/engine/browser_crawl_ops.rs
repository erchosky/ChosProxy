//! Browser-powered crawling (M24, matrix CRAWL-002): the lab browser
//! (Chromium family) opens each page through the proxy, so scripts run and
//! links they add are found; every request it makes is captured like any
//! browsing. Breadth-first, in scope (or the start origin when no scope is
//! defined), up to a page limit.

use super::Engine;
use super::task_ops::{Control, request_target, target_of};
use crate::AppError;
use chosproxy_dom::driver::Driver;
use chosproxy_storage::{ProjectDb, TaskId, TaskProgress, TaskRow, TaskStatus};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::time::Duration;
use tokio::sync::watch;
use ts_rs::TS;
use url::Url;

/// Most pages one browser crawl may open.
pub const MAX_BROWSER_PAGES: u32 = 500;

/// Settings of a browser crawl.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrowserCrawlConfig {
    /// Where to start.
    pub start_url: String,
    /// Pages to open at most.
    pub max_pages: u32,
    /// Seconds to wait for each page to load.
    pub page_timeout_secs: u32,
    /// Lab browser to use (default: the first Chromium-family one).
    #[serde(default)]
    pub browser: Option<String>,
}

impl Engine {
    /// Starts a browser crawl as a task.
    pub async fn browser_crawl_start(
        &self,
        config: BrowserCrawlConfig,
    ) -> Result<TaskRow, AppError> {
        let start = Url::parse(config.start_url.trim())
            .map_err(|e| AppError::Invalid(format!("invalid start URL: {e}")))?;
        if !matches!(start.scheme(), "http" | "https") {
            return Err(AppError::Invalid(
                "the start URL must be http or https".into(),
            ));
        }
        if !(1..=MAX_BROWSER_PAGES).contains(&config.max_pages)
            || !(1..=120).contains(&config.page_timeout_secs)
        {
            return Err(AppError::Invalid(format!(
                "pages: 1–{MAX_BROWSER_PAGES}; page timeout: 1–120 s"
            )));
        }
        self.ensure_in_scope(&target_of(&start), &request_target(&start), "browser crawl")?;
        let json = serde_json::to_string(&config).map_err(|e| AppError::Invalid(e.to_string()))?;
        let name = format!("Browser crawl of {}", start.origin().ascii_serialization());
        self.launch_task(
            "browserCrawl",
            "Browser crawl",
            name,
            json,
            move |engine, db, id, rx| async move {
                engine.run_browser_crawl(&db, id, config, start, rx).await
            },
        )
        .await
    }

    fn browser_crawl_allows(&self, start: &Url, url: &Url) -> bool {
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        let scope_defined = !super::read(&self.inner.scope).is_empty();
        if scope_defined {
            self.ensure_in_scope(&target_of(url), &request_target(url), "browser crawl")
                .is_ok()
        } else {
            url.origin() == start.origin()
        }
    }

    async fn run_browser_crawl(
        &self,
        db: &ProjectDb,
        id: TaskId,
        config: BrowserCrawlConfig,
        start: Url,
        mut control: watch::Receiver<Control>,
    ) -> Result<TaskStatus, AppError> {
        let status = self
            .launch_lab_browser(
                config.browser.clone(),
                Some("http://chosproxy/".into()),
                true,
            )
            .await?;
        let profile = status
            .profile_dir
            .ok_or_else(|| AppError::Invalid("the lab browser has no profile".into()))?;
        let result = self
            .browser_crawl_pages(db, id, &config, &start, &profile, &mut control)
            .await;
        self.close_browser().await;
        result
    }

    async fn browser_crawl_pages(
        &self,
        db: &ProjectDb,
        id: TaskId,
        config: &BrowserCrawlConfig,
        start: &Url,
        profile: &str,
        control: &mut watch::Receiver<Control>,
    ) -> Result<TaskStatus, AppError> {
        let (port, path) = chosproxy_dom::cdp::devtools_endpoint(
            std::path::Path::new(profile),
            Duration::from_secs(20),
        )
        .await
        .map_err(|e| AppError::Invalid(e.to_string()))?;
        let mut driver = Driver::connect(port, &path)
            .await
            .map_err(|e| AppError::Invalid(e.to_string()))?;
        let mut queue: VecDeque<Url> = VecDeque::from([start.clone()]);
        let mut seen: HashSet<String> = HashSet::from([start.to_string()]);
        let (mut visited, mut errors) = (0u64, 0u64);
        let mut log: Vec<(String, String)> = Vec::new();
        let wait = Duration::from_secs(u64::from(config.page_timeout_secs));
        while let Some(url) = queue.pop_front() {
            if *control.borrow_and_update() == Control::Cancel {
                log.push(("info".into(), "Cancelled.".into()));
                self.task_log_lines(db, id, log).await;
                return Ok(TaskStatus::Cancelled);
            }
            if visited >= u64::from(config.max_pages) {
                log.push((
                    "info".into(),
                    format!("Stopped at the limit of {} pages.", config.max_pages),
                ));
                break;
            }
            match driver.visit(url.as_str(), wait).await {
                Ok(page) => {
                    visited += 1;
                    for link in page.links {
                        let Ok(mut next) = Url::parse(&link) else {
                            continue;
                        };
                        next.set_fragment(None);
                        if self.browser_crawl_allows(start, &next) && seen.insert(next.to_string())
                        {
                            queue.push_back(next);
                        }
                    }
                    log.push(("info".into(), format!("{} — {}", page.url, page.title)));
                }
                Err(e) => {
                    errors += 1;
                    log.push(("warning".into(), format!("{url}: {e}")));
                }
            }
            let progress = TaskProgress {
                requests: visited,
                errors,
                discovered: seen.len() as u64,
                queued: queue.len() as u64,
                progress: format!("{visited} page(s) opened, {} queued", queue.len()),
            };
            let db2 = db.clone();
            let _ = tokio::task::spawn_blocking(move || db2.task_progress(id, progress)).await;
            if log.len() >= 20 {
                self.task_log_lines(db, id, std::mem::take(&mut log)).await;
            }
        }
        log.push((
            "info".into(),
            format!(
                "Done: {visited} page(s) opened, {} URL(s) found.",
                seen.len()
            ),
        ));
        self.task_log_lines(db, id, log).await;
        Ok(TaskStatus::Completed)
    }
}
