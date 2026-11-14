//! Encrypted exports & imports for Organizer collections and Settings (M31 C,
//! ORGANIZER-003, ADR-0041).

#![allow(clippy::unwrap_used)]

use chosproxy_app::{Engine, SettingsSection};
use chosproxy_core::ScopeConfig;
use chosproxy_core::paths::AppPaths;
use chosproxy_storage::{
    NewOrganizerItem, OpenOptions, OrganizerStatus, OrganizerUpdate, ProjectDb, Severity,
};
use std::path::Path;

const SECRET_MARKER: &str = "SUPER-SECRET-TOKEN-9876543210";

fn contains_marker(path: &Path) -> bool {
    let bytes = std::fs::read(path).unwrap_or_default();
    bytes
        .windows(SECRET_MARKER.len())
        .any(|w| w == SECRET_MARKER.as_bytes())
}

fn setup() -> (tempfile::TempDir, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    (dir, engine)
}

fn seed_organizer_items(db_path: &Path) -> Vec<i64> {
    let db = ProjectDb::open(db_path, OpenOptions::default()).unwrap();
    let mut req1 = format!(
        "POST /api/login HTTP/1.1\r\nHost: app.example.com\r\nAuthorization: Bearer {SECRET_MARKER}\r\n\r\n{{\"u\":\"alice\"}}"
    )
    .into_bytes();
    req1.extend_from_slice(&[0x00, 0xff]);
    let mut resp1 =
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}".to_vec();
    resp1.extend_from_slice(&[0x00, 0xfe]);

    db.organizer_insert_all(vec![
        NewOrganizerItem {
            fields: OrganizerUpdate {
                name: "Login token".into(),
                notes: format!("Contains {SECRET_MARKER}"),
                tags: vec!["auth".into(), "token".into()],
                status: OrganizerStatus::Confirmed,
                severity: Severity::High,
                collection: "Auth".into(),
            },
            method: "POST".into(),
            url: "https://app.example.com/api/login".into(),
            status_code: Some(200),
            request: req1,
            response: Some(resp1),
        },
        NewOrganizerItem {
            fields: OrganizerUpdate {
                name: "MFA challenge".into(),
                notes: "Nested child collection".into(),
                tags: vec!["mfa".into()],
                status: OrganizerStatus::Investigating,
                severity: Severity::Medium,
                collection: "Auth/Login/MFA".into(),
            },
            method: "GET".into(),
            url: "https://app.example.com/api/mfa".into(),
            status_code: Some(401),
            request: b"GET /api/mfa HTTP/1.1\r\nHost: app.example.com\r\n\r\n".to_vec(),
            response: Some(b"HTTP/1.1 401 Unauthorized\r\n\r\n".to_vec()),
        },
        NewOrganizerItem {
            fields: OrganizerUpdate {
                name: "Billing list".into(),
                notes: "Other collection".into(),
                tags: vec!["billing".into()],
                status: OrganizerStatus::Todo,
                severity: Severity::Low,
                collection: "Billing".into(),
            },
            method: "GET".into(),
            url: "https://billing.example.com/invoices".into(),
            status_code: Some(200),
            request: b"GET /invoices HTTP/1.1\r\nHost: billing.example.com\r\n\r\n".to_vec(),
            response: None,
        },
    ])
    .unwrap()
}

#[tokio::test]
async fn organizer_sealed_and_plain_roundtrip_with_nested_collections_and_exact_bytes() {
    let (dir, engine) = setup();
    let p1 = dir.path().join("src.chosproxy");
    engine.create_project(&p1).await.unwrap();
    engine.close_project().await.unwrap();

    let ids = seed_organizer_items(&p1);
    engine.open_project(&p1).await.unwrap();

    let orig_d1 = engine.organizer_detail(ids[0]).await.unwrap();
    let orig_d2 = engine.organizer_detail(ids[1]).await.unwrap();

    // Exporting parent collection "Auth" includes "Auth" and nested "Auth/Login/MFA" (2 items),
    // excluding "Billing".
    let sealed_path = dir.path().join("auth.chosorg");
    let share = engine
        .organizer_export(
            &sealed_path,
            Some("Auth".into()),
            Some("correct-horse-battery".into()),
        )
        .await
        .unwrap();
    assert_eq!(share.items, 2);
    assert!(share.encrypted);
    assert!(engine.is_sealed_file(&sealed_path).unwrap());
    assert!(
        !contains_marker(&sealed_path),
        "plaintext marker must not appear in sealed file"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&sealed_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "exported file must have 0600 permissions");
    }

    // Export plain all-items bundle too.
    let plain_path = dir.path().join("all-plain.chosorg");
    let plain_share = engine
        .organizer_export(&plain_path, None, None)
        .await
        .unwrap();
    assert_eq!(plain_share.items, 3);
    assert!(!plain_share.encrypted);
    assert!(!engine.is_sealed_file(&plain_path).unwrap());

    // Open a fresh destination project and verify import behavior.
    let p2 = dir.path().join("dst.chosproxy");
    engine.create_project(&p2).await.unwrap();

    // Missing password on sealed file fails with encrypted error.
    let err = engine
        .organizer_import(&sealed_path, None)
        .await
        .unwrap_err();
    assert_eq!(err.payload().kind, "encrypted");
    assert!(engine.organizer_list().await.unwrap().is_empty());

    // Wrong password fails with wrongPassword and adds nothing.
    let err = engine
        .organizer_import(&sealed_path, Some("wrong-password".into()))
        .await
        .unwrap_err();
    assert_eq!(err.payload().kind, "wrongPassword");
    assert!(engine.organizer_list().await.unwrap().is_empty());

    // Correct password imports the 2 items with byte-identical messages and metadata.
    let imported = engine
        .organizer_import(&sealed_path, Some("correct-horse-battery".into()))
        .await
        .unwrap();
    assert_eq!(imported.items, 2);
    assert!(imported.encrypted);

    let list = engine.organizer_list().await.unwrap();
    assert_eq!(list.len(), 2);
    let login = list.iter().find(|i| i.name == "Login token").unwrap();
    assert_eq!(login.notes, format!("Contains {SECRET_MARKER}"));
    assert_eq!(login.tags, vec!["auth".to_string(), "token".to_string()]);
    assert_eq!(login.status, OrganizerStatus::Confirmed);
    assert_eq!(login.severity, Severity::High);
    assert_eq!(login.collection, "Auth");
    assert_eq!(login.method, "POST");
    assert_eq!(login.url, "https://app.example.com/api/login");
    assert_eq!(login.status_code, Some(200));

    let login_detail = engine.organizer_detail(login.id).await.unwrap();
    assert_eq!(login_detail.request.raw_base64, orig_d1.request.raw_base64);
    assert_eq!(
        login_detail.response.as_ref().map(|r| &r.raw_base64),
        orig_d1.response.as_ref().map(|r| &r.raw_base64)
    );

    let mfa = list.iter().find(|i| i.name == "MFA challenge").unwrap();
    assert_eq!(mfa.collection, "Auth/Login/MFA");
    let mfa_detail = engine.organizer_detail(mfa.id).await.unwrap();
    assert_eq!(mfa_detail.request.raw_base64, orig_d2.request.raw_base64);
    assert_eq!(
        mfa_detail.response.as_ref().map(|r| &r.raw_base64),
        orig_d2.response.as_ref().map(|r| &r.raw_base64)
    );
}

#[tokio::test]
async fn tampered_truncated_corrupt_and_readonly_imports_are_rejected_atomically() {
    let (dir, engine) = setup();
    let p1 = dir.path().join("p1.chosproxy");
    engine.create_project(&p1).await.unwrap();
    engine.close_project().await.unwrap();

    seed_organizer_items(&p1);
    engine.open_project(&p1).await.unwrap();

    let sealed_path = dir.path().join("sealed.chosorg");
    engine
        .organizer_export(&sealed_path, None, Some("secret1234".into()))
        .await
        .unwrap();
    let raw = std::fs::read(&sealed_path).unwrap();

    // 1. Tampered version byte in header -> rejected.
    let mut bad_ver = raw.clone();
    bad_ver[8] = 99;
    let bad_ver_path = dir.path().join("bad-ver.chosorg");
    std::fs::write(&bad_ver_path, &bad_ver).unwrap();
    assert!(
        engine
            .organizer_import(&bad_ver_path, Some("secret1234".into()))
            .await
            .is_err()
    );

    // 2. Tampered salt/AAD byte in header -> rejected.
    let mut bad_aad = raw.clone();
    bad_aad[16] ^= 0x01;
    let bad_aad_path = dir.path().join("bad-aad.chosorg");
    std::fs::write(&bad_aad_path, &bad_aad).unwrap();
    assert!(
        engine
            .organizer_import(&bad_aad_path, Some("secret1234".into()))
            .await
            .is_err()
    );

    // 3. Tampered ciphertext/tag byte -> rejected.
    let mut bad_ct = raw.clone();
    let last = bad_ct.len() - 1;
    bad_ct[last] ^= 0x80;
    let bad_ct_path = dir.path().join("bad-ct.chosorg");
    std::fs::write(&bad_ct_path, &bad_ct).unwrap();
    assert!(
        engine
            .organizer_import(&bad_ct_path, Some("secret1234".into()))
            .await
            .is_err()
    );

    // 4. Truncated sealed file -> rejected.
    let trunc_path = dir.path().join("trunc.chosorg");
    std::fs::write(&trunc_path, &raw[..20]).unwrap();
    assert!(
        engine
            .organizer_import(&trunc_path, Some("secret1234".into()))
            .await
            .is_err()
    );

    // 5. Multi-item export where item 2 has invalid tag -> all-or-nothing rollback.
    let partial_path = dir.path().join("partial.chosorg");
    std::fs::write(
        &partial_path,
        br#"{
          "format": "chosproxy-organizer",
          "version": 1,
          "exportedAt": 1700000000000,
          "items": [
            {
              "name": "Valid item 1",
              "notes": "",
              "tags": ["ok"],
              "status": "todo",
              "severity": "low",
              "collection": "A",
              "method": "GET",
              "url": "https://example.com/1",
              "statusCode": 200,
              "request": "R0VUIC8xIEhUVFAvMS4xDQpIb3N0OiBleGFtcGxlLmNvbQ0KDQo=",
              "response": null
            },
            {
              "name": "Invalid item 2",
              "notes": "",
              "tags": ["bad,tag,with,commas"],
              "status": "todo",
              "severity": "low",
              "collection": "A",
              "method": "GET",
              "url": "https://example.com/2",
              "statusCode": 200,
              "request": "R0VUIC8yIEhUVFAvMS4xDQpIb3N0OiBleGFtcGxlLmNvbQ0KDQo=",
              "response": null
            }
          ]
        }"#,
    )
    .unwrap();

    let count_before = engine.organizer_list().await.unwrap().len();
    assert!(engine.organizer_import(&partial_path, None).await.is_err());
    assert_eq!(
        engine.organizer_list().await.unwrap().len(),
        count_before,
        "failed import must roll back all items"
    );

    // 6. Failed export must not corrupt an existing file at destination path.
    let existing = dir.path().join("existing.chosorg");
    std::fs::write(&existing, b"keep-me-intact").unwrap();
    let too_long_pw = "p".repeat(2000);
    assert!(
        engine
            .organizer_export(&existing, None, Some(too_long_pw))
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&existing).unwrap(), b"keep-me-intact");

    // 7. Read-only project allows export, refuses import.
    engine.close_project().await.unwrap();
    engine.open_project_read_only(&p1, None).await.unwrap();
    let ro_export = dir.path().join("ro.chosorg");
    engine
        .organizer_export(&ro_export, None, None)
        .await
        .unwrap();
    let ro_err = engine.organizer_import(&ro_export, None).await.unwrap_err();
    assert_eq!(ro_err.payload().kind, "readOnly");
}

#[tokio::test]
async fn settings_sealed_and_plain_export_preview_and_apply() {
    let (dir, engine) = setup();
    let p1 = dir.path().join("settings-proj.chosproxy");
    engine.create_project(&p1).await.unwrap();

    engine
        .add_url_to_scope("https://secure.example.com/", false, false)
        .unwrap();

    let sealed_settings = dir.path().join("cfg.chossettings");
    engine
        .export_settings_with(
            &sealed_settings,
            &[SettingsSection::App, SettingsSection::Scope],
            Some("settings-pass-123"),
        )
        .await
        .unwrap();
    assert!(engine.is_sealed_file(&sealed_settings).unwrap());
    let raw = std::fs::read(&sealed_settings).unwrap();
    assert!(
        !raw.windows(b"secure.example.com".len())
            .any(|w| w == b"secure.example.com"),
        "sealed settings must not expose scope hosts in plaintext"
    );

    // Reset scope to empty so preview detects a change.
    engine.save_scope_config(ScopeConfig::default()).unwrap();

    // Preview without password or with wrong password fails.
    assert_eq!(
        engine
            .preview_settings_import_with(&sealed_settings, None)
            .await
            .unwrap_err()
            .payload()
            .kind,
        "encrypted"
    );
    assert_eq!(
        engine
            .preview_settings_import_with(&sealed_settings, Some("wrong"))
            .await
            .unwrap_err()
            .payload()
            .kind,
        "wrongPassword"
    );

    // Preview with correct password succeeds and shows scope changed.
    let preview = engine
        .preview_settings_import_with(&sealed_settings, Some("settings-pass-123"))
        .await
        .unwrap();
    let scope_sec = preview
        .sections
        .iter()
        .find(|s| s.section == SettingsSection::Scope)
        .unwrap();
    assert!(scope_sec.valid && scope_sec.changed);

    // Apply with correct password updates scope.
    let applied = engine
        .apply_settings_import_with(
            &sealed_settings,
            &[SettingsSection::Scope],
            Some("settings-pass-123"),
        )
        .await
        .unwrap();
    assert_eq!(applied, vec![SettingsSection::Scope]);
    assert_eq!(engine.scope_config().include[0].host, "secure.example.com");
}
