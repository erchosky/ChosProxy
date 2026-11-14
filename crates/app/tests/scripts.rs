//! ChosScripts end to end (M22): a library with every kind of script,
//! real traffic through the proxy, and each script doing its job.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::Engine;
use chosproxy_core::config::{
    MatchReplaceRule, MatchTarget, ProxyConfig, ScriptEntry, ScriptKind, ScriptsConfig,
};
use chosproxy_core::event::ListenerState;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{FindingQuery, HistoryQuery};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn fetch(proxy: SocketAddr, url: &str) -> String {
    let mut s = TcpStream::connect(proxy).await.unwrap();
    s.write_all(format!("GET {url} HTTP/1.1\r\nHost: lab\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).await.unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

fn script(id: &str, kind: ScriptKind, source: &str) -> ScriptEntry {
    ScriptEntry {
        id: id.into(),
        name: format!("Script {id}"),
        kind,
        enabled: true,
        description: String::new(),
        source: source.into(),
    }
}

#[tokio::test]
async fn every_kind_of_script_does_its_job() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    let library = engine
        .save_scripts(ScriptsConfig {
            scripts: vec![
                script(
                    "lab-name",
                    ScriptKind::Check,
                    r#"fn check(msg) {
                        if contains_ci(msg.response_body, "\"name\":\"lab\"") {
                            #{ title: "Lab name in response", severity: "low",
                               confidence: "certain", evidence: msg.path }
                        }
                    }"#,
                ),
                script(
                    "json-only",
                    ScriptKind::Filter,
                    r#"fn filter(msg) { msg.mime == "JSON" }"#,
                ),
                script(
                    "status-path",
                    ScriptKind::Column,
                    r#"fn column(msg) { `${msg.status} ${msg.path}` }"#,
                ),
                script(
                    "describe",
                    ScriptKind::Action,
                    r#"fn action(msg) { msg.method + " " + msg.url }"#,
                ),
                script(
                    "bye",
                    ScriptKind::Rewrite,
                    r#"fn response(text) { text.replace("hello", "bye"); text }"#,
                ),
                script("broken", ScriptKind::Filter, "fn filter(msg) { let = ; }"),
            ],
            ..ScriptsConfig::default()
        })
        .await
        .unwrap();
    assert!(library.problems.contains_key("broken"));
    assert_eq!(library.problems.len(), 1);

    let mut proxy = ProxyConfig::default();
    proxy.listeners[0].port = 0;
    proxy.match_replace = vec![MatchReplaceRule {
        enabled: true,
        target: MatchTarget::ResponseScript,
        pattern: "bye".into(),
        replace: String::new(),
        regex: false,
        comment: String::new(),
    }];
    engine.save_proxy_config(proxy).await.unwrap();
    engine.new_temporary_project().await.unwrap();
    let addr: SocketAddr = match &engine.listeners().await[0].state {
        ListenerState::Running { address } => address.parse().unwrap(),
        other => panic!("listener not running: {other:?}"),
    };

    // Rewrite script on a gzip response: decoded, changed, re-framed.
    let gzip = fetch(addr, &format!("{}/gzip", lab.url())).await;
    assert!(gzip.contains("bye from gzip"), "{gzip}");
    fetch(addr, &format!("{}/json", lab.url())).await;
    fetch(addr, &format!("{}/", lab.url())).await;

    // Wait for the three exchanges.
    let mut rows = Vec::new();
    for _ in 0..300 {
        rows = engine
            .history_page(HistoryQuery {
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap()
            .rows;
        if rows.len() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(rows.len(), 3);
    let json_row = rows.iter().find(|r| r.path == "/json").unwrap();

    // Filter: only JSON rows (the gzip one is JSON too).
    let filtered = engine
        .history_page_scripted(
            HistoryQuery {
                limit: 50,
                ..Default::default()
            },
            "json-only",
        )
        .await
        .unwrap();
    let paths: Vec<&str> = filtered.page.rows.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(filtered.page.total, 2, "{paths:?}");
    assert!(!paths.contains(&"/"));
    assert_eq!(filtered.scanned, 3);
    assert!(
        engine
            .history_page_scripted(HistoryQuery::default(), "broken")
            .await
            .is_err()
    );
    // Conditions from the builder: an inline filter script.
    let built = engine
        .history_page_conditions(
            HistoryQuery {
                limit: 50,
                ..Default::default()
            },
            r#"fn filter(msg) { msg.status == 200 && contains_ci(msg.path, "/JSON") }"#.into(),
        )
        .await
        .unwrap();
    assert_eq!(built.page.total, 1);
    assert_eq!(built.page.rows[0].path, "/json");
    // The same conditions on the site map: only the endpoints of the
    // matching requests.
    let whole = engine.site_map(HistoryQuery::default()).await.unwrap();
    let map = engine
        .site_map_conditions(
            HistoryQuery::default(),
            r#"fn filter(msg) { msg.status == 200 && contains_ci(msg.path, "/JSON") }"#.into(),
        )
        .await
        .unwrap();
    assert!(whole.entries.len() > 1);
    assert_eq!(map.map.entries.len(), 1, "{:?}", map.map.entries);
    let entry = &map.map.entries[0];
    assert_eq!((entry.path.as_str(), entry.count), ("/json", 1));
    assert_eq!(entry.methods, vec!["GET".to_string()]);
    assert_eq!(entry.statuses, vec![200]);
    assert_eq!(map.scanned, 3);
    assert!(!map.capped && map.errors == 0);
    // It is the stored entry of that endpoint.
    let stored = whole.entries.iter().find(|e| e.path == "/json").unwrap();
    assert_eq!(entry, stored);
    assert!(
        engine
            .site_map_conditions(HistoryQuery::default(), "fn filter(".into())
            .await
            .is_err()
    );

    // Column and action.
    let cells = engine
        .script_column("status-path", vec![json_row.id])
        .await
        .unwrap();
    assert_eq!(cells[0].value, "200 /json");
    let action = engine.script_action("describe", json_row.id).await.unwrap();
    assert_eq!(
        action.output.as_deref(),
        Some(format!("GET {}/json", lab.url()).as_str())
    );

    // A draft tested on an exchange, and a draft that does not compile.
    let test = engine
        .script_test(
            ScriptKind::Check,
            r#"fn check(msg) { #{ title: "seen " + msg.path } }"#.into(),
            Some(json_row.id),
        )
        .await
        .unwrap();
    assert_eq!(test.findings[0].title, "seen /json");
    let bad = engine
        .script_test(ScriptKind::Action, "fn action(msg) {".into(), None)
        .await
        .unwrap();
    assert!(bad.problem.is_some());

    // The check script reported a finding during passive analysis.
    let mut found = Vec::new();
    for _ in 0..400 {
        found = engine
            .findings(FindingQuery {
                search: Some("Lab name in response".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        if !found.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(found.len(), 1, "the check script did not report");
    assert_eq!(found[0].kind, "Script lab-name");

    // Sharing: export two scripts, import them back (off, renamed ids).
    let file = dir.path().join("shared.json");
    let n = engine
        .export_scripts(&file, &["json-only".into(), "describe".into()])
        .unwrap();
    assert_eq!(n, 2);
    let after = engine.import_scripts(&file).await.unwrap();
    let imported: Vec<_> = after
        .config
        .scripts
        .iter()
        .filter(|s| s.id.ends_with("-2"))
        .collect();
    assert_eq!(imported.len(), 2);
    assert!(imported.iter().all(|s| !s.enabled));
    std::fs::write(&file, br#"{"format":"other","version":1,"scripts":[]}"#).unwrap();
    assert!(engine.import_scripts(&file).await.is_err());
    engine.close_project().await.unwrap();
}
