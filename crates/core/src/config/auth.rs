//! Proxy listener authentication: credentials are stored as a salted
//! PBKDF2-HMAC-SHA256 hash, never as the password itself.

use ring::{pbkdf2, rand::SecureRandom};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use ts_rs::TS;

/// Iterations used for new credentials.
pub const PBKDF2_ITERATIONS: u32 = 100_000;
const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;
/// Longest accepted user name.
pub const MAX_USERNAME: usize = 64;
/// Longest accepted password.
pub const MAX_PASSWORD: usize = 256;

/// Credentials a client must send in `Proxy-Authorization: Basic`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ProxyAuth {
    /// User name (no `:`).
    pub username: String,
    /// PBKDF2-HMAC-SHA256 of the password, hex.
    pub password_hash: String,
    /// Random salt, hex.
    pub salt: String,
    /// PBKDF2 iterations.
    pub iterations: u32,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Checks a user name for use in Basic authentication.
pub fn validate_username(username: &str) -> Result<(), String> {
    if username.is_empty() || username.len() > MAX_USERNAME {
        return Err(format!("the user name must be 1–{MAX_USERNAME} characters"));
    }
    if username.contains(':') || username.chars().any(char::is_control) {
        return Err("the user name cannot contain ':' or control characters".into());
    }
    Ok(())
}

impl ProxyAuth {
    /// Hashes new credentials with a random salt. Slow by design (~0.1 s).
    pub fn new(username: &str, password: &str) -> Result<ProxyAuth, String> {
        validate_username(username)?;
        if password.is_empty() || password.len() > MAX_PASSWORD {
            return Err(format!("the password must be 1–{MAX_PASSWORD} bytes"));
        }
        let mut salt = [0u8; SALT_LEN];
        ring::rand::SystemRandom::new()
            .fill(&mut salt)
            .map_err(|_| "the system random generator failed".to_string())?;
        let mut hash = [0u8; HASH_LEN];
        let iterations = NonZeroU32::new(PBKDF2_ITERATIONS).ok_or("invalid iterations")?;
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            &salt,
            password.as_bytes(),
            &mut hash,
        );
        Ok(ProxyAuth {
            username: username.to_string(),
            password_hash: hex(&hash),
            salt: hex(&salt),
            iterations: PBKDF2_ITERATIONS,
        })
    }

    /// Checks the stored fields are well formed.
    pub fn validate(&self) -> Result<(), String> {
        validate_username(&self.username)?;
        let ok = unhex(&self.password_hash).is_some_and(|h| h.len() == HASH_LEN)
            && unhex(&self.salt).is_some_and(|s| s.len() >= 8)
            && (10_000..=10_000_000).contains(&self.iterations);
        if ok {
            Ok(())
        } else {
            Err("invalid stored proxy credentials; set the password again".into())
        }
    }

    /// Verifies a user name and password (constant time for the password).
    pub fn verify(&self, username: &str, password: &str) -> bool {
        let (Some(hash), Some(salt), Some(iterations)) = (
            unhex(&self.password_hash),
            unhex(&self.salt),
            NonZeroU32::new(self.iterations),
        ) else {
            return false;
        };
        let password_ok = pbkdf2::verify(
            pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            &salt,
            password.as_bytes(),
            &hash,
        )
        .is_ok();
        password_ok && username == self.username
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies() {
        let a = ProxyAuth::new("tester", "s3cret pass").unwrap();
        a.validate().unwrap();
        assert!(!a.password_hash.contains("s3cret"));
        assert!(a.verify("tester", "s3cret pass"));
        assert!(!a.verify("tester", "s3cret"));
        assert!(!a.verify("other", "s3cret pass"));
        let b = ProxyAuth::new("tester", "s3cret pass").unwrap();
        assert_ne!(a.salt, b.salt, "random salt");
        assert_ne!(a.password_hash, b.password_hash);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(ProxyAuth::new("a:b", "x").is_err());
        assert!(ProxyAuth::new("", "x").is_err());
        assert!(ProxyAuth::new("a", "").is_err());
        let mut a = ProxyAuth::new("a", "b").unwrap();
        a.password_hash = "zz".into();
        assert!(a.validate().is_err());
        assert!(!a.verify("a", "b"));
    }
}
