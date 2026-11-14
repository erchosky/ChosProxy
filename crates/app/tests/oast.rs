//! ChosOAST end to end: payloads, interactions from the local server and a
//! self-hosted one, linking to the request that carried them, findings.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::Engine;
use chosproxy_core::config::{LocalOast, OastConfig, OastMode, SelfHostedOast};
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_oast::cert::SelfSigned;
use chosproxy_oast::dns;
use chosproxy_oast::server::{OastServer, ServerConfig};
use chosproxy_storage::{FindingQuery, NewRepeaterTab, RepeaterTabUpdate, RepeaterTarget};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn free_tcp_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Sends `request` to the lab through the Repeater (stored in history).
async fn send_through_repeater(engine: &Engine, lab: &LabServer, request: &str) -> i64 {
    let target = RepeaterTarget {
        scheme: "http".into(),
        host: lab.addr().ip().to_string(),
        port: lab.addr().port(),
    };
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
    let sent = engine
        .repeater_send(
            tab.id,
            RepeaterTabUpdate {
                name: tab.name.clone(),
                target,
                request: request.into(),
                update_content_length: true,
                reuse_connection: false,
                use_cookie_jar: false,
            },
        )
        .await
        .unwrap();
    sent.send.exchange_id
}

async fn oast_findings(engine: &Engine) -> Vec<chosproxy_storage::FindingRow> {
    for _ in 0..200 {
        let rows = engine
            .findings(FindingQuery {
                source: Some("oast".into()),
                ..FindingQuery::default()
            })
            .await
            .unwrap();
        if !rows.is_empty() {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Vec::new()
}

async fn http_get(addr: &str, path: &str) {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: target-callback\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut sink = Vec::new();
    s.read_to_end(&mut sink).await.unwrap();
}

#[tokio::test]
async fn local_server_links_interactions_to_the_carrying_request() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();

    // Off by default: no payloads.
    assert_eq!(engine.oast_status().mode, OastMode::Off);
    assert!(engine.oast_new_payload(String::new()).await.is_err());

    let http_port = free_tcp_port();
    let dns_port = free_udp_port();
    let smtp_port = free_tcp_port();
    let status = engine
        .oast_save_config(OastConfig {
            mode: OastMode::Local,
            local: LocalOast {
                http_port,
                dns_enabled: true,
                dns_port,
                smtp_enabled: true,
                smtp_port,
                ..LocalOast::default()
            },
            ..OastConfig::default()
        })
        .await
        .unwrap();
    assert!(status.running, "{:?}", status.error);
    assert_eq!(
        status.smtp_listener.as_deref(),
        Some(format!("127.0.0.1:{smtp_port}").as_str())
    );
    let payload = engine.oast_new_payload("search box".into()).await.unwrap();
    let id = payload.correlation_id.clone();
    assert_eq!(payload.url, format!("http://127.0.0.1:{http_port}/{id}"));
    assert_eq!(payload.dns_name, Some(format!("{id}.oast.chosproxy.test")));

    // The tester opens the payload URL directly: not a link (request to
    // the OAST server itself), and nothing is a finding yet.
    let oast_tab_target = format!("127.0.0.1:{http_port}");
    http_get(&oast_tab_target, &format!("/{id}")).await;

    // The tester sends the payload to the target...
    let carrying = send_through_repeater(
        &engine,
        &lab,
        &format!("GET /json?url=http://{id}.oast.chosproxy.test/ HTTP/1.1\nHost: lab\n\n"),
    )
    .await;
    // ...and the "target" looks the name up and fetches the URL.
    let client = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client
        .send_to(
            &dns::build_query(1, &format!("{id}.oast.chosproxy.test"), dns::TYPE_A),
            ("127.0.0.1", dns_port),
        )
        .await
        .unwrap();
    let mut answer = [0u8; 512];
    let _ = tokio::time::timeout(Duration::from_secs(5), client.recv_from(&mut answer)).await;
    http_get(&oast_tab_target, &format!("/{id}?from=target")).await;
    // ...and sends a mail to an address made of the identifier.
    let mut mail = tokio::net::TcpStream::connect(("127.0.0.1", smtp_port))
        .await
        .unwrap();
    mail.write_all(
        format!(
            "HELO target\r\nMAIL FROM:<app@target.test>\r\nRCPT TO:<{id}@oast.chosproxy.test>\r\n\
             DATA\r\nSubject: hello\r\n\r\nA mail from the target.\r\n.\r\nQUIT\r\n"
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut transcript = Vec::new();
    mail.read_to_end(&mut transcript).await.unwrap();
    assert!(String::from_utf8_lossy(&transcript).contains("recorded, not delivered"));

    let mut findings = Vec::new();
    for _ in 0..100 {
        findings = oast_findings(&engine).await;
        if findings.len() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        findings.len(),
        3,
        "one DNS, one HTTP and one SMTP finding: {findings:?}"
    );
    assert!(
        findings
            .iter()
            .any(|f| f.kind == "External service interaction (SMTP)")
    );
    assert!(
        findings
            .iter()
            .any(|f| f.kind == "External service interaction (DNS)")
    );
    let lab_port = lab.addr().port();
    assert!(
        findings.iter().all(|f| f.port == lab_port),
        "found on the target, not on the OAST server"
    );

    let interactions = engine.oast_interactions(Some(id.clone())).await.unwrap();
    assert_eq!(
        interactions.len(),
        4,
        "two HTTP requests, one lookup and one mail"
    );
    let mail = interactions.iter().find(|i| i.protocol == "smtp").unwrap();
    let detail = engine.oast_interaction(mail.id).await.unwrap();
    assert_eq!(detail.requests.len(), 1);
    assert_eq!(detail.requests[0].id, carrying);
    assert!(
        detail.text.contains("A mail from the target."),
        "{}",
        detail.text
    );
    let fetched = interactions
        .iter()
        .find(|i| i.summary.contains("from=target"))
        .unwrap();
    assert!(
        engine
            .oast_interaction(fetched.id)
            .await
            .unwrap()
            .text
            .contains("from=target")
    );
    let payloads = engine.oast_payloads().await.unwrap();
    assert_eq!((payloads[0].interactions, payloads[0].requests), (4, 1));

    // Identifiers of other projects are not stored.
    http_get(
        &oast_tab_target,
        &format!("/{}", chosproxy_oast::new_id().unwrap()),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(engine.oast_interactions(None).await.unwrap().len(), 4);
    assert!(engine.oast_status().ignored >= 1);

    // Closing the project stops the server.
    engine.close_project().await.unwrap();
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", smtp_port))
            .await
            .is_err()
    );
    assert!(
        tokio::net::TcpStream::connect(&oast_tab_target)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn self_hosted_server_is_polled_with_token_and_pin() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();

    // The tester's own server.
    let cert = SelfSigned::generate("oast.example").unwrap();
    let server = OastServer::start(ServerConfig {
        zone: "oast.example".into(),
        addresses: Default::default(),
        http: Some("127.0.0.1:0".parse().unwrap()),
        https: None,
        dns: Some("127.0.0.1:0".parse().unwrap()),
        smtp: None,
        keep: 1000,
    })
    .await
    .unwrap();
    let api = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_port = api.local_addr().unwrap().port();
    let token = "t".repeat(40);
    let api_task = tokio::spawn(chosproxy_oast::api::serve(
        api,
        cert.server_config().unwrap(),
        token.clone(),
        "oast.example".into(),
        server.log().clone(),
    ));
    let self_hosted = SelfHostedOast {
        zone: "oast.example".into(),
        api_host: "localhost".into(),
        api_port,
        token,
        fingerprint: cert.fingerprint.clone(),
    };
    assert!(
        engine
            .oast_test_server(self_hosted.clone())
            .await
            .unwrap()
            .contains("zone oast.example")
    );
    let wrong = SelfHostedOast {
        fingerprint: "11".repeat(32),
        ..self_hosted.clone()
    };
    assert!(engine.oast_test_server(wrong).await.is_err());

    engine
        .oast_save_config(OastConfig {
            mode: OastMode::SelfHosted,
            self_hosted,
            poll_seconds: 1,
            ..OastConfig::default()
        })
        .await
        .unwrap();
    let payload = engine.oast_new_payload(String::new()).await.unwrap();
    let id = payload.correlation_id;
    assert_eq!(payload.dns_name, Some(format!("{id}.oast.example")));
    send_through_repeater(
        &engine,
        &lab,
        &format!("POST /echo HTTP/1.1\nHost: lab\nContent-Type: application/xml\n\n<x>{id}.oast.example</x>"),
    )
    .await;
    let client = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client
        .send_to(
            &dns::build_query(3, &format!("{id}.oast.example"), dns::TYPE_A),
            server.dns_addr().unwrap(),
        )
        .await
        .unwrap();

    let findings = oast_findings(&engine).await;
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(engine.oast_status().last_poll_at.is_some());
    engine.close_project().await.unwrap();
    api_task.abort();
}
