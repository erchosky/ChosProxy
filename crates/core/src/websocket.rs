//! Captured WebSocket traffic handed by the proxy to storage.

use crate::Source;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which way a message travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum WsDirection {
    /// Client → server.
    ToServer,
    /// Server → client.
    ToClient,
}

impl WsDirection {
    /// Stored value.
    pub fn as_i64(self) -> i64 {
        match self {
            WsDirection::ToServer => 0,
            WsDirection::ToClient => 1,
        }
    }

    /// Parses [`Self::as_i64`].
    pub fn from_i64(v: i64) -> WsDirection {
        if v == 0 {
            WsDirection::ToServer
        } else {
            WsDirection::ToClient
        }
    }
}

/// Bit flags stored with each WebSocket message.
pub mod ws_flags {
    /// The stored payload was cut at the capture limit.
    pub const TRUNCATED: i64 = 1;
    /// The user edited the message during interception.
    pub const EDITED: i64 = 1 << 1;
    /// The user dropped the message during interception.
    pub const DROPPED: i64 = 1 << 2;
    /// An extension bit (e.g. compression) was set: the payload is not plain.
    pub const EXTENSION: i64 = 1 << 3;
}

/// One captured message (a complete data message or a control frame).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsCapturedMessage {
    /// Connection key (see [`WsEvent::Opened`]).
    pub key: i64,
    /// Unix epoch ms.
    pub at: i64,
    /// Direction.
    pub direction: WsDirection,
    /// Opcode (1 text, 2 binary, 8 close, 9 ping, 10 pong).
    pub opcode: u8,
    /// Payload length as delivered.
    pub length: u64,
    /// Payload as delivered (possibly truncated, see flags).
    pub payload: Vec<u8>,
    /// Payload before the user edited it.
    pub original: Option<Vec<u8>>,
    /// Bit set of [`ws_flags`].
    pub flags: i64,
}

/// Lifecycle of a captured WebSocket connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsEvent {
    /// The handshake completed. `key` is unique per connection and is also
    /// stored on the handshake exchange.
    Opened {
        /// Connection key.
        key: i64,
        /// Unix epoch ms.
        started_at: i64,
        /// Producing module.
        source: Source,
        /// `ws`-carrying scheme: `http` or `https`.
        scheme: String,
        /// Host.
        host: String,
        /// Port.
        port: u16,
        /// Path and query of the handshake request.
        path: String,
        /// URL of the handshake request.
        url: String,
    },
    /// A message.
    Message(WsCapturedMessage),
    /// The connection ended.
    Closed {
        /// Connection key.
        key: i64,
        /// Unix epoch ms.
        at: i64,
        /// Close code from the first close frame, if any.
        code: Option<u16>,
        /// Close reason.
        reason: String,
    },
}
