//! A certificate per listener (M31b): its own CA is created next to the
//! shared one, and a certificate that cannot be loaded keeps the listener
//! from starting.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::Engine;
use chosproxy_core::config::{ListenerCertificate, ProxyConfig};
use chosproxy_core::event::ListenerState;
use chosproxy_core::paths::AppPaths;

async fn running(engine: &Engine) -> bool {
    engine
        .listeners()
        .await
        .iter()
        .any(|l| matches!(l.state, ListenerState::Running { .. }))
}

#[tokio::test]
async fn listeners_start_with_the_certificate_they_choose_or_not_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    let mut cfg = ProxyConfig::default();
    cfg.listeners[0].port = 0;
    cfg.listeners[0].certificate = ListenerCertificate::OwnCa;
    engine.save_proxy_config(cfg.clone()).await.unwrap();
    engine.new_temporary_project().await.unwrap();
    assert!(running(&engine).await);
    // Its CA was made, apart from the shared one.
    let paths = AppPaths::under(dir.path());
    let own = paths.data_dir.join("ca/listeners/default/ca-cert.pem");
    let shared = paths.data_dir.join("ca/ca-cert.pem");
    assert!(own.is_file(), "{}", own.display());
    assert_ne!(
        std::fs::read_to_string(&own).unwrap(),
        std::fs::read_to_string(&shared).unwrap()
    );
    // And it is kept: a restart of the listener does not make another.
    let first = std::fs::read_to_string(&own).unwrap();
    engine.stop_listener("default").await.unwrap();
    engine.start_listener("default").await.unwrap();
    assert_eq!(std::fs::read_to_string(&own).unwrap(), first);

    // A certificate file that is not there: the listener does not start.
    engine.stop_listener("default").await.unwrap();
    cfg.listeners[0].certificate = ListenerCertificate::File {
        cert: dir.path().join("missing.pem").display().to_string(),
        key: dir.path().join("missing.key").display().to_string(),
    };
    engine.save_proxy_config(cfg.clone()).await.unwrap();
    let err = engine.start_listener("default").await.unwrap_err();
    assert!(err.to_string().contains("certificate"), "{err}");
    assert!(!running(&engine).await);

    // A CA's own files are a certificate and its key: usable as "chosen".
    cfg.listeners[0].certificate = ListenerCertificate::File {
        cert: own.display().to_string(),
        key: own.with_file_name("ca-key.pem").display().to_string(),
    };
    engine.save_proxy_config(cfg).await.unwrap();
    engine.start_listener("default").await.unwrap();
    assert!(running(&engine).await);
}
