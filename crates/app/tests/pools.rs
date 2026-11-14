//! Resource pools end to end (M26): a pool shared by tools bounds their
//! requests in flight and backs off when the server answers 429.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{Engine, PoolConfig, PoolsConfig};
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{NewRepeaterTab, RepeaterTabUpdate, RepeaterTarget};
use std::time::{Duration, Instant};

#[tokio::test]
async fn a_shared_pool_bounds_concurrency_and_backs_off() {
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
    let send = async |path: &str| {
        let tab = engine
            .repeater_create(NewRepeaterTab {
                name: String::new(),
                target: target.clone(),
                request: format!("GET {path} HTTP/1.1\nHost: lab\n\n"),
                head_crlf: true,
                body_crlf: true,
            })
            .await
            .unwrap();
        engine
            .repeater_send(
                tab.id,
                RepeaterTabUpdate {
                    name: tab.name.clone(),
                    target: target.clone(),
                    request: tab.request.clone(),
                    update_content_length: true,
                    reuse_connection: false,
                    use_cookie_jar: false,
                },
            )
            .await
            .unwrap()
    };

    // Without a pool, two slow requests overlap.
    let start = Instant::now();
    let (a, b) = tokio::join!(send("/slow?ms=300"), send("/slow?ms=300"));
    assert_eq!((a.send.status, b.send.status), (Some(200), Some(200)));
    // Compared below with the pooled run: an absolute limit here would
    // fail on a busy machine.
    let free = start.elapsed();

    // An unknown pool is refused.
    let mut config = PoolsConfig::default();
    config
        .assignments
        .insert("repeater".into(), "One at a time".into());
    assert!(engine.save_resource_pools(config.clone()).await.is_err());

    config.pools.push(PoolConfig {
        name: "One at a time".into(),
        concurrency: 1,
        ..PoolConfig::default()
    });
    let view = engine.save_resource_pools(config).await.unwrap();
    assert_eq!(view.pools[0].tools, vec!["repeater".to_string()]);
    assert!(view.tools.iter().any(|t| t.id == "fuzzer"));

    // With it, they take turns.
    let start = Instant::now();
    let (a, b) = tokio::join!(send("/slow?ms=300"), send("/slow?ms=300"));
    assert_eq!((a.send.status, b.send.status), (Some(200), Some(200)));
    let pooled = start.elapsed();
    assert!(pooled >= Duration::from_millis(590), "{pooled:?}");
    assert!(
        pooled > free + Duration::from_millis(150),
        "taking turns ({pooled:?}) is slower than overlapping ({free:?})"
    );

    // A 429 with Retry-After pauses the pool, and the history says so.
    let busy = send("/busy?s=1").await;
    assert_eq!(busy.send.status, Some(429));
    let notes = busy.detail.row.notes.clone().unwrap_or_default();
    assert!(notes.contains("pauses for 1 s"), "{notes}");
    let view = engine.resource_pools().await;
    assert_eq!(view.pools[0].throttled, 1);
    assert!(view.pools[0].paused_ms.is_some());
    let start = Instant::now();
    let after = send("/").await;
    assert_eq!(after.send.status, Some(200));
    assert!(
        start.elapsed() >= Duration::from_millis(800),
        "{:?}",
        start.elapsed()
    );
    let view = engine.resource_pools().await;
    assert_eq!(view.pools[0].sent, 4);
    assert_eq!(view.pools[0].in_flight, 0);

    // Saved: a new engine on the same home reads the pool back.
    drop(engine);
    let again = Engine::new(AppPaths::under(dir.path())).unwrap();
    assert_eq!(again.pools_config().pools.len(), 1);
}

#[tokio::test]
async fn a_running_task_can_be_given_its_own_pool() {
    use chosproxy_crawler::CrawlConfig;
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    // Two pools; the crawler is assigned to none of them.
    let pool = |name: &str| PoolConfig {
        name: name.into(),
        concurrency: 1,
        ..PoolConfig::default()
    };
    engine
        .save_resource_pools(PoolsConfig {
            pools: vec![pool("careful"), pool("other")],
            assignments: Default::default(),
        })
        .await
        .unwrap();
    // A slow crawl (4 requests a second), so there is time to choose.
    let task = engine
        .crawl_start(CrawlConfig {
            start_urls: vec![format!("{}/", lab.url())],
            requests_per_second: 4,
            concurrency: 2,
            ..CrawlConfig::default()
        })
        .await
        .unwrap();
    assert!(
        engine
            .task_set_pool(task.id, Some("nope".into()))
            .await
            .is_err()
    );
    engine
        .task_set_pool(task.id, Some(" Careful ".into()))
        .await
        .unwrap();
    assert_eq!(
        engine.task_detail(task.id).await.unwrap().pool.as_deref(),
        Some("Careful")
    );
    let sent = |view: &chosproxy_app::PoolsView, name: &str| {
        view.pools
            .iter()
            .find(|p| p.config.name == name)
            .unwrap()
            .sent
    };
    let mut through = 0;
    for _ in 0..200 {
        through = sent(&engine.resource_pools().await, "careful");
        if through >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(through >= 3, "the task's requests went through its pool");
    assert_eq!(sent(&engine.resource_pools().await, "other"), 0);
    // Back to the tool's assignment (none): the pool is left alone.
    engine.task_set_pool(task.id, None).await.unwrap();
    assert_eq!(engine.task_detail(task.id).await.unwrap().pool, None);
    tokio::time::sleep(Duration::from_millis(400)).await;
    let before = sent(&engine.resource_pools().await, "careful");
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(sent(&engine.resource_pools().await, "careful"), before);
    let log = engine.task_detail(task.id).await.unwrap().logs;
    assert!(log.iter().any(|l| {
        l.message
            .contains("Resource pool chosen for this task: Careful")
    }));
    // A task that is not running has no pool to choose.
    engine.task_cancel(task.id).unwrap();
    for _ in 0..200 {
        if engine
            .task_detail(task.id)
            .await
            .unwrap()
            .row
            .status
            .is_finished()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        engine
            .task_set_pool(task.id, Some("careful".into()))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_pool_chosen_at_launch_carries_the_first_requests() {
    use chosproxy_crawler::CrawlConfig;
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    engine
        .save_resource_pools(PoolsConfig {
            pools: vec![PoolConfig {
                name: "launch".into(),
                concurrency: 2,
                ..PoolConfig::default()
            }],
            assignments: Default::default(),
        })
        .await
        .unwrap();
    let crawl = || CrawlConfig {
        start_urls: vec![format!("{}/", lab.url())],
        max_requests: 3,
        requests_per_second: 0,
        ..CrawlConfig::default()
    };
    // An unknown pool is refused before anything starts.
    assert!(
        engine
            .with_launch_pool(Some("nope".into()), engine.crawl_start(crawl()))
            .await
            .is_err()
    );
    assert!(engine.tasks().await.unwrap().is_empty());

    let task = engine
        .with_launch_pool(Some(" Launch ".into()), engine.crawl_start(crawl()))
        .await
        .unwrap()
        .unwrap();
    let mut done = None;
    for _ in 0..400 {
        let d = engine.task_detail(task.id).await.unwrap();
        if d.row.status.is_finished() {
            done = Some(d);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let done = done.expect("the crawl finished");
    let sent = engine
        .resource_pools()
        .await
        .pools
        .iter()
        .find(|p| p.config.name == "launch")
        .unwrap()
        .sent;
    assert_eq!(
        sent, done.row.requests,
        "every request went through the pool"
    );
    assert!(sent >= 1);
    assert!(done.logs.iter().any(|l| {
        l.message
            .contains("Resource pool chosen for this task: Launch")
    }));
}
