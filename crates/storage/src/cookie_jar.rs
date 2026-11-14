//! The project cookie jar: cookies set by responses (proxy and Repeater
//! traffic), which Repeater tabs can add to their requests.
//!
//! Parsing and matching follow RFC 6265 and live in `chosproxy-http`
//! (`cookies`); this module only stores what it is given. A cookie is
//! identified by name, domain and path, as in a browser.

use crate::StorageError;
use crate::db::ProjectDb;
use rusqlite::{Row, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most cookies kept; the least recently updated go first.
pub const MAX_JAR_COOKIES: usize = 5000;
/// Longest cookie value kept.
const MAX_VALUE_BYTES: usize = 8 * 1024;

/// A cookie in the jar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JarCookie {
    /// Row id.
    #[ts(type = "number")]
    pub id: i64,
    /// Domain (lower case).
    pub domain: String,
    /// Sent to `domain` only, not its subdomains.
    pub host_only: bool,
    /// Path prefix.
    pub path: String,
    /// Name.
    pub name: String,
    /// Value.
    pub value: String,
    /// Expiry, Unix epoch ms; `null` for a session cookie.
    #[ts(type = "number | null")]
    pub expires_at: Option<i64>,
    /// Only sent over HTTPS.
    pub secure: bool,
    /// `HttpOnly`.
    pub http_only: bool,
    /// `SameSite` as set.
    pub same_site: Option<String>,
    /// Where it was last set: `proxy`, `repeater` or `user`.
    pub source: String,
    /// First stored, Unix epoch ms.
    #[ts(type = "number")]
    pub created_at: i64,
    /// Last set, Unix epoch ms.
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// A cookie to store (or, if `expired`, to remove).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JarEntry {
    /// Domain (lower case).
    pub domain: String,
    /// Host-only flag.
    pub host_only: bool,
    /// Path.
    pub path: String,
    /// Name.
    pub name: String,
    /// Value.
    pub value: String,
    /// Expiry, Unix epoch ms.
    pub expires_at: Option<i64>,
    /// `Secure`.
    pub secure: bool,
    /// `HttpOnly`.
    pub http_only: bool,
    /// `SameSite`.
    pub same_site: Option<String>,
    /// The response deletes the cookie (expiry in the past).
    pub expired: bool,
}

const COLUMNS: &str = "id, domain, host_only, path, name, value, expires_at, secure, http_only, \
    same_site, source, created_at, updated_at";

fn map_cookie(r: &Row<'_>) -> rusqlite::Result<JarCookie> {
    Ok(JarCookie {
        id: r.get(0)?,
        domain: r.get(1)?,
        host_only: r.get(2)?,
        path: r.get(3)?,
        name: r.get(4)?,
        value: r.get(5)?,
        expires_at: r.get(6)?,
        secure: r.get(7)?,
        http_only: r.get(8)?,
        same_site: r.get(9)?,
        source: r.get(10)?,
        created_at: r.get(11)?,
        updated_at: r.get(12)?,
    })
}

fn truncate(value: &str) -> String {
    if value.len() <= MAX_VALUE_BYTES {
        return value.to_string();
    }
    let mut end = MAX_VALUE_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn store(
    c: &rusqlite::Connection,
    source: &'static str,
    entries: Vec<JarEntry>,
) -> Result<usize, StorageError> {
    let now = chosproxy_core::time::now_ms();
    let mut changed = 0;
    for e in entries {
        if e.expired {
            changed += c.execute(
                "DELETE FROM cookie_jar WHERE name = ?1 AND domain = ?2 AND path = ?3",
                params![e.name, e.domain, e.path],
            )?;
            continue;
        }
        changed += c.execute(
            "INSERT INTO cookie_jar (domain, host_only, path, name, value, expires_at, \
             secure, http_only, same_site, source, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11) \
             ON CONFLICT (name, domain, path) DO UPDATE SET host_only = excluded.host_only, \
             value = excluded.value, expires_at = excluded.expires_at, \
             secure = excluded.secure, http_only = excluded.http_only, \
             same_site = excluded.same_site, source = excluded.source, \
             updated_at = excluded.updated_at",
            params![
                e.domain,
                e.host_only,
                e.path,
                e.name,
                truncate(&e.value),
                e.expires_at,
                e.secure,
                e.http_only,
                e.same_site,
                source,
                now
            ],
        )?;
    }
    c.execute(
        "DELETE FROM cookie_jar WHERE id NOT IN \
         (SELECT id FROM cookie_jar ORDER BY updated_at DESC, id DESC LIMIT ?1)",
        [i64::try_from(MAX_JAR_COOKIES).unwrap_or(i64::MAX)],
    )?;
    Ok(changed)
}

impl ProjectDb {
    /// Stores cookies set by a response (`source` is `proxy` or
    /// `repeater`); expired entries delete their cookie. Returns how many
    /// cookies changed.
    pub fn cookie_jar_store(
        &self,
        source: &'static str,
        entries: Vec<JarEntry>,
    ) -> Result<usize, StorageError> {
        if entries.is_empty() {
            return Ok(0);
        }
        self.write(move |c| store(c, source, entries))
    }

    /// Like [`Self::cookie_jar_store`] without waiting (for the proxy's
    /// capture path).
    pub fn cookie_jar_submit(
        &self,
        source: &'static str,
        entries: Vec<JarEntry>,
    ) -> Result<(), StorageError> {
        if entries.is_empty() {
            return Ok(());
        }
        self.write_detached(move |c| store(c, source, entries).map(|_| ()))
    }

    /// Every cookie that has not expired, by domain, path and name.
    pub fn cookie_jar_list(&self) -> Result<Vec<JarCookie>, StorageError> {
        let now = chosproxy_core::time::now_ms();
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM cookie_jar \
                 WHERE expires_at IS NULL OR expires_at > ?1 \
                 ORDER BY domain, path DESC, name"
            ))?;
            let cookies = stmt
                .query_map([now], map_cookie)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(cookies)
        })
    }

    /// Changes a cookie's value (by hand: source `user`).
    pub fn cookie_jar_set_value(&self, id: i64, value: String) -> Result<(), StorageError> {
        if value.contains([';', '\r', '\n']) {
            return Err(StorageError::InvalidQuery(
                "a cookie value cannot contain ';' or line breaks".into(),
            ));
        }
        self.write(move |c| {
            let n = c.execute(
                "UPDATE cookie_jar SET value = ?2, source = 'user', updated_at = ?3 WHERE id = ?1",
                params![id, truncate(&value), chosproxy_core::time::now_ms()],
            )?;
            if n == 0 {
                return Err(StorageError::NotFound(id));
            }
            Ok(())
        })
    }

    /// Removes some cookies.
    pub fn cookie_jar_delete(&self, ids: Vec<i64>) -> Result<usize, StorageError> {
        self.write(move |c| {
            let mut n = 0;
            for id in ids {
                n += c.execute("DELETE FROM cookie_jar WHERE id = ?1", [id])?;
            }
            Ok(n)
        })
    }

    /// Empties the jar.
    pub fn cookie_jar_clear(&self) -> Result<usize, StorageError> {
        self.write(|c| Ok(c.execute("DELETE FROM cookie_jar", [])?))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::OpenOptions;

    fn entry(name: &str, value: &str) -> JarEntry {
        JarEntry {
            domain: "lab.test".into(),
            host_only: true,
            path: "/".into(),
            name: name.into(),
            value: value.into(),
            expires_at: None,
            secure: false,
            http_only: true,
            same_site: None,
            expired: false,
        }
    }

    #[test]
    fn stores_replaces_edits_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let mut other_path = entry("sid", "x");
        other_path.path = "/admin".into();
        let stale = JarEntry {
            expires_at: Some(1),
            ..entry("old", "1")
        };
        db.cookie_jar_store("proxy", vec![entry("sid", "1"), other_path, stale])
            .unwrap();
        db.cookie_jar_submit("repeater", vec![entry("sid", "2")])
            .unwrap();
        db.flush().unwrap();
        let jar = db.cookie_jar_list().unwrap();
        assert_eq!(jar.len(), 2, "same name and path replaced; expired hidden");
        let root = jar.iter().find(|c| c.path == "/").unwrap();
        assert_eq!(
            (root.value.as_str(), root.source.as_str()),
            ("2", "repeater")
        );

        db.cookie_jar_set_value(root.id, "3".into()).unwrap();
        assert!(db.cookie_jar_set_value(root.id, "a;b".into()).is_err());
        let gone = JarEntry {
            expired: true,
            ..entry("sid", "")
        };
        assert_eq!(db.cookie_jar_store("proxy", vec![gone]).unwrap(), 1);
        let jar = db.cookie_jar_list().unwrap();
        assert_eq!(jar.len(), 1);
        assert_eq!(jar[0].path, "/admin");

        assert_eq!(db.cookie_jar_delete(vec![jar[0].id]).unwrap(), 1);
        db.cookie_jar_store("proxy", vec![entry("a", "1")]).unwrap();
        assert_eq!(db.cookie_jar_clear().unwrap(), 2, "the expired row too");
        assert!(db.cookie_jar_list().unwrap().is_empty());
    }
}
