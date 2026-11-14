//! Correlation identifiers.
//!
//! An identifier is 20 characters from the DNS-safe base32 alphabet: 16
//! random characters and a 4-character checksum. The checksum lets any
//! ChosOAST server, including one that never saw the identifier being
//! created, recognise identifiers in DNS names, Host headers or paths
//! without false positives from ordinary words.

use ring::rand::{SecureRandom, SystemRandom};

/// Characters used (lowercase base32: DNS-safe, case-insensitive).
pub const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
/// Length of an identifier.
pub const ID_LEN: usize = 20;
const RANDOM_LEN: usize = 16;

fn checksum(body: &[u8]) -> [u8; 4] {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    ctx.update(b"chosoast-id-v1:");
    ctx.update(body);
    let d = ctx.finish();
    let bytes = d.as_ref();
    let bits =
        (u32::from(bytes[0]) << 12) | (u32::from(bytes[1]) << 4) | (u32::from(bytes[2]) >> 4);
    let mut out = [0u8; 4];
    for (i, o) in out.iter_mut().enumerate() {
        *o = ALPHABET[((bits >> (15 - 5 * i)) & 31) as usize];
    }
    out
}

/// A new random identifier.
pub fn new_id() -> Result<String, ring::error::Unspecified> {
    let mut raw = [0u8; RANDOM_LEN];
    SystemRandom::new().fill(&mut raw)?;
    let mut id: Vec<u8> = raw.iter().map(|b| ALPHABET[usize::from(b & 31)]).collect();
    let sum = checksum(&id);
    id.extend_from_slice(&sum);
    Ok(String::from_utf8_lossy(&id).into_owned())
}

/// Whether `s` is a well-formed identifier (case-insensitive).
pub fn is_valid(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    let b = lower.as_bytes();
    b.len() == ID_LEN
        && b.iter().all(|c| ALPHABET.contains(c))
        && checksum(&b[..RANDOM_LEN]) == b[RANDOM_LEN..]
}

fn in_alphabet(c: u8) -> bool {
    ALPHABET.contains(&c.to_ascii_lowercase())
}

/// Every identifier found in `data` (lowercase, first occurrence order,
/// no duplicates). Runs of alphabet characters longer than an identifier
/// are searched window by window up to 256 characters.
pub fn extract_ids(data: &[u8]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if !in_alphabet(data[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < data.len() && in_alphabet(data[i]) {
            i += 1;
        }
        let run = &data[start..i];
        if run.len() < ID_LEN || run.len() > 256 {
            continue;
        }
        for w in run.windows(ID_LEN) {
            let candidate = String::from_utf8_lossy(w).to_ascii_lowercase();
            if is_valid(&candidate) && !found.contains(&candidate) {
                found.push(candidate);
            }
        }
    }
    found
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn ids_are_valid_unique_and_found_in_context() {
        let a = new_id().unwrap();
        let b = new_id().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), ID_LEN);
        assert!(is_valid(&a) && is_valid(&a.to_uppercase()));
        let mut broken = a.clone().into_bytes();
        broken[3] = if broken[3] == b'a' { b'b' } else { b'a' };
        assert!(!is_valid(&String::from_utf8(broken).unwrap()));

        let text = format!(
            "GET /x?u=http://{}.oast.example/ HTTP/1.1\r\nHost: {}\r\nX: {}\r\n",
            a.to_uppercase(),
            b,
            a
        );
        assert_eq!(extract_ids(text.as_bytes()), vec![a.clone(), b.clone()]);
        // Embedded in a longer word it is still found.
        assert_eq!(extract_ids(format!("zz{a}zz").as_bytes()), vec![a]);
        // Ordinary text has none.
        assert!(extract_ids(b"abcdefghijklmnopqrstuvwxyz the quick brown fox").is_empty());
    }
}
