//! Owns the local CA for the engine: loading at startup, regeneration,
//! export, and the TLS context handed to the proxy.

use crate::AppError;
use chosproxy_proxy::TlsContext;
use chosproxy_tls::{CaInfo, CaStore, Interceptor, UpstreamTls};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use ts_rs::TS;

/// URL (through the proxy) where the CA certificate can be downloaded.
pub const CA_DOWNLOAD_URL: &str = "http://chosproxy/cert";

/// CA state shown to the user.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CaStatus {
    /// HTTPS interception is possible.
    pub available: bool,
    /// The CA was generated during this run (first start or regeneration).
    pub generated_this_session: bool,
    /// Certificate details.
    pub info: Option<CaInfo>,
    /// Why the CA is unavailable.
    pub error: Option<String>,
    /// Download URL for browsers using the proxy.
    pub download_url: String,
}

/// Export format of the public CA certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum CertFormat {
    /// PEM text (`.pem`): Firefox, Linux trust stores, most tools.
    Pem,
    /// Binary DER (`.crt`/`.der`): Windows, Android, some browsers.
    Der,
}

enum State {
    Ready(Arc<Interceptor>),
    Unavailable(String),
}

pub(crate) struct CaManager {
    store: CaStore,
    upstream: Result<UpstreamTls, String>,
    state: RwLock<State>,
    generated: RwLock<bool>,
}

fn interceptor_from(store: &CaStore) -> Result<(Arc<Interceptor>, bool), String> {
    let (ca, created) = store.load_or_create().map_err(|e| e.to_string())?;
    let interceptor = Interceptor::new(Arc::new(ca)).map_err(|e| e.to_string())?;
    Ok((Arc::new(interceptor), created))
}

impl CaManager {
    /// Loads (or creates on first run) the CA in `dir`. Failure leaves HTTPS
    /// interception disabled and is reported; files are never overwritten.
    pub(crate) fn init(dir: &Path) -> CaManager {
        let store = CaStore::new(dir);
        let upstream = UpstreamTls::new(&[]).map_err(|e| e.to_string());
        let (state, generated) = match interceptor_from(&store) {
            Ok((i, created)) => (State::Ready(i), created),
            Err(e) => {
                tracing::error!(error = %e, "local CA unavailable; HTTPS will not be intercepted");
                (State::Unavailable(e), false)
            }
        };
        CaManager {
            store,
            upstream,
            state: RwLock::new(state),
            generated: RwLock::new(generated),
        }
    }

    pub(crate) fn interceptor(&self) -> Option<Arc<Interceptor>> {
        match &*self.state.read().ok()? {
            State::Ready(i) => Some(i.clone()),
            State::Unavailable(_) => None,
        }
    }

    /// The interceptor of a listener that does not use the shared
    /// certificate (M31b): its own CA, kept in `listeners/<id>` next to the
    /// shared CA and created the first time, or a certificate from files.
    /// `None` for the shared certificate.
    pub(crate) fn listener_interceptor(
        &self,
        listener: &str,
        certificate: &chosproxy_core::config::ListenerCertificate,
    ) -> Result<Option<Arc<Interceptor>>, String> {
        use chosproxy_core::config::ListenerCertificate as C;
        match certificate {
            C::Shared => Ok(None),
            C::OwnCa => {
                let dir = self.listener_ca_dir(listener);
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                Ok(Some(interceptor_from(&CaStore::new(dir))?.0))
            }
            C::File { cert, key } => {
                let shared = self.interceptor().ok_or("the local CA is not available")?;
                let read = |path: &str, what: &str| {
                    let meta = std::fs::metadata(path)
                        .map_err(|e| format!("the {what} file {path}: {e}"))?;
                    if meta.len() > 1024 * 1024 {
                        return Err(format!("the {what} file {path} is too large"));
                    }
                    std::fs::read_to_string(path)
                        .map_err(|e| format!("the {what} file {path}: {e}"))
                };
                let interceptor = Interceptor::with_certificate(
                    shared.authority_shared(),
                    &read(cert, "certificate")?,
                    &read(key, "key")?,
                )
                .map_err(|e| e.to_string())?;
                Ok(Some(Arc::new(interceptor)))
            }
        }
    }

    /// Where a listener's own CA lives.
    pub(crate) fn listener_ca_dir(&self, listener: &str) -> PathBuf {
        self.store
            .cert_path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
            .join("listeners")
            .join(listener)
    }

    /// Context for the proxy, or `None` when interception is impossible.
    pub(crate) fn tls_context(&self) -> Option<TlsContext> {
        let upstream = self.upstream.as_ref().ok()?.clone();
        Some(TlsContext {
            interceptor: self.interceptor()?,
            upstream,
        })
    }

    pub(crate) fn status(&self) -> CaStatus {
        let generated = self.generated.read().map(|g| *g).unwrap_or(false);
        let (info, mut error) = match self.state.read().as_deref() {
            Ok(State::Ready(i)) => (Some(i.authority().info().clone()), None),
            Ok(State::Unavailable(e)) => (None, Some(e.clone())),
            Err(_) => (None, Some("CA state lock poisoned".to_string())),
        };
        if let Err(e) = &self.upstream {
            error.get_or_insert_with(|| format!("upstream TLS unavailable: {e}"));
        }
        CaStatus {
            available: info.is_some() && self.upstream.is_ok(),
            generated_this_session: generated,
            info,
            error,
            download_url: CA_DOWNLOAD_URL.to_string(),
        }
    }

    /// Replaces the CA with one read from two PEM files.
    pub(crate) fn import(&self, cert: &Path, key: &Path) -> Result<(), AppError> {
        let read = |p: &Path| {
            let meta = std::fs::metadata(p)
                .map_err(|e| AppError::Invalid(format!("{}: {e}", p.display())))?;
            if meta.len() > 1024 * 1024 {
                return Err(AppError::Invalid(format!("{} is too large", p.display())));
            }
            std::fs::read_to_string(p)
                .map_err(|e| AppError::Invalid(format!("{}: {e}", p.display())))
        };
        let ca = self.store.import(&read(cert)?, &read(key)?)?;
        let interceptor = Interceptor::new(Arc::new(ca))?;
        if let Ok(mut s) = self.state.write() {
            *s = State::Ready(Arc::new(interceptor));
        }
        Ok(())
    }

    /// Replaces the CA with a new one. Browsers must import it again.
    pub(crate) fn regenerate(&self) -> Result<(), AppError> {
        let ca = self.store.regenerate()?;
        let interceptor = Interceptor::new(Arc::new(ca))?;
        if let Ok(mut s) = self.state.write() {
            *s = State::Ready(Arc::new(interceptor));
        }
        if let Ok(mut g) = self.generated.write() {
            *g = true;
        }
        Ok(())
    }

    /// Writes the public certificate (never the key) to `path`, adding the
    /// conventional extension if missing. Returns the written path.
    pub(crate) fn export(&self, path: &Path, format: CertFormat) -> Result<PathBuf, AppError> {
        let interceptor = self.interceptor().ok_or_else(|| {
            AppError::Invalid("the local CA is not available; see Proxy → HTTPS".into())
        })?;
        let ca = interceptor.authority();
        let (bytes, ext): (&[u8], &str) = match format {
            CertFormat::Pem => (ca.cert_pem().as_bytes(), "pem"),
            CertFormat::Der => (ca.cert_der().as_ref(), "crt"),
        };
        let path = match path.extension() {
            Some(_) => path.to_path_buf(),
            None => path.with_extension(ext),
        };
        std::fs::write(&path, bytes)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_exports_and_regenerates() {
        let dir = tempfile::tempdir().unwrap();
        let m = CaManager::init(&dir.path().join("ca"));
        let s = m.status();
        assert!(s.available, "{:?}", s.error);
        assert!(s.generated_this_session);
        assert!(m.tls_context().is_some());

        let pem = m.export(&dir.path().join("out"), CertFormat::Pem).unwrap();
        assert!(pem.ends_with("out.pem"));
        let text = std::fs::read_to_string(&pem).unwrap();
        assert!(text.contains("BEGIN CERTIFICATE") && !text.contains("PRIVATE"));
        let der = m
            .export(&dir.path().join("c.der"), CertFormat::Der)
            .unwrap();
        assert!(der.ends_with("c.der"));
        assert_eq!(
            std::fs::read(der).unwrap()[0],
            0x30,
            "DER starts with a SEQUENCE"
        );

        let before = s.info.unwrap().fingerprint_sha256;
        m.regenerate().unwrap();
        assert_ne!(m.status().info.unwrap().fingerprint_sha256, before);

        // Reloading the same directory keeps the regenerated CA.
        let again = CaManager::init(&dir.path().join("ca"));
        assert!(!again.status().generated_this_session);
    }

    #[test]
    fn broken_ca_disables_interception_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let ca_dir = dir.path().join("ca");
        CaManager::init(&ca_dir);
        std::fs::write(ca_dir.join("ca-key.pem"), "garbage").unwrap();
        let m = CaManager::init(&ca_dir);
        let s = m.status();
        assert!(!s.available);
        assert!(s.error.unwrap().contains("unusable"));
        assert!(m.tls_context().is_none());
        assert_eq!(
            std::fs::read_to_string(ca_dir.join("ca-key.pem")).unwrap(),
            "garbage"
        );
        // Explicit regeneration recovers.
        m.regenerate().unwrap();
        assert!(m.status().available);
    }
}
