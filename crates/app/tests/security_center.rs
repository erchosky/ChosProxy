//! Integration tests for M31 F: Security Center (§120) and Audit Log
//! coverage & query filtering (§122, `PLATFORM-002`).

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{
    CredentialInfo, Engine, PluginPermission, SecurityCheckStatus, SecurityOverview,
};
use chosproxy_core::config::{AiConfig, AiMode, AiProvider, ProxyConfig};
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{AuditQuery, TaskStatus};
use std::time::Duration;

fn check<'a>(overview: &'a SecurityOverview, id: &str) -> &'a chosproxy_app::SecurityCheck {
    overview
        .checks
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("missing security check {id}"))
}

#[tokio::test]
async fn security_overview_reflects_posture_and_never_leaks_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();

    // 1. Without an open project: project_encryption is Unavailable, defaults are safe.
    let ov = engine.security_overview().await;
    assert_eq!(ov.checks.len(), 13);
    assert_eq!(check(&ov, "ca_certificate").status, SecurityCheckStatus::Ok);
    assert_eq!(
        check(&ov, "proxy_listeners").status,
        SecurityCheckStatus::Ok
    );
    assert_eq!(check(&ov, "automation_api").status, SecurityCheckStatus::Ok);
    assert_eq!(
        check(&ov, "plugins_permissions").status,
        SecurityCheckStatus::Ok
    );
    assert_eq!(check(&ov, "chos_ai").status, SecurityCheckStatus::Ok);
    assert_eq!(check(&ov, "chos_agent").status, SecurityCheckStatus::Ok);
    assert_eq!(
        check(&ov, "upstream_proxies").status,
        SecurityCheckStatus::Ok
    );
    assert_eq!(check(&ov, "oast").status, SecurityCheckStatus::Ok);
    assert_eq!(
        check(&ov, "tls_verification").status,
        SecurityCheckStatus::Ok
    );
    assert_eq!(
        check(&ov, "project_encryption").status,
        SecurityCheckStatus::Unavailable
    );

    // 2. Unencrypted persistent project -> Warn
    let plain_path = dir.path().join("plain.chosproxy");
    engine.create_project(&plain_path).await.unwrap();
    let ov_plain = engine.security_overview().await;
    assert_eq!(
        check(&ov_plain, "project_encryption").status,
        SecurityCheckStatus::Warn
    );
    engine.close_project().await.unwrap();

    // 3. Encrypted persistent project -> Ok, and read-only open -> summary includes "(read-only)"
    let enc_path = dir.path().join("enc.chosproxy");
    let proj_secret = "super-secret-project-pw-9988";
    engine
        .create_project_with(&enc_path, Some(proj_secret))
        .await
        .unwrap();
    let ov_enc = engine.security_overview().await;
    assert_eq!(
        check(&ov_enc, "project_encryption").status,
        SecurityCheckStatus::Ok
    );
    engine.close_project().await.unwrap();

    engine
        .open_project_read_only(&enc_path, Some(proj_secret))
        .await
        .unwrap();
    let ov_ro = engine.security_overview().await;
    assert_eq!(
        check(&ov_ro, "project_encryption").status,
        SecurityCheckStatus::Ok
    );
    assert!(
        check(&ov_ro, "project_encryption")
            .summary
            .contains("(read-only)")
    );
    engine.close_project().await.unwrap();

    // Reopen writable for the rest of the checks
    engine
        .open_project_with(&enc_path, Some(proj_secret))
        .await
        .unwrap();

    // 4. Non-loopback listener without auth -> Warn; with auth -> Info
    let mut proxy_cfg = ProxyConfig::default();
    proxy_cfg.listeners[0].start_on_open = false;
    proxy_cfg.listeners[0].allow_remote = true;
    proxy_cfg.listeners[0].bind_address = "0.0.0.0".parse().unwrap();
    proxy_cfg.listeners[0].port = 0;
    let id = proxy_cfg.listeners[0].id.clone();
    engine.save_proxy_config(proxy_cfg).await.unwrap();
    let ov_remote = engine.security_overview().await;
    assert_eq!(
        check(&ov_remote, "proxy_listeners").status,
        SecurityCheckStatus::Warn
    );

    let listener_secret = "listener-secret-pw-7766";
    engine
        .set_listener_auth(&id, Some(("tester".into(), listener_secret.into())))
        .await
        .unwrap();
    let ov_auth = engine.security_overview().await;
    assert_eq!(
        check(&ov_auth, "proxy_listeners").status,
        SecurityCheckStatus::Info
    );

    // 5. TLS verification exception for non-loopback host -> Warn
    let mut proxy_cfg = engine.proxy_config().await;
    proxy_cfg
        .tls
        .skip_verification_hosts
        .push("staging.example.com".into());
    engine.save_proxy_config(proxy_cfg).await.unwrap();
    let ov_tls = engine.security_overview().await;
    assert_eq!(
        check(&ov_tls, "tls_verification").status,
        SecurityCheckStatus::Warn
    );
    assert!(
        check(&ov_tls, "tls_verification")
            .detail
            .contains("staging.example.com")
    );

    // 6. External ChosAI -> Warn
    let ai_cfg = AiConfig {
        mode: AiMode::External,
        provider: AiProvider::OpenAiCompatible,
        base_url: "https://api.example.com/v1".into(),
        model: "gpt-test".into(),
        ..AiConfig::default()
    };
    engine.save_ai_config(ai_cfg).unwrap();
    let ov_ai = engine.security_overview().await;
    assert_eq!(check(&ov_ai, "chos_ai").status, SecurityCheckStatus::Warn);

    // 7. Keyring credential stored with default File protection -> Warn, never leaks secret
    let cred_secret = "api-bearer-secret-token-5544";
    engine
        .credential_set(
            CredentialInfo {
                id: "test-cred".into(),
                label: "Test API key".into(),
                username: String::new(),
                has_secret: true,
            },
            Some(cred_secret.into()),
        )
        .await
        .unwrap();
    let ov_kr = engine.security_overview().await;
    assert_eq!(
        check(&ov_kr, "keyring_protection").status,
        SecurityCheckStatus::Warn
    );

    let json = serde_json::to_string(&ov_kr).unwrap();
    assert!(!json.contains(proj_secret));
    assert!(!json.contains(listener_secret));
    assert!(!json.contains(cred_secret));

    engine.shutdown().await;
}

#[tokio::test]
async fn audit_log_records_tasks_plugins_and_supports_filtered_queries() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    let proj_path = dir.path().join("audit.chosproxy");
    engine.create_project(&proj_path).await.unwrap();

    // 1. Install example plugin and grant/revoke permissions -> records plugin.permissions.changed
    engine.plugins_install_example().await.unwrap();
    engine
        .plugin_set(
            "hello-plugin",
            false,
            vec![
                PluginPermission::NetworkRead,
                PluginPermission::ScannerPassive,
            ],
        )
        .await
        .unwrap();
    engine
        .plugin_set("hello-plugin", false, vec![PluginPermission::NetworkRead])
        .await
        .unwrap();

    // 2. Run a short crawl task -> records task.crawl.started and task.crawl.completed
    let mut crawl_cfg = engine.crawl_defaults();
    crawl_cfg.start_urls = vec![lab.url()];
    crawl_cfg.max_depth = 1;
    crawl_cfg.max_requests = 5;
    let task = engine.crawl_start(crawl_cfg).await.unwrap();
    for _ in 0..100 {
        let detail = engine.task_detail(task.id).await.unwrap();
        if matches!(
            detail.row.status,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        ) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }

    // 3. Query by action_prefix = "task."
    let task_entries = engine
        .audit_log_query(AuditQuery {
            action_prefix: Some("task.".into()),
            search: None,
            from_ms: None,
            to_ms: None,
            limit: 100,
            offset: 0,
        })
        .await
        .unwrap();
    assert!(
        task_entries
            .iter()
            .any(|e| e.action == "task.crawl.started"),
        "expected task.crawl.started in {task_entries:?}"
    );
    assert!(
        task_entries
            .iter()
            .any(|e| e.action == "task.crawl.completed"),
        "expected task.crawl.completed in {task_entries:?}"
    );

    // 4. Query by action_prefix = "plugin." and search = "revoked"
    let plugin_revoked = engine
        .audit_log_query(AuditQuery {
            action_prefix: Some("plugin.".into()),
            search: Some("revoked=[scanner.passive]".into()),
            from_ms: None,
            to_ms: None,
            limit: 100,
            offset: 0,
        })
        .await
        .unwrap();
    assert_eq!(plugin_revoked.len(), 1);
    assert_eq!(plugin_revoked[0].action, "plugin.permissions.changed");

    // 5. Close and reopen project -> audit entries persist
    engine.close_project().await.unwrap();
    engine.open_project(&proj_path).await.unwrap();
    let all_entries = engine.audit_log(200).await.unwrap();
    assert!(
        all_entries
            .iter()
            .any(|e| e.action == "task.crawl.completed")
    );
    assert!(
        all_entries
            .iter()
            .any(|e| e.action == "plugin.permissions.changed")
    );

    engine.shutdown().await;
    drop(lab);
}
