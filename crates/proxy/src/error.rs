use std::net::SocketAddr;

/// Errors managing listeners.
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    /// The address could not be bound (in use, permission denied, …).
    #[error("cannot listen on {addr}: {source}")]
    Bind {
        /// Requested address.
        addr: SocketAddr,
        /// Underlying error.
        source: std::io::Error,
    },
    /// A listener with this id is already running.
    #[error("listener {0:?} is already running")]
    AlreadyRunning(String),
    /// The listener configuration is not acceptable.
    #[error("invalid listener configuration: {0}")]
    InvalidConfig(String),
}
