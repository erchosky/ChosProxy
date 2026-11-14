//! The keyring behind the OS keychain or a master password (M31b): rules
//! that use a credential work only while the keyring can be read.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{CredentialInfo, Engine, KeyringProtection};
use chosproxy_core::paths::AppPaths;

fn lab_credential() -> CredentialInfo {
    CredentialInfo {
        id: "lab".into(),
        label: "Lab user".into(),
        username: "lab".into(),
        has_secret: false,
    }
}

#[tokio::test]
async fn the_keyring_moves_between_protections_and_locks() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.use_memory_keychain();
    engine
        .credential_set(lab_credential(), Some("secret".into()))
        .await
        .unwrap();
    let status = engine.keyring_status().await.unwrap();
    assert_eq!(status.protection, KeyringProtection::File);
    assert!(!status.locked && status.os_keychain_available);
    let key_file = AppPaths::under(dir.path())
        .data_dir
        .join("keyring/credentials.key");
    assert!(key_file.is_file());

    // The OS keychain: no key file left, the credential still there.
    let status = engine
        .keyring_set_protection(KeyringProtection::OsKeychain, None)
        .await
        .unwrap();
    assert_eq!(status.protection, KeyringProtection::OsKeychain);
    assert!(!key_file.exists());
    assert!(engine.credentials().await.unwrap()[0].has_secret);

    // A master password: kept open in this session.
    assert!(
        engine
            .keyring_set_protection(KeyringProtection::Password, Some("short".into()))
            .await
            .is_err()
    );
    let status = engine
        .keyring_set_protection(KeyringProtection::Password, Some("correct horse".into()))
        .await
        .unwrap();
    assert!(status.protection == KeyringProtection::Password && !status.locked);
    assert_eq!(engine.credentials().await.unwrap().len(), 1);

    // Another session (a new engine on the same files) starts locked.
    let again = Engine::new(AppPaths::under(dir.path())).unwrap();
    again.use_memory_keychain();
    assert!(again.keyring_status().await.unwrap().locked);
    let err = again.credentials().await.unwrap_err();
    assert!(err.to_string().contains("locked"), "{err}");
    assert!(
        again
            .credential_set(lab_credential(), Some("x".into()))
            .await
            .is_err(),
        "nothing is written to a locked keyring"
    );
    assert!(again.keyring_unlock("wrong horse!".into()).await.is_err());
    assert!(
        !again
            .keyring_unlock("correct horse".into())
            .await
            .unwrap()
            .locked
    );
    assert!(again.credentials().await.unwrap()[0].has_secret);
    // Locking forgets the key.
    assert!(again.keyring_lock().await.unwrap().locked);
    assert!(again.credentials().await.is_err());

    // Back to the key file from the session that has it open.
    let status = engine
        .keyring_set_protection(KeyringProtection::File, None)
        .await
        .unwrap();
    assert_eq!(status.protection, KeyringProtection::File);
    assert!(key_file.is_file());
    assert!(!again.keyring_status().await.unwrap().locked);
    assert!(again.credentials().await.unwrap()[0].has_secret);
    // The password never reached the audit log or the files.
    let files = std::fs::read_dir(key_file.parent().unwrap()).unwrap();
    for f in files {
        let bytes = std::fs::read(f.unwrap().path()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("correct horse"));
    }
}
