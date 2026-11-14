//! Scheduled tasks (matrix SCANNER-016).

use crate::StorageError;
use crate::db::ProjectDb;
use rusqlite::{Row, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Schedules kept per project.
pub const MAX_SCHEDULES: i64 = 200;
/// Shortest repeat interval.
pub const MIN_REPEAT_MINUTES: u32 = 5;

/// A scheduled task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScheduleRow {
    /// Id.
    #[ts(type = "number")]
    pub id: i64,
    /// `crawl` or `discovery`.
    pub kind: String,
    /// Name.
    pub name: String,
    /// The task's settings (JSON).
    pub config: String,
    /// Next run, Unix epoch ms.
    #[ts(type = "number")]
    pub next_run: i64,
    /// Minutes between runs (0 = once).
    pub repeat_minutes: u32,
    /// Runs when due.
    pub enabled: bool,
    /// Last run, Unix epoch ms.
    #[ts(type = "number | null")]
    pub last_run: Option<i64>,
    /// Task started by the last run.
    #[ts(type = "number | null")]
    pub last_task_id: Option<i64>,
    /// Why the last run could not start.
    pub last_error: Option<String>,
}

/// A schedule to create.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewSchedule {
    /// `crawl` or `discovery`.
    pub kind: String,
    /// Name.
    pub name: String,
    /// The task's settings (JSON).
    pub config: String,
    /// First run, Unix epoch ms.
    #[ts(type = "number")]
    pub next_run: i64,
    /// Minutes between runs (0 = once).
    pub repeat_minutes: u32,
}

const COLUMNS: &str =
    "id, kind, name, config, next_run, repeat_minutes, enabled, last_run, last_task_id, last_error";

fn map(r: &Row<'_>) -> rusqlite::Result<ScheduleRow> {
    Ok(ScheduleRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        name: r.get(2)?,
        config: r.get(3)?,
        next_run: r.get(4)?,
        repeat_minutes: r.get(5)?,
        enabled: r.get(6)?,
        last_run: r.get(7)?,
        last_task_id: r.get(8)?,
        last_error: r.get(9)?,
    })
}

impl ProjectDb {
    /// Creates a schedule.
    pub fn schedule_create(&self, s: NewSchedule) -> Result<ScheduleRow, StorageError> {
        if !matches!(s.kind.as_str(), "crawl" | "discovery" | "scan") {
            return Err(StorageError::InvalidQuery(
                "only crawls, content discovery and scans can be scheduled".into(),
            ));
        }
        if s.repeat_minutes != 0 && s.repeat_minutes < MIN_REPEAT_MINUTES {
            return Err(StorageError::InvalidQuery(format!(
                "repeat at most every {MIN_REPEAT_MINUTES} minutes"
            )));
        }
        let name: String = s.name.trim().chars().take(200).collect();
        self.write(move |c| {
            let count: i64 = c.query_row("SELECT COUNT(*) FROM schedules", [], |r| r.get(0))?;
            if count >= MAX_SCHEDULES {
                return Err(StorageError::InvalidQuery(format!("at most {MAX_SCHEDULES} schedules")));
            }
            c.execute(
                "INSERT INTO schedules (kind, name, config, next_run, repeat_minutes) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![s.kind, name, s.config, s.next_run, s.repeat_minutes],
            )?;
            let id = c.last_insert_rowid();
            Ok(c.query_row(&format!("SELECT {COLUMNS} FROM schedules WHERE id = ?1"), [id], map)?)
        })
    }

    /// Every schedule, soonest first.
    pub fn schedules(&self) -> Result<Vec<ScheduleRow>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM schedules ORDER BY next_run, id"
            ))?;
            let rows = stmt.query_map([], map)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Enabled schedules due at `now`.
    pub fn schedules_due(&self, now: i64) -> Result<Vec<ScheduleRow>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM schedules WHERE enabled = 1 AND next_run <= ?1 ORDER BY next_run"
            ))?;
            let rows = stmt.query_map([now], map)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Records a run: the next run (or disabled when it was a one-off).
    pub fn schedule_ran(
        &self,
        id: i64,
        now: i64,
        task: Option<i64>,
        error: Option<String>,
    ) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute(
                "UPDATE schedules SET last_run = ?2, last_task_id = COALESCE(?3, last_task_id), \
                 last_error = ?4, \
                 enabled = CASE WHEN repeat_minutes = 0 THEN 0 ELSE enabled END, \
                 next_run = CASE WHEN repeat_minutes = 0 THEN next_run \
                   ELSE MAX(next_run + repeat_minutes * 60000, ?2 + repeat_minutes * 60000) END \
                 WHERE id = ?1",
                params![id, now, task, error],
            )?;
            Ok(())
        })
    }

    /// Turns a schedule on or off.
    pub fn schedule_set_enabled(&self, id: i64, enabled: bool) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute(
                "UPDATE schedules SET enabled = ?2 WHERE id = ?1",
                params![id, enabled],
            )?;
            Ok(())
        })
    }

    /// Deletes a schedule.
    pub fn schedule_delete(&self, id: i64) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute("DELETE FROM schedules WHERE id = ?1", [id])?;
            Ok(())
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::OpenOptions;

    #[test]
    fn creates_runs_and_repeats() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p.chosproxy"), OpenOptions::default()).unwrap();
        let once = db
            .schedule_create(NewSchedule {
                kind: "crawl".into(),
                name: "nightly".into(),
                config: "{}".into(),
                next_run: 1000,
                repeat_minutes: 0,
            })
            .unwrap();
        let repeating = db
            .schedule_create(NewSchedule {
                kind: "discovery".into(),
                name: "hourly".into(),
                config: "{}".into(),
                next_run: 2000,
                repeat_minutes: 60,
            })
            .unwrap();
        assert!(
            db.schedule_create(NewSchedule {
                repeat_minutes: 1,
                ..NewSchedule {
                    kind: "crawl".into(),
                    name: "x".into(),
                    config: "{}".into(),
                    next_run: 0,
                    repeat_minutes: 0
                }
            })
            .is_err()
        );
        assert_eq!(db.schedules_due(500).unwrap().len(), 0);
        assert_eq!(db.schedules_due(5000).unwrap().len(), 2);
        db.schedule_ran(once.id, 5000, Some(7), None).unwrap();
        db.schedule_ran(repeating.id, 5000, None, Some("no project".into()))
            .unwrap();
        let all = db.schedules().unwrap();
        let o = all.iter().find(|s| s.id == once.id).unwrap();
        assert!(!o.enabled && o.last_task_id == Some(7));
        let r = all.iter().find(|s| s.id == repeating.id).unwrap();
        assert!(r.enabled && r.next_run == 5000 + 3_600_000 && r.last_error.is_some());
        db.schedule_delete(once.id).unwrap();
        assert_eq!(db.schedules().unwrap().len(), 1);
    }
}
