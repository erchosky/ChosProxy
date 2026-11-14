//! The per-installation local certificate authority.

use crate::{TlsError, random_serial, sha256_fingerprint};
use chosproxy_core::brand;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, PublicKeyData,
};
use rustls::pki_types::CertificateDer;
use serde::Serialize;
use std::path::{Path, PathBuf};
use time::{Duration, OffsetDateTime};
use ts_rs::TS;

/// How long a newly generated CA is valid.
const CA_LIFETIME: Duration = Duration::days(5 * 365);
const CERT_FILE: &str = "ca-cert.pem";
const KEY_FILE: &str = "ca-key.pem";

/// Public, displayable facts about the CA. Contains no secret material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CaInfo {
    /// Subject common name.
    pub subject: String,
    /// SHA-256 fingerprint of the DER certificate (`AA:BB:…`).
    pub fingerprint_sha256: String,
    /// Validity start, Unix epoch ms.
    #[ts(type = "number")]
    pub not_before: i64,
    /// Validity end, Unix epoch ms.
    #[ts(type = "number")]
    pub not_after: i64,
    /// Where the public certificate is stored.
    pub cert_path: String,
    /// Where the private key is stored (shown so users know what to protect;
    /// the key itself is never exported).
    pub key_path: String,
}

/// A loaded CA: certificate plus the issuer used to sign leaf certificates.
pub struct CertificateAuthority {
    cert_der: CertificateDer<'static>,
    cert_pem: String,
    issuer: Issuer<'static, KeyPair>,
    info: CaInfo,
}

impl std::fmt::Debug for CertificateAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material.
        f.debug_struct("CertificateAuthority")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl CertificateAuthority {
    /// Public certificate, DER.
    pub fn cert_der(&self) -> &CertificateDer<'static> {
        &self.cert_der
    }

    /// Public certificate, PEM.
    pub fn cert_pem(&self) -> &str {
        &self.cert_pem
    }

    /// Displayable metadata.
    pub fn info(&self) -> &CaInfo {
        &self.info
    }

    pub(crate) fn issuer(&self) -> &Issuer<'static, KeyPair> {
        &self.issuer
    }

    /// Validity end of the CA.
    pub(crate) fn not_after(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(self.info.not_after / 1000)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH)
    }

    /// Generates a fresh CA key pair and self-signed certificate.
    fn generate() -> Result<(String, String), TlsError> {
        let key = KeyPair::generate()?; // ECDSA P-256
        let mut params = CertificateParams::default();
        let label = sha256_fingerprint(&key.subject_public_key_info())
            .replace(':', "")
            .chars()
            .take(8)
            .collect::<String>();
        let mut dn = DistinguishedName::new();
        dn.push(
            DnType::CommonName,
            format!("{} Local CA {label}", brand().product_name),
        );
        dn.push(DnType::OrganizationName, brand().product_name.clone());
        dn.push(
            DnType::OrganizationalUnitName,
            "Generated locally for authorized testing - never shared",
        );
        params.distinguished_name = dn;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::days(1);
        params.not_after = now + CA_LIFETIME;
        params.serial_number = Some(random_serial()?);
        let cert = params.self_signed(&key)?;
        Ok((cert.pem(), key.serialize_pem()))
    }

    /// Parses and validates stored PEM files.
    fn from_pem(cert_pem: &str, key_pem: &str, store: &CaStore) -> Result<Self, TlsError> {
        let invalid = |m: &str| TlsError::InvalidCa(m.to_string());
        let key = KeyPair::from_pem(key_pem).map_err(|e| invalid(&format!("private key: {e}")))?;
        let pem = rustls::pki_types::pem::PemObject::from_pem_slice(cert_pem.as_bytes())
            .map_err(|e| invalid(&format!("certificate: {e:?}")))?;
        let cert_der: CertificateDer<'static> = pem;
        let (_, x509) = x509_parser::parse_x509_certificate(&cert_der)
            .map_err(|e| invalid(&format!("certificate: {e}")))?;
        if !x509.is_ca() {
            return Err(invalid("certificate is not a CA"));
        }
        if x509.public_key().raw != key.subject_public_key_info().as_slice() {
            return Err(invalid("private key does not match the certificate"));
        }
        let validity = x509.validity();
        let now = OffsetDateTime::now_utc().unix_timestamp();
        if validity.not_after.timestamp() <= now {
            return Err(invalid("certificate has expired; regenerate the CA"));
        }
        let subject = x509
            .subject()
            .iter_common_name()
            .next()
            .and_then(|cn| cn.as_str().ok())
            .unwrap_or("")
            .to_string();
        let info = CaInfo {
            subject,
            fingerprint_sha256: sha256_fingerprint(&cert_der),
            not_before: validity.not_before.timestamp() * 1000,
            not_after: validity.not_after.timestamp() * 1000,
            cert_path: store.cert_path().display().to_string(),
            key_path: store.key_path().display().to_string(),
        };
        let issuer = Issuer::from_ca_cert_der(&cert_der, key)
            .map_err(|e| invalid(&format!("issuer: {e}")))?;
        Ok(CertificateAuthority {
            cert_der,
            cert_pem: cert_pem.to_string(),
            issuer,
            info,
        })
    }
}

/// Where the CA lives on disk (`<data dir>/ca/`, directory `0700`, key `0600`).
#[derive(Debug, Clone)]
pub struct CaStore {
    dir: PathBuf,
}

impl CaStore {
    /// A store rooted at `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> CaStore {
        CaStore { dir: dir.into() }
    }

    /// Path of the public certificate.
    pub fn cert_path(&self) -> PathBuf {
        self.dir.join(CERT_FILE)
    }

    /// Path of the private key.
    pub fn key_path(&self) -> PathBuf {
        self.dir.join(KEY_FILE)
    }

    fn io(path: &Path) -> impl FnOnce(std::io::Error) -> TlsError + '_ {
        move |source| TlsError::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    /// Loads the CA. `Ok(None)` if no CA exists yet. A partial or invalid CA
    /// is an error and is left untouched on disk.
    pub fn load(&self) -> Result<Option<CertificateAuthority>, TlsError> {
        let (cert, key) = (self.cert_path(), self.key_path());
        match (cert.exists(), key.exists()) {
            (false, false) => Ok(None),
            (true, true) => {
                let cert_pem = std::fs::read_to_string(&cert).map_err(Self::io(&cert))?;
                let key_pem = std::fs::read_to_string(&key).map_err(Self::io(&key))?;
                CertificateAuthority::from_pem(&cert_pem, &key_pem, self).map(Some)
            }
            _ => Err(TlsError::InvalidCa(
                "certificate or private key file is missing".into(),
            )),
        }
    }

    /// Loads the CA, generating one if none exists. Returns whether it was
    /// created now.
    pub fn load_or_create(&self) -> Result<(CertificateAuthority, bool), TlsError> {
        match self.load()? {
            Some(ca) => Ok((ca, false)),
            None => Ok((self.regenerate()?, true)),
        }
    }

    /// Replaces the CA with one you provide (a CA certificate and its
    /// PKCS#8 private key, both PEM), after checking that the certificate
    /// is a CA, has not expired and matches the key. The previous CA is kept
    /// next to it as `*.previous.pem`.
    pub fn import(&self, cert_pem: &str, key_pem: &str) -> Result<CertificateAuthority, TlsError> {
        if key_pem.contains("ENCRYPTED PRIVATE KEY") {
            return Err(TlsError::InvalidCa(
                "the private key is encrypted; export it without a passphrase (PKCS#8)".into(),
            ));
        }
        if key_pem.contains("BEGIN RSA PRIVATE KEY") || key_pem.contains("BEGIN EC PRIVATE KEY") {
            return Err(TlsError::InvalidCa(
                "the private key must be PKCS#8 (\"BEGIN PRIVATE KEY\"); convert it with \
                 `openssl pkcs8 -topk8 -nocrypt`"
                    .into(),
            ));
        }
        let ca = CertificateAuthority::from_pem(cert_pem, key_pem, self)?;
        chosproxy_core::fs::create_private_dir(&self.dir).map_err(Self::io(&self.dir))?;
        for (path, backup) in [
            (self.key_path(), self.dir.join("ca-key.previous.pem")),
            (self.cert_path(), self.dir.join("ca-cert.previous.pem")),
        ] {
            if path.exists() {
                let old = std::fs::read(&path).map_err(Self::io(&path))?;
                chosproxy_core::fs::write_private_atomic(&backup, &old)
                    .map_err(Self::io(&backup))?;
            }
        }
        let (key_path, cert_path) = (self.key_path(), self.cert_path());
        chosproxy_core::fs::write_private_atomic(&key_path, key_pem.as_bytes())
            .map_err(Self::io(&key_path))?;
        chosproxy_core::fs::write_private_atomic(&cert_path, cert_pem.as_bytes())
            .map_err(Self::io(&cert_path))?;
        tracing::info!(subject = %ca.info().subject, "imported a CA");
        Ok(ca)
    }

    /// Generates a new CA, replacing any existing one. Browsers that trusted
    /// the previous CA must import the new certificate.
    pub fn regenerate(&self) -> Result<CertificateAuthority, TlsError> {
        chosproxy_core::fs::create_private_dir(&self.dir).map_err(Self::io(&self.dir))?;
        let (cert_pem, key_pem) = CertificateAuthority::generate()?;
        let key_path = self.key_path();
        let cert_path = self.cert_path();
        chosproxy_core::fs::write_private_atomic(&key_path, key_pem.as_bytes())
            .map_err(Self::io(&key_path))?;
        chosproxy_core::fs::write_private_atomic(&cert_path, cert_pem.as_bytes())
            .map_err(Self::io(&cert_path))?;
        tracing::info!(path = %cert_path.display(), "generated local CA");
        CertificateAuthority::from_pem(&cert_pem, &key_pem, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_a_valid_ca_and_refuses_others() {
        let dir = tempfile::tempdir().unwrap();
        let store = CaStore::new(dir.path().join("ca"));
        let (old, _) = store.load_or_create().unwrap();
        // A CA made elsewhere (here: another store).
        let other = CaStore::new(dir.path().join("other"));
        let (theirs, _) = other.load_or_create().unwrap();
        let key = std::fs::read_to_string(other.key_path()).unwrap();
        let imported = store.import(theirs.cert_pem(), &key).unwrap();
        assert_eq!(
            imported.info().fingerprint_sha256,
            theirs.info().fingerprint_sha256
        );
        let reloaded = store.load().unwrap().unwrap();
        assert_eq!(
            reloaded.info().fingerprint_sha256,
            theirs.info().fingerprint_sha256
        );
        let backup = std::fs::read_to_string(dir.path().join("ca/ca-cert.previous.pem")).unwrap();
        assert_eq!(backup, old.cert_pem());

        // Wrong key, not a CA, other key formats: refused, nothing changed.
        let own_key = std::fs::read_to_string(dir.path().join("ca/ca-cert.previous.pem")).unwrap();
        assert!(store.import(theirs.cert_pem(), &own_key).is_err());
        let wrong = CaStore::new(dir.path().join("wrong"));
        wrong.load_or_create().unwrap();
        let wrong_key = std::fs::read_to_string(wrong.key_path()).unwrap();
        let err = store.import(theirs.cert_pem(), &wrong_key).unwrap_err();
        assert!(err.to_string().contains("does not match"), "{err}");
        let leaf_key = KeyPair::generate().unwrap();
        let leaf = CertificateParams::new(vec!["lab.test".to_string()])
            .unwrap()
            .self_signed(&leaf_key)
            .unwrap();
        let err = store
            .import(&leaf.pem(), &leaf_key.serialize_pem())
            .unwrap_err();
        assert!(err.to_string().contains("not a CA"), "{err}");
        assert!(
            store
                .import(theirs.cert_pem(), "-----BEGIN RSA PRIVATE KEY-----")
                .is_err()
        );
        assert_eq!(
            store.load().unwrap().unwrap().info().fingerprint_sha256,
            theirs.info().fingerprint_sha256
        );
    }

    #[test]
    fn creates_once_then_loads_same_ca() {
        let dir = tempfile::tempdir().unwrap();
        let store = CaStore::new(dir.path().join("ca"));
        assert!(store.load().unwrap().is_none());
        let (a, created) = store.load_or_create().unwrap();
        assert!(created);
        let (b, created) = store.load_or_create().unwrap();
        assert!(!created);
        assert_eq!(a.info(), b.info());
        assert!(a.info().subject.contains("Local CA"));
        assert!(a.cert_pem().starts_with("-----BEGIN CERTIFICATE-----"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(store.key_path()), 0o600);
            assert_eq!(mode(dir.path().join("ca")), 0o700);
        }
    }

    #[test]
    fn regenerate_changes_identity() {
        let dir = tempfile::tempdir().unwrap();
        let store = CaStore::new(dir.path());
        let (a, _) = store.load_or_create().unwrap();
        let b = store.regenerate().unwrap();
        assert_ne!(a.info().fingerprint_sha256, b.info().fingerprint_sha256);
        assert_eq!(store.load().unwrap().unwrap().info(), b.info());
    }

    #[test]
    fn refuses_partial_or_mismatched_ca_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let store = CaStore::new(dir.path());
        store.load_or_create().unwrap();
        let original_cert = std::fs::read(store.cert_path()).unwrap();

        // Key from another CA.
        let other = CaStore::new(dir.path().join("other"));
        other.load_or_create().unwrap();
        std::fs::copy(other.key_path(), store.key_path()).unwrap();
        assert!(matches!(store.load(), Err(TlsError::InvalidCa(_))));
        assert!(matches!(
            store.load_or_create(),
            Err(TlsError::InvalidCa(_))
        ));
        assert_eq!(std::fs::read(store.cert_path()).unwrap(), original_cert);

        std::fs::remove_file(store.key_path()).unwrap();
        assert!(matches!(store.load(), Err(TlsError::InvalidCa(_))));
    }

    #[test]
    fn debug_output_has_no_key_material() {
        let dir = tempfile::tempdir().unwrap();
        let (ca, _) = CaStore::new(dir.path()).load_or_create().unwrap();
        let printed = format!("{ca:?}");
        assert!(!printed.contains("PRIVATE KEY"));
    }
}
