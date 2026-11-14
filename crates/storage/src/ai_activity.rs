//! The AI activity log (M30): what ChosAI and ChosAgent did, one row per
//! model call, tool call, tester message or answer. Rows are never edited;
//! an agent task's rows go with the task.

use crate::StorageError;
use crate::db::ProjectDb;
use crate::tasks::TaskId;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most characters kept in `result`.
const MAX_RESULT: usize = 2_000;
/// Most characters kept in `detail`.
const MAX_DETAIL: usize = 200_000;
/// Most rows returned at once.
pub const MAX_ACTIVITY_PAGE: u32 = 2_000;

/// One thing ChosAI or ChosAgent did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiActivity {
    /// Row id.
    #[ts(type = "number")]
    pub id: i64,
    /// Agent task (none for one-shot ChosAI questions).
    #[ts(type = "number | null")]
    pub task_id: Option<TaskId>,
    /// When (ms since the epoch).
    #[ts(type = "number")]
    pub at: i64,
    /// `goal`, `model`, `tool`, `message`, `answer`, `ask`, `error`, `stop`.
    pub action: String,
    /// Tool name (tool calls).
    pub tool: String,
    /// What it was aimed at ("#12", "https://app.example:443", a model).
    pub target: String,
    /// `allow`, `ask` or `deny` (tool calls).
    pub permission: String,
    /// `ran`, `approved`, `rejected`, `denied`, `failed`, `pending`.
    pub decision: String,
    /// Short outcome.
    pub result: String,
    /// Full text: the arguments, a reply, a message.
    pub detail: String,
}

/// A row to add (id and time are set when it is written).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewAiActivity {
    /// Agent task.
    pub task_id: Option<TaskId>,
    /// See [`AiActivity::action`].
    pub action: String,
    /// Tool name.
    pub tool: String,
    /// Target.
    pub target: String,
    /// Permission.
    pub permission: String,
    /// Decision.
    pub decision: String,
    /// Outcome.
    pub result: String,
    /// Full text.
    pub detail: String,
}

fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s.to_string(),
    }
}

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AiActivity> {
    Ok(AiActivity {
        id: r.get(0)?,
        task_id: r.get(1)?,
        at: r.get(2)?,
        action: r.get(3)?,
        tool: r.get(4)?,
        target: r.get(5)?,
        permission: r.get(6)?,
        decision: r.get(7)?,
        result: r.get(8)?,
        detail: r.get(9)?,
    })
}

const COLUMNS: &str = "id, task_id, at, action, tool, target, permission, decision, result, detail";

impl ProjectDb {
    /// Records one activity row and returns it.
    pub fn ai_activity_add(&self, row: NewAiActivity) -> Result<AiActivity, StorageError> {
        self.write(move |c| {
            let at = chosproxy_core::time::now_ms();
            let result = clip(&row.result, MAX_RESULT);
            let detail = clip(&row.detail, MAX_DETAIL);
            c.execute(
                "INSERT INTO ai_activity \
                 (task_id, at, action, tool, target, permission, decision, result, detail) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    row.task_id,
                    at,
                    row.action,
                    row.tool,
                    row.target,
                    row.permission,
                    row.decision,
                    result,
                    detail
                ],
            )?;
            Ok(AiActivity {
                id: c.last_insert_rowid(),
                task_id: row.task_id,
                at,
                action: row.action,
                tool: row.tool,
                target: row.target,
                permission: row.permission,
                decision: row.decision,
                result,
                detail,
            })
        })
    }

    /// Activity rows, oldest first: of one task, or of every task and
    /// question (`task` none), after row `after`, at most `limit`.
    pub fn ai_activity(
        &self,
        task: Option<TaskId>,
        after: i64,
        limit: u32,
    ) -> Result<Vec<AiActivity>, StorageError> {
        let limit = limit.clamp(1, MAX_ACTIVITY_PAGE);
        self.with_reader(move |c| {
            let rows = match task {
                Some(t) => {
                    let mut stmt = c.prepare(&format!(
                        "SELECT {COLUMNS} FROM ai_activity WHERE task_id = ?1 AND id > ?2 \
                         ORDER BY id LIMIT ?3"
                    ))?;
                    stmt.query_map(params![t, after, limit], map_row)?
                        .collect::<Result<Vec<_>, _>>()?
                }
                None => {
                    let mut stmt = c.prepare(&format!(
                        "SELECT {COLUMNS} FROM ai_activity WHERE id > ?1 ORDER BY id LIMIT ?2"
                    ))?;
                    stmt.query_map(params![after, limit], map_row)?
                        .collect::<Result<Vec<_>, _>>()?
                }
            };
            Ok(rows)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpenOptions;

    #[test]
    fn records_and_pages_activity_and_drops_it_with_the_task() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let task = db.task_create("agent", "Agent", "{}").unwrap();
        let first = db
            .ai_activity_add(NewAiActivity {
                task_id: Some(task.id),
                action: "tool".into(),
                tool: "get_exchange".into(),
                target: "#1".into(),
                permission: "allow".into(),
                decision: "ran".into(),
                result: "x".repeat(5000),
                detail: "{}".into(),
            })
            .unwrap();
        assert!(first.result.chars().count() <= MAX_RESULT + 1);
        db.ai_activity_add(NewAiActivity {
            action: "ask".into(),
            ..NewAiActivity::default()
        })
        .unwrap();
        assert_eq!(db.ai_activity(Some(task.id), 0, 100).unwrap().len(), 1);
        assert_eq!(db.ai_activity(None, 0, 100).unwrap().len(), 2);
        assert_eq!(db.ai_activity(None, first.id, 100).unwrap().len(), 1);
        db.task_set_status(task.id, crate::TaskStatus::Completed, None)
            .unwrap();
        db.task_delete(task.id).unwrap();
        assert!(db.ai_activity(Some(task.id), 0, 100).unwrap().is_empty());
        assert_eq!(db.ai_activity(None, 0, 100).unwrap().len(), 1);
    }
}
