//! Encrypted projects through the engine (M31, ADR-0041): creating and
//! opening with a password, a wrong or missing password leaving the open
//! project alone, password changes, Save As / export choices, backups and
//! restores of encrypted projects, and encrypted temporary projects.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{Engine, ProjectEncryption};
use chosproxy_core::paths::AppPaths;
use std::path::Path;

const MARK: &str = "PLAINTEXT-MARKER-0123456789";

fn on_disk(path: &Path) -> bool {
    let bytes = std::fs::read(path).unwrap_or_default();
    bytes.windows(MARK.len()).any(|w| w == MARK.as_bytes())
}

fn setup() -> (tempfile::TempDir, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    (dir, engine)
}

#[tokio::test]
async fn passwords_open_encrypted_projects_and_mistakes_change_nothing() {
    let (dir, engine) = setup();
    let path = dir.path().join("secret.chosproxy");
    let info = engine
        .create_project_with(&path, Some("correct horse"))
        .await
        .unwrap();
    assert!(info.encrypted);
    engine.save_project_notes(MARK.into()).await.unwrap();
    engine.close_project().await.unwrap();
    assert!(!on_disk(&path));

    // Without or with a wrong password nothing opens, and the project that
    // is open stays open.
    let temp = engine.new_temporary_project().await.unwrap();
    let missing = engine.open_project(&path).await.unwrap_err();
    assert!(
        missing.to_string().contains("password is needed"),
        "{missing}"
    );
    let wrong = engine
        .open_project_with(&path, Some("nope"))
        .await
        .unwrap_err();
    assert!(wrong.to_string().contains("wrong password"), "{wrong}");
    assert_eq!(engine.project().unwrap().path, temp.path);

    // A preview without the password only says it is encrypted.
    let peek = engine.inspect_project(&path).await.unwrap();
    assert!(peek.encrypted && peek.exchanges == 0);
    assert!(
        engine
            .inspect_project_with(&path, Some("correct horse"))
            .await
            .unwrap()
            .encrypted
    );

    let opened = engine
        .open_project_with(&path, Some("correct horse"))
        .await
        .unwrap();
    assert!(opened.encrypted);
    assert_eq!(engine.project_notes().await.unwrap(), MARK);

    // Change the password.
    engine
        .change_project_password("battery staple")
        .await
        .unwrap();
    engine.close_project().await.unwrap();
    assert!(
        engine
            .open_project_with(&path, Some("correct horse"))
            .await
            .is_err()
    );
    engine
        .open_project_with(&path, Some("battery staple"))
        .await
        .unwrap();
    assert_eq!(engine.project_notes().await.unwrap(), MARK);
    let audit = engine.audit_log(100).await.unwrap();
    assert!(audit.iter().any(|a| a.action == "project.password.changed"));
    assert!(
        audit.iter().all(|a| !a.detail.contains("battery")),
        "passwords never reach the audit log"
    );

    // A plain project has no password to change.
    engine.new_temporary_project().await.unwrap();
    assert!(engine.change_project_password("x").await.is_err());
}

#[tokio::test]
async fn copies_backups_and_restores_follow_the_encryption() {
    let (dir, engine) = setup();
    let path = dir.path().join("p.chosproxy");
    engine
        .create_project_with(&path, Some("one"))
        .await
        .unwrap();
    engine.save_project_notes(MARK.into()).await.unwrap();

    // Export: same encryption, none, another password.
    let same = dir.path().join("same.chosproxy");
    let info = engine.export_project(&same, false).await.unwrap();
    assert!(info.encrypted && !on_disk(&same));
    let plain = dir.path().join("plain.chosproxy");
    let info = engine
        .export_project_with(&plain, false, ProjectEncryption::None)
        .await
        .unwrap();
    assert!(!info.encrypted && on_disk(&plain));
    let other = dir.path().join("other.chosproxy");
    engine
        .export_project_with(
            &other,
            false,
            ProjectEncryption::Password {
                password: "two".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        engine
            .inspect_project_with(&other, Some("two"))
            .await
            .unwrap()
            .encrypted
    );
    let audit = engine.audit_log(100).await.unwrap();
    assert!(audit.iter().any(|a| a.action == "project.decrypted"));

    // Backups of an encrypted project are encrypted, verified and restored
    // with the open project's key.
    let backup = engine.backup_now().await.unwrap();
    assert!(!on_disk(Path::new(&backup.path)));
    let restored = engine
        .restore_backup(Path::new(&backup.path))
        .await
        .unwrap();
    assert!(restored.encrypted);
    assert_eq!(engine.project_notes().await.unwrap(), MARK);

    // Save As without encryption, then encrypted again.
    let saved = dir.path().join("saved-plain.chosproxy");
    let info = engine
        .save_project_as_with(&saved, ProjectEncryption::None)
        .await
        .unwrap();
    assert!(!info.encrypted);
    let resaved = dir.path().join("saved-enc.chosproxy");
    let info = engine
        .save_project_as_with(
            &resaved,
            ProjectEncryption::Password {
                password: "three".into(),
            },
        )
        .await
        .unwrap();
    assert!(info.encrypted && !on_disk(&resaved));
}

#[tokio::test]
async fn temporary_projects_can_be_encrypted_with_a_key_in_memory() {
    let (dir, engine) = setup();
    let info = engine.new_temporary_project_with(true).await.unwrap();
    assert!(info.encrypted);
    engine.save_project_notes(MARK.into()).await.unwrap();
    assert!(!on_disk(Path::new(&info.path)));
    // Its random key is not carried to a file: Save As keeps it readable
    // unless a password is given.
    let saved = dir.path().join("kept.chosproxy");
    let saved_info = engine.save_project_as(&saved).await.unwrap();
    assert!(!saved_info.encrypted);
    assert_eq!(engine.project_notes().await.unwrap(), MARK);
    engine.close_project().await.unwrap();
    assert!(!Path::new(&info.path).exists(), "temporary file deleted");
}
