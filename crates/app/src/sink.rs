//! Bridges the proxy's capture sink to the open project database.

use chosproxy_core::event::{EngineEvent, NoticeLevel};
use chosproxy_core::exchange::CapturedExchange;
use chosproxy_core::scope::{Scope, ScopeTarget};
use chosproxy_core::websocket::WsEvent;
use chosproxy_proxy::CaptureSink;
use chosproxy_storage::{ProjectDb, StorageError};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::broadcast;

/// The project currently receiving captures (shared with the engine).
pub(crate) type CurrentDb = Arc<RwLock<Option<ProjectDb>>>;

/// The scope proxy traffic must be in to be stored; `None` stores all.
pub(crate) type StoreFilter = Arc<RwLock<Option<Scope>>>;

pub(crate) struct ProjectSink {
    pub(crate) db: CurrentDb,
    pub(crate) dropped: Arc<AtomicU64>,
    pub(crate) events: broadcast::Sender<EngineEvent>,
    pub(crate) filter: StoreFilter,
    /// WebSocket connections whose handshake was out of scope (not stored).
    pub(crate) skipped_ws: Mutex<HashSet<i64>>,
}

/// Whether `target` passes the store filter.
pub(crate) fn wanted(filter: &StoreFilter, target: &ScopeTarget<'_>) -> bool {
    let guard = match filter.read() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.as_ref().is_none_or(|scope| scope.contains(target))
}

impl ProjectSink {
    fn skipped(&self) -> std::sync::MutexGuard<'_, HashSet<i64>> {
        match self.skipped_ws.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn current(&self) -> Option<ProjectDb> {
        match self.db.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn report(&self, result: Result<(), StorageError>) {
        if let Err(e) = result {
            let n = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            // Warn on the first drop and then every 500 to avoid flooding.
            if n == 1 || n.is_multiple_of(500) {
                tracing::warn!(error = %e, dropped = n, "capture dropped");
                let _ = self.events.send(EngineEvent::Notice {
                    level: NoticeLevel::Warning,
                    message: format!(
                        "{n} captured item(s) could not be stored ({e}). \
                         Traffic was still forwarded."
                    ),
                });
            }
        }
    }
}

impl CaptureSink for ProjectSink {
    fn record(&self, exchange: CapturedExchange) {
        let target = ScopeTarget {
            scheme: &exchange.scheme,
            host: &exchange.host,
            port: exchange.port,
            path: &exchange.path,
        };
        if !wanted(&self.filter, &target) {
            return;
        }
        let Some(db) = self.current() else {
            self.report(Err(StorageError::Closed));
            return;
        };
        // Cookies set by proxied responses go to the project cookie jar.
        if let Some(response) = &exchange.response {
            let entries = crate::cookies::entries_from_response(
                response,
                &exchange.host,
                &exchange.path,
                chosproxy_core::time::now_ms(),
            );
            if let Err(e) = db.cookie_jar_submit("proxy", entries) {
                tracing::debug!(error = %e, "cookie jar update skipped");
            }
        }
        self.report(db.submit(exchange));
    }

    fn ws_event(&self, event: WsEvent) {
        let skip = match &event {
            WsEvent::Opened {
                key,
                scheme,
                host,
                port,
                path,
                ..
            } => {
                let target = ScopeTarget {
                    scheme,
                    host,
                    port: *port,
                    path,
                };
                let out = !wanted(&self.filter, &target);
                if out {
                    self.skipped().insert(*key);
                }
                out
            }
            WsEvent::Message(m) => self.skipped().contains(&m.key),
            WsEvent::Closed { key, .. } => self.skipped().remove(key),
        };
        if skip {
            return;
        }
        let result = match self.current() {
            Some(db) => db.submit_ws(event),
            None => Err(StorageError::Closed),
        };
        self.report(result);
    }
}
