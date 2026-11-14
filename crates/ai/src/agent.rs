//! ChosAgent's protocol (M30, ADR-0040), without I/O: the fixed tool
//! catalog and its permissions, the agent's prompt, the conversation
//! messages, the chat bodies with tools for Ollama and OpenAI-compatible
//! servers, and reading tool calls from their replies.
//!
//! The catalog is the whole world of the agent: ChosProxy's own
//! deterministic tools, nothing else (no shell, files or OS). The engine's
//! dispatcher refuses any name that is not here and enforces the
//! permission of every call; what this module tells the model is only a
//! description.

use crate::Context;
use chosproxy_core::config::{AgentConfig, AgentPermission, AiProvider};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

/// Most tool calls taken from one reply (the rest are ignored).
pub const MAX_CALLS_PER_REPLY: usize = 8;

/// What a tool touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AgentToolKind")]
pub enum ToolKind {
    /// Reads the project (nothing changes, nothing is sent).
    Read,
    /// Only the agent task itself (its plan).
    Task,
    /// Changes the project (notes, findings, session rules).
    Project,
    /// Sends traffic to a target.
    Send,
}

/// A tool of the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSpec {
    /// Name the model calls.
    pub name: &'static str,
    /// One line for the tester.
    pub summary: &'static str,
    /// What the model is told.
    pub description: &'static str,
    /// JSON schema of the arguments.
    pub parameters: &'static str,
    /// What it touches.
    pub kind: ToolKind,
    /// Permission when the settings do not name the tool.
    pub default: AgentPermission,
    /// Highest permission the settings can give it.
    pub highest: AgentPermission,
}

use AgentPermission::{Allow, Ask};

/// Every tool the agent can call.
pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "search_history",
        summary: "Search the HTTP history",
        description: "List stored requests (newest first): id, method, URL, status, MIME type, \
            length. Filter with text found in the URL, request or response, methods, a status \
            class (2 for 2xx …) and in-scope only.",
        parameters: r#"{"type":"object","properties":{"search":{"type":"string"},"methods":{"type":"array","items":{"type":"string"}},"statusClass":{"type":"integer","minimum":1,"maximum":5},"inScopeOnly":{"type":"boolean"},"limit":{"type":"integer","minimum":1,"maximum":50},"offset":{"type":"integer","minimum":0}},"additionalProperties":false}"#,
        kind: ToolKind::Read,
        default: Allow,
        highest: Allow,
    },
    ToolSpec {
        name: "get_exchange",
        summary: "Read a stored request and its response",
        description: "The request and response of a history entry, redacted and cut to a size \
            limit.",
        parameters: r#"{"type":"object","properties":{"id":{"type":"integer"}},"required":["id"],"additionalProperties":false}"#,
        kind: ToolKind::Read,
        default: Allow,
        highest: Allow,
    },
    ToolSpec {
        name: "list_findings",
        summary: "List the findings",
        description: "The project's findings: id, title, severity, confidence, status, URL.",
        parameters: r#"{"type":"object","properties":{},"additionalProperties":false}"#,
        kind: ToolKind::Read,
        default: Allow,
        highest: Allow,
    },
    ToolSpec {
        name: "get_finding",
        summary: "Read a finding",
        description: "A finding with its description, remediation, notes and first evidence.",
        parameters: r#"{"type":"object","properties":{"id":{"type":"integer"}},"required":["id"],"additionalProperties":false}"#,
        kind: ToolKind::Read,
        default: Allow,
        highest: Allow,
    },
    ToolSpec {
        name: "get_scope",
        summary: "Read the target scope",
        description: "The include and exclude rules of the scope. Requests can only be sent to \
            targets inside it.",
        parameters: r#"{"type":"object","properties":{},"additionalProperties":false}"#,
        kind: ToolKind::Read,
        default: Allow,
        highest: Allow,
    },
    ToolSpec {
        name: "analyze_exchange",
        summary: "Run the passive checks on a stored exchange",
        description: "Runs ChosProxy's passive checks (headers, cookies, information disclosure \
            …) on a stored request and response. Sends nothing.",
        parameters: r#"{"type":"object","properties":{"id":{"type":"integer"}},"required":["id"],"additionalProperties":false}"#,
        kind: ToolKind::Read,
        default: Allow,
        highest: Allow,
    },
    ToolSpec {
        name: "decode",
        summary: "Decode a value",
        description: "Decodes text with the Decoder: urlDecode, base64Decode, base64UrlDecode, \
            hexDecode, htmlDecode, unicodeUnescape, jwtDecode, jsonPretty, or auto (the \
            Decoder's own guess, step by step).",
        parameters: r#"{"type":"object","properties":{"text":{"type":"string"},"operation":{"type":"string","enum":["auto","urlDecode","base64Decode","base64UrlDecode","hexDecode","htmlDecode","unicodeUnescape","jwtDecode","jsonPretty"]}},"required":["text","operation"],"additionalProperties":false}"#,
        kind: ToolKind::Read,
        default: Allow,
        highest: Allow,
    },
    ToolSpec {
        name: "update_plan",
        summary: "Keep the task's plan (subtasks)",
        description: "Replaces the plan of this task: up to 20 steps, each with a title and \
            whether it is done. Keep it current; the tester follows it.",
        parameters: r#"{"type":"object","properties":{"steps":{"type":"array","maxItems":20,"items":{"type":"object","properties":{"title":{"type":"string"},"done":{"type":"boolean"}},"required":["title","done"]}}},"required":["steps"],"additionalProperties":false}"#,
        kind: ToolKind::Task,
        default: Allow,
        highest: Allow,
    },
    ToolSpec {
        name: "add_note",
        summary: "Write the note of a history entry",
        description: "Replaces the note of a history entry (keep it short and factual).",
        parameters: r#"{"type":"object","properties":{"id":{"type":"integer"},"note":{"type":"string"}},"required":["id","note"],"additionalProperties":false}"#,
        kind: ToolKind::Project,
        default: Ask,
        highest: Allow,
    },
    ToolSpec {
        name: "draft_finding",
        summary: "Record a tentative finding",
        description: "Records a finding with confidence tentative and status open, for the \
            tester to review, with a history entry as evidence. Only for problems the traffic \
            shows; say what to check by hand.",
        parameters: r#"{"type":"object","properties":{"exchangeId":{"type":"integer"},"title":{"type":"string"},"severity":{"type":"string","enum":["info","low","medium","high"]},"description":{"type":"string"},"remediation":{"type":"string"},"evidenceNote":{"type":"string"}},"required":["exchangeId","title","severity","description"],"additionalProperties":false}"#,
        kind: ToolKind::Project,
        default: Ask,
        highest: Allow,
    },
    ToolSpec {
        name: "propose_session_rule",
        summary: "Add a session handling rule",
        description: "Adds a session rule that keeps tools logged in. rule: {name, hosts \
            [patterns], tools [repeater, fuzzer, sequencer, crawler, plugin, agent; empty = \
            all], useCookieJar, expiredStatuses [numbers], expiredRegex, macroRequests [history \
            ids replayed in order], extract [{name, regex}], headers [{name, value with \
            {{var}}]}. It is added disabled for the tester to review unless enable is true.",
        parameters: r#"{"type":"object","properties":{"rule":{"type":"object"},"enable":{"type":"boolean"}},"required":["rule"],"additionalProperties":false}"#,
        kind: ToolKind::Project,
        default: Ask,
        highest: Ask,
    },
    ToolSpec {
        name: "send_request",
        summary: "Send a request to an in-scope target",
        description: "Sends one HTTP/1.1 request to a target inside the scope and returns the \
            response (redacted, cut). The tester approves every request before it is sent and \
            sees your reason. It is stored in the HTTP history. Use it to check behaviour \
            you need to understand (repeat a stored request, compare a header), never for \
            attack payloads, brute force or load.",
        parameters: r#"{"type":"object","properties":{"scheme":{"type":"string","enum":["http","https"]},"host":{"type":"string"},"port":{"type":"integer","minimum":1,"maximum":65535},"request":{"type":"string","description":"the raw request: request line, headers, empty line, body"},"reason":{"type":"string","description":"why this request is needed, shown to the tester"}},"required":["scheme","host","port","request","reason"],"additionalProperties":false}"#,
        kind: ToolKind::Send,
        default: Ask,
        highest: Ask,
    },
];

/// The catalog entry of a tool.
pub fn tool(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|t| t.name == name)
}

/// What the agent may do with `spec` under `config`: the configured or
/// default permission, never above the tool's highest.
pub fn effective(spec: &ToolSpec, config: &AgentConfig) -> AgentPermission {
    config
        .permissions
        .get(spec.name)
        .copied()
        .unwrap_or(spec.default)
        .min(spec.highest)
}

/// Checks agent settings against the catalog.
pub fn validate_config(config: &AgentConfig) -> Result<(), String> {
    for (name, permission) in &config.permissions {
        let spec = tool(name).ok_or_else(|| format!("there is no agent tool {name:?}"))?;
        if *permission > spec.highest {
            return Err(format!(
                "{name} can be at most {}",
                match spec.highest {
                    AgentPermission::Deny => "deny",
                    AgentPermission::Ask => "ask",
                    AgentPermission::Allow => "allow",
                }
            ));
        }
    }
    Ok(())
}

/// A tool as shown in the settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentToolInfo {
    /// Name.
    pub name: String,
    /// What it does.
    pub summary: String,
    /// What it touches.
    pub kind: ToolKind,
    /// Permission when not set.
    pub default: AgentPermission,
    /// Highest permission it can have.
    pub highest: AgentPermission,
    /// Permission under the current settings.
    pub permission: AgentPermission,
}

/// Every tool with its permission under `config`.
pub fn tool_infos(config: &AgentConfig) -> Vec<AgentToolInfo> {
    TOOLS
        .iter()
        .map(|t| AgentToolInfo {
            name: t.name.into(),
            summary: t.summary.into(),
            kind: t.kind,
            default: t.default,
            highest: t.highest,
            permission: effective(t, config),
        })
        .collect()
}

/// The tools the model is offered (those not denied).
pub fn offered(config: &AgentConfig) -> Vec<&'static ToolSpec> {
    TOOLS
        .iter()
        .filter(|t| effective(t, config) != AgentPermission::Deny)
        .collect()
}

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentRole {
    /// Instructions.
    System,
    /// The tester (the goal, queued messages) or a tool result in text
    /// mode.
    User,
    /// The model.
    Assistant,
    /// A tool result (native tool calling).
    Tool,
}

/// A tool call made by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AgentToolCall")]
pub struct ToolCall {
    /// Call id (made up when the server gives none).
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Arguments.
    #[ts(type = "unknown")]
    pub arguments: Value,
}

/// One message of an agent conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMessage {
    /// Who wrote it.
    pub role: AgentRole,
    /// Text.
    pub content: String,
    /// Tool calls of an assistant message (native mode).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// The call a tool result answers (native mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// The tool a result comes from (both modes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl AgentMessage {
    /// A plain message.
    pub fn text(role: AgentRole, content: impl Into<String>) -> AgentMessage {
        AgentMessage {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_name: None,
        }
    }

    /// The model's reply as it goes back into the conversation.
    pub fn assistant(reply: &AgentReply) -> AgentMessage {
        if reply.native {
            AgentMessage {
                tool_calls: reply.calls.clone(),
                ..AgentMessage::text(AgentRole::Assistant, reply.text.clone())
            }
        } else {
            AgentMessage::text(AgentRole::Assistant, reply.raw.clone())
        }
    }

    /// The result of a call (`native`: the call came as a native tool
    /// call; otherwise the result is a user message).
    pub fn result(call: &ToolCall, content: String, native: bool) -> AgentMessage {
        if native {
            AgentMessage {
                tool_call_id: Some(call.id.clone()),
                tool_name: Some(call.name.clone()),
                ..AgentMessage::text(AgentRole::Tool, content)
            }
        } else {
            AgentMessage {
                tool_name: Some(call.name.clone()),
                ..AgentMessage::text(
                    AgentRole::User,
                    format!("Result of the tool {}:\n{content}", call.name),
                )
            }
        }
    }
}

/// What the model answered.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentReply {
    /// Its text, without fenced tool calls.
    pub text: String,
    /// Its text as it came.
    pub raw: String,
    /// The tools it wants to call (none = it is done).
    pub calls: Vec<ToolCall>,
    /// The calls came as native tool calls (not as fenced JSON).
    pub native: bool,
}

/// The agent's instructions. `tools` are listed for models that cannot
/// call tools natively.
pub fn system_prompt(tools: &[&ToolSpec]) -> String {
    let list = tools
        .iter()
        .map(|t| {
            format!(
                "- {}: {} Arguments: {}",
                t.name, t.description, t.parameters
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "You are ChosAgent, the agent inside ChosProxy, a web security workbench used by a \
tester on systems they own or are authorized to test. You work on the tester's goal by calling \
ChosProxy's tools, one step at a time, and you finish with a clear answer.

Rules:
- You can only use the tools below. You have no shell, no files and no other access.
- Each call is checked by ChosProxy: some run directly, some wait for the tester's approval, \
some are refused. A refused or failed call is not an error of yours: adapt, or explain what the \
tester could do instead.
- Requests can only go to targets inside the scope, and the tester approves each one. Send only \
what you need to understand the application (repeating a stored request, comparing a header or a \
parameter's effect). Never send attack payloads, exploits, brute force, credential guessing or \
load, and do not write them.
- Values shown as [REDACTED:…] were hidden on purpose; do not try to guess them.
- Base what you say on the traffic; say when something is uncertain.
- Keep a short plan with update_plan when the work has several steps.
- When you are done (or blocked), stop calling tools and answer in Markdown: what you did, what \
you found (with history ids), and the manual checks the tester should do next. Answer in the \
language of the goal.

Tools:
{list}

If you cannot call tools natively, call one by replying with only a fenced code block whose \
language is tool, containing {{\"name\": \"<tool>\", \"arguments\": {{…}}}}."
    )
}

/// The first user message: the goal and the attached items.
pub fn goal_message(goal: &str, context: &Context) -> String {
    let mut text = format!("Goal: {}\n", goal.trim());
    if !context.text.is_empty() {
        text.push_str(&format!("\nAttached items:\n\n{}\n", context.text));
    }
    if context.truncated {
        text.push_str("\n(Some attached text was cut to fit.)\n");
    }
    text
}

fn provider_message(provider: AiProvider, m: &AgentMessage) -> Value {
    let role = match m.role {
        AgentRole::System => "system",
        AgentRole::User => "user",
        AgentRole::Assistant => "assistant",
        AgentRole::Tool => "tool",
    };
    let mut v = json!({ "role": role, "content": m.content });
    match provider {
        AiProvider::Ollama => {
            if !m.tool_calls.is_empty() {
                v["tool_calls"] = m
                    .tool_calls
                    .iter()
                    .map(|c| json!({ "function": { "name": c.name, "arguments": c.arguments } }))
                    .collect();
            }
            if m.role == AgentRole::Tool
                && let Some(name) = &m.tool_name
            {
                v["tool_name"] = json!(name);
            }
        }
        AiProvider::OpenAiCompatible => {
            if !m.tool_calls.is_empty() {
                v["tool_calls"] = m
                    .tool_calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": { "name": c.name, "arguments": c.arguments.to_string() }
                        })
                    })
                    .collect();
            }
            if let Some(id) = &m.tool_call_id {
                v["tool_call_id"] = json!(id);
            }
        }
    }
    v
}

/// The JSON body of a chat request with tools (no streaming).
pub fn agent_body(
    provider: AiProvider,
    model: &str,
    messages: &[AgentMessage],
    tools: &[&ToolSpec],
) -> String {
    let msgs: Vec<Value> = messages
        .iter()
        .map(|m| provider_message(provider, m))
        .collect();
    let mut body = match provider {
        AiProvider::Ollama => json!({ "model": model, "messages": msgs, "stream": false }),
        AiProvider::OpenAiCompatible => {
            json!({ "model": model, "messages": msgs, "stream": false, "temperature": 0.2 })
        }
    };
    if !tools.is_empty() {
        body["tools"] = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": serde_json::from_str::<Value>(t.parameters)
                            .unwrap_or_else(|_| json!({ "type": "object" })),
                    }
                })
            })
            .collect();
    }
    body.to_string()
}

fn arguments_of(v: Option<&Value>) -> Value {
    match v {
        Some(Value::String(s)) if s.trim().is_empty() => json!({}),
        Some(Value::String(s)) => {
            serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone()))
        }
        Some(v) => v.clone(),
        None => json!({}),
    }
}

/// Tool calls written as fenced ```tool blocks, and the text around them.
fn fenced_calls(text: &str, step: u32) -> (String, Vec<ToolCall>) {
    let mut calls = Vec::new();
    let mut rest = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("```tool") {
        let after = &remaining[start + 7..];
        let Some(end) = after.find("```") else {
            break;
        };
        rest.push_str(&remaining[..start]);
        let body = after[..end].trim();
        if let Ok(v) = serde_json::from_str::<Value>(body)
            && let Some(name) = v
                .get("name")
                .or_else(|| v.get("tool"))
                .and_then(Value::as_str)
        {
            calls.push(ToolCall {
                id: format!("call_{step}_{}", calls.len()),
                name: name.to_string(),
                arguments: arguments_of(v.get("arguments")),
            });
        }
        remaining = &after[end + 3..];
    }
    rest.push_str(remaining);
    (rest.trim().to_string(), calls)
}

/// Reads a chat reply with tools. `step` numbers made-up call ids.
pub fn parse_agent_reply(
    provider: AiProvider,
    body: &str,
    step: u32,
) -> Result<AgentReply, String> {
    let value = crate::reply_json(body)?;
    let message = match provider {
        AiProvider::Ollama => value.get("message"),
        AiProvider::OpenAiCompatible => value.pointer("/choices/0/message"),
    }
    .ok_or_else(|| format!("no message in the reply: {}", crate::preview(body)))?;
    let raw = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let mut calls: Vec<ToolCall> = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .enumerate()
                .filter_map(|(i, c)| {
                    let f = c.get("function")?;
                    Some(ToolCall {
                        id: c
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map_or_else(|| format!("call_{step}_{i}"), str::to_string),
                        name: f.get("name")?.as_str()?.to_string(),
                        arguments: arguments_of(f.get("arguments")),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let native = !calls.is_empty();
    let text = if native {
        raw.clone()
    } else {
        let (text, fenced) = fenced_calls(&raw, step);
        calls = fenced;
        text
    };
    calls.truncate(MAX_CALLS_PER_REPLY);
    Ok(AgentReply {
        text,
        raw,
        calls,
        native,
    })
}

/// Placeholder for a tool result dropped to save space.
pub const DROPPED: &str = "[older tool result removed to stay within the context limit]";

/// Drops the oldest tool results until the conversation is under `limit`
/// characters (the instructions, the goal and the last four messages are
/// kept).
pub fn compact(messages: &mut [AgentMessage], limit: usize) {
    let size = |m: &[AgentMessage]| m.iter().map(|x| x.content.chars().count()).sum::<usize>();
    let keep_from = messages.len().saturating_sub(4);
    let mut total = size(messages);
    for m in messages.iter_mut().take(keep_from).skip(2) {
        if total <= limit {
            break;
        }
        if m.tool_name.is_some() && m.content != DROPPED {
            total = total - m.content.chars().count() + DROPPED.chars().count();
            m.content = DROPPED.to_string();
        }
    }
}

/// A digest of a conversation, so the engine can check that an external
/// call sends exactly what the tester approved.
pub fn agent_digest(messages: &[AgentMessage]) -> String {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    for m in messages {
        ctx.update(serde_json::to_string(m).unwrap_or_default().as_bytes());
        ctx.update(&[0]);
    }
    ctx.finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn catalog_is_well_formed_and_sending_is_capped_at_ask() {
        let mut names = std::collections::HashSet::new();
        for t in TOOLS {
            assert!(names.insert(t.name), "duplicate {}", t.name);
            let schema: Value = serde_json::from_str(t.parameters).unwrap();
            assert_eq!(schema["type"], "object", "{}", t.name);
            assert!(t.default <= t.highest);
            if t.kind == ToolKind::Send {
                assert_eq!(t.highest, AgentPermission::Ask, "{}", t.name);
            }
        }
        let mut config = AgentConfig::default();
        let send = tool("send_request").unwrap();
        assert_eq!(effective(send, &config), AgentPermission::Ask);
        // A hand-edited agent.toml cannot lift the cap.
        config
            .permissions
            .insert("send_request".into(), AgentPermission::Allow);
        assert_eq!(effective(send, &config), AgentPermission::Ask);
        assert!(
            validate_config(&config)
                .unwrap_err()
                .contains("at most ask")
        );
        config.permissions = BTreeMap::from([
            ("send_request".into(), AgentPermission::Deny),
            ("add_note".into(), AgentPermission::Allow),
        ]);
        assert!(validate_config(&config).is_ok());
        assert!(!offered(&config).iter().any(|t| t.name == "send_request"));
        assert_eq!(
            effective(tool("add_note").unwrap(), &config),
            AgentPermission::Allow
        );
        config
            .permissions
            .insert("run_shell".into(), AgentPermission::Allow);
        assert!(
            validate_config(&config)
                .unwrap_err()
                .contains("no agent tool")
        );
        assert!(tool("run_shell").is_none());
        let infos = tool_infos(&AgentConfig::default());
        assert_eq!(infos.len(), TOOLS.len());
    }

    #[test]
    fn bodies_carry_tools_and_calls_in_each_format() {
        let call = ToolCall {
            id: "c1".into(),
            name: "get_exchange".into(),
            arguments: json!({ "id": 3 }),
        };
        let reply = AgentReply {
            text: String::new(),
            raw: String::new(),
            calls: vec![call.clone()],
            native: true,
        };
        let msgs = vec![
            AgentMessage::text(AgentRole::System, "s"),
            AgentMessage::text(AgentRole::User, "goal"),
            AgentMessage::assistant(&reply),
            AgentMessage::result(&call, "GET /".into(), true),
        ];
        let tools = vec![tool("get_exchange").unwrap()];
        let o: Value =
            serde_json::from_str(&agent_body(AiProvider::Ollama, "m", &msgs, &tools)).unwrap();
        assert_eq!(o["tools"][0]["function"]["name"], "get_exchange");
        assert_eq!(o["tools"][0]["function"]["parameters"]["required"][0], "id");
        assert_eq!(
            o["messages"][2]["tool_calls"][0]["function"]["arguments"]["id"],
            3
        );
        assert_eq!(o["messages"][3]["role"], "tool");
        assert_eq!(o["messages"][3]["tool_name"], "get_exchange");
        let a: Value = serde_json::from_str(&agent_body(
            AiProvider::OpenAiCompatible,
            "m",
            &msgs,
            &tools,
        ))
        .unwrap();
        assert_eq!(a["messages"][2]["tool_calls"][0]["id"], "c1");
        assert_eq!(
            a["messages"][2]["tool_calls"][0]["function"]["arguments"],
            "{\"id\":3}"
        );
        assert_eq!(a["messages"][3]["tool_call_id"], "c1");
        let none: Value =
            serde_json::from_str(&agent_body(AiProvider::Ollama, "m", &msgs, &[])).unwrap();
        assert!(none.get("tools").is_none());
        // Text mode: the result is a user message.
        let text = AgentMessage::result(&call, "x".into(), false);
        assert_eq!(text.role, AgentRole::User);
        assert!(text.content.starts_with("Result of the tool get_exchange"));
    }

    #[test]
    fn reads_native_and_fenced_tool_calls() {
        let o = parse_agent_reply(
            AiProvider::Ollama,
            r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"get_scope","arguments":{}}}]}}"#,
            4,
        )
        .unwrap();
        assert!(o.native);
        assert_eq!(o.calls[0].name, "get_scope");
        assert_eq!(o.calls[0].id, "call_4_0");
        let a = parse_agent_reply(
            AiProvider::OpenAiCompatible,
            r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"x","type":"function","function":{"name":"get_exchange","arguments":"{\"id\":7}"}}]}}]}"#,
            1,
        )
        .unwrap();
        assert_eq!(a.calls[0].id, "x");
        assert_eq!(a.calls[0].arguments["id"], 7);
        let f = parse_agent_reply(
            AiProvider::Ollama,
            "{\"message\":{\"content\":\"Let me look.\\n```tool\\n{\\\"name\\\": \\\"list_findings\\\", \\\"arguments\\\": {}}\\n```\"}}",
            2,
        )
        .unwrap();
        assert!(!f.native);
        assert_eq!(f.text, "Let me look.");
        assert_eq!(f.calls[0].name, "list_findings");
        // A JSON example in a final answer is not a call.
        let done = parse_agent_reply(
            AiProvider::Ollama,
            "{\"message\":{\"content\":\"Done.\\n```json\\n{\\\"name\\\":\\\"x\\\"}\\n```\"}}",
            3,
        )
        .unwrap();
        assert!(done.calls.is_empty());
        assert!(parse_agent_reply(AiProvider::Ollama, r#"{"error":"no tools"}"#, 1).is_err());
    }

    #[test]
    fn compaction_drops_old_tool_results_first() {
        let call = ToolCall {
            id: "c".into(),
            name: "get_exchange".into(),
            arguments: json!({}),
        };
        let mut msgs = vec![
            AgentMessage::text(AgentRole::System, "s"),
            AgentMessage::text(AgentRole::User, "goal"),
        ];
        for _ in 0..6 {
            msgs.push(AgentMessage::result(&call, "x".repeat(1000), true));
        }
        compact(&mut msgs, 3000);
        let dropped = msgs.iter().filter(|m| m.content == DROPPED).count();
        assert!(dropped >= 2);
        assert_eq!(msgs[1].content, "goal");
        assert_ne!(msgs[7].content, DROPPED);
        let before = agent_digest(&msgs);
        msgs[1].content.push('!');
        assert_ne!(before, agent_digest(&msgs));
    }
}
