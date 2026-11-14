//! The event log: a bounded record of what happened (listeners, projects,
//! scope, CA, warnings and errors), so notices that scrolled away can still
//! be read. Entries are also appended to a private JSON Lines file in the
//! log directory, so the entries of earlier sessions are shown after a
//! restart (the last [`EVENT_LOG_CAPACITY`] of them).

use chosproxy_core::event::{EngineEvent, ListenerState, NoticeLevel};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use ts_rs::TS;

/// Entries kept; older ones are discarded.
pub const EVENT_LOG_CAPACITY: usize = 2000;

/// What an entry is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LogCategory {
    /// Engine start, configuration.
    System,
    /// Proxy listeners.
    Listener,
    /// Project opened or closed.
    Project,
    /// Scope rules.
    Scope,
    /// Certificate authority and HTTPS.
    Tls,
    /// Any other notice shown to the user.
    Notice,
}

/// One event log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LogEntry {
    /// Increasing sequence number.
    #[ts(type = "number")]
    pub id: u64,
    /// Unix epoch ms.
    #[ts(type = "number")]
    pub at: i64,
    /// Severity.
    pub level: NoticeLevel,
    /// Area.
    pub category: LogCategory,
    /// Text.
    pub message: String,
    /// Recorded in an earlier session (read back from the log file).
    #[serde(default)]
    pub earlier: bool,
}

#[derive(Debug, Default)]
struct Entries {
    next_id: u64,
    items: VecDeque<LogEntry>,
    /// Append handle of the log file; `None` keeps the log in memory only.
    file: Option<std::fs::File>,
}

/// The log. Cheap to share.
#[derive(Debug, Default)]
pub(crate) struct EventLog {
    entries: Mutex<Entries>,
}

/// Name of the log file in the log directory.
pub const EVENT_LOG_FILE: &str = "event-log.jsonl";

impl EventLog {
    /// A log kept in `path` too: the entries of earlier sessions are read
    /// back (the last [`EVENT_LOG_CAPACITY`]), the file is rewritten with
    /// only those, and new entries are appended. When the file cannot be
    /// used, the log stays in memory and says so.
    pub(crate) fn persistent(path: &Path) -> EventLog {
        let log = EventLog::default();
        let earlier = read_entries(path);
        let opened = (|| -> std::io::Result<std::fs::File> {
            let mut text = String::new();
            for e in &earlier {
                if let Ok(line) = serde_json::to_string(&LogEntry {
                    earlier: false,
                    ..e.clone()
                }) {
                    text.push_str(&line);
                    text.push('\n');
                }
            }
            chosproxy_core::fs::write_private_atomic(path, text.as_bytes())?;
            std::fs::OpenOptions::new().append(true).open(path)
        })();
        {
            let mut g = log.lock();
            g.next_id = earlier.iter().map(|e| e.id).max().unwrap_or(0);
            g.items = earlier.into();
            match opened {
                Ok(file) => g.file = Some(file),
                Err(e) => tracing::warn!(error = %e, "event log file unavailable"),
            }
        }
        if log.lock().file.is_none() {
            log.record(
                NoticeLevel::Warning,
                LogCategory::System,
                "The event log file cannot be written; this session's log is kept in memory only."
                    .into(),
            );
        }
        log
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Entries> {
        match self.entries.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub(crate) fn record(&self, level: NoticeLevel, category: LogCategory, message: String) {
        let mut g = self.lock();
        g.next_id += 1;
        let id = g.next_id;
        if g.items.len() >= EVENT_LOG_CAPACITY {
            g.items.pop_front();
        }
        let entry = LogEntry {
            id,
            at: chosproxy_core::time::now_ms(),
            level,
            category,
            message,
            earlier: false,
        };
        if let Some(file) = g.file.as_mut()
            && let Ok(mut line) = serde_json::to_string(&entry)
        {
            line.push('\n');
            if let Err(e) = file.write_all(line.as_bytes()) {
                tracing::warn!(error = %e, "event log file write failed; keeping the log in memory");
                g.file = None;
            }
        }
        g.items.push_back(entry);
    }

    /// All entries, oldest first.
    pub(crate) fn entries(&self) -> Vec<LogEntry> {
        match self.entries.lock() {
            Ok(g) => g.items.iter().cloned().collect(),
            Err(poisoned) => poisoned.into_inner().items.iter().cloned().collect(),
        }
    }

    /// Clears the log, the file included.
    pub(crate) fn clear(&self) {
        let mut g = self.lock();
        g.items.clear();
        if let Some(file) = g.file.as_ref()
            && let Err(e) = file.set_len(0)
        {
            tracing::warn!(error = %e, "event log file could not be cleared");
        }
    }

    fn record_event(&self, event: &EngineEvent) {
        match event {
            EngineEvent::ListenerChanged { listener_id, state } => match state {
                ListenerState::Running { address } => self.record(
                    NoticeLevel::Info,
                    LogCategory::Listener,
                    format!("Listener {listener_id} started on {address}"),
                ),
                ListenerState::Stopped => self.record(
                    NoticeLevel::Info,
                    LogCategory::Listener,
                    format!("Listener {listener_id} stopped"),
                ),
                ListenerState::Failed { error } => self.record(
                    NoticeLevel::Error,
                    LogCategory::Listener,
                    format!("Listener {listener_id} failed: {error}"),
                ),
            },
            EngineEvent::ScopeChanged => self.record(
                NoticeLevel::Info,
                LogCategory::Scope,
                "Scope rules changed".into(),
            ),
            EngineEvent::CaChanged => self.record(
                NoticeLevel::Info,
                LogCategory::Tls,
                "Local certificate authority changed".into(),
            ),
            EngineEvent::Notice { level, message } => {
                self.record(*level, LogCategory::Notice, message.clone());
            }
            // Recorded with details by the engine, or too frequent to log.
            EngineEvent::ProjectChanged
            | EngineEvent::HistoryAppended { .. }
            | EngineEvent::FindingsChanged { .. }
            | EngineEvent::TasksChanged
            | EngineEvent::AgentChanged { .. }
            | EngineEvent::PluginsChanged
            | EngineEvent::OastChanged
            | EngineEvent::DomChanged
            | EngineEvent::InterceptChanged { .. }
            | EngineEvent::BrowserChanged
            | EngineEvent::WebSocketsChanged => {}
        }
    }
}

/// The last [`EVENT_LOG_CAPACITY`] readable entries of a log file, marked
/// as earlier; unreadable lines are skipped.
fn read_entries(path: &Path) -> Vec<LogEntry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut items: VecDeque<LogEntry> = VecDeque::new();
    for line in text.lines() {
        if let Ok(mut e) = serde_json::from_str::<LogEntry>(line) {
            e.earlier = true;
            if items.len() >= EVENT_LOG_CAPACITY {
                items.pop_front();
            }
            items.push_back(e);
        }
    }
    items.into()
}

/// Records engine events on a background thread until the engine is gone.
pub(crate) fn spawn_recorder(log: Arc<EventLog>, mut rx: broadcast::Receiver<EngineEvent>) {
    let spawned = std::thread::Builder::new()
        .name("event-log".into())
        .spawn(move || {
            loop {
                match rx.blocking_recv() {
                    Ok(event) => log.record_event(&event),
                    Err(RecvError::Lagged(n)) => log.record(
                        NoticeLevel::Warning,
                        LogCategory::System,
                        format!("{n} events arrived too fast and were not recorded"),
                    ),
                    Err(RecvError::Closed) => break,
                }
            }
        });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "cannot start the event log thread");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn kept_across_sessions_in_a_private_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(EVENT_LOG_FILE);
        {
            let log = EventLog::persistent(&path);
            log.record(NoticeLevel::Info, LogCategory::System, "first".into());
            log.record(NoticeLevel::Error, LogCategory::Listener, "second".into());
        }
        // A damaged line is skipped.
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("{not json\n");
        std::fs::write(&path, text).unwrap();

        let log = EventLog::persistent(&path);
        log.record(NoticeLevel::Info, LogCategory::Project, "third".into());
        let e = log.entries();
        let names: Vec<(&str, bool)> = e.iter().map(|e| (e.message.as_str(), e.earlier)).collect();
        assert_eq!(names, [("first", true), ("second", true), ("third", false)]);
        assert!(
            e.windows(2).all(|w| w[0].id < w[1].id),
            "ids keep increasing"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        log.clear();
        assert!(log.entries().is_empty());
        assert!(
            EventLog::persistent(&path).entries().is_empty(),
            "file cleared too"
        );

        // Only the last entries are kept.
        let log = EventLog::persistent(&path);
        for i in 0..EVENT_LOG_CAPACITY + 3 {
            log.record(NoticeLevel::Info, LogCategory::System, format!("e{i}"));
        }
        drop(log);
        let e = EventLog::persistent(&path).entries();
        assert_eq!(e.len(), EVENT_LOG_CAPACITY);
        assert_eq!(e[0].message, "e3");
        let lines = std::fs::read_to_string(&path).unwrap().lines().count();
        assert_eq!(lines, EVENT_LOG_CAPACITY, "the file is compacted on start");
    }

    #[test]
    fn bounded_and_ordered() {
        let log = EventLog::default();
        for i in 0..EVENT_LOG_CAPACITY + 5 {
            log.record(NoticeLevel::Info, LogCategory::System, format!("e{i}"));
        }
        let e = log.entries();
        assert_eq!(e.len(), EVENT_LOG_CAPACITY);
        assert_eq!(e[0].message, "e5");
        assert!(e.windows(2).all(|w| w[0].id < w[1].id));
        log.clear();
        assert!(log.entries().is_empty());
    }

    #[test]
    fn maps_engine_events() {
        let log = EventLog::default();
        log.record_event(&EngineEvent::ListenerChanged {
            listener_id: "default".into(),
            state: ListenerState::Failed {
                error: "address in use".into(),
            },
        });
        log.record_event(&EngineEvent::HistoryAppended {
            last_id: 1,
            added: 1,
        });
        log.record_event(&EngineEvent::Notice {
            level: NoticeLevel::Warning,
            message: "x".into(),
        });
        let e = log.entries();
        assert_eq!(e.len(), 2);
        assert_eq!(
            (e[0].level, e[0].category),
            (NoticeLevel::Error, LogCategory::Listener)
        );
        assert_eq!(e[1].category, LogCategory::Notice);
    }
}
