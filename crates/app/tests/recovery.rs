//! Crash recovery and read-only project mode integration tests (M31 A,
//! `PROJECT-004`, spec §118).

#![allow(clippy::unwrap_used)]

use chosproxy_app::{
    Engine, LiveMode, LiveTaskConfig, ProjectKind, RecoveryTask, SESSION_MARKER_FILE,
    SESSION_MARKER_VERSION, SessionMarker,
};
use chosproxy_core::paths::AppPaths;
use chosproxy_crawler::CrawlConfig;
use chosproxy_storage::{
    HistoryQuery, NewRepeaterTab, OpenOptions, ProjectDb, RepeaterTabUpdate, RepeaterTarget,
    TaskStatus,
};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[tokio::test]
async fn clean_close_and_shutdown_remove_session_marker() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(dir.path());
    let marker_path = paths.data_dir.join(SESSION_MARKER_FILE);

    let engine = Engine::new(paths.clone()).unwrap();
    assert!(engine.recovery_status().is_none());

    let proj_path = dir.path().join("clean.chosproxy");
    let info = engine.create_project(&proj_path).await.unwrap();
    assert!(!info.read_only);
    assert!(marker_path.is_file(), "marker written while project open");

    engine.close_project().await.unwrap();
    assert!(!marker_path.exists(), "clean close removes session marker");
    assert!(engine.recovery_status().is_none());

    // Temporary project closed via shutdown also removes marker.
    engine.new_temporary_project().await.unwrap();
    assert!(marker_path.is_file());
    engine.shutdown().await;
    assert!(!marker_path.exists());
}

#[tokio::test]
async fn simulated_crash_detected_and_restored_with_audit_entry() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(dir.path());
    let proj_path = dir.path().join("crash-persistent.chosproxy");

    {
        let engine = Engine::new(paths.clone()).unwrap();
        engine.create_project(&proj_path).await.unwrap();
        engine
            .save_project_notes("survives unexpected exit".into())
            .await
            .unwrap();
        // Drop `engine` without closing the project or calling shutdown().
    }

    let engine2 = Engine::new(paths.clone()).unwrap();
    let status = engine2
        .recovery_status()
        .expect("stale marker must be detected after unexpected exit");
    assert!(!status.marker_corrupt);
    assert_eq!(status.project_name, "crash-persistent");
    assert_eq!(status.project_kind, ProjectKind::Persistent);
    assert!(status.file_exists);
    assert!(status.restorable);
    assert!(!status.encrypted);

    let restored = engine2.recovery_restore(None).await.unwrap();
    assert_eq!(restored.name, "crash-persistent");
    assert!(!restored.read_only);
    assert!(engine2.recovery_status().is_none());
    assert_eq!(
        engine2.project_notes().await.unwrap(),
        "survives unexpected exit"
    );

    let audit = engine2.audit_log(50).await.unwrap();
    assert!(
        audit.iter().any(|e| e.action == "project.recovered"),
        "restore must record project.recovered audit event"
    );
    engine2.shutdown().await;
}

#[tokio::test]
async fn corrupt_and_unsupported_version_markers_handled_safely() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(dir.path());
    let engine = Engine::new(paths.clone()).unwrap();
    let marker_path = paths.data_dir.join(SESSION_MARKER_FILE);

    // 1. Corrupt JSON bytes.
    std::fs::write(&marker_path, b"{not valid json").unwrap();
    let status = engine.recovery_status().unwrap();
    assert!(status.marker_corrupt);
    assert!(!status.restorable);
    assert!(engine.recovery_restore(None).await.is_err());

    engine.recovery_discard(false).unwrap();
    assert!(!marker_path.exists());
    assert!(engine.recovery_status().is_none());

    // 2. Unsupported future schema version.
    let future = serde_json::json!({
        "version": SESSION_MARKER_VERSION + 99,
        "sessionId": "future-1",
        "pid": 999_999,
        "startedAt": 1,
        "updatedAt": 2,
        "projectName": "future",
        "projectPath": dir.path().join("future.chosproxy").display().to_string(),
        "projectKind": "persistent",
        "encrypted": false
    });
    std::fs::write(&marker_path, serde_json::to_vec(&future).unwrap()).unwrap();
    let status2 = engine.recovery_status().unwrap();
    assert!(status2.marker_corrupt);
    assert!(!status2.restorable);
    engine.recovery_discard(false).unwrap();
    assert!(engine.recovery_status().is_none());
}

#[tokio::test]
async fn reused_pid_with_mismatched_start_time_is_detected_as_crashed() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(dir.path());
    let proj_path = dir.path().join("reused-pid.chosproxy");

    // Create a real project file and close it cleanly.
    let engine = Engine::new(paths.clone()).unwrap();
    engine.create_project(&proj_path).await.unwrap();
    engine.close_project().await.unwrap();

    // Write a marker with the current (alive!) PID, but a bogus process start
    // timestamp (1 second after Unix epoch), simulating OS PID reuse.
    let marker = SessionMarker {
        version: SESSION_MARKER_VERSION,
        session_id: "stale-reused-pid".into(),
        pid: std::process::id(),
        process_start_secs: Some(1),
        started_at: 1_700_000_000_000,
        updated_at: 1_700_000_000_500,
        project_name: "reused-pid".into(),
        project_path: proj_path.display().to_string(),
        project_kind: ProjectKind::Persistent,
        encrypted: false,
        ephemeral_key: false,
        tasks: Vec::new(),
    };
    let marker_path = paths.data_dir.join(SESSION_MARKER_FILE);
    std::fs::write(&marker_path, serde_json::to_vec_pretty(&marker).unwrap()).unwrap();

    let status = engine
        .recovery_status()
        .expect("reused PID with different start time must be treated as crashed");
    assert!(status.restorable);
    assert_eq!(status.project_name, "reused-pid");

    engine.recovery_restore(None).await.unwrap();
    engine.shutdown().await;
}

#[tokio::test]
async fn live_sibling_instance_is_ignored_and_not_deleted_by_second_instance() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(dir.path());
    let marker_path = paths.data_dir.join(SESSION_MARKER_FILE);

    let engine1 = Engine::new(paths.clone()).unwrap();
    let p1 = dir.path().join("one.chosproxy");
    engine1.create_project(&p1).await.unwrap();
    assert!(marker_path.is_file());

    // Second instance starts while engine1 is still alive.
    let engine2 = Engine::new(paths.clone()).unwrap();
    assert!(
        engine2.recovery_status().is_none(),
        "live sibling instance must not trigger crash recovery"
    );

    // Second instance opens and closes its own project; engine1's marker stays.
    let p2 = dir.path().join("two.chosproxy");
    engine2.create_project(&p2).await.unwrap();
    engine2.close_project().await.unwrap();
    engine2.shutdown().await;
    assert!(
        marker_path.is_file(),
        "sibling shutdown must not remove engine1's active marker"
    );

    // Once engine1 drops without closing, a third instance sees engine1's crash.
    drop(engine1);
    let engine3 = Engine::new(paths).unwrap();
    let rec = engine3
        .recovery_status()
        .expect("dropped engine1 must now be detected as crashed");
    assert_eq!(rec.project_name, "one");
    engine3.recovery_discard(false).unwrap();
}

#[tokio::test]
async fn interrupted_traffic_tasks_do_not_restart_while_live_tasks_resume() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(dir.path());
    let proj_path = dir.path().join("tasks-crash.chosproxy");

    let crawl_id;
    let live_id;
    {
        let engine = Engine::new(paths.clone()).unwrap();
        engine.create_project(&proj_path).await.unwrap();

        // Start a live passive task.
        let live_row = engine
            .live_task_start(LiveTaskConfig {
                mode: LiveMode::Audit,
                sources: vec!["proxy".into()],
                ..LiveTaskConfig::default()
            })
            .await
            .unwrap();
        live_id = live_row.id;

        // Seed an unfinished traffic task ('crawl') directly in the DB and
        // marker to simulate a crash mid-crawl without racing network I/O.
        let db = ProjectDb::open(&proj_path, OpenOptions::default()).unwrap();
        let crawl_cfg = serde_json::to_string(&CrawlConfig {
            start_urls: vec!["http://127.0.0.1:9/".into()],
            ..CrawlConfig::default()
        })
        .unwrap();
        let crawl_row = db
            .task_create("crawl", "Crawl 127.0.0.1", &crawl_cfg)
            .unwrap();
        crawl_id = crawl_row.id;
        drop(db);

        // Update marker to include both tasks as running at crash time.
        let marker_path = paths.data_dir.join(SESSION_MARKER_FILE);
        let mut marker: SessionMarker =
            serde_json::from_slice(&std::fs::read(&marker_path).unwrap()).unwrap();
        marker.tasks.push(RecoveryTask {
            id: crawl_id,
            kind: "crawl".into(),
            name: "Crawl 127.0.0.1".into(),
        });
        std::fs::write(&marker_path, serde_json::to_vec_pretty(&marker).unwrap()).unwrap();
    }

    let engine2 = Engine::new(paths).unwrap();
    let status = engine2.recovery_status().unwrap();
    assert!(
        status
            .interrupted_tasks
            .iter()
            .any(|t| t.id == crawl_id && t.kind == "crawl")
    );
    assert!(
        status
            .interrupted_tasks
            .iter()
            .any(|t| t.id == live_id && t.kind == "live")
    );

    engine2.recovery_restore(None).await.unwrap();

    // Verify the crawl task was marked Cancelled with crash error and did NOT restart.
    let crawl_detail = engine2.task_detail(crawl_id).await.unwrap();
    assert_eq!(crawl_detail.row.status, TaskStatus::Cancelled);
    assert_eq!(
        crawl_detail.row.last_error.as_deref(),
        Some("Interrupted by unexpected exit")
    );
    assert_eq!(crawl_detail.row.requests, 0);
    assert!(
        crawl_detail
            .logs
            .iter()
            .any(|l| l.message.contains("exited unexpectedly"))
    );

    // Verify the passive live task resumed.
    for _ in 0..50 {
        if engine2.task_detail(live_id).await.unwrap().row.status == TaskStatus::Running {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let live_detail = engine2.task_detail(live_id).await.unwrap();
    assert_eq!(live_detail.row.status, TaskStatus::Running);
    engine2.shutdown().await;
}

#[tokio::test]
async fn read_only_mode_refuses_writes_and_leaves_file_bytes_and_mtime_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(dir.path());
    let proj_path = dir.path().join("readonly.chosproxy");

    let engine = Engine::new(paths).unwrap();
    engine.create_project(&proj_path).await.unwrap();
    engine
        .save_project_notes("initial note".into())
        .await
        .unwrap();
    let target = RepeaterTarget {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port: 80,
    };
    let tab = engine
        .repeater_create(NewRepeaterTab {
            name: "RO Tab".into(),
            target: target.clone(),
            request: "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n".into(),
            head_crlf: true,
            body_crlf: true,
        })
        .await
        .unwrap();
    engine.close_project().await.unwrap();

    // Set file mtime in the past and snapshot exact bytes.
    let past = SystemTime::now() - Duration::from_secs(120);
    {
        let f = std::fs::File::options()
            .write(true)
            .open(&proj_path)
            .unwrap();
        f.set_modified(past).unwrap();
    }
    let bytes_before = std::fs::read(&proj_path).unwrap();
    let mtime_before = std::fs::metadata(&proj_path).unwrap().modified().unwrap();

    let info = engine
        .open_project_read_only(&proj_path, None)
        .await
        .unwrap();
    assert!(info.read_only);

    // Reads work.
    assert_eq!(engine.project_notes().await.unwrap(), "initial note");
    assert_eq!(
        engine
            .history_page(HistoryQuery {
                limit: 10,
                ..HistoryQuery::default()
            })
            .await
            .unwrap()
            .total,
        0
    );

    // Writes, listeners, Repeater sends, and background tasks are refused with readOnly.
    for err in [
        engine
            .save_project_notes("mutate".into())
            .await
            .unwrap_err(),
        engine.start_listener("default").await.unwrap_err(),
        engine
            .repeater_send(
                tab.id,
                RepeaterTabUpdate {
                    name: tab.name.clone(),
                    target,
                    request: tab.request.clone(),
                    update_content_length: true,
                    reuse_connection: false,
                    use_cookie_jar: false,
                },
            )
            .await
            .unwrap_err(),
        engine
            .crawl_start(CrawlConfig {
                start_urls: vec!["http://127.0.0.1:80/".into()],
                ..CrawlConfig::default()
            })
            .await
            .unwrap_err(),
        engine.passive_reanalyze().await.unwrap_err(),
        engine
            .add_url_to_scope("http://example.com/", false, false)
            .unwrap_err(),
    ] {
        assert_eq!(
            err.kind(),
            "readOnly",
            "expected readOnly error, got {err:?}"
        );
    }

    // Intercept cannot be turned on in read-only mode.
    assert!(!engine.set_intercept(true).enabled);

    engine.close_project().await.unwrap();

    let bytes_after = std::fs::read(&proj_path).unwrap();
    let mtime_after = std::fs::metadata(&proj_path).unwrap().modified().unwrap();
    assert_eq!(
        bytes_before, bytes_after,
        "read-only mode must not alter file bytes"
    );
    assert_eq!(
        mtime_before, mtime_after,
        "read-only mode must not alter file mtime"
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn missing_or_moved_project_encrypted_recovery_and_corrupt_db() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(dir.path());
    let enc_path = dir.path().join("encrypted-crash.chosproxy");
    let moved_path = dir.path().join("moved-crash.chosproxy");

    {
        let engine = Engine::new(paths.clone()).unwrap();
        engine
            .create_project_with(&enc_path, Some("correct-horse-battery"))
            .await
            .unwrap();
        engine
            .save_project_notes("secret note".into())
            .await
            .unwrap();
        // Simulated crash while encrypted project is open.
    }

    // Move the file away temporarily: recovery reports missing file and is not restorable.
    std::fs::rename(&enc_path, &moved_path).unwrap();
    let engine2 = Engine::new(paths.clone()).unwrap();
    let missing = engine2.recovery_status().unwrap();
    assert!(!missing.file_exists);
    assert!(!missing.restorable);
    assert!(
        engine2
            .recovery_restore(Some("correct-horse-battery"))
            .await
            .is_err()
    );

    // Move the file back: now restorable, requires the right password.
    std::fs::rename(&moved_path, &enc_path).unwrap();
    let present = engine2.recovery_status().unwrap();
    assert!(present.file_exists && present.restorable && present.encrypted);

    // Open a temporary project first; a wrong password during recovery must
    // neither close the open temporary project nor delete the recovery marker!
    let temp = engine2.new_temporary_project().await.unwrap();
    let no_pw = engine2.recovery_restore(None).await.unwrap_err();
    assert_eq!(no_pw.kind(), "encrypted");
    let bad_pw = engine2
        .recovery_restore(Some("wrong-password"))
        .await
        .unwrap_err();
    assert_eq!(bad_pw.kind(), "wrongPassword");
    assert_eq!(engine2.project().unwrap().path, temp.path);
    assert!(
        engine2.recovery_status().is_some(),
        "marker kept after wrong password"
    );

    // Correct password restores the encrypted project.
    let restored = engine2
        .recovery_restore(Some("correct-horse-battery"))
        .await
        .unwrap();
    assert_eq!(restored.name, "encrypted-crash");
    assert!(restored.encrypted);
    assert_eq!(engine2.project_notes().await.unwrap(), "secret note");
    engine2.close_project().await.unwrap();

    // Corrupt database file fails quick_check and keeps marker intact.
    let corrupt_path = dir.path().join("corrupt.chosproxy");
    std::fs::write(&corrupt_path, b"SQLite format 3\0corrupt-database-pages").unwrap();
    let marker = SessionMarker {
        version: SESSION_MARKER_VERSION,
        session_id: "corrupt-db-session".into(),
        pid: 999_999,
        process_start_secs: Some(1),
        started_at: 1,
        updated_at: 2,
        project_name: "corrupt".into(),
        project_path: corrupt_path.display().to_string(),
        project_kind: ProjectKind::Persistent,
        encrypted: false,
        ephemeral_key: false,
        tasks: Vec::new(),
    };
    std::fs::write(
        paths.data_dir.join(SESSION_MARKER_FILE),
        serde_json::to_vec_pretty(&marker).unwrap(),
    )
    .unwrap();
    assert!(engine2.recovery_restore(None).await.is_err());
    assert!(
        engine2.recovery_status().is_some(),
        "corrupt DB must preserve recovery marker until user discards it"
    );
    engine2.recovery_discard(false).unwrap();
    engine2.shutdown().await;
}

#[tokio::test]
async fn temporary_projects_protected_until_recovery_decision() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(dir.path());

    // 1. Plain temporary project crash: protected from temp_cleanup, restorable,
    // or discardable with delete_orphan_temp.
    let temp_path: PathBuf;
    {
        let engine = Engine::new(paths.clone()).unwrap();
        let info = engine.new_temporary_project().await.unwrap();
        temp_path = PathBuf::from(&info.path);
        engine.save_project_notes("temp note".into()).await.unwrap();
    }
    assert!(temp_path.is_file());

    let engine2 = Engine::new(paths.clone()).unwrap();
    let rec = engine2.recovery_status().unwrap();
    assert_eq!(rec.project_kind, ProjectKind::Temporary);
    assert!(rec.restorable && !rec.ephemeral_key);

    // Temp cleanup must NOT delete the recoverable temporary project!
    let t_status = engine2.temp_status().await.unwrap();
    let temp_name = temp_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let entry = t_status
        .entries
        .iter()
        .find(|e| e.name == temp_name)
        .unwrap();
    assert!(entry.in_use && !entry.cleanable && !entry.removable);
    let cleaned = engine2.temp_cleanup().await.unwrap();
    assert_eq!(cleaned.removed, 0);
    assert!(temp_path.is_file());

    // Open read-only from recovery, then close: file is still kept if we only
    // opened read-only, or let's test discard with delete_orphan_temp = true.
    engine2.recovery_discard(true).unwrap();
    assert!(
        !temp_path.exists(),
        "discard(true) deletes orphaned temp project"
    );

    // 2. Encrypted temporary project crash: ephemeral key is gone, so
    // restorable == false and ephemeral_key == true.
    let enc_temp_path: PathBuf;
    {
        let engine3 = Engine::new(paths.clone()).unwrap();
        let info = engine3.new_temporary_project_with(true).await.unwrap();
        enc_temp_path = PathBuf::from(&info.path);
    }
    let engine4 = Engine::new(paths).unwrap();
    let enc_rec = engine4.recovery_status().unwrap();
    assert!(enc_rec.ephemeral_key);
    assert!(!enc_rec.restorable);
    assert!(engine4.recovery_restore(None).await.is_err());
    engine4.recovery_discard(true).unwrap();
    assert!(!enc_temp_path.exists());
    engine4.shutdown().await;
}
