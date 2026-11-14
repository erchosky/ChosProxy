//! Encrypted projects (M31, ADR-0041): nothing readable on disk, the key is
//! required and checked, the password can change, and copies keep, drop or
//! change the encryption as asked.

#![allow(clippy::unwrap_used)] // tests: panicking is the failure report

use chosproxy_storage::{
    CopyKey, OpenOptions, ProjectDb, ProjectKey, StorageError, inspect_project_file,
    inspect_project_file_with, is_encrypted_file,
};
use std::path::Path;

const MARK: &str = "PLAINTEXT-MARKER-0123456789";

fn on_disk(path: &Path, needle: &str) -> bool {
    let bytes = std::fs::read(path).unwrap_or_default();
    bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn open(path: &Path, key: Option<&ProjectKey>) -> Result<ProjectDb, StorageError> {
    ProjectDb::open(
        path,
        OpenOptions {
            key: key.cloned(),
            ..OpenOptions::default()
        },
    )
}

fn files(path: &Path) -> Vec<std::path::PathBuf> {
    ["", "-wal", "-shm"]
        .iter()
        .map(|s| std::path::PathBuf::from(format!("{}{s}", path.display())))
        .collect()
}

#[test]
fn encrypted_projects_hide_everything_and_need_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secret.chosproxy");
    let key = ProjectKey::password("correct horse battery").unwrap();
    let db = open(&path, Some(&key)).unwrap();
    assert!(db.is_encrypted());
    db.set_setting("notes", Some(MARK.into())).unwrap();
    db.flush().unwrap();
    // While open (WAL in use) and after closing, nothing is in clear.
    for f in files(&path) {
        assert!(!on_disk(&f, MARK), "{} shows the data", f.display());
    }
    drop(db);
    assert!(!on_disk(&path, MARK));
    assert!(is_encrypted_file(&path).unwrap());

    assert!(matches!(open(&path, None), Err(StorageError::Encrypted)));
    let wrong = ProjectKey::password("wrong").unwrap();
    assert!(matches!(
        open(&path, Some(&wrong)),
        Err(StorageError::WrongKey)
    ));
    assert!(matches!(
        inspect_project_file(&path),
        Err(StorageError::Encrypted)
    ));
    let info = inspect_project_file_with(&path, Some(&key)).unwrap();
    assert!(info.encrypted);
    assert!(
        ProjectDb::quick_check(&path, Some(&key))
            .unwrap()
            .is_empty()
    );

    // Read-only with the key.
    let ro = ProjectDb::open(
        &path,
        OpenOptions {
            read_only: true,
            key: Some(key.clone()),
            ..OpenOptions::default()
        },
    )
    .unwrap();
    assert_eq!(ro.setting("notes").unwrap().as_deref(), Some(MARK));
    drop(ro);

    // A new password: the old one stops working.
    let db = open(&path, Some(&key)).unwrap();
    let new = ProjectKey::password("new password").unwrap();
    db.rekey(new.clone()).unwrap();
    assert_eq!(db.setting("notes").unwrap().as_deref(), Some(MARK));
    db.set_setting("after", Some("1".into())).unwrap();
    drop(db);
    assert!(matches!(
        open(&path, Some(&key)),
        Err(StorageError::WrongKey)
    ));
    let db = open(&path, Some(&new)).unwrap();
    assert_eq!(db.setting("after").unwrap().as_deref(), Some("1"));
    assert!(!on_disk(&path, MARK));
}

#[test]
fn copies_keep_drop_or_change_the_encryption() {
    let dir = tempfile::tempdir().unwrap();
    let key = ProjectKey::password("one").unwrap();
    let path = dir.path().join("p.chosproxy");
    let db = open(&path, Some(&key)).unwrap();
    db.set_setting("notes", Some(MARK.into())).unwrap();

    // Same key (backups, Save As).
    let same = dir.path().join("same.chosproxy");
    db.copy_to(&same, false).unwrap();
    assert!(!on_disk(&same, MARK));
    assert!(db.inspect_copy(&same).unwrap().encrypted);
    assert_eq!(
        open(&same, Some(&key))
            .unwrap()
            .setting("notes")
            .unwrap()
            .as_deref(),
        Some(MARK)
    );

    // Without encryption (explicit choice).
    let plain = dir.path().join("plain.chosproxy");
    db.copy_with(&plain, false, CopyKey::Plain).unwrap();
    assert!(!is_encrypted_file(&plain).unwrap());
    let info = inspect_project_file(&plain).unwrap();
    assert!(!info.encrypted);
    assert_eq!(
        open(&plain, None)
            .unwrap()
            .setting("notes")
            .unwrap()
            .as_deref(),
        Some(MARK)
    );

    // A plain project saved encrypted.
    let plain_db = open(&plain, None).unwrap();
    let two = ProjectKey::password("two").unwrap();
    let enc = dir.path().join("enc.chosproxy");
    plain_db
        .copy_with(&enc, false, CopyKey::Key(two.clone()))
        .unwrap();
    assert!(!on_disk(&enc, MARK));
    assert!(matches!(
        open(&enc, Some(&key)),
        Err(StorageError::WrongKey)
    ));
    let reopened = open(&enc, Some(&two)).unwrap();
    assert_eq!(reopened.setting("notes").unwrap().as_deref(), Some(MARK));
    assert!(reopened.is_encrypted());

    // A plain project cannot be rekeyed in place.
    assert!(plain_db.rekey(two).is_err());
    // An existing destination is never overwritten.
    assert!(db.copy_with(&plain, false, CopyKey::Plain).is_err());
    assert!(!is_encrypted_file(&plain).unwrap());
}

#[test]
fn temporary_keys_are_random_raw_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.chosproxy");
    let key = ProjectKey::random().unwrap();
    let db = open(&path, Some(&key)).unwrap();
    db.set_setting("notes", Some(MARK.into())).unwrap();
    drop(db);
    assert!(!on_disk(&path, MARK));
    assert_eq!(
        open(&path, Some(&key))
            .unwrap()
            .setting("notes")
            .unwrap()
            .as_deref(),
        Some(MARK)
    );
}
