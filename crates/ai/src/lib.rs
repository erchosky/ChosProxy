//! ChosAI (M29, ADR-0039): everything about talking to a model that does
//! not need I/O — what goes into the context (and what is redacted from
//! it), the task prompts, and the request and answer formats of Ollama and
//! OpenAI-compatible servers. The engine does the sending.
//!
//! ChosAI only writes text for the tester: explanations, summaries,
//! suggested manual checks and report wording. It never acts.

pub mod agent;
pub mod redact;
pub mod stream;

use chosproxy_core::config::AiProvider;
use redact::{RedactRules, Redaction, redact};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

/// Most characters kept from one message (request or response) before the
/// whole context is limited.
const PART_LIMIT: usize = 60_000;

/// What the tester asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AiTask {
    /// What a request does and what to look at in it.
    ExplainRequest,
    /// What a response tells (headers, body, errors, security headers).
    ExplainResponse,
    /// A request with its response.
    ExplainExchange,
    /// A finding: why it matters, how to confirm it by hand, how to fix it.
    ExplainFinding,
    /// A piece of JavaScript.
    ExplainJavascript,
    /// How a login works across requests, and a session rule to keep a
    /// tool logged in.
    LoginFlow,
    /// Report wording for a finding (description, impact, remediation).
    DraftFinding,
    /// The tester's own question about the attached items.
    Ask,
}

/// One message to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AiMessage")]
pub struct ChatMessage {
    /// `system`, `user` or `assistant`.
    pub role: String,
    /// Text.
    pub content: String,
}

/// One attached item, before redaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextItem {
    /// What it is ("Request #12", "Finding #3", …).
    pub label: String,
    /// Its text.
    pub text: String,
}

/// The attached items as sent: redacted and limited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AiContext")]
pub struct Context {
    /// The text the model sees.
    pub text: String,
    /// What was hidden.
    pub redactions: Vec<Redaction>,
    /// Some text was cut to stay under the limit.
    pub truncated: bool,
}

/// `text` cut to `limit` characters (on a character boundary), and
/// whether it was cut.
fn cut(text: &str, limit: usize) -> (String, bool) {
    match text.char_indices().nth(limit) {
        Some((at, _)) => (
            format!(
                "{}\n[… cut: {} more characters]",
                &text[..at],
                text[at..].chars().count()
            ),
            true,
        ),
        None => (text.to_string(), false),
    }
}

/// A message as text: head kept, a binary body replaced by its size.
fn message_text(raw: &[u8]) -> String {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|i| i + 2))
        .unwrap_or(raw.len());
    let (head, body) = raw.split_at(split);
    let head = String::from_utf8_lossy(head).into_owned();
    match std::str::from_utf8(body) {
        Ok(text) if !text.contains('\0') => head + text,
        _ => format!("{head}[binary body, {} bytes]", body.len()),
    }
}

/// An exchange as context (the response may be missing).
pub fn exchange_item(label: &str, request: &[u8], response: Option<&[u8]>) -> ContextItem {
    let (req, _) = cut(&message_text(request), PART_LIMIT);
    let mut text = format!("--- request ---\n{req}");
    match response {
        Some(r) => {
            let (res, _) = cut(&message_text(r), PART_LIMIT);
            text.push_str(&format!("\n--- response ---\n{res}"));
        }
        None => text.push_str("\n--- no response ---"),
    }
    ContextItem {
        label: label.to_string(),
        text,
    }
}

/// The items redacted and joined, within `limit` characters.
pub fn assemble(items: &[ContextItem], rules: &RedactRules, limit: usize) -> Context {
    let joined = items
        .iter()
        .map(|i| format!("=== {} ===\n{}", i.label, i.text.trim_end()))
        .collect::<Vec<_>>()
        .join("\n\n");
    let (redacted, redactions) = redact(&joined, rules);
    let (text, truncated) = cut(&redacted, limit);
    Context {
        text,
        redactions,
        truncated,
    }
}

/// What ChosAI is told about its role.
pub const SYSTEM_PROMPT: &str = "You are ChosAI, the assistant inside ChosProxy, a web security \
workbench used by testers on systems they own or are authorized to test. Explain clearly and \
concretely, for someone learning web security as well as for experts. Base what you say on the \
attached traffic and findings; say when something is uncertain or not visible in them, and do \
not invent headers, parameters or behaviour. Prefer suggesting manual checks the tester can do \
with ChosProxy's tools (Repeater, Inspector, Comparer, Decoder) and concrete remediation. Do not \
write exploit code or attack payloads; you cannot send requests or change anything. Values shown \
as [REDACTED:…] were hidden on purpose: do not try to guess them. Answer in Markdown, in the \
language of the tester's question (English if there is none).";

fn instruction(task: AiTask) -> &'static str {
    match task {
        AiTask::ExplainRequest => {
            "Explain this request: what it does, the role of its method, \
            path, parameters, headers and cookies, how it is authenticated, and which inputs \
            deserve a closer look during an authorized test, with why."
        }
        AiTask::ExplainResponse => {
            "Explain this response: status, important headers (caching, \
            cookies and their attributes, security headers present or missing), what the body \
            reveals, and anything a tester should note."
        }
        AiTask::ExplainExchange => {
            "Explain this exchange: what the client asked for, what the \
            server answered, how they relate, and what a tester should look at next (as \
            manual checks)."
        }
        AiTask::ExplainFinding => {
            "Explain this finding: what the problem is, why it matters in \
            this application (realistic impact), how to confirm it manually with the evidence \
            shown, what could make it a false positive, and how to fix it."
        }
        AiTask::ExplainJavascript => {
            "Explain this JavaScript: what it does, which endpoints or \
            data it uses, where untrusted input could reach dangerous sinks (innerHTML, eval, \
            document.write, redirects), and what to verify manually."
        }
        AiTask::LoginFlow => {
            "These requests come from a login. Explain the flow step by step: \
            which request sends the credentials, which responses set session cookies or tokens \
            (and where tokens such as CSRF values are read from), and what a later request \
            needs to stay logged in. Then propose a ChosProxy session rule: the requests to \
            replay as the login macro, the variables to extract (with a regular expression), \
            the headers to set, and a check that tells when the session expired. End with that rule as \
            a fenced code block whose language is chosproxy-session-rule, containing only JSON \
            with the fields name, hosts (host patterns), tools (empty = all), useCookieJar, \
            expiredStatuses (numbers), expiredRegex, macroRequests (the request numbers), \
            extract ([{name, regex}]) and headers ([{name, value}], values may use {{name}})."
        }
        AiTask::DraftFinding => {
            "Write report text for this finding, as Markdown with the \
            sections Description, Impact, Evidence (refer to the attached requests, keep \
            redacted values redacted) and Remediation. Be factual and concise."
        }
        AiTask::Ask => "Answer the tester's question about the attached items.",
    }
}

/// The messages for a task, a question (optional, required for
/// [`AiTask::Ask`]) and the context.
pub fn messages(
    task: AiTask,
    question: Option<&str>,
    context: &Context,
) -> Result<Vec<ChatMessage>, String> {
    let question = question.map(str::trim).filter(|q| !q.is_empty());
    if task == AiTask::Ask && question.is_none() {
        return Err("type a question".into());
    }
    let mut user = format!("{}\n", instruction(task));
    if let Some(q) = question {
        user.push_str(&format!("\nThe tester asks: {q}\n"));
    }
    if context.text.is_empty() {
        user.push_str("\n(No items are attached.)\n");
    } else {
        user.push_str(&format!("\nAttached items:\n\n{}\n", context.text));
    }
    if context.truncated {
        user.push_str("\n(Some attached text was cut to fit; mention it if it matters.)\n");
    }
    Ok(vec![
        ChatMessage {
            role: "system".into(),
            content: SYSTEM_PROMPT.into(),
        },
        ChatMessage {
            role: "user".into(),
            content: user,
        },
    ])
}

/// A short digest of messages, so the engine can check that an external
/// call sends exactly what the tester previewed.
pub fn digest(messages: &[ChatMessage]) -> String {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    for m in messages {
        ctx.update(m.role.as_bytes());
        ctx.update(&[0]);
        ctx.update(m.content.as_bytes());
        ctx.update(&[0]);
    }
    ctx.finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The path of the chat endpoint under the base URL.
pub fn chat_path(provider: AiProvider) -> &'static str {
    match provider {
        AiProvider::Ollama => "/api/chat",
        AiProvider::OpenAiCompatible => "/v1/chat/completions",
    }
}

/// The JSON body of a chat request (no streaming).
pub fn chat_body(provider: AiProvider, model: &str, messages: &[ChatMessage]) -> String {
    let msgs: Vec<Value> = messages
        .iter()
        .map(|m| json!({ "role": m.role, "content": m.content }))
        .collect();
    match provider {
        AiProvider::Ollama => json!({ "model": model, "messages": msgs, "stream": false }),
        AiProvider::OpenAiCompatible => {
            json!({ "model": model, "messages": msgs, "stream": false, "temperature": 0.2 })
        }
    }
    .to_string()
}

/// The answer text of a chat response body.
pub fn parse_reply(provider: AiProvider, body: &str) -> Result<String, String> {
    let value = reply_json(body)?;
    let text = match provider {
        AiProvider::Ollama => value.pointer("/message/content"),
        AiProvider::OpenAiCompatible => value.pointer("/choices/0/message/content"),
    }
    .and_then(Value::as_str)
    .ok_or_else(|| format!("no answer text in the reply: {}", preview(body)))?;
    Ok(text.trim().to_string())
}

/// A reply body as JSON, or the error the server reported.
pub(crate) fn reply_json(body: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(body.trim()).map_err(|_| {
        format!(
            "the model server did not answer with JSON: {}",
            preview(body)
        )
    })?;
    if let Some(e) = value.get("error") {
        let message = e
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| e.as_str())
            .unwrap_or("unknown error");
        return Err(format!("the model server reported: {message}"));
    }
    Ok(value)
}

pub(crate) fn preview(body: &str) -> String {
    cut(body.trim(), 200).0
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn builds_redacted_limited_context() {
        let item = exchange_item(
            "Request #1",
            b"GET /a?token=xyz HTTP/1.1\r\nHost: t\r\nCookie: s=1\r\n\r\n",
            Some(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n\r\n\x89PNG\x00\x01"),
        );
        assert!(item.text.contains("[binary body, 6 bytes]"));
        let ctx = assemble(std::slice::from_ref(&item), &RedactRules::default(), 10_000);
        assert!(
            ctx.text
                .starts_with("=== Request #1 ===\n--- request ---\nGET /a?token=[REDACTED:secret]")
        );
        assert!(!ctx.text.contains("s=1"));
        assert!(!ctx.truncated);
        assert_eq!(ctx.redactions.len(), 2);
        let short = assemble(&[item], &RedactRules::default(), 20);
        assert!(short.truncated && short.text.contains("[… cut:"));
        let none = exchange_item("R", b"POST / HTTP/1.1\n\n", None);
        assert!(none.text.ends_with("--- no response ---"));
    }

    #[test]
    fn prompts_need_a_question_only_for_ask() {
        let ctx = Context {
            text: "=== R ===\nGET /".into(),
            redactions: vec![],
            truncated: true,
        };
        let m = messages(AiTask::ExplainRequest, None, &ctx).unwrap();
        assert_eq!(m[0].role, "system");
        assert!(m[0].content.contains("Do not write exploit code"));
        assert!(m[1].content.contains("Explain this request") && m[1].content.contains("GET /"));
        assert!(m[1].content.contains("was cut"));
        assert!(messages(AiTask::Ask, Some("  "), &ctx).is_err());
        let q = messages(AiTask::Ask, Some("¿Qué hace?"), &ctx).unwrap();
        assert!(q[1].content.contains("The tester asks: ¿Qué hace?"));
        assert_ne!(digest(&m), digest(&q));
        assert_eq!(digest(&m), digest(&m.clone()));
        assert_eq!(digest(&m).len(), 64);
    }

    #[test]
    fn speaks_ollama_and_openai_formats() {
        let m = vec![ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let o: Value =
            serde_json::from_str(&chat_body(AiProvider::Ollama, "llama3.1", &m)).unwrap();
        assert_eq!(o["stream"], false);
        assert_eq!(o["messages"][0]["content"], "hi");
        let a: Value =
            serde_json::from_str(&chat_body(AiProvider::OpenAiCompatible, "x", &m)).unwrap();
        assert_eq!(a["model"], "x");
        assert_eq!(
            chat_path(AiProvider::OpenAiCompatible),
            "/v1/chat/completions"
        );
        assert_eq!(
            parse_reply(
                AiProvider::Ollama,
                r#"{"message":{"role":"assistant","content":" Hi! "}}"#
            )
            .unwrap(),
            "Hi!"
        );
        assert_eq!(
            parse_reply(
                AiProvider::OpenAiCompatible,
                r#"{"choices":[{"message":{"content":"Yo"}}]}"#
            )
            .unwrap(),
            "Yo"
        );
        assert!(
            parse_reply(AiProvider::Ollama, r#"{"error":"model not found"}"#)
                .unwrap_err()
                .contains("model not found")
        );
        assert!(
            parse_reply(
                AiProvider::OpenAiCompatible,
                r#"{"error":{"message":"bad key"}}"#
            )
            .unwrap_err()
            .contains("bad key")
        );
        assert!(parse_reply(AiProvider::Ollama, "<html>").is_err());
        assert!(parse_reply(AiProvider::Ollama, "{}").is_err());
    }
}
