//! Tasks (spec §32): long-running work such as crawls, with status,
//! counters and a log, kept in the project.

use crate::StorageError;
use crate::db::ProjectDb;
use crate::macros::string_enum;
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Row id of a task.
pub type TaskId = i64;

/// Log lines kept per task (older ones are dropped).
pub const MAX_TASK_LOG: i64 = 2000;

string_enum!(
    /// Where a task is in its life.
    TaskStatus {
        /// Waiting to start.
        Queued => "queued",
        /// Working.
        Running => "running",
        /// Stopped for now; can be resumed.
        Paused => "paused",
        /// Finished normally.
        Completed => "completed",
        /// Stopped by an error.
        Failed => "failed",
        /// Stopped by the user (or by closing the project).
        Cancelled => "cancelled",
    }
);

impl TaskStatus {
    /// True once the task can no longer change.
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }
}

/// A task as listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskRow {
    /// Task id.
    #[ts(type = "number")]
    pub id: TaskId,
    /// What it does (`crawl`).
    pub kind: String,
    /// Name shown.
    pub name: String,
    /// Status.
    pub status: TaskStatus,
    /// Configuration it was started with (JSON).
    pub config: String,
    /// Created, Unix epoch ms.
    #[ts(type = "number")]
    pub created_at: i64,
    /// Started.
    #[ts(type = "number | null")]
    pub started_at: Option<i64>,
    /// Finished.
    #[ts(type = "number | null")]
    pub finished_at: Option<i64>,
    /// Requests sent.
    #[ts(type = "number")]
    pub requests: u64,
    /// Requests that failed.
    #[ts(type = "number")]
    pub errors: u64,
    /// Items discovered (URLs for a crawl).
    #[ts(type = "number")]
    pub discovered: u64,
    /// Items waiting.
    #[ts(type = "number")]
    pub queued: u64,
    /// Short progress text.
    pub progress: String,
    /// Why it failed, or the last error.
    pub last_error: Option<String>,
}

/// One log line of a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskLogEntry {
    /// Line id.
    #[ts(type = "number")]
    pub id: i64,
    /// When, Unix epoch ms.
    #[ts(type = "number")]
    pub at: i64,
    /// `info`, `warning` or `error`.
    pub level: String,
    /// Text.
    pub message: String,
}

/// Counters of a running task.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskProgress {
    /// Requests sent.
    pub requests: u64,
    /// Failed requests.
    pub errors: u64,
    /// Items discovered.
    pub discovered: u64,
    /// Items waiting.
    pub queued: u64,
    /// Short text.
    pub progress: String,
}

const COLUMNS: &str = "id, kind, name, status, config, created_at, started_at, finished_at, \
    requests, errors, discovered, queued, progress, last_error";

fn map_task(r: &Row<'_>) -> rusqlite::Result<TaskRow> {
    let n =
        |i: usize| -> rusqlite::Result<u64> { Ok(u64::try_from(r.get::<_, i64>(i)?).unwrap_or(0)) };
    Ok(TaskRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        name: r.get(2)?,
        status: TaskStatus::parse(&r.get::<_, String>(3)?).unwrap_or(TaskStatus::Failed),
        config: r.get(4)?,
        created_at: r.get(5)?,
        started_at: r.get(6)?,
        finished_at: r.get(7)?,
        requests: n(8)?,
        errors: n(9)?,
        discovered: n(10)?,
        queued: n(11)?,
        progress: r.get(12)?,
        last_error: r.get(13)?,
    })
}

fn count(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

impl ProjectDb {
    /// Creates a queued task.
    pub fn task_create(
        &self,
        kind: &str,
        name: &str,
        config: &str,
    ) -> Result<TaskRow, StorageError> {
        let (kind, name, config) = (
            kind.to_string(),
            name.trim().to_string(),
            config.to_string(),
        );
        if name.is_empty() || name.chars().count() > 200 {
            return Err(StorageError::InvalidQuery(
                "the task name must be 1–200 characters".into(),
            ));
        }
        let id = self.write(move |c| {
            c.execute(
                "INSERT INTO tasks (kind, name, status, config, created_at) \
                 VALUES (?1, ?2, 'queued', ?3, ?4)",
                params![kind, name, config, chosproxy_core::time::now_ms()],
            )?;
            Ok(c.last_insert_rowid())
        })?;
        self.task(id)?.ok_or(StorageError::NotFound(id))
    }

    /// Every task, newest first.
    pub fn tasks(&self) -> Result<Vec<TaskRow>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM tasks ORDER BY id DESC LIMIT 500"
            ))?;
            let rows = stmt
                .query_map([], map_task)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// One task.
    pub fn task(&self, id: TaskId) -> Result<Option<TaskRow>, StorageError> {
        self.with_reader(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM tasks WHERE id = ?1"),
                [id],
                map_task,
            )
            .optional()?)
        })
    }

    /// Changes a task's status; sets the start time when it first runs and
    /// the end time when it finishes. `error` replaces the last error.
    pub fn task_set_status(
        &self,
        id: TaskId,
        status: TaskStatus,
        error: Option<String>,
    ) -> Result<(), StorageError> {
        self.write(move |c| {
            let now = chosproxy_core::time::now_ms();
            let n = c.execute(
                "UPDATE tasks SET status = ?2, \
                 started_at = CASE WHEN ?2 = 'running' THEN COALESCE(started_at, ?3) ELSE started_at END, \
                 finished_at = CASE WHEN ?2 IN ('completed', 'failed', 'cancelled') THEN ?3 ELSE NULL END, \
                 last_error = COALESCE(?4, last_error) WHERE id = ?1",
                params![id, status.as_str(), now, error],
            )?;
            if n == 0 {
                return Err(StorageError::NotFound(id));
            }
            Ok(())
        })
    }

    /// Saves a running task's counters.
    pub fn task_progress(&self, id: TaskId, p: TaskProgress) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute(
                "UPDATE tasks SET requests = ?2, errors = ?3, discovered = ?4, queued = ?5, \
                 progress = ?6 WHERE id = ?1",
                params![
                    id,
                    count(p.requests),
                    count(p.errors),
                    count(p.discovered),
                    count(p.queued),
                    p.progress
                ],
            )?;
            Ok(())
        })
    }

    /// Updates the "discovered" counter and the progress text of a task,
    /// keeping its request and error counters (a later phase of a task that
    /// sent requests in an earlier one).
    pub fn task_progress_note(
        &self,
        id: TaskId,
        discovered: u64,
        progress: String,
    ) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute(
                "UPDATE tasks SET discovered = ?2, progress = ?3 WHERE id = ?1",
                params![id, i64::try_from(discovered).unwrap_or(i64::MAX), progress],
            )?;
            Ok(())
        })
    }

    /// Adds log lines to a task (keeps the last [`MAX_TASK_LOG`]).
    pub fn task_log(&self, id: TaskId, lines: Vec<(String, String)>) -> Result<(), StorageError> {
        if lines.is_empty() {
            return Ok(());
        }
        self.write(move |c| {
            let now = chosproxy_core::time::now_ms();
            for (level, message) in lines {
                let message: String = message.chars().take(2000).collect();
                c.execute(
                    "INSERT INTO task_log (task_id, at, level, message) VALUES (?1, ?2, ?3, ?4)",
                    params![id, now, level, message],
                )?;
            }
            c.execute(
                "DELETE FROM task_log WHERE task_id = ?1 AND id <= \
                 (SELECT id FROM task_log WHERE task_id = ?1 ORDER BY id DESC LIMIT 1 OFFSET ?2)",
                params![id, MAX_TASK_LOG],
            )?;
            Ok(())
        })
    }

    /// A task's log, oldest first.
    pub fn task_logs(&self, id: TaskId) -> Result<Vec<TaskLogEntry>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(
                "SELECT id, at, level, message FROM task_log WHERE task_id = ?1 ORDER BY id",
            )?;
            let rows = stmt
                .query_map([id], |r| {
                    Ok(TaskLogEntry {
                        id: r.get(0)?,
                        at: r.get(1)?,
                        level: r.get(2)?,
                        message: r.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Deletes a finished task and its log.
    pub fn task_delete(&self, id: TaskId) -> Result<(), StorageError> {
        self.write(move |c| {
            let status: Option<String> = c
                .query_row("SELECT status FROM tasks WHERE id = ?1", [id], |r| r.get(0))
                .optional()?;
            let status = status.ok_or(StorageError::NotFound(id))?;
            if !TaskStatus::parse(&status).is_some_and(TaskStatus::is_finished) {
                return Err(StorageError::InvalidQuery(
                    "stop the task before deleting it".into(),
                ));
            }
            c.execute("DELETE FROM task_log WHERE task_id = ?1", [id])?;
            c.execute("DELETE FROM fuzz_results WHERE task_id = ?1", [id])?;
            c.execute("DELETE FROM sequencer_tokens WHERE task_id = ?1", [id])?;
            c.execute("DELETE FROM ai_activity WHERE task_id = ?1", [id])?;
            c.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Saves a task's structured summary (JSON; a scan's dashboard).
    pub fn task_set_summary(&self, id: TaskId, summary: String) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute(
                "UPDATE tasks SET summary = ?2 WHERE id = ?1",
                params![id, summary],
            )?;
            Ok(())
        })
    }

    /// A task's structured summary ('' when it has none).
    pub fn task_summary(&self, id: TaskId) -> Result<String, StorageError> {
        self.with_reader(|c| {
            c.query_row("SELECT summary FROM tasks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or(StorageError::NotFound(id))
        })
    }

    /// Replaces the configuration a task keeps (live tasks save where they
    /// got to in it).
    pub fn task_set_config(&self, id: TaskId, config: String) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute(
                "UPDATE tasks SET config = ?2 WHERE id = ?1",
                params![id, config],
            )?;
            Ok(())
        })
    }

    /// Live tasks (kind `live`) left running or paused by a previous
    /// session: they resume when the project is opened.
    pub fn tasks_live_unfinished(&self) -> Result<Vec<TaskRow>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM tasks WHERE kind = 'live' AND status IN \
                 ('queued', 'running', 'paused') ORDER BY id"
            ))?;
            let rows = stmt
                .query_map([], map_task)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Marks tasks left unfinished by a previous session as cancelled (they
    /// cannot continue after the project was closed), except live tasks,
    /// which resume. Returns how many.
    pub fn tasks_interrupted(&self) -> Result<usize, StorageError> {
        self.write(|c| {
            let now = chosproxy_core::time::now_ms();
            let ids: Vec<i64> = c
                .prepare(
                    "SELECT id FROM tasks WHERE status IN ('queued', 'running', 'paused') \
                     AND kind <> 'live'",
                )?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            for id in &ids {
                c.execute(
                    "UPDATE tasks SET status = 'cancelled', finished_at = ?2 WHERE id = ?1",
                    params![id, now],
                )?;
                c.execute(
                    "INSERT INTO task_log (task_id, at, level, message) VALUES (?1, ?2, 'warning', \
                     'Stopped because the project was closed.')",
                    params![id, now],
                )?;
            }
            Ok(ids.len())
        })
    }

    /// Marks non-live tasks left unfinished by an unexpected exit as
    /// cancelled with an explicit crash interruption note (spec §118).
    /// Live tasks (`kind = 'live'`) are left unfinished so they resume.
    pub fn tasks_interrupted_crash(&self) -> Result<usize, StorageError> {
        self.write(|c| {
            let now = chosproxy_core::time::now_ms();
            let ids: Vec<i64> = c
                .prepare(
                    "SELECT id FROM tasks WHERE status IN ('queued', 'running', 'paused') \
                     AND kind <> 'live'",
                )?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            for id in &ids {
                c.execute(
                    "UPDATE tasks SET status = 'cancelled', finished_at = ?2, \
                     last_error = 'Interrupted by unexpected exit' WHERE id = ?1",
                    params![id, now],
                )?;
                c.execute(
                    "INSERT INTO task_log (task_id, at, level, message) VALUES (?1, ?2, 'warning', \
                     'Interrupted because the previous session exited unexpectedly before the project was closed.')",
                    params![id, now],
                )?;
            }
            Ok(ids.len())
        })
    }
}

impl ProjectDb {
    /// Stores tokens of a sequencer capture (`(index, token)` pairs).
    pub fn sequencer_record(
        &self,
        task: TaskId,
        tokens: Vec<(u64, String)>,
    ) -> Result<(), StorageError> {
        if tokens.is_empty() {
            return Ok(());
        }
        self.write(move |c| {
            for (idx, token) in tokens {
                c.execute(
                    "INSERT INTO sequencer_tokens (task_id, idx, token) VALUES (?1, ?2, ?3)",
                    params![task, i64::try_from(idx).unwrap_or(i64::MAX), token],
                )?;
            }
            Ok(())
        })
    }

    /// The tokens of a capture, in capture order.
    pub fn sequencer_tokens(&self, task: TaskId) -> Result<Vec<String>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(
                "SELECT token FROM sequencer_tokens WHERE task_id = ?1 ORDER BY idx, id",
            )?;
            let rows = stmt
                .query_map([task], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            Ok(rows)
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::OpenOptions;

    #[test]
    fn task_summaries() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let t = db.task_create("scan", "Scan", "{}").unwrap();
        assert_eq!(db.task_summary(t.id).unwrap(), "");
        db.task_set_summary(t.id, "{\"phase\":1}".into()).unwrap();
        assert_eq!(db.task_summary(t.id).unwrap(), "{\"phase\":1}");
        assert!(db.task_summary(t.id + 1).is_err());
    }

    #[test]
    fn life_of_a_task() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let t = db.task_create("crawl", " Crawl lab ", "{}").unwrap();
        assert_eq!(
            (t.name.as_str(), t.status),
            ("Crawl lab", TaskStatus::Queued)
        );
        assert!(db.task_create("crawl", " ", "{}").is_err());

        db.task_set_status(t.id, TaskStatus::Running, None).unwrap();
        db.task_progress(
            t.id,
            TaskProgress {
                requests: 3,
                errors: 1,
                discovered: 9,
                queued: 6,
                progress: "depth 1".into(),
            },
        )
        .unwrap();
        db.task_log(
            t.id,
            vec![
                ("info".into(), "started".into()),
                ("error".into(), "x".into()),
            ],
        )
        .unwrap();
        assert!(db.task_delete(t.id).is_err(), "running tasks stay");
        db.task_set_status(t.id, TaskStatus::Paused, None).unwrap();
        let paused = db.task(t.id).unwrap().unwrap();
        assert!(paused.started_at.is_some() && paused.finished_at.is_none());
        assert_eq!((paused.requests, paused.queued), (3, 6));

        let other = db.task_create("crawl", "second", "{}").unwrap();
        db.task_set_status(other.id, TaskStatus::Completed, Some("boom".into()))
            .unwrap();
        assert_eq!(db.tasks_interrupted().unwrap(), 1);
        let rows = db.tasks().unwrap();
        assert_eq!(rows[0].id, other.id, "newest first");
        assert_eq!(rows[0].last_error.as_deref(), Some("boom"));
        assert_eq!(rows[1].status, TaskStatus::Cancelled);
        let logs = db.task_logs(t.id).unwrap();
        assert_eq!(logs.len(), 3);
        assert!(logs[2].message.contains("project was closed"));

        db.sequencer_record(t.id, vec![(1, "b".into()), (0, "a".into())])
            .unwrap();
        assert_eq!(db.sequencer_tokens(t.id).unwrap(), ["a", "b"]);
        db.task_delete(t.id).unwrap();
        assert!(db.task(t.id).unwrap().is_none());
        assert!(db.sequencer_tokens(t.id).unwrap().is_empty());
        assert!(db.task_logs(t.id).unwrap().is_empty());
    }

    #[test]
    fn log_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let t = db.task_create("crawl", "t", "{}").unwrap();
        let lines: Vec<(String, String)> = (0..MAX_TASK_LOG + 5)
            .map(|i| ("info".to_string(), format!("line {i}")))
            .collect();
        db.task_log(t.id, lines).unwrap();
        let logs = db.task_logs(t.id).unwrap();
        assert_eq!(logs.len() as i64, MAX_TASK_LOG);
        assert_eq!(logs[0].message, "line 5");
    }
}

/// A content discovery hit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiscoveryHit {
    /// Row id.
    #[ts(type = "number")]
    pub id: i64,
    /// Unix epoch ms.
    #[ts(type = "number")]
    pub found_at: i64,
    /// URL.
    pub url: String,
    /// `folder` or `file`.
    pub kind: String,
    /// Status code.
    pub status: u16,
    /// Response length.
    #[ts(type = "number")]
    pub length: u64,
    /// The stored exchange.
    #[ts(type = "number | null")]
    pub exchange_id: Option<i64>,
}

impl ProjectDb {
    /// Records hits of a discovery task.
    pub fn discovery_record(
        &self,
        task: TaskId,
        hits: Vec<DiscoveryHit>,
    ) -> Result<(), StorageError> {
        self.write(move |c| {
            for h in &hits {
                c.execute(
                    "INSERT INTO discovery_results (task_id, found_at, url, kind, status, length, exchange_id) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        task,
                        h.found_at,
                        h.url,
                        h.kind,
                        h.status,
                        i64::try_from(h.length).unwrap_or(i64::MAX),
                        h.exchange_id
                    ],
                )?;
            }
            Ok(())
        })
    }

    /// A live crawl's unrequested URL was requested after all: records the
    /// status and length of that response. True if a hit changed.
    pub fn discovery_mark_requested(
        &self,
        task: TaskId,
        url: String,
        status: u16,
        length: u64,
    ) -> Result<bool, StorageError> {
        self.write(move |c| {
            let n = c.execute(
                "UPDATE discovery_results SET status = ?3, length = ?4 \
                 WHERE task_id = ?1 AND url = ?2 AND status = 0",
                rusqlite::params![task, url, status, i64::try_from(length).unwrap_or(i64::MAX)],
            )?;
            Ok(n > 0)
        })
    }

    /// Hits of a discovery task, in the order found.
    pub fn discovery_results(&self, task: TaskId) -> Result<Vec<DiscoveryHit>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(
                "SELECT id, found_at, url, kind, status, length, exchange_id FROM discovery_results \
                 WHERE task_id = ?1 ORDER BY id LIMIT 100000",
            )?;
            let rows = stmt
                .query_map([task], |r| {
                    Ok(DiscoveryHit {
                        id: r.get(0)?,
                        found_at: r.get(1)?,
                        url: r.get(2)?,
                        kind: r.get(3)?,
                        status: r.get(4)?,
                        length: u64::try_from(r.get::<_, i64>(5)?).unwrap_or(0),
                        exchange_id: r.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }
}
