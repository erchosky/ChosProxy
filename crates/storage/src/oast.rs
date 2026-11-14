//! ChosOAST data in the project (ADR-0026): payloads, the interactions
//! received for them and the stored requests that carried them.

use crate::StorageError;
use crate::blob;
use crate::db::ProjectDb;
use crate::model::ExchangeId;
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

/// Payloads kept per project.
pub const MAX_OAST_PAYLOADS: i64 = 10_000;

/// A payload created in this project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OastPayloadRow {
    /// Row id.
    #[ts(type = "number")]
    pub id: i64,
    /// The correlation identifier.
    pub correlation_id: String,
    /// Unix epoch ms.
    #[ts(type = "number")]
    pub created_at: i64,
    /// What the tester noted about it.
    pub note: String,
    /// `<id>.<zone>` when DNS is available.
    pub dns_name: Option<String>,
    /// An HTTP URL that carries the identifier.
    pub url: String,
    /// Interactions received.
    #[ts(type = "number")]
    pub interactions: i64,
    /// Stored requests that carried it.
    #[ts(type = "number")]
    pub requests: i64,
    /// Newest interaction, Unix epoch ms.
    #[ts(type = "number | null")]
    pub last_interaction: Option<i64>,
}

/// An interaction to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOastInteraction {
    /// The identifier it carried.
    pub correlation_id: String,
    /// Unix epoch ms.
    pub received_at: i64,
    /// `dns`, `http` or `https`.
    pub protocol: String,
    /// Who made it.
    pub remote: String,
    /// DNS question or HTTP request line.
    pub summary: String,
    /// Raw packet or request.
    pub raw: Vec<u8>,
    /// Unique key on its server.
    pub server_key: String,
}

/// A stored interaction (without its raw data).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OastInteractionRow {
    /// Row id.
    #[ts(type = "number")]
    pub id: i64,
    /// The identifier.
    pub correlation_id: String,
    /// Unix epoch ms.
    #[ts(type = "number")]
    pub received_at: i64,
    /// `dns`, `http` or `https`.
    pub protocol: String,
    /// Who made it.
    pub remote: String,
    /// DNS question or HTTP request line.
    pub summary: String,
    /// The payload's note.
    pub note: String,
}

/// One interaction with its raw data and the requests that carried the
/// identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOastInteraction {
    /// The row.
    pub row: OastInteractionRow,
    /// Raw packet or request.
    pub raw: Vec<u8>,
    /// Stored requests that carried the identifier, newest first.
    pub exchanges: Vec<ExchangeId>,
}

const INTERACTION_COLUMNS: &str = "i.id, i.correlation_id, i.received_at, i.protocol, i.remote, \
    i.summary, COALESCE(p.note, '')";

fn map_interaction(r: &Row<'_>) -> rusqlite::Result<OastInteractionRow> {
    Ok(OastInteractionRow {
        id: r.get(0)?,
        correlation_id: r.get(1)?,
        received_at: r.get(2)?,
        protocol: r.get(3)?,
        remote: r.get(4)?,
        summary: r.get(5)?,
        note: r.get(6)?,
    })
}

impl ProjectDb {
    /// Stores a new payload.
    pub fn oast_create_payload(
        &self,
        correlation_id: String,
        note: String,
        dns_name: Option<String>,
        url: String,
    ) -> Result<OastPayloadRow, StorageError> {
        if note.len() > 500 {
            return Err(StorageError::InvalidQuery("the note is too long".into()));
        }
        self.write(move |c| {
            let count: i64 = c.query_row("SELECT COUNT(*) FROM oast_payloads", [], |r| r.get(0))?;
            if count >= MAX_OAST_PAYLOADS {
                return Err(StorageError::InvalidQuery(format!(
                    "a project keeps at most {MAX_OAST_PAYLOADS} OAST payloads"
                )));
            }
            let now = chosproxy_core::time::now_ms();
            c.execute(
                "INSERT INTO oast_payloads (correlation_id, created_at, note, dns_name, url) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![correlation_id, now, note, dns_name, url],
            )?;
            Ok(OastPayloadRow {
                id: c.last_insert_rowid(),
                correlation_id,
                created_at: now,
                note,
                dns_name,
                url,
                interactions: 0,
                requests: 0,
                last_interaction: None,
            })
        })
    }

    /// Every payload, newest first, with counts.
    pub fn oast_payloads(&self) -> Result<Vec<OastPayloadRow>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(
                "SELECT p.id, p.correlation_id, p.created_at, p.note, p.dns_name, p.url, \
                 (SELECT COUNT(*) FROM oast_interactions i WHERE i.correlation_id = p.correlation_id), \
                 (SELECT COUNT(*) FROM oast_links l WHERE l.correlation_id = p.correlation_id), \
                 (SELECT MAX(received_at) FROM oast_interactions i WHERE i.correlation_id = p.correlation_id) \
                 FROM oast_payloads p ORDER BY p.id DESC",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(OastPayloadRow {
                        id: r.get(0)?,
                        correlation_id: r.get(1)?,
                        created_at: r.get(2)?,
                        note: r.get(3)?,
                        dns_name: r.get(4)?,
                        url: r.get(5)?,
                        interactions: r.get(6)?,
                        requests: r.get(7)?,
                        last_interaction: r.get(8)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// The identifiers of this project's payloads.
    pub fn oast_known_ids(&self) -> Result<HashSet<String>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare("SELECT correlation_id FROM oast_payloads")?;
            let ids = stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<HashSet<String>, _>>()?;
            Ok(ids)
        })
    }

    /// Stores interactions whose identifier belongs to a payload of this
    /// project (others are ignored); repeated server keys are skipped.
    /// Returns the identifiers that got new interactions.
    pub fn oast_record_interactions(
        &self,
        interactions: Vec<NewOastInteraction>,
    ) -> Result<Vec<String>, StorageError> {
        self.write(move |c| {
            let mut touched: Vec<String> = Vec::new();
            for i in &interactions {
                let known: bool = c
                    .query_row(
                        "SELECT 1 FROM oast_payloads WHERE correlation_id = ?1",
                        [&i.correlation_id],
                        |_| Ok(true),
                    )
                    .optional()?
                    .unwrap_or(false);
                let duplicate: bool = c
                    .query_row(
                        "SELECT 1 FROM oast_interactions WHERE server_key = ?1 AND correlation_id = ?2",
                        params![i.server_key, i.correlation_id],
                        |_| Ok(true),
                    )
                    .optional()?
                    .unwrap_or(false);
                if !known || duplicate {
                    continue;
                }
                let raw = blob::insert(c, &i.raw)?;
                c.execute(
                    "INSERT INTO oast_interactions (correlation_id, received_at, protocol, remote, \
                     summary, raw_blob, server_key) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        i.correlation_id,
                        i.received_at,
                        i.protocol,
                        i.remote,
                        i.summary,
                        raw,
                        i.server_key
                    ],
                )?;
                if !touched.contains(&i.correlation_id) {
                    touched.push(i.correlation_id.clone());
                }
            }
            Ok(touched)
        })
    }

    /// Interactions, newest first, optionally of one identifier.
    pub fn oast_interactions(
        &self,
        correlation_id: Option<String>,
        limit: u32,
    ) -> Result<Vec<OastInteractionRow>, StorageError> {
        let limit = limit.clamp(1, 5000);
        self.with_reader(|c| {
            let sql = format!(
                "SELECT {INTERACTION_COLUMNS} FROM oast_interactions i \
                 LEFT JOIN oast_payloads p ON p.correlation_id = i.correlation_id \
                 WHERE (?1 IS NULL OR i.correlation_id = ?1) ORDER BY i.id DESC LIMIT ?2"
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map(params![correlation_id, limit], map_interaction)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// One interaction with its raw data and linked requests.
    pub fn oast_interaction(&self, id: i64) -> Result<Option<StoredOastInteraction>, StorageError> {
        self.with_reader(|c| {
            let found = c
                .query_row(
                    &format!(
                        "SELECT {INTERACTION_COLUMNS}, i.raw_blob FROM oast_interactions i \
                         LEFT JOIN oast_payloads p ON p.correlation_id = i.correlation_id \
                         WHERE i.id = ?1"
                    ),
                    [id],
                    |r| Ok((map_interaction(r)?, r.get::<_, i64>(7)?)),
                )
                .optional()?;
            let Some((row, raw_blob)) = found else {
                return Ok(None);
            };
            let raw = blob::load(c, raw_blob)?.ok_or(StorageError::CorruptBlob(raw_blob))?;
            let mut stmt = c.prepare(
                "SELECT exchange_id FROM oast_links WHERE correlation_id = ?1 \
                 ORDER BY exchange_id DESC LIMIT 100",
            )?;
            let exchanges = stmt
                .query_map([&row.correlation_id], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Some(StoredOastInteraction {
                row,
                raw,
                exchanges,
            }))
        })
    }

    /// Requests that carried `correlation_id`, newest first.
    pub fn oast_links_for(&self, correlation_id: &str) -> Result<Vec<ExchangeId>, StorageError> {
        let id = correlation_id.to_string();
        self.with_reader(move |c| {
            let mut stmt = c.prepare(
                "SELECT exchange_id FROM oast_links WHERE correlation_id = ?1 ORDER BY exchange_id DESC",
            )?;
            let ids = stmt
                .query_map([&id], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ids)
        })
    }

    /// The linker's position in the history.
    pub fn oast_link_cursor(&self) -> Result<ExchangeId, StorageError> {
        self.with_reader(|c| {
            Ok(
                c.query_row("SELECT link_cursor FROM oast_state WHERE id = 1", [], |r| {
                    r.get(0)
                })?,
            )
        })
    }

    /// Records request → identifier links and moves the linker to `cursor`.
    /// Returns the identifiers that got new links.
    pub fn oast_record_links(
        &self,
        links: Vec<(String, ExchangeId)>,
        cursor: ExchangeId,
    ) -> Result<Vec<String>, StorageError> {
        self.write(move |c| {
            let mut touched = Vec::new();
            for (id, exchange) in &links {
                let added = c.execute(
                    "INSERT OR IGNORE INTO oast_links (correlation_id, exchange_id) VALUES (?1, ?2)",
                    params![id, exchange],
                )?;
                if added > 0 && !touched.contains(id) {
                    touched.push(id.clone());
                }
            }
            c.execute(
                "UPDATE oast_state SET link_cursor = MAX(link_cursor, ?1) WHERE id = 1",
                [cursor],
            )?;
            Ok(touched)
        })
    }

    /// The last sequence number polled from `server` (0 for another server).
    pub fn oast_remote_seq(&self, server: &str) -> Result<u64, StorageError> {
        let server = server.to_string();
        self.with_reader(move |c| {
            let (stored, seq): (String, i64) = c.query_row(
                "SELECT remote_server, remote_seq FROM oast_state WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok(if stored == server {
                u64::try_from(seq).unwrap_or(0)
            } else {
                0
            })
        })
    }

    /// Remembers the last sequence number polled from `server`.
    pub fn oast_set_remote_seq(&self, server: String, seq: u64) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute(
                "UPDATE oast_state SET remote_server = ?1, remote_seq = ?2 WHERE id = 1",
                params![server, i64::try_from(seq).unwrap_or(i64::MAX)],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::OpenOptions;

    fn interaction(id: &str, key: &str) -> NewOastInteraction {
        NewOastInteraction {
            correlation_id: id.into(),
            received_at: 5,
            protocol: "dns".into(),
            remote: "10.0.0.9:5300".into(),
            summary: format!("A {id}.oast.test"),
            raw: b"packet".to_vec(),
            server_key: key.into(),
        }
    }

    #[test]
    fn payloads_interactions_and_links() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p.chosproxy"), OpenOptions::default()).unwrap();
        let p = db
            .oast_create_payload(
                "aaa".into(),
                "login form".into(),
                Some("aaa.oast.test".into()),
                "http://x/aaa".into(),
            )
            .unwrap();
        assert_eq!(p.correlation_id, "aaa");
        assert_eq!(db.oast_known_ids().unwrap().len(), 1);

        // Unknown identifiers and repeated keys are not stored.
        let touched = db
            .oast_record_interactions(vec![
                interaction("aaa", "k1"),
                interaction("aaa", "k1"),
                interaction("zzz", "k2"),
            ])
            .unwrap();
        assert_eq!(touched, vec!["aaa".to_string()]);
        let rows = db.oast_interactions(None, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].note, "login form");

        assert_eq!(
            db.oast_record_links(vec![("aaa".into(), 7), ("aaa".into(), 7)], 9)
                .unwrap(),
            vec!["aaa".to_string()]
        );
        assert!(
            db.oast_record_links(vec![("aaa".into(), 7)], 3)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            db.oast_link_cursor().unwrap(),
            9,
            "the cursor never moves back"
        );
        let detail = db.oast_interaction(rows[0].id).unwrap().unwrap();
        assert_eq!(
            (detail.raw.as_slice(), detail.exchanges.as_slice()),
            (&b"packet"[..], &[7][..])
        );

        let listed = db.oast_payloads().unwrap();
        assert_eq!((listed[0].interactions, listed[0].requests), (1, 1));

        db.oast_set_remote_seq("srv:8443".into(), 42).unwrap();
        assert_eq!(db.oast_remote_seq("srv:8443").unwrap(), 42);
        assert_eq!(db.oast_remote_seq("other:8443").unwrap(), 0);
    }
}
