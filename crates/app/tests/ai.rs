//! ChosAI end to end against the lab's stand-in model (it echoes what it
//! receives): off by default, redaction of what is sent, both API formats,
//! and the preview check for models outside this machine.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_ai::{AiTask, ChatMessage};
use chosproxy_app::{AiAttachment, AiRequest, Engine};
use chosproxy_core::config::{AiConfig, AiMode, AiProvider};
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{NewRepeaterTab, RepeaterTabUpdate, RepeaterTarget};

const SECRET_REQUEST: &str = "POST /json HTTP/1.1\nHost: lab\nAuthorization: Bearer tok-SECRET-1\n\
    Cookie: sid=SECRET-2\nContent-Type: application/json\n\n{\"user\":\"bob\",\"password\":\"SECRET-3\"}";

async fn setup() -> (tempfile::TempDir, LabServer, Engine, i64) {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    let target = RepeaterTarget {
        scheme: "http".into(),
        host: lab.addr().ip().to_string(),
        port: lab.addr().port(),
    };
    let tab = engine
        .repeater_create(NewRepeaterTab {
            name: String::new(),
            target: target.clone(),
            request: SECRET_REQUEST.into(),
            head_crlf: true,
            body_crlf: true,
        })
        .await
        .unwrap();
    let id = engine
        .repeater_send(
            tab.id,
            RepeaterTabUpdate {
                name: tab.name.clone(),
                target,
                request: SECRET_REQUEST.into(),
                update_content_length: true,
                reuse_connection: false,
                use_cookie_jar: false,
            },
        )
        .await
        .unwrap()
        .send
        .exchange_id;
    (dir, lab, engine, id)
}

fn local(lab: &LabServer, provider: AiProvider) -> AiConfig {
    AiConfig {
        mode: AiMode::Local,
        provider,
        base_url: format!("http://{}/ai", lab.addr()),
        model: "lab-model".into(),
        ..AiConfig::default()
    }
}

#[tokio::test]
async fn off_by_default_then_explains_with_secrets_redacted() {
    let (_dir, lab, engine, id) = setup().await;
    let request = AiRequest {
        task: AiTask::ExplainExchange,
        question: None,
        attachments: vec![AiAttachment::Exchange { id }],
        conversation: vec![],
    };
    // Off: the preview works, a call does not.
    let preview = engine.ai_preview(&request).await.unwrap();
    assert!(!preview.enabled);
    assert!(!preview.external, "the default URL is on this machine");
    let err = engine.ai_run(&request, None).await.unwrap_err();
    assert!(err.to_string().contains("ChosAI is off"), "{err}");
    let all = format!("{:?}", preview.messages);
    for secret in ["SECRET-1", "SECRET-2", "SECRET-3"] {
        assert!(!all.contains(secret), "{secret} in the preview");
    }
    let kinds: Vec<&str> = preview
        .context
        .redactions
        .iter()
        .map(|r| r.kind.as_str())
        .collect();
    assert!(
        kinds.contains(&"authorization")
            && kinds.contains(&"cookie")
            && kinds.contains(&"secret-field")
    );

    for provider in [AiProvider::Ollama, AiProvider::OpenAiCompatible] {
        engine.save_ai_config(local(&lab, provider)).unwrap();
        let answer = engine.ai_run(&request, None).await.unwrap();
        // The stand-in model echoes what it received.
        assert!(
            answer
                .text
                .starts_with("echo (lab-model): Explain this exchange"),
            "{}",
            answer.text
        );
        assert!(answer.text.contains("Request #"));
        assert!(
            answer
                .text
                .contains("Authorization: Bearer [REDACTED:authorization]")
        );
        for secret in ["SECRET-1", "SECRET-2", "SECRET-3"] {
            assert!(!answer.text.contains(secret), "{secret} reached the model");
        }
        assert!(answer.sent_chars > 0);
    }
    let ok = engine
        .ai_test(local(&lab, AiProvider::Ollama))
        .await
        .unwrap();
    assert!(ok.text.contains("OK"));

    // Questions and custom patterns.
    let mut config = local(&lab, AiProvider::Ollama);
    config.redact_patterns = vec!["bob".into()];
    engine.save_ai_config(config).unwrap();
    let ask = AiRequest {
        task: AiTask::Ask,
        question: Some("Who logs in?".into()),
        attachments: vec![
            AiAttachment::Exchange { id },
            AiAttachment::Text {
                label: "Note".into(),
                text: "bob is the admin".into(),
            },
        ],
        conversation: vec![],
    };
    let answer = engine.ai_run(&ask, None).await.unwrap();
    assert!(answer.text.contains("The tester asks: Who logs in?"));
    assert!(!answer.text.contains("bob"), "{}", answer.text);
    let empty = AiRequest {
        question: None,
        ..ask.clone()
    };
    assert!(
        engine.ai_run(&empty, None).await.is_err(),
        "ask needs a question"
    );

    // A follow-up: earlier turns are sent again, the tester's redacted.
    let follow = AiRequest {
        conversation: vec![
            ChatMessage {
                role: "assistant".into(),
                content: "It is a login.".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: "And bob's token=SECRET-7?".into(),
            },
        ],
        ..ask.clone()
    };
    let preview = engine.ai_preview(&follow).await.unwrap();
    assert_eq!(preview.messages.len(), 4);
    assert_eq!(preview.messages[2].content, "It is a login.");
    assert!(!preview.messages[3].content.contains("SECRET-7"));
    let answer = engine.ai_run(&follow, None).await.unwrap();
    assert!(
        answer.text.starts_with("echo (lab-model): And"),
        "{}",
        answer.text
    );
    let bad = AiRequest {
        conversation: vec![ChatMessage {
            role: "user".into(),
            content: "skipped the answer".into(),
        }],
        ..ask.clone()
    };
    assert!(engine.ai_preview(&bad).await.is_err());

    // The audit log names the task, not the content.
    let audit = engine.audit_log(50).await.unwrap();
    let asks: Vec<_> = audit.iter().filter(|a| a.action == "ai.ask").collect();
    assert_eq!(asks.len(), 4);
    assert!(
        asks.iter()
            .all(|a| !a.detail.contains("bob") && !a.detail.contains("SECRET"))
    );
    // So does the AI activity log.
    let activity = engine.ai_activity(None, 0, 100).await.unwrap();
    assert_eq!(activity.iter().filter(|a| a.action == "ask").count(), 4);
    assert!(!format!("{activity:?}").contains("SECRET"));
}

#[tokio::test]
async fn external_models_only_get_previewed_context() {
    let (_dir, lab, engine, id) = setup().await;
    // Local mode refuses a URL that is not on this machine.
    let mut bad = local(&lab, AiProvider::OpenAiCompatible);
    bad.base_url = "http://192.0.2.10:1234".into();
    assert!(engine.save_ai_config(bad).is_err());

    engine
        .save_ai_config(AiConfig {
            mode: AiMode::External,
            provider: AiProvider::OpenAiCompatible,
            base_url: "https://ai.invalid".into(),
            model: "remote".into(),
            timeout_secs: 5,
            ..AiConfig::default()
        })
        .unwrap();
    let request = AiRequest {
        task: AiTask::ExplainRequest,
        question: None,
        attachments: vec![AiAttachment::Exchange { id }],
        conversation: vec![],
    };
    let preview = engine.ai_preview(&request).await.unwrap();
    assert!(preview.external);
    let refused = engine.ai_run(&request, None).await.unwrap_err();
    assert!(
        refused.to_string().contains("preview the context"),
        "{refused}"
    );
    let stale = engine.ai_run(&request, Some("0000")).await.unwrap_err();
    assert!(stale.to_string().contains("preview the context"));
    // With the previewed digest the call is attempted (and fails here: the
    // host does not exist), so the check passed.
    let attempted = engine
        .ai_run(&request, Some(&preview.digest))
        .await
        .unwrap_err();
    assert!(
        attempted.to_string().contains("cannot reach the model"),
        "{attempted}"
    );
}

#[tokio::test]
async fn answers_are_streamed_in_both_formats_and_can_be_cancelled() {
    let (_dir, lab, engine, id) = setup().await;
    let request = AiRequest {
        task: AiTask::ExplainExchange,
        question: None,
        attachments: vec![AiAttachment::Exchange { id }],
        conversation: vec![],
    };
    for provider in [AiProvider::Ollama, AiProvider::OpenAiCompatible] {
        engine.save_ai_config(local(&lab, provider)).unwrap();
        let whole = engine.ai_run(&request, None).await.unwrap().text;
        let mut pieces: Vec<String> = Vec::new();
        let answer = engine
            .ai_run_stream(&request, None, "s1", |d| pieces.push(d.to_string()))
            .await
            .unwrap();
        // The same answer, received in more than one piece.
        assert_eq!(answer.text, whole);
        assert!(pieces.len() > 1, "{} piece(s)", pieces.len());
        assert_eq!(pieces.concat().trim(), whole);
        assert!(!answer.text.contains("SECRET-1"));
        // The call is forgotten when it ends.
        assert!(!engine.ai_cancel("s1"));
    }

    // Cancelling stops the call; nothing is logged for it.
    let before = engine.ai_activity(None, 0, 100).await.unwrap().len();
    let mut slow = local(&lab, AiProvider::Ollama);
    slow.model = "slow".into();
    engine.save_ai_config(slow).unwrap();
    let canceller = engine.clone();
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = seen.clone();
    let started = std::time::Instant::now();
    let err = engine
        .ai_run_stream(&request, None, "s2", move |_| {
            if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                assert!(canceller.ai_cancel("s2"));
            }
        })
        .await
        .unwrap_err();
    assert!(matches!(err, chosproxy_app::AppError::Cancelled), "{err}");
    // Two calls cannot share a name: the second is refused, and the first
    // can still be cancelled. (The model is the slow one here.)
    let first = engine.clone();
    let request2 = request.clone();
    let writing = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = writing.clone();
    let running = tokio::spawn(async move {
        first
            .ai_run_stream(&request2, None, "dup", move |_| {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            })
            .await
    });
    for _ in 0..400 {
        if writing.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(writing.load(std::sync::atomic::Ordering::SeqCst));
    let err = engine
        .ai_run_stream(&request, None, "dup", |_| {})
        .await
        .unwrap_err();
    assert!(err.to_string().contains("already being written"), "{err}");
    assert!(engine.ai_cancel("dup"));
    assert!(matches!(
        running.await.unwrap(),
        Err(chosproxy_app::AppError::Cancelled)
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(seen.load(std::sync::atomic::Ordering::SeqCst) < 10);
    assert!(!engine.ai_cancel("s2"));
    assert_eq!(
        engine.ai_activity(None, 0, 100).await.unwrap().len(),
        before
    );

    // Off: no streamed call either.
    engine.save_ai_config(AiConfig::default()).unwrap();
    let err = engine
        .ai_run_stream(&request, None, "s3", |_| {})
        .await
        .unwrap_err();
    assert!(err.to_string().contains("ChosAI is off"), "{err}");
}

#[tokio::test]
async fn a_login_recording_marks_the_history_and_only_takes_proxy_requests() {
    let (_dir, _lab, engine, id) = setup().await;
    assert_eq!(engine.login_recording(), None);
    assert!(engine.login_recording_stop().await.is_err());
    assert_eq!(engine.login_recording_start().await.unwrap(), id);
    assert_eq!(engine.login_recording(), Some(id));
    // Nothing went through the proxy: nothing is picked (the Repeater send
    // of the setup is before the mark and not proxy traffic anyway).
    let rec = engine.login_recording_stop().await.unwrap();
    assert!(rec.ids.is_empty() && rec.seen == 0 && rec.skipped == 0);
    assert_eq!(engine.login_recording(), None);
    // Another project forgets a running recording.
    engine.login_recording_start().await.unwrap();
    engine.new_temporary_project().await.unwrap();
    assert_eq!(engine.login_recording(), None);
}
