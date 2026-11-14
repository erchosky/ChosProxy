//! Session handling (M24, matrix SESSION-001/002/005/006): project rules
//! that keep tool sends logged in.
//!
//! A rule matches by host and tool. Before a send it adds the project's
//! cookie jar and sets headers whose values may use `{{name}}` variables
//! (extracted from login responses) and `{{cred:id.username}}` /
//! `{{cred:id.secret}}` (keyring). After the send, a validity check (status
//! codes and/or a regular expression on the response) tells whether the
//! session expired; if so the rule's login macro (stored requests replayed
//! in order, cookies kept, variables extracted) runs once and the request
//! is sent again. Every macro request is stored in the history.

use super::Engine;
use crate::AppError;
use crate::cookies;
use chosproxy_core::Source;
use chosproxy_core::config::host_matches;
use chosproxy_core::time::now_ms;
use chosproxy_request::{Destination, SendError, SendLimits, Sent, UpstreamSecurity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

/// Project settings key of the rules.
const SETTING_KEY: &str = "session_rules";
/// Most macro requests per rule.
const MAX_MACRO: usize = 10;

/// A value pulled from a login response into `{{name}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionExtraction {
    /// Variable name.
    pub name: String,
    /// Regular expression; group 1 (or the whole match) is the value.
    pub regex: String,
}

/// A header set on every matching request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionHeader {
    /// Header name.
    pub name: String,
    /// Value; `{{var}}` and `{{cred:id.username|secret}}` are filled in.
    pub value: String,
}

/// A session handling rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionRule {
    /// Whether the rule is used.
    pub enabled: bool,
    /// Name.
    pub name: String,
    /// Host patterns (`example.com`, `*.example.com`); empty = every host.
    pub hosts: Vec<String>,
    /// Tools (`repeater`, `fuzzer`, `sequencer`, `crawler`, `plugin`);
    /// empty = every tool.
    pub tools: Vec<String>,
    /// Add matching cookies from the project cookie jar.
    pub use_cookie_jar: bool,
    /// The session expired when the response has one of these statuses…
    pub expired_statuses: Vec<u16>,
    /// …or matches this regular expression (case-insensitive; empty = off).
    pub expired_regex: String,
    /// Login macro: stored exchanges replayed in order.
    #[ts(type = "number[]")]
    pub macro_requests: Vec<i64>,
    /// Values extracted from the macro's responses.
    pub extract: Vec<SessionExtraction>,
    /// Headers set on matching requests.
    pub headers: Vec<SessionHeader>,
}

fn regex(p: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(p)
        .case_insensitive(true)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| format!("invalid regular expression {p:?}: {e}"))
}

/// Checks rules before they are saved.
pub fn validate_rules(rules: &[SessionRule]) -> Result<(), String> {
    if rules.len() > 50 {
        return Err("at most 50 session rules".into());
    }
    for r in rules {
        if r.name.trim().is_empty() || r.name.chars().count() > 80 {
            return Err("every rule needs a name (1-80 characters)".into());
        }
        if r.macro_requests.len() > MAX_MACRO {
            return Err(format!("“{}”: at most {MAX_MACRO} macro requests", r.name));
        }
        if !r.expired_regex.is_empty() {
            regex(&r.expired_regex).map_err(|e| format!("“{}”: {e}", r.name))?;
        }
        for x in &r.extract {
            if x.name.is_empty()
                || !x
                    .name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(format!(
                    "“{}”: variable names use letters, digits and _",
                    r.name
                ));
            }
            regex(&x.regex).map_err(|e| format!("“{}”: {e}", r.name))?;
        }
        for h in &r.headers {
            if h.name.is_empty()
                || h.name.contains([':', '\r', '\n'])
                || h.value.contains(['\r', '\n'])
            {
                return Err(format!("“{}”: invalid header {:?}", r.name, h.name));
            }
        }
    }
    Ok(())
}

impl SessionRule {
    fn applies(&self, tool: &str, host: &str) -> bool {
        self.enabled
            && (self.tools.is_empty()
                || self.tools.iter().any(|t| {
                    t == tool
                        || (tool == "crawler" && t == "scanner")
                        || (tool == "scanner" && t == "crawler")
                }))
            && (self.hosts.is_empty() || self.hosts.iter().any(|h| host_matches(h, host)))
    }

    fn expired(&self, sent: &Sent) -> bool {
        if self.expired_statuses.contains(&sent.head.status) {
            return true;
        }
        !self.expired_regex.is_empty()
            && regex(&self.expired_regex)
                .is_ok_and(|re| re.is_match(&String::from_utf8_lossy(&sent.response)))
    }

    fn needs_login(&self, vars: &HashMap<String, String>) -> bool {
        self.headers.iter().any(|h| {
            self.extract.iter().any(|x| {
                h.value.contains(&format!("{{{{{}}}}}", x.name)) && !vars.contains_key(&x.name)
            })
        })
    }
}

/// Replaces `{{var}}` and `{{cred:id.field}}` in `text`.
fn fill(
    text: &str,
    vars: &HashMap<String, String>,
    creds: &[crate::keyring::Credential],
) -> String {
    let mut out = text.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    for c in creds {
        out = out
            .replace(&format!("{{{{cred:{}.username}}}}", c.id), &c.username)
            .replace(&format!("{{{{cred:{}.secret}}}}", c.id), &c.secret);
    }
    out
}

/// Sets (replaces or adds) a header in a raw request.
fn set_header(request: &[u8], name: &str, value: &str) -> Vec<u8> {
    let Some(end) = chosproxy_http::head::find_head_end(request) else {
        return request.to_vec();
    };
    let (head, body) = request.split_at(end);
    let text = String::from_utf8_lossy(head);
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text
        .split(eol)
        .filter(|l| !l.is_empty())
        .filter(|l| {
            !l.split_once(':')
                .is_some_and(|(n, _)| n.trim().eq_ignore_ascii_case(name))
        })
        .map(str::to_string)
        .collect();
    lines.push(format!("{name}: {value}"));
    let mut out = lines.join(eol).into_bytes();
    out.extend_from_slice(eol.as_bytes());
    out.extend_from_slice(eol.as_bytes());
    out.extend_from_slice(body);
    out
}

fn path_of(request: &[u8]) -> String {
    chosproxy_storage::request_path(&String::from_utf8_lossy(request))
}

impl Engine {
    /// The open project's session rules.
    pub async fn session_rules(&self) -> Result<Vec<SessionRule>, AppError> {
        let db = self.db()?;
        let raw = tokio::task::spawn_blocking(move || db.setting(SETTING_KEY)).await??;
        Ok(raw
            .and_then(|r| serde_json::from_str(&r).ok())
            .unwrap_or_default())
    }

    /// Saves the open project's session rules.
    pub async fn save_session_rules(&self, rules: Vec<SessionRule>) -> Result<(), AppError> {
        validate_rules(&rules).map_err(AppError::Invalid)?;
        let json = serde_json::to_string(&rules).map_err(|e| AppError::Invalid(e.to_string()))?;
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.set_setting(SETTING_KEY, Some(json))).await??;
        if let Ok(mut v) = self.inner.session_vars.lock() {
            v.clear();
        }
        self.audit("session.rules.changed", &format!("{} rule(s)", rules.len()));
        Ok(())
    }

    /// The request with the rule's cookies and headers applied.
    async fn session_prepare(
        &self,
        rule: &SessionRule,
        host: &str,
        https: bool,
        request: &[u8],
    ) -> Vec<u8> {
        let mut out = request.to_vec();
        if rule.use_cookie_jar
            && let Ok(db) = self.db()
        {
            let jar = tokio::task::spawn_blocking(move || db.cookie_jar_list())
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            let matching = cookies::cookies_for(&jar, host, &path_of(request), https, now_ms());
            out = cookies::apply_cookies(&out, &matching).0;
        }
        if !rule.headers.is_empty() {
            let vars = self
                .inner
                .session_vars
                .lock()
                .map(|v| v.get(&rule.name).cloned().unwrap_or_default())
                .unwrap_or_default();
            let creds = self.all_credentials().await;
            for h in &rule.headers {
                out = set_header(&out, &h.name, &fill(&h.value, &vars, &creds));
            }
        }
        out
    }

    /// Runs a rule's login macro: each stored request is sent again (jar
    /// cookies and variables filled in), its cookies stored, variables
    /// extracted, and the exchange recorded in the history.
    async fn session_login(&self, rule: &SessionRule, tool_source: Source) -> Result<(), String> {
        let _one_at_a_time = self.inner.session_macro.lock().await;
        let db = self.db().map_err(|e| e.to_string())?;
        let ids = rule.macro_requests.clone();
        let stored = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || db.exchanges_bulk(&ids))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?
        };
        let creds = self.all_credentials().await;
        let mut vars = self
            .inner
            .session_vars
            .lock()
            .map(|v| v.get(&rule.name).cloned().unwrap_or_default())
            .unwrap_or_default();
        for id in &rule.macro_requests {
            let Some(ex) = stored.iter().find(|e| e.row.id == *id) else {
                return Err(format!("macro request {id} is not in the history"));
            };
            let target = chosproxy_storage::RepeaterTarget {
                scheme: ex.row.scheme.clone(),
                host: ex.row.host.clone(),
                port: ex.row.port,
            };
            let https = ex.row.scheme == "https";
            let text = fill(&String::from_utf8_lossy(&ex.request), &vars, &creds);
            let mut request = chosproxy_http::edit::update_content_length(text.as_bytes(), false);
            let jar = {
                let db = db.clone();
                tokio::task::spawn_blocking(move || db.cookie_jar_list())
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or_default()
            };
            let matching =
                cookies::cookies_for(&jar, &target.host, &path_of(&request), https, now_ms());
            request = cookies::apply_cookies(&request, &matching).0;
            let security = self.security_for_host(&target.host, https).await?;
            let destination = Destination {
                host: target.host.clone(),
                port: target.port,
                tls_name: target.host.clone(),
            };
            let started_at = now_ms();
            let outcome =
                chosproxy_request::send(&destination, &security, &request, &SendLimits::default())
                    .await;
            if let Ok(sent) = &outcome {
                let entries = cookies::entries_from_response(
                    &sent.response,
                    &target.host,
                    &path_of(&request),
                    now_ms(),
                );
                let text = String::from_utf8_lossy(&sent.response).to_string();
                for x in &rule.extract {
                    if let Ok(re) = regex(&x.regex)
                        && let Some(c) = re.captures(&text)
                        && let Some(m) = c.get(1).or_else(|| c.get(0))
                    {
                        vars.insert(x.name.clone(), m.as_str().to_string());
                    }
                }
                let db = db.clone();
                let _ =
                    tokio::task::spawn_blocking(move || db.cookie_jar_store("repeater", entries))
                        .await;
            }
            let mut exchange =
                crate::repeater::captured(tool_source, &target, request, started_at, &outcome);
            exchange.notes = Some(format!("Login macro of the session rule “{}”.", rule.name));
            let db2 = db.clone();
            let _ = tokio::task::spawn_blocking(move || db2.insert(exchange)).await;
            if let Err(e) = outcome {
                return Err(format!("macro request {id} failed: {}", e.message));
            }
        }
        if let Ok(mut v) = self.inner.session_vars.lock() {
            v.insert(rule.name.clone(), vars);
        }
        Ok(())
    }

    async fn all_credentials(&self) -> Vec<crate::keyring::Credential> {
        let k = crate::keyring::Keyring::new(self.inner.paths.data_dir.join("keyring"));
        tokio::task::spawn_blocking(move || k.load())
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default()
    }

    /// How a tool connects to `host`: plain, or TLS with the verification
    /// the TLS settings ask for that host (the one place that decides it).
    pub(super) async fn tls_security(
        &self,
        host: &str,
        https: bool,
    ) -> Result<UpstreamSecurity, AppError> {
        if !https {
            return Ok(UpstreamSecurity::Plain);
        }
        let verification = if self.inner.proxy.tls_policy().skips_verification(host) {
            chosproxy_tls::UpstreamVerification::NotVerified
        } else {
            chosproxy_tls::UpstreamVerification::Verified
        };
        Ok(UpstreamSecurity::Tls {
            tls: self.repeater_tls().await?,
            verification,
        })
    }

    /// [`Engine::tls_security`] with the error as text.
    pub(super) async fn security_for_host(
        &self,
        host: &str,
        https: bool,
    ) -> Result<UpstreamSecurity, String> {
        self.tls_security(host, https)
            .await
            .map_err(|e| e.to_string())
    }

    /// Sends a tool's request through the tool's resource pool and the
    /// matching session rule, if any (`tool`: `repeater`, `fuzzer`,
    /// `sequencer`, `crawler`, `plugin`). Without either this is
    /// `chosproxy_request::send`.
    pub(crate) async fn session_send(
        &self,
        tool: &str,
        source: Source,
        destination: &Destination,
        security: &UpstreamSecurity,
        request: &[u8],
        limits: &SendLimits,
    ) -> Result<Sent, SendError> {
        if let Err(e) = self.ensure_writable() {
            return Err(SendError {
                message: e.to_string(),
                timing: Default::default(),
                peer: None,
            });
        }
        let ticket = self.pool_enter(tool).await;
        let mut outcome = self
            .session_send_unpooled(tool, source, destination, security, request, limits)
            .await;
        if let Some(ticket) = ticket {
            ticket.finish(&mut outcome).await;
        }
        outcome
    }

    async fn session_send_unpooled(
        &self,
        tool: &str,
        source: Source,
        destination: &Destination,
        security: &UpstreamSecurity,
        request: &[u8],
        limits: &SendLimits,
    ) -> Result<Sent, SendError> {
        let rule = match self.session_rules().await {
            Ok(rules) => rules
                .into_iter()
                .find(|r| r.applies(tool, &destination.host)),
            Err(_) => None,
        };
        let Some(rule) = rule else {
            return chosproxy_request::send(destination, security, request, limits).await;
        };
        let https = matches!(security, UpstreamSecurity::Tls { .. });
        let mut notes = Vec::new();
        let vars_missing = self
            .inner
            .session_vars
            .lock()
            .map(|v| rule.needs_login(v.get(&rule.name).unwrap_or(&HashMap::new())))
            .unwrap_or(false);
        if vars_missing && !rule.macro_requests.is_empty() {
            match self.session_login(&rule, source).await {
                Ok(()) => notes.push(format!("Logged in with the session rule “{}”.", rule.name)),
                Err(e) => notes.push(format!("Session login failed: {e}.")),
            }
        }
        let prepared = self
            .session_prepare(&rule, &destination.host, https, request)
            .await;
        let mut outcome = chosproxy_request::send(destination, security, &prepared, limits).await;
        let mut final_request = prepared;
        if let Ok(sent) = &outcome
            && rule.expired(sent)
            && !rule.macro_requests.is_empty()
        {
            match self.session_login(&rule, source).await {
                Ok(()) => {
                    notes.push(format!(
                        "The session had expired; logged in again with “{}” and sent again.",
                        rule.name
                    ));
                    final_request = self
                        .session_prepare(&rule, &destination.host, https, request)
                        .await;
                    outcome =
                        chosproxy_request::send(destination, security, &final_request, limits)
                            .await;
                }
                Err(e) => notes.push(format!("Session login failed: {e}.")),
            }
        }
        outcome.map(|mut sent| {
            if sent.sent_request.is_none() && final_request != request {
                sent.sent_request = Some(final_request);
            }
            let mut all = notes;
            all.append(&mut sent.notes);
            if all.is_empty() {
                all.push(format!("Session rule “{}”.", rule.name));
            }
            sent.notes = all;
            sent
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn headers_variables_and_validation() {
        let mut vars = HashMap::new();
        vars.insert("csrf".to_string(), "t0k".to_string());
        let creds = vec![crate::keyring::Credential {
            id: "lab".into(),
            label: "Lab".into(),
            username: "u".into(),
            secret: "p".into(),
        }];
        assert_eq!(
            fill(
                "{{csrf}} {{cred:lab.username}}:{{cred:lab.secret}}",
                &vars,
                &creds
            ),
            "t0k u:p"
        );
        let r = set_header(
            b"GET / HTTP/1.1\r\nX-Csrf: old\r\nHost: a\r\n\r\nbody",
            "x-csrf",
            "new",
        );
        assert_eq!(
            String::from_utf8_lossy(&r),
            "GET / HTTP/1.1\r\nHost: a\r\nx-csrf: new\r\n\r\nbody"
        );
        let rule = SessionRule {
            enabled: true,
            name: "Lab".into(),
            hosts: vec!["*.lab.test".into()],
            tools: vec!["fuzzer".into()],
            use_cookie_jar: true,
            expired_statuses: vec![401],
            expired_regex: "please log in".into(),
            macro_requests: vec![1],
            extract: vec![SessionExtraction {
                name: "csrf".into(),
                regex: "name=\"csrf\" value=\"([^\"]+)\"".into(),
            }],
            headers: vec![SessionHeader {
                name: "X-Csrf".into(),
                value: "{{csrf}}".into(),
            }],
        };
        assert!(validate_rules(std::slice::from_ref(&rule)).is_ok());
        assert!(rule.applies("fuzzer", "a.lab.test"));
        assert!(!rule.applies("repeater", "a.lab.test"));
        assert!(!rule.applies("fuzzer", "lab.test"));
        assert!(rule.needs_login(&HashMap::new()));
        assert!(!rule.needs_login(&vars));
        let mut bad = rule.clone();
        bad.expired_regex = "(".into();
        assert!(validate_rules(&[bad]).is_err());
    }
}
