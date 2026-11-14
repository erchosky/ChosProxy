//! Sharing Organizer collections (M31, ORGANIZER-003, ADR-0041): a
//! collection (or the whole Organizer) is written as a JSON document, sealed
//! with a password unless the tester chooses not to; importing adds the
//! items as new Organizer items. Imported content is stored only, never
//! run.

use super::Engine;
use crate::AppError;
use crate::projects::{seal_if, unseal};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chosproxy_storage::{NewOrganizerItem, OrganizerStatus, OrganizerUpdate, Severity};
use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

/// Format name of an Organizer export.
const FORMAT: &str = "chosproxy-organizer";
/// Most items in one export or import.
const MAX_ITEMS: usize = 5_000;
/// Largest file read when importing.
const MAX_FILE: u64 = 512 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedItem {
    name: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    tags: Vec<String>,
    status: OrganizerStatus,
    severity: Severity,
    #[serde(default)]
    collection: String,
    method: String,
    url: String,
    status_code: Option<u16>,
    request: String,
    response: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedDocument {
    format: String,
    version: u32,
    #[serde(default)]
    exported_at: i64,
    items: Vec<SharedItem>,
}

/// What an Organizer export or import did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OrganizerShare {
    /// Items written or added.
    #[ts(type = "number")]
    pub items: u64,
    /// The file is encrypted.
    pub encrypted: bool,
}

impl Engine {
    /// Returns true when `path` begins with the ChosProxy sealed-file magic (`CHOSSEAL`).
    pub fn is_sealed_file(&self, path: &Path) -> Result<bool, AppError> {
        use std::io::Read;
        let mut f = std::fs::File::open(path)?;
        let mut head = [0u8; 8];
        let n = f.read(&mut head)?;
        Ok(chosproxy_storage::sealed::is_sealed(&head[..n]))
    }

    /// Writes the Organizer items of `collection` (including nested child
    /// collections `collection/...`; all items when `None` or empty) to
    /// `path`, sealed with `password` when given. The file is written
    /// atomically with `0600` permissions.
    pub async fn organizer_export(
        &self,
        path: &Path,
        collection: Option<String>,
        password: Option<String>,
    ) -> Result<OrganizerShare, AppError> {
        let db = self.db()?;
        let wanted = match collection {
            Some(c) => {
                let n = chosproxy_storage::normalize_collection(&c)?;
                if n.is_empty() { None } else { Some(n) }
            }
            None => None,
        };
        let doc = tokio::task::spawn_blocking(move || -> Result<SharedDocument, AppError> {
            let mut items = Vec::new();
            for item in db.organizer_list()? {
                if let Some(w) = &wanted {
                    let prefix = format!("{w}/");
                    if &item.collection != w && !item.collection.starts_with(&prefix) {
                        continue;
                    }
                }
                if items.len() >= MAX_ITEMS {
                    return Err(AppError::Invalid(format!(
                        "at most {MAX_ITEMS} items can be exported at once"
                    )));
                }
                let Some(stored) = db.organizer_item(item.id)? else {
                    continue;
                };
                let i = stored.item;
                items.push(SharedItem {
                    name: i.name,
                    notes: i.notes,
                    tags: i.tags,
                    status: i.status,
                    severity: i.severity,
                    collection: i.collection,
                    method: i.method,
                    url: i.url,
                    status_code: i.status_code,
                    request: STANDARD.encode(&stored.request),
                    response: stored.response.as_deref().map(|r| STANDARD.encode(r)),
                });
            }
            Ok(SharedDocument {
                format: FORMAT.into(),
                version: 1,
                exported_at: chosproxy_core::time::now_ms(),
                items,
            })
        })
        .await??;
        if doc.items.is_empty() {
            return Err(AppError::Invalid("there are no items to export".into()));
        }
        let count = doc.items.len() as u64;
        let json = serde_json::to_vec(&doc).map_err(|e| AppError::Internal(e.to_string()))?;
        let pw = password.filter(|p| !p.is_empty());
        let encrypted = pw.is_some();
        let dest = path.to_path_buf();
        tokio::task::spawn_blocking(move || -> Result<(), AppError> {
            let data = seal_if(json, pw.as_deref())?;
            chosproxy_core::fs::write_private_atomic(&dest, &data)?;
            Ok(())
        })
        .await??;
        self.audit(
            "organizer.exported",
            &format!(
                "{count} item(s) to {}{}",
                path.display(),
                if encrypted { " (encrypted)" } else { "" }
            ),
        );
        Ok(OrganizerShare {
            items: count,
            encrypted,
        })
    }

    /// Adds the items of an Organizer export as new items. An encrypted
    /// file needs its password; nothing is added unless every item is valid.
    pub async fn organizer_import(
        &self,
        path: &Path,
        password: Option<String>,
    ) -> Result<OrganizerShare, AppError> {
        self.ensure_writable()?;
        let db = self.db()?;
        let src = path.to_path_buf();
        let (items, encrypted) =
            tokio::task::spawn_blocking(move || -> Result<(Vec<SharedItem>, bool), AppError> {
                if std::fs::metadata(&src)?.len() > MAX_FILE {
                    return Err(AppError::Invalid("the file is too large".into()));
                }
                let raw = std::fs::read(&src)?;
                let encrypted = chosproxy_storage::sealed::is_sealed(&raw);
                let data = unseal(raw, password.as_deref())?;
                let doc: SharedDocument = serde_json::from_slice(&data).map_err(|e| {
                    AppError::Invalid(format!("not a ChosProxy Organizer export: {e}"))
                })?;
                if doc.format != FORMAT {
                    return Err(AppError::Invalid(format!(
                        "not a ChosProxy Organizer export (expected format \"{FORMAT}\")"
                    )));
                }
                if doc.version != 1 {
                    return Err(AppError::Invalid(
                        "this Organizer export uses a newer format; upgrade ChosProxy".into(),
                    ));
                }
                if doc.items.len() > MAX_ITEMS {
                    return Err(AppError::Invalid(format!(
                        "at most {MAX_ITEMS} items can be imported at once"
                    )));
                }
                Ok((doc.items, encrypted))
            })
            .await??;
        // Decode everything first; storage then checks and adds all items
        // in one transaction, or none.
        let mut decoded = Vec::with_capacity(items.len());
        for (n, i) in items.into_iter().enumerate() {
            let bad = |what: &str| AppError::Invalid(format!("item {}: invalid {what}", n + 1));
            let request = STANDARD.decode(&i.request).map_err(|_| bad("request"))?;
            let response = i
                .response
                .as_deref()
                .map(|r| STANDARD.decode(r))
                .transpose()
                .map_err(|_| bad("response"))?;
            decoded.push(NewOrganizerItem {
                fields: OrganizerUpdate {
                    name: i.name,
                    notes: i.notes,
                    tags: i.tags,
                    status: i.status,
                    severity: i.severity,
                    collection: i.collection,
                },
                method: i.method,
                url: i.url,
                status_code: i.status_code,
                request,
                response,
            });
        }
        let count = decoded.len() as u64;
        tokio::task::spawn_blocking(move || db.organizer_insert_all(decoded)).await??;
        self.audit(
            "organizer.imported",
            &format!(
                "{count} item(s) from {}{}",
                path.display(),
                if encrypted { " (encrypted)" } else { "" }
            ),
        );
        Ok(OrganizerShare {
            items: count,
            encrypted,
        })
    }
}
