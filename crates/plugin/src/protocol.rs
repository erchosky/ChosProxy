//! The plugin protocol: one JSON object per line on the plugin's standard
//! input (from ChosProxy) and standard output (from the plugin).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Longest line read from a plugin.
pub const MAX_LINE: usize = 1024 * 1024;
/// Body text sent to a plugin per message (bodies are cut there).
pub const MAX_BODY: usize = 256 * 1024;

/// A request and its response, as a plugin receives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "PluginExchange")]
pub struct Exchange {
    /// History id (use it to refer to this exchange).
    #[ts(type = "number")]
    pub id: i64,
    /// Tool that produced it (`proxy`, `repeater`…).
    pub source: String,
    /// Method.
    pub method: String,
    /// Full URL.
    pub url: String,
    /// Host.
    pub host: String,
    /// Response status, if any.
    pub status: Option<u16>,
    /// Request headers, in order, as `[name, value]`.
    pub request_headers: Vec<(String, String)>,
    /// Request body as text (lossy UTF-8, cut at 256 KiB).
    pub request_body: String,
    /// Response headers.
    pub response_headers: Vec<(String, String)>,
    /// Response body as text (decoded, lossy UTF-8, cut at 256 KiB).
    pub response_body: String,
}

/// From ChosProxy to the plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HostMessage {
    /// First message: the API version and what the tester granted.
    Hello {
        /// API version.
        api: u32,
        /// The plugin's id.
        plugin: String,
        /// Permissions granted.
        granted: Vec<String>,
    },
    /// New traffic (sent only with `network.read`).
    Exchange(Box<Exchange>),
    /// API v2: the answer to a call (`send`, `query`).
    Result {
        /// The call's id.
        id: u64,
        /// The call succeeded.
        ok: bool,
        /// What it returned (call-specific).
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
        /// Why it failed.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// API v2: the tester ran one of the plugin's actions on a request.
    Action {
        /// Answer with `output` carrying this number.
        call: u64,
        /// The action's id, as registered.
        action: String,
        /// The request (and response) it was run on.
        exchange: Box<Exchange>,
    },
    /// API v2 (`proxy.intercept`): an in-scope request on its way through
    /// the proxy; answer with `hookResult` carrying this `call` within 2 s.
    Hook {
        /// Answer with `hookResult` carrying this number.
        call: u64,
        /// The request.
        request: HookRequest,
        /// Only for a response hook (M31b, `registerHook` with
        /// `responses`): the whole response to that request, as text.
        /// Answer `continue`, `replace` (with `response`) or `block`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response: Option<String>,
    },
    /// The plugin should exit.
    Shutdown,
}

/// A request shown to a hook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRequest {
    /// `http` or `https`.
    pub scheme: String,
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Full URL.
    pub url: String,
    /// The whole request as text (head and body, CRLF line breaks).
    pub raw: String,
}

/// A finding reported by a plugin (`scanner.passive`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginFinding {
    /// The exchange that shows it.
    pub exchange_id: i64,
    /// Detections with the same key are one finding (per plugin).
    pub key: String,
    /// Type.
    pub kind: String,
    /// Title.
    pub title: String,
    /// `info`, `low`, `medium` or `high`.
    #[serde(default = "info")]
    pub severity: String,
    /// `tentative`, `firm` or `certain`.
    #[serde(default = "firm")]
    pub confidence: String,
    /// What the problem is.
    #[serde(default)]
    pub description: String,
    /// How to fix it.
    #[serde(default)]
    pub remediation: String,
    /// What this exchange shows.
    #[serde(default)]
    pub evidence: String,
}

fn info() -> String {
    "info".into()
}

fn firm() -> String {
    "firm".into()
}

/// From the plugin to ChosProxy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginMessage {
    /// The plugin is ready (answer to `hello`).
    Ready,
    /// A line for the plugin's log.
    Log {
        /// `info`, `warning` or `error`.
        #[serde(default = "info")]
        level: String,
        /// Text.
        message: String,
    },
    /// A finding (`scanner.passive`).
    Finding(PluginFinding),
    /// Highlight and/or note a history entry (`project.write`).
    Annotate {
        /// The exchange.
        exchange_id: i64,
        /// A highlight colour name, or none to keep it.
        highlight: Option<String>,
        /// A note, or none to keep it.
        note: Option<String>,
    },
    /// API v2 (`network.send`): send a request; answered by `result` with
    /// the stored exchange id, status and response.
    Send {
        /// Call id, echoed in the result.
        id: u64,
        /// `http` or `https`.
        scheme: String,
        /// Host.
        host: String,
        /// Port.
        port: u16,
        /// The raw request (line breaks may be `\n` or `\r\n`).
        request: String,
    },
    /// API v2 (`project.read`): read project data; answered by `result`.
    Query {
        /// Call id, echoed in the result.
        id: u64,
        /// What to read.
        query: Query,
    },
    /// API v2 (`ui.extend`): add an action to request menus.
    RegisterAction {
        /// Stable id within the plugin.
        action: String,
        /// Menu label.
        label: String,
    },
    /// API v2: the text to show for an `action` call.
    Output {
        /// The `call` of the action message.
        call: u64,
        /// Text shown to the tester.
        text: String,
    },
    /// API v2 (`proxy.intercept`): receive `hook` messages for in-scope
    /// proxy requests (to these hosts only, when given: `example.com`,
    /// `*.example.com`).
    RegisterHook {
        /// Host patterns; empty = every in-scope host.
        #[serde(default)]
        hosts: Vec<String>,
        /// Also receive the responses to those requests (M31b), as `hook`
        /// messages that carry a `response`.
        #[serde(default)]
        responses: bool,
    },
    /// API v2 (`proxy.intercept`): the decision about a `hook` request.
    HookResult {
        /// The `call` of the hook message.
        call: u64,
        /// `continue`, `replace` (with `request`), `block` (with an
        /// optional `reason`) or `respond` (with `response`).
        action: String,
        /// The request to send instead (`replace`).
        #[serde(default)]
        request: Option<String>,
        /// The response to give the client (`respond`).
        #[serde(default)]
        response: Option<String>,
        /// Why it was blocked (`block`).
        #[serde(default)]
        reason: Option<String>,
    },
}

/// What a `query` reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "what",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Query {
    /// One stored exchange (as in `exchange` messages).
    Exchange {
        /// History id.
        exchange_id: i64,
    },
    /// Recent history rows, newest first, optionally matching a URL text.
    History {
        /// Case-insensitive text in the URL.
        #[serde(default)]
        search: Option<String>,
        /// Rows (1–200).
        #[serde(default = "fifty")]
        limit: u32,
    },
    /// The project's findings.
    Findings,
}

fn fifty() -> u32 {
    50
}

impl PluginMessage {
    /// Reads one line from a plugin.
    pub fn parse(line: &str) -> Result<PluginMessage, String> {
        serde_json::from_str(line).map_err(|e| format!("invalid message: {e}"))
    }
}

impl HostMessage {
    /// One line to write to the plugin (with the newline).
    pub fn to_line(&self) -> String {
        let mut s = serde_json::to_string(self).unwrap_or_else(|_| "{}".into());
        s.push('\n');
        s
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn hook_messages() {
        assert_eq!(
            PluginMessage::parse(r#"{"type": "registerHook", "hosts": ["127.0.0.1"]}"#).unwrap(),
            PluginMessage::RegisterHook {
                hosts: vec!["127.0.0.1".into()],
                responses: false,
            }
        );
        assert_eq!(
            PluginMessage::parse(r#"{"type":"hookResult","call":3,"action":"block"}"#).unwrap(),
            PluginMessage::HookResult {
                call: 3,
                action: "block".into(),
                request: None,
                response: None,
                reason: None,
            }
        );
        let hook = HostMessage::Hook {
            call: 1,
            request: HookRequest {
                scheme: "http".into(),
                host: "h".into(),
                port: 80,
                url: "http://h/".into(),
                raw: "GET / HTTP/1.1\r\n\r\n".into(),
            },
            response: None,
        };
        assert!(
            hook.to_line()
                .starts_with("{\"type\":\"hook\",\"call\":1,\"request\":{")
        );
    }

    #[test]
    fn message_shapes() {
        let hello = HostMessage::Hello {
            api: 1,
            plugin: "p".into(),
            granted: vec!["network.read".into()],
        };
        assert_eq!(
            hello.to_line(),
            "{\"type\":\"hello\",\"api\":1,\"plugin\":\"p\",\"granted\":[\"network.read\"]}\n"
        );
        assert!(HostMessage::Shutdown.to_line().contains("\"shutdown\""));

        let f = PluginMessage::parse(
            r#"{"type":"finding","exchangeId":3,"key":"k","kind":"Debug","title":"T"}"#,
        )
        .unwrap();
        let PluginMessage::Finding(f) = f else {
            panic!("not a finding");
        };
        assert_eq!((f.exchange_id, f.severity.as_str()), (3, "info"));
        assert_eq!(
            PluginMessage::parse(r#"{"type":"annotate","exchangeId":1,"highlight":"yellow"}"#)
                .unwrap(),
            PluginMessage::Annotate {
                exchange_id: 1,
                highlight: Some("yellow".into()),
                note: None
            }
        );
        assert_eq!(
            PluginMessage::parse(r#"{"type":"ready"}"#).unwrap(),
            PluginMessage::Ready
        );
        assert!(PluginMessage::parse(r#"{"type":"delete"}"#).is_err());

        // API v2.
        assert_eq!(
            PluginMessage::parse(
                r#"{"type":"send","id":7,"scheme":"http","host":"lab","port":80,"request":"GET / HTTP/1.1\n\n"}"#
            )
            .unwrap(),
            PluginMessage::Send {
                id: 7,
                scheme: "http".into(),
                host: "lab".into(),
                port: 80,
                request: "GET / HTTP/1.1\n\n".into()
            }
        );
        assert_eq!(
            PluginMessage::parse(
                r#"{"type":"query","id":1,"query":{"what":"history","search":"api"}}"#
            )
            .unwrap(),
            PluginMessage::Query {
                id: 1,
                query: Query::History {
                    search: Some("api".into()),
                    limit: 50
                }
            }
        );
        assert!(
            HostMessage::Result {
                id: 7,
                ok: false,
                data: None,
                error: Some("out of scope".into())
            }
            .to_line()
            .starts_with(r#"{"type":"result","id":7,"ok":false,"error":"out of scope"}"#)
        );
    }
}
