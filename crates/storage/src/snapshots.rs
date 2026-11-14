//! Named site map snapshots and their comparison (matrix TARGET-005).

use crate::StorageError;
use crate::db::ProjectDb;
use crate::model::SiteMapEntry;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

/// Snapshots kept per project.
pub const MAX_SNAPSHOTS: i64 = 100;

/// A saved snapshot (without its entries).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SiteSnapshotInfo {
    /// Id.
    #[ts(type = "number")]
    pub id: i64,
    /// Name.
    pub name: String,
    /// Unix epoch ms.
    #[ts(type = "number")]
    pub created_at: i64,
    /// Endpoints in it.
    #[ts(type = "number")]
    pub endpoints: i64,
}

/// How an endpoint differs between two site maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SiteChange {
    /// Only in the newer map.
    Added,
    /// Only in the older map.
    Removed,
    /// In both, with other statuses or methods.
    Changed,
}

/// One difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SiteDiffEntry {
    /// Kind of change.
    pub change: SiteChange,
    /// `scheme://host:port/path`.
    pub url: String,
    /// Statuses before (empty when added).
    pub before_statuses: Vec<u16>,
    /// Statuses after (empty when removed).
    pub after_statuses: Vec<u16>,
    /// Methods before.
    pub before_methods: Vec<String>,
    /// Methods after.
    pub after_methods: Vec<String>,
}

fn key(e: &SiteMapEntry) -> String {
    let default = if e.scheme == "https" { 443 } else { 80 };
    let port = if e.port == default {
        String::new()
    } else {
        format!(":{}", e.port)
    };
    format!("{}://{}{}{}", e.scheme, e.host, port, e.path)
}

/// Differences from `older` to `newer`, by URL.
pub fn diff_site_maps(older: &[SiteMapEntry], newer: &[SiteMapEntry]) -> Vec<SiteDiffEntry> {
    let old: BTreeMap<String, &SiteMapEntry> = older.iter().map(|e| (key(e), e)).collect();
    let new: BTreeMap<String, &SiteMapEntry> = newer.iter().map(|e| (key(e), e)).collect();
    let mut out = Vec::new();
    for (url, n) in &new {
        match old.get(url) {
            None => out.push(SiteDiffEntry {
                change: SiteChange::Added,
                url: url.clone(),
                before_statuses: vec![],
                after_statuses: n.statuses.clone(),
                before_methods: vec![],
                after_methods: n.methods.clone(),
            }),
            Some(o) if o.statuses != n.statuses || o.methods != n.methods => {
                out.push(SiteDiffEntry {
                    change: SiteChange::Changed,
                    url: url.clone(),
                    before_statuses: o.statuses.clone(),
                    after_statuses: n.statuses.clone(),
                    before_methods: o.methods.clone(),
                    after_methods: n.methods.clone(),
                })
            }
            Some(_) => {}
        }
    }
    for (url, o) in &old {
        if !new.contains_key(url) {
            out.push(SiteDiffEntry {
                change: SiteChange::Removed,
                url: url.clone(),
                before_statuses: o.statuses.clone(),
                after_statuses: vec![],
                before_methods: o.methods.clone(),
                after_methods: vec![],
            });
        }
    }
    out.sort_by(|a, b| a.url.cmp(&b.url));
    out
}

impl ProjectDb {
    /// Saves `entries` as a named snapshot.
    pub fn snapshot_save(
        &self,
        name: String,
        entries: Vec<SiteMapEntry>,
    ) -> Result<SiteSnapshotInfo, StorageError> {
        let name: String = name.trim().chars().take(200).collect();
        if name.is_empty() {
            return Err(StorageError::InvalidQuery("name the snapshot".into()));
        }
        let data = serde_json::to_string(&entries)
            .map_err(|e| StorageError::InvalidQuery(e.to_string()))?;
        let endpoints = i64::try_from(entries.len()).unwrap_or(i64::MAX);
        self.write(move |c| {
            let count: i64 = c.query_row("SELECT COUNT(*) FROM site_snapshots", [], |r| r.get(0))?;
            if count >= MAX_SNAPSHOTS {
                return Err(StorageError::InvalidQuery(format!(
                    "a project keeps at most {MAX_SNAPSHOTS} snapshots; delete one first"
                )));
            }
            let now = chosproxy_core::time::now_ms();
            c.execute(
                "INSERT INTO site_snapshots (name, created_at, endpoints, data) VALUES (?1, ?2, ?3, ?4)",
                params![name, now, endpoints, data],
            )?;
            Ok(SiteSnapshotInfo {
                id: c.last_insert_rowid(),
                name,
                created_at: now,
                endpoints,
            })
        })
    }

    /// Snapshots, newest first.
    pub fn snapshots(&self) -> Result<Vec<SiteSnapshotInfo>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, created_at, endpoints FROM site_snapshots ORDER BY id DESC",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(SiteSnapshotInfo {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        created_at: r.get(2)?,
                        endpoints: r.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// The entries of a snapshot.
    pub fn snapshot_entries(&self, id: i64) -> Result<Option<Vec<SiteMapEntry>>, StorageError> {
        self.with_reader(|c| {
            let data: Option<String> = c
                .query_row("SELECT data FROM site_snapshots WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            data.map(|d| {
                serde_json::from_str(&d).map_err(|e| StorageError::InvalidQuery(e.to_string()))
            })
            .transpose()
        })
    }

    /// Deletes a snapshot.
    pub fn snapshot_delete(&self, id: i64) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute("DELETE FROM site_snapshots WHERE id = ?1", [id])?;
            Ok(())
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::OpenOptions;

    fn entry(path: &str, statuses: Vec<u16>) -> SiteMapEntry {
        SiteMapEntry {
            scheme: "https".into(),
            host: "shop.test".into(),
            port: 443,
            path: path.into(),
            count: 1,
            methods: vec!["GET".into()],
            statuses,
            mimes: vec![],
            sources: vec![],
            has_params: false,
            first_seen: 1,
            last_seen: 1,
            last_id: 1,
            in_scope: false,
        }
    }

    #[test]
    fn saves_lists_and_diffs() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p.chosproxy"), OpenOptions::default()).unwrap();
        let before = vec![
            entry("/", vec![200]),
            entry("/old", vec![200]),
            entry("/admin", vec![302]),
        ];
        let saved = db
            .snapshot_save(" before release ".into(), before.clone())
            .unwrap();
        assert_eq!(
            (saved.name.as_str(), saved.endpoints),
            ("before release", 3)
        );
        assert_eq!(db.snapshots().unwrap().len(), 1);
        assert_eq!(db.snapshot_entries(saved.id).unwrap().unwrap(), before);
        assert!(db.snapshot_save("  ".into(), vec![]).is_err());

        let after = vec![
            entry("/", vec![200]),
            entry("/admin", vec![200]),
            entry("/new", vec![200]),
        ];
        let d = diff_site_maps(&before, &after);
        let summary: Vec<(SiteChange, &str)> =
            d.iter().map(|e| (e.change, e.url.as_str())).collect();
        assert_eq!(
            summary,
            vec![
                (SiteChange::Changed, "https://shop.test/admin"),
                (SiteChange::Added, "https://shop.test/new"),
                (SiteChange::Removed, "https://shop.test/old"),
            ]
        );
        db.snapshot_delete(saved.id).unwrap();
        assert!(db.snapshots().unwrap().is_empty());
    }
}
