//! Client-side TLS towards origin servers.

use crate::{TlsError, provider};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use std::sync::Arc;

/// Whether an upstream certificate was verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamVerification {
    /// Chain and host name verified against trusted roots.
    Verified,
    /// Verification skipped because the user listed this host as an
    /// exception (e.g. a local lab with a self-signed certificate).
    NotVerified,
}

/// TLS client configurations for upstream connections.
#[derive(Clone)]
pub struct UpstreamTls {
    verified: Arc<ClientConfig>,
    unverified: Arc<ClientConfig>,
    /// The same, offering HTTP/2 (`h2`) first through ALPN.
    verified_h2: Arc<ClientConfig>,
    unverified_h2: Arc<ClientConfig>,
}

impl std::fmt::Debug for UpstreamTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpstreamTls").finish_non_exhaustive()
    }
}

/// Accepts any certificate chain but still checks handshake signatures, so
/// the peer must hold the key of the certificate it presents.
#[derive(Debug)]
struct SkipChainVerification(Arc<CryptoProvider>);

impl ServerCertVerifier for SkipChainVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

impl UpstreamTls {
    /// Trusts the operating system roots plus the Mozilla root program, and
    /// any `extra_roots` (e.g. a lab CA in tests).
    pub fn new(extra_roots: &[CertificateDer<'static>]) -> Result<UpstreamTls, TlsError> {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let native = rustls_native_certs::load_native_certs();
        let (added, ignored) = roots.add_parsable_certificates(native.certs);
        tracing::debug!(
            added,
            ignored,
            errors = native.errors.len(),
            "loaded system TLS roots"
        );
        for r in extra_roots {
            roots.add(r.clone())?;
        }
        let provider = provider();
        let build = |verifier: Option<Arc<dyn ServerCertVerifier>>,
                     alpn: &[&[u8]]|
         -> Result<Arc<ClientConfig>, TlsError> {
            let builder = ClientConfig::builder_with_provider(provider.clone())
                .with_safe_default_protocol_versions()?;
            let mut config = match verifier {
                Some(v) => builder
                    .dangerous()
                    .with_custom_certificate_verifier(v)
                    .with_no_client_auth(),
                None => builder
                    .with_root_certificates(roots.clone())
                    .with_no_client_auth(),
            };
            config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
            Ok(Arc::new(config))
        };
        let skip = || -> Option<Arc<dyn ServerCertVerifier>> {
            Some(Arc::new(SkipChainVerification(provider.clone())))
        };
        Ok(UpstreamTls {
            verified: build(None, &[b"http/1.1"])?,
            unverified: build(skip(), &[b"http/1.1"])?,
            // HTTP/1.1 is offered too so a server without HTTP/2 completes
            // the handshake and the caller can see that it chose HTTP/1.1.
            verified_h2: build(None, &[b"h2", b"http/1.1"])?,
            unverified_h2: build(skip(), &[b"h2", b"http/1.1"])?,
        })
    }

    /// Connector for the given verification policy.
    pub fn connector(&self, verification: UpstreamVerification) -> tokio_rustls::TlsConnector {
        tokio_rustls::TlsConnector::from(match verification {
            UpstreamVerification::Verified => self.verified.clone(),
            UpstreamVerification::NotVerified => self.unverified.clone(),
        })
    }

    /// Connector that offers HTTP/2 (`h2`) first through ALPN.
    pub fn connector_h2(&self, verification: UpstreamVerification) -> tokio_rustls::TlsConnector {
        tokio_rustls::TlsConnector::from(match verification {
            UpstreamVerification::Verified => self.verified_h2.clone(),
            UpstreamVerification::NotVerified => self.unverified_h2.clone(),
        })
    }
}
