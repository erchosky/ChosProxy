//! The API token: 32 random bytes (hex) in `data_dir/api.token`, a private
//! file. Anyone who can read it can use the API, like anyone who can read
//! the project files can read the traffic.

use ring::rand::{SecureRandom, SystemRandom};
use std::io;
use std::path::{Path, PathBuf};

/// The token file name in the data directory.
pub const TOKEN_FILE: &str = "api.token";

/// Where the token is kept.
pub fn token_path(data_dir: &Path) -> PathBuf {
    data_dir.join(TOKEN_FILE)
}

fn new_token() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| io::Error::other("no system random source"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Reads the token, creating it first if there is none (or it is unusable).
pub fn load_or_create(data_dir: &Path) -> io::Result<String> {
    let path = token_path(data_dir);
    if let Ok(text) = std::fs::read_to_string(&path) {
        let token = text.trim();
        if token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(token.to_string());
        }
    }
    regenerate(data_dir)
}

/// Reads the token if it exists (clients never create one).
pub fn read(data_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(token_path(data_dir)).ok()?;
    let token = text.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// Writes a new token (the old one stops working once the server reloads).
pub fn regenerate(data_dir: &Path) -> io::Result<String> {
    chosproxy_core::fs::create_private_dir(data_dir)?;
    let token = new_token()?;
    chosproxy_core::fs::write_private_atomic(&token_path(data_dir), token.as_bytes())?;
    Ok(token)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn created_once_then_kept_and_regenerated_on_demand() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).is_none());
        let a = load_or_create(dir.path()).unwrap();
        assert_eq!(a.len(), 64);
        assert_eq!(load_or_create(dir.path()).unwrap(), a);
        assert_eq!(read(dir.path()).unwrap(), a);
        let b = regenerate(dir.path()).unwrap();
        assert_ne!(a, b);
        std::fs::write(token_path(dir.path()), "short").unwrap();
        assert_ne!(load_or_create(dir.path()).unwrap(), "short");
    }
}
