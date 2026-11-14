//! Content discovery end to end against the lab server.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{DiscoveryConfig, Engine};
use chosproxy_core::Source;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{HistoryQuery, TaskRow, TaskStatus};
use std::time::Duration;

async fn wait_finished(engine: &Engine, id: i64) -> TaskRow {
    for _ in 0..400 {
        let row = engine.task_detail(id).await.unwrap().row;
        if row.status.is_finished() {
            return row;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("task {id} did not finish");
}

#[tokio::test]
async fn finds_unlinked_content_and_ignores_soft_404() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    let base = format!("http://{}/", lab.addr());
    let config = DiscoveryConfig {
        base_url: base.clone(),
        builtin_words: false,
        site_words: false,
        well_known_files: false,
        custom_words: ["admin", "backup", "panel", "soft", "nothing"]
            .map(String::from)
            .to_vec(),
        extensions: vec!["zip".into()],
        recursive: true,
        max_depth: 1,
        requests_per_second: 0,
        ..DiscoveryConfig::default()
    };
    let task = engine.discovery_start(config.clone()).await.unwrap();
    assert_eq!(task.kind, "discovery");
    let done = wait_finished(&engine, task.id).await;
    assert_eq!(done.status, TaskStatus::Completed, "{:?}", done.last_error);
    let mut found: Vec<String> = engine
        .discovery_results(task.id)
        .await
        .unwrap()
        .into_iter()
        .map(|h| format!("{} {}", h.kind, h.url.trim_start_matches(&base)))
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            "file backup.zip",
            "folder admin/",
            "folder soft/",
            "folder soft/panel/"
        ],
        "the soft-404 pages under /soft/ are not hits"
    );
    // Only hits are stored, as discovery traffic.
    let history = engine
        .history_page(HistoryQuery {
            limit: 100,
            sources: vec![Source::Discovery],
            ..HistoryQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(history.total, 4);
    assert!(done.requests > 4);

    // A scope that leaves the lab out: refused before any task.
    let mut scope = engine.scope_config();
    scope.include.push(chosproxy_core::scope::ScopeRule {
        host: "elsewhere.test".into(),
        ..Default::default()
    });
    engine.save_scope_config(scope).unwrap();
    let err = engine.discovery_start(config).await.unwrap_err();
    assert!(err.to_string().contains("not in scope"), "{err}");
}

#[tokio::test]
async fn scheduled_discovery_starts_when_due() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    let config = DiscoveryConfig {
        base_url: format!("http://{}/", lab.addr()),
        builtin_words: false,
        site_words: false,
        well_known_files: false,
        custom_words: vec!["admin".into()],
        extensions: vec![],
        requests_per_second: 0,
        ..DiscoveryConfig::default()
    };
    let now = chosproxy_core::time::now_ms();
    let s = engine
        .schedule_create(chosproxy_storage::NewSchedule {
            kind: "discovery".into(),
            name: "check admin".into(),
            config: serde_json::to_string(&config).unwrap(),
            next_run: now + 60_000,
            repeat_minutes: 0,
        })
        .await
        .unwrap();
    assert_eq!(engine.run_due_schedules(now).await, 0, "not due yet");
    assert_eq!(engine.run_due_schedules(now + 61_000).await, 1);
    let after = engine.schedules().await.unwrap();
    assert!(!after[0].enabled, "a one-off schedule turns itself off");
    let task = after[0].last_task_id.unwrap();
    assert_eq!(
        wait_finished(&engine, task).await.status,
        TaskStatus::Completed
    );
    assert_eq!(engine.run_due_schedules(now + 120_000).await, 0);
    engine.schedule_delete(s.id).await.unwrap();

    // Invalid settings are refused when scheduling, not when due.
    assert!(
        engine
            .schedule_create(chosproxy_storage::NewSchedule {
                kind: "discovery".into(),
                name: "bad".into(),
                config: "{\"baseUrl\":\"ftp://x/\"}".into(),
                next_run: now,
                repeat_minutes: 0,
            })
            .await
            .is_err()
    );
}
