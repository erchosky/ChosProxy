use serde::Serialize;
use ts_rs::TS;

/// Errors returned by engine operations. Serializable so UIs can show them.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// No project is open.
    #[error("no project is open")]
    NoProject,
    /// Configuration could not be saved or is invalid.
    #[error(transparent)]
    Config(#[from] chosproxy_core::config::ConfigError),
    /// Storage failure.
    #[error(transparent)]
    Storage(#[from] chosproxy_storage::StorageError),
    /// Proxy failure.
    #[error(transparent)]
    Proxy(#[from] chosproxy_proxy::ProxyError),
    /// Certificate authority or TLS failure.
    #[error(transparent)]
    Tls(#[from] chosproxy_tls::TlsError),
    /// Unknown listener id.
    #[error("no listener with id {0:?}")]
    UnknownListener(String),
    /// Unknown exchange id.
    #[error("exchange {0} not found")]
    UnknownExchange(i64),
    /// Filesystem error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Invalid input from the caller.
    #[error("{0}")]
    Invalid(String),
    /// The user cancelled the operation.
    #[error("cancelled")]
    Cancelled,
    /// A background task failed unexpectedly.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Serializable error shape for IPC.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ErrorPayload {
    /// Machine-readable category.
    pub kind: String,
    /// Human-readable message.
    pub message: String,
}

impl AppError {
    /// Machine-readable category.
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::NoProject => "noProject",
            AppError::Config(_) => "config",
            AppError::Storage(chosproxy_storage::StorageError::Encrypted) => "encrypted",
            AppError::Storage(chosproxy_storage::StorageError::WrongKey) => "wrongPassword",
            AppError::Storage(chosproxy_storage::StorageError::ReadOnly) => "readOnly",
            AppError::Storage(_) => "storage",
            AppError::Proxy(_) => "proxy",
            AppError::Tls(_) => "tls",
            AppError::UnknownListener(_) => "unknownListener",
            AppError::UnknownExchange(_) => "unknownExchange",
            AppError::Io(_) => "io",
            AppError::Invalid(_) => "invalid",
            AppError::Cancelled => "cancelled",
            AppError::Internal(_) => "internal",
        }
    }

    /// IPC payload.
    pub fn payload(&self) -> ErrorPayload {
        ErrorPayload {
            kind: self.kind().to_string(),
            message: self.to_string(),
        }
    }
}

impl From<tokio::task::JoinError> for AppError {
    fn from(e: tokio::task::JoinError) -> Self {
        AppError::Internal(e.to_string())
    }
}
