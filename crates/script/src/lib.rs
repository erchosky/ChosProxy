//! ChosScripts (ADR-0032): small scripts written by the tester, run in a
//! sandbox.
//!
//! The language is [Rhai](https://rhai.rs): it has no access to files, the
//! network, processes or the clock beyond timestamps. Every run is bounded
//! (operations, string, array and map sizes, call depth), so a script cannot
//! hang or exhaust ChosProxy. Scripts only see one message, read-only,
//! through [`Message`].
//!
//! A script is one of five kinds, each defined by the function it provides:
//!
//! | Kind | Function | Returns |
//! |---|---|---|
//! | filter | `fn filter(msg)` | `true` to keep the row |
//! | column | `fn column(msg)` | a value shown in a history column |
//! | rewrite | `fn request(text)` and/or `fn response(text)` | the new message text, or `()` to leave it |
//! | action | `fn action(msg)` | text shown to the tester |
//! | check | `fn check(msg)` | `()`, a finding map, or an array of them |

use chosproxy_http::head::{Headers, find_head_end};
use chosproxy_http::{RequestHead, ResponseHead};
use rhai::packages::{Package, StandardPackage};
use rhai::{AST, Array, Dynamic, Engine, EvalAltResult, Map, Position, Scope};
use serde::Serialize;
use std::cell::RefCell;
use ts_rs::TS;

/// Operations one run may perform.
pub const MAX_OPERATIONS: u64 = 2_000_000;
/// Longest script source.
pub const MAX_SOURCE: usize = 64 * 1024;
/// Most findings one check run may report.
pub const MAX_FINDINGS: usize = 20;
/// Longest text a script's output is cut to.
const MAX_OUTPUT: usize = 64 * 1024;

/// What a script is for (defined with the library configuration).
pub use chosproxy_core::config::ScriptKind as Kind;

/// A script problem, with its line when known.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ScriptProblem")]
#[error("{message}")]
pub struct ScriptError {
    /// What went wrong.
    pub message: String,
    /// 1-based line, if known.
    pub line: Option<u32>,
}

impl ScriptError {
    fn new(message: impl Into<String>) -> ScriptError {
        ScriptError {
            message: message.into(),
            line: None,
        }
    }

    fn at(message: impl Into<String>, pos: Position) -> ScriptError {
        ScriptError {
            message: message.into(),
            line: pos.line().and_then(|l| u32::try_from(l).ok()),
        }
    }
}

fn from_eval(e: &EvalAltResult) -> ScriptError {
    let message = match e {
        EvalAltResult::ErrorTooManyOperations(_) => {
            "the script ran too long (operation limit reached)".to_string()
        }
        other => other.to_string(),
    };
    ScriptError::at(message, e.position())
}

/// One request and its response, as scripts see them (read-only).
#[derive(Debug, Clone, Default)]
pub struct Message {
    method: String,
    url: String,
    scheme: String,
    host: String,
    port: i64,
    path: String,
    http_version: String,
    status: i64,
    mime: String,
    request: String,
    response: String,
    request_body: String,
    response_body: String,
    request_head: Option<RequestHead>,
    response_head: Option<ResponseHead>,
}

/// What a [`Message`] is built from.
#[derive(Debug, Clone, Copy)]
pub struct MessageParts<'a> {
    /// `http` or `https`.
    pub scheme: &'a str,
    /// Host.
    pub host: &'a str,
    /// Port.
    pub port: u16,
    /// Path and query.
    pub path: &'a str,
    /// Full URL.
    pub url: &'a str,
    /// MIME category (`HTML`, `JSON`…), if known.
    pub mime: Option<&'a str>,
    /// Raw request.
    pub request: &'a [u8],
    /// Raw response, if any.
    pub response: Option<&'a [u8]>,
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The body of a raw message, with chunked framing and compression removed
/// when possible.
fn body_text(raw: &[u8]) -> String {
    let plain =
        chosproxy_http::edit::unpack(raw, 16 * 1024 * 1024).unwrap_or_else(|_| raw.to_vec());
    let end = find_head_end(&plain).unwrap_or(plain.len());
    text(&plain[end..])
}

impl Message {
    /// The request as text.
    pub fn request_text(&self) -> String {
        self.request.clone()
    }

    /// The response as text, if there is one.
    pub fn response_text(&self) -> Option<String> {
        self.response_head.as_ref().map(|_| self.response.clone())
    }

    /// Builds the script view of an exchange.
    pub fn new(p: MessageParts<'_>) -> Message {
        let request_head = find_head_end(p.request)
            .and_then(|end| RequestHead::parse(p.request[..end].to_vec()).ok());
        let response_head = p.response.and_then(|r| {
            find_head_end(r).and_then(|end| ResponseHead::parse(r[..end].to_vec()).ok())
        });
        Message {
            method: request_head
                .as_ref()
                .map(|h| h.method.clone())
                .unwrap_or_default(),
            http_version: request_head
                .as_ref()
                .map(|h| text(h.version_token()))
                .unwrap_or_default(),
            url: p.url.to_string(),
            scheme: p.scheme.to_string(),
            host: p.host.to_string(),
            port: i64::from(p.port),
            path: p.path.to_string(),
            status: response_head.as_ref().map_or(0, |h| i64::from(h.status)),
            mime: p.mime.unwrap_or_default().to_string(),
            request: text(p.request),
            response: p.response.map(text).unwrap_or_default(),
            request_body: body_text(p.request),
            response_body: p.response.map(body_text).unwrap_or_default(),
            request_head,
            response_head,
        }
    }
}

thread_local! {
    static OUTPUT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn regex(pattern: &str) -> Result<regex::Regex, Box<EvalAltResult>> {
    regex::RegexBuilder::new(pattern)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| format!("invalid regular expression: {e}").into())
}

/// The sandboxed engine with the message API.
fn engine() -> Engine {
    let mut e = Engine::new_raw();
    e.register_global_module(StandardPackage::new().as_shared_module());
    e.set_max_operations(MAX_OPERATIONS);
    e.set_max_string_size(16 * 1024 * 1024);
    e.set_max_array_size(100_000);
    e.set_max_map_size(100_000);
    e.set_max_call_levels(32);
    e.set_max_expr_depths(64, 32);
    e.set_max_modules(0);
    e.disable_symbol("eval");
    e.on_print(|s| {
        OUTPUT.with(|o| {
            let mut o = o.borrow_mut();
            if o.len() < 1000 {
                o.push(s.to_string());
            }
        });
    });
    e.on_debug(|s, _, _| OUTPUT.with(|o| o.borrow_mut().push(s.to_string())));

    e.register_type_with_name::<Message>("Message")
        .register_get("method", |m: &mut Message| m.method.clone())
        .register_get("url", |m: &mut Message| m.url.clone())
        .register_get("scheme", |m: &mut Message| m.scheme.clone())
        .register_get("host", |m: &mut Message| m.host.clone())
        .register_get("port", |m: &mut Message| m.port)
        .register_get("path", |m: &mut Message| m.path.clone())
        .register_get("http_version", |m: &mut Message| m.http_version.clone())
        .register_get("status", |m: &mut Message| m.status)
        .register_get("mime", |m: &mut Message| m.mime.clone())
        .register_get("request", |m: &mut Message| m.request.clone())
        .register_get("response", |m: &mut Message| m.response.clone())
        .register_get("request_body", |m: &mut Message| m.request_body.clone())
        .register_get("response_body", |m: &mut Message| m.response_body.clone())
        .register_get("has_response", |m: &mut Message| m.response_head.is_some())
        .register_fn("request_header", |m: &mut Message, name: &str| {
            m.request_head
                .as_ref()
                .and_then(|h| h.header_str(name))
                .unwrap_or_default()
        })
        .register_fn("response_header", |m: &mut Message, name: &str| {
            m.response_head
                .as_ref()
                .and_then(|h| h.header_str(name))
                .unwrap_or_default()
        })
        .register_fn("response_headers", |m: &mut Message, name: &str| -> Array {
            m.response_head
                .as_ref()
                .map(|h| {
                    h.headers
                        .iter()
                        .filter(|x| x.name.eq_ignore_ascii_case(name))
                        .map(|x| Dynamic::from(text(&x.value)))
                        .collect()
                })
                .unwrap_or_default()
        });

    e.register_fn(
        "regex_match",
        |t: &str, p: &str| -> Result<bool, Box<EvalAltResult>> { Ok(regex(p)?.is_match(t)) },
    );
    e.register_fn(
        "regex_find",
        |t: &str, p: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            Ok(regex(p)?
                .find(t)
                .map_or(Dynamic::UNIT, |m| Dynamic::from(m.as_str().to_string())))
        },
    );
    e.register_fn(
        "regex_captures",
        |t: &str, p: &str| -> Result<Array, Box<EvalAltResult>> {
            Ok(regex(p)?
                .captures(t)
                .map(|c| {
                    c.iter()
                        .map(|g| g.map_or(Dynamic::UNIT, |g| Dynamic::from(g.as_str().to_string())))
                        .collect()
                })
                .unwrap_or_default())
        },
    );
    e.register_fn(
        "regex_replace",
        |t: &str, p: &str, with: &str| -> Result<String, Box<EvalAltResult>> {
            Ok(regex(p)?.replace_all(t, with).into_owned())
        },
    );
    e.register_fn("base64_encode", |t: &str| {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(t)
    });
    e.register_fn("base64_decode", |t: &str| -> Dynamic {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(t.trim())
            .map_or(Dynamic::UNIT, |b| Dynamic::from(text(&b)))
    });
    e.register_fn("contains_ci", |t: &str, needle: &str| {
        t.to_lowercase().contains(&needle.to_lowercase())
    });
    e
}

/// A compiled script, ready to run (any number of times, from any thread).
pub struct Script {
    kind: Kind,
    engine: Engine,
    ast: AST,
}

impl std::fmt::Debug for Script {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Script")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// A finding reported by a check script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ScriptFinding")]
pub struct Finding {
    /// Title.
    pub title: String,
    /// `info`, `low`, `medium` or `high`.
    pub severity: String,
    /// `tentative`, `firm` or `certain`.
    pub confidence: String,
    /// What in the message shows it.
    pub evidence: String,
    /// What the problem is.
    pub detail: String,
    /// How to fix it.
    pub remediation: String,
    /// What makes two detections the same finding (default: the title and
    /// the origin).
    pub key: String,
}

/// What a run produced, plus anything the script printed.
#[derive(Debug, Clone, PartialEq)]
pub struct Run<T> {
    /// The result.
    pub value: T,
    /// Lines from `print` / `debug`.
    pub printed: Vec<String>,
}

fn cut(mut s: String) -> String {
    if s.len() > MAX_OUTPUT {
        let mut end = MAX_OUTPUT;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push('…');
    }
    s
}

fn one_of(value: &str, allowed: &[&str], default: &str) -> String {
    let v = value.trim().to_ascii_lowercase();
    if allowed.contains(&v.as_str()) {
        v
    } else {
        default.to_string()
    }
}

fn map_finding(map: &Map, msg: &Message) -> Result<Finding, ScriptError> {
    let get = |k: &str| map.get(k).map(|v| v.to_string()).unwrap_or_default();
    let title = get("title");
    if title.trim().is_empty() {
        return Err(ScriptError::new("a finding needs a \"title\""));
    }
    let key = match get("key") {
        k if k.is_empty() => format!("{}://{}:{}|{title}", msg.scheme, msg.host, msg.port),
        k => k,
    };
    Ok(Finding {
        severity: one_of(&get("severity"), &["info", "low", "medium", "high"], "info"),
        confidence: one_of(
            &get("confidence"),
            &["tentative", "firm", "certain"],
            "tentative",
        ),
        evidence: cut(get("evidence")),
        detail: cut(get("detail")),
        remediation: cut(get("remediation")),
        title: title.chars().take(200).collect(),
        key: key.chars().take(500).collect(),
    })
}

/// Parameters an entry point is called with.
fn arity(name: &str) -> usize {
    match name {
        "websocket" => 2,
        "active_check" => 3,
        "active_probes" => 1,
        _ => 1,
    }
}

impl Script {
    /// Compiles `source` as a script of `kind`.
    pub fn compile(kind: Kind, source: &str) -> Result<Script, ScriptError> {
        if source.len() > MAX_SOURCE {
            return Err(ScriptError::new(format!(
                "scripts are limited to {} KB",
                MAX_SOURCE / 1024
            )));
        }
        let engine = engine();
        let ast = engine
            .compile(source)
            .map_err(|e| ScriptError::at(e.to_string(), e.position()))?;
        let defines = |name: &str, arity: usize| {
            ast.iter_functions()
                .any(|f| f.name == name && f.params.len() == arity)
        };
        if !kind.entry_points().iter().any(|n| defines(n, arity(n))) {
            let wanted = kind
                .entry_points()
                .iter()
                .map(|n| format!("fn {n}(…)"))
                .collect::<Vec<_>>()
                .join(" or ");
            return Err(ScriptError::new(format!(
                "this kind of script must define {wanted} with one parameter{}",
                if kind == Kind::Rewrite {
                    " (websocket: the text and the direction)"
                } else {
                    ""
                }
            )));
        }
        Ok(Script { kind, engine, ast })
    }

    /// The script's kind.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// True if the script defines `name` with the parameters it is called
    /// with (one; `websocket` takes two).
    pub fn defines(&self, name: &str) -> bool {
        self.ast
            .iter_functions()
            .any(|f| f.name == name && f.params.len() == arity(name))
    }

    fn call(&self, name: &str, arg: Dynamic) -> Result<Run<Dynamic>, ScriptError> {
        OUTPUT.with(|o| o.borrow_mut().clear());
        let result = self
            .engine
            .call_fn::<Dynamic>(&mut Scope::new(), &self.ast, name, (arg,));
        let printed = OUTPUT.with(|o| std::mem::take(&mut *o.borrow_mut()));
        match result {
            Ok(value) => Ok(Run { value, printed }),
            Err(e) => Err(from_eval(&e)),
        }
    }

    /// `fn filter(msg)`: true to keep the message.
    pub fn filter(&self, msg: &Message) -> Result<Run<bool>, ScriptError> {
        let run = self.call("filter", Dynamic::from(msg.clone()))?;
        let keep = run
            .value
            .as_bool()
            .map_err(|t| ScriptError::new(format!("filter must return true or false, not {t}")))?;
        Ok(Run {
            value: keep,
            printed: run.printed,
        })
    }

    /// `fn column(msg)`: the value shown (empty for `()`).
    pub fn column(&self, msg: &Message) -> Result<Run<String>, ScriptError> {
        let run = self.call("column", Dynamic::from(msg.clone()))?;
        let value = if run.value.is_unit() {
            String::new()
        } else {
            run.value.to_string().chars().take(500).collect()
        };
        Ok(Run {
            value,
            printed: run.printed,
        })
    }

    /// `fn request(text)` / `fn response(text)`: the new text, or `None`
    /// when the script returns `()`, does not define the function, or
    /// returns the text unchanged.
    pub fn rewrite(
        &self,
        response: bool,
        message: &str,
    ) -> Result<Run<Option<String>>, ScriptError> {
        let name = if response { "response" } else { "request" };
        if !self.defines(name) {
            return Ok(Run {
                value: None,
                printed: Vec::new(),
            });
        }
        let run = self.call(name, Dynamic::from(message.to_string()))?;
        let value = if run.value.is_unit() {
            None
        } else {
            let s = run
                .value
                .into_immutable_string()
                .map_err(|t| {
                    ScriptError::new(format!("{name} must return the text or (), not {t}"))
                })?
                .to_string();
            (s != message).then_some(s)
        };
        Ok(Run {
            value,
            printed: run.printed,
        })
    }

    /// `fn websocket(text, to_server)`: the new text of a WebSocket text
    /// message (M31b), or `None` when the script returns `()`, does not
    /// define the function, or returns the text unchanged. `to_server` is
    /// true for messages from the client.
    pub fn websocket(
        &self,
        to_server: bool,
        message: &str,
    ) -> Result<Run<Option<String>>, ScriptError> {
        if !self.defines("websocket") {
            return Ok(Run {
                value: None,
                printed: Vec::new(),
            });
        }
        OUTPUT.with(|o| o.borrow_mut().clear());
        let result = self.engine.call_fn::<Dynamic>(
            &mut Scope::new(),
            &self.ast,
            "websocket",
            (Dynamic::from(message.to_string()), Dynamic::from(to_server)),
        );
        let printed = OUTPUT.with(|o| std::mem::take(&mut *o.borrow_mut()));
        let value = result.map_err(|e| from_eval(&e))?;
        let value = if value.is_unit() {
            None
        } else {
            let s = value
                .into_immutable_string()
                .map_err(|t| {
                    ScriptError::new(format!("websocket must return the text or (), not {t}"))
                })?
                .to_string();
            (s != message).then_some(s)
        };
        Ok(Run { value, printed })
    }

    /// `fn action(msg)`: the text to show.
    pub fn action(&self, msg: &Message) -> Result<Run<String>, ScriptError> {
        let run = self.call("action", Dynamic::from(msg.clone()))?;
        let value = if run.value.is_unit() {
            String::new()
        } else {
            cut(run.value.to_string())
        };
        Ok(Run {
            value,
            printed: run.printed,
        })
    }

    /// `fn payload(p)`: the processed payload (`()` keeps it).
    pub fn payload(&self, value: &str) -> Result<String, ScriptError> {
        let run = self.call("payload", Dynamic::from(value.to_string()))?;
        if run.value.is_unit() {
            return Ok(value.to_string());
        }
        Ok(run.value.to_string())
    }

    /// `fn check(msg)`: the findings it reports.
    pub fn check(&self, msg: &Message) -> Result<Run<Vec<Finding>>, ScriptError> {
        if !self.defines("check") {
            return Ok(Run {
                value: Vec::new(),
                printed: Vec::new(),
            });
        }
        let run = self.call("check", Dynamic::from(msg.clone()))?;
        let findings = Self::decode_findings(&run.value, msg, "check")?;
        Ok(Run {
            value: findings,
            printed: run.printed,
        })
    }

    /// `fn active_probes(value)`: payload strings to inject into an insertion
    /// point for an active check script (`SCANNER-013`, M13).
    pub fn active_probes(&self, value: &str) -> Result<Vec<String>, ScriptError> {
        if !self.defines("active_check") {
            return Ok(Vec::new());
        }
        if !self.defines("active_probes") {
            return Ok(vec![format!("{value}chosprobe7x9k")]);
        }
        let run = self.call("active_probes", Dynamic::from(value.to_string()))?;
        let mut out = Vec::new();
        if run.value.is_unit() {
        } else if let Ok(s) = run.value.clone().into_immutable_string() {
            out.push(s.to_string());
        } else if run.value.is_array() {
            for item in run.value.cast::<Array>() {
                if item.is_unit() {
                    continue;
                }
                out.push(item.to_string());
                if out.len() >= 10 {
                    break;
                }
            }
        } else {
            return Err(ScriptError::new(
                "active_probes must return (), a string, or an array of strings",
            ));
        }
        Ok(out)
    }

    /// `fn active_check(baseline, probe, payload)`: evaluates a probe exchange
    /// against the baseline exchange (`SCANNER-013`, M13).
    pub fn active_check(
        &self,
        baseline: &Message,
        probe: &Message,
        payload: &str,
    ) -> Result<Run<Vec<Finding>>, ScriptError> {
        if !self.defines("active_check") {
            return Ok(Run {
                value: Vec::new(),
                printed: Vec::new(),
            });
        }
        OUTPUT.with(|o| o.borrow_mut().clear());
        let result = self.engine.call_fn::<Dynamic>(
            &mut Scope::new(),
            &self.ast,
            "active_check",
            (
                Dynamic::from(baseline.clone()),
                Dynamic::from(probe.clone()),
                Dynamic::from(payload.to_string()),
            ),
        );
        let printed = OUTPUT.with(|o| std::mem::take(&mut *o.borrow_mut()));
        let value = result.map_err(|e| from_eval(&e))?;
        let findings = Self::decode_findings(&value, probe, "active_check")?;
        Ok(Run {
            value: findings,
            printed,
        })
    }

    fn decode_findings(
        value: &Dynamic,
        msg: &Message,
        fn_name: &str,
    ) -> Result<Vec<Finding>, ScriptError> {
        let mut findings = Vec::new();
        if value.is_unit() {
        } else if value.is_map() {
            let map = value.clone().cast::<Map>();
            findings.push(map_finding(&map, msg)?);
        } else if value.is_array() {
            for item in value.clone().cast::<Array>() {
                if item.is_unit() {
                    continue;
                }
                if !item.is_map() {
                    return Err(ScriptError::new(format!(
                        "{fn_name} must return (), a map or an array of maps"
                    )));
                }
                findings.push(map_finding(&item.cast::<Map>(), msg)?);
                if findings.len() >= MAX_FINDINGS {
                    break;
                }
            }
        } else {
            return Err(ScriptError::new(format!(
                "{fn_name} must return (), a map or an array of maps"
            )));
        }
        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg() -> Message {
        Message::new(MessageParts {
            scheme: "https",
            host: "lab.test",
            port: 443,
            path: "/api?x=1",
            url: "https://lab.test/api?x=1",
            mime: Some("JSON"),
            request: b"GET /api?x=1 HTTP/1.1\r\nHost: lab.test\r\nX-Api-Key: k\r\n\r\n",
            response: Some(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nServer: nginx/1.2\r\nTransfer-Encoding: chunked\r\n\r\n9\r\n{\"a\":\"b\"}\r\n0\r\n\r\n",
            ),
        })
    }

    #[test]
    fn filters_and_columns_read_the_message() {
        let f = Script::compile(
            Kind::Filter,
            r#"fn filter(msg) { msg.status == 200 && msg.request_header("x-api-key") != "" }"#,
        )
        .unwrap();
        assert!(f.filter(&msg()).unwrap().value);
        let c = Script::compile(
            Kind::Column,
            r#"fn column(msg) { print("hi"); msg.response_header("server") + " " + msg.response_body }"#,
        )
        .unwrap();
        let run = c.column(&msg()).unwrap();
        assert_eq!(run.value, r#"nginx/1.2 {"a":"b"}"#);
        assert_eq!(run.printed, vec!["hi"]);
    }

    #[test]
    fn rewrites_actions_and_checks() {
        let r = Script::compile(
            Kind::Rewrite,
            r#"fn request(text) { regex_replace(text, "(?mR)^X-Api-Key: .*$", "X-Api-Key: other") }"#,
        )
        .unwrap();
        let out = r
            .rewrite(false, "GET / HTTP/1.1\r\nX-Api-Key: k\r\n\r\n")
            .unwrap();
        assert_eq!(
            out.value.as_deref(),
            Some("GET / HTTP/1.1\r\nX-Api-Key: other\r\n\r\n")
        );
        assert_eq!(
            r.rewrite(true, "HTTP/1.1 200 OK\r\n\r\n").unwrap().value,
            None
        );

        let a = Script::compile(
            Kind::Action,
            r#"fn action(msg) { `${msg.method} ${msg.url}` }"#,
        )
        .unwrap();
        assert_eq!(
            a.action(&msg()).unwrap().value,
            "GET https://lab.test/api?x=1"
        );

        let c = Script::compile(
            Kind::Check,
            r#"
            fn check(msg) {
                let server = msg.response_header("server");
                if regex_match(server, "/[0-9]") {
                    #{ title: "Server version disclosed", severity: "LOW", evidence: server }
                }
            }"#,
        )
        .unwrap();
        let found = c.check(&msg()).unwrap().value;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, "low");
        assert_eq!(found[0].confidence, "tentative");
        assert_eq!(
            found[0].key,
            "https://lab.test:443|Server version disclosed"
        );
    }

    #[test]
    fn condition_builder_scripts_compile_and_run() {
        // The shape the condition builder writes (apps/desktop/src/lib/conditions.ts).
        let source = r#"// Made by the condition builder.
fn filter(msg) {
    (contains_ci(msg.response_header("Server"), "ng\"inx\u00e9")) || (msg.status > 199) && (msg.request_header("Cookie") == "") && (!contains_ci(msg.url, "logout")) && (msg.method.to_lower() == "GET".to_lower()) && (regex_match(msg.url, "/api")) && (msg.response.len() > 10)
}
"#;
        let s = Script::compile(Kind::Filter, source).unwrap();
        assert!(s.filter(&msg()).unwrap().value);
    }

    #[test]
    fn payload_scripts() {
        let p = Script::compile(
            Kind::Payload,
            r#"fn payload(p) { base64_encode(p + ":x") }"#,
        )
        .unwrap();
        assert_eq!(p.payload("admin").unwrap(), "YWRtaW46eA==");
        let keep = Script::compile(Kind::Payload, "fn payload(p) { () }").unwrap();
        assert_eq!(keep.payload("a").unwrap(), "a");
    }

    #[test]
    fn the_sandbox_holds() {
        // Wrong kind / missing function.
        let e = Script::compile(Kind::Filter, "fn column(msg) { 1 }").unwrap_err();
        assert!(e.message.contains("fn filter"), "{e}");
        // Syntax errors carry a line.
        let e = Script::compile(Kind::Filter, "fn filter(msg) {\n  let = 1;\n}").unwrap_err();
        assert_eq!(e.line, Some(2));
        // Endless loops stop.
        let s = Script::compile(Kind::Filter, "fn filter(msg) { loop {} }").unwrap();
        let e = s.filter(&msg()).unwrap_err();
        assert!(e.message.contains("operation limit"), "{e}");
        // No eval, no modules.
        assert!(Script::compile(Kind::Filter, r#"fn filter(msg) { eval("1") }"#).is_err());
        let s = Script::compile(Kind::Filter, r#"import "x" as y; fn filter(msg) { true }"#);
        assert!(s.is_err() || s.unwrap().filter(&msg()).is_err());
        // Wrong return types are explained.
        let s = Script::compile(Kind::Filter, "fn filter(msg) { 1 }").unwrap();
        assert!(
            s.filter(&msg())
                .unwrap_err()
                .message
                .contains("true or false")
        );
        // Huge strings are refused.
        let s = Script::compile(
            Kind::Action,
            r#"fn action(msg) { let s = "x"; loop { s += s; } }"#,
        )
        .unwrap();
        assert!(s.action(&msg()).is_err());
    }

    #[test]
    fn websocket_messages_are_rewritten_by_direction() {
        let s = Script::compile(
            Kind::Rewrite,
            r#"fn websocket(text, to_server) {
                if to_server { text + "!" } else { () }
            }"#,
        )
        .unwrap();
        assert_eq!(
            s.websocket(true, "hi").unwrap().value.as_deref(),
            Some("hi!")
        );
        assert_eq!(s.websocket(false, "hi").unwrap().value, None);
        // A rewrite script without the function leaves messages alone.
        let http = Script::compile(Kind::Rewrite, "fn request(text) { text }").unwrap();
        assert_eq!(http.websocket(true, "hi").unwrap().value, None);
        let bad = Script::compile(Kind::Rewrite, "fn websocket(text, to_server) { 1 }").unwrap();
        assert!(bad.websocket(true, "hi").is_err());
    }
}
