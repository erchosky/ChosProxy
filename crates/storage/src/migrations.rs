//! Versioned schema migrations tracked with `PRAGMA user_version`.

use crate::StorageError;
use rusqlite::Connection;

/// SQLite `application_id` marking ChosProxy project files ("CHPX").
pub const APPLICATION_ID: i64 = 0x4348_5058;

/// Migrations in order; index + 1 is the schema version they produce.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_intercept_edits.sql"),
    include_str!("../migrations/0003_target.sql"),
    include_str!("../migrations/0004_organizer.sql"),
    include_str!("../migrations/0005_websockets.sql"),
    include_str!("../migrations/0006_project.sql"),
    include_str!("../migrations/0007_findings.sql"),
    include_str!("../migrations/0008_repeater.sql"),
    include_str!("../migrations/0009_repeater_groups_cookies.sql"),
    include_str!("../migrations/0010_passive.sql"),
    include_str!("../migrations/0011_tasks.sql"),
    include_str!("../migrations/0012_fuzz_results.sql"),
    include_str!("../migrations/0013_sequencer_tokens.sql"),
    include_str!("../migrations/0014_oast.sql"),
    include_str!("../migrations/0015_dom_events.sql"),
    include_str!("../migrations/0016_discovery.sql"),
    include_str!("../migrations/0017_site_snapshots.sql"),
    include_str!("../migrations/0018_schedules.sql"),
    include_str!("../migrations/0019_repeater_notes.sql"),
    include_str!("../migrations/0020_repeater_redirects.sql"),
    include_str!("../migrations/0021_finding_suppressions.sql"),
    include_str!("../migrations/0022_task_summaries.sql"),
    include_str!("../migrations/0023_api_definitions.sql"),
    include_str!("../migrations/0024_api_definition_schema.sql"),
    include_str!("../migrations/0025_ai_activity.sql"),
    include_str!("../migrations/0026_repeater_tab_color.sql"),
    include_str!("../migrations/0027_sort_indexes.sql"),
    include_str!("../migrations/0028_repeater_group_collapsed.sql"),
    include_str!("../migrations/0029_fuzz_result_color.sql"),
];

/// Latest schema version.
pub fn latest() -> i64 {
    MIGRATIONS.len() as i64
}

/// Brings the database up to the latest schema. Refuses foreign databases and
/// databases from newer versions.
pub fn migrate(conn: &mut Connection) -> Result<i64, StorageError> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let app_id: i64 = conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let has_tables: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table')",
        [],
        |r| r.get(0),
    )?;
    if has_tables && app_id != APPLICATION_ID {
        return Err(StorageError::NotAProject);
    }
    if version > latest() {
        return Err(StorageError::NewerSchema {
            found: version,
            supported: latest(),
        });
    }
    for (i, sql) in MIGRATIONS
        .iter()
        .enumerate()
        .skip(usize::try_from(version).unwrap_or(0))
    {
        let target = i as i64 + 1;
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        tx.pragma_update(None, "user_version", target)?;
        tx.commit()?;
        tracing::info!(version = target, "applied project schema migration");
    }
    Ok(latest())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_fresh_db_and_is_idempotent() {
        let mut c = Connection::open_in_memory().unwrap();
        assert_eq!(migrate(&mut c).unwrap(), latest());
        assert_eq!(migrate(&mut c).unwrap(), latest());
        let v: i64 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, latest());
    }

    #[test]
    fn upgrades_a_v1_database() {
        let mut c = Connection::open_in_memory().unwrap();
        let tx = c.transaction().unwrap();
        tx.execute_batch(MIGRATIONS[0]).unwrap();
        tx.pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        tx.pragma_update(None, "user_version", 1).unwrap();
        tx.commit().unwrap();
        assert_eq!(migrate(&mut c).unwrap(), latest());
        c.execute_batch(
            "SELECT original_request_blob, original_response_blob, highlight FROM exchanges; \
             SELECT mark FROM site_marks",
        )
        .unwrap();
    }

    #[test]
    fn refuses_foreign_database() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE t(x)").unwrap();
        assert!(matches!(migrate(&mut c), Err(StorageError::NotAProject)));
    }

    #[test]
    fn refuses_newer_schema() {
        let mut c = Connection::open_in_memory().unwrap();
        migrate(&mut c).unwrap();
        c.pragma_update(None, "user_version", latest() + 1).unwrap();
        assert!(matches!(
            migrate(&mut c),
            Err(StorageError::NewerSchema { .. })
        ));
    }
}
