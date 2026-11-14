//! Password-sealed files for sharing exports (M31, ADR-0041): Organizer
//! collections and settings. Only audited primitives from `ring`:
//! PBKDF2-HMAC-SHA256 derives a 256-bit key from the password and a random
//! salt; AES-256-GCM encrypts and authenticates the content. The header
//! (format, version, iterations, salt, nonce) is authenticated too, so any
//! change to the file — header or content — is detected.
//!
//! Layout: `CHOSSEAL` · version (1 byte) · KDF id (1 byte) · iterations
//! (u32, big endian) · salt (16 bytes) · nonce (12 bytes) · ciphertext with
//! the 16-byte tag.

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use std::num::NonZeroU32;

const MAGIC: &[u8; 8] = b"CHOSSEAL";
const VERSION: u8 = 1;
/// PBKDF2-HMAC-SHA256.
const KDF_PBKDF2_SHA256: u8 = 1;
/// Iterations used when sealing (OWASP guidance for PBKDF2-HMAC-SHA256).
pub const SEAL_ITERATIONS: u32 = 600_000;
/// Most iterations accepted when opening (a file cannot make us spin).
const MAX_ITERATIONS: u32 = 10_000_000;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = 8 + 1 + 1 + 4 + SALT_LEN + NONCE_LEN;
const TAG_LEN: usize = 16;
/// Largest content sealed or opened.
pub const MAX_SEALED: usize = 512 * 1024 * 1024;
/// Longest password.
const MAX_PASSWORD: usize = 1024;

/// Why a sealed file could not be made or opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SealError {
    /// The data is not a sealed file.
    #[error("not a ChosProxy encrypted file")]
    NotSealed,
    /// Made by a newer ChosProxy.
    #[error("this encrypted file uses a newer format; upgrade ChosProxy")]
    Unsupported,
    /// Wrong password, or the file was changed or damaged.
    #[error("wrong password, or the file was changed or damaged")]
    Rejected,
    /// The password is empty or too long.
    #[error("{0}")]
    Password(String),
    /// Too large, or no randomness available.
    #[error("{0}")]
    Other(String),
}

/// Whether `data` starts like a sealed file.
pub fn is_sealed(data: &[u8]) -> bool {
    data.starts_with(MAGIC)
}

fn check_password(password: &str) -> Result<(), SealError> {
    if password.is_empty() {
        return Err(SealError::Password("the password is empty".into()));
    }
    if password.chars().count() > MAX_PASSWORD {
        return Err(SealError::Password("the password is too long".into()));
    }
    Ok(())
}

fn derive(password: &str, salt: &[u8], iterations: NonZeroU32) -> Result<LessSafeKey, SealError> {
    let mut key = [0u8; 32];
    ring::pbkdf2::derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        salt,
        password.as_bytes(),
        &mut key,
    );
    let unbound =
        UnboundKey::new(&AES_256_GCM, &key).map_err(|_| SealError::Other("bad key".into()))?;
    Ok(LessSafeKey::new(unbound))
}

/// Seals `content` with `password`.
pub fn seal(password: &str, content: &[u8]) -> Result<Vec<u8>, SealError> {
    seal_with(password, content, SEAL_ITERATIONS)
}

pub(crate) fn seal_with(
    password: &str,
    content: &[u8],
    iterations: u32,
) -> Result<Vec<u8>, SealError> {
    check_password(password)?;
    if content.len() > MAX_SEALED {
        return Err(SealError::Other(
            "the content is too large to encrypt".into(),
        ));
    }
    let iterations = NonZeroU32::new(iterations)
        .filter(|n| n.get() <= MAX_ITERATIONS)
        .ok_or_else(|| SealError::Other("invalid iteration count".into()))?;
    let rng = SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill(&mut salt)
        .and_then(|()| rng.fill(&mut nonce))
        .map_err(|_| SealError::Other("no random numbers available".into()))?;
    let mut out = Vec::with_capacity(HEADER_LEN + content.len() + TAG_LEN);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(KDF_PBKDF2_SHA256);
    out.extend_from_slice(&iterations.get().to_be_bytes());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    let key = derive(password, &salt, iterations)?;
    let mut body = content.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(&out[..HEADER_LEN]),
        &mut body,
    )
    .map_err(|_| SealError::Other("encryption failed".into()))?;
    out.extend_from_slice(&body);
    Ok(out)
}

/// Opens a sealed file with `password`.
pub fn open(password: &str, data: &[u8]) -> Result<Vec<u8>, SealError> {
    if !is_sealed(data) {
        return Err(SealError::NotSealed);
    }
    if data.len() < HEADER_LEN + TAG_LEN {
        return Err(SealError::Rejected);
    }
    if data.len() > MAX_SEALED + HEADER_LEN + TAG_LEN {
        return Err(SealError::Other("the file is too large".into()));
    }
    check_password(password)?;
    let (header, body) = data.split_at(HEADER_LEN);
    if header[8] != VERSION || header[9] != KDF_PBKDF2_SHA256 {
        return Err(SealError::Unsupported);
    }
    let iterations = u32::from_be_bytes([header[10], header[11], header[12], header[13]]);
    let iterations = NonZeroU32::new(iterations)
        .filter(|n| n.get() <= MAX_ITERATIONS)
        .ok_or(SealError::Rejected)?;
    let salt = &header[14..14 + SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&header[14 + SALT_LEN..HEADER_LEN]);
    let key = derive(password, salt, iterations)?;
    let mut buf = body.to_vec();
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(header),
            &mut buf,
        )
        .map_err(|_| SealError::Rejected)?;
    Ok(plain.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbkdf2_matches_a_published_vector() {
        // PBKDF2-HMAC-SHA256, P = "password", S = "salt", c = 1, dkLen = 32.
        let expected = [
            0x12, 0x0f, 0xb6, 0xcf, 0xfc, 0xf8, 0xb3, 0x2c, 0x43, 0xe7, 0x22, 0x52, 0x56, 0xc4,
            0xf8, 0x37, 0xa8, 0x65, 0x48, 0xc9, 0x2c, 0xcc, 0x35, 0x48, 0x08, 0x05, 0x98, 0x7c,
            0xb7, 0x0b, 0xe1, 0x7b,
        ];
        let mut out = [0u8; 32];
        ring::pbkdf2::derive(
            ring::pbkdf2::PBKDF2_HMAC_SHA256,
            NonZeroU32::MIN,
            b"salt",
            b"password",
            &mut out,
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn seals_and_opens_and_detects_every_change() {
        let content = b"{\"format\":\"x\",\"secret\":\"PLAINTEXT\"}";
        let sealed = seal_with("correct horse", content, 1000).unwrap();
        assert!(is_sealed(&sealed));
        assert!(
            !sealed.windows(9).any(|w| w == b"PLAINTEXT"),
            "content is not visible"
        );
        assert_eq!(open("correct horse", &sealed).unwrap(), content);
        // Two seals of the same content differ (random salt and nonce).
        assert_ne!(sealed, seal_with("correct horse", content, 1000).unwrap());

        assert_eq!(open("wrong", &sealed), Err(SealError::Rejected));
        for at in [9, 12, 20, HEADER_LEN, sealed.len() - 1] {
            let mut bad = sealed.clone();
            bad[at] ^= 1;
            let r = open("correct horse", &bad);
            assert!(
                matches!(r, Err(SealError::Rejected | SealError::Unsupported)),
                "change at byte {at} not detected: {r:?}"
            );
        }
        assert_eq!(
            open("correct horse", &sealed[..sealed.len() - 3]),
            Err(SealError::Rejected)
        );
        assert_eq!(open("correct horse", content), Err(SealError::NotSealed));
        assert!(matches!(seal("", content), Err(SealError::Password(_))));
        let mut huge_iter = sealed.clone();
        huge_iter[10..14].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(open("correct horse", &huge_iter), Err(SealError::Rejected));
    }
}
