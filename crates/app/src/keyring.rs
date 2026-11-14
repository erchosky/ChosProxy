//! The credential keyring (M24, matrix SESSION-007): user names and secrets
//! for platform authentication and login macros, encrypted at rest.
//!
//! Entries live in `credentials.enc` (AES-256-GCM, fresh nonce per write);
//! the key is a random 256-bit value in `credentials.key`. Both files are
//! written with private permissions in the data directory. Secrets are
//! never logged, exported or sent back to the UI: the UI can set or delete
//! them, and the engine reads them only where a rule uses them.

use base64::Engine as _;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

const DATA_FILE: &str = "credentials.enc";
const KEY_FILE: &str = "credentials.key";
/// Most entries.
pub const MAX_CREDENTIALS: usize = 200;

/// A stored credential (with its secret: engine-internal only).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Credential {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) username: String,
    pub(crate) secret: String,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret.
        f.debug_struct("Credential")
            .field("id", &self.id)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

/// A credential as the UI sees it (no secret).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CredentialInfo {
    /// Stable id (`[a-z0-9-]{1,48}`), used by rules.
    pub id: String,
    /// Name shown in lists.
    pub label: String,
    /// User name (not secret).
    pub username: String,
    /// A secret is stored.
    pub has_secret: bool,
}

/// What protects the key that encrypts the keyring (M31b).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum KeyringProtection {
    /// A random key in a private file next to the keyring (the default).
    #[default]
    File,
    /// A random key kept by the operating system's keychain.
    OsKeychain,
    /// A key derived from a master password, asked once per session.
    Password,
}

/// Where the keyring stands, for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct KeyringStatus {
    /// What protects the key.
    pub protection: KeyringProtection,
    /// A master password is needed and was not given in this session.
    pub locked: bool,
    /// The operating system's keychain can be used on this system.
    pub os_keychain_available: bool,
}

/// PBKDF2-HMAC-SHA256 rounds for a master password.
const PASSWORD_ITERATIONS: u32 = 600_000;
/// Shortest master password.
pub const MIN_MASTER_PASSWORD: usize = 8;

#[derive(Serialize, Deserialize)]
struct Sealed {
    version: u32,
    nonce: String,
    data: String,
    /// What protects the key (files written before M31b: the key file).
    #[serde(default)]
    protection: KeyringProtection,
    /// Salt of the master password (base64).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    salt: Option<String>,
    /// PBKDF2 rounds of the master password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    iterations: Option<u32>,
}

/// The operating system's secret store, for the keyring key.
pub(crate) trait OsKeychain: Send + Sync {
    /// False where there is none to use.
    fn available(&self) -> bool;
    /// The secret stored for `account`, if any.
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, String>;
    /// Stores (or replaces) the secret of `account`.
    fn set(&self, account: &str, secret: &[u8]) -> Result<(), String>;
    /// Removes the secret of `account` (fine if there is none).
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// Name of ChosProxy's items in the OS keychain.
#[cfg(target_os = "macos")]
const OS_SERVICE: &str = "ChosProxy keyring";

/// The keychain of this system: the macOS Keychain; nothing elsewhere yet
/// (the Secret Service on Linux needs a decision about its dependencies).
#[derive(Debug, Default)]
pub(crate) struct SystemKeychain;

#[cfg(target_os = "macos")]
impl OsKeychain for SystemKeychain {
    fn available(&self) -> bool {
        true
    }

    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, String> {
        match security_framework::passwords::get_generic_password(OS_SERVICE, account) {
            Ok(secret) => Ok(Some(secret)),
            // errSecItemNotFound
            Err(e) if e.code() == -25300 => Ok(None),
            Err(e) => Err(format!("the macOS Keychain: {e}")),
        }
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<(), String> {
        security_framework::passwords::set_generic_password(OS_SERVICE, account, secret)
            .map_err(|e| format!("the macOS Keychain: {e}"))
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        match security_framework::passwords::delete_generic_password(OS_SERVICE, account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(e) => Err(format!("the macOS Keychain: {e}")),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl OsKeychain for SystemKeychain {
    fn available(&self) -> bool {
        false
    }

    fn get(&self, _account: &str) -> Result<Option<Vec<u8>>, String> {
        Err("the operating system's keychain is not supported on this system yet".into())
    }

    fn set(&self, _account: &str, _secret: &[u8]) -> Result<(), String> {
        Err("the operating system's keychain is not supported on this system yet".into())
    }

    fn delete(&self, _account: &str) -> Result<(), String> {
        Ok(())
    }
}

/// A keychain in memory, for tests.
#[derive(Debug, Default)]
pub struct MemoryKeychain(std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>);

impl OsKeychain for MemoryKeychain {
    fn available(&self) -> bool {
        true
    }

    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(account)
            .cloned())
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<(), String> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(account);
        Ok(())
    }
}

/// The keyring files in a directory.
#[derive(Clone)]
pub(crate) struct Keyring {
    dir: PathBuf,
    os: std::sync::Arc<dyn OsKeychain>,
    /// The key derived from the master password, once it was given.
    unlocked: Option<[u8; 32]>,
}

impl std::fmt::Debug for Keyring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keyring")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "no random numbers available".to_string())?;
    Ok(bytes)
}

fn aead(bytes: &[u8]) -> Result<LessSafeKey, String> {
    let unbound = UnboundKey::new(&AES_256_GCM, bytes)
        .map_err(|_| "the keyring key is damaged".to_string())?;
    Ok(LessSafeKey::new(unbound))
}

/// The key a master password gives with this salt.
fn password_key(password: &str, salt: &[u8], iterations: u32) -> Result<[u8; 32], String> {
    let rounds = std::num::NonZeroU32::new(iterations).ok_or("the keyring file is damaged")?;
    let mut key = [0u8; 32];
    ring::pbkdf2::derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA256,
        rounds,
        salt,
        password.as_bytes(),
        &mut key,
    );
    Ok(key)
}

impl Keyring {
    pub(crate) fn new(dir: impl Into<PathBuf>) -> Keyring {
        Keyring::with(dir, std::sync::Arc::new(SystemKeychain), None)
    }

    /// A keyring with the OS keychain to use and the key a master password
    /// gave in this session, if any.
    pub(crate) fn with(
        dir: impl Into<PathBuf>,
        os: std::sync::Arc<dyn OsKeychain>,
        unlocked: Option<[u8; 32]>,
    ) -> Keyring {
        Keyring {
            dir: dir.into(),
            os,
            unlocked,
        }
    }

    /// The account of this keyring in the OS keychain: one per data
    /// directory, so two installations do not share a key.
    fn os_account(&self) -> String {
        let digest =
            ring::digest::digest(&ring::digest::SHA256, self.dir.to_string_lossy().as_bytes());
        digest.as_ref()[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn read_sealed(&self) -> Result<Option<Sealed>, String> {
        let path = self.dir.join(DATA_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|_| "the keyring file is damaged".to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// What protects the key, and whether a master password is missing.
    pub(crate) fn status(&self) -> Result<KeyringStatus, String> {
        let protection = self
            .read_sealed()?
            .map(|s| s.protection)
            .unwrap_or_default();
        Ok(KeyringStatus {
            protection,
            locked: protection == KeyringProtection::Password && self.unlocked.is_none(),
            os_keychain_available: self.os.available(),
        })
    }

    /// The key of the file protection, made the first time when `create`.
    fn file_key(&self, create: bool) -> Result<Option<Vec<u8>>, String> {
        let path = self.dir.join(KEY_FILE);
        match std::fs::read(&path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                chosproxy_core::fs::create_private_dir(&self.dir).map_err(|e| e.to_string())?;
                let k = random::<32>()?;
                chosproxy_core::fs::write_private_atomic(&path, &k).map_err(|e| e.to_string())?;
                Ok(Some(k.to_vec()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// The key that opens a keyring sealed with `protection`.
    fn key_for(&self, protection: KeyringProtection) -> Result<Vec<u8>, String> {
        match protection {
            KeyringProtection::File => self.file_key(false)?.ok_or_else(|| {
                "the keyring key file is missing; the credentials cannot be read".into()
            }),
            KeyringProtection::OsKeychain => self.os.get(&self.os_account())?.ok_or_else(|| {
                "the keyring key is not in the operating system's keychain; the credentials \
                 cannot be read"
                    .into()
            }),
            KeyringProtection::Password => self.unlocked.map(|k| k.to_vec()).ok_or_else(|| {
                "the keyring is locked: give its master password in Settings → Credentials".into()
            }),
        }
    }

    fn open(&self, sealed: &Sealed, key: &[u8]) -> Result<Vec<Credential>, String> {
        let nonce_bytes: [u8; 12] = b64()
            .decode(&sealed.nonce)
            .ok()
            .and_then(|n| n.try_into().ok())
            .ok_or("the keyring file is damaged")?;
        let mut data = b64()
            .decode(&sealed.data)
            .map_err(|_| "the keyring file is damaged".to_string())?;
        let plain = aead(key)?
            .open_in_place(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::empty(),
                &mut data,
            )
            .map_err(|_| {
                "the keyring cannot be decrypted (wrong key or changed file)".to_string()
            })?;
        serde_json::from_slice(plain).map_err(|_| "the keyring file is damaged".to_string())
    }

    /// Every credential (empty when none were saved yet).
    pub(crate) fn load(&self) -> Result<Vec<Credential>, String> {
        let Some(sealed) = self.read_sealed()? else {
            return Ok(Vec::new());
        };
        let key = self.key_for(sealed.protection)?;
        self.open(&sealed, &key)
    }

    fn write(
        &self,
        entries: &[Credential],
        key: &[u8],
        protection: KeyringProtection,
        salt: Option<String>,
    ) -> Result<(), String> {
        let nonce = random::<12>()?;
        let mut data = serde_json::to_vec(entries).map_err(|e| e.to_string())?;
        aead(key)?
            .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut data)
            .map_err(|_| "cannot encrypt the keyring".to_string())?;
        let sealed = Sealed {
            version: 2,
            nonce: b64().encode(nonce),
            data: b64().encode(&data),
            protection,
            iterations: salt.as_ref().map(|_| PASSWORD_ITERATIONS),
            salt,
        };
        let json = serde_json::to_vec(&sealed).map_err(|e| e.to_string())?;
        chosproxy_core::fs::create_private_dir(&self.dir).map_err(|e| e.to_string())?;
        chosproxy_core::fs::write_private_atomic(&self.dir.join(DATA_FILE), &json)
            .map_err(|e| e.to_string())
    }

    /// Replaces every credential (the protection stays as it is).
    pub(crate) fn save(&self, entries: &[Credential]) -> Result<(), String> {
        match self.read_sealed()? {
            None => {
                let key = self
                    .file_key(true)?
                    .ok_or("cannot create the keyring key")?;
                self.write(entries, &key, KeyringProtection::File, None)
            }
            Some(sealed) => {
                let key = match sealed.protection {
                    // A key file lost while the data file stayed is not
                    // replaced silently: the old entries would be gone.
                    KeyringProtection::File => self.key_for(KeyringProtection::File)?,
                    other => self.key_for(other)?,
                };
                self.write(entries, &key, sealed.protection, sealed.salt)
            }
        }
    }

    /// The key a master password gives for this keyring, checked against
    /// the file (an error for a wrong password).
    pub(crate) fn unlock(&self, password: &str) -> Result<[u8; 32], String> {
        let sealed = self
            .read_sealed()?
            .filter(|s| s.protection == KeyringProtection::Password)
            .ok_or("the keyring has no master password")?;
        let salt = sealed
            .salt
            .as_deref()
            .and_then(|s| b64().decode(s).ok())
            .ok_or("the keyring file is damaged")?;
        let key = password_key(
            password,
            &salt,
            sealed.iterations.unwrap_or(PASSWORD_ITERATIONS),
        )?;
        self.open(&sealed, &key)
            .map_err(|_| "wrong master password".to_string())?;
        Ok(key)
    }

    /// Changes what protects the key: the credentials are read with the
    /// current protection (so a locked keyring must be unlocked first) and
    /// written again in one step; then the old key is removed. Returns the
    /// key to keep for the session when the new protection is a password.
    pub(crate) fn set_protection(
        &self,
        protection: KeyringProtection,
        password: Option<&str>,
    ) -> Result<Option<[u8; 32]>, String> {
        let old = self.read_sealed()?.map(|s| s.protection);
        let entries = self.load()?;
        let mut session_key = None;
        match protection {
            KeyringProtection::File => {
                // A fresh key: the old one may have been anywhere.
                let _ = std::fs::remove_file(self.dir.join(KEY_FILE));
                let key = self
                    .file_key(true)?
                    .ok_or("cannot create the keyring key")?;
                self.write(&entries, &key, protection, None)?;
            }
            KeyringProtection::OsKeychain => {
                if !self.os.available() {
                    return Err(
                        "the operating system's keychain is not supported on this system yet"
                            .into(),
                    );
                }
                let key = random::<32>()?;
                self.os.set(&self.os_account(), &key)?;
                self.write(&entries, &key, protection, None)?;
            }
            KeyringProtection::Password => {
                let password = password.unwrap_or("");
                if password.chars().count() < MIN_MASTER_PASSWORD {
                    return Err(format!(
                        "the master password needs at least {MIN_MASTER_PASSWORD} characters"
                    ));
                }
                let salt = random::<16>()?;
                let key = password_key(password, &salt, PASSWORD_ITERATIONS)?;
                self.write(&entries, &key, protection, Some(b64().encode(salt)))?;
                session_key = Some(key);
            }
        }
        // The old key opens nothing any more.
        if old == Some(KeyringProtection::File) && protection != KeyringProtection::File {
            let _ = std::fs::remove_file(self.dir.join(KEY_FILE));
        }
        if old == Some(KeyringProtection::OsKeychain) && protection != KeyringProtection::OsKeychain
        {
            let _ = self.os.delete(&self.os_account());
        }
        Ok(session_key)
    }

    #[cfg(test)]
    fn path(&self) -> &std::path::Path {
        &self.dir
    }
}

/// Checks a credential before it is stored.
pub(crate) fn validate(c: &Credential) -> Result<(), String> {
    if !chosproxy_core::config::valid_script_id(&c.id) {
        return Err("the id must be 1-48 characters of a-z, 0-9 or '-'".into());
    }
    if c.label.trim().is_empty() || c.label.chars().count() > 80 {
        return Err("the name must be 1-80 characters".into());
    }
    if c.username.len() > 256 || c.secret.len() > 4096 {
        return Err("the user name or secret is too long".into());
    }
    if c.username.contains(['\r', '\n']) || c.secret.contains(['\r', '\n']) {
        return Err("line breaks are not allowed".into());
    }
    Ok(())
}

impl Credential {
    pub(crate) fn info(&self) -> CredentialInfo {
        CredentialInfo {
            id: self.id.clone(),
            label: self.label.clone(),
            username: self.username.clone(),
            has_secret: !self.secret.is_empty(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn cred(id: &str, secret: &str) -> Credential {
        Credential {
            id: id.into(),
            label: "Lab admin".into(),
            username: "admin".into(),
            secret: secret.into(),
        }
    }

    #[test]
    fn round_trip_encrypted_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let k = Keyring::new(dir.path().join("keyring"));
        assert!(k.load().unwrap().is_empty());
        k.save(&[cred("lab", "s3cret-value")]).unwrap();
        let raw = std::fs::read_to_string(k.path().join(DATA_FILE)).unwrap();
        assert!(
            !raw.contains("s3cret-value") && !raw.contains("admin"),
            "encrypted at rest"
        );
        let back = k.load().unwrap();
        assert_eq!(back[0].secret, "s3cret-value");
        assert!(
            !format!("{:?}", back[0]).contains("s3cret"),
            "Debug hides the secret"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for f in [DATA_FILE, KEY_FILE] {
                let mode = std::fs::metadata(k.path().join(f))
                    .unwrap()
                    .permissions()
                    .mode();
                assert_eq!(mode & 0o777, 0o600, "{f}");
            }
        }
        // A changed file or another key is detected.
        let mut sealed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        sealed["data"] = serde_json::Value::String(b64().encode([1u8; 40]));
        std::fs::write(k.path().join(DATA_FILE), sealed.to_string()).unwrap();
        assert!(k.load().unwrap_err().contains("cannot be decrypted"));
    }

    #[test]
    fn the_key_can_move_to_the_os_keychain_or_behind_a_password() {
        let dir = tempfile::tempdir().unwrap();
        let os = std::sync::Arc::new(MemoryKeychain::default());
        let at = |unlocked| Keyring::with(dir.path().join("keyring"), os.clone(), unlocked);
        let k = at(None);
        assert_eq!(k.status().unwrap().protection, KeyringProtection::File);
        k.save(&[cred("lab", "s3cret-value")]).unwrap();
        let key_file = k.path().join(KEY_FILE);
        assert!(key_file.is_file());

        // To the OS keychain: the key file goes, the entries stay.
        assert_eq!(
            k.set_protection(KeyringProtection::OsKeychain, None)
                .unwrap(),
            None
        );
        assert!(!key_file.exists());
        assert_eq!(os.get(&k.os_account()).unwrap().unwrap().len(), 32);
        assert_eq!(k.load().unwrap()[0].secret, "s3cret-value");
        k.save(&[cred("lab", "s3cret-value"), cred("two", "x")])
            .unwrap();
        assert_eq!(
            k.status().unwrap().protection,
            KeyringProtection::OsKeychain
        );
        // Without the keychain item nothing can be read (and nothing is
        // overwritten with a new key).
        let lost = Keyring::with(
            dir.path().join("keyring"),
            std::sync::Arc::new(MemoryKeychain::default()),
            None,
        );
        assert!(
            lost.load()
                .unwrap_err()
                .contains("not in the operating system")
        );
        assert!(lost.save(&[]).is_err());

        // Behind a master password: short ones are refused.
        assert!(
            k.set_protection(KeyringProtection::Password, Some("short"))
                .is_err()
        );
        let key = k
            .set_protection(KeyringProtection::Password, Some("correct horse"))
            .unwrap()
            .unwrap();
        assert!(
            os.get(&k.os_account()).unwrap().is_none(),
            "the old key is gone"
        );
        let raw = std::fs::read_to_string(k.path().join(DATA_FILE)).unwrap();
        assert!(!raw.contains("correct horse") && raw.contains("\"salt\""));
        // Locked until the password is given.
        let locked = at(None);
        assert!(locked.status().unwrap().locked);
        assert!(locked.load().unwrap_err().contains("locked"));
        assert!(locked.save(&[]).is_err());
        assert_eq!(
            locked.unlock("wrong horse!").unwrap_err(),
            "wrong master password"
        );
        assert_eq!(locked.unlock("correct horse").unwrap(), key);
        let open = at(Some(key));
        assert!(!open.status().unwrap().locked);
        assert_eq!(open.load().unwrap().len(), 2);
        open.save(&[cred("lab", "changed")]).unwrap();
        assert_eq!(at(Some(key)).load().unwrap()[0].secret, "changed");
        // Changing it needs the keyring open.
        assert!(
            locked
                .set_protection(KeyringProtection::File, None)
                .is_err()
        );
        // Back to the key file.
        assert_eq!(
            open.set_protection(KeyringProtection::File, None).unwrap(),
            None
        );
        assert!(key_file.is_file());
        assert_eq!(at(None).load().unwrap()[0].secret, "changed");
        assert!(at(None).unlock("correct horse").is_err());
    }

    #[test]
    fn a_keyring_written_before_protections_still_opens() {
        let dir = tempfile::tempdir().unwrap();
        let k = Keyring::new(dir.path().join("keyring"));
        k.save(&[cred("lab", "old")]).unwrap();
        // Version 1 files had no protection field.
        let path = k.path().join(DATA_FILE);
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        v.as_object_mut().unwrap().remove("protection");
        v["version"] = 1.into();
        std::fs::write(&path, v.to_string()).unwrap();
        assert_eq!(k.load().unwrap()[0].secret, "old");
        assert_eq!(k.status().unwrap().protection, KeyringProtection::File);
    }

    #[test]
    fn validation() {
        assert!(validate(&cred("ok-1", "x")).is_ok());
        assert!(validate(&cred("Bad Id", "x")).is_err());
        assert!(validate(&cred("ok", "a\nb")).is_err());
    }
}
