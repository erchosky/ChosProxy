//! Organizer: exchanges the user keeps, with notes, tags, status and
//! severity. Messages are copied, so items outlive the history.

use crate::db::ProjectDb;
use crate::macros::string_enum;
use crate::model::ExchangeId;
use crate::{StorageError, blob};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Row id of an organizer item.
pub type OrganizerId = i64;

string_enum!(
    /// Where the user is with an item.
    OrganizerStatus {
        /// Not looked at yet.
        Todo => "todo",
        /// Being looked at.
        Investigating => "investigating",
        /// Worth following up.
        Interesting => "interesting",
        /// A confirmed problem.
        Confirmed => "confirmed",
        /// Looked like a problem but is not.
        FalsePositive => "falsePositive",
        /// Finished.
        Done => "done",
    }
);

string_enum!(
    /// Severity assigned by the user.
    Severity {
        /// Not rated.
        None => "none",
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

/// An organizer item without its messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OrganizerItem {
    /// Item id.
    #[ts(type = "number")]
    pub id: OrganizerId,
    /// Created, Unix epoch ms.
    #[ts(type = "number")]
    pub created_at: i64,
    /// Last edited, Unix epoch ms.
    #[ts(type = "number")]
    pub updated_at: i64,
    /// History entry it came from, if any.
    #[ts(type = "number | null")]
    pub exchange_id: Option<ExchangeId>,
    /// Name.
    pub name: String,
    /// Notes.
    pub notes: String,
    /// Tags (lower case).
    pub tags: Vec<String>,
    /// Status.
    pub status: OrganizerStatus,
    /// Severity.
    pub severity: Severity,
    /// Request method.
    pub method: String,
    /// Request URL.
    pub url: String,
    /// Response status.
    pub status_code: Option<u16>,
    /// Collection the item belongs to (empty = none).
    pub collection: String,
}

/// Editable fields of an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OrganizerUpdate {
    /// Name (1–200 characters).
    pub name: String,
    /// Notes (up to 20 000 characters).
    pub notes: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Status.
    pub status: OrganizerStatus,
    /// Severity.
    pub severity: Severity,
    /// Collection (up to 60 characters; empty = none).
    #[serde(default)]
    pub collection: String,
}

/// An item with its stored messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOrganizerItem {
    /// Metadata.
    pub item: OrganizerItem,
    /// Raw request.
    pub request: Vec<u8>,
    /// Raw response.
    pub response: Option<Vec<u8>>,
}

/// An item to add from an import (M31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOrganizerItem {
    /// Name, notes, tags, status, severity, collection.
    pub fields: OrganizerUpdate,
    /// Request method.
    pub method: String,
    /// Request URL.
    pub url: String,
    /// Response status.
    pub status_code: Option<u16>,
    /// Raw request.
    pub request: Vec<u8>,
    /// Raw response.
    pub response: Option<Vec<u8>>,
}

const COLUMNS: &str = "id, created_at, updated_at, exchange_id, name, notes, tags, status, severity, \
    method, url, status_code, collection";

fn map_item(r: &Row<'_>) -> rusqlite::Result<OrganizerItem> {
    let tags: String = r.get(6)?;
    Ok(OrganizerItem {
        id: r.get(0)?,
        created_at: r.get(1)?,
        updated_at: r.get(2)?,
        exchange_id: r.get(3)?,
        name: r.get(4)?,
        notes: r.get(5)?,
        tags: tags
            .split(',')
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect(),
        status: OrganizerStatus::parse(&r.get::<_, String>(7)?).unwrap_or(OrganizerStatus::Todo),
        severity: Severity::parse(&r.get::<_, String>(8)?).unwrap_or(Severity::None),
        method: r.get(9)?,
        url: r.get(10)?,
        status_code: r.get(11)?,
        collection: r.get(12)?,
    })
}

fn normalize_tags(tags: &[String]) -> Result<String, StorageError> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = t.trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        if t.len() > 40 || t.contains(',') {
            return Err(StorageError::InvalidQuery(format!(
                "invalid tag {t:?}: up to 40 characters, no commas"
            )));
        }
        if !out.contains(&t) {
            out.push(t);
        }
    }
    if out.len() > 20 {
        return Err(StorageError::InvalidQuery("at most 20 tags".into()));
    }
    Ok(out.join(","))
}

fn path_of(url: &str) -> &str {
    url.find("://")
        .and_then(|i| url[i + 3..].find('/').map(|j| &url[i + 3 + j..]))
        .unwrap_or(url)
}

/// Most levels of nested collections.
pub const MAX_COLLECTION_DEPTH: usize = 4;

/// A collection name as stored: levels separated by `/` ("Auth/Login"),
/// each trimmed, empty levels dropped (M31b: nested collections).
pub fn normalize_collection(name: &str) -> Result<String, StorageError> {
    let levels: Vec<&str> = name
        .split('/')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if levels.len() > MAX_COLLECTION_DEPTH {
        return Err(StorageError::InvalidQuery(format!(
            "collections nest at most {MAX_COLLECTION_DEPTH} levels"
        )));
    }
    let joined = levels.join("/");
    if joined.chars().count() > 60 {
        return Err(StorageError::InvalidQuery(
            "collection names are limited to 60 characters".into(),
        ));
    }
    Ok(joined)
}

impl ProjectDb {
    /// Copies a history entry into the Organizer and returns the new item id.
    pub fn organizer_add(&self, exchange_id: ExchangeId) -> Result<OrganizerId, StorageError> {
        self.write(move |c| {
            let found = c
                .query_row(
                    "SELECT method, url, status, request_blob, response_blob FROM exchanges WHERE id = ?1",
                    [exchange_id],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<u16>>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, Option<i64>>(4)?,
                        ))
                    },
                )
                .optional()?;
            let Some((method, url, status, req, res)) = found else {
                return Err(StorageError::NotFound(exchange_id));
            };
            let copy = |id: i64| -> Result<i64, StorageError> {
                let data = blob::load(c, id)?.ok_or(StorageError::CorruptBlob(id))?;
                blob::insert(c, &data)
            };
            let req_copy = copy(req)?;
            let res_copy = res.map(copy).transpose()?;
            let now = chosproxy_core::time::now_ms();
            let name: String = format!("{method} {}", path_of(&url)).chars().take(200).collect();
            c.execute(
                "INSERT INTO organizer_items (created_at, updated_at, exchange_id, name, status, \
                 severity, method, url, status_code, request_blob, response_blob) \
                 VALUES (?1, ?1, ?2, ?3, 'todo', 'none', ?4, ?5, ?6, ?7, ?8)",
                params![now, exchange_id, name, method, url, status, req_copy, res_copy],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Items, newest first.
    pub fn organizer_list(&self) -> Result<Vec<OrganizerItem>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM organizer_items ORDER BY id DESC"
            ))?;
            let items = stmt
                .query_map([], map_item)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(items)
        })
    }

    /// One item with its messages.
    pub fn organizer_item(
        &self,
        id: OrganizerId,
    ) -> Result<Option<StoredOrganizerItem>, StorageError> {
        self.with_reader(|c| {
            let found = c
                .query_row(
                    &format!("SELECT {COLUMNS}, request_blob, response_blob FROM organizer_items WHERE id = ?1"),
                    [id],
                    |r| Ok((map_item(r)?, r.get::<_, i64>(13)?, r.get::<_, Option<i64>>(14)?)),
                )
                .optional()?;
            let Some((item, req, res)) = found else {
                return Ok(None);
            };
            let request = blob::load(c, req)?.ok_or(StorageError::CorruptBlob(req))?;
            let response = match res {
                Some(b) => Some(blob::load(c, b)?.ok_or(StorageError::CorruptBlob(b))?),
                None => None,
            };
            Ok(Some(StoredOrganizerItem {
                item,
                request,
                response,
            }))
        })
    }

    /// Adds items brought from elsewhere (an imported collection, M31), all
    /// or none: every item is checked like an edit first, then they are
    /// written in one transaction. They have no history entry.
    pub fn organizer_insert_all(
        &self,
        items: Vec<NewOrganizerItem>,
    ) -> Result<Vec<OrganizerId>, StorageError> {
        let mut checked = Vec::with_capacity(items.len());
        for (n, item) in items.into_iter().enumerate() {
            let bad = |why: &str| StorageError::InvalidQuery(format!("item {}: {why}", n + 1));
            let f = &item.fields;
            let name = f.name.trim().to_string();
            if name.is_empty() || name.chars().count() > 200 {
                return Err(bad("the name must be 1–200 characters"));
            }
            if f.notes.chars().count() > 20_000 {
                return Err(bad("notes are limited to 20 000 characters"));
            }
            let tags = normalize_tags(&f.tags).map_err(|e| bad(&e.to_string()))?;
            let collection = normalize_collection(&f.collection)?;
            if item.method.is_empty()
                || item.method.len() > 32
                || item.url.len() > 16 * 1024
                || item.request.is_empty()
                || item.request.len() > 64 * 1024 * 1024
                || item
                    .response
                    .as_ref()
                    .is_some_and(|r| r.len() > 64 * 1024 * 1024)
            {
                return Err(bad("invalid or too large message"));
            }
            checked.push((name, tags, collection, item));
        }
        self.write(move |c| {
            let now = chosproxy_core::time::now_ms();
            let mut ids = Vec::with_capacity(checked.len());
            for (name, tags, collection, item) in checked {
                let req = blob::insert(c, &item.request)?;
                let res = item
                    .response
                    .as_deref()
                    .map(|r| blob::insert(c, r))
                    .transpose()?;
                c.execute(
                    "INSERT INTO organizer_items (created_at, updated_at, exchange_id, name, \
                     notes, tags, status, severity, method, url, status_code, request_blob, \
                     response_blob, collection) \
                     VALUES (?1, ?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        now,
                        name,
                        item.fields.notes,
                        tags,
                        item.fields.status.as_str(),
                        item.fields.severity.as_str(),
                        item.method,
                        item.url,
                        item.status_code,
                        req,
                        res,
                        collection
                    ],
                )?;
                ids.push(c.last_insert_rowid());
            }
            Ok(ids)
        })
    }

    /// Saves the editable fields of an item.
    pub fn organizer_update(
        &self,
        id: OrganizerId,
        update: OrganizerUpdate,
    ) -> Result<(), StorageError> {
        let name = update.name.trim().to_string();
        if name.is_empty() || name.chars().count() > 200 {
            return Err(StorageError::InvalidQuery(
                "the name must be 1–200 characters".into(),
            ));
        }
        if update.notes.chars().count() > 20_000 {
            return Err(StorageError::InvalidQuery(
                "notes are limited to 20 000 characters".into(),
            ));
        }
        let tags = normalize_tags(&update.tags)?;
        let collection = normalize_collection(&update.collection)?;
        self.write(move |c| {
            let n = c.execute(
                "UPDATE organizer_items SET name = ?2, notes = ?3, tags = ?4, status = ?5, \
                 severity = ?6, updated_at = ?7, collection = ?8 WHERE id = ?1",
                params![
                    id,
                    name,
                    update.notes,
                    tags,
                    update.status.as_str(),
                    update.severity.as_str(),
                    chosproxy_core::time::now_ms(),
                    collection
                ],
            )?;
            if n == 0 {
                return Err(StorageError::NotFound(id));
            }
            Ok(())
        })
    }

    /// Deletes an item and its copied messages.
    pub fn organizer_delete(&self, id: OrganizerId) -> Result<(), StorageError> {
        self.write(move |c| {
            let blobs = c
                .query_row(
                    "SELECT request_blob, response_blob FROM organizer_items WHERE id = ?1",
                    [id],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)),
                )
                .optional()?
                .ok_or(StorageError::NotFound(id))?;
            c.execute("DELETE FROM organizer_items WHERE id = ?1", [id])?;
            c.execute("DELETE FROM blobs WHERE id = ?1", [blobs.0])?;
            if let Some(b) = blobs.1 {
                c.execute("DELETE FROM blobs WHERE id = ?1", [b])?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpenOptions;
    use crate::model::NewExchange;
    use chosproxy_core::Source;

    fn exchange() -> NewExchange {
        NewExchange {
            started_at: 1,
            duration_ms: None,
            source: Source::Proxy,
            listener: None,
            client_addr: None,
            scheme: "http".into(),
            method: "POST".into(),
            host: "lab.test".into(),
            port: 80,
            path: "/login?x=1".into(),
            extension: None,
            url: "http://lab.test/login?x=1".into(),
            http_version: "HTTP/1.1".into(),
            status: Some(302),
            reason: None,
            mime: None,
            request: b"POST /login?x=1 HTTP/1.1\r\n\r\n".to_vec(),
            response: Some(b"HTTP/1.1 302 Found\r\n\r\n".to_vec()),
            original_request: None,
            original_response: None,
            request_len: 10,
            response_len: Some(10),
            server_ip: None,
            tls: false,
            error: None,
            notes: None,
            flags: 0,
            ws_key: None,
        }
    }

    #[test]
    fn add_list_update_delete() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let ex = db.insert(exchange()).unwrap();
        let id = db.organizer_add(ex).unwrap();
        assert!(matches!(
            db.organizer_add(999),
            Err(StorageError::NotFound(999))
        ));

        let items = db.organizer_list().unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.name, "POST /login?x=1");
        assert_eq!(
            (item.status, item.severity),
            (OrganizerStatus::Todo, Severity::None)
        );
        assert_eq!(item.status_code, Some(302));

        db.organizer_update(
            id,
            OrganizerUpdate {
                name: " Login redirect ".into(),
                notes: "check open redirect".into(),
                tags: vec!["Auth".into(), "auth".into(), " redirect ".into(), "".into()],
                status: OrganizerStatus::Interesting,
                severity: Severity::Medium,
                collection: " Login flow ".into(),
            },
        )
        .unwrap();
        let stored = db.organizer_item(id).unwrap().unwrap();
        assert_eq!(stored.item.name, "Login redirect");
        assert_eq!(stored.item.tags, vec!["auth", "redirect"]);
        assert_eq!(stored.item.status, OrganizerStatus::Interesting);
        assert_eq!(stored.item.collection, "Login flow");
        assert_eq!(stored.request, exchange().request);
        assert_eq!(stored.response, exchange().response);

        let bad = |name: &str, tags: Vec<String>| OrganizerUpdate {
            name: name.into(),
            notes: String::new(),
            tags,
            status: OrganizerStatus::Done,
            severity: Severity::None,
            collection: String::new(),
        };
        assert!(db.organizer_update(id, bad("", vec![])).is_err());
        assert!(
            db.organizer_update(id, bad("x", vec!["a,b".into()]))
                .is_err()
        );
        assert!(matches!(
            db.organizer_update(77, bad("x", vec![])),
            Err(StorageError::NotFound(77))
        ));

        db.organizer_delete(id).unwrap();
        assert!(db.organizer_item(id).unwrap().is_none());
        assert!(db.organizer_delete(id).is_err());
        assert!(db.exchange(ex).unwrap().is_some(), "history untouched");
    }

    #[test]
    fn collections_nest_with_slashes() {
        assert_eq!(
            normalize_collection(" Auth / Login ").unwrap(),
            "Auth/Login"
        );
        assert_eq!(normalize_collection("a//b/").unwrap(), "a/b");
        assert_eq!(normalize_collection("  ").unwrap(), "");
        assert!(normalize_collection("a/b/c/d/e").is_err());
        assert!(normalize_collection(&"x".repeat(61)).is_err());
    }
}
