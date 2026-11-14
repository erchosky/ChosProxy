//! Repeater end to end: tabs in the project, real sends to the lab server,
//! history, redirects, cancellation and Send to Repeater from the history.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{AppError, Engine};
use chosproxy_core::Source;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::model::Highlight;
use chosproxy_storage::{
    GroupSendMode, NewRepeaterTab, RepeaterGroupUpdate, RepeaterTab, RepeaterTabUpdate,
    RepeaterTarget,
};
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

fn lab_target(lab: &LabServer) -> RepeaterTarget {
    RepeaterTarget {
        scheme: "http".into(),
        host: lab.addr().ip().to_string(),
        port: lab.addr().port(),
    }
}

fn update(tab: &RepeaterTab, target: RepeaterTarget, request: &str) -> RepeaterTabUpdate {
    RepeaterTabUpdate {
        name: tab.name.clone(),
        target,
        request: request.into(),
        update_content_length: true,
        reuse_connection: false,
        use_cookie_jar: false,
    }
}

async fn lab_tab(engine: &Engine, lab: &LabServer, request: &str) -> RepeaterTab {
    engine
        .repeater_create(NewRepeaterTab {
            name: String::new(),
            target: lab_target(lab),
            request: request.into(),
            head_crlf: true,
            body_crlf: true,
        })
        .await
        .unwrap()
}

fn body(result: &chosproxy_app::RepeaterSendResult) -> String {
    result
        .detail
        .response
        .as_ref()
        .unwrap()
        .body_text
        .clone()
        .unwrap()
}

#[tokio::test]
async fn send_edit_resend_and_history() {
    let (_dir, lab, engine) = setup().await;
    let tab = engine
        .repeater_create(NewRepeaterTab {
            name: String::new(),
            target: lab_target(&lab),
            request: "GET /json HTTP/1.1\nHost: lab\n\n".into(),
            head_crlf: true,
            body_crlf: true,
        })
        .await
        .unwrap();

    let first = engine
        .repeater_send(tab.id, update(&tab, lab_target(&lab), &tab.request))
        .await
        .unwrap();
    assert_eq!(first.send.status, Some(200));
    assert_eq!(first.detail.row.source, Source::Repeater);
    let body = first
        .detail
        .response
        .as_ref()
        .unwrap()
        .body_text
        .clone()
        .unwrap();
    assert!(body.contains("\"name\":\"lab\""), "{body}");
    assert!(first.send.timing.total_ms >= first.send.timing.wait_ms);

    // Edited request with a body: Content-Length is computed on send.
    let echo = "POST /echo HTTP/1.1\nHost: lab\nContent-Type: text/plain\n\nhello repeater";
    let second = engine
        .repeater_send(tab.id, update(&tab, lab_target(&lab), echo))
        .await
        .unwrap();
    assert_eq!(second.send.status, Some(200));
    assert_eq!(second.tab.request, echo, "the edit is saved with the send");
    let sent = second.detail.request.body_text.clone().unwrap();
    assert_eq!(sent, "hello repeater");
    let echoed = second
        .detail
        .response
        .as_ref()
        .unwrap()
        .body_text
        .clone()
        .unwrap();
    assert!(echoed.contains("hello repeater"), "{echoed}");

    let history = engine.repeater_history(tab.id).await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].method, "POST");
    assert_eq!(history[1].exchange_id, first.send.exchange_id);

    let snapshot = engine
        .repeater_snapshot(first.send.exchange_id)
        .await
        .unwrap();
    assert_eq!(
        snapshot.request_text.as_deref(),
        Some("GET /json HTTP/1.1\nHost: lab\n\n")
    );

    // The tab and its history survive in the project.
    let tabs = engine.repeater_tabs().await.unwrap();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].request, echo);
}

#[tokio::test]
async fn failures_are_recorded_and_redirects_followed() {
    let (_dir, lab, engine) = setup().await;
    let tab = engine
        .repeater_create(NewRepeaterTab {
            name: "redirect".into(),
            target: lab_target(&lab),
            request: "GET /redirect?to=/json HTTP/1.1\nHost: lab\n\n".into(),
            head_crlf: true,
            body_crlf: true,
        })
        .await
        .unwrap();
    let sent = engine
        .repeater_send(tab.id, update(&tab, lab_target(&lab), &tab.request))
        .await
        .unwrap();
    assert_eq!(sent.send.status, Some(302));
    let next = engine
        .repeater_redirect(sent.send.exchange_id)
        .await
        .unwrap();
    assert!(
        next.request.starts_with("GET /json HTTP/1.1\n"),
        "{}",
        next.request
    );
    let followed = engine
        .repeater_send(tab.id, update(&tab, next.target, &next.request))
        .await
        .unwrap();
    assert_eq!(followed.send.status, Some(200));
    assert!(
        engine
            .repeater_redirect(followed.send.exchange_id)
            .await
            .is_err()
    );

    // Nothing listens there: the failure is a history entry with an error.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let refused = engine
        .repeater_send(
            tab.id,
            update(
                &tab,
                RepeaterTarget {
                    scheme: "http".into(),
                    host: "127.0.0.1".into(),
                    port,
                },
                "GET / HTTP/1.1\nHost: x\n\n",
            ),
        )
        .await
        .unwrap();
    assert_eq!(refused.send.status, None);
    assert!(refused.send.error.is_some());
    assert_eq!(engine.repeater_history(tab.id).await.unwrap().len(), 3);
}

#[tokio::test]
async fn cancel_stops_a_slow_send() {
    let (_dir, lab, engine) = setup().await;
    let tab = engine
        .repeater_create(NewRepeaterTab {
            name: String::new(),
            target: lab_target(&lab),
            request: "GET /slow?ms=5000 HTTP/1.1\nHost: lab\n\n".into(),
            head_crlf: true,
            body_crlf: true,
        })
        .await
        .unwrap();
    let sending = {
        let engine = engine.clone();
        let up = update(&tab, lab_target(&lab), &tab.request);
        tokio::spawn(async move { engine.repeater_send(tab.id, up).await })
    };
    let mut cancelled = false;
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if engine.repeater_cancel(tab.id) {
            cancelled = true;
            break;
        }
    }
    assert!(cancelled, "the send never started");
    let result = tokio::time::timeout(Duration::from_secs(2), sending)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(AppError::Cancelled)));
    assert!(engine.repeater_history(tab.id).await.unwrap().is_empty());
    assert!(!engine.repeater_cancel(tab.id));
}

#[tokio::test]
async fn send_to_repeater_from_a_stored_exchange() {
    let (_dir, lab, engine) = setup().await;
    // Make a history entry by sending once, then open it in a new tab.
    let first = engine
        .repeater_create(NewRepeaterTab {
            name: String::new(),
            target: lab_target(&lab),
            request: "POST /echo HTTP/1.1\nHost: lab\nContent-Length: 3\n\nabc".into(),
            head_crlf: true,
            body_crlf: true,
        })
        .await
        .unwrap();
    let sent = engine
        .repeater_send(first.id, update(&first, lab_target(&lab), &first.request))
        .await
        .unwrap();
    let copy = engine
        .repeater_from_exchange(sent.send.exchange_id)
        .await
        .unwrap();
    assert_eq!(copy.name, "2");
    assert_eq!(copy.port, lab.addr().port());
    assert!(copy.request.ends_with("\n\nabc"));
    assert!(matches!(
        engine.repeater_from_exchange(9999).await,
        Err(AppError::UnknownExchange(9999))
    ));

    let dup = engine.repeater_duplicate(copy.id).await.unwrap();
    engine.repeater_move(dup.id, 0).await.unwrap();
    let order: Vec<i64> = engine
        .repeater_tabs()
        .await
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(order, vec![dup.id, first.id, copy.id]);
    engine.repeater_close(first.id).await.unwrap();
    assert_eq!(engine.repeater_tabs().await.unwrap().len(), 2);

    // Without a project nothing works.
    engine.close_project().await.unwrap();
    assert!(matches!(
        engine.repeater_tabs().await,
        Err(AppError::NoProject)
    ));
}

#[tokio::test]
async fn kept_connections_and_the_cookie_jar() {
    let (_dir, lab, engine) = setup().await;
    let tab = lab_tab(&engine, &lab, "GET /cookies/set HTTP/1.1\nHost: lab\n\n").await;
    let keep = |request: &str| RepeaterTabUpdate {
        reuse_connection: true,
        use_cookie_jar: true,
        ..update(&tab, lab_target(&lab), request)
    };

    let set = engine
        .repeater_send(tab.id, keep("GET /cookies/set HTTP/1.1\nHost: lab\n\n"))
        .await
        .unwrap();
    assert!(!set.send.reused && set.send.jar_cookies.is_empty());
    let jar = engine.cookie_jar().await.unwrap();
    let names: Vec<&str> = jar.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["session", "theme"]);
    assert!(jar.iter().all(|c| c.source == "repeater"));

    // The jar replaces the theme written in the request and adds the session.
    let headers = engine
        .repeater_send(
            tab.id,
            keep("GET /headers HTTP/1.1\nHost: lab\nCookie: theme=light\n\n"),
        )
        .await
        .unwrap();
    assert!(headers.send.reused, "the kept connection carried it");
    assert_eq!(headers.send.timing.connect_ms, 0);
    assert_eq!(headers.send.jar_cookies, vec!["theme", "session"]);
    let echoed = body(&headers);
    assert!(
        echoed.contains("theme=dark; session=lab-session-value"),
        "{echoed}"
    );
    assert_eq!(
        headers.tab.request, "GET /headers HTTP/1.1\nHost: lab\nCookie: theme=light\n\n",
        "the editor text is not changed"
    );

    // Edited by hand, deleted, cleared.
    let theme = jar.iter().find(|c| c.name == "theme").unwrap();
    engine
        .cookie_jar_set_value(theme.id, "blue".into())
        .await
        .unwrap();
    let again = engine
        .repeater_send(tab.id, keep("GET /headers HTTP/1.1\nHost: lab\n\n"))
        .await
        .unwrap();
    assert!(body(&again).contains("theme=blue"));
    let session = jar.iter().find(|c| c.name == "session").unwrap();
    assert_eq!(engine.cookie_jar_delete(vec![session.id]).await.unwrap(), 1);
    assert_eq!(engine.cookie_jar_clear().await.unwrap(), 1);

    // Without the options: a new connection, nothing added.
    let plain = engine
        .repeater_send(
            tab.id,
            update(
                &tab,
                lab_target(&lab),
                "GET /headers HTTP/1.1\nHost: lab\n\n",
            ),
        )
        .await
        .unwrap();
    assert!(!plain.send.reused && plain.send.jar_cookies.is_empty());
    assert!(!body(&plain).contains("theme="));
}

#[tokio::test]
async fn group_sends() {
    let (_dir, lab, engine) = setup().await;
    let login = lab_tab(&engine, &lab, "GET /cookies/set HTTP/1.1\nHost: lab\n\n").await;
    let check = lab_tab(&engine, &lab, "GET /headers HTTP/1.1\nHost: lab\n\n").await;
    let json = lab_tab(&engine, &lab, "GET /json HTTP/1.1\nHost: lab\n\n").await;
    engine
        .repeater_update(
            check.id,
            RepeaterTabUpdate {
                use_cookie_jar: true,
                ..update(&check, lab_target(&lab), &check.request)
            },
        )
        .await
        .unwrap();
    let group = engine
        .repeater_group_create(
            "Flow".into(),
            Highlight::Blue,
            vec![login.id, check.id, json.id],
        )
        .await
        .unwrap();
    let set_mode = |mode| RepeaterGroupUpdate {
        name: "Flow".into(),
        color: Highlight::Blue,
        send_mode: mode,
        collapsed: false,
    };

    // Sequence: the cookie set by the first tab is sent by the second.
    let result = engine.repeater_send_group(group.id).await.unwrap();
    assert_eq!(result.mode, GroupSendMode::Sequence);
    let tabs: Vec<i64> = result.items.iter().map(|i| i.tab_id).collect();
    assert_eq!(tabs, vec![login.id, check.id, json.id]);
    let second = result.items[1].send.as_ref().unwrap();
    assert_eq!(second.jar_cookies, vec!["session", "theme"]);
    assert!(
        result
            .items
            .iter()
            .all(|i| i.send.as_ref().unwrap().status == Some(200))
    );

    // Single connection: later requests reuse the first one's connection.
    engine
        .repeater_group_update(group.id, set_mode(GroupSendMode::Single))
        .await
        .unwrap();
    let single = engine.repeater_send_group(group.id).await.unwrap();
    let reused: Vec<bool> = single
        .items
        .iter()
        .map(|i| i.send.as_ref().unwrap().reused)
        .collect();
    assert_eq!(reused, vec![false, true, true]);

    // Parallel: all sent, each on its own connection.
    engine
        .repeater_group_update(group.id, set_mode(GroupSendMode::Parallel))
        .await
        .unwrap();
    let parallel = engine.repeater_send_group(group.id).await.unwrap();
    assert_eq!(parallel.items.len(), 3);
    for item in &parallel.items {
        let send = item.send.as_ref().unwrap();
        assert_eq!(send.status, Some(200));
        assert!(!send.reused);
    }
    assert_eq!(engine.repeater_history(check.id).await.unwrap().len(), 3);

    // A tab that closes the connection stops a single-connection send.
    engine
        .repeater_update(
            login.id,
            update(
                &login,
                lab_target(&lab),
                "GET /json HTTP/1.1\nHost: lab\nConnection: close\n\n",
            ),
        )
        .await
        .unwrap();
    engine
        .repeater_group_update(group.id, set_mode(GroupSendMode::Single))
        .await
        .unwrap();
    let stopped = engine.repeater_send_group(group.id).await.unwrap();
    assert!(stopped.items[0].send.is_some());
    assert!(
        stopped.items[1]
            .problem
            .as_deref()
            .unwrap()
            .contains("closed")
    );

    // Single connection needs one target.
    let mut elsewhere = lab_target(&lab);
    elsewhere.port = 1;
    engine
        .repeater_update(json.id, update(&json, elsewhere, &json.request))
        .await
        .unwrap();
    let refused = engine.repeater_send_group(group.id).await.unwrap_err();
    assert!(matches!(refused, AppError::Invalid(_)), "{refused}");

    engine.repeater_group_delete(group.id).await.unwrap();
    assert!(engine.repeater_groups().await.unwrap().is_empty());
    assert!(
        engine
            .repeater_tabs()
            .await
            .unwrap()
            .iter()
            .all(|t| t.group_id.is_none())
    );
}

#[tokio::test]
async fn http2_tabs_and_single_packet_groups() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    let target = RepeaterTarget {
        scheme: "https".into(),
        host: "127.0.0.1".into(),
        port: lab.addr().port(),
    };
    let new_tab = |request: &str| NewRepeaterTab {
        name: String::new(),
        target: target.clone(),
        request: request.into(),
        head_crlf: true,
        body_crlf: true,
    };

    // One tab: the request line says HTTP/2, so it is sent over HTTP/2,
    // even with "reuse the connection" on.
    let tab = engine
        .repeater_create(new_tab(
            "POST /version HTTP/2\nHost: 127.0.0.1\nConnection: keep-alive\n\nping",
        ))
        .await
        .unwrap();
    let result = engine
        .repeater_send(
            tab.id,
            RepeaterTabUpdate {
                reuse_connection: true,
                ..update(&tab, target.clone(), &tab.request)
            },
        )
        .await
        .unwrap();
    assert_eq!(result.send.status, Some(200));
    let text = body(&result);
    assert!(text.contains(r#""version":"HTTP/2""#), "{text}");
    assert!(text.contains(r#""body":"ping""#), "{text}");
    let row = &result.detail.row;
    assert_eq!(row.http_version, "HTTP/2");
    let notes = row.notes.clone().unwrap_or_default();
    assert!(
        notes.contains("Sent over HTTP/2") && notes.contains("Connection"),
        "{notes}"
    );

    // A parallel group of HTTP/2 tabs: one connection, final frames together.
    let mut ids = Vec::new();
    for i in 1..=3 {
        let t = engine
            .repeater_create(new_tab(&format!(
                "POST /version HTTP/2\nHost: 127.0.0.1\n\nrace{i}"
            )))
            .await
            .unwrap();
        ids.push(t.id);
    }
    let group = engine
        .repeater_group_create("Race".into(), Highlight::Red, ids.clone())
        .await
        .unwrap();
    engine
        .repeater_group_update(
            group.id,
            RepeaterGroupUpdate {
                name: "Race".into(),
                color: Highlight::Red,
                send_mode: GroupSendMode::Parallel,
                collapsed: false,
            },
        )
        .await
        .unwrap();
    let parallel = engine.repeater_send_group(group.id).await.unwrap();
    assert_eq!(parallel.items.len(), 3);
    for item in &parallel.items {
        let send = item.send.as_ref().unwrap();
        assert_eq!(send.status, Some(200), "{item:?}");
    }
    engine.close_project().await.unwrap();
}
