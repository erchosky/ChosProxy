//! Engine-side Intercept operations: presenting paused messages for editing
//! and turning the user's edits back into exact bytes.

use crate::AppError;
use crate::detail::message_view;
use crate::types::MessageView;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chosproxy_http::edit::{self, NotEditable};
use chosproxy_proxy::{Decision, HeldMessage, HeldSummary, InterceptQueue};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Largest decoded body produced by "unpack".
const UNPACK_LIMIT: usize = 32 * 1024 * 1024;

/// Intercept switch and queue.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InterceptState {
    /// Intercept is on.
    pub enabled: bool,
    /// Paused messages, oldest first.
    pub pending: Vec<HeldSummary>,
}

/// One paused message prepared for the editor.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HeldDetail {
    /// Listing data.
    pub summary: HeldSummary,
    /// Exact bytes, base64 (for the hex view).
    pub raw_base64: String,
    /// Editable text (line endings normalized to `\n`), when the message can
    /// round-trip through a text editor.
    pub text: Option<String>,
    /// Why the message cannot be edited as text.
    pub not_editable: Option<String>,
    /// The body is chunked or compressed and can be unpacked for editing.
    pub packed: bool,
    /// For responses: the request it answers.
    pub request: Option<MessageView>,
}

/// The user's edit of a paused message.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HeldEdit {
    /// Edited text (as returned in [`HeldDetail::text`], modified).
    pub text: String,
    /// Recompute `Content-Length` from the edited body.
    pub update_content_length: bool,
}

pub(crate) fn state(queue: &InterceptQueue) -> InterceptState {
    InterceptState {
        enabled: queue.is_enabled(),
        pending: queue.pending(),
    }
}

fn not_paused(id: u64) -> AppError {
    AppError::Invalid(format!(
        "message {id} is no longer paused (already forwarded, dropped or its connection closed)"
    ))
}

fn detail(message: HeldMessage) -> HeldDetail {
    if message.summary.kind.is_websocket() {
        let text = (message.summary.ws_opcode.as_deref() == Some("text"))
            .then(|| String::from_utf8(message.raw.clone()).ok())
            .flatten();
        return HeldDetail {
            packed: false,
            raw_base64: STANDARD.encode(&message.raw),
            request: None,
            not_editable: text
                .is_none()
                .then(|| "binary WebSocket message — it can only be forwarded or dropped".into()),
            summary: message.summary,
            text,
        };
    }
    let (text, not_editable) = match edit::to_text(&message.raw) {
        Ok(form) => (Some(form.text), None),
        Err(e) => {
            let hint = match e {
                NotEditable::Binary if edit::is_packed(&message.raw) => {
                    " — use Unpack to decode the body first"
                }
                NotEditable::Binary => " — it can only be forwarded or dropped",
                _ => "",
            };
            (None, Some(format!("{e}{hint}")))
        }
    };
    HeldDetail {
        packed: edit::is_packed(&message.raw),
        raw_base64: STANDARD.encode(&message.raw),
        request: message
            .request
            .as_deref()
            .map(|r| message_view(r, Some(r.len() as u64), false)),
        summary: message.summary,
        text,
        not_editable,
    }
}

pub(crate) fn get(queue: &InterceptQueue, id: u64) -> Result<HeldDetail, AppError> {
    queue.get(id).map(detail).ok_or_else(|| not_paused(id))
}

pub(crate) fn unpack(queue: &InterceptQueue, id: u64) -> Result<HeldDetail, AppError> {
    let message = queue.get(id).ok_or_else(|| not_paused(id))?;
    let unpacked = edit::unpack(&message.raw, UNPACK_LIMIT)
        .map_err(|e| AppError::Invalid(format!("cannot unpack: {e}")))?;
    if !queue.replace(id, unpacked) {
        return Err(not_paused(id));
    }
    get(queue, id)
}

/// Bytes to forward: the current message, or the user's text converted back
/// with the message's own line-ending conventions.
fn bytes_to_forward(message: &HeldMessage, edit: Option<HeldEdit>) -> Result<Vec<u8>, AppError> {
    let Some(edit) = edit else {
        return Ok(message.raw.clone());
    };
    if message.summary.kind.is_websocket() {
        // A WebSocket text message is its payload: no head, no line endings.
        return Ok(edit.text.into_bytes());
    }
    let form = edit::to_text(&message.raw)
        .map_err(|e| AppError::Invalid(format!("this message cannot be edited as text: {e}")))?;
    if edit.text == form.text {
        return Ok(message.raw.clone());
    }
    let raw = edit::from_text(&edit.text, form.head_eol, form.body_eol);
    Ok(if edit.update_content_length {
        edit::update_content_length(&raw, false)
    } else {
        raw
    })
}

pub(crate) fn forward(
    queue: &InterceptQueue,
    id: u64,
    edit: Option<HeldEdit>,
    intercept_response: bool,
) -> Result<(), AppError> {
    let message = queue.get(id).ok_or_else(|| not_paused(id))?;
    let raw = bytes_to_forward(&message, edit)?;
    if queue.decide(
        id,
        Decision::Forward {
            raw,
            intercept_response,
        },
    ) {
        Ok(())
    } else {
        Err(not_paused(id))
    }
}

pub(crate) fn drop_message(queue: &InterceptQueue, id: u64) -> Result<(), AppError> {
    if queue.decide(id, Decision::Drop) {
        Ok(())
    } else {
        Err(not_paused(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chosproxy_proxy::HeldKind;

    fn held(raw: &[u8]) -> HeldMessage {
        HeldMessage {
            summary: HeldSummary {
                id: 1,
                kind: HeldKind::Request,
                held_at: 0,
                method: "POST".into(),
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

    #[test]
    fn websocket_messages_are_edited_as_plain_text() {
        let mut m = held(b"{\"a\":1}\r\n");
        m.summary.kind = HeldKind::WsToServer;
        m.summary.ws_opcode = Some("text".into());
        let d = detail(m.clone());
        assert_eq!(d.text.as_deref(), Some("{\"a\":1}\r\n"));
        assert!(!d.packed && d.request.is_none());
        let edit = HeldEdit {
            text: "{\"a\":2}".into(),
            update_content_length: true,
        };
        assert_eq!(bytes_to_forward(&m, Some(edit)).unwrap(), b"{\"a\":2}");
        m.summary.ws_opcode = Some("binary".into());
        m.raw = vec![0xff, 0x00];
        assert!(detail(m).text.is_none());
    }

    #[test]
    fn unchanged_text_forwards_original_bytes() {
        let raw = b"POST / HTTP/1.1\r\nHost: a\r\nContent-Length: 3\r\n\r\na=1";
        let m = held(raw);
        let text = edit::to_text(raw).unwrap().text;
        let edit = HeldEdit {
            text,
            update_content_length: true,
        };
        assert_eq!(bytes_to_forward(&m, Some(edit)).unwrap(), raw);
        assert_eq!(bytes_to_forward(&m, None).unwrap(), raw);
    }

    #[test]
    fn edits_restore_crlf_and_fix_length() {
        let m = held(b"POST / HTTP/1.1\r\nHost: a\r\nContent-Length: 3\r\n\r\na=1");
        let edit = HeldEdit {
            text: "POST /x HTTP/1.1\nHost: a\nContent-Length: 3\n\na=12345".into(),
            update_content_length: true,
        };
        assert_eq!(
            bytes_to_forward(&m, Some(edit)).unwrap(),
            b"POST /x HTTP/1.1\r\nHost: a\r\nContent-Length: 7\r\n\r\na=12345"
        );
        let keep = HeldEdit {
            text: "POST / HTTP/1.1\nHost: a\nContent-Length: 3\n\nab".into(),
            update_content_length: false,
        };
        assert!(
            bytes_to_forward(&m, Some(keep))
                .unwrap()
                .ends_with(b"Content-Length: 3\r\n\r\nab")
        );
    }

    #[test]
    fn binary_messages_explain_how_to_edit() {
        let d = detail(held(
            b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n\x1f\x8b\xff",
        ));
        assert!(d.text.is_none());
        assert!(d.packed);
        assert!(d.not_editable.unwrap().contains("Unpack"));
        let m = held(b"HTTP/1.1 200 OK\r\n\r\n\xff\xfe");
        assert!(
            bytes_to_forward(
                &m,
                Some(HeldEdit {
                    text: "x".into(),
                    update_content_length: true
                })
            )
            .is_err()
        );
    }
}
