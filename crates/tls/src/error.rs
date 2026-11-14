/// TLS and certificate errors.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    /// Filesystem error reading or writing CA files.
    #[error("cannot access {path}: {source}")]
    Io {
        /// File involved.
        path: std::path::PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// Certificate or key generation failed.
    #[error("certificate generation failed: {0}")]
    Generate(#[from] rcgen::Error),
    /// The stored CA is unreadable or inconsistent. It is never overwritten
    /// automatically; the user must regenerate it explicitly.
    #[error("stored CA is unusable: {0}")]
    InvalidCa(String),
    /// A host name cannot be put in a certificate.
    #[error("invalid host name for a certificate: {0:?}")]
    InvalidHost(String),
    /// rustls configuration error.
    #[error("TLS configuration error: {0}")]
    Config(#[from] rustls::Error),
    /// The system random number generator failed.
    #[error("system random number generator failed")]
    Random,
}
