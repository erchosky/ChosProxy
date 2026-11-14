//! WebSocket messages prepared for display and export.

use crate::AppError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chosproxy_storage::{StoredWsMessage, WsConnectionRow, WsMessageRow};
use serde::Serialize;
use ts_rs::TS;

/// Payloads longer than this are not shown as text (Base64 is always sent).
const TEXT_LIMIT: usize = 4 * 1024 * 1024;

/// One message with its payload, ready for the viewer.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WsMessageDetail {
    /// Metadata.
    pub row: WsMessageRow,
    /// Payload as delivered, Base64.
    pub payload_base64: String,
    /// Payload as UTF-8 text, when it is valid text.
    pub text: Option<String>,
    /// Payload before it was edited, Base64.
    pub original_base64: Option<String>,
    /// Original payload as text.
    pub original_text: Option<String>,
}

fn as_text(bytes: &[u8]) -> Option<String> {
    (bytes.len() <= TEXT_LIMIT)
        .then(|| std::str::from_utf8(bytes).ok().map(str::to_string))
        .flatten()
}

pub(crate) fn detail(m: StoredWsMessage) -> WsMessageDetail {
    WsMessageDetail {
        payload_base64: STANDARD.encode(&m.payload),
        text: as_text(&m.payload),
        original_base64: m.original.as_deref().map(|o| STANDARD.encode(o)),
        original_text: m.original.as_deref().and_then(as_text),
        row: m.row,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportedMessage<'a> {
    at: i64,
    direction: chosproxy_core::websocket::WsDirection,
    opcode: &'a str,
    length: u64,
    edited: bool,
    dropped: bool,
    truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_text: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Export<'a> {
    format: &'static str,
    exported_at: i64,
    connection: &'a WsConnectionRow,
    messages: Vec<ExportedMessage<'a>>,
}

/// The connection and its messages as pretty JSON (text payloads as text,
/// others as Base64).
pub(crate) fn export_json(
    connection: &WsConnectionRow,
    messages: &[StoredWsMessage],
) -> Result<Vec<u8>, AppError> {
    let export = Export {
        format: "chosproxy-websocket-v1",
        exported_at: chosproxy_core::time::now_ms(),
        connection,
        messages: messages
            .iter()
            .map(|m| {
                let text = std::str::from_utf8(&m.payload).ok().map(str::to_string);
                ExportedMessage {
                    at: m.row.at,
                    direction: m.row.direction,
                    opcode: &m.row.opcode,
                    length: m.row.length,
                    edited: m.row.edited,
                    dropped: m.row.dropped,
                    truncated: m.row.truncated,
                    base64: text.is_none().then(|| STANDARD.encode(&m.payload)),
                    text,
                    original_text: m
                        .original
                        .as_deref()
                        .map(|o| String::from_utf8_lossy(o).into_owned()),
                }
            })
            .collect(),
    };
    serde_json::to_vec_pretty(&export).map_err(|e| AppError::Internal(e.to_string()))
}
