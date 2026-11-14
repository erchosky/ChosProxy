//! Imported API definitions (M27, matrix SCANNER-014). The endpoints are
//! stored as JSON written and read by the engine.

use crate::StorageError;
use crate::db::ProjectDb;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Definitions kept per project.
pub const MAX_API_DEFINITIONS: i64 = 100;
/// Largest endpoint list stored (JSON).
const MAX_ENDPOINTS_JSON: usize = 64 * 1024 * 1024;

/// A stored definition, without its endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiDefinitionInfo {
    /// Id.
    #[ts(type = "number")]
    pub id: i64,
    /// Unix epoch ms.
    #[ts(type = "number")]
    pub created_at: i64,
    /// File name it was read from.
    pub name: String,
    /// `openApi3`, `swagger2`, `postman` or `wsdl`.
    pub format: String,
    /// Title in the definition.
    pub title: String,
    /// API version in the definition.
    pub version: String,
    /// Base URL given at import (empty if none).
    pub base_url: String,
    /// Number of endpoints.
    #[ts(type = "number")]
    pub endpoint_count: i64,
    /// Problems met while reading it.
    pub warnings: Vec<String>,
}

/// A definition to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewApiDefinition {
    /// File name.
    pub name: String,
    /// Format tag.
    pub format: String,
    /// Title.
    pub title: String,
    /// API version.
    pub version: String,
    /// Base URL given at import.
    pub base_url: String,
    /// Number of endpoints.
    pub endpoint_count: i64,
    /// Warnings.
    pub warnings: Vec<String>,
    /// Endpoints as JSON.
    pub endpoints_json: String,
    /// A GraphQL schema as JSON (empty for other formats).
    pub schema_json: String,
}

const COLUMNS: &str =
    "id, created_at, name, format, title, version, base_url, endpoint_count, warnings";

fn map_info(r: &rusqlite::Row<'_>) -> rusqlite::Result<ApiDefinitionInfo> {
    let warnings: String = r.get(8)?;
    Ok(ApiDefinitionInfo {
        id: r.get(0)?,
        created_at: r.get(1)?,
        name: r.get(2)?,
        format: r.get(3)?,
        title: r.get(4)?,
        version: r.get(5)?,
        base_url: r.get(6)?,
        endpoint_count: r.get(7)?,
        warnings: serde_json::from_str(&warnings).unwrap_or_default(),
    })
}

impl ProjectDb {
    /// Stores a definition.
    pub fn api_definition_add(
        &self,
        new: NewApiDefinition,
    ) -> Result<ApiDefinitionInfo, StorageError> {
        if new.endpoints_json.len() > MAX_ENDPOINTS_JSON
            || new.schema_json.len() > MAX_ENDPOINTS_JSON
        {
            return Err(StorageError::InvalidQuery(
                "the definition is too large to keep".into(),
            ));
        }
        let name: String = new.name.chars().take(300).collect();
        let warnings = serde_json::to_string(&new.warnings.iter().take(200).collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".into());
        self.write(move |c| {
            let count: i64 =
                c.query_row("SELECT COUNT(*) FROM api_definitions", [], |r| r.get(0))?;
            if count >= MAX_API_DEFINITIONS {
                return Err(StorageError::InvalidQuery(format!(
                    "at most {MAX_API_DEFINITIONS} API definitions per project"
                )));
            }
            let now = chosproxy_core::time::now_ms();
            c.execute(
                "INSERT INTO api_definitions (created_at, name, format, title, version, base_url, \
                 endpoint_count, warnings, endpoints, schema) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    now,
                    name,
                    new.format,
                    new.title.chars().take(300).collect::<String>(),
                    new.version.chars().take(100).collect::<String>(),
                    new.base_url,
                    new.endpoint_count,
                    warnings,
                    new.endpoints_json,
                    new.schema_json
                ],
            )?;
            let id = c.last_insert_rowid();
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM api_definitions WHERE id = ?1"),
                [id],
                map_info,
            )?)
        })
    }

    /// The stored definitions, newest first.
    pub fn api_definitions(&self) -> Result<Vec<ApiDefinitionInfo>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM api_definitions ORDER BY id DESC"
            ))?;
            let rows = stmt
                .query_map([], map_info)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// One definition and its endpoints JSON.
    pub fn api_definition(
        &self,
        id: i64,
    ) -> Result<Option<(ApiDefinitionInfo, String)>, StorageError> {
        self.with_reader(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS}, endpoints FROM api_definitions WHERE id = ?1"),
                [id],
                |r| Ok((map_info(r)?, r.get(9)?)),
            )
            .optional()?)
        })
    }

    /// The GraphQL schema JSON of a definition (empty for other formats).
    pub fn api_definition_schema(&self, id: i64) -> Result<Option<String>, StorageError> {
        self.with_reader(|c| {
            Ok(c.query_row(
                "SELECT schema FROM api_definitions WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// Deletes a definition.
    pub fn api_definition_delete(&self, id: i64) -> Result<(), StorageError> {
        self.write(move |c| {
            if c.execute("DELETE FROM api_definitions WHERE id = ?1", [id])? == 0 {
                return Err(StorageError::InvalidQuery(format!(
                    "no API definition {id}"
                )));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::OpenOptions;

    #[test]
    fn stores_lists_and_deletes_definitions() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let info = db
            .api_definition_add(NewApiDefinition {
                name: "pets.yaml".into(),
                format: "openApi3".into(),
                title: "Pets".into(),
                version: "1".into(),
                base_url: String::new(),
                endpoint_count: 2,
                warnings: vec!["w".into()],
                endpoints_json: "[1,2]".into(),
                schema_json: "{}".into(),
            })
            .unwrap();
        assert_eq!(info.name, "pets.yaml");
        assert_eq!(info.warnings, vec!["w".to_string()]);
        assert_eq!(db.api_definitions().unwrap().len(), 1);
        let (got, json) = db.api_definition(info.id).unwrap().unwrap();
        assert_eq!((got.endpoint_count, json.as_str()), (2, "[1,2]"));
        assert_eq!(
            db.api_definition_schema(info.id).unwrap().as_deref(),
            Some("{}")
        );
        db.api_definition_delete(info.id).unwrap();
        assert!(db.api_definition(info.id).unwrap().is_none());
        assert!(db.api_definition_delete(info.id).is_err());
    }
}
