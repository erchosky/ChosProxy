//! Engine operations for Repeater: tabs and groups, sending (alone, as a
//! group, over a kept connection, with cookies from the jar), cancelling,
//! history, and the cookie jar.

use super::{Engine, limits_of, lock};
use crate::AppError;
use crate::cookies;
use crate::repeater::{self, RepeaterRedirect};
use crate::types::ExchangeDetail;
use chosproxy_core::time::now_ms;
use chosproxy_http::head::Headers;
use chosproxy_http::{ResponseHead, Scheme};
use chosproxy_request::{
    Connection, Destination, ParallelRequest, SendError, SendLimits, SendTiming, Sent,
    UpstreamSecurity,
};
use chosproxy_storage::model::Highlight;
use chosproxy_storage::{
    GroupSendMode, JarCookie, NewRepeaterTab, ProjectDb, RepeaterGroup, RepeaterGroupId,
    RepeaterGroupUpdate, RepeaterSendRow, RepeaterTab, RepeaterTabId, RepeaterTabUpdate,
    RepeaterTarget, SendDetails, SendTimingRow, validate_target,
};
use chosproxy_tls::UpstreamTls;
use serde::Serialize;
use ts_rs::TS;

/// A connection kept open for a tab, and what it was opened for.
pub(crate) struct KeptConnection {
    key: ConnectionKey,
    connection: Connection,
}

/// Target and TLS verification of a connection: a kept connection is only
/// reused for the same.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectionKey {
    target: RepeaterTarget,
    verified: bool,
}

/// A tab's request, ready to go.
struct Prepared {
    tab: RepeaterTab,
    target: RepeaterTarget,
    request: Vec<u8>,
    jar_cookies: Vec<String>,
    destination: Destination,
    security: UpstreamSecurity,
    key: ConnectionKey,
}

/// What happened to one tab of a group send.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeaterGroupItem {
    /// The tab.
    #[ts(type = "number")]
    pub tab_id: RepeaterTabId,
    /// Its history entry, when the request was sent.
    pub send: Option<RepeaterSendRow>,
    /// Why it was not sent.
    pub problem: Option<String>,
}

/// The outcome of sending a group.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeaterGroupSendResult {
    /// The group.
    #[ts(type = "number")]
    pub group_id: RepeaterGroupId,
    /// How it was sent.
    pub mode: GroupSendMode,
    /// One item per tab, in tab order.
    pub items: Vec<RepeaterGroupItem>,
}

fn timing_row(t: SendTiming) -> SendTimingRow {
    SendTimingRow {
        dns_ms: t.dns_ms,
        connect_ms: t.connect_ms,
        tls_ms: t.tls_ms,
        wait_ms: t.wait_ms,
        download_ms: t.download_ms,
        total_ms: t.total_ms,
    }
}

/// The outcome of a send: its history entry and the exchange to display.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeaterSendResult {
    /// The tab after saving the edits that were sent.
    pub tab: RepeaterTab,
    /// History entry.
    pub send: RepeaterSendRow,
    /// Request and response as stored.
    pub detail: ExchangeDetail,
}

/// A past send prepared for display and for restoring into the editor.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeaterSnapshot {
    /// Request and response as stored.
    pub detail: ExchangeDetail,
    /// The request as editor text, when it can be edited as text.
    pub request_text: Option<String>,
}

impl Engine {
    /// Repeater tabs of the open project, in order.
    pub async fn repeater_tabs(&self) -> Result<Vec<RepeaterTab>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_tabs()).await??)
    }

    /// Creates a tab.
    pub async fn repeater_create(&self, tab: NewRepeaterTab) -> Result<RepeaterTab, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_create(tab)).await??)
    }

    /// Creates a tab holding a request from the history ("Send to Repeater").
    pub async fn repeater_from_exchange(&self, exchange_id: i64) -> Result<RepeaterTab, AppError> {
        let db = self.db()?;
        let stored = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || db.exchange(exchange_id))
                .await??
                .ok_or(AppError::UnknownExchange(exchange_id))?
        };
        if stored.row.request_truncated {
            return Err(AppError::Invalid(format!(
                "request #{exchange_id} was stored truncated (larger than the capture limit) \
                 and cannot be repeated"
            )));
        }
        let text = repeater::editor_text(&stored.request)?;
        let tab = NewRepeaterTab {
            name: String::new(),
            target: RepeaterTarget {
                scheme: stored.row.scheme.clone(),
                host: stored.row.host.clone(),
                port: stored.row.port,
            },
            request: text.text,
            head_crlf: text.head_crlf,
            body_crlf: text.body_crlf,
        };
        Ok(tokio::task::spawn_blocking(move || db.repeater_create(tab)).await??)
    }

    /// Saves a tab's name, target, request and options.
    pub async fn repeater_update(
        &self,
        id: RepeaterTabId,
        update: RepeaterTabUpdate,
    ) -> Result<RepeaterTab, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_update(id, update)).await??)
    }

    /// Copies a tab (without its history).
    pub async fn repeater_duplicate(&self, id: RepeaterTabId) -> Result<RepeaterTab, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_duplicate(id)).await??)
    }

    /// Moves a tab to a new index.
    pub async fn repeater_move(&self, id: RepeaterTabId, position: u32) -> Result<(), AppError> {
        let db = self.db()?;
        let position = usize::try_from(position).unwrap_or(usize::MAX);
        Ok(tokio::task::spawn_blocking(move || db.repeater_move(id, position)).await??)
    }

    /// Closes a tab, cancelling its send if one is running. Its sends stay
    /// in the Logger.
    pub async fn repeater_close(&self, id: RepeaterTabId) -> Result<(), AppError> {
        self.repeater_cancel(id);
        lock(&self.inner.repeater_connections).remove(&id);
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_delete(id)).await??)
    }

    /// A tab's sends, newest first.
    pub async fn repeater_history(
        &self,
        id: RepeaterTabId,
    ) -> Result<Vec<RepeaterSendRow>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_history(id)).await??)
    }

    /// A past send: the stored exchange and its request as editor text.
    pub async fn repeater_snapshot(&self, exchange_id: i64) -> Result<RepeaterSnapshot, AppError> {
        let detail = self.exchange_detail(exchange_id).await?;
        let db = self.db()?;
        let stored = tokio::task::spawn_blocking(move || db.exchange(exchange_id))
            .await??
            .ok_or(AppError::UnknownExchange(exchange_id))?;
        Ok(RepeaterSnapshot {
            detail,
            request_text: repeater::editor_text(&stored.request).ok().map(|t| t.text),
        })
    }

    /// The request that follows the redirect answered by a stored send.
    pub async fn repeater_redirect(&self, exchange_id: i64) -> Result<RepeaterRedirect, AppError> {
        let db = self.db()?;
        let stored = tokio::task::spawn_blocking(move || db.exchange(exchange_id))
            .await??
            .ok_or(AppError::UnknownExchange(exchange_id))?;
        let head = stored
            .response
            .as_deref()
            .and_then(|r| {
                let end = chosproxy_http::head::find_head_end(r)?;
                ResponseHead::parse(r[..end].to_vec()).ok()
            })
            .ok_or_else(|| AppError::Invalid("the send has no response".into()))?;
        let location = head
            .header_str("location")
            .ok_or_else(|| AppError::Invalid("the response has no Location header".into()))?;
        let request = repeater::editor_text(&stored.request)?.text;
        let from = RepeaterTarget {
            scheme: stored.row.scheme,
            host: stored.row.host,
            port: stored.row.port,
        };
        repeater::follow_redirect(&request, &from, head.status, &location)
            .map_err(AppError::Invalid)
    }

    /// Cancels a running send of a tab. False if none was running.
    pub fn repeater_cancel(&self, id: RepeaterTabId) -> bool {
        let handle = lock(&self.inner.repeater_inflight).remove(&id);
        handle.map(|h| h.abort()).is_some()
    }

    /// Cancels a running group send. False if none was running.
    pub fn repeater_cancel_group(&self, id: RepeaterGroupId) -> bool {
        let handle = lock(&self.inner.repeater_group_inflight).remove(&id);
        handle.map(|h| h.abort()).is_some()
    }

    /// Cancels every running send and closes kept connections (closing the
    /// project).
    pub(crate) fn repeater_cancel_all(&self) {
        let handles: Vec<_> = lock(&self.inner.repeater_inflight)
            .drain()
            .map(|(_, h)| h)
            .chain(
                lock(&self.inner.repeater_group_inflight)
                    .drain()
                    .map(|(_, h)| h),
            )
            .collect();
        for h in handles {
            h.abort();
        }
        lock(&self.inner.repeater_connections).clear();
    }

    /// TLS client configurations: the proxy's, or (when the local CA is not
    /// loaded) a set built once for Repeater.
    pub(crate) async fn repeater_tls(&self) -> Result<UpstreamTls, AppError> {
        if let Some(tls) = self.inner.proxy.upstream_tls() {
            return Ok(tls);
        }
        let cached = match self.inner.fallback_tls.lock() {
            Ok(g) => g.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        if let Some(tls) = cached {
            return Ok(tls);
        }
        let tls = tokio::task::spawn_blocking(|| UpstreamTls::new(&[])).await??;
        if let Ok(mut g) = self.inner.fallback_tls.lock() {
            *g = Some(tls.clone());
        }
        Ok(tls)
    }

    /// The request of a tab as it will be sent: bytes built from the editor
    /// text, cookies from the jar added if the tab asks for it, TLS set up.
    async fn prepare(&self, db: &ProjectDb, tab: RepeaterTab) -> Result<Prepared, AppError> {
        self.ensure_writable()?;
        let target = validate_target(&RepeaterTarget {
            scheme: tab.scheme.clone(),
            host: tab.host.clone(),
            port: tab.port,
        })?;
        let mut request = repeater::request_bytes(
            &tab.request,
            tab.head_crlf,
            tab.body_crlf,
            tab.update_content_length,
        )?;
        let https = repeater::scheme_of(&target) == Scheme::Https;
        let mut jar_cookies = Vec::new();
        if tab.use_cookie_jar {
            let jar: Vec<JarCookie> = {
                let db = db.clone();
                tokio::task::spawn_blocking(move || db.cookie_jar_list()).await??
            };
            let path = chosproxy_storage::request_path(&tab.request);
            let matching = cookies::cookies_for(&jar, &target.host, &path, https, now_ms());
            let (with_cookies, changed) = cookies::apply_cookies(&request, &matching);
            request = with_cookies;
            jar_cookies = changed;
        }
        let verified = !self
            .inner
            .proxy
            .tls_policy()
            .skips_verification(&target.host);
        let security = self.tls_security(&target.host, https).await?;
        let destination = Destination {
            host: target.host.clone(),
            port: target.port,
            tls_name: target.host.clone(),
        };
        Ok(Prepared {
            key: ConnectionKey {
                target: target.clone(),
                verified: https && verified,
            },
            tab,
            target,
            request,
            jar_cookies,
            destination,
            security,
        })
    }

    fn send_limits(&self) -> SendLimits {
        let max = limits_of(&self.settings()).max_message_bytes.max(64 * 1024);
        SendLimits {
            max_response_bytes: max,
            ..SendLimits::default()
        }
    }

    /// Sends a prepared request: over the tab's kept connection when it
    /// keeps one (opening it if needed), otherwise on a new connection.
    async fn transmit(&self, p: &Prepared, limits: &SendLimits) -> Result<Sent, SendError> {
        let id = p.tab.id;
        // HTTP/2 requests always use their own connection.
        if !p.tab.reuse_connection || chosproxy_http::head::request_is_h2(&p.request) {
            lock(&self.inner.repeater_connections).remove(&id);
            return self
                .session_send(
                    "repeater",
                    chosproxy_core::Source::Repeater,
                    &p.destination,
                    &p.security,
                    &p.request,
                    limits,
                )
                .await;
        }
        let kept = lock(&self.inner.repeater_connections).remove(&id);
        let reusable = match kept {
            Some(mut k) if k.key == p.key => {
                k.connection.is_reusable().await.then_some(k.connection)
            }
            _ => None,
        };
        let ticket = self.pool_enter("repeater").await;
        let mut connection = match reusable {
            Some(c) => c,
            None => Connection::open(&p.destination, &p.security).await?,
        };
        let mut sent = connection.send(&p.request, limits).await;
        if let Some(ticket) = ticket {
            ticket.finish(&mut sent).await;
        }
        if matches!(&sent, Ok(s) if s.keep_alive) {
            lock(&self.inner.repeater_connections).insert(
                id,
                KeptConnection {
                    key: p.key.clone(),
                    connection,
                },
            );
        } else {
            connection.close().await;
        }
        sent
    }

    /// Stores a send in the project (exchange, tab history) and puts the
    /// cookies its response sets in the jar.
    async fn record(
        &self,
        db: &ProjectDb,
        p: Prepared,
        started_at: i64,
        outcome: Result<Sent, SendError>,
    ) -> Result<RepeaterSendRow, AppError> {
        let (timing, reused) = match &outcome {
            Ok(sent) => (sent.timing, sent.reused),
            Err(e) => (e.timing, false),
        };
        let jar_entries = match &outcome {
            Ok(sent) => cookies::entries_from_response(
                &sent.response,
                &p.target.host,
                &chosproxy_storage::request_path(&p.tab.request),
                now_ms(),
            ),
            Err(_) => Vec::new(),
        };
        let exchange = repeater::captured(
            chosproxy_core::Source::Repeater,
            &p.target,
            p.request,
            started_at,
            &outcome,
        );
        let details = SendDetails {
            timing: timing_row(timing),
            reused,
            jar_cookies: p.jar_cookies,
        };
        let tab_id = p.tab.id;
        let db = db.clone();
        Ok(tokio::task::spawn_blocking(move || {
            let exchange_id = db.insert(exchange)?;
            if let Err(e) = db.cookie_jar_store("repeater", jar_entries) {
                tracing::warn!(error = %e, "could not update the cookie jar");
            }
            db.repeater_record_send(tab_id, exchange_id, details)
        })
        .await??)
    }

    /// Saves the tab with `update`, sends its request and records the result
    /// (also when no response arrives). Fails with `Cancelled` if
    /// [`Self::repeater_cancel`] stops it.
    /// Sets the note of a tab.
    pub async fn repeater_set_note(
        &self,
        id: RepeaterTabId,
        note: String,
    ) -> Result<RepeaterTab, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_set_note(id, note)).await??)
    }

    /// Sets when a tab follows redirects automatically.
    pub async fn repeater_set_redirect_policy(
        &self,
        id: RepeaterTabId,
        policy: String,
    ) -> Result<RepeaterTab, AppError> {
        let db = self.db()?;
        Ok(
            tokio::task::spawn_blocking(move || db.repeater_set_redirect_policy(id, policy))
                .await??,
        )
    }

    pub async fn repeater_send(
        &self,
        id: RepeaterTabId,
        update: RepeaterTabUpdate,
    ) -> Result<RepeaterSendResult, AppError> {
        let db = self.db()?;
        validate_target(&update.target)?;
        let tab = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || db.repeater_update(id, update)).await??
        };
        let prepared = self.prepare(&db, tab.clone()).await?;
        let limits = self.send_limits();

        let started_at = now_ms();
        let task = {
            let mut inflight = lock(&self.inner.repeater_inflight);
            if inflight.contains_key(&id) {
                return Err(AppError::Invalid(
                    "this tab is already sending; wait or cancel it".into(),
                ));
            }
            let engine = self.clone();
            let task = tokio::spawn(async move {
                let outcome = engine.transmit(&prepared, &limits).await;
                (prepared, outcome)
            });
            inflight.insert(id, task.abort_handle());
            task
        };
        let joined = task.await;
        lock(&self.inner.repeater_inflight).remove(&id);
        let (prepared, outcome) = match joined {
            Ok(done) => done,
            Err(e) if e.is_cancelled() => {
                // The kept connection, if any, was dropped with the task.
                return Err(AppError::Cancelled);
            }
            Err(e) => return Err(e.into()),
        };
        let row = self.record(&db, prepared, started_at, outcome).await?;
        let detail = self.exchange_detail(row.exchange_id).await?;
        Ok(RepeaterSendResult {
            tab,
            send: row,
            detail,
        })
    }

    /// Repeater groups of the open project, in order.
    pub async fn repeater_groups(&self) -> Result<Vec<RepeaterGroup>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_groups()).await??)
    }

    /// Creates a group of tabs.
    pub async fn repeater_group_create(
        &self,
        name: String,
        color: Highlight,
        tabs: Vec<RepeaterTabId>,
    ) -> Result<RepeaterGroup, AppError> {
        let db = self.db()?;
        Ok(
            tokio::task::spawn_blocking(move || db.repeater_group_create(&name, color, tabs))
                .await??,
        )
    }

    /// Renames, recolours or changes the send mode of a group.
    pub async fn repeater_group_update(
        &self,
        id: RepeaterGroupId,
        update: RepeaterGroupUpdate,
    ) -> Result<RepeaterGroup, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_group_update(id, update)).await??)
    }

    /// Removes a group (its tabs stay).
    pub async fn repeater_group_delete(&self, id: RepeaterGroupId) -> Result<(), AppError> {
        self.repeater_cancel_group(id);
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_group_delete(id)).await??)
    }

    /// Sets or clears a tab's highlight colour.
    pub async fn repeater_set_color(
        &self,
        tab: RepeaterTabId,
        color: Option<chosproxy_storage::Highlight>,
    ) -> Result<RepeaterTab, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_set_color(tab, color)).await??)
    }

    /// Moves a tab into a group, or out of its group with `None`.
    pub async fn repeater_set_group(
        &self,
        tab: RepeaterTabId,
        group: Option<RepeaterGroupId>,
    ) -> Result<RepeaterTab, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.repeater_set_group(tab, group)).await??)
    }

    /// Sends every tab of a group as saved, the way the group's send mode
    /// says, and records each send in its tab. Fails with `Cancelled` if
    /// [`Self::repeater_cancel_group`] stops it (sends already finished
    /// stay recorded).
    pub async fn repeater_send_group(
        &self,
        id: RepeaterGroupId,
    ) -> Result<RepeaterGroupSendResult, AppError> {
        let db = self.db()?;
        let (group, tabs) = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || {
                let group = db
                    .repeater_group(id)?
                    .ok_or(chosproxy_storage::StorageError::NotFound(id))?;
                Ok::<_, chosproxy_storage::StorageError>((group, db.repeater_group_tabs(id)?))
            })
            .await??
        };
        if tabs.is_empty() {
            return Err(AppError::Invalid("the group has no tabs".into()));
        }
        if group.send_mode == GroupSendMode::Single
            && tabs.iter().any(|t| {
                (&t.scheme, &t.host, t.port) != (&tabs[0].scheme, &tabs[0].host, tabs[0].port)
            })
        {
            return Err(AppError::Invalid(
                "sending over a single connection needs every tab of the group to have the \
                 same target"
                    .into(),
            ));
        }
        let mode = group.send_mode;
        let task = {
            let mut inflight = lock(&self.inner.repeater_group_inflight);
            if inflight.contains_key(&id) {
                return Err(AppError::Invalid(
                    "this group is already sending; wait or cancel it".into(),
                ));
            }
            let engine = self.clone();
            let task = tokio::spawn(async move {
                match mode {
                    GroupSendMode::Sequence => engine.group_sequence(&db, tabs).await,
                    GroupSendMode::Single => engine.group_single(&db, tabs).await,
                    GroupSendMode::Parallel => engine.group_parallel(&db, tabs).await,
                }
            });
            inflight.insert(id, task.abort_handle());
            task
        };
        let joined = task.await;
        lock(&self.inner.repeater_group_inflight).remove(&id);
        let items = match joined {
            Ok(items) => items?,
            Err(e) if e.is_cancelled() => return Err(AppError::Cancelled),
            Err(e) => return Err(e.into()),
        };
        Ok(RepeaterGroupSendResult {
            group_id: id,
            mode,
            items,
        })
    }

    /// One tab after another, each as it is set (kept connection, jar);
    /// cookies set by one response are in the jar for the next request.
    async fn group_sequence(
        &self,
        db: &ProjectDb,
        tabs: Vec<RepeaterTab>,
    ) -> Result<Vec<RepeaterGroupItem>, AppError> {
        let limits = self.send_limits();
        let mut items = Vec::with_capacity(tabs.len());
        for tab in tabs {
            let tab_id = tab.id;
            let prepared = match self.prepare(db, tab).await {
                Ok(p) => p,
                Err(e) => {
                    items.push(not_sent(tab_id, e.to_string()));
                    continue;
                }
            };
            let started_at = now_ms();
            let outcome = self.transmit(&prepared, &limits).await;
            let row = self.record(db, prepared, started_at, outcome).await?;
            items.push(sent_item(row));
        }
        Ok(items)
    }

    /// One tab after another over one new connection, kept open between
    /// them; stops when the server closes it.
    async fn group_single(
        &self,
        db: &ProjectDb,
        tabs: Vec<RepeaterTab>,
    ) -> Result<Vec<RepeaterGroupItem>, AppError> {
        let limits = self.send_limits();
        let mut items = Vec::with_capacity(tabs.len());
        let mut connection: Option<Connection> = None;
        let mut stopped: Option<String> = None;
        for tab in tabs {
            let (tab_id, name) = (tab.id, tab.name.clone());
            if let Some(why) = &stopped {
                items.push(not_sent(tab_id, why.clone()));
                continue;
            }
            let prepared = match self.prepare(db, tab).await {
                Ok(p) => p,
                Err(e) => {
                    items.push(not_sent(tab_id, e.to_string()));
                    continue;
                }
            };
            let ticket = self.pool_enter("repeater").await;
            let started_at = now_ms();
            let mut outcome = match connection.as_mut() {
                Some(c) => c.send(&prepared.request, &limits).await,
                None => match Connection::open(&prepared.destination, &prepared.security).await {
                    Ok(mut c) => {
                        let sent = c.send(&prepared.request, &limits).await;
                        connection = Some(c);
                        sent
                    }
                    Err(e) => Err(e),
                },
            };
            if let Some(ticket) = ticket {
                ticket.finish(&mut outcome).await;
            }
            let open = matches!(&outcome, Ok(s) if s.keep_alive);
            if !open {
                stopped = Some(format!(
                    "not sent: the connection was closed after “{name}”"
                ));
            }
            let row = self.record(db, prepared, started_at, outcome).await?;
            items.push(sent_item(row));
        }
        if let Some(c) = connection {
            c.close().await;
        }
        Ok(items)
    }

    /// Every tab at the same moment, each on its own connection, with the
    /// last byte of every request written together.
    async fn group_parallel(
        &self,
        db: &ProjectDb,
        tabs: Vec<RepeaterTab>,
    ) -> Result<Vec<RepeaterGroupItem>, AppError> {
        let limits = self.send_limits();
        let mut items: Vec<Option<RepeaterGroupItem>> = Vec::with_capacity(tabs.len());
        let mut ready = Vec::new();
        for tab in tabs {
            let tab_id = tab.id;
            match self.prepare(db, tab).await {
                Ok(p) => {
                    ready.push((items.len(), p));
                    items.push(None);
                }
                Err(e) => items.push(Some(not_sent(tab_id, e.to_string()))),
            }
        }
        let jobs: Vec<ParallelRequest> = ready
            .iter()
            .map(|(_, p)| ParallelRequest {
                destination: p.destination.clone(),
                security: p.security.clone(),
                request: p.request.clone(),
            })
            .collect();
        let started_at = now_ms();
        let outcomes = chosproxy_request::send_parallel(jobs, &limits).await;
        for ((index, prepared), outcome) in ready.into_iter().zip(outcomes) {
            let row = self.record(db, prepared, started_at, outcome).await?;
            if let Some(slot) = items.get_mut(index) {
                *slot = Some(sent_item(row));
            }
        }
        Ok(items.into_iter().flatten().collect())
    }

    /// Cookies in the jar.
    pub async fn cookie_jar(&self) -> Result<Vec<JarCookie>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.cookie_jar_list()).await??)
    }

    /// Changes a jar cookie's value.
    pub async fn cookie_jar_set_value(&self, id: i64, value: String) -> Result<(), AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.cookie_jar_set_value(id, value)).await??)
    }

    /// Removes jar cookies; returns how many.
    pub async fn cookie_jar_delete(&self, ids: Vec<i64>) -> Result<usize, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.cookie_jar_delete(ids)).await??)
    }

    /// Empties the jar; returns how many cookies were removed.
    pub async fn cookie_jar_clear(&self) -> Result<usize, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.cookie_jar_clear()).await??)
    }
}

fn not_sent(tab_id: RepeaterTabId, problem: String) -> RepeaterGroupItem {
    RepeaterGroupItem {
        tab_id,
        send: None,
        problem: Some(problem),
    }
}

fn sent_item(row: RepeaterSendRow) -> RepeaterGroupItem {
    RepeaterGroupItem {
        tab_id: row.tab_id,
        send: Some(row),
        problem: None,
    }
}
