//! Suppression rules (M25): "findings of this type on this host (and path)
//! are false positives". Automatic sources (passive analysis, scans,
//! plugins) do not create findings that match a rule; open ones can be
//! marked as false positives when the rule is added.

use crate::StorageError;
use crate::db::ProjectDb;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most rules in one project.
pub const MAX_SUPPRESSIONS: i64 = 500;

/// A suppression rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SuppressionRule {
    /// Row id.
    #[ts(type = "number")]
    pub id: i64,
    /// When it was added (ms since the epoch).
    #[ts(type = "number")]
    pub created_at: i64,
    /// The finding type it applies to (exactly, e.g. "Cookie without HttpOnly").
    pub kind: String,
    /// Host (`app.example`, `*.example` for subdomains, empty for any).
    pub host: String,
    /// Path prefix (`/static/`), empty for any path.
    pub path_prefix: String,
    /// Why these findings are false positives.
    pub reason: String,
}

/// A rule to add.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewSuppression {
    /// The finding type.
    pub kind: String,
    /// Host pattern (empty = any).
    pub host: String,
    /// Path prefix (empty = any).
    pub path_prefix: String,
    /// Why.
    pub reason: String,
}

impl SuppressionRule {
    /// Whether a finding of `kind` on `host` and `path` is suppressed.
    pub fn matches(&self, kind: &str, host: &str, path: &str) -> bool {
        self.kind == kind && host_matches(&self.host, host) && path.starts_with(&self.path_prefix)
    }
}

fn host_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.trim().to_ascii_lowercase();
    let host = host.to_ascii_lowercase();
    if pattern.is_empty() {
        return true;
    }
    match pattern.strip_prefix("*.") {
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => host == pattern,
    }
}

fn map_rule(r: &rusqlite::Row<'_>) -> rusqlite::Result<SuppressionRule> {
    Ok(SuppressionRule {
        id: r.get(0)?,
        created_at: r.get(1)?,
        kind: r.get(2)?,
        host: r.get(3)?,
        path_prefix: r.get(4)?,
        reason: r.get(5)?,
    })
}

fn all_rules(c: &Connection) -> Result<Vec<SuppressionRule>, StorageError> {
    let mut stmt = c.prepare(
        "SELECT id, created_at, kind, host, path_prefix, reason FROM finding_suppressions \
         ORDER BY id",
    )?;
    let rules = stmt
        .query_map([], map_rule)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rules)
}

/// Whether an automatic detection on exchange `exchange_id` is suppressed
/// (used while recording, inside the write transaction).
pub(crate) fn is_suppressed(
    c: &Connection,
    kind: &str,
    exchange_id: i64,
) -> Result<bool, StorageError> {
    let rules: Vec<SuppressionRule> = {
        let mut stmt = c.prepare_cached(
            "SELECT id, created_at, kind, host, path_prefix, reason FROM finding_suppressions \
             WHERE kind = ?1",
        )?;
        stmt.query_map([kind], map_rule)?
            .collect::<Result<Vec<_>, _>>()?
    };
    if rules.is_empty() {
        return Ok(false);
    }
    let place: Option<(String, String)> = c
        .query_row(
            "SELECT host, path FROM exchanges WHERE id = ?1",
            [exchange_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((host, path)) = place else {
        return Ok(false);
    };
    Ok(rules.iter().any(|r| r.matches(kind, &host, &path)))
}

impl ProjectDb {
    /// The suppression rules, oldest first.
    pub fn suppressions(&self) -> Result<Vec<SuppressionRule>, StorageError> {
        self.with_reader(all_rules)
    }

    /// Adds a rule. With `mark_open`, open findings it matches become false
    /// positives (with the reason in their notes). Returns the rule and how
    /// many findings were marked.
    pub fn suppression_add(
        &self,
        rule: NewSuppression,
        mark_open: bool,
    ) -> Result<(SuppressionRule, u32), StorageError> {
        let kind = rule.kind.trim().to_string();
        let host = rule.host.trim().to_ascii_lowercase();
        let path_prefix = rule.path_prefix.trim().to_string();
        let reason = rule.reason.trim().to_string();
        if kind.is_empty() || kind.len() > 300 {
            return Err(StorageError::InvalidQuery(
                "a rule needs the finding type".into(),
            ));
        }
        if host.len() > 255 || path_prefix.len() > 2000 || reason.len() > 2000 {
            return Err(StorageError::InvalidQuery("the rule is too long".into()));
        }
        if host.contains(|c: char| c.is_whitespace() || c == '/') {
            return Err(StorageError::InvalidQuery(
                "the host is a name like app.example or *.example".into(),
            ));
        }
        if !path_prefix.is_empty() && !path_prefix.starts_with('/') {
            return Err(StorageError::InvalidQuery(
                "the path prefix starts with /".into(),
            ));
        }
        self.write(move |c| {
            let count: i64 = c.query_row("SELECT COUNT(*) FROM finding_suppressions", [], |r| {
                r.get(0)
            })?;
            if count >= MAX_SUPPRESSIONS {
                return Err(StorageError::InvalidQuery(format!(
                    "at most {MAX_SUPPRESSIONS} suppression rules"
                )));
            }
            let now = chosproxy_core::time::now_ms();
            c.execute(
                "INSERT INTO finding_suppressions (created_at, kind, host, path_prefix, reason) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![now, kind, host, path_prefix, reason],
            )?;
            let rule = SuppressionRule {
                id: c.last_insert_rowid(),
                created_at: now,
                kind,
                host,
                path_prefix,
                reason,
            };
            let mut marked = 0u32;
            if mark_open {
                let candidates: Vec<(i64, String, String)> = {
                    let mut stmt = c.prepare(
                        "SELECT id, host, path FROM findings WHERE kind = ?1 AND status = 'open'",
                    )?;
                    stmt.query_map([&rule.kind], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                        .collect::<Result<Vec<_>, _>>()?
                };
                let note = if rule.reason.is_empty() {
                    "Marked as a false positive by a suppression rule.".to_string()
                } else {
                    format!(
                        "Marked as a false positive by a suppression rule: {}",
                        rule.reason
                    )
                };
                for (id, host, path) in candidates {
                    if rule.matches(&rule.kind, &host, &path) {
                        c.execute(
                            "UPDATE findings SET status = 'falsePositive', updated_at = ?2, \
                             notes = trim(notes || char(10) || ?3) WHERE id = ?1",
                            params![id, now, note],
                        )?;
                        marked += 1;
                    }
                }
            }
            Ok((rule, marked))
        })
    }

    /// Removes a rule (findings already marked keep their status).
    pub fn suppression_delete(&self, id: i64) -> Result<(), StorageError> {
        self.write(move |c| {
            let n = c.execute("DELETE FROM finding_suppressions WHERE id = ?1", [id])?;
            if n == 0 {
                return Err(StorageError::InvalidQuery(format!(
                    "no suppression rule {id}"
                )));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(host: &str, path: &str) -> SuppressionRule {
        SuppressionRule {
            id: 1,
            created_at: 0,
            kind: "Cookie without HttpOnly".into(),
            host: host.into(),
            path_prefix: path.into(),
            reason: String::new(),
        }
    }

    #[test]
    fn matches_type_host_and_path() {
        let k = "Cookie without HttpOnly";
        assert!(rule("", "").matches(k, "a.test", "/x"));
        assert!(!rule("", "").matches("Other", "a.test", "/x"));
        assert!(rule("A.test", "").matches(k, "a.test", "/"));
        assert!(!rule("a.test", "").matches(k, "b.a.test", "/"));
        assert!(rule("*.a.test", "").matches(k, "b.a.test", "/"));
        assert!(rule("*.a.test", "").matches(k, "a.test", "/"));
        assert!(!rule("*.a.test", "").matches(k, "ba.test", "/"));
        assert!(rule("", "/static/").matches(k, "x", "/static/a.js?v=1"));
        assert!(!rule("", "/static/").matches(k, "x", "/api"));
    }
}
