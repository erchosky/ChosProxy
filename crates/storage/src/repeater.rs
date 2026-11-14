//! Repeater tabs and their send history.
//!
//! A tab stores the request as the user edits it (text with `\n` line
//! endings plus the line-ending conventions to restore when sending) and the
//! target it is sent to. Each send is an ordinary exchange (source
//! "repeater"); `repeater_sends` links it to its tab with the timing.

use crate::StorageError;
use crate::db::ProjectDb;
use crate::model::{ExchangeId, Highlight};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Row id of a Repeater tab.
pub type RepeaterTabId = i64;
/// Row id of a Repeater tab group.
pub type RepeaterGroupId = i64;

/// Most groups in one project.
pub const MAX_REPEATER_GROUPS: usize = 50;

/// Most tabs in one project.
pub const MAX_REPEATER_TABS: usize = 200;
/// Largest request text a tab can hold.
pub const MAX_REPEATER_REQUEST_BYTES: usize = 8 * 1024 * 1024;
/// Longest tab name, in characters.
const MAX_NAME_CHARS: usize = 100;
/// Sends listed per tab (newest first).
const HISTORY_LIMIT: i64 = 1000;

/// A Repeater tab: the request being edited and where it is sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeaterTab {
    /// Tab id.
    #[ts(type = "number")]
    pub id: RepeaterTabId,
    /// Order among the tabs (ascending).
    #[ts(type = "number")]
    pub position: i64,
    /// Name shown on the tab.
    pub name: String,
    /// `http` or `https`.
    pub scheme: String,
    /// Host connected to.
    pub host: String,
    /// Port connected to.
    pub port: u16,
    /// Request text (`\n` line endings).
    pub request: String,
    /// The head is sent with CRLF line endings (otherwise LF).
    pub head_crlf: bool,
    /// The body is sent with CRLF line endings (otherwise LF).
    pub body_crlf: bool,
    /// `Content-Length` is recomputed from the body before sending.
    pub update_content_length: bool,
    /// Keep the connection open between sends (HTTP/1.1 keep-alive).
    pub reuse_connection: bool,
    /// Add matching cookies from the project cookie jar before sending.
    pub use_cookie_jar: bool,
    /// Group the tab belongs to.
    #[ts(type = "number | null")]
    pub group_id: Option<RepeaterGroupId>,
    /// The tab's own highlight colour (M31b), if it has one.
    pub color: Option<Highlight>,
    /// Created, Unix epoch ms.
    #[ts(type = "number")]
    pub created_at: i64,
    /// Last changed, Unix epoch ms.
    #[ts(type = "number")]
    pub updated_at: i64,
    /// The tester's note on this tab.
    pub note: String,
    /// When responses are redirects: `never`, `onSite`, `inScope` or
    /// `always` follow them automatically.
    pub redirect_policy: String,
}

/// Redirect policies a tab can have.
pub const REDIRECT_POLICIES: &[&str] = &["never", "onSite", "inScope", "always"];

/// A tab to create.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewRepeaterTab {
    /// Name; empty for the next free number.
    pub name: String,
    /// Target.
    pub target: RepeaterTarget,
    /// Request text (`\n` line endings).
    pub request: String,
    /// Head line endings to restore.
    pub head_crlf: bool,
    /// Body line endings to restore.
    pub body_crlf: bool,
}

/// Where a tab sends its request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeaterTarget {
    /// `http` or `https`.
    pub scheme: String,
    /// Host name or IP address.
    pub host: String,
    /// TCP port.
    pub port: u16,
}

/// Editable fields of a tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeaterTabUpdate {
    /// Name.
    pub name: String,
    /// Target.
    pub target: RepeaterTarget,
    /// Request text (`\n` line endings).
    pub request: String,
    /// Recompute `Content-Length` before sending.
    pub update_content_length: bool,
    /// Keep the connection open between sends.
    pub reuse_connection: bool,
    /// Add cookies from the cookie jar before sending.
    pub use_cookie_jar: bool,
}

/// How a group sends its tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum GroupSendMode {
    /// One after another, each on its own connection (as its tab is set).
    Sequence,
    /// One after another on a single connection (all tabs need one target).
    Single,
    /// All at once, each on its own connection, synchronised on the last byte.
    Parallel,
}

impl GroupSendMode {
    fn as_str(self) -> &'static str {
        match self {
            GroupSendMode::Sequence => "sequence",
            GroupSendMode::Single => "single",
            GroupSendMode::Parallel => "parallel",
        }
    }

    fn parse(s: &str) -> GroupSendMode {
        match s {
            "single" => GroupSendMode::Single,
            "parallel" => GroupSendMode::Parallel,
            _ => GroupSendMode::Sequence,
        }
    }
}

/// A group of Repeater tabs, sent together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeaterGroup {
    /// Group id.
    #[ts(type = "number")]
    pub id: RepeaterGroupId,
    /// Order among the groups.
    #[ts(type = "number")]
    pub position: i64,
    /// Name.
    pub name: String,
    /// Colour shown on its tabs.
    pub color: Highlight,
    /// How "Send group" sends its tabs.
    pub send_mode: GroupSendMode,
    /// Collapsed in the tab strip: only the chip (and the selected tab) shows.
    pub collapsed: bool,
    /// Created, Unix epoch ms.
    #[ts(type = "number")]
    pub created_at: i64,
}

/// Editable fields of a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeaterGroupUpdate {
    /// Name.
    pub name: String,
    /// Colour.
    pub color: Highlight,
    /// Send mode.
    pub send_mode: GroupSendMode,
    /// Collapsed in the tab strip.
    #[serde(default)]
    pub collapsed: bool,
}

/// What else is recorded with a send.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SendDetails {
    /// Timing.
    pub timing: SendTimingRow,
    /// Sent over a connection that carried an earlier request.
    pub reused: bool,
    /// Names of the cookies added from the cookie jar.
    pub jar_cookies: Vec<String>,
}

/// Timing of one send, in milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SendTimingRow {
    /// DNS resolution.
    #[ts(type = "number")]
    pub dns_ms: u64,
    /// TCP connect.
    #[ts(type = "number")]
    pub connect_ms: u64,
    /// TLS handshake.
    #[ts(type = "number")]
    pub tls_ms: u64,
    /// Waiting for the response head.
    #[ts(type = "number")]
    pub wait_ms: u64,
    /// Receiving the body.
    #[ts(type = "number")]
    pub download_ms: u64,
    /// Total.
    #[ts(type = "number")]
    pub total_ms: u64,
}

/// One entry of a tab's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeaterSendRow {
    /// Send id.
    #[ts(type = "number")]
    pub id: i64,
    /// Tab it belongs to.
    #[ts(type = "number")]
    pub tab_id: RepeaterTabId,
    /// The stored exchange (request and response).
    #[ts(type = "number")]
    pub exchange_id: ExchangeId,
    /// Sent, Unix epoch ms.
    #[ts(type = "number")]
    pub sent_at: i64,
    /// Request method.
    pub method: String,
    /// Request URL.
    pub url: String,
    /// Response status, if a response arrived.
    pub status: Option<u16>,
    /// Response size on the wire.
    #[ts(type = "number | null")]
    pub response_len: Option<u64>,
    /// Why no (complete) response arrived.
    pub error: Option<String>,
    /// Timing.
    pub timing: SendTimingRow,
    /// Sent over a connection that carried an earlier request.
    pub reused: bool,
    /// Names of the cookies added from the cookie jar.
    pub jar_cookies: Vec<String>,
}

const TAB_COLUMNS: &str = "id, position, name, scheme, host, port, request, head_crlf, body_crlf, \
    update_content_length, created_at, updated_at, reuse_connection, use_cookie_jar, group_id, note, \
    redirect_policy, color";

fn map_tab(r: &Row<'_>) -> rusqlite::Result<RepeaterTab> {
    Ok(RepeaterTab {
        id: r.get(0)?,
        position: r.get(1)?,
        name: r.get(2)?,
        scheme: r.get(3)?,
        host: r.get(4)?,
        port: r.get(5)?,
        request: r.get(6)?,
        head_crlf: r.get(7)?,
        body_crlf: r.get(8)?,
        update_content_length: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        reuse_connection: r.get(12)?,
        use_cookie_jar: r.get(13)?,
        group_id: r.get(14)?,
        note: r.get(15)?,
        redirect_policy: r.get(16)?,
        color: Highlight::parse(&r.get::<_, String>(17)?),
    })
}

const GROUP_COLUMNS: &str = "id, position, name, color, send_mode, created_at, collapsed";

fn map_group(r: &Row<'_>) -> rusqlite::Result<RepeaterGroup> {
    Ok(RepeaterGroup {
        id: r.get(0)?,
        position: r.get(1)?,
        name: r.get(2)?,
        color: Highlight::parse(&r.get::<_, String>(3)?).unwrap_or(Highlight::Blue),
        send_mode: GroupSendMode::parse(&r.get::<_, String>(4)?),
        created_at: r.get(5)?,
        collapsed: r.get(6)?,
    })
}

const SEND_COLUMNS: &str = "s.id, s.tab_id, s.exchange_id, s.sent_at, e.method, e.url, e.status, \
    e.response_len, e.error, s.dns_ms, s.connect_ms, s.tls_ms, s.wait_ms, s.download_ms, s.total_ms, \
    s.reused, s.jar_cookies";

fn map_send(r: &Row<'_>) -> rusqlite::Result<RepeaterSendRow> {
    let ms =
        |i: usize| -> rusqlite::Result<u64> { Ok(u64::try_from(r.get::<_, i64>(i)?).unwrap_or(0)) };
    Ok(RepeaterSendRow {
        id: r.get(0)?,
        tab_id: r.get(1)?,
        exchange_id: r.get(2)?,
        sent_at: r.get(3)?,
        method: r.get(4)?,
        url: r.get(5)?,
        status: r.get(6)?,
        response_len: r
            .get::<_, Option<i64>>(7)?
            .map(|n| u64::try_from(n).unwrap_or(0)),
        error: r.get(8)?,
        timing: SendTimingRow {
            dns_ms: ms(9)?,
            connect_ms: ms(10)?,
            tls_ms: ms(11)?,
            wait_ms: ms(12)?,
            download_ms: ms(13)?,
            total_ms: ms(14)?,
        },
        reused: r.get(15)?,
        jar_cookies: r
            .get::<_, String>(16)?
            .split_whitespace()
            .map(str::to_string)
            .collect(),
    })
}

fn invalid(message: impl Into<String>) -> StorageError {
    StorageError::InvalidQuery(message.into())
}

/// Checks a target and returns it normalized (lower-case scheme and host).
pub fn validate_target(t: &RepeaterTarget) -> Result<RepeaterTarget, StorageError> {
    let scheme = t.scheme.trim().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(invalid("the protocol must be http or https"));
    }
    let host = t.host.trim().trim_matches(['[', ']']).to_ascii_lowercase();
    if host.is_empty()
        || host.len() > 253
        || host
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '/' | '?' | '#' | '@'))
    {
        return Err(invalid(format!("invalid host {:?}", t.host)));
    }
    if t.port == 0 {
        return Err(invalid("the port must be 1–65535"));
    }
    Ok(RepeaterTarget {
        scheme,
        host,
        port: t.port,
    })
}

fn validate_name(name: &str) -> Result<String, StorageError> {
    let name = name.trim().to_string();
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(invalid(format!(
            "tab names are limited to {MAX_NAME_CHARS} characters"
        )));
    }
    Ok(name)
}

fn validate_request(request: &str) -> Result<(), StorageError> {
    if request.len() > MAX_REPEATER_REQUEST_BYTES {
        return Err(invalid(format!(
            "the request is larger than {} MiB",
            MAX_REPEATER_REQUEST_BYTES / 1024 / 1024
        )));
    }
    Ok(())
}

/// The path of a request (origin form) for scope checks: the request-line
/// target, reduced to its path for absolute-form targets; `/` if none.
pub fn request_path(request: &str) -> String {
    let line = request.lines().next().unwrap_or_default();
    let target = line.split_whitespace().nth(1).unwrap_or("/");
    let path = if !target.starts_with('/')
        && let Some(i) = target.find("://")
    {
        target[i + 3..]
            .find('/')
            .map_or("/", |j| &target[i + 3 + j..])
    } else {
        target
    };
    if path.starts_with('/') {
        path.chars().take(8192).collect()
    } else {
        "/".into()
    }
}

impl ProjectDb {
    /// All tabs, in order.
    pub fn repeater_tabs(&self) -> Result<Vec<RepeaterTab>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {TAB_COLUMNS} FROM repeater_tabs ORDER BY position, id"
            ))?;
            let tabs = stmt
                .query_map([], map_tab)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(tabs)
        })
    }

    /// One tab.
    pub fn repeater_tab(&self, id: RepeaterTabId) -> Result<Option<RepeaterTab>, StorageError> {
        self.with_reader(|c| {
            Ok(c.query_row(
                &format!("SELECT {TAB_COLUMNS} FROM repeater_tabs WHERE id = ?1"),
                [id],
                map_tab,
            )
            .optional()?)
        })
    }

    /// Creates a tab after the last one.
    pub fn repeater_create(&self, tab: NewRepeaterTab) -> Result<RepeaterTab, StorageError> {
        let target = validate_target(&tab.target)?;
        let name = validate_name(&tab.name)?;
        validate_request(&tab.request)?;
        let path = request_path(&tab.request);
        let id = self.write(move |c| {
            let count: i64 = c.query_row("SELECT COUNT(*) FROM repeater_tabs", [], |r| r.get(0))?;
            if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_REPEATER_TABS {
                return Err(invalid(format!(
                    "at most {MAX_REPEATER_TABS} Repeater tabs; close some first"
                )));
            }
            let position: i64 = c.query_row(
                "SELECT COALESCE(MAX(position), 0) + 1 FROM repeater_tabs",
                [],
                |r| r.get(0),
            )?;
            let now = chosproxy_core::time::now_ms();
            c.execute(
                "INSERT INTO repeater_tabs (position, created_at, updated_at, name, scheme, host, \
                 port, path, request, head_crlf, body_crlf) \
                 VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    position,
                    now,
                    name,
                    target.scheme,
                    target.host,
                    target.port,
                    path,
                    tab.request,
                    tab.head_crlf,
                    tab.body_crlf
                ],
            )?;
            let id = c.last_insert_rowid();
            if name.is_empty() {
                c.execute(
                    "UPDATE repeater_tabs SET name = ?2 WHERE id = ?1",
                    params![id, position.to_string()],
                )?;
            }
            Ok(id)
        })?;
        self.repeater_tab(id)?.ok_or(StorageError::NotFound(id))
    }

    /// Saves the editable fields of a tab.
    pub fn repeater_update(
        &self,
        id: RepeaterTabId,
        update: RepeaterTabUpdate,
    ) -> Result<RepeaterTab, StorageError> {
        let target = validate_target(&update.target)?;
        let name = validate_name(&update.name)?;
        if name.is_empty() {
            return Err(invalid("the tab needs a name"));
        }
        validate_request(&update.request)?;
        let path = request_path(&update.request);
        self.write(move |c| {
            let n = c.execute(
                "UPDATE repeater_tabs SET name = ?2, scheme = ?3, host = ?4, port = ?5, path = ?6, \
                 request = ?7, update_content_length = ?8, updated_at = ?9, \
                 reuse_connection = ?10, use_cookie_jar = ?11 WHERE id = ?1",
                params![
                    id,
                    name,
                    target.scheme,
                    target.host,
                    target.port,
                    path,
                    update.request,
                    update.update_content_length,
                    chosproxy_core::time::now_ms(),
                    update.reuse_connection,
                    update.use_cookie_jar
                ],
            )?;
            if n == 0 {
                return Err(StorageError::NotFound(id));
            }
            Ok(())
        })?;
        self.repeater_tab(id)?.ok_or(StorageError::NotFound(id))
    }

    /// Copies a tab (without its history) right after it.
    pub fn repeater_duplicate(&self, id: RepeaterTabId) -> Result<RepeaterTab, StorageError> {
        let tab = self.repeater_tab(id)?.ok_or(StorageError::NotFound(id))?;
        let name: String = format!("{} (copy)", tab.name)
            .chars()
            .take(MAX_NAME_CHARS)
            .collect();
        let copy = self.repeater_create(NewRepeaterTab {
            name,
            target: RepeaterTarget {
                scheme: tab.scheme,
                host: tab.host,
                port: tab.port,
            },
            request: tab.request,
            head_crlf: tab.head_crlf,
            body_crlf: tab.body_crlf,
        })?;
        let (copy_id, after) = (copy.id, tab.position);
        self.write(move |c| {
            // Make room right after the original.
            c.execute(
                "UPDATE repeater_tabs SET position = position + 1 WHERE position > ?1 AND id != ?2",
                params![after, copy_id],
            )?;
            c.execute(
                "UPDATE repeater_tabs SET position = ?2, \
                 (update_content_length, reuse_connection, use_cookie_jar, group_id) = \
                 (SELECT update_content_length, reuse_connection, use_cookie_jar, group_id \
                  FROM repeater_tabs WHERE id = ?3) WHERE id = ?1",
                params![copy_id, after + 1, id],
            )?;
            Ok(())
        })?;
        self.repeater_tab(copy_id)?
            .ok_or(StorageError::NotFound(copy_id))
    }

    /// Moves a tab to `position` (0-based index among the tabs).
    pub fn repeater_move(&self, id: RepeaterTabId, position: usize) -> Result<(), StorageError> {
        self.write(move |c| {
            let mut ids: Vec<i64> = c
                .prepare("SELECT id FROM repeater_tabs ORDER BY position, id")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            let from = ids
                .iter()
                .position(|t| *t == id)
                .ok_or(StorageError::NotFound(id))?;
            let moved = ids.remove(from);
            ids.insert(position.min(ids.len()), moved);
            for (i, tab) in ids.iter().enumerate() {
                c.execute(
                    "UPDATE repeater_tabs SET position = ?2 WHERE id = ?1",
                    params![tab, i64::try_from(i).unwrap_or(i64::MAX)],
                )?;
            }
            Ok(())
        })
    }

    /// Closes a tab. Its sends stay in the Logger.
    pub fn repeater_delete(&self, id: RepeaterTabId) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute("DELETE FROM repeater_sends WHERE tab_id = ?1", [id])?;
            if c.execute("DELETE FROM repeater_tabs WHERE id = ?1", [id])? == 0 {
                return Err(StorageError::NotFound(id));
            }
            Ok(())
        })
    }

    /// Links a stored exchange to a tab's history.
    pub fn repeater_record_send(
        &self,
        tab_id: RepeaterTabId,
        exchange_id: ExchangeId,
        details: SendDetails,
    ) -> Result<RepeaterSendRow, StorageError> {
        let timing = details.timing;
        let jar_cookies = details.jar_cookies.join(" ");
        let id = self.write(move |c| {
            let exists: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM repeater_tabs WHERE id = ?1)",
                [tab_id],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(StorageError::NotFound(tab_id));
            }
            let ms = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
            c.execute(
                "INSERT INTO repeater_sends (tab_id, exchange_id, sent_at, dns_ms, connect_ms, \
                 tls_ms, wait_ms, download_ms, total_ms, reused, jar_cookies) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    tab_id,
                    exchange_id,
                    chosproxy_core::time::now_ms(),
                    ms(timing.dns_ms),
                    ms(timing.connect_ms),
                    ms(timing.tls_ms),
                    ms(timing.wait_ms),
                    ms(timing.download_ms),
                    ms(timing.total_ms),
                    details.reused,
                    jar_cookies
                ],
            )?;
            Ok(c.last_insert_rowid())
        })?;
        self.with_reader(|c| {
            Ok(c.query_row(
                &format!(
                    "SELECT {SEND_COLUMNS} FROM repeater_sends s \
                     JOIN exchanges e ON e.id = s.exchange_id WHERE s.id = ?1"
                ),
                [id],
                map_send,
            )?)
        })
    }

    /// A tab's sends, newest first. Sends whose exchange no longer exists
    /// (e.g. removed by an in-scope export) are skipped.
    pub fn repeater_history(
        &self,
        tab_id: RepeaterTabId,
    ) -> Result<Vec<RepeaterSendRow>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {SEND_COLUMNS} FROM repeater_sends s \
                 JOIN exchanges e ON e.id = s.exchange_id \
                 WHERE s.tab_id = ?1 ORDER BY s.id DESC LIMIT ?2"
            ))?;
            let rows = stmt
                .query_map(params![tab_id, HISTORY_LIMIT], map_send)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }
}

fn validate_group_name(name: &str) -> Result<String, StorageError> {
    let name = validate_name(name)?;
    if name.is_empty() {
        return Err(invalid("the group needs a name"));
    }
    Ok(name)
}

/// Moves `ids` (in this order) so they sit together, starting where the
/// first of them was; other tabs keep their relative order.
fn gather(c: &rusqlite::Connection, ids: &[RepeaterTabId]) -> Result<(), StorageError> {
    let all: Vec<i64> = c
        .prepare("SELECT id FROM repeater_tabs ORDER BY position, id")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let Some(start) = all.iter().position(|t| ids.contains(t)) else {
        return Ok(());
    };
    let mut rest: Vec<i64> = all.into_iter().filter(|t| !ids.contains(t)).collect();
    let at = start.min(rest.len());
    rest.splice(at..at, ids.iter().copied());
    for (i, tab) in rest.iter().enumerate() {
        c.execute(
            "UPDATE repeater_tabs SET position = ?2 WHERE id = ?1",
            params![tab, i64::try_from(i).unwrap_or(i64::MAX)],
        )?;
    }
    Ok(())
}

fn group_members(
    c: &rusqlite::Connection,
    group: RepeaterGroupId,
) -> Result<Vec<RepeaterTabId>, StorageError> {
    Ok(
        c.prepare("SELECT id FROM repeater_tabs WHERE group_id = ?1 ORDER BY position, id")?
            .query_map([group], |r| r.get(0))?
            .collect::<Result<_, _>>()?,
    )
}

impl ProjectDb {
    /// All groups, in order.
    pub fn repeater_groups(&self) -> Result<Vec<RepeaterGroup>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {GROUP_COLUMNS} FROM repeater_groups ORDER BY position, id"
            ))?;
            let groups = stmt
                .query_map([], map_group)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(groups)
        })
    }

    /// One group.
    pub fn repeater_group(
        &self,
        id: RepeaterGroupId,
    ) -> Result<Option<RepeaterGroup>, StorageError> {
        self.with_reader(|c| {
            Ok(c.query_row(
                &format!("SELECT {GROUP_COLUMNS} FROM repeater_groups WHERE id = ?1"),
                [id],
                map_group,
            )
            .optional()?)
        })
    }

    /// The tabs of a group, in order.
    pub fn repeater_group_tabs(
        &self,
        id: RepeaterGroupId,
    ) -> Result<Vec<RepeaterTab>, StorageError> {
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {TAB_COLUMNS} FROM repeater_tabs WHERE group_id = ?1 ORDER BY position, id"
            ))?;
            let tabs = stmt
                .query_map([id], map_tab)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(tabs)
        })
    }

    /// Creates a group holding `tabs` (moved next to each other, taken out
    /// of any other group).
    pub fn repeater_group_create(
        &self,
        name: &str,
        color: Highlight,
        tabs: Vec<RepeaterTabId>,
    ) -> Result<RepeaterGroup, StorageError> {
        let name = validate_group_name(name)?;
        let id = self.write(move |c| {
            let count: i64 =
                c.query_row("SELECT COUNT(*) FROM repeater_groups", [], |r| r.get(0))?;
            if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_REPEATER_GROUPS {
                return Err(invalid(format!(
                    "at most {MAX_REPEATER_GROUPS} Repeater groups"
                )));
            }
            let position: i64 = c.query_row(
                "SELECT COALESCE(MAX(position), 0) + 1 FROM repeater_groups",
                [],
                |r| r.get(0),
            )?;
            c.execute(
                "INSERT INTO repeater_groups (position, created_at, name, color) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    position,
                    chosproxy_core::time::now_ms(),
                    name,
                    color.as_str()
                ],
            )?;
            let id = c.last_insert_rowid();
            for tab in &tabs {
                if c.execute(
                    "UPDATE repeater_tabs SET group_id = ?2 WHERE id = ?1",
                    params![tab, id],
                )? == 0
                {
                    return Err(StorageError::NotFound(*tab));
                }
            }
            gather(c, &group_members(c, id)?)?;
            Ok(id)
        })?;
        self.repeater_group(id)?.ok_or(StorageError::NotFound(id))
    }

    /// Renames, recolours or changes how a group sends.
    pub fn repeater_group_update(
        &self,
        id: RepeaterGroupId,
        update: RepeaterGroupUpdate,
    ) -> Result<RepeaterGroup, StorageError> {
        let name = validate_group_name(&update.name)?;
        self.write(move |c| {
            let n = c.execute(
                "UPDATE repeater_groups SET name = ?2, color = ?3, send_mode = ?4, collapsed = ?5 \
                 WHERE id = ?1",
                params![
                    id,
                    name,
                    update.color.as_str(),
                    update.send_mode.as_str(),
                    update.collapsed
                ],
            )?;
            if n == 0 {
                return Err(StorageError::NotFound(id));
            }
            Ok(())
        })?;
        self.repeater_group(id)?.ok_or(StorageError::NotFound(id))
    }

    /// Removes a group; its tabs stay, ungrouped.
    pub fn repeater_group_delete(&self, id: RepeaterGroupId) -> Result<(), StorageError> {
        self.write(move |c| {
            c.execute(
                "UPDATE repeater_tabs SET group_id = NULL WHERE group_id = ?1",
                [id],
            )?;
            if c.execute("DELETE FROM repeater_groups WHERE id = ?1", [id])? == 0 {
                return Err(StorageError::NotFound(id));
            }
            Ok(())
        })
    }

    /// Sets the note of a tab (at most 20 000 characters).
    pub fn repeater_set_note(
        &self,
        tab: RepeaterTabId,
        note: String,
    ) -> Result<RepeaterTab, StorageError> {
        if note.chars().count() > 20_000 {
            return Err(StorageError::InvalidQuery("the note is too long".into()));
        }
        self.write(move |c| {
            let changed = c.execute(
                "UPDATE repeater_tabs SET note = ?2, updated_at = ?3 WHERE id = ?1",
                params![tab, note, chosproxy_core::time::now_ms()],
            )?;
            if changed == 0 {
                return Err(StorageError::NotFound(tab));
            }
            Ok(c.query_row(
                &format!("SELECT {TAB_COLUMNS} FROM repeater_tabs WHERE id = ?1"),
                [tab],
                map_tab,
            )?)
        })
    }

    /// Sets when a tab follows redirects automatically.
    pub fn repeater_set_redirect_policy(
        &self,
        tab: RepeaterTabId,
        policy: String,
    ) -> Result<RepeaterTab, StorageError> {
        if !REDIRECT_POLICIES.contains(&policy.as_str()) {
            return Err(StorageError::InvalidQuery(format!(
                "unknown redirect policy {policy:?}"
            )));
        }
        self.write(move |c| {
            let changed = c.execute(
                "UPDATE repeater_tabs SET redirect_policy = ?2, updated_at = ?3 WHERE id = ?1",
                params![tab, policy, chosproxy_core::time::now_ms()],
            )?;
            if changed == 0 {
                return Err(StorageError::NotFound(tab));
            }
            Ok(c.query_row(
                &format!("SELECT {TAB_COLUMNS} FROM repeater_tabs WHERE id = ?1"),
                [tab],
                map_tab,
            )?)
        })
    }

    /// Puts a tab in a group (after its last tab) or, with `None`, takes it
    /// out of its group.
    /// Sets or clears (`None`) a tab's highlight colour.
    pub fn repeater_set_color(
        &self,
        tab: RepeaterTabId,
        color: Option<Highlight>,
    ) -> Result<RepeaterTab, StorageError> {
        self.write(move |c| {
            let n = c.execute(
                "UPDATE repeater_tabs SET color = ?2 WHERE id = ?1",
                params![tab, color.map_or("", Highlight::as_str)],
            )?;
            if n == 0 {
                return Err(StorageError::NotFound(tab));
            }
            Ok(c.query_row(
                &format!("SELECT {TAB_COLUMNS} FROM repeater_tabs WHERE id = ?1"),
                [tab],
                map_tab,
            )?)
        })
    }

    pub fn repeater_set_group(
        &self,
        tab: RepeaterTabId,
        group: Option<RepeaterGroupId>,
    ) -> Result<RepeaterTab, StorageError> {
        self.write(move |c| {
            if let Some(g) = group {
                let exists: bool = c.query_row(
                    "SELECT EXISTS(SELECT 1 FROM repeater_groups WHERE id = ?1)",
                    [g],
                    |r| r.get(0),
                )?;
                if !exists {
                    return Err(StorageError::NotFound(g));
                }
            }
            let n = c.execute(
                "UPDATE repeater_tabs SET group_id = ?2 WHERE id = ?1",
                params![tab, group],
            )?;
            if n == 0 {
                return Err(StorageError::NotFound(tab));
            }
            if let Some(g) = group {
                let mut members = group_members(c, g)?;
                members.retain(|t| *t != tab);
                members.push(tab);
                gather(c, &members)?;
            }
            Ok(())
        })?;
        self.repeater_tab(tab)?.ok_or(StorageError::NotFound(tab))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use crate::OpenOptions;
    use crate::model::NewExchange;
    use chosproxy_core::Source;

    fn target() -> RepeaterTarget {
        RepeaterTarget {
            scheme: "HTTPS".into(),
            host: " Lab.Test ".into(),
            port: 8443,
        }
    }

    fn new_tab(name: &str) -> NewRepeaterTab {
        NewRepeaterTab {
            name: name.into(),
            target: target(),
            request: "GET /a?b=1 HTTP/1.1\nHost: lab.test\n\n".into(),
            head_crlf: true,
            body_crlf: false,
        }
    }

    fn exchange(status: Option<u16>) -> NewExchange {
        NewExchange {
            started_at: 1,
            duration_ms: Some(5),
            source: Source::Repeater,
            listener: None,
            client_addr: None,
            scheme: "https".into(),
            method: "GET".into(),
            host: "lab.test".into(),
            port: 8443,
            path: "/a?b=1".into(),
            extension: None,
            url: "https://lab.test:8443/a?b=1".into(),
            http_version: "HTTP/1.1".into(),
            status,
            reason: None,
            mime: None,
            request: b"GET /a?b=1 HTTP/1.1\r\n\r\n".to_vec(),
            response: status.map(|_| b"HTTP/1.1 200 OK\r\n\r\n".to_vec()),
            original_request: None,
            original_response: None,
            request_len: 10,
            response_len: status.map(|_| 19),
            server_ip: None,
            tls: true,
            error: status.is_none().then(|| "no response".into()),
            notes: None,
            flags: 0,
            ws_key: None,
        }
    }

    fn open() -> (tempfile::TempDir, ProjectDb) {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        (dir, db)
    }

    #[test]
    fn paths_for_scope() {
        assert_eq!(request_path("GET /a?b HTTP/1.1\n"), "/a?b");
        assert_eq!(request_path("GET http://h:1/x/y HTTP/1.1\n"), "/x/y");
        assert_eq!(request_path("GET http://h HTTP/1.1\n"), "/");
        assert_eq!(request_path("CONNECT h:443 HTTP/1.1\n"), "/");
        assert_eq!(request_path(""), "/");
    }

    #[test]
    fn notes_and_redirect_policies_are_kept() {
        let (_d, db) = open();
        let tab = db.repeater_create(new_tab("a")).unwrap();
        assert_eq!(tab.redirect_policy, "never");
        let tab = db
            .repeater_set_redirect_policy(tab.id, "inScope".into())
            .unwrap();
        assert_eq!(tab.redirect_policy, "inScope");
        assert!(
            db.repeater_set_redirect_policy(tab.id, "sometimes".into())
                .is_err()
        );
        assert!(
            db.repeater_set_redirect_policy(999, "always".into())
                .is_err()
        );
        let tab = db.repeater_set_note(tab.id, "login flow".into()).unwrap();
        let again = db.repeater_tabs().unwrap();
        assert_eq!(again[0].note, "login flow");
        assert_eq!(again[0].redirect_policy, "inScope");
        assert_eq!(again[0].id, tab.id);
    }

    #[test]
    fn create_update_duplicate_move_delete() {
        let (_dir, db) = open();
        let a = db.repeater_create(new_tab("")).unwrap();
        assert_eq!(a.name, "1");
        assert_eq!((a.scheme.as_str(), a.host.as_str()), ("https", "lab.test"));
        assert!(a.update_content_length && a.head_crlf && !a.body_crlf);
        let b = db.repeater_create(new_tab(" Login ")).unwrap();
        assert_eq!(b.name, "Login");

        let updated = db
            .repeater_update(
                a.id,
                RepeaterTabUpdate {
                    name: "Search".into(),
                    target: RepeaterTarget {
                        scheme: "http".into(),
                        host: "127.0.0.1".into(),
                        port: 9090,
                    },
                    request: "POST /s HTTP/1.1\n\nq=1".into(),
                    update_content_length: false,
                    reuse_connection: true,
                    use_cookie_jar: true,
                },
            )
            .unwrap();
        assert_eq!(updated.name, "Search");
        assert_eq!(updated.port, 9090);
        assert!(!updated.update_content_length);
        assert!(updated.request.starts_with("POST"));

        let copy = db.repeater_duplicate(a.id).unwrap();
        assert_eq!(copy.name, "Search (copy)");
        assert!(!copy.update_content_length && copy.reuse_connection && copy.use_cookie_jar);
        let order: Vec<i64> = db.repeater_tabs().unwrap().iter().map(|t| t.id).collect();
        assert_eq!(order, vec![a.id, copy.id, b.id]);

        db.repeater_move(b.id, 0).unwrap();
        let order: Vec<i64> = db.repeater_tabs().unwrap().iter().map(|t| t.id).collect();
        assert_eq!(order, vec![b.id, a.id, copy.id]);

        db.repeater_delete(copy.id).unwrap();
        assert!(matches!(
            db.repeater_delete(copy.id),
            Err(StorageError::NotFound(_))
        ));
        assert_eq!(db.repeater_tabs().unwrap().len(), 2);
    }

    #[test]
    fn rejects_bad_input() {
        let (_dir, db) = open();
        let mut bad = new_tab("x");
        bad.target.scheme = "ftp".into();
        assert!(db.repeater_create(bad).is_err());
        let mut bad = new_tab("x");
        bad.target.host = "a/b".into();
        assert!(db.repeater_create(bad).is_err());
        let mut bad = new_tab("x");
        bad.target.port = 0;
        assert!(db.repeater_create(bad).is_err());
        let mut bad = new_tab("x");
        bad.request = "a".repeat(MAX_REPEATER_REQUEST_BYTES + 1);
        assert!(db.repeater_create(bad).is_err());
        let tab = db.repeater_create(new_tab("x")).unwrap();
        let blank_name = RepeaterTabUpdate {
            name: "  ".into(),
            target: target(),
            request: String::new(),
            update_content_length: true,
            reuse_connection: false,
            use_cookie_jar: false,
        };
        assert!(db.repeater_update(tab.id, blank_name).is_err());
    }

    #[test]
    fn history_newest_first_and_removed_with_the_tab() {
        let (_dir, db) = open();
        let tab = db.repeater_create(new_tab("")).unwrap();
        let ok = db.insert(exchange(Some(200))).unwrap();
        let failed = db.insert(exchange(None)).unwrap();
        let details = SendDetails {
            timing: SendTimingRow {
                total_ms: 12,
                wait_ms: 7,
                ..SendTimingRow::default()
            },
            reused: true,
            jar_cookies: vec!["sid".into(), "csrf".into()],
        };
        let first = db
            .repeater_record_send(tab.id, ok, details.clone())
            .unwrap();
        assert_eq!(first.status, Some(200));
        assert_eq!(first.timing.total_ms, 12);
        assert!(first.reused);
        assert_eq!(first.jar_cookies, vec!["sid", "csrf"]);
        db.repeater_record_send(tab.id, failed, SendDetails::default())
            .unwrap();
        assert!(db.repeater_record_send(999, ok, details).is_err());

        let history = db.repeater_history(tab.id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].exchange_id, failed);
        assert_eq!(history[0].error.as_deref(), Some("no response"));
        assert_eq!(history[1].url, "https://lab.test:8443/a?b=1");

        db.repeater_delete(tab.id).unwrap();
        assert!(db.repeater_history(tab.id).unwrap().is_empty());
        assert!(db.exchange(ok).unwrap().is_some(), "exchanges stay");
    }

    #[test]
    fn groups_keep_their_tabs_together() {
        let (_dir, db) = open();
        let ids: Vec<i64> = (0..5)
            .map(|_| db.repeater_create(new_tab("")).unwrap().id)
            .collect();
        // Tabs 2 and 4 grouped: they end up side by side where tab 2 was.
        let g = db
            .repeater_group_create(" Race ", Highlight::Red, vec![ids[1], ids[3]])
            .unwrap();
        assert_eq!(
            (g.name.as_str(), g.send_mode),
            ("Race", GroupSendMode::Sequence)
        );
        let order: Vec<i64> = db.repeater_tabs().unwrap().iter().map(|t| t.id).collect();
        assert_eq!(order, vec![ids[0], ids[1], ids[3], ids[2], ids[4]]);

        // Adding tab 1 puts it after the group's last tab.
        let t = db.repeater_set_group(ids[0], Some(g.id)).unwrap();
        assert_eq!(t.group_id, Some(g.id));
        // A tab's own colour, set and cleared.
        assert_eq!(t.color, None);
        let coloured = db.repeater_set_color(t.id, Some(Highlight::Green)).unwrap();
        assert_eq!(coloured.color, Some(Highlight::Green));
        assert_eq!(db.repeater_set_color(t.id, None).unwrap().color, None);
        assert!(db.repeater_set_color(9_999, None).is_err());
        let members: Vec<i64> = db
            .repeater_group_tabs(g.id)
            .unwrap()
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(members, vec![ids[1], ids[3], ids[0]]);

        let g = db
            .repeater_group_update(
                g.id,
                RepeaterGroupUpdate {
                    name: "Login".into(),
                    color: Highlight::Green,
                    send_mode: GroupSendMode::Parallel,
                    collapsed: true,
                },
            )
            .unwrap();
        assert_eq!(
            (g.color, g.send_mode, g.collapsed),
            (Highlight::Green, GroupSendMode::Parallel, true)
        );
        assert_eq!(db.repeater_groups().unwrap().len(), 1);

        // A duplicate joins the original's group.
        let copy = db.repeater_duplicate(ids[1]).unwrap();
        assert_eq!(copy.group_id, Some(g.id));

        db.repeater_set_group(ids[0], None).unwrap();
        assert_eq!(db.repeater_group_tabs(g.id).unwrap().len(), 3);
        db.repeater_group_delete(g.id).unwrap();
        assert!(
            db.repeater_tabs()
                .unwrap()
                .iter()
                .all(|t| t.group_id.is_none())
        );
        assert!(db.repeater_set_group(ids[0], Some(g.id)).is_err());
        assert!(
            db.repeater_group_create(" ", Highlight::Red, vec![])
                .is_err()
        );
    }

    #[test]
    fn tab_limit() {
        let (_dir, db) = open();
        for _ in 0..MAX_REPEATER_TABS {
            db.repeater_create(new_tab("t")).unwrap();
        }
        assert!(db.repeater_create(new_tab("t")).is_err());
    }
}
