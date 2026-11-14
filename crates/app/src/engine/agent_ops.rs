//! ChosAgent in the engine (M30, ADR-0040): agent tasks, the tool
//! dispatcher that enforces DENY / ASK / ALLOW, approvals, queued tester
//! messages and the AI activity log.
//!
//! - The model is ChosAI's (`ai.toml`); the agent's permissions and limits
//!   are in `agent.toml`.
//! - Every tool call goes through [`Engine::agent_dispatch`]: a name that is
//!   not in the catalog is refused, a denied tool is refused, an ASK tool
//!   waits for the tester's decision on the exact arguments. The prompt is
//!   not trusted to keep any of this.
//! - `send_request` is capped at ASK, needs a defined scope containing the
//!   target, and counts against the task's send budget. Its requests are
//!   stored in the history with source `agent`.
//! - With a model outside this machine, every model call waits for the
//!   tester to approve the new context (what is added since the last call),
//!   and the engine checks the conversation digest before sending.
//! - Results given to the model are redacted with ChosAI's rules and cut.

use super::Engine;
use super::ai_ops::describe;
use super::lock;
use super::task_ops::Control;
use crate::AppError;
use crate::engine::{AiAttachment, SessionExtraction, SessionHeader, SessionRule};
use chosproxy_ai::agent::{
    self, AgentMessage, AgentReply, AgentRole, AgentToolInfo, ToolCall, ToolSpec,
};
use chosproxy_ai::redact::{RedactRules, redact};
use chosproxy_ai::{assemble, exchange_item};
use chosproxy_core::Source;
use chosproxy_core::config::{AgentConfig, AgentPermission, AiConfig, AiMode, ConfigSection};
use chosproxy_core::event::EngineEvent;
use chosproxy_core::time::now_ms;
use chosproxy_storage::{
    AiActivity, Confidence, FindingQuery, FindingSeverity, HistoryQuery, NewAiActivity, NewFinding,
    ProjectDb, TaskId, TaskRow, TaskStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::{oneshot, watch};
use ts_rs::TS;

/// Longest goal.
const MAX_GOAL: usize = 4_000;
/// Longest queued message.
const MAX_MESSAGE: usize = 4_000;
/// Messages waiting per task.
const MAX_QUEUED: usize = 20;
/// Largest response body kept for the model (before the context limit).
const MAX_SEND_RESPONSE: usize = 1024 * 1024;

/// Approval ids, unique in this process.
static NEXT_APPROVAL: AtomicI64 = AtomicI64::new(1);

/// What a new agent task works on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentStart {
    /// What the tester wants done.
    pub goal: String,
    /// Items given to the agent with the goal.
    pub attachments: Vec<AiAttachment>,
}

/// What an approval is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentApprovalKind {
    /// Running a tool with these arguments.
    Tool,
    /// Sending this context to a model outside this machine.
    Outgoing,
}

/// A call waiting for the tester.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentApproval {
    /// Approval id.
    #[ts(type = "number")]
    pub id: i64,
    /// Agent task.
    #[ts(type = "number")]
    pub task_id: TaskId,
    /// Tool call or outgoing context.
    pub kind: AgentApprovalKind,
    /// Tool name (empty for outgoing context).
    pub tool: String,
    /// One line: what will happen.
    pub summary: String,
    /// Where it goes (a target, a history id, the model).
    pub target: String,
    /// The agent's reason, if it gave one.
    pub reason: String,
    /// Exactly what runs or is sent: the request, the arguments, the
    /// new context.
    pub detail: String,
    /// When it was asked (ms since the epoch).
    #[ts(type = "number")]
    pub at: i64,
}

/// A step of an agent's plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentPlanStep {
    /// What the step is.
    pub title: String,
    /// Whether it is done.
    #[serde(default)]
    pub done: bool,
}

/// The live state of an agent task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentState {
    /// The task is running in this session.
    pub running: bool,
    /// Its plan (subtasks).
    pub plan: Vec<AgentPlanStep>,
    /// Calls waiting for the tester.
    pub approvals: Vec<AgentApproval>,
    /// Tester messages not yet given to the agent.
    pub queued: Vec<String>,
    /// Model calls made.
    pub steps: u32,
    /// Requests sent.
    pub sends: u32,
}

/// The tester's answer to an approval.
struct Decision {
    approve: bool,
    note: String,
}

struct Pending {
    approval: AgentApproval,
    reply: oneshot::Sender<Decision>,
}

/// An agent task's state in this session.
#[derive(Default)]
pub(crate) struct AgentRuntime {
    pending: HashMap<i64, Pending>,
    queued: Vec<String>,
    plan: Vec<AgentPlanStep>,
    steps: u32,
    sends: u32,
}

/// How a call went, for the log and for the model.
struct Outcome {
    decision: &'static str,
    result: String,
    for_model: String,
}

impl Outcome {
    fn refused(decision: &'static str, why: String) -> Outcome {
        Outcome {
            decision,
            result: why.clone(),
            for_model: format!("Refused: {why}"),
        }
    }
}

// Arguments of the tools (lenient: unknown fields are ignored).

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct SearchArgs {
    search: Option<String>,
    methods: Vec<String>,
    status_class: Option<u8>,
    in_scope_only: bool,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Deserialize)]
struct IdArgs {
    id: i64,
}

#[derive(Deserialize)]
struct DecodeArgs {
    text: String,
    operation: String,
}

#[derive(Deserialize)]
struct PlanArgs {
    steps: Vec<AgentPlanStep>,
}

#[derive(Deserialize)]
struct NoteArgs {
    id: i64,
    note: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FindingArgs {
    exchange_id: i64,
    title: String,
    severity: String,
    description: String,
    #[serde(default)]
    remediation: String,
    #[serde(default)]
    evidence_note: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ProposedRule {
    name: String,
    hosts: Vec<String>,
    tools: Vec<String>,
    use_cookie_jar: Option<bool>,
    expired_statuses: Vec<u16>,
    expired_regex: String,
    macro_requests: Vec<i64>,
    extract: Vec<SessionExtraction>,
    headers: Vec<SessionHeader>,
}

#[derive(Deserialize)]
struct RuleArgs {
    rule: ProposedRule,
    #[serde(default)]
    enable: bool,
}

#[derive(Deserialize)]
struct SendArgs {
    scheme: String,
    host: String,
    port: u16,
    request: String,
    reason: String,
}

/// A tool call checked and ready: what to show and what to run.
enum Prepared {
    Search(SearchArgs),
    Exchange(i64),
    Findings,
    Finding(i64),
    Scope,
    Analyze(i64),
    Decode(DecodeArgs),
    Plan(Vec<AgentPlanStep>),
    Note(NoteArgs),
    DraftFinding(NewFinding),
    Rule(Box<SessionRule>),
    Send {
        target: chosproxy_storage::RepeaterTarget,
        text: String,
    },
}

fn args<T: for<'de> Deserialize<'de>>(call: &ToolCall) -> Result<T, String> {
    let value = if call.arguments.is_null() {
        json!({})
    } else {
        call.arguments.clone()
    };
    serde_json::from_value(value).map_err(|e| format!("invalid arguments for {}: {e}", call.name))
}

fn permission_str(p: AgentPermission) -> &'static str {
    match p {
        AgentPermission::Deny => "deny",
        AgentPermission::Ask => "ask",
        AgentPermission::Allow => "allow",
    }
}

fn cut(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((at, _)) => format!(
            "{}\n[… cut: {} more characters]",
            &text[..at],
            text[at..].chars().count()
        ),
        None => text.to_string(),
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

impl Engine {
    /// ChosAgent settings.
    pub fn agent_config(&self) -> AgentConfig {
        self.inner.store.load::<AgentConfig>().value
    }

    /// Saves ChosAgent settings (checked against the tool catalog: a tool
    /// that sends cannot be set above ASK).
    pub fn save_agent_config(&self, config: AgentConfig) -> Result<AgentConfig, AppError> {
        config.validate().map_err(AppError::Invalid)?;
        agent::validate_config(&config).map_err(AppError::Invalid)?;
        self.inner.store.save(&config)?;
        let summary = agent::tool_infos(&config)
            .iter()
            .map(|t| format!("{}={}", t.name, permission_str(t.permission)))
            .collect::<Vec<_>>()
            .join(", ");
        self.audit(
            "agent.settings",
            &format!(
                "{} steps, {} sends; {summary}",
                config.max_steps, config.max_sends
            ),
        );
        Ok(config)
    }

    /// The tool catalog with the permissions in force.
    pub fn agent_tools(&self) -> Vec<AgentToolInfo> {
        agent::tool_infos(&self.agent_config())
    }

    /// The AI activity log: of one agent task, or everything (`task`
    /// none), after row `after`.
    pub async fn ai_activity(
        &self,
        task: Option<TaskId>,
        after: i64,
        limit: u32,
    ) -> Result<Vec<AiActivity>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.ai_activity(task, after, limit)).await??)
    }

    /// Records an activity row (failures are only logged).
    pub(super) async fn ai_log(&self, row: NewAiActivity) {
        let task = row.task_id;
        let Ok(db) = self.db() else {
            return;
        };
        match tokio::task::spawn_blocking(move || db.ai_activity_add(row)).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "could not record AI activity"),
            Err(e) => tracing::warn!(error = %e, "could not record AI activity"),
        }
        let _ = self
            .inner
            .events
            .send(EngineEvent::AgentChanged { task_id: task });
    }

    fn agent_changed(&self, task: TaskId) {
        let _ = self.inner.events.send(EngineEvent::AgentChanged {
            task_id: Some(task),
        });
    }

    /// Starts an agent task.
    pub async fn agent_start(&self, start: AgentStart) -> Result<TaskRow, AppError> {
        self.agent_launch(start, None).await
    }

    /// Starts a new agent task that continues an earlier one that is not
    /// running any more (interrupted when the project closed, stopped, or
    /// out of steps): same goal and items, seeded with what the activity
    /// log kept of the earlier conversation, plus an optional message from
    /// the tester. The earlier task is left as it is; nothing restarts by
    /// itself.
    pub async fn agent_continue(&self, task: TaskId, message: String) -> Result<TaskRow, AppError> {
        if lock(&self.inner.agents).contains_key(&task) {
            return Err(AppError::Invalid(
                "that agent task is still running: send it a message instead".into(),
            ));
        }
        if message.chars().count() > MAX_MESSAGE {
            return Err(AppError::Invalid(format!(
                "a message is at most {MAX_MESSAGE} characters"
            )));
        }
        let db = self.db()?;
        let (row, rows) = tokio::task::spawn_blocking(move || {
            Ok::<_, chosproxy_storage::StorageError>((
                db.task(task)?,
                db.ai_activity(Some(task), 0, MAX_EARLIER_ROWS)?,
            ))
        })
        .await??;
        let row = row
            .filter(|r| r.kind == "agent")
            .ok_or_else(|| AppError::Invalid(format!("no agent task #{task}")))?;
        let start: AgentStart = serde_json::from_str(&row.config)
            .map_err(|e| AppError::Invalid(format!("task #{task} cannot be read: {e}")))?;
        let mut earlier = earlier_summary(&rows, MAX_EARLIER);
        let message = message.trim();
        if !message.is_empty() {
            earlier.push_str(&format!("\nThe tester adds now: {message}\n"));
        }
        let new = self.agent_launch(start, Some((task, earlier))).await?;
        self.audit(
            "agent.continue",
            &format!("task #{} continues task #{task}", new.id),
        );
        Ok(new)
    }

    async fn agent_launch(
        &self,
        start: AgentStart,
        earlier: Option<(TaskId, String)>,
    ) -> Result<TaskRow, AppError> {
        let config = self.ai_config();
        if config.mode == AiMode::Disabled {
            return Err(AppError::Invalid(
                "ChosAI is off: choose a model in Settings → ChosAI first".into(),
            ));
        }
        let goal = start.goal.trim().to_string();
        if goal.is_empty() {
            return Err(AppError::Invalid(
                "describe what the agent should do".into(),
            ));
        }
        if goal.chars().count() > MAX_GOAL {
            return Err(AppError::Invalid(format!(
                "the goal is longer than {MAX_GOAL} characters"
            )));
        }
        if start.attachments.len() > 20 {
            return Err(AppError::Invalid("at most 20 attached items".into()));
        }
        let name = format!(
            "Agent: {}{}",
            if earlier.is_some() {
                "(continued) "
            } else {
                ""
            },
            goal.chars().take(60).collect::<String>().replace('\n', " ")
        );
        let json = serde_json::to_string(&start)
            .map_err(|e| AppError::Invalid(format!("invalid agent task: {e}")))?;
        let task = self
            .launch_task(
                "agent",
                "Agent",
                name,
                json,
                move |engine, db, id, rx| async move {
                    // Registered before the first step can look for it.
                    lock(&engine.inner.agents).insert(id, AgentRuntime::default());
                    let result = engine
                        .run_agent(&db, id, goal, start.attachments, earlier, rx)
                        .await;
                    // Approvals left behind fail; the runtime goes.
                    lock(&engine.inner.agents).remove(&id);
                    engine.agent_changed(id);
                    result
                },
            )
            .await?;
        self.audit("agent.start", &format!("task #{}", task.id));
        Ok(task)
    }

    /// The live state of an agent task (empty when it is not running).
    pub fn agent_state(&self, task: TaskId) -> AgentState {
        let agents = lock(&self.inner.agents);
        match agents.get(&task) {
            Some(rt) => {
                let mut approvals: Vec<AgentApproval> =
                    rt.pending.values().map(|p| p.approval.clone()).collect();
                approvals.sort_by_key(|a| a.id);
                AgentState {
                    running: true,
                    plan: rt.plan.clone(),
                    approvals,
                    queued: rt.queued.clone(),
                    steps: rt.steps,
                    sends: rt.sends,
                }
            }
            None => AgentState {
                running: false,
                plan: Vec::new(),
                approvals: Vec::new(),
                queued: Vec::new(),
                steps: 0,
                sends: 0,
            },
        }
    }

    /// Every call waiting for the tester, oldest first.
    pub fn agent_approvals(&self) -> Vec<AgentApproval> {
        let agents = lock(&self.inner.agents);
        let mut all: Vec<AgentApproval> = agents
            .values()
            .flat_map(|rt| rt.pending.values().map(|p| p.approval.clone()))
            .collect();
        all.sort_by_key(|a| a.id);
        all
    }

    /// Approves or rejects a waiting call.
    pub fn agent_decide(&self, approval: i64, approve: bool, note: String) -> Result<(), AppError> {
        let pending = {
            let mut agents = lock(&self.inner.agents);
            agents
                .values_mut()
                .find_map(|rt| rt.pending.remove(&approval))
        };
        let pending =
            pending.ok_or_else(|| AppError::Invalid("that call is no longer waiting".into()))?;
        let task = pending.approval.task_id;
        let summary = pending.approval.summary.clone();
        let _ = pending.reply.send(Decision {
            approve,
            note: note.trim().chars().take(1000).collect(),
        });
        self.audit(
            if approve {
                "agent.approval.approved"
            } else {
                "agent.approval.rejected"
            },
            &format!("task #{task} approval #{approval}: {summary}"),
        );
        self.agent_changed(task);
        Ok(())
    }

    /// Queues a message from the tester; the agent reads it before its next
    /// model call.
    pub fn agent_message(&self, task: TaskId, text: String) -> Result<(), AppError> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err(AppError::Invalid("the message is empty".into()));
        }
        if text.chars().count() > MAX_MESSAGE {
            return Err(AppError::Invalid(format!(
                "a message is at most {MAX_MESSAGE} characters"
            )));
        }
        {
            let mut agents = lock(&self.inner.agents);
            let rt = agents
                .get_mut(&task)
                .ok_or_else(|| AppError::Invalid("the agent task is not running".into()))?;
            if rt.queued.len() >= MAX_QUEUED {
                return Err(AppError::Invalid("too many messages are waiting".into()));
            }
            rt.queued.push(text);
        }
        self.agent_changed(task);
        Ok(())
    }

    fn with_runtime<T>(&self, task: TaskId, f: impl FnOnce(&mut AgentRuntime) -> T) -> Option<T> {
        lock(&self.inner.agents).get_mut(&task).map(f)
    }

    /// Waits for the tester's decision on `approval`; `None` when the task
    /// is cancelled meanwhile.
    async fn agent_wait(
        &self,
        approval: AgentApproval,
        rx: &mut watch::Receiver<Control>,
    ) -> Option<Decision> {
        let (tx, mut reply) = oneshot::channel();
        let id = approval.id;
        let task = approval.task_id;
        self.with_runtime(task, |rt| {
            rt.pending.insert(
                id,
                Pending {
                    approval,
                    reply: tx,
                },
            );
        })?;
        self.agent_changed(task);
        let decision = loop {
            if *rx.borrow() == Control::Cancel {
                break None;
            }
            tokio::select! {
                d = &mut reply => break d.ok(),
                changed = rx.changed() => if changed.is_err() { break None; },
            }
        };
        self.with_runtime(task, |rt| rt.pending.remove(&id));
        self.agent_changed(task);
        decision
    }

    /// Waits while the task is paused; false when it was cancelled.
    async fn agent_running(&self, rx: &mut watch::Receiver<Control>) -> bool {
        loop {
            match *rx.borrow() {
                Control::Run => return true,
                Control::Cancel | Control::Suspend => return false,
                Control::Pause => {}
            }
            if rx.changed().await.is_err() {
                return false;
            }
        }
    }

    async fn run_agent(
        &self,
        db: &ProjectDb,
        id: TaskId,
        goal: String,
        attachments: Vec<AiAttachment>,
        earlier: Option<(TaskId, String)>,
        mut rx: watch::Receiver<Control>,
    ) -> Result<TaskStatus, AppError> {
        let ai = self.ai_config();
        let settings = self.agent_config();
        let rules = RedactRules::new(&ai.redact_patterns).map_err(AppError::Invalid)?;
        let limit = ai.max_context_chars as usize;
        let external = !ai.is_loopback();
        let model = describe(&ai);
        let mut items = Vec::with_capacity(attachments.len());
        for a in &attachments {
            items.push(self.ai_item(a).await?);
        }
        let context = assemble(&items, &rules, limit);
        let tools = agent::offered(&settings);
        let mut messages = vec![
            AgentMessage::text(AgentRole::System, agent::system_prompt(&tools)),
            AgentMessage::text(AgentRole::User, agent::goal_message(&goal, &context)),
        ];
        self.ai_log(NewAiActivity {
            task_id: Some(id),
            action: "goal".into(),
            target: model.clone(),
            result: format!(
                "{} attached item(s), {} redaction(s){}",
                attachments.len(),
                context.redactions.len(),
                if external { ", external model" } else { "" }
            ),
            detail: messages[1].content.clone(),
            ..NewAiActivity::default()
        })
        .await;
        if let Some((from, text)) = earlier {
            // The same redaction as any other context.
            let text = chosproxy_ai::redact::redact(&text, &rules).0;
            self.ai_log(NewAiActivity {
                task_id: Some(id),
                action: "resume".into(),
                target: format!("task #{from}"),
                result: format!("continues task #{from}"),
                detail: text.clone(),
                ..NewAiActivity::default()
            })
            .await;
            messages.push(AgentMessage::text(
                AgentRole::User,
                format!(
                    "This task continues an earlier one (task #{from}) that stopped before it \
                     finished. What its activity log kept, oldest first — tool results are \
                     shortened, so read an item again if you need it:\n\n{text}\n\nContinue \
                     from there; do not repeat what is already done."
                ),
            ));
        }
        self.set_status(db, id, TaskStatus::Running).await?;
        // What the tester approved last (external models): messages up to
        // this index were sent before.
        let mut approved_upto = 0usize;
        let mut native_tools = true;
        for step in 1..=settings.max_steps {
            if !self.agent_running(&mut rx).await {
                return Ok(self.agent_stopped(id).await);
            }
            let queued = self
                .with_runtime(id, |rt| std::mem::take(&mut rt.queued))
                .unwrap_or_default();
            for text in queued {
                self.ai_log(NewAiActivity {
                    task_id: Some(id),
                    action: "message".into(),
                    detail: text.clone(),
                    ..NewAiActivity::default()
                })
                .await;
                messages.push(AgentMessage::text(
                    AgentRole::User,
                    format!("Message from the tester: {text}"),
                ));
            }
            agent::compact(&mut messages, limit.saturating_mul(4));
            if external {
                let new: Vec<String> = messages[approved_upto.min(messages.len())..]
                    .iter()
                    .map(|m| format!("[{:?}]\n{}", m.role, m.content))
                    .collect();
                let digest = agent::agent_digest(&messages);
                let approval = AgentApproval {
                    id: NEXT_APPROVAL.fetch_add(1, Ordering::Relaxed),
                    task_id: id,
                    kind: AgentApprovalKind::Outgoing,
                    tool: String::new(),
                    summary: format!(
                        "Send {} new message(s) to the external model (step {step})",
                        new.len()
                    ),
                    target: model.clone(),
                    reason: String::new(),
                    detail: new.join("\n\n"),
                    at: now_ms(),
                };
                let Some(decision) = self.agent_wait(approval, &mut rx).await else {
                    return Ok(self.agent_stopped(id).await);
                };
                self.ai_log(NewAiActivity {
                    task_id: Some(id),
                    action: "outgoing".into(),
                    target: model.clone(),
                    permission: "ask".into(),
                    decision: if decision.approve {
                        "approved"
                    } else {
                        "rejected"
                    }
                    .into(),
                    result: decision.note.clone(),
                    ..NewAiActivity::default()
                })
                .await;
                if !decision.approve {
                    self.task_log_lines(
                        db,
                        id,
                        vec![(
                            "warning".into(),
                            "Stopped: the tester did not approve sending the context to the \
                             external model."
                                .into(),
                        )],
                    )
                    .await;
                    return Ok(TaskStatus::Cancelled);
                }
                // Nothing may change between the approval and the call.
                if agent::agent_digest(&messages) != digest {
                    return Err(AppError::Invalid(
                        "the context changed after it was approved".into(),
                    ));
                }
            }
            self.with_runtime(id, |rt| rt.steps = step);
            let reply = match self
                .agent_model_call(&ai, &messages, &tools, step, &mut native_tools)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    self.ai_log(NewAiActivity {
                        task_id: Some(id),
                        action: "error".into(),
                        target: model.clone(),
                        result: e.to_string(),
                        ..NewAiActivity::default()
                    })
                    .await;
                    return Err(e);
                }
            };
            approved_upto = messages.len() + 1;
            self.ai_log(NewAiActivity {
                task_id: Some(id),
                action: "model".into(),
                target: model.clone(),
                result: if reply.calls.is_empty() {
                    "answer".into()
                } else {
                    format!("{} tool call(s)", reply.calls.len())
                },
                detail: reply.raw.clone(),
                ..NewAiActivity::default()
            })
            .await;
            self.progress(db, id, step, settings.max_steps).await;
            messages.push(AgentMessage::assistant(&reply));
            if reply.calls.is_empty() {
                self.ai_log(NewAiActivity {
                    task_id: Some(id),
                    action: "answer".into(),
                    target: model.clone(),
                    detail: reply.text.clone(),
                    ..NewAiActivity::default()
                })
                .await;
                self.task_log_lines(
                    db,
                    id,
                    vec![(
                        "info".into(),
                        format!("The agent answered after {step} step(s)."),
                    )],
                )
                .await;
                return Ok(TaskStatus::Completed);
            }
            for call in &reply.calls {
                if !self.agent_running(&mut rx).await {
                    return Ok(self.agent_stopped(id).await);
                }
                let result = self
                    .agent_dispatch(id, &settings, call, &mut rx)
                    .await
                    .unwrap_or_else(|| "Stopped by the tester.".into());
                let (redacted, _) = redact(&result, &rules);
                messages.push(AgentMessage::result(
                    call,
                    cut(&redacted, limit),
                    reply.native,
                ));
            }
        }
        self.ai_log(NewAiActivity {
            task_id: Some(id),
            action: "stop".into(),
            result: format!("step limit reached ({})", settings.max_steps),
            ..NewAiActivity::default()
        })
        .await;
        self.task_log_lines(
            db,
            id,
            vec![(
                "warning".into(),
                format!(
                    "Stopped at the step limit ({}); raise it in Settings → ChosAgent or \
                     start a new task.",
                    settings.max_steps
                ),
            )],
        )
        .await;
        Ok(TaskStatus::Completed)
    }

    async fn agent_stopped(&self, id: TaskId) -> TaskStatus {
        self.ai_log(NewAiActivity {
            task_id: Some(id),
            action: "stop".into(),
            result: "stopped by the tester".into(),
            ..NewAiActivity::default()
        })
        .await;
        TaskStatus::Cancelled
    }

    async fn progress(&self, db: &ProjectDb, id: TaskId, step: u32, max: u32) {
        let sends = self.with_runtime(id, |rt| rt.sends).unwrap_or(0);
        let db = db.clone();
        let progress = chosproxy_storage::TaskProgress {
            requests: u64::from(sends),
            discovered: u64::from(step),
            progress: format!("step {step} of at most {max}, {sends} request(s) sent"),
            ..Default::default()
        };
        let _ = tokio::task::spawn_blocking(move || db.task_progress(id, progress)).await;
        let _ = self.inner.events.send(EngineEvent::TasksChanged);
    }

    /// One model call. A server that refuses tools gets them described in
    /// the prompt instead (fenced calls), from then on.
    async fn agent_model_call(
        &self,
        ai: &AiConfig,
        messages: &[AgentMessage],
        tools: &[&'static ToolSpec],
        step: u32,
        native_tools: &mut bool,
    ) -> Result<AgentReply, AppError> {
        let offered: &[&ToolSpec] = if *native_tools { tools } else { &[] };
        let body = agent::agent_body(ai.provider, &ai.model, messages, offered);
        let text = self.ai_post(ai, &body).await?;
        match agent::parse_agent_reply(ai.provider, &text, step) {
            Ok(r) => Ok(r),
            Err(e) if *native_tools && e.to_ascii_lowercase().contains("tool") => {
                *native_tools = false;
                let body = agent::agent_body(ai.provider, &ai.model, messages, &[]);
                let text = self.ai_post(ai, &body).await?;
                agent::parse_agent_reply(ai.provider, &text, step).map_err(AppError::Invalid)
            }
            Err(e) => Err(AppError::Invalid(e)),
        }
    }

    /// Checks, authorizes and runs one tool call, records it, and returns
    /// what the model is told (`None`: the task was stopped while waiting).
    async fn agent_dispatch(
        &self,
        task: TaskId,
        settings: &AgentConfig,
        call: &ToolCall,
        rx: &mut watch::Receiver<Control>,
    ) -> Option<String> {
        let mut log = NewAiActivity {
            task_id: Some(task),
            action: "tool".into(),
            tool: call.name.chars().take(64).collect(),
            detail: pretty(&call.arguments),
            ..NewAiActivity::default()
        };
        let Some(spec) = agent::tool(&call.name) else {
            let o = Outcome::refused(
                "denied",
                format!(
                    "there is no tool {:?}; use only the listed tools",
                    call.name
                ),
            );
            return Some(self.agent_record(log, o).await);
        };
        let permission = agent::effective(spec, settings);
        log.permission = permission_str(permission).into();
        if permission == AgentPermission::Deny {
            let o = Outcome::refused(
                "denied",
                format!("the tester does not allow {} in this project", spec.name),
            );
            return Some(self.agent_record(log, o).await);
        }
        let (prepared, target, summary, reason) = match self.agent_prepare(task, settings, call) {
            Ok(p) => p,
            Err(e) => return Some(self.agent_record(log, Outcome::refused("failed", e)).await),
        };
        log.target = target.clone();
        let mut approved = false;
        if permission == AgentPermission::Ask {
            let detail = match &prepared {
                Prepared::Send { text, .. } => text.clone(),
                _ => pretty(&call.arguments),
            };
            let approval = AgentApproval {
                id: NEXT_APPROVAL.fetch_add(1, Ordering::Relaxed),
                task_id: task,
                kind: AgentApprovalKind::Tool,
                tool: spec.name.into(),
                summary,
                target,
                reason,
                detail,
                at: now_ms(),
            };
            let decision = self.agent_wait(approval, rx).await?;
            if !decision.approve {
                let why = if decision.note.is_empty() {
                    "the tester rejected this call".to_string()
                } else {
                    format!("the tester rejected this call: {}", decision.note)
                };
                return Some(
                    self.agent_record(log, Outcome::refused("rejected", why))
                        .await,
                );
            }
            if !decision.note.is_empty() {
                log.result = format!("note: {}", decision.note);
            }
            approved = true;
        }
        let outcome = match self.agent_run(task, prepared).await {
            Ok(text) => Outcome {
                decision: if approved { "approved" } else { "ran" },
                result: text
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect(),
                for_model: text,
            },
            Err(e) => Outcome::refused("failed", e),
        };
        Some(self.agent_record(log, outcome).await)
    }

    async fn agent_record(&self, mut log: NewAiActivity, outcome: Outcome) -> String {
        log.decision = outcome.decision.into();
        log.result = if log.result.is_empty() {
            outcome.result
        } else {
            format!("{}; {}", log.result, outcome.result)
        };
        self.ai_log(log).await;
        outcome.for_model
    }

    /// Reads and checks a call's arguments before any approval: what it
    /// would do, where, and the agent's reason. Calls that would be refused
    /// anyway (out of scope, over budget) are refused here.
    fn agent_prepare(
        &self,
        task: TaskId,
        settings: &AgentConfig,
        call: &ToolCall,
    ) -> Result<(Prepared, String, String, String), String> {
        let none = String::new;
        Ok(match call.name.as_str() {
            "search_history" => {
                let a: SearchArgs = args(call)?;
                let s = format!(
                    "Search the history for {:?}",
                    a.search.clone().unwrap_or_default()
                );
                (Prepared::Search(a), "history".into(), s, none())
            }
            "get_exchange" => {
                let a: IdArgs = args(call)?;
                (
                    Prepared::Exchange(a.id),
                    format!("#{}", a.id),
                    format!("Read request #{}", a.id),
                    none(),
                )
            }
            "list_findings" => (
                Prepared::Findings,
                "findings".into(),
                "List the findings".into(),
                none(),
            ),
            "get_finding" => {
                let a: IdArgs = args(call)?;
                (
                    Prepared::Finding(a.id),
                    format!("finding #{}", a.id),
                    format!("Read finding #{}", a.id),
                    none(),
                )
            }
            "get_scope" => (
                Prepared::Scope,
                "scope".into(),
                "Read the scope".into(),
                none(),
            ),
            "analyze_exchange" => {
                let a: IdArgs = args(call)?;
                (
                    Prepared::Analyze(a.id),
                    format!("#{}", a.id),
                    format!("Run the passive checks on #{}", a.id),
                    none(),
                )
            }
            "decode" => {
                let a: DecodeArgs = args(call)?;
                if a.text.len() > 200_000 {
                    return Err("the text to decode is too large".into());
                }
                let s = format!("Decode a value ({})", a.operation);
                (Prepared::Decode(a), "decoder".into(), s, none())
            }
            "update_plan" => {
                let a: PlanArgs = args(call)?;
                if a.steps.len() > 20 || a.steps.iter().any(|s| s.title.chars().count() > 200) {
                    return Err("a plan has at most 20 steps of 200 characters".into());
                }
                (
                    Prepared::Plan(a.steps),
                    "plan".into(),
                    "Update the plan".into(),
                    none(),
                )
            }
            "add_note" => {
                let a: NoteArgs = args(call)?;
                if a.note.chars().count() > 2000 {
                    return Err("a note is at most 2000 characters".into());
                }
                let t = format!("#{}", a.id);
                let s = format!("Set the note of #{}: {}", a.id, a.note);
                (Prepared::Note(a), t, s, none())
            }
            "draft_finding" => {
                let a: FindingArgs = args(call)?;
                let severity = FindingSeverity::parse(&a.severity)
                    .filter(|s| *s != FindingSeverity::Critical)
                    .ok_or("severity is info, low, medium or high")?;
                if a.title.trim().is_empty() || a.title.chars().count() > 300 {
                    return Err("a finding needs a title (at most 300 characters)".into());
                }
                let t = format!("#{}", a.exchange_id);
                let s = format!(
                    "Record a tentative {} finding \"{}\" on #{}",
                    severity.as_str(),
                    a.title.trim(),
                    a.exchange_id
                );
                (
                    Prepared::DraftFinding(NewFinding {
                        exchange_id: a.exchange_id,
                        kind: "ChosAgent".into(),
                        title: a.title.trim().into(),
                        severity,
                        confidence: Confidence::Tentative,
                        description: format!(
                            "{}\n\n_Drafted by ChosAgent (task #{task}); review before \
                             relying on it._",
                            a.description.trim()
                        ),
                        remediation: a.remediation,
                        evidence_note: a.evidence_note,
                    }),
                    t,
                    s,
                    none(),
                )
            }
            "propose_session_rule" => {
                let a: RuleArgs = args(call)?;
                let r = a.rule;
                let rule = SessionRule {
                    enabled: a.enable,
                    name: if r.name.trim().is_empty() {
                        "ChosAgent rule".into()
                    } else {
                        r.name.trim().chars().take(80).collect()
                    },
                    hosts: r.hosts,
                    tools: r.tools,
                    use_cookie_jar: r.use_cookie_jar.unwrap_or(true),
                    expired_statuses: r.expired_statuses,
                    expired_regex: r.expired_regex,
                    macro_requests: r.macro_requests,
                    extract: r.extract,
                    headers: r.headers,
                };
                super::session_ops::validate_rules(std::slice::from_ref(&rule))?;
                let s = format!(
                    "Add the session rule \"{}\" ({})",
                    rule.name,
                    if rule.enabled { "enabled" } else { "disabled" }
                );
                let pretty_rule = serde_json::to_string_pretty(&rule).unwrap_or_default();
                (
                    Prepared::Rule(Box::new(rule)),
                    "session rules".into(),
                    s,
                    pretty_rule,
                )
            }
            "send_request" => {
                let a: SendArgs = args(call)?;
                if read_scope_empty(self) {
                    return Err(
                        "no scope is defined: the tester must define the target scope before \
                         the agent can send requests"
                            .into(),
                    );
                }
                let sends = self.with_runtime(task, |rt| rt.sends).unwrap_or(0);
                if sends >= settings.max_sends {
                    return Err(format!(
                        "the send limit of this task ({}) is reached",
                        settings.max_sends
                    ));
                }
                if a.request.len() > 1024 * 1024 {
                    return Err("the request is too large".into());
                }
                let target =
                    chosproxy_storage::validate_target(&chosproxy_storage::RepeaterTarget {
                        scheme: a.scheme.to_ascii_lowercase(),
                        host: a.host.trim().to_ascii_lowercase(),
                        port: a.port,
                    })
                    .map_err(|e| e.to_string())?;
                let path = chosproxy_storage::request_path(&a.request);
                self.ensure_in_scope(&target, &path, "agent")
                    .map_err(|e| e.to_string())?;
                let where_ = format!("{}://{}:{}", target.scheme, target.host, target.port);
                let first = a.request.lines().next().unwrap_or_default().to_string();
                (
                    Prepared::Send {
                        target,
                        text: a.request,
                    },
                    where_.clone(),
                    format!("Send {first} to {where_}"),
                    a.reason.chars().take(1000).collect(),
                )
            }
            other => return Err(format!("there is no tool {other:?}")),
        })
    }

    /// Runs a checked (and, if needed, approved) call.
    async fn agent_run(&self, task: TaskId, prepared: Prepared) -> Result<String, String> {
        let e = |e: AppError| e.to_string();
        match prepared {
            Prepared::Search(a) => {
                let q = HistoryQuery {
                    limit: a.limit.unwrap_or(20).clamp(1, 50),
                    offset: a.offset.unwrap_or(0),
                    search: a.search.filter(|s| !s.trim().is_empty()),
                    in_scope_only: a.in_scope_only,
                    methods: a.methods.into_iter().take(10).collect(),
                    status_classes: a.status_class.into_iter().collect(),
                    ..HistoryQuery::default()
                };
                let page = self.history_page(q).await.map_err(e)?;
                let mut out = format!(
                    "{} matching request(s); showing {}:\n",
                    page.total,
                    page.rows.len()
                );
                for r in &page.rows {
                    out.push_str(&format!(
                        "#{} {} {} → {} {} {} bytes\n",
                        r.id,
                        r.method,
                        r.url,
                        r.status.map_or_else(|| "-".to_string(), |s| s.to_string()),
                        r.mime.as_deref().unwrap_or(""),
                        r.response_len.unwrap_or(0)
                    ));
                }
                Ok(out)
            }
            Prepared::Exchange(id) => {
                let item = self
                    .ai_item(&AiAttachment::Exchange { id })
                    .await
                    .map_err(e)?;
                Ok(format!("{}\n{}", item.label, item.text))
            }
            Prepared::Findings => {
                let rows = self.findings(FindingQuery::default()).await.map_err(e)?;
                let mut out = format!("{} finding(s):\n", rows.len());
                for f in rows.iter().take(200) {
                    out.push_str(&format!(
                        "#{} [{} / {} / {}] {} — {}://{}:{}{}\n",
                        f.id,
                        f.severity.as_str(),
                        f.confidence.as_str(),
                        f.status.as_str(),
                        f.title,
                        f.scheme,
                        f.host,
                        f.port,
                        f.path
                    ));
                }
                Ok(out)
            }
            Prepared::Finding(id) => {
                let item = self
                    .ai_item(&AiAttachment::Finding { id })
                    .await
                    .map_err(e)?;
                Ok(item.text)
            }
            Prepared::Scope => {
                let scope = self.scope_config();
                Ok(serde_json::to_string_pretty(&scope).unwrap_or_default())
            }
            Prepared::Analyze(id) => {
                let db = self.db().map_err(e)?;
                let ex = tokio::task::spawn_blocking(move || db.exchanges_bulk(&[id]))
                    .await
                    .map_err(|x| x.to_string())?
                    .map_err(|x| x.to_string())?
                    .pop()
                    .ok_or_else(|| format!("there is no request #{id}"))?;
                let issues = super::passive_ops::issues_in(&ex, &[]);
                if issues.is_empty() {
                    return Ok(format!("The passive checks found nothing in #{id}."));
                }
                let mut out = format!("{} detection(s) in #{id}:\n", issues.len());
                for i in issues {
                    out.push_str(&format!(
                        "- [{} / {}] {}: {}\n",
                        i.severity.as_str(),
                        i.confidence.as_str(),
                        i.title,
                        i.evidence_note
                    ));
                }
                Ok(out)
            }
            Prepared::Decode(a) => decode_text(&a.text, &a.operation),
            Prepared::Plan(steps) => {
                let done = steps.iter().filter(|s| s.done).count();
                let total = steps.len();
                self.with_runtime(task, |rt| rt.plan = steps);
                self.agent_changed(task);
                Ok(format!("Plan saved ({done} of {total} done)."))
            }
            Prepared::Note(a) => {
                self.annotate_exchange(a.id, Some(a.note.trim().to_string()), None)
                    .await
                    .map_err(e)?;
                Ok(format!("The note of #{} is set.", a.id))
            }
            Prepared::DraftFinding(new) => {
                let id = self.finding_create(new).await.map_err(e)?;
                let _ = self
                    .inner
                    .events
                    .send(EngineEvent::FindingsChanged { created: 1 });
                Ok(format!(
                    "Finding #{id} recorded as tentative, for the tester to review."
                ))
            }
            Prepared::Rule(rule) => {
                let mut rules = self.session_rules().await.map_err(e)?;
                let name = rule.name.clone();
                let enabled = rule.enabled;
                rules.push(*rule);
                self.save_session_rules(rules).await.map_err(e)?;
                Ok(format!(
                    "The session rule \"{name}\" was added ({}).",
                    if enabled {
                        "enabled"
                    } else {
                        "disabled until the tester enables it"
                    }
                ))
            }
            Prepared::Send { target, text } => self.agent_send(task, target, &text).await,
        }
    }

    async fn agent_send(
        &self,
        task: TaskId,
        target: chosproxy_storage::RepeaterTarget,
        text: &str,
    ) -> Result<String, String> {
        // Checked again: the scope may have changed while the tester decided.
        if read_scope_empty(self) {
            return Err("no scope is defined".into());
        }
        let path = chosproxy_storage::request_path(text);
        self.ensure_in_scope(&target, &path, "agent")
            .map_err(|e| e.to_string())?;
        self.with_runtime(task, |rt| rt.sends += 1);
        let request =
            crate::repeater::request_bytes(text, true, true, true).map_err(|e| e.to_string())?;
        let https = crate::repeater::scheme_of(&target) == chosproxy_http::Scheme::Https;
        let security = self.security_for_host(&target.host, https).await?;
        let destination = chosproxy_request::Destination {
            host: target.host.clone(),
            port: target.port,
            tls_name: target.host.clone(),
        };
        let limits = chosproxy_request::SendLimits {
            max_response_bytes: 4 * 1024 * 1024,
            ..chosproxy_request::SendLimits::default()
        };
        let started_at = now_ms();
        let outcome = self
            .session_send(
                "agent",
                Source::Agent,
                &destination,
                &security,
                &request,
                &limits,
            )
            .await;
        let mut exchange = crate::repeater::captured(
            Source::Agent,
            &target,
            request.clone(),
            started_at,
            &outcome,
        );
        exchange.notes = Some(
            [
                Some(format!(
                    "Sent by ChosAgent (task #{task}), approved by the tester."
                )),
                exchange.notes.take(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" "),
        );
        let db = self.db().map_err(|e| e.to_string())?;
        let exchange_id = tokio::task::spawn_blocking(move || db.insert(exchange))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        match outcome {
            Ok(sent) => {
                let response = &sent.response[..sent.response.len().min(MAX_SEND_RESPONSE)];
                let item = exchange_item(
                    &format!("Request #{exchange_id}"),
                    sent.sent_request.as_deref().unwrap_or(&request),
                    Some(response),
                );
                Ok(format!("Sent; stored as #{exchange_id}.\n{}", item.text))
            }
            Err(err) => Ok(format!(
                "The request #{exchange_id} failed: {}",
                err.message
            )),
        }
    }
}

/// Most activity rows of an earlier task read to continue it.
const MAX_EARLIER_ROWS: u32 = 2_000;
/// Most characters of an earlier conversation given to a continued task.
const MAX_EARLIER: usize = 12_000;

/// What the activity log kept of an agent task, as text for the task that
/// continues it: oldest first, each entry shortened, and the oldest entries
/// dropped when the whole is longer than `limit` characters.
fn earlier_summary(rows: &[chosproxy_storage::AiActivity], limit: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for r in rows {
        let line = match r.action.as_str() {
            "resume" => format!("[earlier still]\n{}", cut(&r.detail, 3_000)),
            "model" if r.result != "answer" && !r.detail.trim().is_empty() => {
                format!("Agent: {}", cut(r.detail.trim(), 600))
            }
            "tool" => format!(
                "Tool {}{} [{}]: {}",
                r.tool,
                if r.target.is_empty() {
                    String::new()
                } else {
                    format!(" on {}", r.target)
                },
                r.decision,
                cut(&r.result, 400)
            ),
            "message" => format!("Tester: {}", cut(&r.detail, 1_000)),
            "answer" => format!("Agent's answer: {}", cut(&r.detail, 2_000)),
            "stop" | "error" => format!("Stopped: {}", cut(&r.result, 300)),
            _ => continue,
        };
        lines.push(line);
    }
    let mut kept: Vec<String> = Vec::new();
    let mut size = 0usize;
    for line in lines.into_iter().rev() {
        let n = line.chars().count() + 1;
        if size + n > limit && !kept.is_empty() {
            kept.push("[… older entries left out]".into());
            break;
        }
        size += n;
        kept.push(line);
    }
    if kept.is_empty() {
        return "(the activity log has nothing for it)\n".into();
    }
    kept.reverse();
    let mut out = kept.join("\n");
    out.push('\n');
    out
}

fn read_scope_empty(engine: &Engine) -> bool {
    super::read(&engine.inner.scope).is_empty()
}

/// Runs a Decoder operation on text.
fn decode_text(text: &str, operation: &str) -> Result<String, String> {
    use chosproxy_decoder::{Operation, apply, smart_decode};
    let ops: Vec<Operation> = if operation == "auto" {
        smart_decode(text.as_bytes())
    } else {
        let op = match operation {
            "urlDecode" => Operation::UrlDecode,
            "base64Decode" => Operation::Base64Decode,
            "base64UrlDecode" => Operation::Base64UrlDecode,
            "hexDecode" => Operation::HexDecode,
            "htmlDecode" => Operation::HtmlDecode,
            "unicodeUnescape" => Operation::UnicodeUnescape,
            "jwtDecode" => Operation::JwtDecode,
            "jsonPretty" => Operation::JsonPretty,
            other => return Err(format!("unknown operation {other:?}")),
        };
        vec![op]
    };
    if ops.is_empty() {
        return Ok("The Decoder recognised no encoding.".into());
    }
    let mut data = text.as_bytes().to_vec();
    let mut out = String::new();
    for op in ops {
        data = apply(op, &data).map_err(|e| format!("{op:?} failed: {e}"))?;
        out.push_str(&format!("{op:?}:\n"));
        match std::str::from_utf8(&data) {
            Ok(t) => out.push_str(&cut(t, 20_000)),
            Err(_) => out.push_str(&format!("[binary, {} bytes]", data.len())),
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chosproxy_ai::agent::ToolKind;

    #[test]
    fn decodes_with_the_decoder() {
        assert!(decode_text("a%20b", "urlDecode").unwrap().contains("a b"));
        assert!(
            decode_text("aGVsbG8gd29ybGQgZnJvbSBjaG9zcHJveHk=", "auto")
                .unwrap()
                .contains("hello world")
        );
        assert!(decode_text("x", "rot13").is_err());
    }

    #[test]
    fn every_catalog_tool_has_a_dispatcher_branch() {
        // `agent_prepare` matches tool names; a catalog entry without a
        // branch would be refused at run time, so keep them together.
        let source = include_str!("agent_ops.rs");
        for t in agent::TOOLS {
            assert!(
                source.contains(&format!("\"{}\" =>", t.name)),
                "no branch for {}",
                t.name
            );
        }
        assert_eq!(
            agent::TOOLS
                .iter()
                .filter(|t| t.kind == ToolKind::Send)
                .count(),
            1
        );
    }
}
