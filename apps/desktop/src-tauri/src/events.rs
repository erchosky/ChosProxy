//! Forwards engine events to the WebView, coalescing history updates so a
//! busy proxy cannot flood the UI (at most one history event per interval).

use chosproxy_core::event::EngineEvent;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::{self, error::RecvError};

/// Name of the event the frontend listens to.
pub(crate) const ENGINE_EVENT: &str = "engine://event";
const HISTORY_INTERVAL: Duration = Duration::from_millis(250);

pub(crate) fn forward(app: AppHandle, mut rx: broadcast::Receiver<EngineEvent>) {
    tauri::async_runtime::spawn(async move {
        let mut pending: Option<(i64, u32)> = None;
        let mut tick = tokio::time::interval(HISTORY_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                received = rx.recv() => match received {
                    Ok(EngineEvent::HistoryAppended { last_id, added }) => {
                        let (id, n) = pending.unwrap_or((0, 0));
                        pending = Some((id.max(last_id), n.saturating_add(added)));
                    }
                    Ok(other) => emit(&app, &other),
                    // Missed events: make the UI refresh history anyway.
                    Err(RecvError::Lagged(_)) => {
                        pending = Some(pending.unwrap_or((0, 0)));
                    }
                    Err(RecvError::Closed) => break,
                },
                _ = tick.tick() => {
                    if let Some((last_id, added)) = pending.take() {
                        emit(&app, &EngineEvent::HistoryAppended { last_id, added });
                    }
                }
            }
        }
    });
}

fn emit(app: &AppHandle, event: &EngineEvent) {
    if let Err(e) = app.emit(ENGINE_EVENT, event) {
        tracing::warn!(error = %e, "could not deliver event to the UI");
    }
}
