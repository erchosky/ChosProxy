//! WebSocket connections and messages in the project database.

use crate::db::{ProjectDb, search_regex};
use crate::{StorageError, blob};
use chosproxy_core::Source;
use chosproxy_core::websocket::{WsDirection, WsEvent, ws_flags};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most connections listed at once.
pub const MAX_CONNECTIONS: u32 = 2000;
/// Most messages examined by one message search.
pub const MAX_SEARCHED_MESSAGES: usize = 100_000;
/// Characters of payload shown in listings.
const PREVIEW_CHARS: usize = 200;

/// A WebSocket connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WsConnectionRow {
    /// Connection id.
    #[ts(type = "number")]
    pub id: i64,
    /// Handshake exchange in the history, if stored.
    #[ts(type = "number | null")]
    pub exchange_id: Option<i64>,
    /// Opened, Unix epoch ms.
    #[ts(type = "number")]
    pub started_at: i64,
    /// Closed, Unix epoch ms (`null` while open or if the app stopped first).
    #[ts(type = "number | null")]
    pub ended_at: Option<i64>,
    /// Producing module.
    pub source: Source,
    /// `http` (ws://) or `https` (wss://).
    pub scheme: String,
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Handshake URL.
    pub url: String,
    /// Close code.
    pub close_code: Option<u16>,
    /// Close reason.
    pub close_reason: Option<String>,
    /// Messages captured.
    #[ts(type = "number")]
    pub messages: u64,
    /// Payload bytes captured.
    #[ts(type = "number")]
    pub bytes: u64,
    /// In the current scope.
    pub in_scope: bool,
}

/// Connection filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct WsConnectionQuery {
    /// Case-insensitive substring of the URL.
    pub search: Option<String>,
    /// Only connections in scope.
    pub in_scope_only: bool,
}

/// A message without its full payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WsMessageRow {
    /// Message id.
    #[ts(type = "number")]
    pub id: i64,
    /// Connection id.
    #[ts(type = "number")]
    pub connection_id: i64,
    /// Unix epoch ms.
    #[ts(type = "number")]
    pub at: i64,
    /// Direction.
    pub direction: WsDirection,
    /// `text`, `binary`, `close`, `ping`, `pong`, …
    pub opcode: String,
    /// Payload length as delivered.
    #[ts(type = "number")]
    pub length: u64,
    /// Start of the payload (text) or its first bytes in hex (binary).
    pub preview: String,
    /// Edited during interception.
    pub edited: bool,
    /// Dropped during interception.
    pub dropped: bool,
    /// Stored payload cut at the capture limit.
    pub truncated: bool,
    /// An extension bit was set (payload not plain).
    pub extension: bool,
}

/// Message filter and paging.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct WsMessageQuery {
    /// Connection.
    #[ts(type = "number")]
    pub connection_id: i64,
    /// Only one direction.
    pub direction: Option<WsDirection>,
    /// Hide ping, pong and close frames.
    pub hide_control: bool,
    /// Text searched in payloads (case-insensitive substring, or a regular
    /// expression when `regex` is set).
    pub search: Option<String>,
    /// `search` is a regular expression.
    pub regex: bool,
    /// Rows to skip.
    pub offset: u32,
    /// Rows to return (1..=1000).
    pub limit: u32,
}

/// A page of messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WsMessagePage {
    /// Messages matching the filter.
    #[ts(type = "number")]
    pub total: u64,
    /// Requested rows, oldest first.
    pub rows: Vec<WsMessageRow>,
    /// The search stopped at its limit.
    pub search_truncated: bool,
}

/// A message with its payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredWsMessage {
    /// Metadata.
    pub row: WsMessageRow,
    /// Payload as delivered.
    pub payload: Vec<u8>,
    /// Payload before it was edited.
    pub original: Option<Vec<u8>>,
}

fn opcode_name(op: i64) -> &'static str {
    match op {
        0 => "continuation",
        1 => "text",
        2 => "binary",
        8 => "close",
        9 => "ping",
        10 => "pong",
        _ => "reserved",
    }
}

fn preview(opcode: i64, payload: &[u8]) -> String {
    if opcode == 2 {
        payload
            .iter()
            .take(32)
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        String::from_utf8_lossy(payload)
            .chars()
            .take(PREVIEW_CHARS)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    }
}

pub(crate) fn apply_event(c: &Connection, event: &WsEvent) -> Result<(), StorageError> {
    match event {
        WsEvent::Opened {
            key,
            started_at,
            source,
            scheme,
            host,
            port,
            path,
            url,
        } => {
            c.execute(
                "INSERT OR IGNORE INTO ws_connections (conn_key, started_at, source, scheme, host, \
                 port, path, url) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    key,
                    started_at,
                    source.as_str(),
                    scheme,
                    host,
                    port,
                    path,
                    url
                ],
            )?;
        }
        WsEvent::Message(m) => {
            let Some(conn_id) = c
                .query_row(
                    "SELECT id FROM ws_connections WHERE conn_key = ?1",
                    [m.key],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
            else {
                return Ok(()); // connection not recorded (e.g. opened in another project)
            };
            let payload = blob::insert(c, &m.payload)?;
            let original = m
                .original
                .as_deref()
                .map(|o| blob::insert(c, o))
                .transpose()?;
            let length = i64::try_from(m.length).unwrap_or(i64::MAX);
            c.prepare_cached(
                "INSERT INTO ws_messages (connection_id, at, direction, opcode, length, \
                 payload_blob, original_blob, flags) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?
            .execute(params![
                conn_id,
                m.at,
                m.direction.as_i64(),
                m.opcode,
                length,
                payload,
                original,
                m.flags
            ])?;
            c.prepare_cached(
                "UPDATE ws_connections SET messages = messages + 1, bytes = bytes + ?2 WHERE id = ?1",
            )?
            .execute(params![conn_id, length])?;
        }
        WsEvent::Closed {
            key,
            at,
            code,
            reason,
        } => {
            c.execute(
                "UPDATE ws_connections SET ended_at = ?2, close_code = ?3, close_reason = ?4 \
                 WHERE conn_key = ?1",
                params![key, at, code, (!reason.is_empty()).then_some(reason)],
            )?;
        }
    }
    Ok(())
}

const CONN_COLUMNS: &str = "c.id, (SELECT e.id FROM exchanges e WHERE e.ws_key = c.conn_key LIMIT 1), \
    c.started_at, c.ended_at, c.source, c.scheme, c.host, c.port, c.url, c.close_code, \
    c.close_reason, c.messages, c.bytes, chos_in_scope(c.scheme, c.host, c.port, c.path)";

fn map_connection(r: &Row<'_>) -> rusqlite::Result<WsConnectionRow> {
    let to_u64 = |v: i64| u64::try_from(v).unwrap_or(0);
    Ok(WsConnectionRow {
        id: r.get(0)?,
        exchange_id: r.get(1)?,
        started_at: r.get(2)?,
        ended_at: r.get(3)?,
        source: Source::parse(&r.get::<_, String>(4)?).unwrap_or(Source::Proxy),
        scheme: r.get(5)?,
        host: r.get(6)?,
        port: r.get(7)?,
        url: r.get(8)?,
        close_code: r.get(9)?,
        close_reason: r.get(10)?,
        messages: to_u64(r.get(11)?),
        bytes: to_u64(r.get(12)?),
        in_scope: r.get(13)?,
    })
}

const MSG_COLUMNS: &str =
    "id, connection_id, at, direction, opcode, length, flags, payload_blob, original_blob";

/// A message row plus its blob ids.
fn map_message(r: &Row<'_>) -> rusqlite::Result<(WsMessageRow, i64, i64, Option<i64>)> {
    let opcode: i64 = r.get(4)?;
    let flags: i64 = r.get(6)?;
    Ok((
        WsMessageRow {
            id: r.get(0)?,
            connection_id: r.get(1)?,
            at: r.get(2)?,
            direction: WsDirection::from_i64(r.get(3)?),
            opcode: opcode_name(opcode).to_string(),
            length: u64::try_from(r.get::<_, i64>(5)?).unwrap_or(0),
            preview: String::new(),
            edited: flags & ws_flags::EDITED != 0,
            dropped: flags & ws_flags::DROPPED != 0,
            truncated: flags & ws_flags::TRUNCATED != 0,
            extension: flags & ws_flags::EXTENSION != 0,
        },
        opcode,
        r.get(7)?,
        r.get(8)?,
    ))
}

fn load_blob(c: &Connection, id: i64) -> Result<Vec<u8>, StorageError> {
    blob::load(c, id)?.ok_or(StorageError::CorruptBlob(id))
}

impl ProjectDb {
    /// WebSocket connections, newest first.
    pub fn ws_connections(
        &self,
        q: &WsConnectionQuery,
    ) -> Result<Vec<WsConnectionRow>, StorageError> {
        let mut clauses = Vec::new();
        let mut args: Vec<Value> = Vec::new();
        if let Some(s) = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            clauses.push("instr(lower(c.url), lower(?)) > 0");
            args.push(Value::Text(s.to_string()));
        }
        if q.in_scope_only {
            clauses.push("chos_in_scope(c.scheme, c.host, c.port, c.path)");
        }
        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {CONN_COLUMNS} FROM ws_connections c {where_sql} ORDER BY c.id DESC LIMIT {MAX_CONNECTIONS}"
            ))?;
            let rows = stmt
                .query_map(params_from_iter(args.iter()), map_connection)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// One connection.
    pub fn ws_connection(&self, id: i64) -> Result<Option<WsConnectionRow>, StorageError> {
        self.with_reader(|c| {
            Ok(c.query_row(
                &format!("SELECT {CONN_COLUMNS} FROM ws_connections c WHERE c.id = ?1"),
                [id],
                map_connection,
            )
            .optional()?)
        })
    }

    /// A page of a connection's messages, oldest first.
    pub fn ws_messages(&self, q: &WsMessageQuery) -> Result<WsMessagePage, StorageError> {
        if q.limit == 0 || q.limit > 1000 {
            return Err(StorageError::InvalidQuery("limit must be 1..=1000".into()));
        }
        let search = q
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                if q.regex {
                    search_regex(s).map(Some)
                } else {
                    Ok(None)
                }
                .map(|re| (s.to_lowercase(), re))
            })
            .transpose()?;
        let mut sql = format!("SELECT {MSG_COLUMNS} FROM ws_messages WHERE connection_id = ?1");
        if let Some(d) = q.direction {
            sql.push_str(&format!(" AND direction = {}", d.as_i64()));
        }
        if q.hide_control {
            sql.push_str(" AND opcode < 8");
        }
        sql.push_str(" ORDER BY id");
        self.with_reader(|c| {
            let mut stmt = c.prepare(&sql)?;
            let mut rows = stmt.query([q.connection_id])?;
            let mut matched = Vec::new();
            let mut total: u64 = 0;
            let mut examined = 0usize;
            let mut search_truncated = false;
            let start = u64::from(q.offset);
            let end = start + u64::from(q.limit);
            while let Some(r) = rows.next()? {
                let (mut row, opcode, payload_blob, _) = map_message(r)?;
                let in_page = |total: u64| total >= start && total < end;
                if let Some((needle, re)) = &search {
                    examined += 1;
                    if examined > MAX_SEARCHED_MESSAGES {
                        search_truncated = true;
                        break;
                    }
                    let payload = load_blob(c, payload_blob)?;
                    let text = String::from_utf8_lossy(&payload);
                    let hit = match re {
                        Some(re) => re.is_match(&text),
                        None => text.to_lowercase().contains(needle.as_str()),
                    };
                    if !hit {
                        continue;
                    }
                    if in_page(total) {
                        row.preview = preview(opcode, &payload);
                        matched.push(row);
                    }
                } else if in_page(total) {
                    row.preview = preview(opcode, &load_blob(c, payload_blob)?);
                    matched.push(row);
                }
                total += 1;
            }
            Ok(WsMessagePage {
                total,
                rows: matched,
                search_truncated,
            })
        })
    }

    /// One message with its payload.
    pub fn ws_message(&self, id: i64) -> Result<Option<StoredWsMessage>, StorageError> {
        self.with_reader(|c| {
            let found = c
                .query_row(
                    &format!("SELECT {MSG_COLUMNS} FROM ws_messages WHERE id = ?1"),
                    [id],
                    map_message,
                )
                .optional()?;
            let Some((mut row, opcode, payload_blob, original_blob)) = found else {
                return Ok(None);
            };
            let payload = load_blob(c, payload_blob)?;
            row.preview = preview(opcode, &payload);
            let original = original_blob.map(|b| load_blob(c, b)).transpose()?;
            Ok(Some(StoredWsMessage {
                row,
                payload,
                original,
            }))
        })
    }

    /// Data messages (not ping, pong or close) newest first, before
    /// `before_id`, each with its connection's URL and payload — the global
    /// search reads them page by page.
    pub fn ws_messages_for_search(
        &self,
        before_id: Option<i64>,
        limit: usize,
        in_scope_only: bool,
    ) -> Result<Vec<(WsMessageRow, String, Vec<u8>)>, StorageError> {
        let columns = MSG_COLUMNS
            .split(", ")
            .map(|c| format!("m.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!(
            "SELECT {columns}, c.url FROM ws_messages m JOIN ws_connections c \
             ON c.id = m.connection_id WHERE m.opcode < 8 AND m.id < ?1"
        );
        if in_scope_only {
            sql.push_str(" AND chos_in_scope(c.scheme, c.host, c.port, c.path)");
        }
        sql.push_str(" ORDER BY m.id DESC LIMIT ?2");
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.with_reader(|c| {
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map(params![before_id.unwrap_or(i64::MAX), limit], |r| {
                    Ok((map_message(r)?, r.get::<_, String>(9)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|((mut row, opcode, payload_blob, _), url)| {
                    let payload = load_blob(c, payload_blob)?;
                    row.preview = preview(opcode, &payload);
                    Ok((row, url, payload))
                })
                .collect()
        })
    }

    /// Every message of a connection with its payload, oldest first (export).
    pub fn ws_all_messages(
        &self,
        connection_id: i64,
    ) -> Result<Vec<StoredWsMessage>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {MSG_COLUMNS} FROM ws_messages WHERE connection_id = ?1 ORDER BY id"
            ))?;
            let rows = stmt
                .query_map([connection_id], map_message)?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|(mut row, opcode, payload_blob, original_blob)| {
                    let payload = load_blob(c, payload_blob)?;
                    row.preview = preview(opcode, &payload);
                    Ok(StoredWsMessage {
                        row,
                        payload,
                        original: original_blob.map(|b| load_blob(c, b)).transpose()?,
                    })
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpenOptions;
    use chosproxy_core::websocket::WsCapturedMessage;

    fn opened(key: i64) -> WsEvent {
        WsEvent::Opened {
            key,
            started_at: 1,
            source: Source::Proxy,
            scheme: "http".into(),
            host: "lab.test".into(),
            port: 80,
            path: "/ws".into(),
            url: "http://lab.test/ws".into(),
        }
    }

    fn message(
        key: i64,
        direction: WsDirection,
        opcode: u8,
        payload: &[u8],
        flags: i64,
    ) -> WsEvent {
        WsEvent::Message(WsCapturedMessage {
            key,
            at: 2,
            direction,
            opcode,
            length: payload.len() as u64,
            payload: payload.to_vec(),
            original: (flags & ws_flags::EDITED != 0).then(|| b"before".to_vec()),
            flags,
        })
    }

    #[test]
    fn stores_lists_searches_and_pages() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        db.submit_ws(opened(7)).unwrap();
        db.submit_ws(message(7, WsDirection::ToClient, 1, b"welcome", 0))
            .unwrap();
        db.submit_ws(message(
            7,
            WsDirection::ToServer,
            1,
            b"{\"q\":\"Secret\"}",
            ws_flags::EDITED,
        ))
        .unwrap();
        db.submit_ws(message(7, WsDirection::ToServer, 9, b"", 0))
            .unwrap();
        db.submit_ws(message(7, WsDirection::ToClient, 2, &[0xde, 0xad], 0))
            .unwrap();
        db.submit_ws(message(99, WsDirection::ToClient, 1, b"orphan", 0))
            .unwrap();
        db.submit_ws(WsEvent::Closed {
            key: 7,
            at: 3,
            code: Some(1000),
            reason: "bye".into(),
        })
        .unwrap();
        db.flush().unwrap();

        let conns = db.ws_connections(&WsConnectionQuery::default()).unwrap();
        assert_eq!(conns.len(), 1);
        let conn = &conns[0];
        assert_eq!((conn.messages, conn.close_code), (4, Some(1000)));
        assert_eq!(conn.close_reason.as_deref(), Some("bye"));
        assert!(!conn.in_scope);
        assert!(
            db.ws_connections(&WsConnectionQuery {
                search: Some("nomatch".into()),
                ..Default::default()
            })
            .unwrap()
            .is_empty()
        );

        let q = |patch: WsMessageQuery| WsMessageQuery {
            connection_id: conn.id,
            limit: 100,
            ..patch
        };
        let all = db.ws_messages(&q(WsMessageQuery::default())).unwrap();
        assert_eq!(all.total, 4);
        assert_eq!(all.rows[0].preview, "welcome");
        assert_eq!(all.rows[3].preview, "de ad");
        assert_eq!(all.rows[3].opcode, "binary");
        let no_control = db
            .ws_messages(&q(WsMessageQuery {
                hide_control: true,
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(no_control.total, 3);
        let to_server = db
            .ws_messages(&q(WsMessageQuery {
                direction: Some(WsDirection::ToServer),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(to_server.total, 2);
        let search = db
            .ws_messages(&q(WsMessageQuery {
                search: Some("secret".into()),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(search.total, 1);
        assert!(search.rows[0].edited);
        let regex = db
            .ws_messages(&q(WsMessageQuery {
                search: Some(r#""q":"\w+""#.into()),
                regex: true,
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(regex.total, 1);
        assert!(
            db.ws_messages(&q(WsMessageQuery {
                search: Some("(".into()),
                regex: true,
                ..Default::default()
            }))
            .is_err()
        );
        let paged = db
            .ws_messages(&WsMessageQuery {
                connection_id: conn.id,
                offset: 1,
                limit: 2,
                ..Default::default()
            })
            .unwrap();
        assert_eq!((paged.total, paged.rows.len()), (4, 2));

        let edited = db.ws_message(search.rows[0].id).unwrap().unwrap();
        assert_eq!(edited.original.as_deref(), Some(&b"before"[..]));
        assert_eq!(db.ws_all_messages(conn.id).unwrap().len(), 4);
    }
}
