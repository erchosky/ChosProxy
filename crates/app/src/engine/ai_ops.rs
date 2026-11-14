//! ChosAI in the engine (M29, ADR-0039): settings, building the context
//! from attached items (redacted, limited), the preview, and the call to
//! the model through the Request Engine (network settings apply).
//!
//! - Off unless the tester turns it on (`ai.toml`).
//! - Only the attached items are sent, never the project.
//! - A model that is not on this machine is only called with a context
//!   the tester previewed: the call carries the digest of the previewed
//!   messages and is refused if they differ.
//! - Calls are not stored in the history; the audit log records the task,
//!   the model and the size, not the content.

use super::Engine;
use super::lock;
use crate::AppError;
use chosproxy_ai::redact::RedactRules;
use chosproxy_ai::{
    AiTask, ChatMessage, Context, ContextItem, assemble, chat_body, chat_path, digest,
    exchange_item, messages, parse_reply,
};
use chosproxy_core::config::{AiConfig, AiMode, ConfigSection};
use chosproxy_http::ResponseHead;
use chosproxy_http::content::{body_text, split_message};
use chosproxy_request::{Destination, SendLimits};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ts_rs::TS;

/// Most items attached to one question.
const MAX_ATTACHMENTS: usize = 20;
/// Largest attached text.
const MAX_TEXT: usize = 400_000;
/// Largest answer read.
const MAX_ANSWER: usize = 4 * 1024 * 1024;

/// Something attached to a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum AiAttachment {
    /// A stored request and its response.
    Exchange {
        /// History id.
        #[ts(type = "number")]
        id: i64,
    },
    /// A finding with its first evidence.
    Finding {
        /// Finding id.
        #[ts(type = "number")]
        id: i64,
    },
    /// Text from an editor (a Repeater draft, a JavaScript selection…).
    Text {
        /// What it is.
        label: String,
        /// The text.
        text: String,
    },
}

/// A question to ChosAI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiRequest {
    /// What is asked.
    pub task: AiTask,
    /// The tester's question (needed for `ask`).
    pub question: Option<String>,
    /// The items the question is about.
    pub attachments: Vec<AiAttachment>,
    /// A conversation (M30): the turns after the first question, oldest
    /// first — the model's answers and the tester's follow-up questions,
    /// alternating and ending with a follow-up. Empty for a first question.
    #[serde(default)]
    pub conversation: Vec<ChatMessage>,
}

/// Most turns in a conversation.
const MAX_TURNS: usize = 40;
/// Longest turn.
const MAX_TURN: usize = 40_000;

/// Checks the turns of a conversation and redacts the tester's.
fn conversation(turns: &[ChatMessage], rules: &RedactRules) -> Result<Vec<ChatMessage>, AppError> {
    if turns.len() > MAX_TURNS {
        return Err(AppError::Invalid(format!(
            "a conversation keeps at most {MAX_TURNS} turns: start a new one"
        )));
    }
    let mut out = Vec::with_capacity(turns.len());
    for (i, t) in turns.iter().enumerate() {
        let want = if i % 2 == 0 { "assistant" } else { "user" };
        if t.role != want {
            return Err(AppError::Invalid(
                "a conversation alternates answers and follow-up questions".into(),
            ));
        }
        if t.content.chars().count() > MAX_TURN {
            return Err(AppError::Invalid(
                "a turn of the conversation is too long".into(),
            ));
        }
        let content = if t.role == "user" {
            if t.content.trim().is_empty() {
                return Err(AppError::Invalid("type a follow-up question".into()));
            }
            chosproxy_ai::redact::redact(&t.content, rules).0
        } else {
            t.content.clone()
        };
        out.push(ChatMessage {
            role: t.role.clone(),
            content,
        });
    }
    if out.last().is_some_and(|t| t.role != "user") {
        return Err(AppError::Invalid(
            "a conversation ends with a follow-up question".into(),
        ));
    }
    Ok(out)
}

/// Exactly what would be sent, before sending it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiPreview {
    /// The messages, as the model would receive them.
    pub messages: Vec<ChatMessage>,
    /// The context and what was hidden in it.
    pub context: Context,
    /// Digest to pass back when sending (required for external models).
    pub digest: String,
    /// The model would be called outside this machine.
    pub external: bool,
    /// ChosAI is turned on.
    pub enabled: bool,
    /// "Ollama llama3.1 at http://127.0.0.1:11434" and the like.
    pub target: String,
}

/// The model's answer.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiAnswer {
    /// The answer (Markdown).
    pub text: String,
    /// Model that answered.
    pub model: String,
    /// Time taken, in ms.
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    /// Characters sent.
    #[ts(type = "number")]
    pub sent_chars: u64,
}

/// The requests of a recorded login (M31b, AI-004).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoginRecording {
    /// History ids to attach to "Explain a login flow", oldest first.
    #[ts(type = "number[]")]
    pub ids: Vec<i64>,
    /// Proxy requests seen while recording.
    pub seen: u32,
    /// Of those, left out (static files, or more than fit).
    pub skipped: u32,
}

/// Most proxy requests looked at for one recorded login.
const MAX_RECORDED: u32 = 2_000;

/// Path extensions that are not part of a login.
const STATIC_EXTENSIONS: &[&str] = &[
    "js", "mjs", "css", "png", "jpg", "jpeg", "gif", "svg", "ico", "webp", "avif", "woff", "woff2",
    "ttf", "otf", "eot", "map", "mp4", "webm", "mp3",
];

/// Picks the requests of a login among what the proxy saw: no static
/// files; when more than `most` are left, requests that are not GET first
/// (the form posts), then the earliest others. Ids come back in order.
fn pick_login_requests(rows: &[(i64, String, Option<String>)], most: usize) -> Vec<i64> {
    let dynamic: Vec<&(i64, String, Option<String>)> = rows
        .iter()
        .filter(|(_, method, ext)| {
            !(method.eq_ignore_ascii_case("GET")
                && ext
                    .as_deref()
                    .is_some_and(|e| STATIC_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str())))
                && !method.eq_ignore_ascii_case("OPTIONS")
                && !method.eq_ignore_ascii_case("CONNECT")
        })
        .collect();
    let mut ids: Vec<i64> = dynamic
        .iter()
        .filter(|(_, m, _)| !m.eq_ignore_ascii_case("GET"))
        .map(|(id, _, _)| *id)
        .take(most)
        .collect();
    for (id, m, _) in &dynamic {
        if ids.len() >= most {
            break;
        }
        if m.eq_ignore_ascii_case("GET") {
            ids.push(*id);
        }
    }
    ids.sort_unstable();
    ids
}

pub(super) fn describe(config: &AiConfig) -> String {
    format!(
        "{} {} at {}",
        match config.provider {
            chosproxy_core::config::AiProvider::Ollama => "Ollama",
            chosproxy_core::config::AiProvider::OpenAiCompatible => "OpenAI-compatible",
        },
        config.model,
        config.base_url
    )
}

impl Engine {
    /// ChosAI settings.
    pub fn ai_config(&self) -> AiConfig {
        self.inner.store.load::<AiConfig>().value
    }

    /// Saves ChosAI settings (patterns are checked).
    pub fn save_ai_config(&self, config: AiConfig) -> Result<AiConfig, AppError> {
        config.validate().map_err(AppError::Invalid)?;
        RedactRules::new(&config.redact_patterns).map_err(AppError::Invalid)?;
        self.inner.store.save(&config)?;
        self.audit(
            "ai.settings",
            &format!(
                "ChosAI {} ({})",
                match config.mode {
                    AiMode::Disabled => "off",
                    AiMode::Local => "local",
                    AiMode::External => "external",
                },
                describe(&config)
            ),
        );
        Ok(config)
    }

    /// Starts recording a login: remembers where the history is now. The
    /// tester then logs in through the proxy (the lab browser); nothing is
    /// sent or replayed by ChosProxy. Returns the last history id.
    pub async fn login_recording_start(&self) -> Result<i64, AppError> {
        let db = self.db()?;
        let last = tokio::task::spawn_blocking(move || db.last_exchange_id()).await??;
        *lock(&self.inner.login_recording) = Some(last);
        Ok(last)
    }

    /// The history id a login recording started after, if one is running.
    pub fn login_recording(&self) -> Option<i64> {
        *lock(&self.inner.login_recording)
    }

    /// Ends the recording and returns the proxy requests made since it
    /// started that belong to a login, for "Explain a login flow".
    pub async fn login_recording_stop(&self) -> Result<LoginRecording, AppError> {
        let after = lock(&self.inner.login_recording)
            .take()
            .ok_or_else(|| AppError::Invalid("no login is being recorded".into()))?;
        let db = self.db()?;
        let rows =
            tokio::task::spawn_blocking(move || db.proxy_exchanges_after(after, MAX_RECORDED))
                .await??;
        let ids = pick_login_requests(&rows, MAX_ATTACHMENTS);
        let seen = u32::try_from(rows.len()).unwrap_or(u32::MAX);
        Ok(LoginRecording {
            skipped: seen.saturating_sub(u32::try_from(ids.len()).unwrap_or(u32::MAX)),
            ids,
            seen,
        })
    }

    pub(super) async fn ai_item(&self, attachment: &AiAttachment) -> Result<ContextItem, AppError> {
        match attachment {
            AiAttachment::Exchange { id } => {
                let db = self.db()?;
                let id = *id;
                let ex = tokio::task::spawn_blocking(move || db.exchanges_bulk(&[id]))
                    .await??
                    .pop()
                    .ok_or(AppError::UnknownExchange(id))?;
                Ok(exchange_item(
                    &format!("Request #{id}: {} {}", ex.row.method, ex.row.url),
                    &ex.request,
                    ex.response.as_deref(),
                ))
            }
            AiAttachment::Finding { id } => {
                let detail = self.finding_detail(*id).await?;
                let row = &detail.row;
                let mut text = format!(
                    "Title: {}\nType: {}\nSeverity: {}\nConfidence: {}\nStatus: {}\nURL: {}\n\n\
                     Description:\n{}\n\nRemediation:\n{}\n",
                    row.title,
                    row.kind,
                    row.severity.as_str(),
                    row.confidence.as_str(),
                    row.status.as_str(),
                    row.url,
                    detail.description,
                    detail.remediation
                );
                if !detail.notes.trim().is_empty() {
                    text.push_str(&format!("\nTester notes:\n{}\n", detail.notes));
                }
                if let Some(ev) = detail.evidence.first() {
                    let decode = |b64: &str| {
                        use base64::Engine as _;
                        base64::engine::general_purpose::STANDARD
                            .decode(b64)
                            .unwrap_or_default()
                    };
                    let request = decode(&ev.request.raw_base64);
                    let response = ev.response.as_ref().map(|r| decode(&r.raw_base64));
                    if !ev.row.note.is_empty() {
                        text.push_str(&format!("\nEvidence note: {}\n", ev.row.note));
                    }
                    text.push_str("\nEvidence:\n");
                    text.push_str(&exchange_item("evidence", &request, response.as_deref()).text);
                }
                Ok(ContextItem {
                    label: format!("Finding #{id}"),
                    text,
                })
            }
            AiAttachment::Text { label, text } => {
                if text.len() > MAX_TEXT {
                    return Err(AppError::Invalid("the attached text is too large".into()));
                }
                Ok(ContextItem {
                    label: label.chars().take(200).collect(),
                    text: text.clone(),
                })
            }
        }
    }

    /// What would be sent for a question, redacted and limited.
    pub async fn ai_preview(&self, request: &AiRequest) -> Result<AiPreview, AppError> {
        if request.attachments.len() > MAX_ATTACHMENTS {
            return Err(AppError::Invalid(format!(
                "at most {MAX_ATTACHMENTS} attached items"
            )));
        }
        let config = self.ai_config();
        let rules = RedactRules::new(&config.redact_patterns).map_err(AppError::Invalid)?;
        let mut items = Vec::with_capacity(request.attachments.len());
        for a in &request.attachments {
            items.push(self.ai_item(a).await?);
        }
        let context = assemble(&items, &rules, config.max_context_chars as usize);
        let mut messages = messages(request.task, request.question.as_deref(), &context)
            .map_err(AppError::Invalid)?;
        messages.extend(conversation(&request.conversation, &rules)?);
        Ok(AiPreview {
            digest: digest(&messages),
            messages,
            context,
            external: !config.is_loopback(),
            enabled: config.mode != AiMode::Disabled,
            target: describe(&config),
        })
    }

    /// Asks the model. For a model outside this machine, `approved` must be
    /// the digest of a preview of the same question.
    pub async fn ai_run(
        &self,
        request: &AiRequest,
        approved: Option<&str>,
    ) -> Result<AiAnswer, AppError> {
        let (config, preview) = self.ai_checked(request, approved).await?;
        let answer = self.ai_call(&config, &preview.messages).await?;
        self.ai_asked(request, &config, &preview, &answer, "").await;
        Ok(answer)
    }

    /// Like [`Engine::ai_run`], and hands the answer to `on_delta` piece by
    /// piece while the model writes it (M31b). `stream` names the call so
    /// [`Engine::ai_cancel`] can stop it; a cancelled call is an error and
    /// nothing of it is kept.
    pub async fn ai_run_stream(
        &self,
        request: &AiRequest,
        approved: Option<&str>,
        stream: &str,
        mut on_delta: impl FnMut(&str) + Send,
    ) -> Result<AiAnswer, AppError> {
        let (config, preview) = self.ai_checked(request, approved).await?;
        let cancel = Arc::new(tokio::sync::Notify::new());
        {
            let mut streams = lock(&self.inner.ai_streams);
            if streams.len() >= MAX_STREAMS {
                return Err(AppError::Invalid(
                    "too many ChosAI answers are being written: wait for one".into(),
                ));
            }
            if streams.contains_key(stream) {
                // Its end would forget the other call's cancel handle.
                return Err(AppError::Invalid(
                    "an answer with that name is already being written".into(),
                ));
            }
            streams.insert(stream.to_string(), cancel.clone());
        }
        let _guard = StreamGuard {
            engine: self,
            stream,
        };
        let body =
            chosproxy_ai::stream::stream_body(config.provider, &config.model, &preview.messages);
        let started = Instant::now();
        let mut parser = chosproxy_ai::stream::StreamParser::new(config.provider);
        let mut on_body = |bytes: &[u8]| {
            let delta = parser.feed(bytes);
            if !delta.is_empty() {
                on_delta(&delta);
            }
        };
        let status = tokio::select! {
            sent = self.ai_send(&config, &body, Some(&mut on_body)) => sent?.0,
            () = cancel.notified() => return Err(AppError::Cancelled),
        };
        let partial = parser.text().chars().take(200).collect::<String>();
        let text = parser.finish().map_err(|e| {
            if (200..300).contains(&status) {
                AppError::Invalid(e)
            } else {
                AppError::Invalid(format!("the model server answered {status}: {e} {partial}"))
            }
        })?;
        let answer = AiAnswer {
            text,
            model: config.model.clone(),
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            sent_chars: preview
                .messages
                .iter()
                .map(|m| m.content.chars().count() as u64)
                .sum(),
        };
        self.ai_asked(request, &config, &preview, &answer, ", streamed")
            .await;
        Ok(answer)
    }

    /// Stops a streamed answer; false if there is none with that name.
    pub fn ai_cancel(&self, stream: &str) -> bool {
        let streams = lock(&self.inner.ai_streams);
        match streams.get(stream) {
            Some(cancel) => {
                // `notify_one` keeps the permit if the call is not waiting yet.
                cancel.notify_one();
                true
            }
            None => false,
        }
    }

    /// The settings and the preview of a question that may be sent.
    async fn ai_checked(
        &self,
        request: &AiRequest,
        approved: Option<&str>,
    ) -> Result<(AiConfig, AiPreview), AppError> {
        let config = self.ai_config();
        if config.mode == AiMode::Disabled {
            return Err(AppError::Invalid(
                "ChosAI is off: turn it on in Settings → ChosAI".into(),
            ));
        }
        let preview = self.ai_preview(request).await?;
        if preview.external && approved != Some(preview.digest.as_str()) {
            return Err(AppError::Invalid(
                "the model is outside this machine: preview the context and send exactly that"
                    .into(),
            ));
        }
        Ok((config, preview))
    }

    /// Records an answered question in the audit and AI activity logs.
    async fn ai_asked(
        &self,
        request: &AiRequest,
        config: &AiConfig,
        preview: &AiPreview,
        answer: &AiAnswer,
        how: &str,
    ) {
        self.audit(
            "ai.ask",
            &format!(
                "{:?}{} with {} item(s), {} characters, to {}{}{how}",
                request.task,
                if request.conversation.is_empty() {
                    String::new()
                } else {
                    format!(" (follow-up {})", request.conversation.len().div_ceil(2))
                },
                request.attachments.len(),
                answer.sent_chars,
                describe(config),
                if preview.external { " (external)" } else { "" }
            ),
        );
        self.ai_log(chosproxy_storage::NewAiActivity {
            action: "ask".into(),
            tool: format!("{:?}", request.task),
            target: describe(config),
            decision: if preview.external { "previewed" } else { "ran" }.into(),
            result: format!(
                "{} item(s), {} characters sent, answered in {} ms",
                request.attachments.len(),
                answer.sent_chars,
                answer.elapsed_ms
            ),
            detail: preview
                .context
                .text
                .lines()
                .filter(|l| l.starts_with("=== "))
                .collect::<Vec<_>>()
                .join("\n"),
            ..Default::default()
        })
        .await;
    }

    /// A tiny call that checks the model answers (no project data).
    pub async fn ai_test(&self, config: AiConfig) -> Result<AiAnswer, AppError> {
        config.validate().map_err(AppError::Invalid)?;
        let msgs = vec![ChatMessage {
            role: "user".into(),
            content: "Reply with the single word OK.".into(),
        }];
        self.ai_call(&config, &msgs).await
    }

    async fn ai_call(&self, config: &AiConfig, msgs: &[ChatMessage]) -> Result<AiAnswer, AppError> {
        let body = chat_body(config.provider, &config.model, msgs);
        let started = Instant::now();
        let text = self.ai_post(config, &body).await?;
        let answer = parse_reply(config.provider, &text).map_err(AppError::Invalid)?;
        Ok(AiAnswer {
            text: answer,
            model: config.model.clone(),
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            sent_chars: msgs.iter().map(|m| m.content.chars().count() as u64).sum(),
        })
    }

    /// Posts a chat body to the model and returns the reply body (a reply
    /// with an `error` field is returned for the caller to read).
    pub(super) async fn ai_post(&self, config: &AiConfig, body: &str) -> Result<String, AppError> {
        let (status, text) = self.ai_send(config, body, None).await?;
        if !(200..300).contains(&status) && !text.contains("\"error\"") {
            return Err(AppError::Invalid(format!(
                "the model server answered {status}: {}",
                text.chars().take(200).collect::<String>()
            )));
        }
        Ok(text)
    }

    /// Posts a chat body; returns the status and the reply body. With
    /// `on_body`, the body is also handed over while it arrives.
    async fn ai_send(
        &self,
        config: &AiConfig,
        body: &str,
        on_body: Option<chosproxy_request::BodySink<'_>>,
    ) -> Result<(u16, String), AppError> {
        let (https, host, port, base_path) = config.endpoint().map_err(AppError::Invalid)?;
        let key = match &config.api_key_credential {
            Some(id) if !id.is_empty() => Some(self.credential_secret(id)?.ok_or_else(|| {
                AppError::Invalid(format!(
                    "no credential {id:?} in the keyring for the API key"
                ))
            })?),
            _ => None,
        };
        let default_port = if https { 443 } else { 80 };
        let host_header = if port == default_port {
            host.clone()
        } else if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let mut request = format!(
            "POST {base_path}{} HTTP/1.1\r\nHost: {host_header}\r\nContent-Type: application/json\r\n\
             Accept: {}\r\nUser-Agent: ChosProxy-ChosAI\r\nConnection: close\r\n",
            chat_path(config.provider),
            if on_body.is_some() {
                "application/x-ndjson, text/event-stream, application/json"
            } else {
                "application/json"
            }
        );
        if let Some(k) = key.filter(|k| !k.is_empty()) {
            request.push_str(&format!(
                "Authorization: Bearer {}\r\n",
                k.replace(['\r', '\n'], "")
            ));
        }
        request.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
        let security = self
            .security_for_host(&host, https)
            .await
            .map_err(AppError::Invalid)?;
        let destination = Destination {
            host: host.clone(),
            port,
            tls_name: host.clone(),
        };
        let timeout = Duration::from_secs(u64::from(config.timeout_secs));
        let limits = SendLimits {
            response_timeout: timeout,
            body_idle_timeout: timeout,
            max_response_bytes: MAX_ANSWER,
        };
        let sent = match on_body {
            Some(on_body) => {
                chosproxy_request::send_streaming(
                    &destination,
                    &security,
                    request.as_bytes(),
                    &limits,
                    on_body,
                )
                .await
            }
            None => {
                chosproxy_request::send(&destination, &security, request.as_bytes(), &limits).await
            }
        }
        .map_err(|e| {
            AppError::Invalid(format!(
                "cannot reach the model at {}: {}",
                config.base_url, e.message
            ))
        })?;
        let (head, raw_body) = split_message(&sent.response);
        let parsed = ResponseHead::parse(head.to_vec())
            .map_err(|e| AppError::Invalid(format!("unreadable answer from the model: {e}")))?;
        let text = body_text(&parsed, raw_body, MAX_ANSWER).unwrap_or_default();
        Ok((parsed.status, text))
    }
}

/// Most answers streamed at once.
const MAX_STREAMS: usize = 8;

/// Forgets a streamed call when it ends, however it ends.
struct StreamGuard<'a> {
    engine: &'a Engine,
    stream: &'a str,
}

impl Drop for StreamGuard<'_> {
    fn drop(&mut self) {
        lock(&self.engine.inner.ai_streams).remove(self.stream);
    }
}

#[cfg(test)]
mod tests {
    use super::pick_login_requests;

    #[test]
    fn a_recorded_login_keeps_posts_and_drops_static_files() {
        let row = |id: i64, m: &str, e: Option<&str>| (id, m.to_string(), e.map(str::to_string));
        let rows = vec![
            row(1, "GET", None),
            row(2, "GET", Some("css")),
            row(3, "GET", Some("JS")),
            row(4, "POST", Some("php")),
            row(5, "OPTIONS", None),
            row(6, "GET", Some("html")),
            row(7, "POST", Some("js")),
        ];
        assert_eq!(pick_login_requests(&rows, 20), vec![1, 4, 6, 7]);
        // Not enough room: the posts first, then the earliest pages.
        assert_eq!(pick_login_requests(&rows, 3), vec![1, 4, 7]);
        assert_eq!(pick_login_requests(&rows, 1), vec![4]);
        assert!(pick_login_requests(&[], 20).is_empty());
    }
}
