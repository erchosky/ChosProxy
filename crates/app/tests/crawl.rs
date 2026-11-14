//! Crawler end to end: a crawl of the lab server as a task, with pause,
//! resume, cancel and scope.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{CrawlConfig, Engine};
use chosproxy_core::Source;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{HistoryQuery, TaskRow, TaskStatus};
use std::time::Duration;

async fn setup() -> (tempfile::TempDir, LabServer, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    (dir, lab, engine)
}

async fn wait_for(engine: &Engine, id: i64, done: impl Fn(&TaskRow) -> bool) -> TaskRow {
    for _ in 0..400 {
        let row = engine.task_detail(id).await.unwrap().row;
        if done(&row) {
            return row;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("task {id} did not reach the expected state");
}

#[tokio::test]
async fn crawls_the_lab_within_limits() {
    let (_dir, lab, engine) = setup().await;
    let task = engine
        .crawl_start(CrawlConfig {
            start_urls: vec![format!("{}/", lab.url())],
            max_depth: 2,
            requests_per_second: 0,
            ..CrawlConfig::default()
        })
        .await
        .unwrap();
    assert_eq!(task.kind, "crawl");
    let done = wait_for(&engine, task.id, |t| t.status.is_finished()).await;
    assert_eq!(done.status, TaskStatus::Completed, "{:?}", done.last_error);
    assert!(done.requests >= 8, "{done:?}");
    assert!(done.finished_at.is_some());
    // The crawl graph: every request but the start URLs was found somewhere.
    let graph = engine.crawl_graph(task.id).await.unwrap();
    assert_eq!(graph.nodes.len() as u64, done.requests, "{graph:?}");
    assert!(!graph.truncated);
    let starts: Vec<_> = graph.nodes.iter().filter(|n| n.depth == 0).collect();
    assert!(starts.iter().any(|n| n.reason == "start URL"));
    for node in graph.nodes.iter().filter(|n| n.depth > 0) {
        let edge = graph.edges.iter().find(|e| e.to == node.id);
        let from = edge.map(|e| graph.nodes.iter().find(|n| n.id == e.from).unwrap());
        assert!(
            from.is_some_and(|f| f.depth < node.depth || node.reason == "redirected"),
            "{node:?} has no page it was found on"
        );
    }
    assert!(engine.crawl_graph(9_999).await.is_err());

    let page = engine
        .history_page(HistoryQuery {
            limit: 200,
            sources: vec![Source::Crawler],
            ..Default::default()
        })
        .await
        .unwrap();
    let paths: Vec<&str> = page.rows.iter().map(|r| r.path.as_str()).collect();
    for expected in ["/", "/json", "/headers", "/app.js", "/robots.txt"] {
        assert!(paths.contains(&expected), "{expected} in {paths:?}");
    }
    assert!(
        !paths.iter().any(|p| p.starts_with("/form")),
        "POST forms are never submitted: {paths:?}"
    );
    // Each request says why the crawler made it.
    let note = |path: &str| {
        page.rows
            .iter()
            .find(|r| r.path == path)
            .and_then(|r| r.notes.clone())
            .unwrap_or_default()
    };
    assert_eq!(note("/"), "Crawler: start URL.");
    let index_id = page.rows.iter().find(|r| r.path == "/").unwrap().id;
    assert!(
        note("/app.js").starts_with(&format!("Crawler: a script reference on #{index_id}")),
        "{}",
        note("/app.js")
    );
    assert!(note("/robots.txt").contains("robots.txt"));
    let logs = engine.task_detail(task.id).await.unwrap().logs;
    let text: Vec<&str> = logs.iter().map(|l| l.message.as_str()).collect();
    assert!(text.iter().any(|m| m.contains("POST form")), "{text:?}");
    assert!(
        text.last().unwrap().starts_with("Finished after"),
        "{text:?}"
    );

    // The crawl fed the cookie jar (the lab sets cookies on /cookies/set).
    let jar = engine.cookie_jar().await.unwrap();
    assert!(
        jar.iter()
            .any(|c| c.name == "session" && c.source == "crawler")
    );

    engine.task_delete(task.id).await.unwrap();
    assert!(engine.tasks().await.unwrap().is_empty());
}

#[tokio::test]
async fn pause_resume_cancel_and_scope() {
    let (_dir, lab, engine) = setup().await;
    let slow = CrawlConfig {
        start_urls: vec![format!("{}/", lab.url())],
        requests_per_second: 4,
        concurrency: 1,
        ..CrawlConfig::default()
    };
    let task = engine.crawl_start(slow.clone()).await.unwrap();
    wait_for(&engine, task.id, |t| t.status == TaskStatus::Running).await;
    engine.task_pause(task.id).unwrap();
    let paused = wait_for(&engine, task.id, |t| t.status == TaskStatus::Paused).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let still = engine.task_detail(task.id).await.unwrap().row;
    assert!(
        still.requests <= paused.requests + 1,
        "no new requests while paused"
    );
    assert!(
        engine.task_delete(task.id).await.is_err(),
        "running tasks are kept"
    );

    engine.task_resume(task.id).unwrap();
    wait_for(&engine, task.id, |t| t.status == TaskStatus::Running).await;
    engine.task_cancel(task.id).unwrap();
    let cancelled = wait_for(&engine, task.id, |t| t.status.is_finished()).await;
    assert_eq!(cancelled.status, TaskStatus::Cancelled);
    assert!(engine.task_pause(task.id).is_err(), "no longer running");

    // A scope that leaves out the lab: the crawl refuses to start anywhere.
    let mut scope = engine.scope_config();
    scope.include.push(chosproxy_core::scope::ScopeRule {
        host: "elsewhere.test".into(),
        ..Default::default()
    });
    engine.save_scope_config(scope).unwrap();
    let refused = engine.crawl_start(slow).await.unwrap();
    let failed = wait_for(&engine, refused.id, |t| t.status.is_finished()).await;
    assert_eq!(failed.status, TaskStatus::Failed);
    assert!(failed.last_error.unwrap().contains("in scope"));

    // Invalid settings are refused before a task exists.
    assert!(
        engine
            .crawl_start(CrawlConfig {
                start_urls: vec!["not a url".into()],
                ..CrawlConfig::default()
            })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn closing_the_project_stops_tasks() {
    let (dir, lab, engine) = setup().await;
    let path = dir.path().join("crawl.chosproxy");
    engine.save_project_as(&path).await.unwrap();
    let task = engine
        .crawl_start(CrawlConfig {
            start_urls: vec![format!("{}/", lab.url())],
            requests_per_second: 2,
            ..CrawlConfig::default()
        })
        .await
        .unwrap();
    wait_for(&engine, task.id, |t| t.status == TaskStatus::Running).await;
    engine.close_project().await.unwrap();
    engine.open_project(&path).await.unwrap();
    let row = engine.task_detail(task.id).await.unwrap();
    assert_eq!(row.row.status, TaskStatus::Cancelled);
    assert!(
        row.logs
            .iter()
            .any(|l| l.message.contains("project was closed"))
    );
}
