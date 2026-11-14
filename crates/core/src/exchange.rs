//! A captured HTTP exchange: the unit every traffic producer (proxy,
//! repeater, scanner, …) hands to storage and analysis.

use crate::Source;

/// Bit flags stored with each exchange.
pub mod flags {
    /// The stored request body was cut at the capture limit.
    pub const REQUEST_TRUNCATED: i64 = 1;
    /// The stored response body was cut at the capture limit.
    pub const RESPONSE_TRUNCATED: i64 = 1 << 1;
    /// The request carried both Content-Length and Transfer-Encoding.
    pub const CL_TE_CONFLICT: i64 = 1 << 2;
    /// The connection was upgraded (e.g. WebSocket) after this exchange.
    pub const UPGRADED: i64 = 1 << 3;
    /// The request was edited during interception before being sent.
    pub const EDITED_REQUEST: i64 = 1 << 4;
    /// The response was edited during interception before being delivered.
    pub const EDITED_RESPONSE: i64 = 1 << 5;
    /// The user dropped the request or response during interception.
    pub const DROPPED: i64 = 1 << 6;
}

/// One request/response exchange as captured, ready to be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedExchange {
    /// Start time, Unix epoch ms.
    pub started_at: i64,
    /// Total duration until the response completed.
    pub duration_ms: Option<i64>,
    /// Producing module.
    pub source: Source,
    /// Listener id (proxy traffic).
    pub listener: Option<String>,
    /// Client socket address.
    pub client_addr: Option<String>,
    /// `http` or `https`.
    pub scheme: String,
    /// Request method.
    pub method: String,
    /// Target host.
    pub host: String,
    /// Target port.
    pub port: u16,
    /// Origin-form target (path + query).
    pub path: String,
    /// Lowercase file extension of the path, if any.
    pub extension: Option<String>,
    /// Full URL.
    pub url: String,
    /// Version token of the request.
    pub http_version: String,
    /// Response status, if a response was received.
    pub status: Option<u16>,
    /// Response reason phrase.
    pub reason: Option<String>,
    /// MIME category (see `chosproxy_http::content::classify_mime`).
    pub mime: Option<String>,
    /// Raw request as sent upstream (possibly truncated, see flags).
    pub request: Vec<u8>,
    /// Raw response as delivered to the client (possibly truncated, see flags).
    pub response: Option<Vec<u8>>,
    /// The request before the user edited it (only when edited).
    pub original_request: Option<Vec<u8>>,
    /// The response before the user edited it (only when edited).
    pub original_response: Option<Vec<u8>>,
    /// Request size on the wire.
    pub request_len: u64,
    /// Response size on the wire.
    pub response_len: Option<u64>,
    /// IP address of the upstream server.
    pub server_ip: Option<String>,
    /// Whether the upstream connection used TLS.
    pub tls: bool,
    /// Error that prevented a complete exchange.
    pub error: Option<String>,
    /// Free-form notes (including automatic anomaly notes).
    pub notes: Option<String>,
    /// Bit set of [`flags`].
    pub flags: i64,
    /// Key of the WebSocket connection this handshake opened.
    pub ws_key: Option<i64>,
}
