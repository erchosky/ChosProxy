//! Storage side of passive analysis: the analyzer's position in the history
//! and recording its detections as findings (source `passive`).
//!
//! A detection carries a `dedup_key`; the first one creates a finding and
//! later ones add evidence to it (at most [`MAX_PASSIVE_EVIDENCE`] pieces).
//! A finding the tester marked as a false positive or an accepted risk is
//! left alone; one marked fixed is opened again when the problem returns.

use crate::StorageError;
use crate::db::ProjectDb;
use crate::findings::{
    Confidence, FindingSeverity, check_line, check_text, copy_exchange, insert_evidence,
};
use crate::model::ExchangeId;
use rusqlite::{Connection, OptionalExtension, params};

/// Pieces of evidence kept per automatic finding.
pub const MAX_PASSIVE_EVIDENCE: i64 = 5;

/// One detection of the passive analyzer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassiveIssue {
    /// Who found it: `passive` or `plugin`.
    pub source: &'static str,
    /// The exchange it was found in.
    pub exchange_id: ExchangeId,
    /// Detections with the same key are the same finding.
    pub dedup_key: String,
    /// Type.
    pub kind: String,
    /// Title.
    pub title: String,
    /// Severity.
    pub severity: FindingSeverity,
    /// Confidence.
    pub confidence: Confidence,
    /// What the problem is.
    pub description: String,
    /// How to fix it.
    pub remediation: String,
    /// What this exchange shows.
    pub evidence_note: String,
}

/// What recording a batch changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassiveRecorded {
    /// New findings.
    pub created: u32,
    /// Findings that got evidence or were opened again.
    pub updated: u32,
    /// New detections not recorded because a suppression rule matches.
    pub suppressed: u32,
}

fn record_one(
    c: &Connection,
    issue: &PassiveIssue,
    now: i64,
    out: &mut PassiveRecorded,
) -> Result<(), StorageError> {
    let existing: Option<(i64, String)> = c
        .query_row(
            "SELECT id, status FROM findings WHERE dedup_key = ?1",
            [&issue.dedup_key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let note = check_text("evidence note", &issue.evidence_note)?;
    match existing {
        None => {
            if crate::suppressions::is_suppressed(c, &issue.kind, issue.exchange_id)? {
                out.suppressed += 1;
                return Ok(());
            }
            let copied = match copy_exchange(c, issue.exchange_id) {
                Ok(copied) => copied,
                // Removed meanwhile (e.g. history cleared): nothing to show.
                Err(StorageError::NotFound(_)) => return Ok(()),
                Err(e) => return Err(e),
            };
            let title: String = issue.title.chars().take(300).collect();
            c.execute(
                "INSERT INTO findings (created_at, updated_at, source, dedup_key, kind, title, \
                 severity, confidence, status, scheme, host, port, path, url, description, \
                 remediation) VALUES (?1, ?1, ?14, ?2, ?3, ?4, ?5, ?6, 'open', ?7, ?8, ?9, \
                 ?10, ?11, ?12, ?13)",
                params![
                    now,
                    issue.dedup_key,
                    check_line("type", &issue.kind)?,
                    check_line("title", &title)?,
                    issue.severity.as_str(),
                    issue.confidence.as_str(),
                    copied.scheme,
                    copied.host,
                    copied.port,
                    copied.path,
                    copied.url,
                    check_text("description", &issue.description)?,
                    check_text("remediation", &issue.remediation)?,
                    issue.source
                ],
            )?;
            let id = c.last_insert_rowid();
            insert_evidence(c, id, issue.exchange_id, &note, &copied)?;
            out.created += 1;
        }
        Some((_, status)) if status == "falsePositive" || status == "acceptedRisk" => {}
        Some((id, status)) => {
            let mut changed = false;
            if status == "fixed" {
                c.execute(
                    "UPDATE findings SET status = 'open', notes = trim(notes || char(10) || ?2) \
                     WHERE id = ?1",
                    params![
                        id,
                        "Seen again by an automatic check after being marked fixed."
                    ],
                )?;
                changed = true;
            }
            let (count, already): (i64, bool) = c.query_row(
                "SELECT COUNT(*), COALESCE(SUM(exchange_id = ?2), 0) > 0 \
                 FROM finding_evidence WHERE finding_id = ?1",
                params![id, issue.exchange_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if count < MAX_PASSIVE_EVIDENCE && !already {
                match copy_exchange(c, issue.exchange_id) {
                    Ok(copied) => {
                        insert_evidence(c, id, issue.exchange_id, &note, &copied)?;
                        changed = true;
                    }
                    Err(StorageError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }
            }
            if changed {
                c.execute(
                    "UPDATE findings SET updated_at = ?2 WHERE id = ?1",
                    params![id, now],
                )?;
                out.updated += 1;
            }
        }
    }
    Ok(())
}

/// A request a crawl made: id, URL, status and its notes.
pub type CrawlExchange = (ExchangeId, String, Option<u16>, String);

impl ProjectDb {
    /// The last exchange id the analyzer has looked at.
    pub fn passive_cursor(&self) -> Result<ExchangeId, StorageError> {
        self.with_reader(|c| {
            Ok(c.query_row(
                "SELECT last_exchange_id FROM passive_state WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
        })
    }

    /// Moves the analyzer's position (0 analyzes all traffic again).
    pub fn passive_set_cursor(&self, cursor: ExchangeId) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute(
                "UPDATE passive_state SET last_exchange_id = ?1, updated_at = ?2 WHERE id = 1",
                params![cursor, chosproxy_core::time::now_ms()],
            )?;
            Ok(())
        })
    }

    /// Ids of stored exchanges after `after`, oldest first, at most `limit`.
    pub fn exchange_ids_after(
        &self,
        after: ExchangeId,
        limit: u32,
    ) -> Result<Vec<ExchangeId>, StorageError> {
        self.with_reader(|c| {
            let mut stmt =
                c.prepare("SELECT id FROM exchanges WHERE id > ?1 ORDER BY id LIMIT ?2")?;
            let ids = stmt
                .query_map(params![after, limit], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ids)
        })
    }

    /// Distinct URLs of the exchanges up to `up_to`, at most `limit`.
    pub fn exchange_urls(
        &self,
        up_to: ExchangeId,
        limit: u32,
    ) -> Result<Vec<String>, StorageError> {
        self.with_reader(|c| {
            let mut stmt =
                c.prepare("SELECT DISTINCT url FROM exchanges WHERE id <= ?1 LIMIT ?2")?;
            let urls = stmt
                .query_map(params![up_to, limit], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(urls)
        })
    }

    /// Proxy exchanges stored after `after`, oldest first, at most `limit`:
    /// id, method and path extension (for picking the requests of a
    /// recorded login).
    pub fn proxy_exchanges_after(
        &self,
        after: ExchangeId,
        limit: u32,
    ) -> Result<Vec<(ExchangeId, String, Option<String>)>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(
                "SELECT id, method, extension FROM exchanges WHERE id > ?1 AND source = 'proxy' \
                 ORDER BY id LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![after, limit], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Requests a crawl made between two times (ms), oldest first, at most
    /// `limit`: id, URL, status and the notes that say why each was made.
    pub fn crawl_exchanges(
        &self,
        from: i64,
        to: i64,
        limit: u32,
    ) -> Result<Vec<CrawlExchange>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(
                "SELECT id, url, status, notes FROM exchanges WHERE started_at BETWEEN ?1 AND ?2 \
                 AND source IN ('crawler', 'scanner') AND notes LIKE '%Crawler: %' \
                 ORDER BY id LIMIT ?3",
            )?;
            let rows = stmt
                .query_map(params![from, to, limit], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// The highest exchange id stored (0 when empty).
    pub fn last_exchange_id(&self) -> Result<ExchangeId, StorageError> {
        self.with_reader(|c| {
            Ok(
                c.query_row("SELECT COALESCE(MAX(id), 0) FROM exchanges", [], |r| {
                    r.get(0)
                })?,
            )
        })
    }

    /// Records detections of an automatic source (a plugin) as findings,
    /// merged by dedup key like passive ones.
    pub fn findings_record_auto(
        &self,
        issues: Vec<PassiveIssue>,
    ) -> Result<PassiveRecorded, StorageError> {
        self.write(move |c| {
            let now = chosproxy_core::time::now_ms();
            let mut out = PassiveRecorded::default();
            for issue in &issues {
                record_one(c, issue, now, &mut out)?;
            }
            Ok(out)
        })
    }

    /// Records detections as findings and moves the analyzer to `cursor`, in
    /// one transaction.
    pub fn passive_record(
        &self,
        issues: Vec<PassiveIssue>,
        cursor: ExchangeId,
    ) -> Result<PassiveRecorded, StorageError> {
        self.write(move |c| {
            let now = chosproxy_core::time::now_ms();
            let mut out = PassiveRecorded::default();
            for issue in &issues {
                record_one(c, issue, now, &mut out)?;
            }
            c.execute(
                "UPDATE passive_state SET last_exchange_id = MAX(last_exchange_id, ?1), \
                 updated_at = ?2 WHERE id = 1",
                params![cursor, now],
            )?;
            Ok(out)
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::OpenOptions;
    use crate::findings::{FindingQuery, FindingStatus, FindingUpdate};
    use crate::model::NewExchange;
    use chosproxy_core::Source;

    fn exchange(path: &str) -> NewExchange {
        NewExchange {
            started_at: 1,
            duration_ms: Some(5),
            source: Source::Proxy,
            listener: None,
            client_addr: None,
            scheme: "https".into(),
            method: "GET".into(),
            host: "shop.example".into(),
            port: 443,
            path: path.into(),
            extension: None,
            url: format!("https://shop.example{path}"),
            http_version: "HTTP/1.1".into(),
            status: Some(200),
            reason: None,
            mime: None,
            request: format!("GET {path} HTTP/1.1\r\n\r\n").into_bytes(),
            response: Some(b"HTTP/1.1 200 OK\r\n\r\n".to_vec()),
            original_request: None,
            original_response: None,
            request_len: 10,
            response_len: Some(19),
            server_ip: None,
            tls: true,
            error: None,
            notes: None,
            flags: 0,
            ws_key: None,
        }
    }

    fn issue(exchange_id: i64) -> PassiveIssue {
        PassiveIssue {
            source: "passive",
            exchange_id,
            dedup_key: "passive:hsts:https://shop.example:443".into(),
            kind: "Strict Transport Security".into(),
            title: "Strict-Transport-Security missing on shop.example".into(),
            severity: FindingSeverity::Low,
            confidence: Confidence::Certain,
            description: "HSTS makes browsers use HTTPS.".into(),
            remediation: "Send the header.".into(),
            evidence_note: "No Strict-Transport-Security".into(),
        }
    }

    #[test]
    fn merges_detections_and_respects_reviews() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let ids: Vec<i64> = (0..8)
            .map(|i| db.insert(exchange(&format!("/p{i}"))).unwrap())
            .collect();
        assert_eq!(db.passive_cursor().unwrap(), 0);
        assert_eq!(
            db.exchange_ids_after(ids[5], 10).unwrap(),
            vec![ids[6], ids[7]]
        );
        assert_eq!(db.last_exchange_id().unwrap(), ids[7]);

        let first = db.passive_record(vec![issue(ids[0])], ids[0]).unwrap();
        assert_eq!(
            first,
            PassiveRecorded {
                created: 1,
                updated: 0,
                suppressed: 0
            }
        );
        // The same exchange again changes nothing; others add evidence up to the cap.
        let again = db
            .passive_record(ids.iter().map(|id| issue(*id)).collect(), ids[7])
            .unwrap();
        assert_eq!(
            again,
            PassiveRecorded {
                created: 0,
                updated: 4,
                suppressed: 0
            }
        );
        assert_eq!(db.passive_cursor().unwrap(), ids[7]);

        let rows = db
            .findings(&FindingQuery {
                source: Some("passive".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].evidence_count, 5);
        assert_eq!(rows[0].url, "https://shop.example/p0");
        let id = rows[0].id;

        let set_status = |status| {
            db.finding_update(
                id,
                FindingUpdate {
                    kind: "Strict Transport Security".into(),
                    title: "t".into(),
                    severity: FindingSeverity::Low,
                    confidence: Confidence::Certain,
                    status,
                    description: String::new(),
                    remediation: String::new(),
                    notes: String::new(),
                },
            )
            .unwrap();
        };
        set_status(FindingStatus::FalsePositive);
        let ignored = db.passive_record(vec![issue(ids[6])], ids[7]).unwrap();
        assert_eq!(ignored, PassiveRecorded::default());

        set_status(FindingStatus::Fixed);
        let reopened = db.passive_record(vec![issue(ids[6])], ids[7]).unwrap();
        assert_eq!(reopened.updated, 1);
        let f = db.finding(id).unwrap().unwrap();
        assert_eq!(f.row.status, FindingStatus::Open);
        assert!(f.notes.contains("after being marked fixed"));

        // A cursor never moves back by recording; reset does.
        db.passive_record(Vec::new(), 1).unwrap();
        assert_eq!(db.passive_cursor().unwrap(), ids[7]);
        db.passive_set_cursor(0).unwrap();
        assert_eq!(db.passive_cursor().unwrap(), 0);
        assert!(
            db.findings(&FindingQuery {
                source: Some("manual".into()),
                ..Default::default()
            })
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn suppression_rules_mark_open_findings_and_skip_new_ones() {
        use crate::{FindingStatus, NewSuppression};
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let a = db.insert(exchange("/a")).unwrap();
        let b = db.insert(exchange("/static/b")).unwrap();
        db.passive_record(vec![issue(a)], a).unwrap();
        let rule = NewSuppression {
            kind: "Strict Transport Security".into(),
            host: "*.example".into(),
            path_prefix: String::new(),
            reason: "HSTS is set by the CDN".into(),
        };
        let (saved, marked) = db.suppression_add(rule.clone(), true).unwrap();
        assert_eq!(marked, 1);
        assert_eq!(saved.host, "*.example");
        let rows = db.findings(&FindingQuery::default()).unwrap();
        assert_eq!(rows[0].status, FindingStatus::FalsePositive);
        // A new detection of the same type elsewhere on the host is skipped.
        let mut other = issue(b);
        other.dedup_key = "passive:hsts:other".into();
        let recorded = db.findings_record_auto(vec![other.clone()]).unwrap();
        assert_eq!((recorded.created, recorded.suppressed), (0, 1));
        // Without the rule it is recorded.
        db.suppression_delete(saved.id).unwrap();
        assert!(db.suppressions().unwrap().is_empty());
        let recorded = db.findings_record_auto(vec![other]).unwrap();
        assert_eq!(recorded.created, 1);
        // Bad rules are refused.
        let bad = NewSuppression {
            path_prefix: "static".into(),
            ..rule
        };
        assert!(db.suppression_add(bad, false).is_err());
    }
}
