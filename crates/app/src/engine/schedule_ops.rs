//! Scheduled tasks (matrix SCANNER-016): crawls and content discovery that
//! start at a set time, once or repeatedly. Schedules live in the project
//! and only run while it is open in ChosProxy; each run goes through the
//! normal start path, so scope and limits apply as when started by hand.

use super::Engine;
use super::lock;
use crate::AppError;
use chosproxy_core::event::{EngineEvent, NoticeLevel};
use chosproxy_core::time::now_ms;
use chosproxy_crawler::CrawlConfig;
use chosproxy_discovery::DiscoveryConfig;
use chosproxy_storage::{NewSchedule, ScheduleRow};
use std::sync::{Arc, Weak};
use std::time::Duration;

/// How often due schedules are checked.
const TICK: Duration = Duration::from_secs(15);

impl Engine {
    /// Schedules of the open project, soonest first.
    pub async fn schedules(&self) -> Result<Vec<ScheduleRow>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.schedules()).await??)
    }

    /// Creates a schedule after checking its task settings.
    pub async fn schedule_create(&self, s: NewSchedule) -> Result<ScheduleRow, AppError> {
        match s.kind.as_str() {
            "crawl" => {
                let c: CrawlConfig = serde_json::from_str(&s.config)
                    .map_err(|e| AppError::Invalid(format!("invalid crawl settings: {e}")))?;
                c.validate().map_err(AppError::Invalid)?;
            }
            "discovery" => {
                let c: DiscoveryConfig = serde_json::from_str(&s.config)
                    .map_err(|e| AppError::Invalid(format!("invalid discovery settings: {e}")))?;
                c.validate().map_err(AppError::Invalid)?;
            }
            "scan" => {
                let launch: super::ScanLaunch = serde_json::from_str(&s.config)
                    .map_err(|e| AppError::Invalid(format!("invalid scan settings: {e}")))?;
                self.validate_scan_launch(&launch)?;
            }
            _ => {
                return Err(AppError::Invalid(
                    "only crawls, content discovery and scans can be scheduled".into(),
                ));
            }
        }
        let db = self.db()?;
        let row = tokio::task::spawn_blocking(move || db.schedule_create(s)).await??;
        let _ = self.inner.events.send(EngineEvent::TasksChanged);
        Ok(row)
    }

    /// Turns a schedule on or off.
    pub async fn schedule_set_enabled(&self, id: i64, enabled: bool) -> Result<(), AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.schedule_set_enabled(id, enabled)).await??;
        let _ = self.inner.events.send(EngineEvent::TasksChanged);
        Ok(())
    }

    /// Deletes a schedule.
    pub async fn schedule_delete(&self, id: i64) -> Result<(), AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.schedule_delete(id)).await??;
        let _ = self.inner.events.send(EngineEvent::TasksChanged);
        Ok(())
    }

    /// Starts the scheduler loop once (it follows whichever project is
    /// open and stops when the engine is dropped).
    pub(crate) fn ensure_scheduler(&self) {
        let mut task = lock(&self.inner.scheduler_task);
        if task.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        let weak: Weak<super::Inner> = Arc::downgrade(&self.inner);
        *task = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(TICK).await;
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                Engine::from_inner(inner).run_due_schedules(now_ms()).await;
            }
        }));
    }

    /// Starts every schedule due at `now` (also used by tests).
    pub async fn run_due_schedules(&self, now: i64) -> usize {
        let Ok(db) = self.db() else {
            return 0;
        };
        if db.is_read_only() {
            return 0;
        }
        let due = {
            let db = db.clone();
            match tokio::task::spawn_blocking(move || db.schedules_due(now)).await {
                Ok(Ok(d)) => d,
                _ => return 0,
            }
        };
        let mut started = 0;
        for s in due {
            let result = match s.kind.as_str() {
                "crawl" => match serde_json::from_str::<CrawlConfig>(&s.config) {
                    Ok(c) => self.crawl_start(c).await,
                    Err(e) => Err(AppError::Invalid(e.to_string())),
                },
                "discovery" => match serde_json::from_str::<DiscoveryConfig>(&s.config) {
                    Ok(c) => self.discovery_start(c).await,
                    Err(e) => Err(AppError::Invalid(e.to_string())),
                },
                "scan" => match serde_json::from_str::<super::ScanLaunch>(&s.config) {
                    Ok(launch) => self.scan_start(launch).await,
                    Err(e) => Err(AppError::Invalid(e.to_string())),
                },
                other => Err(AppError::Invalid(format!("unknown task kind {other}"))),
            };
            let (task, error) = match result {
                Ok(t) => {
                    started += 1;
                    (Some(t.id), None)
                }
                Err(e) => {
                    self.notice(
                        NoticeLevel::Warning,
                        format!("Scheduled task “{}” could not start: {e}", s.name),
                    );
                    (None, Some(e.to_string()))
                }
            };
            let db = db.clone();
            let _ =
                tokio::task::spawn_blocking(move || db.schedule_ran(s.id, now, task, error)).await;
        }
        if started > 0 {
            let _ = self.inner.events.send(EngineEvent::TasksChanged);
        }
        started
    }
}
