//! Fuzzer end to end against the lab server.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{Engine, FuzzConfig};
use chosproxy_core::Source;
use chosproxy_core::paths::AppPaths;
use chosproxy_fuzzer::{AttackMode, FuzzTarget, PayloadSet, PayloadSource};
use chosproxy_lab::LabServer;
use chosproxy_storage::{FuzzResultQuery, FuzzSort, HistoryQuery, TaskRow, TaskStatus};
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

fn target(lab: &LabServer) -> FuzzTarget {
    FuzzTarget {
        scheme: "http".into(),
        host: lab.addr().ip().to_string(),
        port: lab.addr().port(),
    }
}

fn list(items: &[&str]) -> PayloadSet {
    PayloadSet {
        source: PayloadSource::List {
            items: items.iter().map(|s| (*s).to_string()).collect(),
        },
        processing: vec![],
    }
}

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
async fn sniper_attack_with_grep() {
    let (_dir, lab, engine) = setup().await;
    let config = FuzzConfig {
        target: target(&lab),
        template: "POST /echo HTTP/1.1\nHost: lab\nContent-Type: text/plain\n\nhello §world§"
            .into(),
        mode: AttackMode::Sniper,
        payload_sets: vec![list(&["a", "needle", "ccc"])],
        requests_per_second: 0,
        grep_match: vec!["NEEDLE".into()],
        grep_extract: vec![r"hello (\w+)".into()],
        ..FuzzConfig::default()
    };
    let preview = engine.fuzz_preview(&config);
    assert_eq!((preview.positions, preview.requests), (1, 3));
    assert_eq!(preview.samples[1], ["needle"]);

    let task = engine.fuzz_start(config).await.unwrap();
    assert_eq!(task.kind, "fuzz");
    let done = wait_finished(&engine, task.id).await;
    assert_eq!(done.status, TaskStatus::Completed, "{:?}", done.last_error);
    assert_eq!(
        (done.requests, done.discovered),
        (4, 1),
        "baseline + 3, one match"
    );

    let page = engine
        .fuzz_results(
            task.id,
            FuzzResultQuery {
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 4);
    let baseline = &page.rows[0];
    assert_eq!(
        (baseline.idx, baseline.payloads.as_slice()),
        (0, ["world".to_string()].as_slice())
    );
    assert_eq!(baseline.extracts, ["world"]);
    let needle = page.rows.iter().find(|r| r.payloads == ["needle"]).unwrap();
    assert_eq!(needle.status, Some(200));
    assert_eq!(needle.matches, [true], "case-insensitive by default");
    assert_eq!(needle.extracts, ["needle"]);
    // Content-Length followed the payload: the echo saw the whole body.
    let detail = engine
        .exchange_detail(needle.exchange_id.unwrap())
        .await
        .unwrap();
    assert_eq!(detail.row.source, Source::Fuzzer);
    assert_eq!(detail.request.body_text.as_deref(), Some("hello needle"));

    let longest = engine
        .fuzz_results(
            task.id,
            FuzzResultQuery {
                sort: FuzzSort::Length,
                descending: true,
                limit: 1,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(longest.rows[0].payloads, ["needle"]);
    let fuzz_traffic = engine
        .history_page(HistoryQuery {
            limit: 50,
            sources: vec![Source::Fuzzer],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(fuzz_traffic.total, 4);
}

#[tokio::test]
async fn cluster_bomb_status_codes_and_cancel() {
    let (_dir, lab, engine) = setup().await;
    let config = FuzzConfig {
        target: target(&lab),
        template: "GET /status/§200§?x=§1§ HTTP/1.1\nHost: lab\n\n".into(),
        mode: AttackMode::ClusterBomb,
        payload_sets: vec![
            list(&["200", "404"]),
            PayloadSet {
                source: PayloadSource::Numbers {
                    from: 1,
                    to: 2,
                    step: 1,
                    min_digits: 0,
                },
                processing: vec![],
            },
        ],
        requests_per_second: 0,
        ..FuzzConfig::default()
    };
    let task = engine.fuzz_start(config.clone()).await.unwrap();
    wait_finished(&engine, task.id).await;
    let different = engine
        .fuzz_results(
            task.id,
            FuzzResultQuery {
                different_status_only: true,
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        different.total, 2,
        "the two 404 requests differ from the 200 baseline"
    );
    assert!(different.rows.iter().all(|r| r.status == Some(404)));

    // A slow attack stopped early keeps what it sent.
    let slow = FuzzConfig {
        requests_per_second: 2,
        concurrency: 1,
        payload_sets: vec![list(&["200"; 1]), {
            let mut s = list(&[]);
            s.source = PayloadSource::Numbers {
                from: 1,
                to: 50,
                step: 1,
                min_digits: 0,
            };
            s
        }],
        ..config
    };
    let task = engine.fuzz_start(slow).await.unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    engine.task_cancel(task.id).unwrap();
    let stopped = wait_finished(&engine, task.id).await;
    assert_eq!(stopped.status, TaskStatus::Cancelled);
    assert!(stopped.requests < 51);

    // Invalid attacks are refused before a task exists.
    let mut bad = FuzzConfig {
        target: target(&lab),
        template: "GET / HTTP/1.1\n\n".into(),
        payload_sets: vec![list(&["x"])],
        ..FuzzConfig::default()
    };
    assert!(
        engine.fuzz_start(bad.clone()).await.is_err(),
        "no positions"
    );
    bad.template = "GET /§a§ HTTP/1.1\n\n".into();
    bad.max_requests = 0;
    assert!(
        engine
            .fuzz_preview(&bad)
            .error
            .unwrap()
            .contains("maximum requests")
    );
}

#[tokio::test]
async fn fuzzer_and_sequencer_stay_in_scope() {
    let (_dir, lab, engine) = setup().await;
    let config = FuzzConfig {
        target: target(&lab),
        template: "GET /echo?q=§x§ HTTP/1.1\nHost: lab\n\n".into(),
        payload_sets: vec![list(&["a"])],
        ..FuzzConfig::default()
    };
    let capture = chosproxy_sequencer::CaptureConfig {
        scheme: "http".into(),
        host: lab.addr().ip().to_string(),
        port: lab.addr().port(),
        request: "GET /token HTTP/1.1\nHost: lab\n\n".into(),
        location: chosproxy_sequencer::TokenLocation::Cookie {
            name: "session".into(),
        },
        count: 20,
        ..chosproxy_sequencer::CaptureConfig::default()
    };

    // A scope that leaves the lab out: both tools refuse before any task.
    let mut scope = engine.scope_config();
    scope.include.push(chosproxy_core::scope::ScopeRule {
        host: "elsewhere.test".into(),
        ..Default::default()
    });
    engine.save_scope_config(scope.clone()).unwrap();
    let err = engine.fuzz_start(config.clone()).await.unwrap_err();
    assert!(err.to_string().contains("not in scope"), "{err}");
    let err = engine.sequencer_start(capture.clone()).await.unwrap_err();
    assert!(err.to_string().contains("not in scope"), "{err}");
    assert!(engine.tasks().await.unwrap().is_empty());

    // Once the lab is in scope they run.
    scope.include.push(chosproxy_core::scope::ScopeRule {
        host: lab.addr().ip().to_string(),
        ..Default::default()
    });
    engine.save_scope_config(scope).unwrap();
    let task = engine.fuzz_start(config).await.unwrap();
    assert_eq!(
        wait_finished(&engine, task.id).await.status,
        TaskStatus::Completed
    );
    engine.sequencer_start(capture).await.unwrap();
}

#[tokio::test]
async fn auto_mark_uses_insertion_points() {
    use chosproxy_scanner::PointKind;
    let (_dir, _lab, engine) = setup().await;
    let request = "POST /login?next=/home&x HTTP/1.1\nHost: lab\nCookie: sid=abc; theme=dark\n\
                   Content-Type: application/x-www-form-urlencoded\n\nuser=bob&pass=§old§";
    assert_eq!(
        engine.fuzz_auto_mark(request, &[]),
        "POST /login?next=§/home§&x HTTP/1.1\nHost: lab\nCookie: sid=§abc§; theme=§dark§\n\
         Content-Type: application/x-www-form-urlencoded\n\nuser=§bob§&pass=§old§"
    );
    // JSON values are marked too; path segments only when asked for.
    let json = "POST /api/v1 HTTP/1.1\nContent-Type: application/json\n\n{\"a\":1,\"b\":[\"x\"]}";
    assert_eq!(
        engine.fuzz_auto_mark(json, &[]),
        "POST /api/v1 HTTP/1.1\nContent-Type: application/json\n\n{\"a\":§1§,\"b\":[\"§x§\"]}"
    );
    assert_eq!(
        engine.fuzz_auto_mark(json, &[PointKind::PathSegment]),
        "POST /§api§/§v1§ HTTP/1.1\nContent-Type: application/json\n\n{\"a\":1,\"b\":[\"x\"]}"
    );
    // Never more positions than an attack accepts.
    let many = format!("GET /?{} HTTP/1.1\n\n", "a=1&".repeat(50));
    let marked = engine.fuzz_auto_mark(&many, &[]);
    assert_eq!(
        marked.matches('§').count(),
        2 * chosproxy_fuzzer::MAX_POSITIONS
    );
}

#[tokio::test]
async fn stop_conditions_export_and_highlights() {
    use chosproxy_fuzzer::StopConditions;
    use chosproxy_storage::Highlight;

    let (dir, lab, engine) = setup().await;
    let payloads: Vec<String> = (0..20).map(|i| format!("p{i}")).collect();
    let refs: Vec<&str> = payloads.iter().map(String::as_str).collect();
    let config = FuzzConfig {
        target: target(&lab),
        template: "POST /echo HTTP/1.1\nHost: lab\nContent-Type: text/plain\n\nhello §x§".into(),
        payload_sets: vec![list(&refs)],
        concurrency: 1,
        requests_per_second: 0,
        grep_match: vec!["hello".into()],
        stop: StopConditions {
            after_matches: 3,
            ..StopConditions::default()
        },
        ..FuzzConfig::default()
    };
    let task = engine.fuzz_start(config).await.unwrap();
    let done = wait_finished(&engine, task.id).await;
    assert_eq!(done.status, TaskStatus::Completed, "{:?}", done.last_error);
    // One at a time: the baseline, then three matching requests.
    assert_eq!(done.requests, 4, "stopped after three matches");
    let detail = engine.task_detail(task.id).await.unwrap();
    assert!(
        detail
            .logs
            .iter()
            .any(|l| l.message.contains("Stop condition met"))
    );

    // Highlight one result, then export them all.
    let page = engine
        .fuzz_results(
            task.id,
            FuzzResultQuery {
                limit: 10,
                ..FuzzResultQuery::default()
            },
        )
        .await
        .unwrap();
    let second = &page.rows[1];
    let coloured = engine
        .fuzz_set_color(second.id, Some(Highlight::Orange))
        .await
        .unwrap();
    assert_eq!(coloured.color, Some(Highlight::Orange));
    let path = dir.path().join("results.csv");
    engine.fuzz_export_csv(task.id, &path).await.unwrap();
    let csv = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 5, "{csv}");
    assert!(lines[0].starts_with("request,payloads,status"));
    assert!(lines[1].starts_with("baseline,"));
    assert!(lines[2].starts_with("1,p0,200,") && lines[2].ends_with(",yes,orange"));
    assert!(engine.fuzz_export_csv(9_999, &path).await.is_err());
}
