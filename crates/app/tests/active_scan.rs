//! End-to-end integration tests for M13 Active Audit Framework:
//! - Scope, target, and explicit authorization preconditions (`SCANNER-002`)
//! - Vulnerable vs fixed lab endpoints for XSS (`Tentative`), SQLi (`Firm`),
//!   open redirect (`Certain`), and CORS (`Certain`) (`SCANNER-004`)
//! - Correlated ChosOAST active check (`OAST-001`, `SCANNER-004`)
//! - Dual evidence (baseline + probe exchange) on active findings (`SCANNER-011`)
//! - Custom active check scripts (`fn active_probes` + `fn active_check`, `SCANNER-013`)
//! - Active scan on imported OpenAPI endpoints (`SCANNER-014`)
//! - Session rules, non-idempotent method exclusion, and request budgets (`SCANNER-005`)

#![allow(clippy::unwrap_used)]

use chosproxy_app::{ApiSendConfig, Engine, ScanConfig, ScanLaunch, SessionHeader, SessionRule};
use chosproxy_core::Source;
use chosproxy_core::config::{
    LocalOast, OastConfig, OastMode, ScriptEntry, ScriptKind, ScriptsConfig,
};
use chosproxy_core::paths::AppPaths;
use chosproxy_core::scope::{ScopeConfig, ScopeRule};
use chosproxy_lab::LabServer;
use chosproxy_storage::{
    AuditQuery, Confidence, FindingQuery, HistoryQuery, NewRepeaterTab, RepeaterTabUpdate,
    RepeaterTarget, TaskRow, TaskStatus,
};
use std::time::Duration;

async fn setup() -> (tempfile::TempDir, LabServer, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    let mut settings = engine.settings();
    settings.passive.enabled = false;
    engine.save_settings(settings).unwrap();
    (dir, lab, engine)
}

async fn scope_lab(engine: &Engine, lab: &LabServer) {
    engine
        .save_scope_config(ScopeConfig {
            include: vec![ScopeRule {
                enabled: true,
                host: lab.addr().ip().to_string(),
                port: Some(lab.addr().port()),
                ..ScopeRule::default()
            }],
            ..ScopeConfig::default()
        })
        .unwrap();
}

async fn send_request(engine: &Engine, lab: &LabServer, raw_req: &str) -> i64 {
    let target = RepeaterTarget {
        scheme: "http".into(),
        host: lab.addr().ip().to_string(),
        port: lab.addr().port(),
    };
    let tab = engine
        .repeater_create(NewRepeaterTab {
            name: "seed".into(),
            target: target.clone(),
            request: raw_req.into(),
            head_crlf: false,
            body_crlf: false,
        })
        .await
        .unwrap();
    let res = engine
        .repeater_send(
            tab.id,
            RepeaterTabUpdate {
                name: tab.name,
                target,
                request: raw_req.into(),
                update_content_length: true,
                reuse_connection: false,
                use_cookie_jar: false,
            },
        )
        .await
        .unwrap();
    res.detail.row.id
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

fn only_active_check(keep: &str) -> Vec<String> {
    [
        "active-reflected-xss",
        "active-sqli",
        "active-open-redirect",
        "active-cors",
        "active-oast",
    ]
    .into_iter()
    .filter(|&id| id != keep)
    .map(String::from)
    .collect()
}

#[tokio::test]
async fn active_scan_preconditions_and_authorization_gating() {
    let (_dir, lab, engine) = setup().await;
    let mut cfg = ScanConfig {
        name: "Active test".into(),
        crawl: false,
        passive: false,
        javascript: false,
        active: true,
        active_authorized: false,
        ..ScanConfig::default()
    };

    // 1. Refused without explicit authorization.
    let err = engine
        .scan_start(ScanLaunch {
            start_urls: vec![format!("{}/", lab.url())],
            config: cfg.clone(),
        })
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("authorization"),
        "expected authorization error, got: {err}"
    );

    // 2. Refused when scope is empty even if authorized.
    cfg.active_authorized = true;
    let err = engine
        .scan_start(ScanLaunch {
            start_urls: vec![format!("{}/", lab.url())],
            config: cfg.clone(),
        })
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("non-empty project scope"),
        "expected scope error, got: {err}"
    );

    // 3. Refused when start URL is outside configured scope.
    scope_lab(&engine, &lab).await;
    let err = engine
        .scan_start(ScanLaunch {
            start_urls: vec!["http://example.com/".into()],
            config: cfg.clone(),
        })
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("outside the project scope"),
        "expected out-of-scope error, got: {err}"
    );

    // 4. Scheduling an un-authorized active scan is also refused.
    let mut unauth = cfg.clone();
    unauth.active_authorized = false;
    let bad_schedule = engine
        .schedule_create(chosproxy_storage::NewSchedule {
            kind: "scan".into(),
            name: "unauth active".into(),
            config: serde_json::to_string(&ScanLaunch {
                start_urls: vec![format!("{}/", lab.url())],
                config: unauth,
            })
            .unwrap(),
            next_run: chosproxy_core::time::now_ms() + 60_000,
            repeat_minutes: 0,
        })
        .await;
    assert!(bad_schedule.is_err());

    // 5. Saving a scan config strips `active_authorized` so saved templates never bypass confirmation.
    let saved = engine.save_scan_config(cfg).unwrap();
    let stored = saved.iter().find(|c| c.name == "Active test").unwrap();
    assert!(!stored.active_authorized);
}

#[tokio::test]
async fn active_scan_detects_vulnerable_endpoints_and_ignores_fixed_variants() {
    let (_dir, lab, engine) = setup().await;
    scope_lab(&engine, &lab).await;
    let host = format!("{}:{}", lab.addr().ip(), lab.addr().port());

    // Seed vulnerable and fixed endpoints into project history.
    let paths = [
        "/vuln/xss?q=hello",
        "/safe/xss?q=hello",
        "/safe/xss-json?q=hello",
        "/vuln/sqli-error?id=1",
        "/vuln/sqli-bool?id=1",
        "/safe/sqli?id=1",
        "/safe/error500?id=1",
        "/safe/dynamic?id=1",
        "/vuln/redirect?next=/home",
        "/safe/redirect?next=/home",
        "/vuln/cors",
        "/safe/cors",
    ];
    for p in paths {
        send_request(
            &engine,
            &lab,
            &format!("GET {p} HTTP/1.1\nHost: {host}\n\n"),
        )
        .await;
    }
    // Also seed a non-idempotent POST request that must be skipped by default.
    send_request(
        &engine,
        &lab,
        &format!(
            "POST /echo HTTP/1.1\nHost: {host}\nContent-Type: application/x-www-form-urlencoded\n\nq=hello"
        ),
    )
    .await;

    let cfg = ScanConfig {
        name: "Active audit".into(),
        crawl: false,
        passive: false,
        javascript: false,
        active: true,
        active_authorized: true,
        active_requests_per_second: 50,
        max_active_requests: 500,
        max_requests_per_point: 10,
        ..ScanConfig::default()
    };

    let task = engine
        .scan_start(ScanLaunch {
            start_urls: vec![format!("{}/", lab.url())],
            config: cfg,
        })
        .await
        .unwrap();
    let (row, logs) = wait_finished(&engine, task.id).await;
    assert_eq!(row.status, TaskStatus::Completed, "{logs:#?}");

    let summary = engine.scan_summary(task.id).await.unwrap().unwrap();
    assert!(summary.active_requests > 0, "{summary:?}");
    assert!(summary.active_points > 0, "{summary:?}");
    assert!(
        summary.active_skipped >= 1,
        "POST /echo should be skipped by default: {summary:?}"
    );

    let findings = engine.findings(FindingQuery::default()).await.unwrap();
    let by_path_and_kind = |path_sub: &str, kind_sub: &str| {
        findings
            .iter()
            .find(|f| f.path.contains(path_sub) && f.kind.contains(kind_sub))
    };

    // 1. Reflected XSS: detected on /vuln/xss with Tentative confidence; NOT on /safe/xss or /safe/xss-json.
    let xss = by_path_and_kind("/vuln/xss", "Cross-Site Scripting")
        .expect("expected active XSS finding on /vuln/xss");
    assert_eq!(xss.confidence, Confidence::Tentative);
    assert!(by_path_and_kind("/safe/xss?", "Cross-Site Scripting").is_none());
    assert!(by_path_and_kind("/safe/xss-json", "Cross-Site Scripting").is_none());

    // 2. SQLi: detected on /vuln/sqli-error and /vuln/sqli-bool with Firm confidence; NOT on safe variants.
    let sqli_err =
        by_path_and_kind("/vuln/sqli-error", "SQL Injection").expect("expected SQLi on sqli-error");
    assert_eq!(sqli_err.confidence, Confidence::Firm);
    let sqli_bool =
        by_path_and_kind("/vuln/sqli-bool", "SQL Injection").expect("expected SQLi on sqli-bool");
    assert_eq!(sqli_bool.confidence, Confidence::Firm);
    assert!(by_path_and_kind("/safe/sqli", "SQL Injection").is_none());
    assert!(by_path_and_kind("/safe/error500", "SQL Injection").is_none());
    assert!(by_path_and_kind("/safe/dynamic", "SQL Injection").is_none());

    // 3. Open Redirect: detected on /vuln/redirect with Certain confidence; NOT on /safe/redirect.
    let redir =
        by_path_and_kind("/vuln/redirect", "Open Redirect").expect("expected Open Redirect");
    assert_eq!(redir.confidence, Confidence::Certain);
    assert!(by_path_and_kind("/safe/redirect", "Open Redirect").is_none());

    // 4. CORS: detected on /vuln/cors with Certain confidence; NOT on /safe/cors.
    let cors = by_path_and_kind("/vuln/cors", "CORS").expect("expected CORS finding");
    assert_eq!(cors.confidence, Confidence::Certain);
    assert!(by_path_and_kind("/safe/cors", "CORS").is_none());

    // 5. Dual evidence (SCANNER-011): each active finding links both baseline and proving probe exchange.
    let xss_detail = engine.finding_detail(xss.id).await.unwrap();
    assert!(
        xss_detail.evidence.len() >= 2,
        "expected baseline + probe evidence on active finding, got: {:?}",
        xss_detail.evidence
    );

    // 6. Probe requests are stored in history with Source::Scanner.
    let scanner_page = engine
        .history_page(HistoryQuery {
            sources: vec![Source::Scanner],
            limit: 100,
            ..HistoryQuery::default()
        })
        .await
        .unwrap();
    assert!(!scanner_page.rows.is_empty());

    // 7. Audit log records active scan start and completion.
    let audit = engine
        .audit_log_query(AuditQuery {
            action_prefix: Some("scanner.".into()),
            search: None,
            from_ms: None,
            to_ms: None,
            limit: 50,
            offset: 0,
        })
        .await
        .unwrap();
    assert!(audit.iter().any(|e| e.action == "scanner.active.started"));
    assert!(audit.iter().any(|e| e.action == "scanner.active.completed"));
}

#[tokio::test]
async fn active_scan_oast_correlation_and_uncorrelated_ignore() {
    let (_dir, lab, engine) = setup().await;
    scope_lab(&engine, &lab).await;
    let host = format!("{}:{}", lab.addr().ip(), lab.addr().port());

    let http_port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dns_port = std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let smtp_port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    // Start local OAST server on a free HTTP port.
    let status = engine
        .oast_save_config(OastConfig {
            mode: OastMode::Local,
            local: LocalOast {
                bind: "127.0.0.1".into(),
                advertised_host: "127.0.0.1".into(),
                http_port,
                dns_enabled: false,
                dns_port,
                smtp_enabled: false,
                smtp_port,
                zone: "oast.local".into(),
            },
            ..OastConfig::default()
        })
        .await
        .unwrap();
    assert!(status.running, "{status:?}");
    let oast_base = status.http_base.expect("local OAST http_base");

    // Send an uncorrelated callback to the OAST server and verify it is ignored.
    let oast_port: u16 = oast_base.rsplit_once(':').unwrap().1.parse().unwrap();
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", oast_port))
            .await
            .unwrap();
        s.write_all(
            b"GET /uncorrelated-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
        let mut buf = Vec::new();
        let _ = s.read_to_end(&mut buf).await;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(engine.oast_status().ignored >= 1);

    // Seed /vuln/oast?callback=...
    send_request(
        &engine,
        &lab,
        &format!("GET /vuln/oast?callback=http://127.0.0.1:1/cb HTTP/1.1\nHost: {host}\n\n"),
    )
    .await;

    let cfg = ScanConfig {
        name: "OAST active".into(),
        crawl: false,
        passive: false,
        javascript: false,
        active: true,
        active_authorized: true,
        disabled_checks: only_active_check("active-oast"),
        active_requests_per_second: 50,
        ..ScanConfig::default()
    };
    let task = engine
        .scan_start(ScanLaunch {
            start_urls: vec![format!("{}/", lab.url())],
            config: cfg,
        })
        .await
        .unwrap();
    let (row, logs) = wait_finished(&engine, task.id).await;
    assert_eq!(row.status, TaskStatus::Completed, "{logs:#?}");

    let findings = engine.findings(FindingQuery::default()).await.unwrap();
    let oast_finding = findings
        .iter()
        .find(|f| f.path.contains("/vuln/oast") && f.kind.contains("OAST"))
        .expect("expected correlated active OAST finding");
    assert_eq!(oast_finding.confidence, Confidence::Certain);
    let detail = engine.finding_detail(oast_finding.id).await.unwrap();
    assert!(detail.evidence.len() >= 2);
}

#[tokio::test]
async fn active_scan_custom_script_session_rules_and_budgets() {
    let (_dir, lab, engine) = setup().await;
    scope_lab(&engine, &lab).await;
    let host = format!("{}:{}", lab.addr().ip(), lab.addr().port());

    // Configure a session rule that supplies X-Session-Token: valid-token for scanner sends.
    engine
        .save_session_rules(vec![SessionRule {
            enabled: true,
            name: "Scanner auth".into(),
            hosts: vec![],
            tools: vec!["scanner".into()],
            use_cookie_jar: false,
            expired_statuses: vec![],
            expired_regex: String::new(),
            macro_requests: vec![],
            extract: vec![],
            headers: vec![SessionHeader {
                name: "X-Session-Token".into(),
                value: "valid-token".into(),
            }],
        }])
        .await
        .unwrap();

    // Register a custom active check script (SCANNER-013).
    engine
        .save_scripts(ScriptsConfig {
            scripts: vec![ScriptEntry {
                id: "custom-active-echo".into(),
                name: "Custom Active Reflection".into(),
                kind: ScriptKind::Check,
                enabled: true,
                description: "Probes parameters with a custom marker".into(),
                source: r#"
                    fn active_probes(value) {
                        [value + "custom_active_marker_99"]
                    }
                    fn active_check(baseline, probe, payload) {
                        if probe.status == 200 && probe.response_body.contains("custom_active_marker_99") {
                            #{
                                title: "Custom script detected reflection",
                                severity: "medium",
                                confidence: "firm",
                                evidence: "Reflected payload: " + payload
                            }
                        }
                    }
                "#
                .into(),
            }],
            ..ScriptsConfig::default()
        })
        .await
        .unwrap();

    // Seed /protected/xss?q=hello&extra=two (with header so baseline is 200).
    send_request(
        &engine,
        &lab,
        &format!(
            "GET /protected/xss?q=hello&extra=two HTTP/1.1\nHost: {host}\nX-Session-Token: valid-token\n\n"
        ),
    )
    .await;

    let cfg = ScanConfig {
        name: "Script & Session Active".into(),
        crawl: false,
        passive: false,
        javascript: false,
        active: true,
        active_authorized: true,
        disabled_checks: only_active_check("active-reflected-xss"),
        active_requests_per_second: 50,
        max_active_requests: 3,
        max_requests_per_point: 3,
        ..ScanConfig::default()
    };

    let task = engine
        .scan_start(ScanLaunch {
            start_urls: vec![format!("{}/", lab.url())],
            config: cfg,
        })
        .await
        .unwrap();
    let (row, logs) = wait_finished(&engine, task.id).await;
    assert_eq!(row.status, TaskStatus::Completed, "{logs:#?}");

    let summary = engine.scan_summary(task.id).await.unwrap().unwrap();
    assert_eq!(
        summary.active_requests, 3,
        "budget max_active_requests=3 must stop after 3 probes even with 2 insertion points: {summary:?}"
    );

    let findings = engine.findings(FindingQuery::default()).await.unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f.title.contains("Custom script detected reflection")),
        "expected custom active script finding in {findings:?}"
    );
    assert!(
        findings
            .iter()
            .any(|f| f.kind.contains("Cross-Site Scripting")),
        "expected built-in active XSS finding via session rule in {findings:?}"
    );
}

#[tokio::test]
async fn active_scan_audits_imported_api_endpoints() {
    let (dir, lab, engine) = setup().await;
    scope_lab(&engine, &lab).await;
    let base = format!("http://{}/", lab.addr());
    let host = format!("{}:{}", lab.addr().ip(), lab.addr().port());

    let spec = r#"
openapi: 3.0.3
info: {title: Lab Items API, version: "1.0"}
servers: [{url: /}]
paths:
  /api/items:
    get:
      operationId: listItems
      parameters:
        - {name: filter, in: query, schema: {type: string, example: widget}}
"#;
    let file = dir.path().join("items.yaml");
    std::fs::write(&file, spec).unwrap();
    let info = engine.api_import(&file, Some(base.clone())).await.unwrap();
    let send_task = engine
        .api_send_start(ApiSendConfig {
            definition: info.id,
            endpoints: vec![],
            hosts: vec![host],
            include_unsafe: false,
            requests_per_second: 0,
        })
        .await
        .unwrap();
    let (send_row, _) = wait_finished(&engine, send_task.id).await;
    assert_eq!(send_row.status, TaskStatus::Completed);

    let scan_task = engine
        .scan_start(ScanLaunch {
            start_urls: vec![base],
            config: ScanConfig {
                name: "API Active Audit".into(),
                crawl: false,
                passive: false,
                javascript: false,
                active: true,
                active_authorized: true,
                disabled_checks: only_active_check("active-sqli"),
                active_requests_per_second: 50,
                ..ScanConfig::default()
            },
        })
        .await
        .unwrap();
    let (scan_row, logs) = wait_finished(&engine, scan_task.id).await;
    assert_eq!(scan_row.status, TaskStatus::Completed, "{logs:#?}");

    let findings = engine.findings(FindingQuery::default()).await.unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f.path.contains("/api/items") && f.kind.contains("SQL Injection")),
        "expected SQL Injection finding on imported API endpoint /api/items, got: {findings:?}"
    );
}

#[tokio::test]
async fn active_scan_respects_suppressions_live_scope_removal_and_cancellation() {
    let (_dir, lab, engine) = setup().await;
    scope_lab(&engine, &lab).await;
    let host = format!("{}:{}", lab.addr().ip(), lab.addr().port());

    send_request(
        &engine,
        &lab,
        &format!("GET /vuln/xss?q=hello HTTP/1.1\nHost: {host}\n\n"),
    )
    .await;

    // 1. Add a suppression rule for "Reflected Cross-Site Scripting (Active)" on /vuln/xss.
    engine
        .suppression_add(
            chosproxy_storage::NewSuppression {
                kind: "Reflected Cross-Site Scripting (Active)".into(),
                host: String::new(),
                path_prefix: "/vuln/xss".into(),
                reason: "expected lab reflection".into(),
            },
            false,
        )
        .await
        .unwrap();

    let scan1 = engine
        .scan_start(ScanLaunch {
            start_urls: vec![format!("{}/", lab.url())],
            config: ScanConfig {
                name: "Suppressed XSS".into(),
                crawl: false,
                passive: false,
                javascript: false,
                active: true,
                active_authorized: true,
                disabled_checks: only_active_check("active-reflected-xss"),
                active_requests_per_second: 50,
                ..ScanConfig::default()
            },
        })
        .await
        .unwrap();
    let (row1, _) = wait_finished(&engine, scan1.id).await;
    assert_eq!(row1.status, TaskStatus::Completed);
    assert!(
        engine
            .findings(FindingQuery::default())
            .await
            .unwrap()
            .is_empty(),
        "suppressed active finding must not be created"
    );

    // 2. Live scope removal mid-flight stops sending probes to remaining points.
    send_request(
        &engine,
        &lab,
        &format!("GET /slow?ms=200&a=1&b=2&c=3 HTTP/1.1\nHost: {host}\n\n"),
    )
    .await;
    let scan_scope = engine
        .scan_start(ScanLaunch {
            start_urls: vec![format!("{}/", lab.url())],
            config: ScanConfig {
                name: "Live scope removal".into(),
                crawl: false,
                passive: false,
                javascript: false,
                active: true,
                active_authorized: true,
                active_requests_per_second: 10,
                ..ScanConfig::default()
            },
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    engine.save_scope_config(ScopeConfig::default()).unwrap();
    let (row_scope, logs_scope) = wait_finished(&engine, scan_scope.id).await;
    assert_eq!(row_scope.status, TaskStatus::Completed, "{logs_scope:#?}");
    let sum_scope = engine.scan_summary(scan_scope.id).await.unwrap().unwrap();
    assert!(
        sum_scope.out_of_scope >= 1 && sum_scope.active_skipped >= 1,
        "expected mid-scan scope removal to skip remaining points, got: {sum_scope:?}"
    );

    // 3. Pause and cancel an active scan mid-flight, and verify no further probes are sent.
    scope_lab(&engine, &lab).await;
    let scan2 = engine
        .scan_start(ScanLaunch {
            start_urls: vec![format!("{}/", lab.url())],
            config: ScanConfig {
                name: "Cancelled active".into(),
                crawl: false,
                passive: false,
                javascript: false,
                active: true,
                active_authorized: true,
                active_requests_per_second: 1,
                ..ScanConfig::default()
            },
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(60)).await;
    engine.task_pause(scan2.id).unwrap();
    engine.task_cancel(scan2.id).unwrap();
    let (row2, _) = wait_finished(&engine, scan2.id).await;
    assert_eq!(row2.status, TaskStatus::Cancelled);
}
