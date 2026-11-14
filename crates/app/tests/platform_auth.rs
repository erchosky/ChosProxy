//! Platform authentication end to end (M24): keyring credentials answer
//! Basic and Digest challenges of the lab for Repeater sends. In its own
//! test binary because the authentication settings are process-wide.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{CredentialInfo, Engine};
use chosproxy_core::config::{AuthScheme, NetworkConfig, PlatformAuthRule};
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{NewRepeaterTab, RepeaterTabUpdate, RepeaterTarget};

#[tokio::test]
async fn basic_and_digest_challenges_are_answered_from_the_keyring() {
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
    let send = async |path: &str| {
        let tab = engine
            .repeater_create(NewRepeaterTab {
                name: String::new(),
                target: target.clone(),
                request: format!("GET {path} HTTP/1.1\nHost: lab\n\n"),
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

    // Without a rule: 401.
    assert_eq!(send("/auth/basic").await.send.status, Some(401));

    // A credential in the keyring and two rules for the lab host.
    engine
        .credential_set(
            CredentialInfo {
                id: "lab".into(),
                label: "Lab user".into(),
                username: "lab".into(),
                has_secret: false,
            },
            Some("secret".into()),
        )
        .await
        .unwrap();
    let rule = |scheme| PlatformAuthRule {
        enabled: true,
        host: "127.0.0.1".into(),
        scheme,
        credential: "lab".into(),
    };
    engine
        .save_network_config(NetworkConfig {
            auth: vec![
                rule(AuthScheme::Basic),
                rule(AuthScheme::Digest),
                rule(AuthScheme::Ntlm),
            ],
            ..NetworkConfig::default()
        })
        .unwrap();

    for (path, scheme) in [
        ("/auth/basic", "Basic"),
        ("/auth/digest", "Digest"),
        ("/auth/ntlm", "NTLM"),
    ] {
        let result = send(path).await;
        assert_eq!(result.send.status, Some(200), "{path}");
        let row = &result.detail.row;
        let notes = row.notes.clone().unwrap_or_default();
        assert!(
            notes.contains(&format!("Platform authentication ({scheme})")),
            "{notes}"
        );
        assert!(
            result
                .detail
                .request
                .head
                .contains(&format!("Authorization: {scheme} ")),
            "the stored request is the one sent"
        );
    }
    // NTLM with the wrong password: the server's refusal is what comes back.
    engine
        .credential_set(
            CredentialInfo {
                id: "lab".into(),
                label: "Lab user".into(),
                username: "LAB\\lab".into(),
                has_secret: false,
            },
            Some("not the password".into()),
        )
        .await
        .unwrap();
    let refused = send("/auth/ntlm").await;
    assert_eq!(refused.send.status, Some(401));
    // The right password with a domain in the user name works too.
    engine
        .credential_set(
            CredentialInfo {
                id: "lab".into(),
                label: "Lab user".into(),
                username: "LAB\\lab".into(),
                has_secret: false,
            },
            Some("secret".into()),
        )
        .await
        .unwrap();
    assert_eq!(send("/auth/ntlm").await.send.status, Some(200));
    // The secret never appears in the credential list.
    let listed = engine.credentials().await.unwrap();
    assert!(listed[0].has_secret);
    assert!(!format!("{listed:?}").contains("secret\""));
    chosproxy_request::set_platform_auth(Vec::new());
    engine.close_project().await.unwrap();
}
