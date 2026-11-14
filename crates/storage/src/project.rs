//! Whole-project operations: consistent copies (Save As, export, backups),
//! validating project files, the project settings layer and the audit log.

use crate::db::{ProjectDb, configure, register_functions};
use crate::key::{ProjectKey, is_encrypted_file};
use crate::{StorageError, migrations};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

/// Longest value stored in one project setting.
pub const MAX_SETTING_BYTES: usize = 1024 * 1024;

/// What a project file contains (checked without opening it for writing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectFileInfo {
    /// Schema version.
    #[ts(type = "number")]
    pub schema: i64,
    /// Stored exchanges.
    #[ts(type = "number")]
    pub exchanges: u64,
    /// File size in bytes.
    #[ts(type = "number")]
    pub size: u64,
    /// Created, Unix epoch ms.
    #[ts(type = "number | null")]
    pub created_at: Option<i64>,
    /// The file is encrypted (M31).
    pub encrypted: bool,
}

/// How a copy of the project is encrypted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyKey {
    /// Like the project (encrypted with its key, or plain).
    Same,
    /// Not encrypted.
    Plain,
    /// Encrypted with this key.
    Key(ProjectKey),
}

/// One audit log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AuditEntry {
    /// Entry id.
    #[ts(type = "number")]
    pub id: i64,
    /// Unix epoch ms.
    #[ts(type = "number")]
    pub at: i64,
    /// Short machine-readable action, e.g. `scope.changed`.
    pub action: String,
    /// Human-readable details.
    pub detail: String,
}

/// Filter and pagination options for querying the project audit log (M31 F, §122).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct AuditQuery {
    /// Optional action prefix / category (e.g. `"project."`, `"plugin."`, `"task."`, `"agent."`).
    pub action_prefix: Option<String>,
    /// Case-insensitive text search across `action` and `detail`.
    pub search: Option<String>,
    /// Start timestamp inclusive (Unix epoch ms).
    #[ts(type = "number | null")]
    pub from_ms: Option<i64>,
    /// End timestamp inclusive (Unix epoch ms).
    #[ts(type = "number | null")]
    pub to_ms: Option<i64>,
    /// Maximum entries returned (clamped to `1..=10_000`; `0` defaults to `500`).
    pub limit: u32,
    /// Pagination offset.
    pub offset: u32,
}

/// Checks that `path` is an intact ChosProxy project this build can open.
/// An encrypted file fails with [`StorageError::Encrypted`].
pub fn inspect_project_file(path: &Path) -> Result<ProjectFileInfo, StorageError> {
    inspect_project_file_with(path, None)
}

/// Like [`inspect_project_file`], with the key of an encrypted file.
pub fn inspect_project_file_with(
    path: &Path,
    key: Option<&ProjectKey>,
) -> Result<ProjectFileInfo, StorageError> {
    let size = std::fs::metadata(path)?.len();
    let encrypted = is_encrypted_file(path)?;
    if encrypted && key.is_none() {
        return Err(StorageError::Encrypted);
    }
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    ProjectKey::unlock(key.filter(|_| encrypted), &c)?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    let app_id: i64 = c.pragma_query_value(None, "application_id", |r| r.get(0))?;
    if app_id != migrations::APPLICATION_ID {
        return Err(StorageError::NotAProject);
    }
    let schema: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if schema > migrations::latest() {
        return Err(StorageError::NewerSchema {
            found: schema,
            supported: migrations::latest(),
        });
    }
    let check: String = c.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(StorageError::InvalidQuery(format!(
            "the project file is damaged: {check}"
        )));
    }
    let exchanges: i64 = c.query_row("SELECT COUNT(*) FROM exchanges", [], |r| r.get(0))?;
    let created_at = c
        .query_row(
            "SELECT value FROM project_meta WHERE key = 'created_at'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|v| v.parse().ok());
    Ok(ProjectFileInfo {
        schema,
        exchanges: u64::try_from(exchanges).unwrap_or(0),
        size,
        created_at,
        encrypted,
    })
}

/// Deletes traffic outside the scope from a copied project, then compacts it.
fn prune_out_of_scope(c: &Connection) -> Result<(), StorageError> {
    c.execute_batch(
        "BEGIN;
         DELETE FROM exchanges WHERE NOT chos_in_scope(scheme, host, port, path);
         DELETE FROM ws_messages WHERE connection_id IN
             (SELECT id FROM ws_connections WHERE NOT chos_in_scope(scheme, host, port, path));
         DELETE FROM ws_connections WHERE NOT chos_in_scope(scheme, host, port, path);
         DELETE FROM site_marks WHERE NOT chos_in_scope(scheme, host, port, path);
         DELETE FROM repeater_sends WHERE tab_id IN
             (SELECT id FROM repeater_tabs WHERE NOT chos_in_scope(scheme, host, port, path))
             OR exchange_id NOT IN (SELECT id FROM exchanges);
         DELETE FROM repeater_tabs WHERE NOT chos_in_scope(scheme, host, port, path);
         DELETE FROM repeater_groups WHERE id NOT IN
             (SELECT group_id FROM repeater_tabs WHERE group_id IS NOT NULL);
         -- Cookies carry no port or scheme to check against the scope, and
         -- are session secrets: an in-scope copy leaves the jar empty.
         DELETE FROM cookie_jar;
         DELETE FROM blobs WHERE id NOT IN (
             SELECT request_blob FROM exchanges
             UNION SELECT response_blob FROM exchanges WHERE response_blob IS NOT NULL
             UNION SELECT original_request_blob FROM exchanges WHERE original_request_blob IS NOT NULL
             UNION SELECT original_response_blob FROM exchanges WHERE original_response_blob IS NOT NULL
             UNION SELECT payload_blob FROM ws_messages
             UNION SELECT original_blob FROM ws_messages WHERE original_blob IS NOT NULL
             UNION SELECT request_blob FROM organizer_items
             UNION SELECT response_blob FROM organizer_items WHERE response_blob IS NOT NULL
             UNION SELECT request_blob FROM finding_evidence
             UNION SELECT response_blob FROM finding_evidence WHERE response_blob IS NOT NULL);
         COMMIT;
         VACUUM;",
    )?;
    Ok(())
}

impl ProjectDb {
    /// Writes a consistent copy of the project to `dest` (which must not
    /// exist). With `in_scope_only`, traffic outside the current scope is left
    /// out. Returns the size of the copy.
    pub fn copy_to(&self, dest: &Path, in_scope_only: bool) -> Result<u64, StorageError> {
        self.copy_with(dest, in_scope_only, CopyKey::Same)
    }

    /// Checks a copy of this project (a backup, an export) with this
    /// project's key.
    pub fn inspect_copy(&self, path: &Path) -> Result<ProjectFileInfo, StorageError> {
        inspect_project_file_with(path, self.current_key().as_ref())
    }

    /// Writes a consistent copy encrypted as `key` says (M31): `Same` keeps
    /// the project's encryption, `Plain` and `Key` re-encode every page
    /// with SQLCipher's `sqlcipher_export`.
    pub fn copy_with(
        &self,
        dest: &Path,
        in_scope_only: bool,
        key: CopyKey,
    ) -> Result<u64, StorageError> {
        if dest.exists() {
            return Err(StorageError::InvalidQuery(format!(
                "{} already exists",
                dest.display()
            )));
        }
        let dest_text = dest.to_str().ok_or_else(|| {
            StorageError::InvalidQuery("the destination path is not UTF-8".into())
        })?;
        self.flush()?;
        let current = self.current_key();
        let target = match &key {
            CopyKey::Same => current.clone(),
            CopyKey::Plain => None,
            CopyKey::Key(k) => Some(k.clone()),
        };
        let result = (|| -> Result<(), StorageError> {
            if key == CopyKey::Same || target == current {
                let source = if self.is_read_only() {
                    // A read-only project can still be saved as a copy.
                    let c =
                        Connection::open_with_flags(self.path(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                    ProjectKey::unlock(current.as_ref(), &c)?;
                    c.busy_timeout(std::time::Duration::from_secs(5))?;
                    c
                } else {
                    let c = Connection::open(self.path())?;
                    ProjectKey::unlock(current.as_ref(), &c)?;
                    configure(&c)?;
                    c
                };
                source.execute("VACUUM INTO ?1", [dest_text])?;
            } else {
                // The export only writes to the attached file.
                let source = Connection::open(self.path())?;
                ProjectKey::unlock(current.as_ref(), &source)?;
                source.busy_timeout(std::time::Duration::from_secs(5))?;
                let secret = target.as_ref().map_or("", |k| k.secret());
                source.execute(
                    "ATTACH DATABASE ?1 AS chos_copy KEY ?2",
                    [dest_text, secret],
                )?;
                let exported = (|| -> Result<(), StorageError> {
                    source.query_row("SELECT sqlcipher_export('chos_copy')", [], |_| Ok(()))?;
                    let version: i64 =
                        source.pragma_query_value(None, "user_version", |r| r.get(0))?;
                    source.execute_batch(&format!(
                        "PRAGMA chos_copy.user_version = {version}; \
                         PRAGMA chos_copy.application_id = {};",
                        migrations::APPLICATION_ID
                    ))?;
                    Ok(())
                })();
                source.execute_batch("DETACH DATABASE chos_copy")?;
                exported?;
            }
            chosproxy_core::fs::restrict_file(dest)?;
            if in_scope_only {
                let copy = Connection::open(dest)?;
                ProjectKey::unlock(target.as_ref(), &copy)?;
                register_functions(&copy, self.shared_scope())?;
                prune_out_of_scope(&copy)?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_file(dest);
            return Err(e);
        }
        Ok(std::fs::metadata(dest)?.len())
    }

    /// A cheap summary of the project's contents; it changes whenever traffic,
    /// annotations, Organizer items, marks or project settings change.
    pub fn fingerprint(&self) -> Result<String, StorageError> {
        self.flush()?;
        self.with_reader(|c| {
            Ok(c.query_row(
                "SELECT (SELECT COUNT(*) FROM exchanges) || ':' || \
                        (SELECT IFNULL(SUM(length(IFNULL(notes, '')) + length(IFNULL(highlight, ''))), 0) FROM exchanges) || ':' || \
                        (SELECT IFNULL(MAX(id), 0) FROM ws_messages) || ':' || \
                        (SELECT COUNT(*) || '/' || IFNULL(MAX(updated_at), 0) FROM organizer_items) || ':' || \
                        (SELECT COUNT(*) || '/' || IFNULL(MAX(updated_at), 0) FROM project_settings) || ':' || \
                        (SELECT COUNT(*) FROM site_marks) || ':' || \
                        (SELECT IFNULL(MAX(id), 0) FROM audit_log)",
                [],
                |r| r.get(0),
            )?)
        })
    }

    /// A project setting (JSON text), if set.
    pub fn setting(&self, key: &str) -> Result<Option<String>, StorageError> {
        let key = key.to_string();
        self.with_reader(move |c| {
            Ok(c.query_row(
                "SELECT value FROM project_settings WHERE key = ?1",
                [&key],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// Sets (`Some`) or removes a project setting.
    pub fn set_setting(&self, key: &str, value: Option<String>) -> Result<(), StorageError> {
        if value.as_ref().is_some_and(|v| v.len() > MAX_SETTING_BYTES) {
            return Err(StorageError::InvalidQuery(
                "the setting is too large".into(),
            ));
        }
        let key = key.to_string();
        self.write(move |c| {
            match value {
                Some(v) => c.execute(
                    "INSERT INTO project_settings (key, value, updated_at) VALUES (?1, ?2, ?3) \
                     ON CONFLICT DO UPDATE SET value = ?2, updated_at = ?3",
                    params![key, v, chosproxy_core::time::now_ms()],
                )?,
                None => c.execute("DELETE FROM project_settings WHERE key = ?1", [key])?,
            };
            Ok(())
        })
    }

    /// Appends an entry to the audit log.
    pub fn audit(&self, action: &str, detail: &str) -> Result<(), StorageError> {
        let (action, detail) = (action.to_string(), detail.to_string());
        self.write(move |c| {
            c.execute(
                "INSERT INTO audit_log (at, action, detail) VALUES (?1, ?2, ?3)",
                params![chosproxy_core::time::now_ms(), action, detail],
            )?;
            Ok(())
        })
    }

    /// The newest audit log entries, newest first.
    pub fn audit_log(&self, limit: u32) -> Result<Vec<AuditEntry>, StorageError> {
        self.audit_log_query(&AuditQuery {
            limit,
            ..AuditQuery::default()
        })
    }

    /// Queries the audit log with filtering and pagination, newest first.
    pub fn audit_log_query(&self, query: &AuditQuery) -> Result<Vec<AuditEntry>, StorageError> {
        let query = query.clone();
        self.with_reader(move |c| {
            let limit = if query.limit == 0 {
                500
            } else {
                query.limit.clamp(1, 10_000)
            };
            let prefix = query
                .action_prefix
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_lowercase());
            let search = query
                .search
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_lowercase());
            let from_ms = query.from_ms;
            let to_ms = query.to_ms;

            let mut stmt = c.prepare(
                "SELECT id, at, action, detail FROM audit_log \
                 WHERE (?1 IS NULL OR at >= ?1) \
                   AND (?2 IS NULL OR at <= ?2) \
                 ORDER BY id DESC",
            )?;
            let iter = stmt.query_map(params![from_ms, to_ms], |r| {
                Ok(AuditEntry {
                    id: r.get(0)?,
                    at: r.get(1)?,
                    action: r.get(2)?,
                    detail: r.get(3)?,
                })
            })?;
            let mut rows = Vec::new();
            let mut skipped = 0u32;
            for r in iter {
                let entry = r?;
                if let Some(p) = &prefix
                    && !entry.action.to_lowercase().starts_with(p)
                {
                    continue;
                }
                if let Some(needle) = &search {
                    let act = entry.action.to_lowercase();
                    let det = entry.detail.to_lowercase();
                    if !act.contains(needle) && !det.contains(needle) {
                        continue;
                    }
                }
                if skipped < query.offset {
                    skipped += 1;
                    continue;
                }
                rows.push(entry);
                if rows.len() >= limit as usize {
                    break;
                }
            }
            Ok(rows)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpenOptions;
    use crate::model::{HistoryQuery, NewExchange};
    use chosproxy_core::{Scope, ScopeConfig, Source, scope::ScopeRule};

    fn exchange(host: &str) -> NewExchange {
        NewExchange {
            started_at: 1,
            duration_ms: None,
            source: Source::Proxy,
            listener: None,
            client_addr: None,
            scheme: "http".into(),
            method: "GET".into(),
            host: host.into(),
            port: 80,
            path: "/".into(),
            extension: None,
            url: format!("http://{host}/"),
            http_version: "HTTP/1.1".into(),
            status: Some(200),
            reason: None,
            mime: None,
            request: format!("GET / HTTP/1.1\r\nHost: {host}\r\n\r\n").into_bytes(),
            response: Some(b"HTTP/1.1 200 OK\r\n\r\n".to_vec()),
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
    fn copies_whole_or_in_scope_and_validates_files() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p.chosproxy"), OpenOptions::default()).unwrap();
        db.submit(exchange("lab.test")).unwrap();
        db.submit(exchange("other.test")).unwrap();
        db.set_setting("notes", Some("\"# Notes\"".into())).unwrap();

        let full = dir.path().join("full.chosproxy");
        assert!(
            db.copy_to(&full, false).unwrap() > 0,
            "pending writes flushed"
        );
        let info = inspect_project_file(&full).unwrap();
        assert_eq!((info.exchanges, info.schema), (2, migrations::latest()));
        assert!(info.created_at.is_some());
        assert!(db.copy_to(&full, false).is_err(), "never overwrites");

        db.set_scope(
            Scope::compile(&ScopeConfig {
                include: vec![ScopeRule {
                    host: "lab.test".into(),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .unwrap(),
        );
        let scoped = dir.path().join("scoped.chosproxy");
        db.copy_to(&scoped, true).unwrap();
        assert_eq!(inspect_project_file(&scoped).unwrap().exchanges, 1);
        let reopened = ProjectDb::open(&scoped, OpenOptions::default()).unwrap();
        let page = reopened
            .history_page(&HistoryQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.rows[0].host, "lab.test");
        assert_eq!(
            reopened.setting("notes").unwrap().as_deref(),
            Some("\"# Notes\"")
        );

        let junk = dir.path().join("junk");
        std::fs::write(&junk, b"not a database").unwrap();
        assert!(inspect_project_file(&junk).is_err());
    }

    #[test]
    fn settings_and_append_only_audit_log() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        assert_eq!(db.setting("scope").unwrap(), None);
        db.set_setting("scope", Some("{}".into())).unwrap();
        db.set_setting("scope", Some("{\"a\":1}".into())).unwrap();
        assert_eq!(db.setting("scope").unwrap().as_deref(), Some("{\"a\":1}"));
        db.set_setting("scope", None).unwrap();
        assert_eq!(db.setting("scope").unwrap(), None);
        assert!(
            db.set_setting("big", Some("x".repeat(MAX_SETTING_BYTES + 1)))
                .is_err()
        );

        let before = db.fingerprint().unwrap();
        db.audit("scope.changed", "1 include rule").unwrap();
        assert_ne!(db.fingerprint().unwrap(), before);
        db.audit("project.exported", "/tmp/x").unwrap();
        let log = db.audit_log(10).unwrap();
        assert_eq!(log[0].action, "project.exported");
        assert_eq!(log.len(), 2);
        let tampered = db.write(|c| {
            c.execute("DELETE FROM audit_log", [])?;
            Ok(())
        });
        assert!(tampered.is_err(), "append-only");
        assert_eq!(db.audit_log(10).unwrap().len(), 2);
    }
}
