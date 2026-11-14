//! The credential keyring in the engine (M24): set, list (without
//! secrets) and delete credentials; other parts read secrets by id.

use super::Engine;
use super::lock;
use crate::AppError;
use crate::keyring::{
    Credential, CredentialInfo, Keyring, KeyringProtection, KeyringStatus, MAX_CREDENTIALS,
    validate,
};

impl Engine {
    fn keyring(&self) -> Keyring {
        let os = super::read(&self.inner.os_keychain).clone();
        let key = *lock(&self.inner.keyring_key);
        Keyring::with(self.inner.paths.data_dir.join("keyring"), os, key)
    }

    /// Uses a keychain in memory instead of the operating system's (tests
    /// must not touch the tester's real keychain).
    #[doc(hidden)]
    pub fn use_memory_keychain(&self) {
        if let Ok(mut os) = self.inner.os_keychain.write() {
            *os = std::sync::Arc::new(crate::keyring::MemoryKeychain::default());
        }
    }

    /// What protects the keyring, and whether it waits for its password.
    pub async fn keyring_status(&self) -> Result<KeyringStatus, AppError> {
        let k = self.keyring();
        tokio::task::spawn_blocking(move || k.status())
            .await?
            .map_err(AppError::Invalid)
    }

    /// Opens a keyring protected by a master password for this session.
    /// The password is not kept: only the key it gives, in memory.
    pub async fn keyring_unlock(&self, password: String) -> Result<KeyringStatus, AppError> {
        let k = self.keyring();
        let key = tokio::task::spawn_blocking(move || k.unlock(&password))
            .await?
            .map_err(AppError::Invalid)?;
        *lock(&self.inner.keyring_key) = Some(key);
        self.audit("keyring.unlocked", "the keyring was unlocked");
        self.reload_platform_auth();
        self.keyring_status().await
    }

    /// Forgets the master password's key: the keyring is locked again.
    pub async fn keyring_lock(&self) -> Result<KeyringStatus, AppError> {
        *lock(&self.inner.keyring_key) = None;
        self.reload_platform_auth();
        self.keyring_status().await
    }

    /// Changes what protects the keyring key: a private file (the
    /// default), the operating system's keychain, or a master password
    /// (given here, at least 8 characters, never stored). The credentials
    /// are kept; a locked keyring must be unlocked first.
    pub async fn keyring_set_protection(
        &self,
        protection: KeyringProtection,
        password: Option<String>,
    ) -> Result<KeyringStatus, AppError> {
        let k = self.keyring();
        let key =
            tokio::task::spawn_blocking(move || k.set_protection(protection, password.as_deref()))
                .await?
                .map_err(AppError::Invalid)?;
        *lock(&self.inner.keyring_key) = key;
        self.audit(
            "keyring.protection",
            match protection {
                KeyringProtection::File => "the keyring key is in a private file",
                KeyringProtection::OsKeychain => {
                    "the keyring key is in the operating system's keychain"
                }
                KeyringProtection::Password => "the keyring is protected by a master password",
            },
        );
        self.keyring_status().await
    }

    /// Stored credentials, without their secrets.
    pub async fn credentials(&self) -> Result<Vec<CredentialInfo>, AppError> {
        let k = self.keyring();
        let all = tokio::task::spawn_blocking(move || k.load())
            .await?
            .map_err(AppError::Invalid)?;
        Ok(all.iter().map(Credential::info).collect())
    }

    /// Adds or changes a credential. `secret: None` keeps the stored one.
    pub async fn credential_set(
        &self,
        info: CredentialInfo,
        secret: Option<String>,
    ) -> Result<Vec<CredentialInfo>, AppError> {
        let k = self.keyring();
        let id = info.id.clone();
        let all = tokio::task::spawn_blocking(move || {
            let mut all = k.load()?;
            let previous = all
                .iter()
                .find(|c| c.id == info.id)
                .map(|c| c.secret.clone());
            let entry = Credential {
                id: info.id.clone(),
                label: info.label.trim().to_string(),
                username: info.username,
                secret: secret.or(previous).unwrap_or_default(),
            };
            validate(&entry)?;
            all.retain(|c| c.id != entry.id);
            all.push(entry);
            if all.len() > MAX_CREDENTIALS {
                return Err(format!("at most {MAX_CREDENTIALS} credentials"));
            }
            k.save(&all)?;
            Ok(all)
        })
        .await?
        .map_err(AppError::Invalid)?;
        self.audit("keyring.changed", &format!("credential {id} saved"));
        self.reload_platform_auth();
        Ok(all.iter().map(Credential::info).collect())
    }

    /// Deletes a credential.
    pub async fn credential_delete(&self, id: &str) -> Result<Vec<CredentialInfo>, AppError> {
        let k = self.keyring();
        let target = id.to_string();
        let all = tokio::task::spawn_blocking(move || {
            let mut all = k.load()?;
            all.retain(|c| c.id != target);
            k.save(&all)?;
            Ok::<_, String>(all)
        })
        .await?
        .map_err(AppError::Invalid)?;
        self.audit("keyring.changed", &format!("credential {id} deleted"));
        self.reload_platform_auth();
        Ok(all.iter().map(Credential::info).collect())
    }

    /// Resolves the platform authentication rules of the network settings
    /// against the keyring and hands them to the Request Engine.
    /// The secret of one credential (for internal use, never shown).
    pub(crate) fn credential_secret(&self, id: &str) -> Result<Option<String>, AppError> {
        Ok(self
            .keyring()
            .load()
            .map_err(AppError::Invalid)?
            .into_iter()
            .find(|c| c.id == id)
            .map(|c| c.secret))
    }

    pub(crate) fn reload_platform_auth(&self) {
        let rules = self.network_config().auth;
        let creds = if rules.iter().any(|r| r.enabled) {
            match self.keyring().load() {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, "keyring not readable; platform authentication off");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        let resolved = rules
            .iter()
            .filter(|r| r.enabled)
            .filter_map(|r| {
                let c = creds.iter().find(|c| c.id == r.credential)?;
                Some(chosproxy_request::PlatformCredential {
                    host: r.host.clone(),
                    scheme: r.scheme,
                    username: c.username.clone(),
                    password: c.secret.clone(),
                })
            })
            .collect();
        chosproxy_request::set_platform_auth(resolved);
    }
}
