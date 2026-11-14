//! Keys of encrypted projects (M31, ADR-0041). The encryption itself is
//! SQLCipher's (AES-256, with its own key derivation for passwords); this
//! module only carries the secret and checks its shape. A key is never
//! logged or serialized.

use crate::StorageError;
use std::sync::Arc;

/// The first bytes of every unencrypted SQLite file.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
/// Longest password accepted.
const MAX_PASSWORD: usize = 1024;

/// The secret that opens an encrypted project.
#[derive(Clone, PartialEq, Eq)]
pub struct ProjectKey(Arc<str>);

impl std::fmt::Debug for ProjectKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProjectKey(hidden)")
    }
}

impl ProjectKey {
    /// A password chosen by the tester (SQLCipher derives the key from it).
    pub fn password(password: &str) -> Result<ProjectKey, StorageError> {
        if password.is_empty() {
            return Err(StorageError::InvalidQuery("the password is empty".into()));
        }
        if password.chars().count() > MAX_PASSWORD || password.contains('\0') {
            return Err(StorageError::InvalidQuery(
                "the password is too long or contains a NUL character".into(),
            ));
        }
        // SQLCipher reads x'…' as a raw key; a password must not look like one.
        if password.starts_with("x'") || password.starts_with("X'") {
            return Err(StorageError::InvalidQuery(
                "a password cannot start with x'".into(),
            ));
        }
        Ok(ProjectKey(password.into()))
    }

    /// A random 256-bit key (for temporary projects, kept in memory only).
    pub fn random() -> Result<ProjectKey, StorageError> {
        use ring::rand::SecureRandom;
        let mut bytes = [0u8; 32];
        ring::rand::SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| StorageError::InvalidQuery("no random numbers available".into()))?;
        let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
        Ok(ProjectKey(format!("x'{hex}'").into()))
    }

    /// The value given to `PRAGMA key` / `rekey` / `ATTACH … KEY`.
    pub(crate) fn secret(&self) -> &str {
        &self.0
    }

    /// Unlocks a connection (must be its first statement) and checks the key
    /// is right.
    pub(crate) fn unlock(
        key: Option<&ProjectKey>,
        c: &rusqlite::Connection,
    ) -> Result<(), StorageError> {
        if let Some(k) = key {
            c.pragma_update(None, "key", k.secret())?;
        }
        match c.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        }) {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::NotADatabase =>
            {
                Err(if key.is_some() {
                    StorageError::WrongKey
                } else {
                    StorageError::Encrypted
                })
            }
            Err(e) => Err(e.into()),
        }
    }
}

/// Whether a file looks like an encrypted database: it exists, is not
/// empty, and does not start with SQLite's plain header.
pub fn is_encrypted_file(path: &std::path::Path) -> std::io::Result<bool> {
    use std::io::Read;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let mut head = [0u8; 16];
    let mut read = 0;
    while read < head.len() {
        let n = f.read(&mut head[read..])?;
        if n == 0 {
            break;
        }
        read += n;
    }
    Ok(read > 0 && (read < 16 || &head != SQLITE_MAGIC))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_checked_and_never_shown() {
        assert!(ProjectKey::password("").is_err());
        assert!(ProjectKey::password("x'00'").is_err());
        assert!(ProjectKey::password("a\0b").is_err());
        let k = ProjectKey::password("correct horse").unwrap();
        assert_eq!(format!("{k:?}"), "ProjectKey(hidden)");
        let r = ProjectKey::random().unwrap();
        assert!(r.secret().starts_with("x'") && r.secret().len() == 67);
        assert_ne!(r, ProjectKey::random().unwrap());
    }
}
