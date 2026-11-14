//! Network settings through the engine (M21): host name overrides and
//! Match & Replace rules saved, reloaded at start and applied to real
//! traffic captured in a project. Kept in its own test binary because the
//! network settings are process-wide.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::Engine;
use chosproxy_core::config::{
    HostOverride, MatchReplaceRule, MatchTarget, NetworkConfig, ProxyConfig, UpstreamKind,
    UpstreamRule,
};
use chosproxy_core::event::ListenerState;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::HistoryQuery;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn fetch(proxy: SocketAddr, url: &str) -> String {
    let mut s = TcpStream::connect(proxy).await.unwrap();
    s.write_all(format!("GET {url} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).await.unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

#[tokio::test]
async fn saved_network_settings_and_rules_apply_to_traffic() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();

    // Invalid settings are refused and nothing is saved.
    let bad = NetworkConfig {
        upstream: vec![UpstreamRule {
            enabled: true,
            host: "*".into(),
            kind: UpstreamKind::Socks5,
            proxy_host: "127.0.0.1".into(),
            proxy_port: 0,
            username: String::new(),
            password: String::new(),
            remote_dns: true,
        }],
        ..NetworkConfig::default()
    };
    assert!(engine.save_network_config(bad).is_err());
    assert_eq!(engine.network_config(), NetworkConfig::default());

    let network = NetworkConfig {
        hosts: vec![HostOverride {
            enabled: true,
            host: "lab.local".into(),
            address: "127.0.0.1".parse().unwrap(),
        }],
        ..NetworkConfig::default()
    };
    engine.save_network_config(network.clone()).unwrap();
    let mut proxy = ProxyConfig::default();
    proxy.listeners[0].port = 0;
    proxy.match_replace = vec![MatchReplaceRule {
        enabled: true,
        target: MatchTarget::ResponseBody,
        pattern: r#""name":"lab""#.into(),
        replace: r#""name":"renamed""#.into(),
        regex: false,
        comment: "rename".into(),
    }];
    engine.save_proxy_config(proxy.clone()).await.unwrap();
    // An invalid rule is refused.
    let mut broken = proxy.clone();
    broken.match_replace[0].regex = true;
    broken.match_replace[0].pattern = "(".into();
    assert!(engine.save_proxy_config(broken).await.is_err());
    drop(engine);

    // A new engine loads both from disk.
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    assert_eq!(engine.network_config(), network);
    engine.new_temporary_project().await.unwrap();
    let addr: SocketAddr = match &engine.listeners().await[0].state {
        ListenerState::Running { address } => address.parse().unwrap(),
        other => panic!("listener not running: {other:?}"),
    };
    let port = lab.addr().port();
    let response = fetch(addr, &format!("http://lab.local:{port}/json")).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains(r#""name":"renamed""#), "{response}");

    let mut row = None;
    for _ in 0..200 {
        let page = engine
            .history_page(HistoryQuery {
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        if let Some(r) = page.rows.into_iter().next() {
            row = Some(r);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let row = row.unwrap();
    assert_eq!(row.host, "lab.local");
    assert!(row.edited);
    assert!(
        row.notes
            .unwrap_or_default()
            .contains("Response changed by Match & Replace: rename")
    );
    engine.close_project().await.unwrap();
    chosproxy_request::set_network(NetworkConfig::default());
}
