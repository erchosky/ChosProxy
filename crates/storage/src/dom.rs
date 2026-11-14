//! DOM Inspector observations in the project (ADR-0027).

use crate::StorageError;
use crate::db::ProjectDb;
use crate::model::ExchangeId;
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Observations kept per project (oldest dropped beyond).
pub const MAX_DOM_EVENTS: i64 = 20_000;

/// One observation to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDomEvent {
    /// Unix epoch ms.
    pub at: i64,
    /// `flow`, `redirect`, `messageListener`, `prototypePollution`, `clobbering`.
    pub kind: String,
    /// The sink.
    pub sink: String,
    /// The source.
    pub source: Option<String>,
    /// The source's value.
    pub source_value: Option<String>,
    /// The part of the source found in the sink value.
    pub matched: Option<String>,
    /// How it changed on the way.
    pub transformation: Option<String>,
    /// Value at the sink.
    pub value: String,
    /// The session canary reached the sink.
    pub canary: bool,
    /// Message listeners: the code mentions `origin`.
    pub checks_origin: Option<bool>,
    /// URL of the frame.
    pub frame_url: String,
    /// The frame is the top-level page.
    pub top_frame: bool,
    /// `file:line:col` of the sink call.
    pub location: Option<String>,
    /// JavaScript stack.
    pub stack: Vec<String>,
    /// Same observation, same place.
    pub dedup_key: String,
}

/// A stored observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DomEventRow {
    /// Row id.
    #[ts(type = "number")]
    pub id: i64,
    /// First seen, Unix epoch ms.
    #[ts(type = "number")]
    pub first_at: i64,
    /// Last seen, Unix epoch ms.
    #[ts(type = "number")]
    pub last_at: i64,
    /// Times seen.
    #[ts(type = "number")]
    pub hits: i64,
    /// Kind.
    pub kind: String,
    /// Sink.
    pub sink: String,
    /// Source.
    pub source: Option<String>,
    /// Source value.
    pub source_value: Option<String>,
    /// Matched part.
    pub matched: Option<String>,
    /// Transformation.
    pub transformation: Option<String>,
    /// Value at the sink.
    pub value: String,
    /// The canary reached the sink.
    pub canary: bool,
    /// Message listeners: the code mentions `origin`.
    pub checks_origin: Option<bool>,
    /// Frame URL.
    pub frame_url: String,
    /// Top-level frame.
    pub top_frame: bool,
    /// `file:line:col`.
    pub location: Option<String>,
    /// Stack lines.
    pub stack: Vec<String>,
    /// The stored response that delivered the page, if found.
    #[ts(type = "number | null")]
    pub exchange_id: Option<ExchangeId>,
}

const COLUMNS: &str = "id, first_at, last_at, hits, kind, sink, source, source_value, matched, \
    transformation, value, canary, checks_origin, frame_url, top_frame, location, stack, exchange_id";

fn map(r: &Row<'_>) -> rusqlite::Result<DomEventRow> {
    let stack: String = r.get(16)?;
    Ok(DomEventRow {
        id: r.get(0)?,
        first_at: r.get(1)?,
        last_at: r.get(2)?,
        hits: r.get(3)?,
        kind: r.get(4)?,
        sink: r.get(5)?,
        source: r.get(6)?,
        source_value: r.get(7)?,
        matched: r.get(8)?,
        transformation: r.get(9)?,
        value: r.get(10)?,
        canary: r.get(11)?,
        checks_origin: r.get(12)?,
        frame_url: r.get(13)?,
        top_frame: r.get(14)?,
        location: r.get(15)?,
        stack: if stack.is_empty() {
            Vec::new()
        } else {
            stack.split('\n').map(str::to_string).collect()
        },
        exchange_id: r.get(17)?,
    })
}

/// The URL without its fragment (how it appears in the history).
pub fn without_fragment(url: &str) -> &str {
    url.split_once('#').map_or(url, |(u, _)| u)
}

impl ProjectDb {
    /// Stores observations, merging repeats. Returns the rows that are new
    /// or changed.
    pub fn dom_record(&self, events: Vec<NewDomEvent>) -> Result<Vec<DomEventRow>, StorageError> {
        self.write(move |c| {
            let mut ids = Vec::new();
            for e in &events {
                let exchange: Option<i64> = c
                    .query_row(
                        "SELECT id FROM exchanges WHERE url = ?1 ORDER BY id DESC LIMIT 1",
                        [without_fragment(&e.frame_url)],
                        |r| r.get(0),
                    )
                    .optional()?;
                let existing: Option<i64> = c
                    .query_row("SELECT id FROM dom_events WHERE dedup_key = ?1", [&e.dedup_key], |r| r.get(0))
                    .optional()?;
                let id = match existing {
                    Some(id) => {
                        c.execute(
                            "UPDATE dom_events SET hits = hits + 1, last_at = MAX(last_at, ?2), \
                             exchange_id = COALESCE(?3, exchange_id) WHERE id = ?1",
                            params![id, e.at, exchange],
                        )?;
                        id
                    }
                    None => {
                        c.execute(
                            "INSERT INTO dom_events (first_at, last_at, kind, sink, source, source_value, \
                             matched, transformation, value, canary, checks_origin, frame_url, top_frame, \
                             location, stack, exchange_id, dedup_key) VALUES \
                             (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                            params![
                                e.at,
                                e.kind,
                                e.sink,
                                e.source,
                                e.source_value,
                                e.matched,
                                e.transformation,
                                e.value,
                                e.canary,
                                e.checks_origin,
                                e.frame_url,
                                e.top_frame,
                                e.location,
                                e.stack.join("\n"),
                                exchange,
                                e.dedup_key
                            ],
                        )?;
                        c.last_insert_rowid()
                    }
                };
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            c.execute(
                "DELETE FROM dom_events WHERE id IN (SELECT id FROM dom_events ORDER BY last_at DESC \
                 LIMIT -1 OFFSET ?1)",
                [MAX_DOM_EVENTS],
            )?;
            let mut out = Vec::new();
            let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM dom_events WHERE id = ?1"))?;
            for id in ids {
                if let Some(row) = stmt.query_row([id], map).optional()? {
                    out.push(row);
                }
            }
            Ok(out)
        })
    }

    /// Observations, most recent first.
    pub fn dom_events(&self, limit: u32) -> Result<Vec<DomEventRow>, StorageError> {
        let limit = limit.clamp(1, 5000);
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM dom_events ORDER BY last_at DESC, id DESC LIMIT ?1"
            ))?;
            let rows = stmt
                .query_map([limit], map)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Deletes every observation.
    pub fn dom_clear(&self) -> Result<(), StorageError> {
        self.write(|c| {
            c.execute("DELETE FROM dom_events", [])?;
            Ok(())
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::OpenOptions;

    fn event(key: &str, at: i64) -> NewDomEvent {
        NewDomEvent {
            at,
            kind: "flow".into(),
            sink: "element.innerHTML".into(),
            source: Some("location.hash".into()),
            source_value: Some("x".into()),
            matched: Some("chosabc".into()),
            transformation: Some("unchanged".into()),
            value: "<b>chosabc</b>".into(),
            canary: true,
            checks_origin: None,
            frame_url: "http://lab.test/dom/hash#chosabc".into(),
            top_frame: true,
            location: Some("http://lab.test/dom/hash:5:3".into()),
            stack: vec!["at a".into(), "at b".into()],
            dedup_key: key.into(),
        }
    }

    #[test]
    fn records_merges_and_lists() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p.chosproxy"), OpenOptions::default()).unwrap();
        let first = db.dom_record(vec![event("k", 1)]).unwrap();
        assert_eq!(
            (first.len(), first[0].hits, first[0].exchange_id),
            (1, 1, None)
        );
        let again = db.dom_record(vec![event("k", 5), event("k2", 3)]).unwrap();
        assert_eq!(again.len(), 2);
        assert_eq!((again[0].hits, again[0].last_at), (2, 5));
        let all = db.dom_events(10).unwrap();
        assert_eq!(all[0].stack, vec!["at a".to_string(), "at b".to_string()]);
        assert!(all[0].canary);
        db.dom_clear().unwrap();
        assert!(db.dom_events(10).unwrap().is_empty());
        assert_eq!(without_fragment("http://a/b?c#d"), "http://a/b?c");
    }
}
