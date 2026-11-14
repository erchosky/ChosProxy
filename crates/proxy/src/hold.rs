//! Intercept: messages paused until the user forwards (possibly edited) or
//! drops them.

use chosproxy_core::Scope;
use chosproxy_core::config::InterceptRules;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use ts_rs::TS;

/// Most messages that can wait at the same time; beyond this new messages
/// pass through (and the user is told) instead of piling up.
pub const MAX_PENDING: usize = 500;
/// Most bytes held in the queue at once (messages are at most 8 MiB each).
pub const MAX_PENDING_BYTES: usize = 256 * 1024 * 1024;

/// What kind of message is paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HeldKind {
    /// A request on its way to the server.
    Request,
    /// A response on its way to the client.
    Response,
    /// A WebSocket message from the client to the server.
    WsToServer,
    /// A WebSocket message from the server to the client.
    WsToClient,
}

impl HeldKind {
    /// A WebSocket message (the raw bytes are its payload).
    pub fn is_websocket(self) -> bool {
        matches!(self, HeldKind::WsToServer | HeldKind::WsToClient)
    }
}

/// A paused message as listed in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HeldSummary {
    /// Queue id.
    #[ts(type = "number")]
    pub id: u64,
    /// Request or response.
    pub kind: HeldKind,
    /// When it was paused (Unix ms).
    #[ts(type = "number")]
    pub held_at: i64,
    /// Method of the request.
    pub method: String,
    /// Full URL of the request.
    pub url: String,
    /// Target host.
    pub host: String,
    /// Response status (responses only).
    pub status: Option<u16>,
    /// Size of the paused message.
    #[ts(type = "number")]
    pub size: u64,
    /// WebSocket messages: `text` or `binary`.
    pub ws_opcode: Option<String>,
}

/// A paused message with its bytes.
#[derive(Debug, Clone)]
pub struct HeldMessage {
    /// Listing data.
    pub summary: HeldSummary,
    /// Current bytes (original, or replaced e.g. by unpacking).
    pub raw: Vec<u8>,
    /// For responses: the request it answers (context for the user).
    pub request: Option<Vec<u8>>,
}

/// The user's decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Send these bytes on. For requests, `intercept_response` also pauses
    /// the matching response.
    Forward {
        /// Bytes to send (edited or not).
        raw: Vec<u8>,
        /// Pause the response to this request.
        intercept_response: bool,
    },
    /// Do not deliver the message.
    Drop,
}

struct Pending {
    message: HeldMessage,
    original: Vec<u8>,
    reply: oneshot::Sender<Decision>,
}

struct State {
    enabled: bool,
    rules: Arc<InterceptRules>,
    scope: Arc<Scope>,
    next_id: u64,
    pending: BTreeMap<u64, Pending>,
}

impl State {
    fn held_bytes(&self) -> usize {
        self.pending
            .values()
            .map(|p| p.message.raw.len() + p.original.len())
            .sum()
    }
}

/// The queue of paused messages plus the Intercept switch and rules.
pub struct InterceptQueue {
    state: Mutex<State>,
    on_change: Box<dyn Fn(usize) + Send + Sync>,
}

impl std::fmt::Debug for InterceptQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InterceptQueue").finish_non_exhaustive()
    }
}

impl InterceptQueue {
    /// A queue with Intercept off. `on_change` receives the number of
    /// pending messages whenever the queue or the switch changes.
    pub(crate) fn new(on_change: Box<dyn Fn(usize) + Send + Sync>) -> InterceptQueue {
        InterceptQueue {
            state: Mutex::new(State {
                enabled: false,
                rules: Arc::new(InterceptRules::default()),
                scope: Arc::default(),
                next_id: 1,
                pending: BTreeMap::new(),
            }),
            on_change,
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        let mut guard = match self.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        f(&mut guard)
    }

    fn changed(&self) {
        let n = self.with(|s| s.pending.len());
        (self.on_change)(n);
    }

    /// Whether Intercept is on.
    pub fn is_enabled(&self) -> bool {
        self.with(|s| s.enabled)
    }

    /// Current rules.
    pub fn rules(&self) -> Arc<InterceptRules> {
        self.with(|s| s.rules.clone())
    }

    /// Replaces the rules (applies to new messages).
    pub fn set_rules(&self, rules: InterceptRules) {
        self.with(|s| s.rules = Arc::new(rules));
    }

    /// Scope used by the "only in scope" rule.
    pub fn scope(&self) -> Arc<Scope> {
        self.with(|s| s.scope.clone())
    }

    /// Replaces the scope (applies to new messages).
    pub fn set_scope(&self, scope: Scope) {
        self.with(|s| s.scope = Arc::new(scope));
    }

    /// Switches Intercept on or off. Turning it off releases every paused
    /// message unmodified, as the user would otherwise be stuck.
    pub fn set_enabled(&self, enabled: bool) {
        let released = self.with(|s| {
            s.enabled = enabled;
            if enabled {
                Vec::new()
            } else {
                std::mem::take(&mut s.pending).into_values().collect()
            }
        });
        for p in released {
            let _ = p.reply.send(Decision::Forward {
                raw: p.original,
                intercept_response: false,
            });
        }
        self.changed();
    }

    /// Pauses `message`. Returns its id and the decision channel, or `None`
    /// (do not pause) when Intercept is off or the queue is full.
    pub(crate) fn hold(
        &self,
        mut message: HeldMessage,
    ) -> Option<(u64, oneshot::Receiver<Decision>)> {
        let (tx, rx) = oneshot::channel();
        let accepted = self.with(|s| {
            let incoming = message.raw.len() * 2;
            if !s.enabled
                || s.pending.len() >= MAX_PENDING
                || s.held_bytes() + incoming > MAX_PENDING_BYTES
            {
                return None;
            }
            let id = s.next_id;
            s.next_id += 1;
            message.summary.id = id;
            let original = message.raw.clone();
            s.pending.insert(
                id,
                Pending {
                    message,
                    original,
                    reply: tx,
                },
            );
            Some(id)
        });
        let id = accepted?;
        self.changed();
        Some((id, rx))
    }

    /// Paused messages, oldest first.
    pub fn pending(&self) -> Vec<HeldSummary> {
        self.with(|s| {
            s.pending
                .values()
                .map(|p| p.message.summary.clone())
                .collect()
        })
    }

    /// One paused message.
    pub fn get(&self, id: u64) -> Option<HeldMessage> {
        self.with(|s| s.pending.get(&id).map(|p| p.message.clone()))
    }

    /// Replaces the bytes shown for a paused message (e.g. after unpacking a
    /// compressed body). Returns false if it is no longer paused.
    pub fn replace(&self, id: u64, raw: Vec<u8>) -> bool {
        self.with(|s| match s.pending.get_mut(&id) {
            Some(p) => {
                p.message.summary.size = raw.len() as u64;
                p.message.raw = raw;
                true
            }
            None => false,
        })
    }

    /// Applies the user's decision. Returns false if the message is no
    /// longer paused (already decided, or its connection is gone).
    pub fn decide(&self, id: u64, decision: Decision) -> bool {
        let pending = self.with(|s| s.pending.remove(&id));
        let Some(p) = pending else {
            return false;
        };
        let delivered = p.reply.send(decision).is_ok();
        self.changed();
        delivered
    }

    /// Forwards every paused message as currently shown.
    pub fn forward_all(&self) {
        let all: Vec<Pending> =
            self.with(|s| std::mem::take(&mut s.pending).into_values().collect());
        for p in all {
            let _ = p.reply.send(Decision::Forward {
                raw: p.message.raw,
                intercept_response: false,
            });
        }
        self.changed();
    }

    /// Removes a message whose connection ended while it was paused.
    pub(crate) fn abandon(&self, id: u64) {
        if self.with(|s| s.pending.remove(&id).is_some()) {
            self.changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn message(raw: &[u8]) -> HeldMessage {
        HeldMessage {
            summary: HeldSummary {
                id: 0,
                kind: HeldKind::Request,
                held_at: 0,
                method: "GET".into(),
                url: "http://a/".into(),
                host: "a".into(),
                status: None,
                size: raw.len() as u64,
                ws_opcode: None,
            },
            raw: raw.to_vec(),
            request: None,
        }
    }

    fn queue() -> (InterceptQueue, Arc<AtomicUsize>) {
        let last = Arc::new(AtomicUsize::new(usize::MAX));
        let l = last.clone();
        (
            InterceptQueue::new(Box::new(move |n| l.store(n, Ordering::SeqCst))),
            last,
        )
    }

    #[tokio::test]
    async fn holds_only_when_enabled_and_delivers_decisions() {
        let (q, last) = queue();
        assert!(q.hold(message(b"x")).is_none(), "off by default");
        q.set_enabled(true);
        let (_, rx) = q.hold(message(b"GET / HTTP/1.1\r\n\r\n")).unwrap();
        assert_eq!(last.load(Ordering::SeqCst), 1);
        let id = q.pending()[0].id;
        assert!(q.replace(id, b"edited".to_vec()));
        assert_eq!(q.get(id).unwrap().raw, b"edited");
        assert!(q.decide(id, Decision::Drop));
        assert_eq!(rx.await.unwrap(), Decision::Drop);
        assert!(!q.decide(id, Decision::Drop), "already decided");
        assert_eq!(last.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn turning_off_releases_originals_and_forward_all_sends_current() {
        let (q, _) = queue();
        q.set_enabled(true);
        let (_, a) = q.hold(message(b"a")).unwrap();
        let (_, b) = q.hold(message(b"b")).unwrap();
        let id_b = q.pending()[1].id;
        q.replace(id_b, b"B".to_vec());
        q.set_enabled(false);
        assert!(matches!(a.await.unwrap(), Decision::Forward { raw, .. } if raw == b"a"));
        assert!(matches!(b.await.unwrap(), Decision::Forward { raw, .. } if raw == b"b"));

        q.set_enabled(true);
        let (_, c) = q.hold(message(b"c")).unwrap();
        let id_c = q.pending()[0].id;
        q.replace(id_c, b"C".to_vec());
        q.forward_all();
        assert!(matches!(c.await.unwrap(), Decision::Forward { raw, .. } if raw == b"C"));
        assert!(q.pending().is_empty());
    }

    #[test]
    fn abandoned_messages_leave_the_queue() {
        let (q, last) = queue();
        q.set_enabled(true);
        let (id, _rx) = q.hold(message(b"x")).unwrap();
        q.abandon(id);
        assert!(q.pending().is_empty());
        assert_eq!(last.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn queue_bytes_are_bounded() {
        let (q, _) = queue();
        q.set_enabled(true);
        // Raw and original copies are both counted: 2 x 1/3 fits, 4 x 1/3 does not.
        let big = vec![b'x'; MAX_PENDING_BYTES / 3];
        let first = q.hold(message(&big));
        assert!(first.is_some());
        assert!(
            q.hold(message(&big)).is_none(),
            "would exceed the byte budget"
        );
        drop(first);
    }

    #[test]
    fn queue_is_bounded() {
        let (q, _) = queue();
        q.set_enabled(true);
        let receivers: Vec<_> = (0..MAX_PENDING)
            .map(|_| q.hold(message(b"m")).unwrap())
            .collect();
        assert!(q.hold(message(b"m")).is_none());
        drop(receivers);
    }
}
