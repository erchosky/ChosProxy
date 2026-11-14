//! Plugin interception hooks (M26, matrix EXT-002): plugins granted
//! `proxy.intercept` register a request hook; the proxy then asks them about
//! each in-scope request (after Match & Replace, before Intercept), in
//! plugin id order. Each may let it go on, change it, block it or answer it.
//!
//! The rules that keep this safe for the tester:
//! - only with a scope defined, and only for in-scope requests;
//! - a plugin gets [`HOOK_TIMEOUT`] to answer, then the request goes on
//!   unchanged (fail open), and so does an answer that cannot be used;
//! - binary or very large requests are not shown to hooks;
//! - every change, block and answer is noted in the history entry and the
//!   plugin's log.

use super::{Engine, Inner, read};
use chosproxy_core::config::host_matches;
use chosproxy_http::edit::{self, Eol};
use chosproxy_http::head::find_head_end;
use chosproxy_http::{RequestHead, ResponseHead};
use chosproxy_plugin::{HookRequest, HostMessage};
use chosproxy_proxy::{HookFuture, HookOutcome, HookTarget, HookVerdict, RequestHook};
use std::sync::Weak;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// How long a plugin has to decide about a request.
pub(crate) const HOOK_TIMEOUT: Duration = Duration::from_secs(2);
/// Largest request shown to hooks.
const MAX_HOOK_REQUEST: usize = 1024 * 1024;
/// Most host patterns per hook.
pub(crate) const MAX_HOOK_HOSTS: usize = 20;

/// A plugin's `hookResult`.
#[derive(Debug)]
pub(crate) struct HookAnswer {
    pub(crate) action: String,
    pub(crate) request: Option<String>,
    pub(crate) response: Option<String>,
    pub(crate) reason: Option<String>,
}

/// The proxy's view of the plugins' hooks (holds the engine weakly: the
/// proxy belongs to the engine).
pub(crate) struct PluginHooks {
    inner: Weak<Inner>,
}

/// An owned copy of a hook target.
struct Target {
    scheme: String,
    host: String,
    port: u16,
    path: String,
}

impl RequestHook for PluginHooks {
    fn wants(&self, target: &HookTarget<'_>) -> bool {
        let Some(inner) = self.inner.upgrade() else {
            return false;
        };
        let engine = Engine::from_inner(inner);
        engine.hook_in_scope(target) && !engine.hook_plugins(target.host, false).is_empty()
    }

    fn wants_response(&self, target: &HookTarget<'_>) -> bool {
        let Some(inner) = self.inner.upgrade() else {
            return false;
        };
        let engine = Engine::from_inner(inner);
        engine.hook_in_scope(target) && !engine.hook_plugins(target.host, true).is_empty()
    }

    fn on_response(
        &self,
        target: &HookTarget<'_>,
        request: Vec<u8>,
        response: Vec<u8>,
    ) -> HookFuture {
        let inner = self.inner.upgrade();
        let target = Target {
            scheme: target.scheme.to_string(),
            host: target.host.to_string(),
            port: target.port,
            path: target.path.to_string(),
        };
        Box::pin(async move {
            match inner {
                Some(inner) => {
                    Engine::from_inner(inner)
                        .run_response_hooks(target, request, response)
                        .await
                }
                None => HookOutcome::pass(),
            }
        })
    }

    fn on_request(&self, target: &HookTarget<'_>, request: Vec<u8>) -> HookFuture {
        let inner = self.inner.upgrade();
        let target = Target {
            scheme: target.scheme.to_string(),
            host: target.host.to_string(),
            port: target.port,
            path: target.path.to_string(),
        };
        Box::pin(async move {
            match inner {
                Some(inner) => Engine::from_inner(inner).run_hooks(target, request).await,
                None => HookOutcome::pass(),
            }
        })
    }
}

/// Text to bytes: CRLF in the head, the body's own line breaks kept,
/// Content-Length set to the body.
fn message_bytes(text: &str) -> Vec<u8> {
    let body_crlf = text
        .split_once("\n\n")
        .or_else(|| text.split_once("\r\n\r\n"))
        .is_some_and(|(_, body)| body.contains("\r\n"));
    let raw = edit::from_text(text, Eol::CrLf, if body_crlf { Eol::CrLf } else { Eol::Lf });
    edit::update_content_length(&raw, true)
}

/// A request a plugin wants sent instead: it must parse.
fn replacement(text: &str) -> Result<Vec<u8>, String> {
    // The HTTP/2 text form has no Content-Length of its own to keep right.
    let raw = if chosproxy_http::head::request_is_h2(text.as_bytes()) {
        edit::from_text(text, Eol::CrLf, Eol::Lf)
    } else {
        message_bytes(text)
    };
    let end = find_head_end(&raw).ok_or("the request has no complete head")?;
    let head = RequestHead::parse(raw[..end].to_vec()).map_err(|e| e.to_string())?;
    if !head.target.starts_with('/') {
        return Err("the request line must use a path (origin form)".into());
    }
    Ok(raw)
}

/// A response as a plugin reads it: chunked transfer coding removed (the
/// body in one piece, with a Content-Length). Anything else is unchanged.
fn unchunked(raw: Vec<u8>) -> Vec<u8> {
    let Some(end) = find_head_end(&raw) else {
        return raw;
    };
    let Ok(head) = ResponseHead::parse(raw[..end].to_vec()) else {
        return raw;
    };
    use chosproxy_http::head::Headers as _;
    if head
        .header_tokens("transfer-encoding")
        .last()
        .is_none_or(|t| t != "chunked")
    {
        return raw;
    }
    let Ok((body, true)) = chosproxy_http::content::dechunk(&raw[end..]) else {
        return raw;
    };
    // The head without its Transfer-Encoding lines, then the plain body.
    let head_text = String::from_utf8_lossy(&raw[..end]);
    let mut out = String::new();
    for line in head_text.split_inclusive('\n') {
        if line.trim().is_empty() {
            continue;
        }
        if !line.to_ascii_lowercase().starts_with("transfer-encoding:") {
            out.push_str(line);
        }
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut out = out.into_bytes();
    out.extend_from_slice(&body);
    out
}

/// A response a plugin wants given to the client: it must parse.
fn synthetic_response(text: &str) -> Result<Vec<u8>, String> {
    let raw = message_bytes(text);
    let end = find_head_end(&raw).ok_or("the response has no complete head")?;
    ResponseHead::parse(raw[..end].to_vec()).map_err(|e| e.to_string())?;
    Ok(raw)
}

impl Engine {
    /// The proxy hook to install on the proxy.
    pub(crate) fn plugin_hooks(&self) -> PluginHooks {
        PluginHooks {
            inner: std::sync::Arc::downgrade(&self.inner),
        }
    }

    /// Hooks apply only with a scope defined, to in-scope requests.
    fn hook_in_scope(&self, t: &HookTarget<'_>) -> bool {
        let scope = read(&self.inner.scope);
        !scope.is_empty()
            && scope.contains(&chosproxy_core::scope::ScopeTarget {
                scheme: t.scheme,
                host: t.host,
                port: t.port,
                path: if t.path.starts_with('/') { t.path } else { "/" },
            })
    }

    /// Running plugins with a hook for `host` (`responses`: only those that
    /// asked for responses too), in id order.
    fn hook_plugins(
        &self,
        host: &str,
        responses: bool,
    ) -> Vec<(String, mpsc::Sender<HostMessage>)> {
        let plugins = super::lock(&self.inner.plugins);
        let mut out: Vec<(String, mpsc::Sender<HostMessage>)> = plugins
            .iter()
            .filter_map(|(id, rt)| {
                let hosts = rt.hook.as_ref()?;
                let calls = rt.calls.clone()?;
                if responses && !rt.hook_responses {
                    return None;
                }
                (hosts.is_empty() || hosts.iter().any(|p| host_matches(p, host)))
                    .then(|| (id.clone(), calls))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Asks one plugin about a request (or, with `response`, a response)
    /// and waits for its decision, at most [`HOOK_TIMEOUT`]. The error is
    /// the note for the history: the message goes on unchanged.
    async fn ask_hook(
        &self,
        plugin: &str,
        sender: &mpsc::Sender<HostMessage>,
        request: HookRequest,
        response: Option<String>,
        what: &str,
    ) -> Result<HookAnswer, String> {
        static CALLS: AtomicU64 = AtomicU64::new(1);
        let call = CALLS.fetch_add(1, Ordering::Relaxed);
        let url = request.url.clone();
        let (tx, rx) = oneshot::channel();
        if let Some(rt) = super::lock(&self.inner.plugins).get_mut(plugin) {
            rt.hook_pending.insert(call, tx);
        }
        let message = HostMessage::Hook {
            call,
            request,
            response,
        };
        let answer = tokio::time::timeout(HOOK_TIMEOUT, async {
            sender.send(message).await.ok()?;
            rx.await.ok()
        })
        .await;
        match answer {
            Ok(Some(a)) => Ok(a),
            Ok(None) => Err(format!("Plugin {plugin} stopped before deciding.")),
            Err(_) => {
                if let Some(rt) = super::lock(&self.inner.plugins).get_mut(plugin) {
                    rt.hook_pending.remove(&call);
                }
                self.plugin_log(
                    plugin,
                    "warning",
                    format!("Hook timed out on {url}; the {what} went on unchanged."),
                );
                Err(format!(
                    "Plugin {plugin} did not decide within {} s; the {what} went on.",
                    HOOK_TIMEOUT.as_secs()
                ))
            }
        }
    }

    /// Shows a complete response to the plugins that asked for responses.
    async fn run_response_hooks(
        &self,
        target: Target,
        request: Vec<u8>,
        response: Vec<u8>,
    ) -> HookOutcome {
        if response.len() > MAX_HOOK_REQUEST || request.len() > MAX_HOOK_REQUEST {
            return HookOutcome {
                verdict: HookVerdict::Continue,
                notes: vec!["Response larger than 1 MiB: plugin hooks not applied.".into()],
            };
        }
        // Plugins read the body in one piece; if none changes it, the
        // response goes on exactly as the server sent it.
        let Ok(mut text) = String::from_utf8(unchunked(response.clone())) else {
            return HookOutcome {
                verdict: HookVerdict::Continue,
                notes: vec!["Binary response: plugin hooks not applied.".into()],
            };
        };
        let request_text = String::from_utf8_lossy(&request).into_owned();
        let url = chosproxy_http::target::format_url(
            if target.scheme == "https" {
                chosproxy_http::Scheme::Https
            } else {
                chosproxy_http::Scheme::Http
            },
            &target.host,
            target.port,
            &target.path,
        );
        let mut current = response;
        let mut changed = false;
        let mut notes = Vec::new();
        for (plugin, sender) in self.hook_plugins(&target.host, true) {
            let shown = HookRequest {
                scheme: target.scheme.clone(),
                host: target.host.clone(),
                port: target.port,
                url: url.clone(),
                raw: request_text.clone(),
            };
            let answer = match self
                .ask_hook(&plugin, &sender, shown, Some(text.clone()), "response")
                .await
            {
                Ok(a) => a,
                Err(note) => {
                    notes.push(note);
                    continue;
                }
            };
            match answer.action.as_str() {
                "continue" => {}
                "replace" | "respond" => match answer.response.as_deref().map(synthetic_response) {
                    Some(Ok(raw)) => {
                        if raw != current && raw != text.as_bytes() {
                            notes.push(format!("Plugin {plugin} changed the response."));
                            text = String::from_utf8_lossy(&raw).into_owned();
                            current = raw;
                            changed = true;
                        }
                    }
                    Some(Err(e)) => notes.push(format!(
                        "Plugin {plugin} gave a response that cannot be used ({e}); ignored."
                    )),
                    None => notes.push(format!(
                        "Plugin {plugin} asked to replace the response without one; ignored."
                    )),
                },
                "block" => {
                    let reason: String = answer
                        .reason
                        .unwrap_or_default()
                        .chars()
                        .take(300)
                        .collect();
                    let why = if reason.trim().is_empty() {
                        format!("Response blocked by plugin {plugin}.")
                    } else {
                        format!("Response blocked by plugin {plugin}: {}", reason.trim())
                    };
                    self.plugin_log(&plugin, "info", format!("Blocked the response of {url}."));
                    notes.push(why.clone());
                    return HookOutcome {
                        verdict: HookVerdict::Block(why),
                        notes,
                    };
                }
                other => notes.push(format!(
                    "Plugin {plugin} answered with an unknown action {other:?}; ignored."
                )),
            }
        }
        HookOutcome {
            verdict: if changed {
                HookVerdict::Replace(current)
            } else {
                HookVerdict::Continue
            },
            notes,
        }
    }

    async fn run_hooks(&self, target: Target, request: Vec<u8>) -> HookOutcome {
        if request.len() > MAX_HOOK_REQUEST {
            return HookOutcome {
                verdict: HookVerdict::Continue,
                notes: vec!["Request larger than 1 MiB: plugin hooks not applied.".into()],
            };
        }
        let Ok(mut text) = String::from_utf8(request.clone()) else {
            return HookOutcome {
                verdict: HookVerdict::Continue,
                notes: vec!["Binary request: plugin hooks not applied.".into()],
            };
        };
        let mut current = request;
        let mut changed = false;
        let mut notes = Vec::new();
        let url = chosproxy_http::target::format_url(
            if target.scheme == "https" {
                chosproxy_http::Scheme::Https
            } else {
                chosproxy_http::Scheme::Http
            },
            &target.host,
            target.port,
            &target.path,
        );
        for (plugin, sender) in self.hook_plugins(&target.host, false) {
            let request = HookRequest {
                scheme: target.scheme.clone(),
                host: target.host.clone(),
                port: target.port,
                url: url.clone(),
                raw: text.clone(),
            };
            let answer = match self
                .ask_hook(&plugin, &sender, request, None, "request")
                .await
            {
                Ok(a) => a,
                Err(note) => {
                    notes.push(note);
                    continue;
                }
            };
            match answer.action.as_str() {
                "continue" => {}
                "replace" => match answer.request.as_deref().map(replacement) {
                    Some(Ok(raw)) => {
                        if raw != current {
                            notes.push(format!("Plugin {plugin} changed the request."));
                            text = String::from_utf8_lossy(&raw).into_owned();
                            current = raw;
                            changed = true;
                        }
                    }
                    Some(Err(e)) => {
                        notes.push(format!(
                            "Plugin {plugin} gave a request that cannot be sent ({e}); ignored."
                        ));
                    }
                    None => notes.push(format!(
                        "Plugin {plugin} asked to replace the request without one; ignored."
                    )),
                },
                "block" => {
                    let reason: String = answer
                        .reason
                        .unwrap_or_default()
                        .chars()
                        .take(300)
                        .collect();
                    let why = if reason.trim().is_empty() {
                        format!("Blocked by plugin {plugin}.")
                    } else {
                        format!("Blocked by plugin {plugin}: {}", reason.trim())
                    };
                    self.plugin_log(&plugin, "info", format!("Blocked {url}."));
                    notes.push(why.clone());
                    return HookOutcome {
                        verdict: HookVerdict::Block(why),
                        notes,
                    };
                }
                "respond" => match answer.response.as_deref().map(synthetic_response) {
                    Some(Ok(raw)) => {
                        self.plugin_log(&plugin, "info", format!("Answered {url} itself."));
                        notes.push(format!(
                            "Answered by plugin {plugin}; the request was not sent."
                        ));
                        return HookOutcome {
                            verdict: HookVerdict::Respond(raw),
                            notes,
                        };
                    }
                    Some(Err(e)) => notes.push(format!(
                        "Plugin {plugin} gave a response that cannot be used ({e}); ignored."
                    )),
                    None => notes.push(format!(
                        "Plugin {plugin} asked to respond without a response; ignored."
                    )),
                },
                other => notes.push(format!(
                    "Plugin {plugin} answered with an unknown action {other:?}; ignored."
                )),
            }
        }
        HookOutcome {
            verdict: if changed {
                HookVerdict::Replace(current)
            } else {
                HookVerdict::Continue
            },
            notes,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn answers_are_checked() {
        let raw = replacement("POST /a HTTP/1.1\nHost: x\n\nhello").unwrap();
        assert_eq!(
            String::from_utf8(raw).unwrap(),
            "POST /a HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhello"
        );
        assert!(replacement("GET http://x/ HTTP/1.1\nHost: x\n\n").is_err());
        assert!(replacement("not a request").is_err());
        let res = synthetic_response("HTTP/1.1 200 OK\nContent-Type: text/plain\n\nhi").unwrap();
        assert!(
            String::from_utf8(res)
                .unwrap()
                .contains("Content-Length: 2\r\n\r\nhi")
        );
        assert!(synthetic_response("hello").is_err());
        // HTTP/2 requests keep their text form.
        let h2 = replacement("GET /a HTTP/2\nhost: x\n\n").unwrap();
        assert_eq!(
            String::from_utf8(h2).unwrap(),
            "GET /a HTTP/2\r\nhost: x\r\n\r\n"
        );
        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nX-A: 1\r\n\r\n2\r\nok\r\n0\r\n\r\n";
        assert_eq!(
            String::from_utf8(unchunked(chunked.to_vec())).unwrap(),
            "HTTP/1.1 200 OK\r\nX-A: 1\r\nContent-Length: 2\r\n\r\nok"
        );
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec();
        assert_eq!(unchunked(plain.clone()), plain);
        // A cut chunked body is left as it is.
        let cut = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nok".to_vec();
        assert_eq!(unchunked(cut.clone()), cut);
    }
}
