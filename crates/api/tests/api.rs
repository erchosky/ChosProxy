//! The automation API end to end: authentication and request checks, then
//! scope, a send, history, a crawl task and a report against the lab.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_api::ApiServer;
use chosproxy_app::Engine;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// One HTTP/1.1 call; `headers` replace the defaults when they share a name.
async fn call(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> (u16, Value) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let mut head = vec![
        ("Host".to_string(), addr.to_string()),
        ("Authorization".to_string(), format!("Bearer {TOKEN}")),
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Content-Length".to_string(), body.len().to_string()),
        ("Connection".to_string(), "close".to_string()),
    ];
    for (k, v) in headers {
        head.retain(|(n, _)| !n.eq_ignore_ascii_case(k));
        if !v.is_empty() {
            head.push(((*k).to_string(), (*v).to_string()));
        }
    }
    let mut text = format!("{method} {path} HTTP/1.1\r\n");
    for (k, v) in head {
        text.push_str(&format!("{k}: {v}\r\n"));
    }
    text.push_str("\r\n");
    text.push_str(&body);
    stream.write_all(text.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let raw = String::from_utf8_lossy(&raw).into_owned();
    let status: u16 = raw[9..12].parse().unwrap();
    let body = raw.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

async fn setup() -> (tempfile::TempDir, LabServer, Engine, ApiServer) {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    let server = ApiServer::start(engine.clone(), 0, TOKEN.into())
        .await
        .unwrap();
    (dir, lab, engine, server)
}

#[tokio::test]
async fn refuses_requests_without_the_token_or_from_browsers() {
    let (_dir, _lab, _engine, server) = setup().await;
    let addr = server.addr();
    assert!(addr.ip().is_loopback());
    let (s, _) = call(addr, "GET", "/v1/status", None, &[("Authorization", "")]).await;
    assert_eq!(s, 401);
    let (s, _) = call(
        addr,
        "GET",
        "/v1/status",
        None,
        &[("Authorization", "Bearer nope")],
    )
    .await;
    assert_eq!(s, 401);
    let (s, body) = call(
        addr,
        "GET",
        "/v1/status",
        None,
        &[("Origin", "https://evil.test")],
    )
    .await;
    assert_eq!(s, 403, "{body}");
    let (s, _) = call(addr, "GET", "/v1/status", None, &[("Host", "evil.test")]).await;
    assert_eq!(s, 403, "DNS rebinding: a foreign Host is refused");
    let (s, body) = call(addr, "GET", "/v1/status", None, &[]).await;
    assert_eq!(s, 200);
    assert_eq!(body["api"], 1);
    assert_eq!(body["project"], Value::Null);
    let (s, _) = call(addr, "GET", "/v1/nothing", None, &[]).await;
    assert_eq!(s, 404);
    let (s, body) = call(addr, "GET", "/v1/history", None, &[]).await;
    assert_eq!(s, 409, "no project open: {body}");
    let (s, _) = call(addr, "POST", "/v1/scope/add", Some(json!({"nope": 1})), &[]).await;
    assert_eq!(s, 400);
    server.stop().await;
}

#[tokio::test]
async fn drives_the_engine_against_the_lab() {
    let (_dir, lab, engine, server) = setup().await;
    let addr = server.addr();
    let (s, project) = call(addr, "POST", "/v1/project/temporary", None, &[]).await;
    assert_eq!(s, 200, "{project}");
    let lab_url = format!("http://{}/", lab.addr());
    let target =
        json!({"scheme": "http", "host": lab.addr().ip().to_string(), "port": lab.addr().port()});

    // A scope that leaves the lab out refuses sends to it.
    let (s, _) = call(
        addr,
        "POST",
        "/v1/scope/add",
        Some(json!({"url": "http://elsewhere.test/"})),
        &[],
    )
    .await;
    assert_eq!(s, 200);
    let send = json!({"target": target, "request": "GET /json HTTP/1.1\nHost: lab\n\n"});
    let (s, body) = call(addr, "POST", "/v1/send", Some(send.clone()), &[]).await;
    assert_eq!(s, 400, "{body}");
    assert!(body["error"].as_str().unwrap().contains("not in scope"));

    let (s, _) = call(
        addr,
        "POST",
        "/v1/scope/add",
        Some(json!({"url": lab_url})),
        &[],
    )
    .await;
    assert_eq!(s, 200);
    let (s, sent) = call(addr, "POST", "/v1/send", Some(send), &[]).await;
    assert_eq!(s, 200, "{sent}");
    assert_eq!(sent["status"], 200);
    let id = sent["exchangeId"].as_i64().unwrap();
    let (s, detail) = call(addr, "GET", &format!("/v1/history/{id}"), None, &[]).await;
    assert_eq!(s, 200);
    assert_eq!(detail["row"]["path"], "/json");
    let (_, page) = call(addr, "GET", "/v1/history?limit=5&search=json", None, &[]).await;
    assert_eq!(page["total"], 1);

    // A crawl as a task, followed to its end.
    let (s, task) = call(
        addr,
        "POST",
        "/v1/crawl",
        Some(json!({"startUrls": [lab_url], "maxDepth": 1, "requestsPerSecond": 0})),
        &[],
    )
    .await;
    assert_eq!(s, 200, "{task}");
    let task_id = task["id"].as_i64().unwrap();
    let mut status = Value::Null;
    for _ in 0..400 {
        let (_, detail) = call(addr, "GET", &format!("/v1/tasks/{task_id}"), None, &[]).await;
        status = detail["row"]["status"].clone();
        if status != "running" && status != "queued" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(status, "completed");
    let (_, configs) = call(addr, "GET", "/v1/scan/configs", None, &[]).await;
    assert!(configs.as_array().unwrap().len() >= 4);

    // A report of the findings, as Markdown.
    let (s, report) = call(
        addr,
        "POST",
        "/v1/report",
        Some(json!({"format": "markdown"})),
        &[],
    )
    .await;
    assert_eq!(s, 200, "{report}");
    assert!(report["content"].as_str().unwrap().starts_with('#'));

    // Changes made through the API are audited.
    let audit = engine.audit_log(50).await.unwrap();
    assert!(audit.iter().any(|a| a.action == "api.scope"), "{audit:?}");
    assert!(audit.iter().any(|a| a.action == "api.crawl"));
    server.stop().await;
}

#[tokio::test]
async fn describes_itself_and_every_described_route_exists() {
    let (_dir, _lab, _engine, server) = setup().await;
    let addr = server.addr();
    let (status, doc) = call(addr, "GET", "/v1/openapi.json", None, &[]).await;
    assert_eq!(status, 200);
    assert_eq!(doc["openapi"], "3.1.0");
    assert_eq!(
        doc["servers"][0]["url"],
        format!("http://127.0.0.1:{}", addr.port())
    );
    // The token is needed for the description too.
    let (status, _) = call(
        addr,
        "GET",
        "/v1/openapi.json",
        None,
        &[("Authorization", "")],
    )
    .await;
    assert_eq!(status, 401);
    // Every route of the table is answered by the router: whatever it says
    // (no project, bad body…), it is not "no such route".
    for r in chosproxy_api::routes::ROUTES {
        if r.path == "/events" {
            continue;
        }
        let path = format!("/v1{}", r.path).replace("{id}", "1").replace(
            "{action}",
            if r.path.starts_with("/tasks") {
                "pause"
            } else {
                "start"
            },
        );
        let (status, body) = call(addr, r.method, &path, None, &[]).await;
        let message = body["error"].as_str().unwrap_or("");
        assert!(
            !message.starts_with("no ") || !message.contains("/v1/"),
            "{} {path}: {status} {message}",
            r.method
        );
    }
    let (status, body) = call(addr, "GET", "/v1/nothing-here", None, &[]).await;
    assert_eq!(status, 404);
    assert!(body["error"].as_str().unwrap().starts_with("no GET /v1/"));
}

/// Opens `GET /v1/events` and returns the stream after the response head.
async fn open_events(addr: SocketAddr, token: &str) -> (u16, tokio::net::TcpStream, String) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            format!(
                "GET /v1/events HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut seen = String::new();
    let mut buf = [0u8; 4096];
    while !seen.contains("\r\n\r\n") {
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert!(n > 0, "closed before the head: {seen}");
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    let status = seen[9..12].parse().unwrap();
    (status, stream, seen)
}

/// Reads until `needle` shows up (or the stream ends).
async fn read_until(stream: &mut tokio::net::TcpStream, seen: &mut String, needle: &str) -> bool {
    let mut buf = [0u8; 4096];
    while !seen.contains(needle) {
        match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => seen.push_str(&String::from_utf8_lossy(&buf[..n])),
            _ => return false,
        }
    }
    true
}

#[tokio::test]
async fn streams_engine_events_to_clients_with_the_token() {
    let (_dir, _lab, engine, server) = setup().await;
    let addr = server.addr();
    let (status, _, _) = open_events(addr, &"0".repeat(64)).await;
    assert_eq!(status, 401);

    let (status, mut stream, mut seen) = open_events(addr, TOKEN).await;
    assert_eq!(status, 200);
    assert!(
        seen.to_ascii_lowercase().contains("text/event-stream"),
        "{seen}"
    );
    assert!(read_until(&mut stream, &mut seen, ": connected").await);
    engine.new_temporary_project().await.unwrap();
    assert!(
        read_until(&mut stream, &mut seen, "event: projectChanged").await,
        "{seen}"
    );
    assert!(seen.contains("data: {"), "{seen}");

    // A bounded number of streams.
    let mut more = Vec::new();
    for _ in 1..chosproxy_api::MAX_EVENT_STREAMS {
        let (status, s, _) = open_events(addr, TOKEN).await;
        assert_eq!(status, 200);
        more.push(s);
    }
    let (status, _, _) = open_events(addr, TOKEN).await;
    assert_eq!(status, 429);
    // A client that goes away frees its place.
    drop(more.pop());
    let mut freed = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let (status, s, _) = open_events(addr, TOKEN).await;
        if status == 200 {
            more.push(s);
            freed = true;
            break;
        }
    }
    assert!(freed, "a closed stream kept its place");

    // Stopping the server ends the streams.
    server.stop().await;
    let mut buf = [0u8; 4096];
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the stream outlived the server");
}

#[tokio::test]
async fn a_kept_connection_stops_being_served_when_the_api_stops() {
    let (_dir, _lab, _engine, server) = setup().await;
    let addr = server.addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let request =
        format!("GET /v1/status HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {TOKEN}\r\n\r\n");
    // The first request on the connection is answered and it stays open.
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200"));

    server.stop().await;
    // The same connection again: no answer any more.
    let _ = stream.write_all(request.as_bytes()).await;
    let mut answered = false;
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if String::from_utf8_lossy(&buf[..n]).contains("HTTP/1.1 200") {
                        answered = true;
                    }
                }
            }
        }
    })
    .await;
    assert!(
        ended.is_ok(),
        "the connection stayed open after the API stopped"
    );
    assert!(!answered, "a request was served after the API stopped");
}
