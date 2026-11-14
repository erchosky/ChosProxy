//! TLS for authorized interception (see ADR-0011).
//!
//! * [`CaStore`] — the per-installation local certificate authority: generated
//!   on first use, private key stored `0600`, never exported or transmitted.
//! * [`Interceptor`] — issues short-lived per-host certificates signed by the
//!   local CA and builds TLS configurations for both sides of an
//!   intercepted connection.
//! * Upstream connections verify server certificates against the system and
//!   Mozilla roots; hosts explicitly listed by the user may skip verification
//!   (typical for local labs with self-signed certificates), and that fact is
//!   recorded with every affected exchange.

mod ca;
mod error;
mod leaf;
mod upstream;

pub use ca::{CaInfo, CaStore, CertificateAuthority};
pub use error::TlsError;
pub use leaf::Interceptor;
pub use upstream::{UpstreamTls, UpstreamVerification};

/// The cryptographic provider used everywhere (no process-global state).
pub(crate) fn provider() -> std::sync::Arc<rustls::crypto::CryptoProvider> {
    std::sync::Arc::new(rustls::crypto::ring::default_provider())
}

/// Lowercase hex SHA-256 of `data`, bytes separated by `:`.
pub(crate) fn sha256_fingerprint(data: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, data)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// 16 random bytes with the top bit cleared: a positive, unique serial.
pub(crate) fn random_serial() -> Result<rcgen::SerialNumber, TlsError> {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| TlsError::Random)?;
    bytes[0] &= 0x7f;
    Ok(rcgen::SerialNumber::from_slice(&bytes))
}
