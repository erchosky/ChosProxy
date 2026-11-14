//! WebSocket relay: after a successful handshake the proxy reads frames in
//! both directions, forwards them unchanged (byte for byte), records every
//! message, and pauses data messages that match the Intercept rules.

use crate::conn::{Client, ConnCtx};
use crate::hold::{Decision, HeldKind, HeldMessage, HeldSummary, InterceptQueue};
use chosproxy_core::ScopeTarget;
use chosproxy_core::Source;
use chosproxy_core::time::now_ms;
use chosproxy_core::websocket::{WsCapturedMessage, WsDirection, WsEvent, ws_flags};
use chosproxy_http::Scheme;
use chosproxy_request::Upstream;
use chosproxy_websocket::{
    Assembler, Message, Opcode, encode_frame, parse_close, random_mask, read_frame,
};
use std::sync::atomic::{AtomicI64, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

/// Largest message paused for editing; larger ones pass through.
const MAX_HELD_MESSAGE: usize = 8 * 1024 * 1024;
/// Time the remaining direction gets to finish once the other has ended.
const CLOSE_GRACE: Duration = Duration::from_secs(5);

/// The connection being relayed.
#[derive(Debug, Clone)]
pub(crate) struct WsInfo {
    pub(crate) key: i64,
    pub(crate) scheme: Scheme,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path: String,
    pub(crate) url: String,
}

/// A key unique per connection (also across restarts: it starts from the
/// clock).
pub(crate) fn new_key() -> i64 {
    static NEXT: OnceLock<AtomicI64> = OnceLock::new();
    NEXT.get_or_init(|| AtomicI64::new(now_ms() << 12))
        .fetch_add(1, Ordering::Relaxed)
}

/// Abandons a paused message if its wait is cancelled (connection closed).
struct HoldGuard<'a> {
    queue: &'a InterceptQueue,
    id: u64,
    armed: bool,
}

impl Drop for HoldGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.queue.abandon(self.id);
        }
    }
}

struct Pump<'a> {
    ctx: &'a ConnCtx,
    info: &'a WsInfo,
    direction: WsDirection,
    close: &'a Mutex<Option<(Option<u16>, String)>>,
    /// Bit per direction set once that side has sent a close frame.
    closed_by: &'a AtomicU8,
    done: &'a CancellationToken,
}

impl Pump<'_> {
    fn to_server(&self) -> bool {
        self.direction == WsDirection::ToServer
    }

    fn should_hold(&self, opcode: Opcode) -> bool {
        if !matches!(opcode, Opcode::Text | Opcode::Binary) {
            return false;
        }
        let queue = self.ctx.shared.intercept();
        if !queue.is_enabled() {
            return false;
        }
        let rules = queue.rules();
        let in_scope = rules.only_in_scope
            && queue.scope().contains(&ScopeTarget {
                scheme: self.info.scheme.as_str(),
                host: &self.info.host,
                port: self.info.port,
                path: &self.info.path,
            });
        rules.intercepts_websocket(&self.info.host, in_scope, self.to_server())
    }

    /// Pauses a message; returns the payload to send (`None` = dropped).
    async fn decide(&self, message: &Message) -> Option<Vec<u8>> {
        let queue = self.ctx.shared.intercept();
        let held = HeldMessage {
            summary: HeldSummary {
                id: 0,
                kind: if self.to_server() {
                    HeldKind::WsToServer
                } else {
                    HeldKind::WsToClient
                },
                held_at: now_ms(),
                method: "WS".into(),
                url: self.info.url.clone(),
                host: self.info.host.clone(),
                status: None,
                size: message.payload.len() as u64,
                ws_opcode: Some(message.opcode.name().to_string()),
            },
            raw: message.payload.clone(),
            request: None,
        };
        let Some((id, rx)) = queue.hold(held) else {
            return Some(message.payload.clone());
        };
        let mut guard = HoldGuard {
            queue,
            id,
            armed: true,
        };
        let decision = tokio::select! {
            d = rx => d.ok(),
            _ = self.done.cancelled() => None,
        };
        guard.armed = decision.is_none();
        match decision {
            Some(Decision::Forward { raw, .. }) => Some(raw),
            Some(Decision::Drop) => None,
            // Released without a decision (Intercept switched off): unchanged.
            None => Some(message.payload.clone()),
        }
    }

    fn record(&self, message: &Message, delivered: Option<&[u8]>, held: bool) {
        let limit = self.ctx.shared.limits().max_message_bytes;
        let mut flags = 0;
        if message.extension {
            flags |= ws_flags::EXTENSION;
        }
        let (payload, original) = match delivered {
            Some(d) if held && d != message.payload.as_slice() => {
                flags |= ws_flags::EDITED;
                (d.to_vec(), Some(message.payload.clone()))
            }
            Some(d) => (d.to_vec(), None),
            None => {
                flags |= ws_flags::DROPPED;
                (message.payload.clone(), None)
            }
        };
        let length = payload.len() as u64;
        let cap = |mut v: Vec<u8>, flags: &mut i64| {
            if v.len() > limit {
                v.truncate(limit);
                *flags |= ws_flags::TRUNCATED;
            }
            v
        };
        let payload = cap(payload, &mut flags);
        let original = original.map(|o| cap(o, &mut flags));
        self.ctx
            .shared
            .ws_event(WsEvent::Message(WsCapturedMessage {
                key: self.info.key,
                at: now_ms(),
                direction: self.direction,
                opcode: message.opcode.as_u8(),
                length,
                payload,
                original,
                flags,
            }));
    }

    async fn run<R, W>(&self, reader: &mut R, writer: &mut W)
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut assembler = Assembler::default();
        // Whether the data message currently being assembled is paused (its
        // fragments are buffered instead of forwarded).
        let mut holding = false;
        loop {
            let frame = tokio::select! {
                _ = self.done.cancelled() => break,
                f = read_frame(reader) => f,
            };
            let frame = match frame {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(e) => {
                    tracing::debug!(url = %self.info.url, error = %e, "WebSocket relay ended");
                    break;
                }
            };
            if !frame.opcode.is_control() && frame.opcode != Opcode::Continuation {
                // Buffer the message when Intercept may pause it or Match &
                // Replace may change it (text messages).
                let rewrite = frame.opcode == Opcode::Text
                    && self
                        .ctx
                        .shared
                        .match_replace()
                        .has_websocket(self.to_server());
                holding = frame.rsv == 0
                    && frame.payload.len() <= MAX_HELD_MESSAGE
                    && (rewrite || self.should_hold(frame.opcode));
            }
            let control = frame.opcode.is_control();
            if (control || !holding) && writer.write_all(&frame.raw).await.is_err() {
                break;
            }
            let Some(message) = assembler.push(frame) else {
                continue;
            };
            if message.opcode == Opcode::Close {
                self.close
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get_or_insert_with(|| parse_close(&message.payload));
                let bit = if self.to_server() { 1 } else { 2 };
                if self.closed_by.fetch_or(bit, Ordering::Relaxed) | bit == 3 {
                    // Both sides sent their close frame: the connection is over.
                    self.record(&message, Some(&message.payload), false);
                    self.done.cancel();
                    break;
                }
            }
            if control || !holding {
                self.record(&message, Some(&message.payload), false);
                continue;
            }
            holding = false;
            // Match & Replace first, then Intercept shows the result.
            let mut current = message.clone();
            if message.opcode == Opcode::Text
                && let Some(applied) = self
                    .ctx
                    .shared
                    .match_replace()
                    .apply_websocket(self.to_server(), &message.payload)
            {
                if let Some(why) = &applied.skipped {
                    tracing::warn!(url = %self.info.url, "WebSocket rule: {why}");
                }
                current.payload = applied.message;
            }
            let delivered = if message.payload.len() > MAX_HELD_MESSAGE {
                Some(current.payload.clone())
            } else if self.should_hold(message.opcode) {
                self.decide(&current).await
            } else {
                Some(current.payload.clone())
            };
            let bytes = match &delivered {
                Some(p) if *p == message.payload => message.frames.concat(),
                Some(p) => {
                    let mask = self.to_server().then(random_mask);
                    encode_frame(message.opcode, p, mask)
                }
                None => Vec::new(),
            };
            self.record(&message, delivered.as_deref(), true);
            if !bytes.is_empty() && writer.write_all(&bytes).await.is_err() {
                break;
            }
        }
        let _ = writer.shutdown().await;
    }
}

/// Relays a WebSocket connection until both sides have closed.
pub(crate) async fn relay(ctx: &ConnCtx, client: Client, up: Upstream, info: WsInfo) {
    ctx.shared.ws_event(WsEvent::Opened {
        key: info.key,
        started_at: now_ms(),
        source: Source::Proxy,
        scheme: info.scheme.as_str().to_string(),
        host: info.host.clone(),
        port: info.port,
        path: info.path.clone(),
        url: info.url.clone(),
    });
    let close = Mutex::new(None);
    let closed_by = AtomicU8::new(0);
    let done = CancellationToken::new();
    let pump = |direction| Pump {
        ctx,
        info: &info,
        direction,
        close: &close,
        closed_by: &closed_by,
        done: &done,
    };
    let to_server = pump(WsDirection::ToServer);
    let to_client = pump(WsDirection::ToClient);
    let Client {
        mut reader,
        mut writer,
    } = client;
    let Upstream {
        reader: mut up_reader,
        writer: mut up_writer,
        ..
    } = up;
    {
        let a = to_server.run(&mut reader, &mut up_writer);
        let b = to_client.run(&mut up_reader, &mut writer);
        tokio::pin!(a, b);
        // When one side ends, give the other a moment to finish its close.
        tokio::select! {
            _ = &mut a => { let _ = tokio::time::timeout(CLOSE_GRACE, &mut b).await; }
            _ = &mut b => { let _ = tokio::time::timeout(CLOSE_GRACE, &mut a).await; }
        }
        done.cancel();
    }
    let (code, reason) = close
        .into_inner()
        .unwrap_or_else(|p| p.into_inner())
        .unwrap_or((None, String::new()));
    ctx.shared.ws_event(WsEvent::Closed {
        key: info.key,
        at: now_ms(),
        code,
        reason,
    });
}
