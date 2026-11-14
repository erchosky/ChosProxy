//! Scans end to end: the configuration library, and a scan task that
//! crawls the lab and audits each structural pattern once.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{Engine, ScanConfig, ScanLaunch, builtin_scan_configs};
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{FindingQuery, TaskRow, TaskStatus};
use std::time::Duration;

async fn setup() -> (tempfile::TempDir, LabServer, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    // The background analyzer is off, so findings come from the scan.
    let mut settings = engine.settings();
    settings.passive.enabled = false;
    engine.save_settings(settings).unwrap();
    (dir, lab, engine)
}

async fn wait_finished(engine: &Engine, id: i64) -> (TaskRow, Vec<String>) {
    for _ in 0..800 {
        let detail = engine.task_detail(id).await.unwrap();
        if detail.row.status.is_finished() {
            let logs = detail.logs.into_iter().map(|l| l.message).collect();
            return (detail.row, logs);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("task {id} did not finish");
}

fn named(engine: &Engine, name: &str) -> ScanConfig {
    engine
        .scan_configs()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap()
}

#[tokio::test]
async fn library_saves_imports_and_exports_configurations() {
    let (dir, _lab, engine) = setup().await;
    let builtins = builtin_scan_configs();
    assert_eq!(engine.scan_configs().len(), builtins.len());
    for c in &builtins {
        assert!(c.builtin);
        c.validate().unwrap();
        assert!(
            c.crawl_config.requests_per_second > 0,
            "none is unthrottled"
        );
    }

    // Built-in names are reserved; copies under a new name are saved.
    let mut mine = named(&engine, "Crawl and audit (light)");
    assert!(engine.save_scan_config(mine.clone()).is_err());
    mine.name = "  Mine ".into();
    mine.crawl_config.max_requests = 50;
    let all = engine.save_scan_config(mine.clone()).unwrap();
    let saved = all.iter().find(|c| c.name == "Mine").unwrap();
    assert!(!saved.builtin);
    assert_eq!(saved.crawl_config.max_requests, 50);
    // Saving the same name replaces it.
    mine.name = "mine".into();
    mine.max_items = 10;
    let all = engine.save_scan_config(mine).unwrap();
    assert_eq!(all.len(), builtins.len() + 1);
    // Invalid limits are refused.
    let mut bad = named(&engine, "mine");
    assert_eq!(bad.max_items, 10, "replaced, with the new spelling");
    bad.name = "Bad".into();
    bad.crawl_config.concurrency = 0;
    assert!(engine.save_scan_config(bad.clone()).is_err());
    bad.crawl_config.concurrency = 1;
    bad.crawl = false;
    bad.passive = false;
    bad.javascript = false;
    assert!(engine.save_scan_config(bad).is_err(), "no phase");

    // Export and import: names in use get a suffix.
    let file = dir.path().join("scans.json");
    assert_eq!(
        engine
            .export_scan_configs(&file, &["nothing".into()])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.export_scan_configs(&file, &["MINE".into()]).unwrap(),
        1
    );
    let all = engine.import_scan_configs(&file).unwrap();
    assert!(all.iter().any(|c| c.name == "mine (2)"));
    let garbage = dir.path().join("garbage.json");
    std::fs::write(
        &garbage,
        b"{\"format\":\"other\",\"version\":1,\"configs\":[]}",
    )
    .unwrap();
    assert!(engine.import_scan_configs(&garbage).is_err());

    let all = engine.delete_scan_config("MINE (2)").unwrap();
    assert_eq!(all.len(), builtins.len() + 1);
    assert!(
        engine.delete_scan_config("Crawl only").is_err(),
        "built-ins stay"
    );
}

#[tokio::test]
async fn scan_crawls_then_audits_each_pattern_once() {
    let (_dir, lab, engine) = setup().await;
    let mut config = named(&engine, "Crawl and audit (light)");
    config.crawl_config.requests_per_second = 0;
    let task = engine
        .scan_start(ScanLaunch {
            start_urls: vec![format!("{}/", lab.url())],
            config,
        })
        .await
        .unwrap();
    assert_eq!(task.kind, "scan");
    let (row, logs) = wait_finished(&engine, task.id).await;
    assert_eq!(
        row.status,
        TaskStatus::Completed,
        "{:?} {logs:#?}",
        row.last_error
    );
    assert!(row.requests > 0, "the crawl phase sent requests");
    let has = |text: &str| logs.iter().any(|l| l.contains(text));
    assert!(has("Phase: crawl."), "{logs:#?}");
    assert!(has("Phase: audit of the crawled items"), "{logs:#?}");
    assert!(has("Audited #"), "{logs:#?}");
    assert!(has("Audit finished after reading"), "{logs:#?}");
    let findings = engine.findings(FindingQuery::default()).await.unwrap();
    assert!(
        !findings.is_empty(),
        "the passive checks found something on the lab"
    );
    let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
    for kind in [
        "Endpoints referenced in JavaScript",
        "JavaScript source map referenced",
        "DOM sinks in JavaScript",
    ] {
        assert!(kinds.contains(&kind), "{kind} missing from {kinds:?}");
    }

    // The dashboard summary: phases done, counters, issues by severity.
    let summary = engine.scan_summary(task.id).await.unwrap().unwrap();
    assert_eq!(summary.config, "Crawl and audit (light)");
    assert_eq!(summary.phases.len(), 3);
    assert!(
        summary
            .phases
            .iter()
            .all(|p| p.state == chosproxy_app::PhaseState::Done),
        "{summary:?}"
    );
    assert_eq!(summary.crawl_requests, row.requests);
    assert!(summary.audited > 0 && summary.scripts > 0, "{summary:?}");
    assert!(summary.findings_created > 0);
    assert!(
        summary.issues_by_severity.values().sum::<u32>() >= summary.findings_created,
        "{summary:?}"
    );

    // Auditing stored traffic sends nothing and skips repeated patterns.
    let before = engine.task_detail(task.id).await.unwrap().row.requests;
    let audit = engine
        .scan_start(ScanLaunch {
            start_urls: vec![lab.url().to_string()],
            config: named(&engine, "Audit stored traffic"),
        })
        .await
        .unwrap();
    let (row, logs) = wait_finished(&engine, audit.id).await;
    assert_eq!(row.status, TaskStatus::Completed, "{logs:#?}");
    assert_eq!(row.requests, 0);
    assert!(logs.iter().any(|l| l.contains("Audited #")), "{logs:#?}");
    assert_eq!(
        before,
        engine.task_detail(task.id).await.unwrap().row.requests
    );

    // Out-of-origin start URLs find nothing to audit, and say so.
    let none = engine
        .scan_start(ScanLaunch {
            start_urls: vec!["http://127.0.0.1:9/".into()],
            config: named(&engine, "Audit stored traffic"),
        })
        .await
        .unwrap();
    let (_, logs) = wait_finished(&engine, none.id).await;
    assert!(
        logs.iter()
            .any(|l| l.contains("No stored traffic under the start URLs")),
        "{logs:#?}"
    );
}

#[tokio::test]
async fn scans_can_be_scheduled() {
    let (_dir, lab, engine) = setup().await;
    let launch = ScanLaunch {
        start_urls: vec![format!("{}/", lab.url())],
        config: named(&engine, "Audit stored traffic"),
    };
    let now = chosproxy_core::time::now_ms();
    engine
        .schedule_create(chosproxy_storage::NewSchedule {
            kind: "scan".into(),
            name: "nightly audit".into(),
            config: serde_json::to_string(&launch).unwrap(),
            next_run: now + 60_000,
            repeat_minutes: 1440,
        })
        .await
        .unwrap();
    assert_eq!(engine.run_due_schedules(now + 61_000).await, 1);
    let task = engine.schedules().await.unwrap()[0].last_task_id.unwrap();
    let (row, _) = wait_finished(&engine, task).await;
    assert_eq!(row.kind, "scan");
    assert_eq!(row.status, TaskStatus::Completed);
    // Invalid scan settings are refused when scheduling.
    let mut bad = launch;
    bad.start_urls = vec!["not a url".into()];
    assert!(
        engine
            .schedule_create(chosproxy_storage::NewSchedule {
                kind: "scan".into(),
                name: "bad".into(),
                config: serde_json::to_string(&bad).unwrap(),
                next_run: now + 60_000,
                repeat_minutes: 0,
            })
            .await
            .is_err()
    );
}
