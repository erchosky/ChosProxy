//! Browser-powered crawling end to end (M24): the lab browser renders a
//! page whose links only exist after a script runs, and the crawl finds
//! and opens them through the proxy. Skipped when no Chromium-family
//! browser is installed.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::browser::{BrowserKind, detect};
use chosproxy_app::{BrowserCrawlConfig, Engine};
use chosproxy_core::config::ProxyConfig;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{HistoryQuery, TaskStatus};
use std::time::Duration;

#[tokio::test]
async fn the_browser_crawl_finds_links_made_by_scripts() {
    let Some(browser) = detect()
        .into_iter()
        .find(|b| b.kind == BrowserKind::Chromium)
    else {
        eprintln!("skipped: no Chromium-family browser installed");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    let mut cfg = ProxyConfig::default();
    cfg.listeners[0].port = 0;
    engine.save_proxy_config(cfg).await.unwrap();
    engine.new_temporary_project().await.unwrap();

    let task = engine
        .browser_crawl_start(BrowserCrawlConfig {
            start_url: format!("{}/js-links", lab.url()),
            max_pages: 10,
            page_timeout_secs: 20,
            browser: Some(browser.id),
        })
        .await
        .unwrap();
    let mut done = None;
    for _ in 0..600 {
        let detail = engine.task_detail(task.id).await.unwrap();
        if matches!(
            detail.row.status,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        ) {
            done = Some(detail);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let detail = done.expect("the crawl finished");
    assert_eq!(detail.row.status, TaskStatus::Completed, "{detail:?}");

    // The script-made pages were opened through the proxy.
    let rows = engine
        .history_page(HistoryQuery {
            limit: 200,
            ..Default::default()
        })
        .await
        .unwrap()
        .rows;
    let paths: Vec<&str> = rows.iter().map(|r| r.path.as_str()).collect();
    for p in ["/js-links", "/js-only", "/js-only-2", "/json"] {
        assert!(paths.contains(&p), "{p} not visited: {paths:?}");
    }
    engine.close_project().await.unwrap();
}
