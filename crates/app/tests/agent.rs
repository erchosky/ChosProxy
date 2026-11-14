//! ChosAgent end to end against the lab's scripted stand-in model: the
//! dispatcher enforces DENY / ASK / ALLOW whatever the model asks for,
//! sending needs a defined scope and the tester's approval of the exact
//! request, unknown tools are refused, models without native tools work
//! with fenced calls, external models wait for approval of every context,
//! and everything lands in the AI activity log.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{AgentApprovalKind, AgentStart, AiAttachment, Engine};
use chosproxy_core::Source;
use chosproxy_core::config::{AgentConfig, AgentPermission, AiConfig, AiMode, AiProvider};
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{AiActivity, HistoryQuery, TaskStatus};
use std::collections::BTreeMap;
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

fn model(lab: &LabServer, provider: AiProvider, name: &str) -> AiConfig {
    AiConfig {
        mode: AiMode::Local,
        provider,
        base_url: format!("http://{}/agent", lab.addr()),
        model: name.into(),
        ..AiConfig::default()
    }
}

fn goal() -> AgentStart {
    AgentStart {
        goal: "Look at the lab's home page".into(),
        attachments: vec![AiAttachment::Text {
            label: "Note".into(),
            text: "token=SECRET-9".into(),
        }],
    }
}

async fn finished(engine: &Engine, id: i64) -> TaskStatus {
    for _ in 0..400 {
        let row = engine.task_detail(id).await.unwrap().row;
        if row.status.is_finished() {
            return row.status;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the agent task did not finish");
}

async fn approval(engine: &Engine, id: i64) -> chosproxy_app::AgentApproval {
    for _ in 0..400 {
        if let Some(a) = engine.agent_state(id).approvals.into_iter().next() {
            return a;
        }
        let row = engine.task_detail(id).await.unwrap().row;
        assert!(!row.status.is_finished(), "finished while waiting: {row:?}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("no approval was asked");
}

async fn tools(engine: &Engine, id: i64) -> Vec<AiActivity> {
    engine
        .ai_activity(Some(id), 0, 500)
        .await
        .unwrap()
        .into_iter()
        .filter(|a| a.action == "tool")
        .collect()
}

#[tokio::test]
async fn without_a_scope_nothing_is_sent_and_unknown_tools_are_refused() {
    let (_dir, lab, engine) = setup().await;
    // Off: no task.
    assert!(engine.agent_start(goal()).await.is_err());
    engine
        .save_ai_config(model(&lab, AiProvider::Ollama, "lab-agent"))
        .unwrap();
    let task = engine.agent_start(goal()).await.unwrap();
    assert_eq!(finished(&engine, task.id).await, TaskStatus::Completed);
    let calls = tools(&engine, task.id).await;
    let by = |name: &str| calls.iter().find(|c| c.tool == name).unwrap().clone();
    assert_eq!(by("update_plan").decision, "ran");
    assert_eq!(by("search_history").permission, "allow");
    let send = by("send_request");
    assert_eq!(send.permission, "ask");
    assert_eq!(send.decision, "failed");
    assert!(send.result.contains("no scope is defined"), "{send:?}");
    let shell = by("run_shell");
    assert_eq!(shell.decision, "denied");
    assert!(shell.result.contains("no tool"));
    assert_eq!(by("get_scope").decision, "ran");
    // Nothing reached the history from the agent.
    let page = engine
        .history_page(HistoryQuery {
            sources: vec![Source::Agent],
            limit: 10,
            ..HistoryQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, 0);
    // The goal was redacted before the model saw it.
    let log = engine.ai_activity(Some(task.id), 0, 500).await.unwrap();
    let goal_row = log.iter().find(|a| a.action == "goal").unwrap();
    assert!(!goal_row.detail.contains("SECRET-9"), "{}", goal_row.detail);
    let answer = log.iter().find(|a| a.action == "answer").unwrap();
    assert!(answer.detail.starts_with("Done after 5 tool results"));
    assert!(!format!("{log:?}").contains("SECRET-9"));
}

#[tokio::test]
async fn sends_only_the_approved_request_in_scope_and_respects_rejections() {
    let (_dir, lab, engine) = setup().await;
    engine
        .add_url_to_scope(&format!("http://{}/", lab.addr()), false, false)
        .unwrap();
    for provider in [AiProvider::Ollama, AiProvider::OpenAiCompatible] {
        engine
            .save_ai_config(model(&lab, provider, "lab-agent"))
            .unwrap();
        // Approve.
        let task = engine.agent_start(goal()).await.unwrap();
        let a = approval(&engine, task.id).await;
        assert_eq!(a.kind, AgentApprovalKind::Tool);
        assert_eq!(a.tool, "send_request");
        assert!(
            a.detail.starts_with("GET /?agent=1 HTTP/1.1"),
            "{}",
            a.detail
        );
        assert_eq!(a.reason, "See what the home page returns.");
        let state = engine.agent_state(task.id);
        assert_eq!(state.plan.len(), 2, "the plan is kept while it runs");
        engine
            .agent_message(task.id, "Keep it short".into())
            .unwrap();
        engine.agent_decide(a.id, true, String::new()).unwrap();
        assert!(
            engine.agent_decide(a.id, true, String::new()).is_err(),
            "decided once"
        );
        assert_eq!(finished(&engine, task.id).await, TaskStatus::Completed);
        let send = tools(&engine, task.id)
            .await
            .into_iter()
            .find(|c| c.tool == "send_request")
            .unwrap();
        assert_eq!(send.decision, "approved");
        assert!(send.result.contains("stored as #"), "{send:?}");
        let log = engine.ai_activity(Some(task.id), 0, 500).await.unwrap();
        assert!(
            log.iter()
                .any(|r| r.action == "message" && r.detail == "Keep it short")
        );

        // Reject: nothing is sent, the model is told and goes on.
        let task = engine.agent_start(goal()).await.unwrap();
        let a = approval(&engine, task.id).await;
        engine.agent_decide(a.id, false, "not now".into()).unwrap();
        assert_eq!(finished(&engine, task.id).await, TaskStatus::Completed);
        let send = tools(&engine, task.id)
            .await
            .into_iter()
            .find(|c| c.tool == "send_request")
            .unwrap();
        assert_eq!(send.decision, "rejected");
        assert!(send.result.contains("not now"));
    }
    let page = engine
        .history_page(HistoryQuery {
            sources: vec![Source::Agent],
            limit: 10,
            ..HistoryQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, 2, "one approved send per provider");
    let ex = engine.exchange_detail(page.rows[0].id).await.unwrap();
    assert!(
        ex.row.notes.as_deref().unwrap_or("").contains("ChosAgent"),
        "{:?}",
        ex.row.notes
    );

    // Stopping while an approval waits cancels the task.
    let task = engine.agent_start(goal()).await.unwrap();
    approval(&engine, task.id).await;
    engine.task_cancel(task.id).unwrap();
    assert_eq!(finished(&engine, task.id).await, TaskStatus::Cancelled);
    assert!(engine.agent_approvals().is_empty());
}

#[tokio::test]
async fn permissions_are_enforced_by_the_engine_and_capped() {
    let (_dir, lab, engine) = setup().await;
    engine
        .add_url_to_scope(&format!("http://{}/", lab.addr()), false, false)
        .unwrap();
    // Sending can never be allowed without asking.
    let mut config = AgentConfig {
        permissions: BTreeMap::from([("send_request".into(), AgentPermission::Allow)]),
        ..AgentConfig::default()
    };
    assert!(engine.save_agent_config(config.clone()).is_err());
    config.permissions = BTreeMap::from([
        ("send_request".into(), AgentPermission::Deny),
        ("search_history".into(), AgentPermission::Deny),
    ]);
    engine.save_agent_config(config).unwrap();
    let tools_view = engine.agent_tools();
    assert_eq!(
        tools_view
            .iter()
            .find(|t| t.name == "send_request")
            .unwrap()
            .permission,
        AgentPermission::Deny
    );
    // The scripted model calls denied tools anyway: the engine refuses.
    engine
        .save_ai_config(model(&lab, AiProvider::Ollama, "lab-agent"))
        .unwrap();
    let task = engine.agent_start(goal()).await.unwrap();
    assert_eq!(finished(&engine, task.id).await, TaskStatus::Completed);
    let calls = tools(&engine, task.id).await;
    for name in ["send_request", "search_history"] {
        let c = calls.iter().find(|c| c.tool == name).unwrap();
        assert_eq!(
            (c.permission.as_str(), c.decision.as_str()),
            ("deny", "denied")
        );
    }
    assert!(engine.agent_approvals().is_empty());
}

#[tokio::test]
async fn models_without_native_tools_use_fenced_calls() {
    let (_dir, lab, engine) = setup().await;
    engine
        .save_ai_config(model(&lab, AiProvider::Ollama, "text-only"))
        .unwrap();
    let task = engine.agent_start(goal()).await.unwrap();
    assert_eq!(finished(&engine, task.id).await, TaskStatus::Completed);
    let calls = tools(&engine, task.id).await;
    assert!(
        calls
            .iter()
            .any(|c| c.tool == "update_plan" && c.decision == "ran")
    );
    assert!(
        calls
            .iter()
            .any(|c| c.tool == "get_scope" && c.decision == "ran")
    );
}

#[tokio::test]
async fn external_models_wait_for_every_context_to_be_approved() {
    let (_dir, _lab, engine) = setup().await;
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
    // Rejected: no call, the task stops.
    let task = engine.agent_start(goal()).await.unwrap();
    let a = approval(&engine, task.id).await;
    assert_eq!(a.kind, AgentApprovalKind::Outgoing);
    assert!(a.detail.contains("Goal: Look at the lab's home page"));
    assert!(!a.detail.contains("SECRET-9"));
    engine.agent_decide(a.id, false, String::new()).unwrap();
    assert_eq!(finished(&engine, task.id).await, TaskStatus::Cancelled);
    // Approved: the call is attempted (the host does not exist).
    let task = engine.agent_start(goal()).await.unwrap();
    let a = approval(&engine, task.id).await;
    engine.agent_decide(a.id, true, String::new()).unwrap();
    assert_eq!(finished(&engine, task.id).await, TaskStatus::Failed);
    let log = engine.ai_activity(Some(task.id), 0, 100).await.unwrap();
    assert!(
        log.iter()
            .any(|r| r.action == "outgoing" && r.decision == "approved")
    );
    assert!(
        log.iter()
            .any(|r| r.action == "error" && r.result.contains("cannot reach the model"))
    );
}

#[tokio::test]
async fn a_stopped_task_is_continued_by_a_new_one_seeded_with_its_log() {
    let (_dir, lab, engine) = setup().await;
    engine
        .add_url_to_scope(&format!("http://{}/", lab.addr()), false, false)
        .unwrap();
    engine
        .save_ai_config(model(&lab, AiProvider::Ollama, "lab-agent"))
        .unwrap();
    let task = engine.agent_start(goal()).await.unwrap();
    // It waits for an approval: still running, so it cannot be continued.
    let _ = approval(&engine, task.id).await;
    let err = engine
        .agent_continue(task.id, String::new())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("still running"), "{err}");
    // Stopped half way (as when the project closes under it).
    engine.task_cancel(task.id).unwrap();
    assert_eq!(finished(&engine, task.id).await, TaskStatus::Cancelled);
    let before = engine
        .ai_activity(Some(task.id), 0, 500)
        .await
        .unwrap()
        .len();
    assert!(engine.agent_continue(9_999, String::new()).await.is_err());

    let next = engine
        .agent_continue(task.id, "Skip the request, token=SECRET-9".into())
        .await
        .unwrap();
    assert_ne!(next.id, task.id);
    assert!(
        next.name.starts_with("Agent: (continued) "),
        "{}",
        next.name
    );
    // Nothing was sent by itself: the new task asks again.
    let a = approval(&engine, next.id).await;
    assert_eq!(a.tool, "send_request");
    let log = engine.ai_activity(Some(next.id), 0, 500).await.unwrap();
    let resume = log.iter().find(|r| r.action == "resume").unwrap();
    assert_eq!(resume.target, format!("task #{}", task.id));
    assert!(
        resume.detail.contains("Tool update_plan"),
        "{}",
        resume.detail
    );
    assert!(
        resume
            .detail
            .contains("The tester adds now: Skip the request")
    );
    assert!(!resume.detail.contains("SECRET-9"), "{}", resume.detail);
    engine.agent_decide(a.id, false, String::new()).unwrap();
    assert!(finished(&engine, next.id).await.is_finished());
    // The earlier task is left as it was.
    assert_eq!(
        engine
            .ai_activity(Some(task.id), 0, 500)
            .await
            .unwrap()
            .len(),
        before
    );
    assert_eq!(
        engine.task_detail(task.id).await.unwrap().row.status,
        TaskStatus::Cancelled
    );
}
