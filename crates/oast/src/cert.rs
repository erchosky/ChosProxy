//! The self-signed certificate a self-hosted server presents (for its API
//! and for HTTPS interactions). Clients pin it by fingerprint.

use crate::api::fingerprint;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use std::sync::Arc;

/// A certificate and its private key.
#[derive(Clone)]
pub struct SelfSigned {
    /// DER certificate.
    pub cert_der: Vec<u8>,
    /// PKCS#8 DER private key.
    pub key_der: Vec<u8>,
    /// SHA-256 fingerprint of the certificate (`aa:bb:…`).
    pub fingerprint: String,
}

impl std::fmt::Debug for SelfSigned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelfSigned")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

impl SelfSigned {
    /// A new certificate for `name` (and `*.name`), valid for ten years.
    pub fn generate(name: &str) -> Result<SelfSigned, rcgen::Error> {
        let mut params = CertificateParams::new(vec![name.to_string(), format!("*.{name}")])?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, format!("ChosOAST {name}"));
        params.distinguished_name = dn;
        let now = rcgen::date_time_ymd(2026, 1, 1);
        params.not_before = now;
        params.not_after = rcgen::date_time_ymd(2036, 1, 1);
        let key = KeyPair::generate()?;
        let cert = params.self_signed(&key)?;
        let cert_der = cert.der().to_vec();
        Ok(SelfSigned {
            fingerprint: fingerprint(&cert_der),
            cert_der,
            key_der: key.serialize_der(),
        })
    }

    /// A certificate and key loaded from storage.
    pub fn from_parts(cert_der: Vec<u8>, key_der: Vec<u8>) -> SelfSigned {
        SelfSigned {
            fingerprint: fingerprint(&cert_der),
            cert_der,
            key_der,
        }
    }

    /// TLS server configuration presenting this certificate.
    pub fn server_config(&self) -> Result<Arc<rustls::ServerConfig>, rustls::Error> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(self.key_der.clone().into());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(vec![self.cert_der.clone().into()], key)?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}
