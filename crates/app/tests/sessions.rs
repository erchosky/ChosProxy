//! Session rules end to end (M24): a login macro recorded from the history
//! logs in, extracts a CSRF token into a header, and logs in again when the
//! session expires.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{Engine, SessionExtraction, SessionHeader, SessionRule};
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{HistoryQuery, NewRepeaterTab, RepeaterTabUpdate, RepeaterTarget};

#[tokio::test]
async fn login_macro_keeps_tool_requests_logged_in() {
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
    let send = async |request: &str| {
        let tab = engine
            .repeater_create(NewRepeaterTab {
                name: String::new(),
                target: target.clone(),
                request: request.into(),
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

    // The tester logs in once by hand; those requests become the macro.
    let login = send("POST /session/login HTTP/1.1\nHost: lab\nContent-Type: application/x-www-form-urlencoded\n\nuser={{cred:lab.username}}&pass={{cred:lab.secret}}").await;
    assert_eq!(
        login.send.status,
        Some(401),
        "placeholders are not filled outside rules"
    );
    engine
        .credential_set(
            chosproxy_app::CredentialInfo {
                id: "lab".into(),
                label: "Lab".into(),
                username: "lab".into(),
                has_secret: false,
            },
            Some("secret".into()),
        )
        .await
        .unwrap();
    let token = send("GET /session/token HTTP/1.1\nHost: lab\n\n").await;
    engine.cookie_jar_clear().await.unwrap();

    let rule = SessionRule {
        enabled: true,
        name: "Lab login".into(),
        hosts: vec!["127.0.0.1".into()],
        tools: vec!["repeater".into()],
        use_cookie_jar: true,
        expired_statuses: vec![401, 403],
        expired_regex: String::new(),
        macro_requests: vec![login.detail.row.id, token.detail.row.id],
        extract: vec![SessionExtraction {
            name: "csrf".into(),
            regex: r#"name="csrf" value="([^"]+)""#.into(),
        }],
        headers: vec![SessionHeader {
            name: "X-Csrf".into(),
            value: "{{csrf}}".into(),
        }],
    };
    engine.save_session_rules(vec![rule]).await.unwrap();
    assert_eq!(engine.session_rules().await.unwrap().len(), 1);

    // First send: the variable is missing, so the macro logs in first.
    let api = send("GET /session/api HTTP/1.1\nHost: lab\n\n").await;
    assert_eq!(api.send.status, Some(200));
    let notes = api.detail.row.notes.clone().unwrap_or_default();
    assert!(notes.contains("Logged in with the session rule"), "{notes}");
    assert!(api.detail.request.head.contains("X-Csrf: csrf-123"));
    assert!(api.detail.request.head.contains("labsession=ok-1"));

    // The session expires (cookie gone): 401, log in again, send again.
    engine.cookie_jar_clear().await.unwrap();
    let again = send("GET /session/api HTTP/1.1\nHost: lab\n\n").await;
    assert_eq!(again.send.status, Some(200));
    let notes = again.detail.row.notes.clone().unwrap_or_default();
    assert!(notes.contains("had expired"), "{notes}");

    // Macro requests are in the history, marked.
    let rows = engine
        .history_page(HistoryQuery {
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap()
        .rows;
    let macro_rows = rows
        .iter()
        .filter(|r| {
            r.notes
                .as_deref()
                .is_some_and(|n| n.contains("Login macro"))
        })
        .count();
    assert_eq!(macro_rows, 4, "two logins of two requests each");
    engine.close_project().await.unwrap();
}
