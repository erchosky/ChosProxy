//! Application logging: a daily-rotated file in the log directory plus
//! stderr, with a level that can be changed at runtime.
//!
//! Traffic content is never logged by default; code that logs header values
//! must go through `chosproxy_core::redact`.

use chosproxy_core::config::LogLevel;
use std::path::Path;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry, fmt, reload};

/// Environment variable overriding the configured level (e.g. `debug`,
/// `chosproxy_proxy=trace`).
pub const LOG_ENV: &str = "CHOSPROXY_LOG";

/// Keeps the log writer alive and allows changing the level.
pub struct LogHandle {
    reload: reload::Handle<EnvFilter, Registry>,
    _guard: tracing_appender::non_blocking::WorkerGuard,
}

impl std::fmt::Debug for LogHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogHandle").finish_non_exhaustive()
    }
}

fn filter_for(level: LogLevel) -> EnvFilter {
    std::env::var(LOG_ENV)
        .ok()
        .and_then(|v| EnvFilter::try_new(v).ok())
        .unwrap_or_else(|| {
            // Our crates at the chosen level; dependencies stay quieter.
            EnvFilter::new(format!(
                "warn,chosproxy={0},chosproxy_core={0},chosproxy_http={0},chosproxy_storage={0},\
                 chosproxy_proxy={0},chosproxy_app={0},chosproxy_desktop={0}",
                level.as_filter()
            ))
        })
}

/// Installs the global subscriber. Call once at startup.
pub fn init(log_dir: &Path, level: LogLevel) -> Result<LogHandle, String> {
    chosproxy_core::fs::create_private_dir(log_dir).map_err(|e| e.to_string())?;
    let appender = tracing_appender::rolling::daily(log_dir, "chosproxy.log");
    let (file_writer, guard) = tracing_appender::non_blocking(appender);
    let (filter, reload) = reload::Layer::new(filter_for(level));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(file_writer).with_ansi(false))
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init()
        .map_err(|e| e.to_string())?;
    Ok(LogHandle {
        reload,
        _guard: guard,
    })
}

impl LogHandle {
    /// Changes the level at runtime.
    pub fn set_level(&self, level: LogLevel) {
        if let Err(e) = self.reload.reload(filter_for(level)) {
            tracing::warn!(error = %e, "could not change log level");
        }
    }
}
