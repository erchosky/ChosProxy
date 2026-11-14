//! Per-host leaf certificates and server-side TLS for interception.

use crate::{CertificateAuthority, TlsError, provider, random_serial};
use base64::Engine as _;
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, KeyPair,
    KeyUsagePurpose, PublicKeyData,
};
use rustls::ServerConfig;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use time::{Duration, OffsetDateTime};

/// Leaf certificates are short-lived (browsers cap publicly trusted
/// certificates well below a year; we stay far below that).
const LEAF_LIFETIME: Duration = Duration::days(90);
/// Cached host certificates before the cache is reset.
const CACHE_LIMIT: usize = 2048;

/// Issues certificates for intercepted hosts and builds server TLS configs.
///
/// One leaf key pair is generated per process and shared by all host
/// certificates (only certificates differ per host). It lives in memory only.
pub struct Interceptor {
    ca: Arc<CertificateAuthority>,
    leaf_key: KeyPair,
    signing_key: Arc<dyn rustls::sign::SigningKey>,
    leaf_spki_pin: String,
    provider: Arc<CryptoProvider>,
    cache: Mutex<HashMap<String, Arc<CertifiedKey>>>,
    /// One certificate for every host, instead of one issued per host.
    fixed: Option<Arc<CertifiedKey>>,
}

impl std::fmt::Debug for Interceptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Interceptor")
            .field("ca", &self.ca.info().subject)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct FixedCert(Arc<CertifiedKey>);

impl ResolvesServerCert for FixedCert {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }
}

/// Normalizes and validates a host for use in a certificate.
fn normalize_host(host: &str) -> Result<String, TlsError> {
    let h = host
        .trim()
        .trim_end_matches('.')
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    let ok = !h.is_empty()
        && h.len() <= 253
        && h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':'));
    if ok {
        Ok(h)
    } else {
        Err(TlsError::InvalidHost(host.to_string()))
    }
}

impl Interceptor {
    /// Creates an interceptor signing with `ca`.
    pub fn new(ca: Arc<CertificateAuthority>) -> Result<Interceptor, TlsError> {
        let provider = provider();
        let leaf_key = KeyPair::generate()?;
        let der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
        let signing_key = provider.key_provider.load_private_key(der)?;
        let spki = leaf_key.subject_public_key_info();
        let digest = ring::digest::digest(&ring::digest::SHA256, &spki);
        let leaf_spki_pin = base64::engine::general_purpose::STANDARD.encode(digest.as_ref());
        Ok(Interceptor {
            ca,
            leaf_key,
            signing_key,
            leaf_spki_pin,
            provider,
            cache: Mutex::new(HashMap::new()),
            fixed: None,
        })
    }

    /// Creates an interceptor that presents one chosen certificate (PEM,
    /// with its chain) and private key (PEM) for every host. `ca` is only
    /// what [`Interceptor::authority`] reports; it signs nothing here.
    pub fn with_certificate(
        ca: Arc<CertificateAuthority>,
        cert_pem: &str,
        key_pem: &str,
    ) -> Result<Interceptor, TlsError> {
        use rustls::pki_types::pem::PemObject;
        let invalid = |what: &str| TlsError::InvalidCa(format!("listener certificate: {what}"));
        use rustls::pki_types::CertificateDer;
        let chain: Vec<CertificateDer<'static>> =
            CertificateDer::pem_slice_iter(cert_pem.as_bytes())
                .collect::<Result<_, _>>()
                .map_err(|e| invalid(&format!("the certificate file cannot be read ({e:?})")))?;
        if chain.is_empty() {
            return Err(invalid("the certificate file has no certificate"));
        }
        let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes())
            .map_err(|e| invalid(&format!("the key file has no private key ({e:?})")))?;
        let mut interceptor = Interceptor::new(ca)?;
        let signing_key = interceptor
            .provider
            .key_provider
            .load_private_key(key)
            .map_err(|e| invalid(&format!("the private key cannot be used ({e})")))?;
        let certified = CertifiedKey::new(chain, signing_key);
        certified
            .keys_match()
            .map_err(|e| invalid(&format!("the key does not belong to the certificate ({e})")))?;
        interceptor.fixed = Some(Arc::new(certified));
        Ok(interceptor)
    }

    /// True when it presents one chosen certificate for every host.
    pub fn is_fixed(&self) -> bool {
        self.fixed.is_some()
    }

    /// The signing CA.
    pub fn authority(&self) -> &CertificateAuthority {
        &self.ca
    }

    /// The signing CA, shared.
    pub fn authority_shared(&self) -> Arc<CertificateAuthority> {
        self.ca.clone()
    }

    /// Base64 SHA-256 of the leaf public key (SPKI). Chromium-based lab
    /// browsers accept interception certificates when given this pin via
    /// `--ignore-certificate-errors-spki-list`, without installing the CA in
    /// any trust store.
    pub fn leaf_spki_pin(&self) -> &str {
        &self.leaf_spki_pin
    }

    /// Certificate and key for `host` (cached).
    pub fn certified_key(&self, host: &str) -> Result<Arc<CertifiedKey>, TlsError> {
        if let Some(fixed) = &self.fixed {
            return Ok(fixed.clone());
        }
        let host = normalize_host(host)?;
        if let Some(ck) = self.cache.lock().ok().and_then(|c| c.get(&host).cloned()) {
            return Ok(ck);
        }
        let mut params = CertificateParams::new(vec![host.clone()])
            .map_err(|_| TlsError::InvalidHost(host.clone()))?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, host.clone());
        params.distinguished_name = dn;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::days(1);
        params.not_after = (now + LEAF_LIFETIME).min(self.ca.not_after());
        params.serial_number = Some(random_serial()?);
        let cert = params.signed_by(&self.leaf_key, self.ca.issuer())?;
        let ck = Arc::new(CertifiedKey::new(
            vec![cert.der().clone()],
            self.signing_key.clone(),
        ));
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= CACHE_LIMIT {
                cache.clear();
            }
            cache.insert(host, ck.clone());
        }
        Ok(ck)
    }

    /// Server TLS configuration presenting a certificate for `host`,
    /// offering HTTP/1.1 via ALPN.
    pub fn server_config(&self, host: &str) -> Result<Arc<ServerConfig>, TlsError> {
        self.server_config_with(host, false)
    }

    /// The same, also offering HTTP/2 (`h2`, preferred) when `http2`.
    pub fn server_config_with(
        &self,
        host: &str,
        http2: bool,
    ) -> Result<Arc<ServerConfig>, TlsError> {
        let ck = self.certified_key(host)?;
        let mut config = ServerConfig::builder_with_provider(self.provider.clone())
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(FixedCert(ck)));
        config.alpn_protocols = if http2 {
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        } else {
            vec![b"http/1.1".to_vec()]
        };
        Ok(Arc::new(config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CaStore;
    use rustls::pki_types::ServerName;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn interceptor(dir: &std::path::Path) -> Interceptor {
        let (ca, _) = CaStore::new(dir).load_or_create().unwrap();
        Interceptor::new(Arc::new(ca)).unwrap()
    }

    fn client_trusting(ca: &CertificateAuthority) -> tokio_rustls::TlsConnector {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(ca.cert_der().clone()).unwrap();
        let config = rustls::ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        tokio_rustls::TlsConnector::from(Arc::new(config))
    }

    async fn handshake(
        i: &Interceptor,
        cert_host: &str,
        client_name: &str,
        client: tokio_rustls::TlsConnector,
    ) -> Result<Vec<u8>, String> {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let acceptor = tokio_rustls::TlsAcceptor::from(i.server_config(cert_host).unwrap());
        let server = tokio::spawn(async move {
            let mut s = acceptor.accept(b).await.map_err(|e| e.to_string())?;
            s.write_all(b"hello").await.map_err(|e| e.to_string())?;
            s.shutdown().await.map_err(|e| e.to_string())
        });
        let name = ServerName::try_from(client_name.to_string()).unwrap();
        let mut c = client.connect(name, a).await.map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        c.read_to_end(&mut buf).await.map_err(|e| e.to_string())?;
        server.await.unwrap()?;
        Ok(buf)
    }

    #[tokio::test]
    async fn issued_certificates_validate_against_the_ca() {
        let dir = tempfile::tempdir().unwrap();
        let i = interceptor(dir.path());
        for host in ["lab.example.test", "127.0.0.1", "::1"] {
            let got = handshake(&i, host, host, client_trusting(i.authority())).await;
            assert_eq!(got.unwrap(), b"hello", "{host}");
        }
    }

    #[tokio::test]
    async fn wrong_host_or_untrusted_ca_fails() {
        let dir = tempfile::tempdir().unwrap();
        let i = interceptor(dir.path());
        let err = handshake(&i, "a.test", "b.test", client_trusting(i.authority())).await;
        assert!(err.is_err());
        let other = interceptor(&dir.path().join("other"));
        let err = handshake(&i, "a.test", "a.test", client_trusting(other.authority())).await;
        assert!(err.is_err());
    }

    #[test]
    fn caches_and_validates_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let i = interceptor(dir.path());
        let a = i.certified_key("Example.TEST.").unwrap();
        let b = i.certified_key("example.test").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        for bad in ["", "a b", "evil\n.test", "x/y"] {
            assert!(i.certified_key(bad).is_err(), "{bad:?}");
        }
        assert_eq!(i.leaf_spki_pin().len(), 44);
    }

    #[test]
    fn a_chosen_certificate_is_presented_for_every_host() {
        let dir = tempfile::tempdir().unwrap();
        let (ca, _) = crate::CaStore::new(dir.path()).load_or_create().unwrap();
        let ca = Arc::new(ca);
        let key = KeyPair::generate().unwrap();
        let cert = CertificateParams::new(vec!["fixed.test".to_string()])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        let fixed =
            Interceptor::with_certificate(ca.clone(), &cert.pem(), &key.serialize_pem()).unwrap();
        assert!(fixed.is_fixed());
        let a = fixed.certified_key("one.example").unwrap();
        let b = fixed.certified_key("two.example").unwrap();
        assert_eq!(a.cert[0], *cert.der());
        assert_eq!(a.cert, b.cert);
        assert!(fixed.server_config_with("anything", true).is_ok());
        // A key that is not the certificate's, and files that are not PEM.
        let other = KeyPair::generate().unwrap();
        let err = Interceptor::with_certificate(ca.clone(), &cert.pem(), &other.serialize_pem())
            .unwrap_err();
        assert!(err.to_string().contains("does not belong"), "{err}");
        assert!(
            Interceptor::with_certificate(ca.clone(), "not pem", &key.serialize_pem()).is_err()
        );
        assert!(Interceptor::with_certificate(ca.clone(), &cert.pem(), "not pem").is_err());
        assert!(!Interceptor::new(ca).unwrap().is_fixed());
    }
}
