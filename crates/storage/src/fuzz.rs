//! Fuzzer results: one row per request of an attack (task kind `fuzz`).

use crate::StorageError;
use crate::db::ProjectDb;
use crate::model::{ExchangeId, Highlight};
use crate::tasks::TaskId;
use rusqlite::types::Value;
use rusqlite::{Row, params, params_from_iter};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most rows returned per page.
const MAX_PAGE: u32 = 5000;

/// One request of an attack, to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFuzzResult {
    /// Request number (0 = baseline).
    pub idx: u64,
    /// Payload(s) used.
    pub payloads: Vec<String>,
    /// Position changed (sniper).
    pub position: Option<u32>,
    /// The stored exchange.
    pub exchange_id: Option<ExchangeId>,
    /// Response status.
    pub status: Option<u16>,
    /// Response length.
    pub length: Option<u64>,
    /// Time taken.
    pub duration_ms: u64,
    /// Why no response arrived.
    pub error: Option<String>,
    /// Match terms found.
    pub matches: Vec<bool>,
    /// Extracted values.
    pub extracts: Vec<String>,
}

/// A result row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FuzzResultRow {
    /// Row id.
    #[ts(type = "number")]
    pub id: i64,
    /// Request number (0 = baseline with the original values).
    #[ts(type = "number")]
    pub idx: u64,
    /// Payload(s) used.
    pub payloads: Vec<String>,
    /// Position changed (sniper).
    pub position: Option<u32>,
    /// The stored exchange.
    #[ts(type = "number | null")]
    pub exchange_id: Option<ExchangeId>,
    /// Response status.
    pub status: Option<u16>,
    /// Response length.
    #[ts(type = "number | null")]
    pub length: Option<u64>,
    /// Time taken, ms.
    #[ts(type = "number")]
    pub duration_ms: u64,
    /// Why no response arrived.
    pub error: Option<String>,
    /// One flag per match term.
    pub matches: Vec<bool>,
    /// One value per extract pattern.
    pub extracts: Vec<String>,
    /// Highlight colour.
    pub color: Option<Highlight>,
}

/// Column to sort results by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FuzzSort {
    /// Request number.
    #[default]
    Index,
    /// Status code.
    Status,
    /// Response length.
    Length,
    /// Time taken.
    Duration,
}

/// Which results to list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct FuzzResultQuery {
    /// Sort column.
    pub sort: FuzzSort,
    /// Descending.
    pub descending: bool,
    /// Only rows where a match term was found.
    pub matched_only: bool,
    /// Only rows whose status differs from the baseline's, or that failed.
    pub different_status_only: bool,
    /// Rows skipped.
    pub offset: u32,
    /// Rows returned (at most 5000).
    pub limit: u32,
}

/// A page of results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FuzzResultsPage {
    /// Rows.
    pub rows: Vec<FuzzResultRow>,
    /// Rows matching the query.
    #[ts(type = "number")]
    pub total: u64,
}

/// JSON for the small lists stored in result rows.
fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn map_row(r: &Row<'_>) -> rusqlite::Result<FuzzResultRow> {
    let json = |i: usize| -> rusqlite::Result<String> { r.get(i) };
    Ok(FuzzResultRow {
        id: r.get(0)?,
        idx: u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
        payloads: serde_json::from_str(&json(2)?).unwrap_or_default(),
        position: r.get(3)?,
        exchange_id: r.get(4)?,
        status: r.get(5)?,
        length: r
            .get::<_, Option<i64>>(6)?
            .map(|n| u64::try_from(n).unwrap_or(0)),
        duration_ms: u64::try_from(r.get::<_, i64>(7)?).unwrap_or(0),
        error: r.get(8)?,
        matches: serde_json::from_str(&json(9)?).unwrap_or_default(),
        extracts: serde_json::from_str(&json(10)?).unwrap_or_default(),
        color: Highlight::parse(&r.get::<_, String>(11)?),
    })
}

const RESULT_COLUMNS: &str = "id, idx, payloads, position, exchange_id, status, length, \
    duration_ms, error, matches, extracts, color";

impl ProjectDb {
    /// Stores results of an attack.
    pub fn fuzz_record(&self, task: TaskId, rows: Vec<NewFuzzResult>) -> Result<(), StorageError> {
        if rows.is_empty() {
            return Ok(());
        }
        self.write(move |c| {
            for r in rows {
                c.execute(
                    "INSERT INTO fuzz_results (task_id, idx, payloads, position, exchange_id, status, \
                     length, duration_ms, error, matched, matches, extracts) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        task,
                        to_i64(r.idx),
                        json(&r.payloads),
                        r.position,
                        r.exchange_id,
                        r.status,
                        r.length.map(to_i64),
                        to_i64(r.duration_ms),
                        r.error,
                        r.matches.iter().any(|m| *m),
                        json(&r.matches),
                        json(&r.extracts)
                    ],
                )?;
            }
            Ok(())
        })
    }

    /// A page of an attack's results.
    pub fn fuzz_results(
        &self,
        task: TaskId,
        q: &FuzzResultQuery,
    ) -> Result<FuzzResultsPage, StorageError> {
        let mut clauses = vec!["task_id = ?".to_string()];
        let args = vec![Value::Integer(task)];
        if q.matched_only {
            clauses.push("matched = 1".into());
        }
        if q.different_status_only {
            clauses.push(
                "(status IS NULL OR status IS NOT (SELECT status FROM fuzz_results b \
                 WHERE b.task_id = fuzz_results.task_id AND b.idx = 0))"
                    .into(),
            );
        }
        let where_sql = clauses.join(" AND ");
        let column = match q.sort {
            FuzzSort::Index => "idx",
            FuzzSort::Status => "COALESCE(status, 0)",
            FuzzSort::Length => "COALESCE(length, -1)",
            FuzzSort::Duration => "duration_ms",
        };
        let order = if q.descending { "DESC" } else { "ASC" };
        let limit = q.limit.clamp(1, MAX_PAGE);
        self.with_reader(|c| {
            let total: i64 = c.query_row(
                &format!("SELECT COUNT(*) FROM fuzz_results WHERE {where_sql}"),
                params_from_iter(args.iter()),
                |r| r.get(0),
            )?;
            let mut page_args = args.clone();
            page_args.push(Value::Integer(i64::from(limit)));
            page_args.push(Value::Integer(i64::from(q.offset)));
            let mut stmt = c.prepare(&format!(
                "SELECT {RESULT_COLUMNS} FROM fuzz_results WHERE {where_sql} \
                 ORDER BY {column} {order}, idx LIMIT ? OFFSET ?"
            ))?;
            let rows = stmt
                .query_map(params_from_iter(page_args.iter()), map_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(FuzzResultsPage {
                rows,
                total: u64::try_from(total).unwrap_or(0),
            })
        })
    }

    /// Every result of an attack, in request order (for export).
    pub fn fuzz_all_results(&self, task: TaskId) -> Result<Vec<FuzzResultRow>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {RESULT_COLUMNS} FROM fuzz_results WHERE task_id = ?1 ORDER BY idx"
            ))?;
            let rows = stmt
                .query_map([task], map_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Sets or clears a result's highlight colour.
    pub fn fuzz_set_color(
        &self,
        id: i64,
        color: Option<Highlight>,
    ) -> Result<FuzzResultRow, StorageError> {
        self.write(move |c| {
            let n = c.execute(
                "UPDATE fuzz_results SET color = ?2 WHERE id = ?1",
                params![id, color.map(Highlight::as_str).unwrap_or_default()],
            )?;
            if n == 0 {
                return Err(StorageError::NotFound(id));
            }
            Ok(c.query_row(
                &format!("SELECT {RESULT_COLUMNS} FROM fuzz_results WHERE id = ?1"),
                [id],
                map_row,
            )?)
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::OpenOptions;

    fn result(idx: u64, status: Option<u16>, length: u64, matched: bool) -> NewFuzzResult {
        NewFuzzResult {
            idx,
            payloads: vec![format!("p{idx}")],
            position: Some(0),
            exchange_id: None,
            status,
            length: Some(length),
            duration_ms: idx * 10,
            error: status.is_none().then(|| "timeout".into()),
            matches: vec![matched, false],
            extracts: vec![format!("x{idx}")],
        }
    }

    #[test]
    fn stores_sorts_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let task = db.task_create("fuzz", "Fuzz", "{}").unwrap();
        db.fuzz_record(
            task.id,
            vec![
                result(0, Some(200), 100, false),
                result(1, Some(200), 120, true),
                result(2, Some(500), 90, false),
                result(3, None, 0, false),
            ],
        )
        .unwrap();
        let all = db
            .fuzz_results(
                task.id,
                &FuzzResultQuery {
                    limit: 10,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(all.total, 4);
        assert_eq!(all.rows[1].payloads, ["p1"]);
        assert_eq!(all.rows[1].matches, [true, false]);
        assert_eq!(all.rows[1].extracts, ["x1"]);
        assert_eq!(all.rows[1].color, None);

        // Colours and the full export list.
        let red = db
            .fuzz_set_color(all.rows[2].id, Some(Highlight::Red))
            .unwrap();
        assert_eq!(red.color, Some(Highlight::Red));
        assert!(db.fuzz_set_color(9_999, None).is_err());
        let every = db.fuzz_all_results(task.id).unwrap();
        assert_eq!(
            every.iter().map(|r| r.idx).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(every[2].color, Some(Highlight::Red));
        assert_eq!(db.fuzz_set_color(red.id, None).unwrap().color, None);

        let by_length = db
            .fuzz_results(
                task.id,
                &FuzzResultQuery {
                    sort: FuzzSort::Length,
                    descending: true,
                    limit: 10,
                    ..Default::default()
                },
            )
            .unwrap();
        let idx: Vec<u64> = by_length.rows.iter().map(|r| r.idx).collect();
        assert_eq!(idx, [1, 0, 2, 3]);

        let matched = db
            .fuzz_results(
                task.id,
                &FuzzResultQuery {
                    matched_only: true,
                    limit: 10,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(matched.rows.iter().map(|r| r.idx).collect::<Vec<_>>(), [1]);
        let different = db
            .fuzz_results(
                task.id,
                &FuzzResultQuery {
                    different_status_only: true,
                    limit: 10,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            different.rows.iter().map(|r| r.idx).collect::<Vec<_>>(),
            [2, 3]
        );

        let page = db
            .fuzz_results(
                task.id,
                &FuzzResultQuery {
                    offset: 3,
                    limit: 2,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!((page.total, page.rows.len()), (4, 1));

        db.task_set_status(task.id, crate::TaskStatus::Completed, None)
            .unwrap();
        db.task_delete(task.id).unwrap();
        assert_eq!(
            db.fuzz_results(
                task.id,
                &FuzzResultQuery {
                    limit: 10,
                    ..Default::default()
                }
            )
            .unwrap()
            .total,
            0
        );
    }
}
