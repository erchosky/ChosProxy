//! Sequencer end to end: capture tokens from the lab and analyze them.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{CaptureConfig, Engine};
use chosproxy_core::Source;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_sequencer::{ObservationLevel, TokenLocation};
use chosproxy_storage::{HistoryQuery, TaskRow, TaskStatus};
use std::time::Duration;

async fn wait_finished(engine: &Engine, id: i64) -> TaskRow {
    for _ in 0..600 {
        let row = engine.task_detail(id).await.unwrap().row;
        if row.status.is_finished() {
            return row;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("task {id} did not finish");
}

#[tokio::test]
async fn captures_and_analyzes_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    let base = CaptureConfig {
        scheme: "http".into(),
        host: lab.addr().ip().to_string(),
        port: lab.addr().port(),
        request: "GET /cookies/set HTTP/1.1\nHost: lab\n\n".into(),
        location: TokenLocation::Cookie {
            name: "session".into(),
        },
        count: 30,
        requests_per_second: 0,
        ..CaptureConfig::default()
    };
    let task = engine.sequencer_start(base.clone()).await.unwrap();
    assert_eq!(task.kind, "sequencer");
    let done = wait_finished(&engine, task.id).await;
    assert_eq!(done.status, TaskStatus::Completed, "{:?}", done.last_error);
    assert_eq!(done.discovered, 30);
    let tokens = engine.sequencer_tokens(task.id).await.unwrap();
    assert_eq!(tokens.len(), 30);
    assert!(tokens.iter().all(|t| t == "lab-session-value"));

    // The lab's fixed cookie is the worst possible token: every one repeats.
    let analysis = engine.sequencer_analysis(task.id).await.unwrap();
    assert_eq!((analysis.samples, analysis.unique), (30, 1));
    assert!(analysis.estimated_entropy_bits < 1.0);
    assert_eq!(analysis.observations[0].level, ObservationLevel::Concern);
    let traffic = engine
        .history_page(HistoryQuery {
            limit: 100,
            sources: vec![Source::Sequencer],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(traffic.total, 30);
    let traffic_rows = traffic.rows.clone();

    // Random and sequential tokens from the lab tell apart.
    let token = TokenLocation::Cookie {
        name: "token".into(),
    };
    let random = engine
        .sequencer_start(CaptureConfig {
            request: "GET /token HTTP/1.1\nHost: lab\n\n".into(),
            location: token.clone(),
            count: 300,
            concurrency: 8,
            ..base.clone()
        })
        .await
        .unwrap();
    wait_finished(&engine, random.id).await;
    let good = engine.sequencer_analysis(random.id).await.unwrap();
    assert_eq!((good.unique, good.encoding.as_str()), (300, "hex"));
    assert!(
        good.estimated_entropy_bits > 100.0,
        "{}",
        good.estimated_entropy_bits
    );
    assert!(!good.sequential);
    assert!(
        good.observations
            .iter()
            .all(|o| o.level != ObservationLevel::Concern),
        "{:?}",
        good.observations
    );
    let counter = engine
        .sequencer_start(CaptureConfig {
            request: "GET /token/sequential HTTP/1.1\nHost: lab\n\n".into(),
            location: token,
            count: 50,
            concurrency: 1,
            ..base.clone()
        })
        .await
        .unwrap();
    wait_finished(&engine, counter.id).await;
    let weak = engine.sequencer_analysis(counter.id).await.unwrap();
    assert!(weak.sequential);
    assert_eq!(weak.observations[0].level, ObservationLevel::Concern);

    // A location that is never found ends the capture with an explanation.
    let missing = engine
        .sequencer_start(CaptureConfig {
            location: TokenLocation::Header {
                name: "X-Nothing".into(),
            },
            count: 4,
            ..base.clone()
        })
        .await
        .unwrap();
    let failed = wait_finished(&engine, missing.id).await;
    assert_eq!(failed.status, TaskStatus::Failed);
    assert!(failed.last_error.unwrap().contains("token location"));

    // Pasted tokens.
    let pasted: Vec<String> = (0..200)
        .map(|i| format!("{:08x}", i * 2_654_435_761u64 % 4_294_967_296))
        .collect();
    let manual = engine.sequencer_analyze(pasted).await.unwrap();
    assert_eq!(manual.encoding, "hex");
    assert_eq!(manual.unique, 200);
    assert!(engine.sequencer_analyze(vec!["one".into()]).await.is_err());

    // Prepared from a stored response: the cookies it sets are suggested.
    engine
        .sequencer_start(CaptureConfig { count: 2, ..base })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let row = traffic_rows[0].clone();
    let prepared = engine.sequencer_prepare(row.id).await.unwrap();
    assert_eq!(
        prepared.config.location,
        TokenLocation::Cookie {
            name: "session".into()
        }
    );
    assert_eq!(prepared.candidates.len(), 2);
    assert!(
        prepared
            .config
            .request
            .starts_with("GET /cookies/set HTTP/1.1")
    );
}
