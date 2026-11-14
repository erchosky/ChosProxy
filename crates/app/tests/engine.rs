//! Engine-level flow: open a project, capture real traffic through a
//! listener, query it back, close and clean up.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{Engine, ProjectKind};
use chosproxy_core::config::{ProxyConfig, Theme};
use chosproxy_core::event::{EngineEvent, ListenerState};
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::HistoryQuery;
use rustls::pki_types::pem::PemObject;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn engine(root: &std::path::Path) -> Engine {
    Engine::new(AppPaths::under(root)).unwrap()
}

async fn ephemeral_proxy_config(engine: &Engine) {
    let mut cfg = ProxyConfig::default();
    cfg.listeners[0].port = 0;
    engine.save_proxy_config(cfg).await.unwrap();
}

async fn proxy_addr(engine: &Engine) -> SocketAddr {
    match &engine.listeners().await[0].state {
        ListenerState::Running { address } => address.parse().unwrap(),
        other => panic!("listener not running: {other:?}"),
    }
}

async fn fetch_via_proxy(proxy: SocketAddr, url: &str) -> String {
    let mut s = TcpStream::connect(proxy).await.unwrap();
    s.write_all(format!("GET {url} HTTP/1.1\r\nHost: lab\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).await.unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

async fn wait_for_history(engine: &Engine, n: u64) -> chosproxy_storage::HistoryPage {
    for _ in 0..200 {
        let page = engine
            .history_page(HistoryQuery {
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap();
        if page.total >= n {
            return page;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("history never reached {n} rows");
}

#[tokio::test]
async fn temporary_project_captures_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    let mut events = engine.subscribe();
    ephemeral_proxy_config(&engine).await;

    // Listeners need a project.
    assert!(engine.start_listener("default").await.is_err());

    let info = engine.new_temporary_project().await.unwrap();
    assert_eq!(info.kind, ProjectKind::Temporary);
    let proxy = proxy_addr(&engine).await;

    let response = fetch_via_proxy(proxy, &format!("{}/gzip", lab.url())).await;
    assert!(response.starts_with("HTTP/1.1 200"));

    let page = wait_for_history(&engine, 1).await;
    let row = &page.rows[0];
    assert_eq!(row.path, "/gzip");
    let detail = engine.exchange_detail(row.id).await.unwrap();
    let res = detail.response.unwrap();
    assert_eq!(res.decoded_from, vec!["gzip"]);
    assert!(res.body_text.unwrap().contains("hello from gzip"));
    assert!(detail.request.head.starts_with("GET /gzip HTTP/1.1"));

    let stats = engine.dashboard().await.unwrap();
    assert_eq!(stats.project.exchanges, 1);
    assert_eq!(stats.proxy.exchanges, 1);

    // Events were emitted for the listener and the history.
    let mut saw_listener = false;
    let mut saw_history = false;
    while let Ok(ev) = events.try_recv() {
        match ev {
            EngineEvent::ListenerChanged { .. } => saw_listener = true,
            EngineEvent::HistoryAppended { .. } => saw_history = true,
            _ => {}
        }
    }
    assert!(saw_listener && saw_history);

    engine.close_project().await.unwrap();
    assert!(engine.project().is_none());
    assert!(
        !std::path::Path::new(&info.path).exists(),
        "temporary project deleted"
    );
    assert!(TcpStream::connect(proxy).await.is_err(), "listener stopped");
    assert!(
        engine
            .history_page(HistoryQuery {
                limit: 1,
                ..Default::default()
            })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn persistent_project_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    let info = engine
        .create_project(&dir.path().join("lab"))
        .await
        .unwrap();
    assert!(info.path.ends_with("lab.chosproxy"));
    let proxy = proxy_addr(&engine).await;
    fetch_via_proxy(proxy, &format!("{}/json", lab.url())).await;
    wait_for_history(&engine, 1).await;
    engine.close_project().await.unwrap();
    assert!(std::path::Path::new(&info.path).exists());

    // Creating over an existing file is refused; opening works.
    assert!(
        engine
            .create_project(&dir.path().join("lab"))
            .await
            .is_err()
    );
    engine
        .open_project(std::path::Path::new(&info.path))
        .await
        .unwrap();
    assert_eq!(wait_for_history(&engine, 1).await.total, 1);
    engine.shutdown().await;
}

#[tokio::test]
async fn settings_persist_and_invalid_ones_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let mut s = engine.settings();
    s.ui.theme = Theme::Light;
    engine.save_settings(s.clone()).unwrap();
    assert_eq!(
        Engine::new(AppPaths::under(dir.path())).unwrap().settings(),
        s
    );

    let mut bad = ProxyConfig::default();
    bad.listeners[0].bind_address = "0.0.0.0".parse().unwrap();
    assert!(engine.save_proxy_config(bad).await.is_err());
}

#[tokio::test]
async fn changing_a_running_listener_restarts_it() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    let first = proxy_addr(&engine).await;
    let mut cfg = engine.proxy_config().await;
    cfg.listeners[0].bind_address = "::1".parse().unwrap();
    let statuses = engine.save_proxy_config(cfg).await;
    // IPv6 loopback may be unavailable in some CI sandboxes; either outcome
    // must be reported, never silently ignored.
    let state = &statuses.unwrap()[0].state;
    match state {
        ListenerState::Running { address } => assert_ne!(address, &first.to_string()),
        ListenerState::Failed { .. } => {}
        ListenerState::Stopped => panic!("listener left stopped"),
    }
    engine.shutdown().await;
}

#[tokio::test]
async fn https_is_intercepted_and_ca_is_managed() {
    use tokio::io::BufReader;
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start_tls("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    let ca = engine.ca_status();
    assert!(ca.available, "{:?}", ca.error);
    assert!(ca.generated_this_session);
    assert_eq!(ca.download_url, "http://chosproxy/cert");
    assert!(
        engine.launch_browser(None, None).await.is_err(),
        "needs a project"
    );

    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    let proxy = proxy_addr(&engine).await;

    // CONNECT + TLS trusting the engine's CA.
    let mut s = BufReader::new(TcpStream::connect(proxy).await.unwrap());
    let authority = lab.addr().to_string();
    s.get_mut()
        .write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let head = chosproxy_http::head::read_head(&mut s, 4096)
        .await
        .unwrap()
        .unwrap();
    assert!(head.starts_with(b"HTTP/1.1 200"));
    let pem_path = engine
        .export_ca(&dir.path().join("ca"), chosproxy_app::CertFormat::Pem)
        .unwrap();
    let pem = std::fs::read(&pem_path).unwrap();
    let der = rustls::pki_types::CertificateDer::from_pem_slice(&pem).unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(der).unwrap();
    let config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut tls = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config))
        .connect("localhost".try_into().unwrap(), s)
        .await
        .unwrap();
    tls.write_all(b"GET /gzip HTTP/1.1\r\nHost: lab\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut out = Vec::new();
    let _ = tls.read_to_end(&mut out).await;
    assert!(out.starts_with(b"HTTP/1.1 200"));

    let page = wait_for_history(&engine, 1).await;
    let row = &page.rows[0];
    assert_eq!(row.scheme, "https");
    assert!(row.tls);
    let detail = engine.exchange_detail(row.id).await.unwrap();
    assert!(
        detail
            .response
            .unwrap()
            .body_text
            .unwrap()
            .contains("hello from gzip")
    );
    assert!(engine.dashboard().await.unwrap().proxy.intercepted >= 1);

    // Regeneration changes the CA and keeps interception available.
    let before = engine.ca_status().info.unwrap().fingerprint_sha256;
    let after = engine.regenerate_ca().unwrap();
    assert_ne!(after.info.unwrap().fingerprint_sha256, before);
    assert!(after.available);
    engine.shutdown().await;
}

#[tokio::test]
async fn intercepted_request_is_edited_through_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    let proxy = proxy_addr(&engine).await;
    assert!(!engine.intercept_state().enabled, "Intercept starts off");
    assert!(engine.set_intercept(true).enabled);

    let url = format!("{}/echo", lab.url());
    let client = tokio::spawn(async move {
        let mut s = TcpStream::connect(proxy).await.unwrap();
        s.write_all(
            format!("POST {url} HTTP/1.1\r\nHost: lab\r\nContent-Length: 3\r\nConnection: close\r\n\r\na=1")
                .as_bytes(),
        )
        .await
        .unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        String::from_utf8_lossy(&out).into_owned()
    });

    let mut pending = Vec::new();
    for _ in 0..300 {
        pending = engine.intercept_state().pending;
        if !pending.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let held = engine.held_message(pending[0].id).unwrap();
    let text = held.text.unwrap();
    assert!(
        text.starts_with("POST /echo HTTP/1.1\nHost: lab\n"),
        "{text}"
    );
    engine
        .forward_held(
            pending[0].id,
            Some(chosproxy_app::HeldEdit {
                text: text.replace("a=1", "changed=yes"),
                update_content_length: true,
            }),
            false,
        )
        .unwrap();
    assert!(
        engine.forward_held(pending[0].id, None, false).is_err(),
        "already forwarded"
    );

    let response = client.await.unwrap();
    assert!(response.contains("\"body\":\"changed=yes\""), "{response}");
    let page = wait_for_history(&engine, 1).await;
    assert!(page.rows[0].edited);
    let detail = engine.exchange_detail(page.rows[0].id).await.unwrap();
    assert!(detail.request.body_text.unwrap().contains("changed=yes"));
    assert_eq!(
        detail.original_request.unwrap().body_text.as_deref(),
        Some("a=1")
    );

    // Closing the project switches Intercept off.
    engine.close_project().await.unwrap();
    assert!(!engine.intercept_state().enabled);
}

#[tokio::test]
async fn target_scope_site_map_annotations_and_search() {
    use chosproxy_app::{SearchLocation, SearchQuery};
    use chosproxy_storage::{Highlight, SiteMark};

    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    let proxy = proxy_addr(&engine).await;
    fetch_via_proxy(proxy, &format!("{}/gzip", lab.url())).await;
    fetch_via_proxy(proxy, &format!("{}/?q=1", lab.url())).await;
    let page = wait_for_history(&engine, 2).await;
    assert!(
        page.rows.iter().all(|r| !r.in_scope),
        "nothing in scope yet"
    );

    // Scope: add the lab origin, check URLs, see rows flip in scope.
    let mut events = engine.subscribe();
    let cfg = engine
        .add_url_to_scope(&format!("{}/anything", lab.url()), false, false)
        .unwrap();
    assert_eq!(cfg.include.len(), 1);
    assert!(matches!(events.try_recv(), Ok(EngineEvent::ScopeChanged)));
    assert!(
        engine
            .check_scope(&format!("{}/x", lab.url()))
            .unwrap()
            .in_scope
    );
    assert!(!engine.check_scope("http://other.test/").unwrap().in_scope);
    assert!(engine.check_scope("not a url").is_err());
    let in_scope = engine
        .history_page(HistoryQuery {
            limit: 50,
            in_scope_only: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(in_scope.total, 2);
    let reloaded = Engine::new(AppPaths::under(dir.path())).unwrap();
    assert_eq!(reloaded.scope_config(), cfg, "scope.toml persisted");
    let mut bad = cfg.clone();
    bad.include[0].host = "*.bad".into();
    assert!(engine.save_scope_config(bad).is_err());
    engine
        .add_url_to_scope(&format!("{}/gzip", lab.url()), true, true)
        .unwrap();
    assert!(
        !engine
            .check_scope(&format!("{}/gzip", lab.url()))
            .unwrap()
            .in_scope
    );

    // Site map and marks.
    let map = engine.site_map(HistoryQuery::default()).await.unwrap();
    let paths: Vec<&str> = map.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["/", "/gzip"]);
    assert!(map.entries[0].in_scope && !map.entries[1].in_scope);
    let e = &map.entries[0];
    engine
        .set_site_mark(
            e.scheme.clone(),
            e.host.clone(),
            e.port,
            "/".into(),
            Some(SiteMark::Favorite),
        )
        .await
        .unwrap();
    let map = engine.site_map(HistoryQuery::default()).await.unwrap();
    assert_eq!(map.marks[0].mark, SiteMark::Favorite);

    // Annotations.
    let gzip_id = page.rows.iter().find(|r| r.path == "/gzip").unwrap().id;
    engine
        .annotate_exchange(
            gzip_id,
            Some("zebra note 42".into()),
            Some(Highlight::Yellow),
        )
        .await
        .unwrap();
    assert!(engine.annotate_exchange(9999, None, None).await.is_err());
    let detail = engine.exchange_detail(gzip_id).await.unwrap();
    assert_eq!(detail.row.highlight, Some(Highlight::Yellow));

    // Search finds text inside the gzip body, in notes, and by regex.
    let search = |text: &str, regex: bool| SearchQuery {
        text: text.into(),
        regex,
        ..SearchQuery::default()
    };
    let r = engine
        .search(search("HELLO FROM GZIP", false))
        .await
        .unwrap();
    assert_eq!(r.hits.len(), 1);
    assert_eq!(r.hits[0].location, SearchLocation::ResponseBody);
    assert_eq!(r.hits[0].matched, "hello from gzip");
    assert_eq!(r.scanned, 2);
    let r = engine
        .search(SearchQuery {
            case_sensitive: true,
            ..search("HELLO FROM GZIP", false)
        })
        .await
        .unwrap();
    assert!(r.hits.is_empty());
    let r = engine.search(search("zebra note", false)).await.unwrap();
    assert_eq!(r.hits[0].location, SearchLocation::Notes);
    let r = engine
        .search(SearchQuery {
            in_request_headers: false,
            in_response_headers: false,
            in_response_bodies: false,
            in_notes: false,
            ..search(r"q=\d", true)
        })
        .await
        .unwrap();
    assert_eq!(r.hits.len(), 1);
    assert_eq!(r.hits[0].location, SearchLocation::Url);
    assert!(engine.search(search("(", true)).await.is_err());
    assert!(engine.search(search("  ", false)).await.is_err());
    let r = engine
        .search(SearchQuery {
            filter: HistoryQuery {
                in_scope_only: true,
                ..Default::default()
            },
            ..search("hello", false)
        })
        .await
        .unwrap();
    assert_eq!(r.scanned, 1, "the excluded /gzip is not searched");
    engine.close_project().await.unwrap();
}

#[tokio::test]
async fn out_of_scope_proxy_traffic_can_be_left_unstored() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    let proxy = proxy_addr(&engine).await;

    // Only /gzip is in scope; out-of-scope traffic is not stored.
    let mut cfg = engine
        .add_url_to_scope(&format!("{}/gzip", lab.url()), false, true)
        .unwrap();
    cfg.skip_out_of_scope_proxy = true;
    engine.save_scope_config(cfg.clone()).unwrap();

    // Forwarded either way.
    let out = fetch_via_proxy(proxy, &format!("{}/?q=1", lab.url())).await;
    assert!(out.starts_with("HTTP/1.1 200"), "{out}");
    fetch_via_proxy(proxy, &format!("{}/gzip", lab.url())).await;
    fetch_via_proxy(proxy, &format!("{}/gzip", lab.url())).await;
    let page = wait_for_history(&engine, 2).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let page = engine
        .history_page(HistoryQuery {
            limit: 50,
            ..Default::default()
        })
        .await
        .unwrap_or(page);
    assert_eq!(page.total, 2, "the out-of-scope request was not stored");
    assert!(page.rows.iter().all(|r| r.path == "/gzip"));

    // Turned off, everything is stored again.
    cfg.skip_out_of_scope_proxy = false;
    engine.save_scope_config(cfg).unwrap();
    fetch_via_proxy(proxy, &format!("{}/?q=2", lab.url())).await;
    let page = wait_for_history(&engine, 3).await;
    assert!(page.rows.iter().any(|r| r.path == "/?q=2"));
}

#[tokio::test]
async fn listener_authentication_through_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    assert!(
        engine
            .set_listener_auth("default", Some(("bad:name".into(), "pw".into())))
            .await
            .is_err()
    );
    assert!(
        engine
            .set_listener_auth("nope", Some(("u".into(), "pw".into())))
            .await
            .is_err()
    );
    engine
        .set_listener_auth("default", Some(("tester".into(), "correct horse".into())))
        .await
        .unwrap();
    let stored = std::fs::read_to_string(walk_find(dir.path(), "proxy.toml").unwrap()).unwrap();
    assert!(stored.contains("password_hash") && !stored.contains("correct horse"));

    let proxy = proxy_addr(&engine).await;
    let url = format!("{}/json", lab.url());
    assert!(
        fetch_via_proxy(proxy, &url)
            .await
            .starts_with("HTTP/1.1 407")
    );

    engine.set_listener_auth("default", None).await.unwrap();
    let proxy = proxy_addr(&engine).await;
    assert!(
        fetch_via_proxy(proxy, &url)
            .await
            .starts_with("HTTP/1.1 200")
    );
    engine.close_project().await.unwrap();
}

fn walk_find(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(found) = walk_find(&p, name) {
                return Some(found);
            }
        } else if p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
    }
    None
}

#[tokio::test]
async fn event_log_records_the_session() {
    use chosproxy_app::LogCategory;
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    engine.close_project().await.unwrap();
    let mut log = Vec::new();
    for _ in 0..200 {
        log = engine.event_log();
        if log.iter().any(|e| e.message.contains("stopped")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let cats: Vec<LogCategory> = log.iter().map(|e| e.category).collect();
    assert!(cats.contains(&LogCategory::Project), "{log:?}");
    assert!(
        log.iter()
            .any(|e| e.message.starts_with("Listener default started"))
    );
    assert!(log.iter().any(|e| e.message.starts_with("Closed ")));
    engine.clear_event_log();
    assert!(engine.event_log().is_empty());
}

#[tokio::test]
async fn proxied_cookies_fill_the_cookie_jar() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    fetch_via_proxy(
        proxy_addr(&engine).await,
        &format!("{}/cookies/set", lab.url()),
    )
    .await;
    wait_for_history(&engine, 1).await;
    let mut jar = Vec::new();
    for _ in 0..50 {
        jar = engine.cookie_jar().await.unwrap();
        if jar.len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let session = jar.iter().find(|c| c.name == "session").unwrap();
    assert_eq!(session.value, "lab-session-value");
    assert!(session.http_only && session.host_only);
    assert_eq!(
        (session.path.as_str(), session.source.as_str()),
        ("/", "proxy")
    );
}

#[tokio::test]
async fn organizer_and_decoder_through_the_engine() {
    use base64::Engine as _;
    use chosproxy_app::DecodeOperation;
    use chosproxy_storage::{OrganizerStatus, OrganizerUpdate, Severity};

    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    fetch_via_proxy(proxy_addr(&engine).await, &format!("{}/gzip", lab.url())).await;
    let row = wait_for_history(&engine, 1).await.rows[0].clone();

    let id = engine.organizer_add(row.id).await.unwrap();
    assert!(engine.organizer_add(9999).await.is_err());
    engine
        .organizer_update(
            id,
            OrganizerUpdate {
                name: "gzip endpoint".into(),
                notes: "compressed JSON".into(),
                tags: vec!["api".into()],
                status: OrganizerStatus::Confirmed,
                severity: Severity::Low,
                collection: String::new(),
            },
        )
        .await
        .unwrap();
    let list = engine.organizer_list().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].status, OrganizerStatus::Confirmed);
    let detail = engine.organizer_detail(id).await.unwrap();
    assert!(
        detail
            .response
            .unwrap()
            .body_text
            .unwrap()
            .contains("hello from gzip")
    );
    engine.organizer_delete(id).await.unwrap();
    assert!(engine.organizer_list().await.unwrap().is_empty());

    let b64 = base64::engine::general_purpose::STANDARD;
    let steps = engine
        .decode(
            &b64.encode("aGk%3D"),
            &[DecodeOperation::UrlDecode, DecodeOperation::Base64Decode],
        )
        .unwrap();
    assert_eq!(steps[1].output_text.as_deref(), Some("hi"));
    let smart = engine
        .smart_decode(&b64.encode("aGVsbG8gd29ybGQ%3D"))
        .unwrap();
    assert_eq!(
        smart.ops,
        vec![DecodeOperation::UrlDecode, DecodeOperation::Base64Decode]
    );
    engine.close_project().await.unwrap();
}

#[tokio::test]
async fn websockets_are_stored_searched_exported_and_intercepted() {
    use chosproxy_app::HeldEdit;
    use chosproxy_storage::{WsConnectionQuery, WsMessageQuery};
    use chosproxy_websocket::{Opcode, encode_frame, random_mask, read_frame};
    use tokio::io::BufReader;

    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    let mut cfg = ProxyConfig::default();
    cfg.listeners[0].port = 0;
    cfg.intercept.requests = false;
    cfg.intercept.websocket_to_server = true;
    engine.save_proxy_config(cfg).await.unwrap();
    engine.new_temporary_project().await.unwrap();
    let mut events = engine.subscribe();
    let proxy = proxy_addr(&engine).await;

    let stream = TcpStream::connect(proxy).await.unwrap();
    let (r, mut w) = stream.into_split();
    let mut r = BufReader::new(r);
    w.write_all(
        format!(
            "GET {}/ws HTTP/1.1\r\nHost: lab\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            lab.url()
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let head = chosproxy_http::head::read_head(&mut r, 65536)
        .await
        .unwrap()
        .unwrap();
    assert!(head.starts_with(b"HTTP/1.1 101"));
    let read = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut r));
    assert_eq!(read.await.unwrap().unwrap().unwrap().payload, b"welcome");

    // Intercept is on for client→server messages: edit one through the engine.
    engine.set_intercept(true);
    w.write_all(&encode_frame(
        Opcode::Text,
        b"secret-token=abc",
        Some(random_mask()),
    ))
    .await
    .unwrap();
    let mut held = None;
    for _ in 0..300 {
        if let Some(h) = engine.intercept_state().pending.first() {
            held = Some(h.clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let held = held.expect("message paused");
    let detail = engine.held_message(held.id).unwrap();
    assert_eq!(detail.text.as_deref(), Some("secret-token=abc"));
    engine
        .forward_held(
            held.id,
            Some(HeldEdit {
                text: "secret-token=EDITED".into(),
                update_content_length: false,
            }),
            false,
        )
        .unwrap();
    let read = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut r));
    assert_eq!(
        read.await.unwrap().unwrap().unwrap().payload,
        b"echo: secret-token=EDITED"
    );
    engine.set_intercept(false);
    w.write_all(&encode_frame(
        Opcode::Close,
        &[0x03, 0xe8],
        Some(random_mask()),
    ))
    .await
    .unwrap();
    let read = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut r));
    assert_eq!(read.await.unwrap().unwrap().unwrap().opcode, Opcode::Close);

    // Stored: connection (linked to its handshake), messages, search, export.
    let mut conns = Vec::new();
    for _ in 0..300 {
        conns = engine
            .ws_connections(WsConnectionQuery::default())
            .await
            .unwrap();
        if conns.first().is_some_and(|c| c.close_code.is_some()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let conn = conns[0].clone();
    assert_eq!(conn.close_code, Some(1000));
    let handshake = engine
        .exchange_detail(conn.exchange_id.unwrap())
        .await
        .unwrap();
    assert_eq!(handshake.row.status, Some(101));
    let page = engine
        .ws_messages(WsMessageQuery {
            connection_id: conn.id,
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, 5, "welcome, edited, echo, close ×2");
    let edited = page.rows.iter().find(|m| m.edited).unwrap();
    let detail = engine.ws_message(edited.id).await.unwrap();
    assert_eq!(detail.text.as_deref(), Some("secret-token=EDITED"));
    assert_eq!(detail.original_text.as_deref(), Some("secret-token=abc"));
    let found = engine
        .ws_messages(WsMessageQuery {
            connection_id: conn.id,
            search: Some("ECHO:".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(found.total, 1);
    // Global search reaches message payloads too (control frames excluded).
    let global = engine
        .search(chosproxy_app::SearchQuery {
            text: "token=EDITED".into(),
            ..chosproxy_app::SearchQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(global.ws_hits.len(), 2, "the edited message and its echo");
    assert!(global.ws_hits[0].message.id > global.ws_hits[1].message.id);
    assert!(global.ws_hits[0].url.ends_with("/ws"));
    assert_eq!(global.ws_hits[1].matched, "token=EDITED");
    assert_eq!(global.ws_scanned, 3);
    let none = engine
        .search(chosproxy_app::SearchQuery {
            text: "token=EDITED".into(),
            in_websockets: false,
            ..chosproxy_app::SearchQuery::default()
        })
        .await
        .unwrap();
    assert!(none.ws_hits.is_empty());
    let out = dir.path().join("ws.json");
    engine.ws_export(conn.id, &out).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(json["format"], "chosproxy-websocket-v1");
    assert_eq!(json["messages"].as_array().unwrap().len(), 5);
    assert_eq!(json["messages"][1]["originalText"], "secret-token=abc");
    let mut saw_ws_event = false;
    while let Ok(e) = events.try_recv() {
        saw_ws_event |= matches!(e, EngineEvent::WebSocketsChanged);
    }
    assert!(saw_ws_event);
    engine.close_project().await.unwrap();
}

#[tokio::test]
async fn projects_save_as_backups_layers_and_settings_transfer() {
    use chosproxy_app::{BackupSettings, SettingsSection};
    use chosproxy_core::scope::ScopeRule;

    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    let temp = engine.new_temporary_project().await.unwrap();
    fetch_via_proxy(proxy_addr(&engine).await, &format!("{}/json", lab.url())).await;
    wait_for_history(&engine, 1).await;

    // Save As turns the temporary project into a file and keeps the history.
    let file = dir.path().join("engagement");
    let saved = engine.save_project_as(&file).await.unwrap();
    assert_eq!(saved.kind, ProjectKind::Persistent);
    assert!(saved.path.ends_with("engagement.chosproxy"));
    assert!(
        !std::path::Path::new(&temp.path).exists(),
        "temporary file removed"
    );
    assert_eq!(wait_for_history(&engine, 1).await.total, 1);
    assert!(
        engine.save_project_as(&file).await.is_err(),
        "never overwrites"
    );

    // Scope layers: the project gets its own scope; the global one is untouched.
    let global_before = engine.scope_config();
    engine.set_scope_override(true).unwrap();
    let mut project_scope = engine.scope_config();
    project_scope.include.push(ScopeRule {
        host: "127.0.0.1".into(),
        ..Default::default()
    });
    engine.save_scope_config(project_scope.clone()).unwrap();
    let reloaded_global = Engine::new(AppPaths::under(dir.path())).unwrap();
    assert_eq!(
        reloaded_global.scope_config(),
        global_before,
        "global scope.toml untouched"
    );

    // Export of in-scope traffic only.
    let export = engine
        .export_project(&dir.path().join("export"), true)
        .await
        .unwrap();
    assert_eq!(export.exchanges, 1);

    // Notes.
    engine
        .save_project_notes("# Findings\n- login".into())
        .await
        .unwrap();
    assert_eq!(engine.project_notes().await.unwrap(), "# Findings\n- login");

    // Backups: verified files, rotation keeps the newest.
    let backup_dir = dir.path().join("backups");
    engine
        .save_backup_settings(BackupSettings {
            enabled: true,
            max_backups: 2,
            directory: Some(backup_dir.display().to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    let mut made = Vec::new();
    for _ in 0..3 {
        made.push(engine.backup_now().await.unwrap());
        tokio::time::sleep(Duration::from_millis(1100)).await; // names have 1 s resolution
    }
    let list = engine.list_backups().await.unwrap();
    assert_eq!(list.len(), 2);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&backup_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "backup folder is private");
        let file = std::fs::metadata(&list[0].path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file, 0o600);
    }
    assert_eq!(list[0].path, made[2].path, "newest kept");
    assert!(
        engine
            .project_settings()
            .await
            .unwrap()
            .last_backup_at
            .is_some()
    );

    // Settings export (no credentials), preview and validated import.
    let settings = dir.path().join("settings.json");
    engine
        .export_settings(
            &settings,
            &[SettingsSection::Proxy, SettingsSection::ProjectScope],
        )
        .await
        .unwrap();
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(text.contains("chosproxy-settings-v1") && !text.contains("password_hash"));
    let mut doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    doc["sections"]["projectScope"]["include"][0]["host"] = "changed.test".into();
    doc["sections"]["proxy"]["listeners"][0]["port"] = 0.into();
    doc["sections"]["bogus"] = 1.into();
    std::fs::write(&settings, serde_json::to_vec(&doc).unwrap()).unwrap();
    let preview = engine.preview_settings_import(&settings).await.unwrap();
    assert!(preview.warnings.iter().any(|w| w.contains("bogus")));
    let scope_preview = preview
        .sections
        .iter()
        .find(|s| s.section == SettingsSection::ProjectScope)
        .unwrap();
    assert!(scope_preview.valid && scope_preview.changed);
    assert!(
        engine.scope_config().include[0].host != "changed.test",
        "preview applies nothing"
    );
    engine
        .apply_settings_import(&settings, &[SettingsSection::ProjectScope])
        .await
        .unwrap();
    assert_eq!(engine.scope_config().include[0].host, "changed.test");
    doc["sections"]["projectScope"]["include"][0]["host"] = "*.bad".into();
    std::fs::write(&settings, serde_json::to_vec(&doc).unwrap()).unwrap();
    assert!(
        engine
            .apply_settings_import(&settings, &[SettingsSection::ProjectScope])
            .await
            .is_err()
    );
    assert_eq!(
        engine.scope_config().include[0].host,
        "changed.test",
        "invalid import not applied"
    );

    // Audit log records the security-relevant actions.
    let actions: Vec<String> = engine
        .audit_log(100)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    for expected in [
        "project.created_from",
        "scope.override.enabled",
        "scope.changed",
        "project.exported",
        "backup.created",
        "settings.exported",
        "settings.imported",
    ] {
        assert!(
            actions.iter().any(|a| a == expected),
            "missing {expected}: {actions:?}"
        );
    }

    // Reopening loads the project's own scope; restoring opens a copy.
    let path = std::path::PathBuf::from(&saved.path);
    engine.close_project().await.unwrap();
    assert_eq!(
        engine.scope_config(),
        global_before,
        "global scope after close"
    );
    engine.open_project(&path).await.unwrap();
    assert!(engine.scope_override());
    assert_eq!(engine.scope_config().include[0].host, "changed.test");
    let restored = engine
        .restore_backup(std::path::Path::new(&list[0].path))
        .await
        .unwrap();
    assert!(restored.path.contains("-restored-"));
    assert_eq!(wait_for_history(&engine, 1).await.total, 1);
    assert!(
        std::path::Path::new(&list[0].path).exists(),
        "backup left untouched"
    );
    engine.close_project().await.unwrap();
}

#[tokio::test]
async fn findings_documented_from_history() {
    use chosproxy_storage::{
        Confidence, FindingQuery, FindingSeverity, FindingStatus, FindingUpdate, NewFinding,
    };
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    let proxy = proxy_addr(&engine).await;
    fetch_via_proxy(proxy, &format!("{}/status/500", lab.url())).await;
    fetch_via_proxy(proxy, &format!("{}/json", lab.url())).await;
    let rows = wait_for_history(&engine, 2).await.rows;

    let id = engine
        .finding_create(NewFinding {
            exchange_id: rows[1].id,
            kind: "Error handling".into(),
            title: "Server error on status endpoint".into(),
            severity: FindingSeverity::Low,
            confidence: Confidence::Tentative,
            description: "Returns 500.".into(),
            remediation: String::new(),
            evidence_note: "first request".into(),
        })
        .await
        .unwrap();
    engine
        .finding_add_evidence(id, rows[0].id, "related".into())
        .await
        .unwrap();
    let detail = engine.finding_detail(id).await.unwrap();
    assert_eq!(detail.evidence.len(), 2);
    assert!(
        detail.evidence[0]
            .request
            .head
            .starts_with("GET /status/500")
    );
    assert_eq!(engine.dashboard().await.unwrap().findings.low, 1);

    engine
        .finding_update(
            id,
            FindingUpdate {
                kind: detail.row.kind.clone(),
                title: detail.row.title.clone(),
                severity: FindingSeverity::Medium,
                confidence: Confidence::Certain,
                status: FindingStatus::Confirmed,
                description: detail.description.clone(),
                remediation: "Handle the error.".into(),
                notes: String::new(),
            },
        )
        .await
        .unwrap();
    let listed = engine.findings(FindingQuery::default()).await.unwrap();
    assert_eq!(listed[0].status, FindingStatus::Confirmed);
    let actions: Vec<String> = engine
        .audit_log(50)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    for a in ["finding.created", "finding.status", "finding.severity"] {
        assert!(actions.iter().any(|x| x == a), "missing {a}");
    }
    engine.finding_delete(id).await.unwrap();
    assert!(
        engine
            .findings(FindingQuery::default())
            .await
            .unwrap()
            .is_empty()
    );
    engine.close_project().await.unwrap();
}

#[tokio::test]
async fn report_from_chosen_findings() {
    use chosproxy_app::{ReportFormat, ReportOptions};
    use chosproxy_storage::{Confidence, FindingSeverity, NewFinding};
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    let proxy = proxy_addr(&engine).await;
    fetch_via_proxy(proxy, &format!("{}/status/500", lab.url())).await;
    let rows = wait_for_history(&engine, 1).await.rows;
    let new = |title: &str, severity| NewFinding {
        exchange_id: rows[0].id,
        kind: "Error handling".into(),
        title: title.into(),
        severity,
        confidence: Confidence::Firm,
        description: "Returns <500>.".into(),
        remediation: "Handle the error.".into(),
        evidence_note: "the request".into(),
    };
    let low = engine
        .finding_create(new("Low one", FindingSeverity::Low))
        .await
        .unwrap();
    let high = engine
        .finding_create(new("High one", FindingSeverity::High))
        .await
        .unwrap();
    let left_out = engine
        .finding_create(new("Not chosen", FindingSeverity::Medium))
        .await
        .unwrap();
    assert!(left_out > high);

    let mut options = ReportOptions {
        title: "Lab assessment".into(),
        client: "Lab".into(),
        author: "Tester".into(),
        executive_summary: "Two issues.".into(),
        methodology: "Manual testing.".into(),
        finding_ids: vec![low, high],
        include_evidence: true,
        redact_secrets: true,
        max_evidence_kib: 64,
    };
    let html = engine.report_preview(options.clone()).await.unwrap();
    assert!(html.find("1. High one").unwrap() < html.find("2. Low one").unwrap());
    assert!(!html.contains("Not chosen"));
    assert!(html.contains("Returns &lt;500&gt;."));
    assert!(html.contains("GET /status/500"), "evidence included");

    let md_path = dir.path().join("report.md");
    engine
        .export_report(options.clone(), ReportFormat::Markdown, &md_path)
        .await
        .unwrap();
    assert!(
        std::fs::read_to_string(&md_path)
            .unwrap()
            .contains("### 1. High one")
    );

    options.include_evidence = false;
    let json_path = dir.path().join("report.json");
    engine
        .export_report(options, ReportFormat::Json, &json_path)
        .await
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
    assert_eq!(json["format"], "chosproxy-report-v1");
    assert_eq!(json["findings"].as_array().unwrap().len(), 2);
    assert!(
        json["findings"][0]["evidence"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let exports = engine
        .audit_log(50)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.action == "report.exported")
        .count();
    assert_eq!(exports, 2);
    engine.close_project().await.unwrap();
}

#[tokio::test]
async fn passive_analysis_records_findings_from_traffic() {
    use chosproxy_storage::{FindingQuery, FindingStatus};

    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = engine(dir.path());
    ephemeral_proxy_config(&engine).await;
    engine.new_temporary_project().await.unwrap();
    let proxy = proxy_addr(&engine).await;
    // The index page twice: one finding per issue, with two pieces of evidence.
    for path in ["/", "/", "/json"] {
        fetch_via_proxy(proxy, &format!("{}{path}", lab.url())).await;
    }
    wait_for_history(&engine, 3).await;

    let passive = FindingQuery {
        source: Some("passive".into()),
        ..Default::default()
    };
    let mut rows = Vec::new();
    for _ in 0..100 {
        let status = engine.passive_status().await.unwrap();
        rows = engine.findings(passive.clone()).await.unwrap();
        if status.analyzed_up_to == status.last_exchange_id && !rows.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
    assert!(kinds.contains(&"Content Security Policy"), "{kinds:?}");
    assert!(
        kinds.contains(&"Frameable page (clickjacking)"),
        "{kinds:?}"
    );
    let csp = rows
        .iter()
        .find(|r| r.kind == "Content Security Policy")
        .unwrap();
    assert_eq!(csp.evidence_count, 2, "repeated detections merge");
    assert_eq!(csp.status, FindingStatus::Open);
    let detail = engine.finding_detail(csp.id).await.unwrap();
    assert!(detail.description.contains("Content Security Policy"));
    assert!(!detail.remediation.is_empty());

    // Analyzing everything again adds nothing new.
    let before = rows.len();
    engine.passive_reanalyze().await.unwrap();
    for _ in 0..100 {
        let s = engine.passive_status().await.unwrap();
        if s.analyzed_up_to == s.last_exchange_id {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        engine.findings(passive.clone()).await.unwrap().len(),
        before
    );

    // Turned off: new traffic is not analyzed.
    let mut settings = engine.settings();
    settings.passive.enabled = false;
    engine.save_settings(settings).unwrap();
    fetch_via_proxy(proxy, &format!("{}/headers", lab.url())).await;
    wait_for_history(&engine, 4).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let status = engine.passive_status().await.unwrap();
    assert!(!status.enabled);
    assert!(status.analyzed_up_to < status.last_exchange_id);
    assert_eq!(engine.passive_checks().len(), status.checks_total as usize);
}
