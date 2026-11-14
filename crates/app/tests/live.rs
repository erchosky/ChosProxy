//! Background JavaScript and cross-request analysis, and live tasks (M26).

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::Engine;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{FindingQuery, NewRepeaterTab, RepeaterTabUpdate, RepeaterTarget};
use std::time::Duration;

struct Lab {
    engine: Engine,
    target: RepeaterTarget,
    _lab: LabServer,
    _dir: tempfile::TempDir,
}

async fn lab() -> Lab {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    let target = RepeaterTarget {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port: lab.addr().port(),
    };
    Lab {
        engine,
        target,
        _lab: lab,
        _dir: dir,
    }
}

impl Lab {
    async fn send(&self, request: &str) -> i64 {
        let tab = self
            .engine
            .repeater_create(NewRepeaterTab {
                name: String::new(),
                target: self.target.clone(),
                request: request.to_string(),
                head_crlf: true,
                body_crlf: true,
            })
            .await
            .unwrap();
        self.engine
            .repeater_send(
                tab.id,
                RepeaterTabUpdate {
                    name: tab.name.clone(),
                    target: self.target.clone(),
                    request: tab.request.clone(),
                    update_content_length: true,
                    reuse_connection: false,
                    use_cookie_jar: false,
                },
            )
            .await
            .unwrap()
            .detail
            .row
            .id
    }

    /// Waits until the background analyzer has read everything.
    async fn analyzed(&self) {
        for _ in 0..200 {
            let s = self.engine.passive_status().await.unwrap();
            if s.analyzed_up_to == s.last_exchange_id {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the analyzer did not catch up");
    }

    async fn kinds(&self, source: &str) -> Vec<String> {
        self.engine
            .findings(FindingQuery {
                source: Some(source.into()),
                ..Default::default()
            })
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.kind)
            .collect()
    }
}

#[tokio::test]
async fn the_background_analyzer_runs_javascript_and_cross_request_checks() {
    let lab = lab().await;
    lab.send("GET /app.js HTTP/1.1\nHost: lab\n\n").await;
    // A login that keeps the session cookie the browser had, then a page
    // that accepts it.
    let login = lab
        .send(
            "POST /form HTTP/1.1\nHost: lab\nCookie: SESSIONID=fixed-before-login\n\
             Content-Type: application/x-www-form-urlencoded\n\nuser=lab&password=secret",
        )
        .await;
    lab.send("GET / HTTP/1.1\nHost: lab\nCookie: SESSIONID=fixed-before-login\n\n")
        .await;
    lab.analyzed().await;
    let kinds = lab.kinds("passive").await;
    assert!(
        kinds
            .iter()
            .any(|k| k == "JavaScript source map referenced"),
        "{kinds:?}"
    );
    assert!(
        kinds
            .iter()
            .any(|k| k == "Session token not renewed at login"),
        "{kinds:?}"
    );
    let finding = lab
        .engine
        .findings(FindingQuery {
            source: Some("passive".into()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.kind == "Session token not renewed at login")
        .unwrap();
    let detail = lab.engine.finding_detail(finding.id).await.unwrap();
    assert!(
        detail
            .evidence
            .iter()
            .any(|e| e.row.note.contains(&format!("#{login}"))),
        "{:?}",
        detail.evidence
    );
    let checks = lab.engine.passive_checks();
    assert!(checks.iter().any(|c| c.id == "cross-referer-leak"));
    assert!(checks.iter().any(|c| c.id == "js-secrets"));
}

async fn wait_for(mut done: impl AsyncFnMut() -> bool) {
    for _ in 0..600 {
        if done().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out");
}

#[tokio::test]
async fn a_live_audit_follows_new_traffic_with_its_own_checks() {
    use chosproxy_app::{LiveMode, LiveTaskConfig};
    use chosproxy_storage::TaskStatus;

    let lab = lab().await;
    // Only the live task analyzes here.
    let mut settings = lab.engine.settings();
    settings.passive.enabled = false;
    lab.engine.save_settings(settings).unwrap();
    // Traffic from before the task is not read (new traffic only).
    lab.send("GET /app.js HTTP/1.1\nHost: lab\n\n").await;
    let bad = LiveTaskConfig {
        sources: vec!["mail".into()],
        ..LiveTaskConfig::default()
    };
    assert!(lab.engine.live_task_start(bad).await.is_err());
    let task = lab
        .engine
        .live_task_start(LiveTaskConfig {
            mode: LiveMode::Audit,
            sources: vec!["repeater".into()],
            disabled_checks: vec!["js-endpoints".into()],
            ..LiveTaskConfig::default()
        })
        .await
        .unwrap();
    assert!(task.name.starts_with("Live passive audit"));
    wait_for(async || {
        lab.engine.task_detail(task.id).await.unwrap().row.status == TaskStatus::Running
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(lab.kinds("passive").await.is_empty(), "old traffic skipped");

    lab.send("GET /app.js?v=2 HTTP/1.1\nHost: lab\n\n").await;
    wait_for(async || {
        lab.kinds("passive")
            .await
            .iter()
            .any(|k| k == "JavaScript source map referenced")
    })
    .await;
    let kinds = lab.kinds("passive").await;
    assert!(
        !kinds
            .iter()
            .any(|k| k == "Endpoints referenced in JavaScript"),
        "turned off: {kinds:?}"
    );

    // Pause, stop.
    lab.engine.task_pause(task.id).unwrap();
    wait_for(async || {
        lab.engine.task_detail(task.id).await.unwrap().row.status == TaskStatus::Paused
    })
    .await;
    lab.engine.task_cancel(task.id).unwrap();
    wait_for(async || {
        lab.engine.task_detail(task.id).await.unwrap().row.status == TaskStatus::Cancelled
    })
    .await;
    let detail = lab.engine.task_detail(task.id).await.unwrap();
    assert!(detail.row.discovered >= 1);
    assert!(
        detail
            .logs
            .iter()
            .any(|l| l.message.contains("Nothing is sent")),
        "{:?}",
        detail.logs
    );
}

#[tokio::test]
async fn a_live_crawl_lists_unrequested_urls_and_resumes_after_reopening() {
    use chosproxy_app::{LiveMode, LiveTaskConfig};
    use chosproxy_storage::TaskStatus;

    let lab = lab().await;
    let path = lab._dir.path().join("live.chosproxy");
    lab.engine.create_project(&path).await.unwrap();
    let path = std::path::PathBuf::from(lab.engine.project().unwrap().path);
    let task = lab
        .engine
        .live_task_start(LiveTaskConfig {
            mode: LiveMode::Crawl,
            sources: vec!["repeater".into()],
            ..LiveTaskConfig::default()
        })
        .await
        .unwrap();
    lab.send("GET / HTTP/1.1\nHost: lab\n\n").await;
    let port = lab.target.port;
    let status_url = format!("http://127.0.0.1:{port}/status/404");
    // The links of a page are listed one after another: wait for all that
    // is checked below, not just the first of them.
    wait_for(async || {
        let hits = lab.engine.discovery_results(task.id).await.unwrap();
        hits.iter().any(|h| h.url == status_url)
            && hits
                .iter()
                .any(|h| h.kind == "form (POST)" && h.url.ends_with("/form"))
    })
    .await;
    let hits = lab.engine.discovery_results(task.id).await.unwrap();
    assert!(hits.iter().all(|h| h.status == 0));
    assert!(
        hits.iter()
            .any(|h| h.kind == "form (POST)" && h.url.ends_with("/form")),
        "{hits:?}"
    );
    assert!(
        !hits
            .iter()
            .any(|h| h.url == format!("http://127.0.0.1:{port}/"))
    );

    // Closing the project suspends the task; opening it resumes it.
    lab.engine.close_project().await.unwrap();
    lab.engine.open_project(&path).await.unwrap();
    wait_for(async || {
        lab.engine.task_detail(task.id).await.unwrap().row.status == TaskStatus::Running
    })
    .await;
    // A listed URL requested after all is marked with its status.
    lab.send("GET /status/404 HTTP/1.1\nHost: lab\n\n").await;
    wait_for(async || {
        lab.engine
            .discovery_results(task.id)
            .await
            .unwrap()
            .iter()
            .any(|h| h.url == status_url && h.status == 404)
    })
    .await;
    // The log lines are written by the task: give them time to land.
    wait_for(async || {
        let logs = lab.engine.task_detail(task.id).await.unwrap().logs;
        logs.iter().any(|l| l.message.contains("Resumed after"))
    })
    .await;
    let logs = lab.engine.task_detail(task.id).await.unwrap().logs;
    assert!(
        logs.iter().any(|l| l.message.contains("Suspended")),
        "{logs:?}"
    );
    assert!(
        logs.iter().any(|l| l.message.contains("Resumed after")),
        "{logs:?}"
    );
    lab.engine.task_cancel(task.id).unwrap();
}
