//! Signed plugins (ADR-0033): a publisher signs the plugin's fingerprint
//! (`hash_dir`, which covers every file) together with its id and version
//! using Ed25519. ChosProxy shows whether a plugin is unsigned, signed by
//! an unknown key, signed by a trusted publisher, or tampered with. A
//! signature never grants anything by itself: the tester still enables the
//! plugin and grants each permission.

use crate::manifest::{Manifest, hash_dir};
use base64::Engine as _;
use ring::rand::SystemRandom;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The signature file inside a plugin directory.
pub const SIG_FILE: &str = "plugin.sig";

/// Contents of `plugin.sig`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigFile {
    /// Who signed it (a name chosen by the publisher; only trust decides).
    pub publisher: String,
    /// Ed25519 public key, Base64.
    pub key: String,
    /// Ed25519 signature of [`signed_message`], Base64.
    pub signature: String,
}

/// What the signature of a plugin directory shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verified {
    /// No `plugin.sig`.
    Unsigned,
    /// A signature that does not match the files (or cannot be read).
    Invalid(String),
    /// A valid signature by this key.
    Valid {
        /// Publisher name in the signature.
        publisher: String,
        /// Public key, Base64.
        key: String,
    },
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// The bytes a publisher signs.
pub fn signed_message(id: &str, version: &str, hash: &str) -> Vec<u8> {
    format!("chosproxy-plugin-signature-v1\n{id}\n{version}\n{hash}\n").into_bytes()
}

/// A short fingerprint of a public key (first 16 hex digits of SHA-256),
/// for people comparing keys.
pub fn key_fingerprint(key_b64: &str) -> String {
    let bytes = b64().decode(key_b64).unwrap_or_default();
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    digest
        .as_ref()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// A new signing key: (PKCS#8 private key, public key Base64).
pub fn generate_key() -> Result<(Vec<u8>, String), String> {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
        .map_err(|_| "cannot generate a key".to_string())?;
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).map_err(|e| e.to_string())?;
    Ok((
        pkcs8.as_ref().to_vec(),
        b64().encode(pair.public_key().as_ref()),
    ))
}

/// Signs a plugin directory with a PKCS#8 key and writes `plugin.sig`.
pub fn sign_dir(dir: &Path, pkcs8: &[u8], publisher: &str) -> Result<SigFile, String> {
    let publisher = publisher.trim();
    if publisher.is_empty() || publisher.chars().count() > 100 {
        return Err("the publisher name must be 1–100 characters".into());
    }
    let manifest = Manifest::load(dir)?;
    let hash = hash_dir(dir)?;
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8).map_err(|e| format!("invalid key: {e}"))?;
    let sig = pair.sign(&signed_message(&manifest.id, &manifest.version, &hash));
    let file = SigFile {
        publisher: publisher.to_string(),
        key: b64().encode(pair.public_key().as_ref()),
        signature: b64().encode(sig.as_ref()),
    };
    let text = toml::to_string(&file).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(SIG_FILE), text).map_err(|e| e.to_string())?;
    Ok(file)
}

/// Checks the signature of a plugin directory against its files.
pub fn verify_dir(dir: &Path, manifest: &Manifest) -> Verified {
    let path = dir.join(SIG_FILE);
    if !path.exists() {
        return Verified::Unsigned;
    }
    let file: SigFile = match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|t| toml::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(f) => f,
        Err(e) => return Verified::Invalid(format!("unreadable signature: {e}")),
    };
    let hash = match hash_dir(dir) {
        Ok(h) => h,
        Err(e) => return Verified::Invalid(e),
    };
    let (Ok(key), Ok(sig)) = (b64().decode(&file.key), b64().decode(&file.signature)) else {
        return Verified::Invalid("the signature or key is not valid Base64".into());
    };
    let message = signed_message(&manifest.id, &manifest.version, &hash);
    match UnparsedPublicKey::new(&ED25519, key).verify(&message, &sig) {
        Ok(()) => Verified::Valid {
            publisher: file.publisher,
            key: file.key,
        },
        Err(_) => Verified::Invalid(
            "the signature does not match the files (changed after signing?)".into(),
        ),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn plugin(dir: &Path) {
        std::fs::write(
            dir.join("plugin.toml"),
            "id = \"signed\"\nname = \"Signed\"\nversion = \"1.0\"\napi = 2\ncommand = [\"python3\", \"p.py\"]\npermissions = []\n",
        )
        .unwrap();
        std::fs::write(dir.join("p.py"), "print('hi')\n").unwrap();
    }

    #[test]
    fn sign_verify_and_detect_changes() {
        let dir = tempfile::tempdir().unwrap();
        plugin(dir.path());
        let manifest = Manifest::load(dir.path()).unwrap();
        assert_eq!(verify_dir(dir.path(), &manifest), Verified::Unsigned);
        let before = hash_dir(dir.path()).unwrap();
        let (key, public) = generate_key().unwrap();
        sign_dir(dir.path(), &key, "Lab Team").unwrap();
        assert_eq!(
            hash_dir(dir.path()).unwrap(),
            before,
            "the signature is not hashed"
        );
        assert_eq!(
            verify_dir(dir.path(), &manifest),
            Verified::Valid {
                publisher: "Lab Team".into(),
                key: public.clone()
            }
        );
        assert_eq!(key_fingerprint(&public).len(), 23);

        // Any change to a file breaks it.
        std::fs::write(dir.path().join("p.py"), "print('evil')\n").unwrap();
        assert!(matches!(
            verify_dir(dir.path(), &manifest),
            Verified::Invalid(_)
        ));
        // So does a different key claiming the same publisher.
        std::fs::write(dir.path().join("p.py"), "print('hi')\n").unwrap();
        let (other, _) = generate_key().unwrap();
        let forged = sign_dir(dir.path(), &other, "Lab Team").unwrap();
        assert_ne!(forged.key, public);
        assert!(
            matches!(verify_dir(dir.path(), &manifest), Verified::Valid { key, .. } if key != public)
        );
        std::fs::write(dir.path().join(SIG_FILE), "nonsense").unwrap();
        assert!(matches!(
            verify_dir(dir.path(), &manifest),
            Verified::Invalid(_)
        ));
    }
}
