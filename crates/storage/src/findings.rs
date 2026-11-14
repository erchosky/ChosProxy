//! Findings: problems the user documents, each with one or more pieces of
//! evidence (copies of the requests and responses that show it).

use crate::db::ProjectDb;
use crate::macros::string_enum;
use crate::model::ExchangeId;
use crate::{StorageError, blob};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Row id of a finding.
pub type FindingId = i64;

string_enum!(
    /// How serious a finding is.
    FindingSeverity {
        /// Informational.
        Info => "info",
        /// Low.
        Low => "low",
        /// Medium.
        Medium => "medium",
        /// High.
        High => "high",
        /// Critical.
        Critical => "critical",
    }
);

string_enum!(
    /// How sure the finding is.
    Confidence {
        /// Needs confirmation.
        Tentative => "tentative",
        /// Very likely.
        Firm => "firm",
        /// Confirmed by evidence.
        Certain => "certain",
    }
);

string_enum!(
    /// Review status. False positives and accepted risks are kept as records.
    FindingStatus {
        /// Not reviewed yet.
        Open => "open",
        /// Reviewed and real.
        Confirmed => "confirmed",
        /// Not a real problem.
        FalsePositive => "falsePositive",
        /// Real, but accepted by the owner.
        AcceptedRisk => "acceptedRisk",
        /// Fixed and verified.
        Fixed => "fixed",
    }
);

/// A finding as listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FindingRow {
    /// Finding id.
    #[ts(type = "number")]
    pub id: FindingId,
    /// Created, Unix epoch ms.
    #[ts(type = "number")]
    pub created_at: i64,
    /// Last change, Unix epoch ms.
    #[ts(type = "number")]
    pub updated_at: i64,
    /// Where it came from (`manual` today).
    pub source: String,
    /// Type, e.g. "Missing security header".
    pub kind: String,
    /// Title.
    pub title: String,
    /// Severity.
    pub severity: FindingSeverity,
    /// Confidence.
    pub confidence: Confidence,
    /// Review status.
    pub status: FindingStatus,
    /// Scheme of the affected location.
    pub scheme: String,
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Path (without query).
    pub path: String,
    /// URL of the first evidence.
    pub url: String,
    /// Pieces of evidence.
    #[ts(type = "number")]
    pub evidence_count: u64,
    /// In the current scope.
    pub in_scope: bool,
}

/// One piece of evidence without its messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EvidenceRow {
    /// Evidence id.
    #[ts(type = "number")]
    pub id: i64,
    /// Added, Unix epoch ms.
    #[ts(type = "number")]
    pub created_at: i64,
    /// History entry it was copied from.
    #[ts(type = "number | null")]
    pub exchange_id: Option<ExchangeId>,
    /// What this evidence shows.
    pub note: String,
    /// Request method.
    pub method: String,
    /// Request URL.
    pub url: String,
    /// Response status.
    pub status_code: Option<u16>,
}

/// Evidence with its copied messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvidence {
    /// Metadata.
    pub row: EvidenceRow,
    /// Raw request.
    pub request: Vec<u8>,
    /// Raw response.
    pub response: Option<Vec<u8>>,
}

/// A finding with its texts and evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFinding {
    /// Listing data.
    pub row: FindingRow,
    /// What the problem is.
    pub description: String,
    /// How to fix it.
    pub remediation: String,
    /// The tester's notes.
    pub notes: String,
    /// Evidence, oldest first.
    pub evidence: Vec<StoredEvidence>,
}

/// A new finding, documented from a history entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewFinding {
    /// History entry used as first evidence (its URL is the location).
    #[ts(type = "number")]
    pub exchange_id: ExchangeId,
    /// Type.
    pub kind: String,
    /// Title.
    pub title: String,
    /// Severity.
    pub severity: FindingSeverity,
    /// Confidence.
    pub confidence: Confidence,
    /// What the problem is.
    #[serde(default)]
    pub description: String,
    /// How to fix it.
    #[serde(default)]
    pub remediation: String,
    /// What the first evidence shows.
    #[serde(default)]
    pub evidence_note: String,
}

/// Editable fields of a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FindingUpdate {
    /// Type.
    pub kind: String,
    /// Title.
    pub title: String,
    /// Severity.
    pub severity: FindingSeverity,
    /// Confidence.
    pub confidence: Confidence,
    /// Review status.
    pub status: FindingStatus,
    /// What the problem is.
    pub description: String,
    /// How to fix it.
    pub remediation: String,
    /// The tester's notes.
    pub notes: String,
}

/// Finding filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct FindingQuery {
    /// Case-insensitive text in title, type, host, path or notes.
    pub search: Option<String>,
    /// Only these severities (empty = all).
    pub severities: Vec<FindingSeverity>,
    /// Only these statuses (empty = all).
    pub statuses: Vec<FindingStatus>,
    /// Only findings in scope.
    pub in_scope_only: bool,
    /// Only findings from this source (`manual`, `passive`); all if empty.
    pub source: Option<String>,
}

/// Open and confirmed findings per severity (for the dashboard).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FindingCounts {
    /// Informational.
    #[ts(type = "number")]
    pub info: u64,
    /// Low.
    #[ts(type = "number")]
    pub low: u64,
    /// Medium.
    #[ts(type = "number")]
    pub medium: u64,
    /// High.
    #[ts(type = "number")]
    pub high: u64,
    /// Critical.
    #[ts(type = "number")]
    pub critical: u64,
}

const MAX_TEXT: usize = 50_000;
const MAX_LINE: usize = 300;

pub(crate) fn check_line(name: &str, v: &str) -> Result<String, StorageError> {
    let v = v.trim();
    if v.is_empty() || v.chars().count() > MAX_LINE {
        return Err(StorageError::InvalidQuery(format!(
            "the {name} must be 1–{MAX_LINE} characters"
        )));
    }
    Ok(v.to_string())
}

pub(crate) fn check_text(name: &str, v: &str) -> Result<String, StorageError> {
    if v.chars().count() > MAX_TEXT {
        return Err(StorageError::InvalidQuery(format!(
            "the {name} is limited to {MAX_TEXT} characters"
        )));
    }
    Ok(v.trim_end().to_string())
}

const COLUMNS: &str = "f.id, f.created_at, f.updated_at, f.source, f.kind, f.title, f.severity, \
    f.confidence, f.status, f.scheme, f.host, f.port, f.path, f.url, \
    (SELECT COUNT(*) FROM finding_evidence e WHERE e.finding_id = f.id), \
    chos_in_scope(f.scheme, f.host, f.port, f.path)";

fn map_row(r: &Row<'_>) -> rusqlite::Result<FindingRow> {
    Ok(FindingRow {
        id: r.get(0)?,
        created_at: r.get(1)?,
        updated_at: r.get(2)?,
        source: r.get(3)?,
        kind: r.get(4)?,
        title: r.get(5)?,
        severity: FindingSeverity::parse(&r.get::<_, String>(6)?).unwrap_or(FindingSeverity::Info),
        confidence: Confidence::parse(&r.get::<_, String>(7)?).unwrap_or(Confidence::Tentative),
        status: FindingStatus::parse(&r.get::<_, String>(8)?).unwrap_or(FindingStatus::Open),
        scheme: r.get(9)?,
        host: r.get(10)?,
        port: r.get(11)?,
        path: r.get(12)?,
        url: r.get(13)?,
        evidence_count: u64::try_from(r.get::<_, i64>(14)?).unwrap_or(0),
        in_scope: r.get(15)?,
    })
}

/// What evidence copied from an exchange records.
pub(crate) struct Copied {
    method: String,
    pub(crate) url: String,
    status: Option<u16>,
    pub(crate) scheme: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path: String,
    request: i64,
    response: Option<i64>,
}

/// Copies an exchange's messages into new blobs.
pub(crate) fn copy_exchange(
    c: &Connection,
    exchange_id: ExchangeId,
) -> Result<Copied, StorageError> {
    let found = c
        .query_row(
            "SELECT method, url, status, scheme, host, port, path, request_blob, response_blob \
             FROM exchanges WHERE id = ?1",
            [exchange_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<u16>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, u16>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                ))
            },
        )
        .optional()?;
    let Some((method, url, status, scheme, host, port, path, req, res)) = found else {
        return Err(StorageError::NotFound(exchange_id));
    };
    let copy = |id: i64| -> Result<i64, StorageError> {
        let data = blob::load(c, id)?.ok_or(StorageError::CorruptBlob(id))?;
        blob::insert(c, &data)
    };
    Ok(Copied {
        method,
        url,
        status,
        scheme,
        host,
        port,
        path: path.split(['?', '#']).next().unwrap_or("/").to_string(),
        request: copy(req)?,
        response: res.map(copy).transpose()?,
    })
}

pub(crate) fn insert_evidence(
    c: &Connection,
    finding_id: FindingId,
    exchange_id: ExchangeId,
    note: &str,
    copied: &Copied,
) -> Result<i64, StorageError> {
    let inserted = c.execute(
        "INSERT INTO finding_evidence (finding_id, created_at, exchange_id, note, method, url, \
         status_code, request_blob, response_blob) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            finding_id,
            chosproxy_core::time::now_ms(),
            exchange_id,
            note,
            copied.method,
            copied.url,
            copied.status,
            copied.request,
            copied.response
        ],
    );
    match inserted {
        Ok(_) => Ok(c.last_insert_rowid()),
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Err(StorageError::InvalidQuery(format!(
                "request #{exchange_id} is already evidence of this finding"
            )))
        }
        Err(e) => Err(e.into()),
    }
}

fn delete_blobs(c: &Connection, blobs: &[(i64, Option<i64>)]) -> Result<(), StorageError> {
    for (req, res) in blobs {
        c.execute("DELETE FROM blobs WHERE id = ?1", [req])?;
        if let Some(b) = res {
            c.execute("DELETE FROM blobs WHERE id = ?1", [b])?;
        }
    }
    Ok(())
}

impl ProjectDb {
    /// Creates a finding with a history entry as its first evidence.
    pub fn finding_create(&self, new: NewFinding) -> Result<FindingId, StorageError> {
        let kind = check_line("type", &new.kind)?;
        let title = check_line("title", &new.title)?;
        let description = check_text("description", &new.description)?;
        let remediation = check_text("remediation", &new.remediation)?;
        let note = check_text("evidence note", &new.evidence_note)?;
        self.write(move |c| {
            let copied = copy_exchange(c, new.exchange_id)?;
            let now = chosproxy_core::time::now_ms();
            c.execute(
                "INSERT INTO findings (created_at, updated_at, source, kind, title, severity, \
                 confidence, status, scheme, host, port, path, url, description, remediation) \
                 VALUES (?1, ?1, 'manual', ?2, ?3, ?4, ?5, 'open', ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    now,
                    kind,
                    title,
                    new.severity.as_str(),
                    new.confidence.as_str(),
                    copied.scheme,
                    copied.host,
                    copied.port,
                    copied.path,
                    copied.url,
                    description,
                    remediation
                ],
            )?;
            let id = c.last_insert_rowid();
            insert_evidence(c, id, new.exchange_id, &note, &copied)?;
            Ok(id)
        })
    }

    /// Adds a history entry as evidence of an existing finding.
    pub fn finding_add_evidence(
        &self,
        finding_id: FindingId,
        exchange_id: ExchangeId,
        note: &str,
    ) -> Result<i64, StorageError> {
        let note = check_text("evidence note", note)?;
        self.write(move |c| {
            let exists: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM findings WHERE id = ?1)",
                [finding_id],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(StorageError::NotFound(finding_id));
            }
            let copied = copy_exchange(c, exchange_id)?;
            let id = insert_evidence(c, finding_id, exchange_id, &note, &copied)?;
            c.execute(
                "UPDATE findings SET updated_at = ?2 WHERE id = ?1",
                params![finding_id, chosproxy_core::time::now_ms()],
            )?;
            Ok(id)
        })
    }

    /// Removes one piece of evidence (the finding stays).
    pub fn finding_remove_evidence(&self, evidence_id: i64) -> Result<(), StorageError> {
        self.write(move |c| {
            let blobs = c
                .query_row(
                    "SELECT request_blob, response_blob FROM finding_evidence WHERE id = ?1",
                    [evidence_id],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)),
                )
                .optional()?
                .ok_or(StorageError::NotFound(evidence_id))?;
            c.execute("DELETE FROM finding_evidence WHERE id = ?1", [evidence_id])?;
            delete_blobs(c, &[blobs])
        })
    }

    /// Findings matching `q`, most severe and newest first.
    pub fn findings(&self, q: &FindingQuery) -> Result<Vec<FindingRow>, StorageError> {
        let mut clauses: Vec<String> = Vec::new();
        let mut args: Vec<Value> = Vec::new();
        if let Some(s) = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            clauses.push(
                "instr(lower(f.title || ' ' || f.kind || ' ' || f.host || ' ' || f.path || ' ' || f.notes), lower(?)) > 0"
                    .into(),
            );
            args.push(Value::Text(s.to_string()));
        }
        let list = |n: usize| vec!["?"; n].join(", ");
        if !q.severities.is_empty() {
            clauses.push(format!("f.severity IN ({})", list(q.severities.len())));
            args.extend(q.severities.iter().map(|s| Value::Text(s.as_str().into())));
        }
        if !q.statuses.is_empty() {
            clauses.push(format!("f.status IN ({})", list(q.statuses.len())));
            args.extend(q.statuses.iter().map(|s| Value::Text(s.as_str().into())));
        }
        if q.in_scope_only {
            clauses.push("chos_in_scope(f.scheme, f.host, f.port, f.path)".into());
        }
        if let Some(source) = q.source.as_deref().filter(|s| !s.is_empty()) {
            clauses.push("f.source = ?".into());
            args.push(Value::Text(source.to_string()));
        }
        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM findings f {where_sql} ORDER BY \
                 CASE f.severity WHEN 'critical' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 \
                 WHEN 'low' THEN 3 ELSE 4 END, f.updated_at DESC LIMIT 5000"
            ))?;
            let rows = stmt
                .query_map(params_from_iter(args.iter()), map_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// One finding with its evidence.
    pub fn finding(&self, id: FindingId) -> Result<Option<StoredFinding>, StorageError> {
        self.with_reader(|c| {
            let found = c
                .query_row(
                    &format!(
                        "SELECT {COLUMNS}, f.description, f.remediation, f.notes FROM findings f WHERE f.id = ?1"
                    ),
                    [id],
                    |r| {
                        Ok((
                            map_row(r)?,
                            r.get::<_, String>(16)?,
                            r.get::<_, String>(17)?,
                            r.get::<_, String>(18)?,
                        ))
                    },
                )
                .optional()?;
            let Some((row, description, remediation, notes)) = found else {
                return Ok(None);
            };
            let mut stmt = c.prepare(
                "SELECT id, created_at, exchange_id, note, method, url, status_code, request_blob, \
                 response_blob FROM finding_evidence WHERE finding_id = ?1 ORDER BY id",
            )?;
            let raw = stmt
                .query_map([id], |r| {
                    Ok((
                        EvidenceRow {
                            id: r.get(0)?,
                            created_at: r.get(1)?,
                            exchange_id: r.get(2)?,
                            note: r.get(3)?,
                            method: r.get(4)?,
                            url: r.get(5)?,
                            status_code: r.get(6)?,
                        },
                        r.get::<_, i64>(7)?,
                        r.get::<_, Option<i64>>(8)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let evidence = raw
                .into_iter()
                .map(|(row, req, res)| {
                    Ok(StoredEvidence {
                        row,
                        request: blob::load(c, req)?.ok_or(StorageError::CorruptBlob(req))?,
                        response: res
                            .map(|b| blob::load(c, b)?.ok_or(StorageError::CorruptBlob(b)))
                            .transpose()?,
                    })
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
            Ok(Some(StoredFinding {
                row,
                description,
                remediation,
                notes,
                evidence,
            }))
        })
    }

    /// Saves a finding's editable fields.
    pub fn finding_update(&self, id: FindingId, u: FindingUpdate) -> Result<(), StorageError> {
        let kind = check_line("type", &u.kind)?;
        let title = check_line("title", &u.title)?;
        let description = check_text("description", &u.description)?;
        let remediation = check_text("remediation", &u.remediation)?;
        let notes = check_text("notes", &u.notes)?;
        self.write(move |c| {
            let n = c.execute(
                "UPDATE findings SET kind = ?2, title = ?3, severity = ?4, confidence = ?5, \
                 status = ?6, description = ?7, remediation = ?8, notes = ?9, updated_at = ?10 \
                 WHERE id = ?1",
                params![
                    id,
                    kind,
                    title,
                    u.severity.as_str(),
                    u.confidence.as_str(),
                    u.status.as_str(),
                    description,
                    remediation,
                    notes,
                    chosproxy_core::time::now_ms()
                ],
            )?;
            if n == 0 {
                return Err(StorageError::NotFound(id));
            }
            Ok(())
        })
    }

    /// Deletes a finding and its evidence copies.
    pub fn finding_delete(&self, id: FindingId) -> Result<(), StorageError> {
        self.write(move |c| {
            let blobs = {
                let mut stmt = c.prepare(
                    "SELECT request_blob, response_blob FROM finding_evidence WHERE finding_id = ?1",
                )?;
                stmt.query_map([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?
            };
            c.execute("DELETE FROM finding_evidence WHERE finding_id = ?1", [id])?;
            if c.execute("DELETE FROM findings WHERE id = ?1", [id])? == 0 {
                return Err(StorageError::NotFound(id));
            }
            delete_blobs(c, &blobs)
        })
    }

    /// Open and confirmed findings per severity.
    pub fn finding_counts(&self) -> Result<FindingCounts, StorageError> {
        self.with_reader(|c| {
            let mut counts = FindingCounts::default();
            let mut stmt = c.prepare(
                "SELECT severity, COUNT(*) FROM findings WHERE status IN ('open', 'confirmed') \
                 GROUP BY severity",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            for row in rows {
                let (sev, n) = row?;
                let n = u64::try_from(n).unwrap_or(0);
                match FindingSeverity::parse(&sev) {
                    Some(FindingSeverity::Info) => counts.info = n,
                    Some(FindingSeverity::Low) => counts.low = n,
                    Some(FindingSeverity::Medium) => counts.medium = n,
                    Some(FindingSeverity::High) => counts.high = n,
                    Some(FindingSeverity::Critical) => counts.critical = n,
                    None => {}
                }
            }
            Ok(counts)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpenOptions;
    use crate::model::NewExchange;
    use chosproxy_core::Source;

    fn exchange(path: &str) -> NewExchange {
        NewExchange {
            started_at: 1,
            duration_ms: None,
            source: Source::Proxy,
            listener: None,
            client_addr: None,
            scheme: "https".into(),
            method: "GET".into(),
            host: "lab.test".into(),
            port: 443,
            path: path.into(),
            extension: None,
            url: format!("https://lab.test{path}"),
            http_version: "HTTP/1.1".into(),
            status: Some(200),
            reason: None,
            mime: None,
            request: format!("GET {path} HTTP/1.1\r\n\r\n").into_bytes(),
            response: Some(b"HTTP/1.1 200 OK\r\n\r\nbody".to_vec()),
            original_request: None,
            original_response: None,
            request_len: 10,
            response_len: Some(10),
            server_ip: None,
            tls: true,
            error: None,
            notes: None,
            flags: 0,
            ws_key: None,
        }
    }

    fn new(exchange_id: i64) -> NewFinding {
        NewFinding {
            exchange_id,
            kind: "Information disclosure".into(),
            title: "Debug page reachable".into(),
            severity: FindingSeverity::Medium,
            confidence: Confidence::Firm,
            description: "The page shows internal settings.".into(),
            remediation: "Remove it from production.".into(),
            evidence_note: "first seen here".into(),
        }
    }

    #[test]
    fn create_list_evidence_update_delete() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let a = db.insert(exchange("/debug?x=1")).unwrap();
        let b = db.insert(exchange("/debug/vars")).unwrap();
        let id = db.finding_create(new(a)).unwrap();
        assert!(db.finding_create(new(999)).is_err());
        assert!(
            db.finding_create(NewFinding {
                title: " ".into(),
                ..new(a)
            })
            .is_err()
        );

        db.finding_add_evidence(id, b, "also here").unwrap();
        assert!(
            db.finding_add_evidence(id, b, "dup").is_err(),
            "same request twice"
        );
        assert!(db.finding_add_evidence(77, b, "").is_err());

        let rows = db.findings(&FindingQuery::default()).unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!((row.path.as_str(), row.evidence_count), ("/debug", 2));
        assert_eq!(row.status, FindingStatus::Open);

        let stored = db.finding(id).unwrap().unwrap();
        assert_eq!(stored.evidence.len(), 2);
        assert_eq!(stored.evidence[1].row.note, "also here");
        assert_eq!(
            stored.evidence[0].response.as_deref(),
            Some(&b"HTTP/1.1 200 OK\r\n\r\nbody"[..])
        );

        db.finding_update(
            id,
            FindingUpdate {
                kind: "Information disclosure".into(),
                title: "Debug page reachable".into(),
                severity: FindingSeverity::High,
                confidence: Confidence::Certain,
                status: FindingStatus::FalsePositive,
                description: String::new(),
                remediation: String::new(),
                notes: "only on staging".into(),
            },
        )
        .unwrap();
        let fp = db
            .findings(&FindingQuery {
                statuses: vec![FindingStatus::FalsePositive],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(fp.len(), 1, "false positives are kept as records");
        assert_eq!(db.finding_counts().unwrap(), FindingCounts::default());
        let search = db
            .findings(&FindingQuery {
                search: Some("STAGING".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(search.len(), 1);

        db.finding_remove_evidence(stored.evidence[1].row.id)
            .unwrap();
        assert_eq!(db.finding(id).unwrap().unwrap().evidence.len(), 1);
        db.finding_delete(id).unwrap();
        assert!(db.finding(id).unwrap().is_none());
        assert!(db.exchange(a).unwrap().is_some(), "history untouched");
    }

    #[test]
    fn in_scope_exports_keep_finding_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let a = db.insert(exchange("/")).unwrap();
        db.finding_create(new(a)).unwrap();
        // Empty scope: every exchange is pruned from the copy.
        let copy = dir.path().join("copy");
        db.copy_to(&copy, true).unwrap();
        let copied = ProjectDb::open(&copy, OpenOptions::default()).unwrap();
        let rows = copied.findings(&FindingQuery::default()).unwrap();
        let f = copied.finding(rows[0].id).unwrap().unwrap();
        assert_eq!(
            f.evidence[0].request,
            exchange("/").request,
            "evidence intact"
        );
        assert!(copied.exchange(a).unwrap().is_none());
    }

    #[test]
    fn counts_open_findings_by_severity() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let a = db.insert(exchange("/")).unwrap();
        db.finding_create(new(a)).unwrap();
        db.finding_create(NewFinding {
            severity: FindingSeverity::Low,
            ..new(a)
        })
        .unwrap();
        let counts = db.finding_counts().unwrap();
        assert_eq!((counts.medium, counts.low, counts.high), (1, 1, 0));
    }
}
