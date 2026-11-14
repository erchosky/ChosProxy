//! Match & Replace: proxied messages changed automatically by rules
//! (first line, each header line, or the body), before Intercept sees them.

use chosproxy_core::config::{MatchReplaceRule, MatchTarget};
use chosproxy_http::content::split_message;
use chosproxy_http::edit::{is_packed, unpack, update_content_length};
use chosproxy_script::Script;
use regex::bytes::{NoExpand, Regex, RegexBuilder};
use std::sync::Arc;

/// Which message a rule set is applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Request,
    Response,
}

struct Compiled {
    target: MatchTarget,
    /// Script targets: the rewrite script.
    script: Option<Arc<Script>>,
    /// `None`: an empty match, which adds a header line.
    re: Option<Regex>,
    replace: Vec<u8>,
    expand: bool,
    label: String,
}

/// Compiled rules, ready to apply.
#[derive(Default)]
pub struct MatchReplace {
    rules: Vec<Compiled>,
}

impl std::fmt::Debug for MatchReplace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatchReplace")
            .field("rules", &self.rules.len())
            .finish()
    }
}

/// A message after the rules ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Applied {
    pub(crate) message: Vec<u8>,
    /// Labels of the rules that changed something.
    pub(crate) rules: Vec<String>,
    /// Body rules could not run (why).
    pub(crate) skipped: Option<String>,
}

impl MatchReplace {
    /// Compiles the enabled rules (script rules are left out).
    pub fn new(rules: &[MatchReplaceRule]) -> Result<MatchReplace, String> {
        Self::with_scripts(rules, &|_| None)
    }

    /// Compiles the enabled rules; `script` finds the rewrite script a
    /// script rule names (rules whose script is missing or disabled are
    /// left out).
    pub fn with_scripts(
        rules: &[MatchReplaceRule],
        script: &dyn Fn(&str) -> Option<Arc<Script>>,
    ) -> Result<MatchReplace, String> {
        let mut out = Vec::new();
        for (i, r) in rules.iter().enumerate() {
            r.validate(i + 1)?;
            if !r.enabled {
                continue;
            }
            if r.target.is_script() {
                if let Some(s) = script(&r.pattern) {
                    out.push(Compiled {
                        target: r.target,
                        script: Some(s),
                        re: None,
                        replace: Vec::new(),
                        expand: false,
                        label: if r.comment.trim().is_empty() {
                            format!("script {}", r.pattern)
                        } else {
                            r.comment.trim().to_string()
                        },
                    });
                }
                continue;
            }
            let re = if r.pattern.is_empty() {
                None
            } else {
                let source = if r.regex {
                    r.pattern.clone()
                } else {
                    regex::escape(&r.pattern)
                };
                Some(
                    RegexBuilder::new(&source)
                        .size_limit(MatchReplaceRule::REGEX_SIZE_LIMIT)
                        .build()
                        .map_err(|e| format!("rule {}: {e}", i + 1))?,
                )
            };
            out.push(Compiled {
                target: r.target,
                script: None,
                re,
                replace: r.replace.as_bytes().to_vec(),
                expand: r.regex,
                label: if r.comment.trim().is_empty() {
                    format!("rule {}", i + 1)
                } else {
                    r.comment.trim().to_string()
                },
            });
        }
        Ok(MatchReplace { rules: out })
    }

    fn for_side(&self, side: Side) -> impl Iterator<Item = &Compiled> {
        self.rules.iter().filter(move |r| {
            !r.target.is_websocket() && r.target.is_request() == (side == Side::Request)
        })
    }

    /// True if rules change WebSocket text messages in this direction.
    pub(crate) fn has_websocket(&self, to_server: bool) -> bool {
        let target = if to_server {
            MatchTarget::WsToServer
        } else {
            MatchTarget::WsToClient
        };
        self.rules
            .iter()
            .any(|r| r.target == target || r.target == MatchTarget::WsScript)
    }

    /// Applies the WebSocket rules of one direction to a text message.
    /// `None` when nothing changed.
    pub(crate) fn apply_websocket(&self, to_server: bool, payload: &[u8]) -> Option<Applied> {
        let target = if to_server {
            MatchTarget::WsToServer
        } else {
            MatchTarget::WsToClient
        };
        let mut used = Vec::new();
        let mut current = payload.to_vec();
        let mut skipped = None;
        // Rules run in their order, text rules and script rules alike.
        for r in self
            .rules
            .iter()
            .filter(|r| r.target == target || r.target == MatchTarget::WsScript)
        {
            if let Some(script) = &r.script {
                let Ok(text) = std::str::from_utf8(&current) else {
                    skipped = Some(format!("{} not applied: the message is not text", r.label));
                    continue;
                };
                match script.websocket(to_server, text) {
                    Ok(run) => {
                        if let Some(new) = run.value {
                            current = new.into_bytes();
                            note(&mut used, &r.label);
                        }
                    }
                    // A failing script leaves the message as it is.
                    Err(e) => skipped = Some(format!("{} failed: {e}", r.label)),
                }
                continue;
            }
            let Some(re) = &r.re else { continue };
            if let Some(changed) = replace(re, &current, &r.replace, r.expand) {
                current = changed;
                note(&mut used, &r.label);
            }
        }
        (current != payload || skipped.is_some()).then_some(Applied {
            message: current,
            rules: used,
            skipped,
        })
    }

    /// True if any rule applies to this side.
    pub(crate) fn has(&self, side: Side) -> bool {
        self.for_side(side).next().is_some()
    }

    /// True if a body or script rule applies to this side (the whole
    /// message must be buffered).
    pub(crate) fn has_body(&self, side: Side) -> bool {
        self.for_side(side)
            .any(|r| r.target.is_body() || r.target.is_script())
    }

    /// Applies the first-line and header rules to a message head (ending
    /// with its blank line). `None` when nothing changed.
    pub(crate) fn apply_head(&self, side: Side, head: &[u8]) -> Option<Applied> {
        let mut used = Vec::new();
        let out = self.rewrite_head(side, head, &mut used);
        (out != head).then_some(Applied {
            message: out,
            rules: used,
            skipped: None,
        })
    }

    fn rewrite_head(&self, side: Side, head: &[u8], used: &mut Vec<String>) -> Vec<u8> {
        let lines: Vec<&[u8]> = head.split_inclusive(|b| *b == b'\n').collect();
        let eol: &[u8] = if head.ends_with(b"\r\n\r\n") || head.ends_with(b"\r\n") {
            b"\r\n"
        } else {
            b"\n"
        };
        let (first_target, header_target) = match side {
            Side::Request => (MatchTarget::RequestFirstLine, MatchTarget::RequestHeader),
            Side::Response => (MatchTarget::ResponseFirstLine, MatchTarget::ResponseHeader),
        };
        let mut out = Vec::with_capacity(head.len() + 64);
        for (i, line) in lines.iter().enumerate() {
            let (content, ending) = split_eol(line);
            let terminator = i > 0 && content.is_empty();
            if terminator {
                for r in self.rules.iter().filter(|r| r.target == header_target) {
                    if r.re.is_none() {
                        out.extend_from_slice(&r.replace);
                        out.extend_from_slice(eol);
                        note(used, &r.label);
                    }
                }
                out.extend_from_slice(line);
                continue;
            }
            let target = if i == 0 { first_target } else { header_target };
            let mut current = content.to_vec();
            for r in self.rules.iter().filter(|r| r.target == target) {
                let Some(re) = &r.re else { continue };
                if let Some(changed) = replace(re, &current, &r.replace, r.expand) {
                    current = changed;
                    note(used, &r.label);
                }
            }
            // A header line emptied by a rule is removed.
            if i > 0 && current.is_empty() && !content.is_empty() {
                continue;
            }
            out.extend_from_slice(&current);
            out.extend_from_slice(ending);
        }
        out
    }

    /// Applies every rule of `side` to a complete message. `limit` bounds a
    /// decompressed body. `None` when nothing changed.
    pub(crate) fn apply(&self, side: Side, message: &[u8], limit: usize) -> Option<Applied> {
        let mut used = Vec::new();
        let (head, _) = split_message(message);
        let new_head = self.rewrite_head(side, head, &mut used);
        let mut current = new_head;
        current.extend_from_slice(&message[head.len()..]);
        let body_target = match side {
            Side::Request => MatchTarget::RequestBody,
            Side::Response => MatchTarget::ResponseBody,
        };
        let body_rules: Vec<&Compiled> = self
            .rules
            .iter()
            .filter(|r| r.target == body_target)
            .collect();
        let mut skipped = None;
        if !body_rules.is_empty() {
            let unpacked = if is_packed(&current) {
                match unpack(&current, limit) {
                    Ok(u) => Some(u),
                    Err(e) => {
                        skipped = Some(format!("body rules not applied: {e}"));
                        None
                    }
                }
            } else {
                Some(current.clone())
            };
            if let Some(plain) = unpacked {
                let (h, body) = split_message(&plain);
                let mut new_body = body.to_vec();
                let mut changed = false;
                for r in body_rules {
                    let Some(re) = &r.re else { continue };
                    if let Some(b) = replace(re, &new_body, &r.replace, r.expand) {
                        new_body = b;
                        changed = true;
                        note(&mut used, &r.label);
                    }
                }
                if changed {
                    let mut rebuilt = h.to_vec();
                    rebuilt.extend_from_slice(&new_body);
                    current = update_content_length(&rebuilt, true);
                }
            }
        }
        let script_target = match side {
            Side::Request => MatchTarget::RequestScript,
            Side::Response => MatchTarget::ResponseScript,
        };
        for r in self.rules.iter().filter(|r| r.target == script_target) {
            let Some(script) = &r.script else { continue };
            let plain = if is_packed(&current) {
                match unpack(&current, limit) {
                    Ok(u) => u,
                    Err(e) => {
                        skipped = Some(format!("{} not applied: {e}", r.label));
                        continue;
                    }
                }
            } else {
                current.clone()
            };
            let Ok(text) = std::str::from_utf8(&plain) else {
                skipped = Some(format!("{} not applied: the message is not text", r.label));
                continue;
            };
            match script.rewrite(side == Side::Response, text) {
                Ok(run) => {
                    if let Some(new) = run.value {
                        current = update_content_length(new.as_bytes(), true);
                        note(&mut used, &r.label);
                    }
                }
                Err(e) => skipped = Some(format!("{} failed: {e}", r.label)),
            }
        }
        if current == message && skipped.is_none() {
            return None;
        }
        Some(Applied {
            message: current,
            rules: used,
            skipped,
        })
    }
}

fn note(used: &mut Vec<String>, label: &str) {
    if !used.iter().any(|u| u == label) {
        used.push(label.to_string());
    }
}

fn split_eol(line: &[u8]) -> (&[u8], &[u8]) {
    if let Some(c) = line.strip_suffix(b"\r\n") {
        (c, b"\r\n")
    } else if let Some(c) = line.strip_suffix(b"\n") {
        (c, b"\n")
    } else {
        (line, b"")
    }
}

fn replace(re: &Regex, text: &[u8], with: &[u8], expand: bool) -> Option<Vec<u8>> {
    if !re.is_match(text) {
        return None;
    }
    let out = if expand {
        re.replace_all(text, with).into_owned()
    } else {
        re.replace_all(text, NoExpand(with)).into_owned()
    };
    (out != text).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(target: MatchTarget, pattern: &str, replace: &str, regex: bool) -> MatchReplaceRule {
        MatchReplaceRule {
            enabled: true,
            target,
            pattern: pattern.into(),
            replace: replace.into(),
            regex,
            comment: String::new(),
        }
    }

    #[test]
    fn headers_are_replaced_removed_and_added() {
        let mr = MatchReplace::new(&[
            rule(
                MatchTarget::RequestHeader,
                r"^User-Agent:.*$",
                "User-Agent: ChosProxy",
                true,
            ),
            rule(MatchTarget::RequestHeader, r"^Cookie:.*$", "", true),
            rule(MatchTarget::RequestHeader, "", "X-Test: 1", false),
            MatchReplaceRule {
                comment: "never".into(),
                ..rule(MatchTarget::ResponseHeader, "Server", "S", false)
            },
        ])
        .unwrap();
        let head = b"GET / HTTP/1.1\r\nHost: a\r\nUser-Agent: curl/8\r\nCookie: s=1\r\n\r\n";
        let a = mr.apply_head(Side::Request, head).unwrap();
        assert_eq!(
            a.message,
            b"GET / HTTP/1.1\r\nHost: a\r\nUser-Agent: ChosProxy\r\nX-Test: 1\r\n\r\n"
        );
        assert_eq!(a.rules, vec!["rule 1", "rule 2", "rule 3"]);
        assert!(mr.has(Side::Response) && !mr.has_body(Side::Request));
    }

    #[test]
    fn first_line_and_literal_text() {
        let mr = MatchReplace::new(&[rule(MatchTarget::RequestFirstLine, "/v1/", "/v2/$1", false)])
            .unwrap();
        let a = mr
            .apply_head(Side::Request, b"GET /v1/x HTTP/1.1\r\n\r\n")
            .unwrap();
        assert_eq!(
            a.message, b"GET /v2/$1x HTTP/1.1\r\n\r\n",
            "literal: no groups"
        );
        assert!(
            mr.apply_head(Side::Request, b"GET /v3 HTTP/1.1\r\n\r\n")
                .is_none()
        );
    }

    #[test]
    fn bodies_are_decoded_then_replaced() {
        use std::io::Write;
        let mr = MatchReplace::new(&[rule(
            MatchTarget::ResponseBody,
            r#""admin":\s*false"#,
            r#""admin":true"#,
            true,
        )])
        .unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(br#"{"admin": false}"#).unwrap();
        let body = gz.finish().unwrap();
        let mut msg = format!(
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        msg.extend_from_slice(&body);
        let a = mr.apply(Side::Response, &msg, 1 << 20).unwrap();
        assert_eq!(
            a.message,
            b"HTTP/1.1 200 OK\r\nContent-Length: 14\r\n\r\n{\"admin\":true}".to_vec()
        );
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc";
        assert!(mr.apply(Side::Response, plain, 1 << 20).is_none());
    }

    #[test]
    fn script_rules_rewrite_whole_messages() {
        let script = Arc::new(
            Script::compile(
                chosproxy_script::Kind::Rewrite,
                r#"fn response(text) { text.replace("secret", "[hidden]"); text }"#,
            )
            .unwrap(),
        );
        let rule = MatchReplaceRule {
            enabled: true,
            target: MatchTarget::ResponseScript,
            pattern: "hide".into(),
            replace: String::new(),
            regex: false,
            comment: String::new(),
        };
        let mr = MatchReplace::with_scripts(std::slice::from_ref(&rule), &|id| {
            (id == "hide").then(|| script.clone())
        })
        .unwrap();
        assert!(mr.has_body(Side::Response) && !mr.has(Side::Request));
        let a = mr
            .apply(
                Side::Response,
                b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nmy secret",
                1 << 20,
            )
            .unwrap();
        assert_eq!(
            a.message,
            b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\nmy [hidden]"
        );
        assert_eq!(a.rules, vec!["script hide"]);
        // Binary messages are left alone, with a note.
        let a = mr
            .apply(Side::Response, b"HTTP/1.1 200 OK\r\n\r\n\xff\xfe", 1 << 20)
            .unwrap();
        assert!(a.skipped.unwrap().contains("not text"));
        // A missing script leaves the rule out.
        assert!(!MatchReplace::new(&[rule]).unwrap().has(Side::Response));
    }

    #[test]
    fn websocket_rules() {
        let mr = MatchReplace::new(&[rule(
            MatchTarget::WsToServer,
            r#""role":"user""#,
            r#""role":"admin""#,
            false,
        )])
        .unwrap();
        assert!(mr.has_websocket(true) && !mr.has_websocket(false));
        assert!(!mr.has(Side::Request) && !mr.has(Side::Response));
        let a = mr.apply_websocket(true, br#"{"role":"user"}"#).unwrap();
        assert_eq!(a.message, br#"{"role":"admin"}"#);
        assert!(mr.apply_websocket(false, br#"{"role":"user"}"#).is_none());
    }

    #[test]
    fn websocket_script_rules() {
        let script = Arc::new(
            Script::compile(
                chosproxy_core::config::ScriptKind::Rewrite,
                r#"fn websocket(text, to_server) {
                    if to_server { regex_replace(text, "user", "admin") }
                    else if text.contains("boom") { throw "no" }
                    else { () }
                }"#,
            )
            .unwrap(),
        );
        let mut by_script = rule(MatchTarget::WsScript, "ws", "", false);
        by_script.comment = "roles".into();
        let rules = [
            rule(MatchTarget::WsToServer, "guest", "user", false),
            by_script,
        ];
        let mr =
            MatchReplace::with_scripts(&rules, &|id| (id == "ws").then(|| script.clone())).unwrap();
        assert!(mr.has_websocket(true) && mr.has_websocket(false));
        assert!(!mr.has(Side::Request) && !mr.has_body(Side::Response));
        // In order: the text rule, then the script.
        let a = mr.apply_websocket(true, br#"{"role":"guest"}"#).unwrap();
        assert_eq!(a.message, br#"{"role":"admin"}"#);
        assert_eq!(a.rules, ["rule 1", "roles"]);
        // The other direction: unchanged, and a failing script changes nothing.
        assert!(mr.apply_websocket(false, b"user").is_none());
        let failed = mr.apply_websocket(false, b"boom").unwrap();
        assert_eq!(failed.message, b"boom");
        assert!(failed.skipped.unwrap().contains("roles failed"));
        // Without the script the rule is left out.
        assert!(!MatchReplace::new(&rules[1..]).unwrap().has_websocket(false));
    }

    #[test]
    fn disabled_and_invalid_rules() {
        let mut r = rule(MatchTarget::RequestBody, "a", "b", false);
        r.enabled = false;
        let mr = MatchReplace::new(&[r]).unwrap();
        assert!(!mr.has(Side::Request));
        assert!(MatchReplace::new(&[rule(MatchTarget::RequestBody, "(", "", true)]).is_err());
        assert!(
            MatchReplace::new(&[rule(MatchTarget::RequestHeader, "a", "x\r\ny", false)]).is_err()
        );
        assert!(MatchReplace::new(&[rule(MatchTarget::RequestBody, "", "x", false)]).is_err());
    }
}
