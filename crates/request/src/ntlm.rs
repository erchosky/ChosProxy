//! NTLMv2 over HTTP for platform authentication (M31b, matrix PROXY-017,
//! [MS-NLMP]): the three messages a client exchanges with a server that
//! answers `401` with `WWW-Authenticate: NTLM`. Only NTLMv2 is spoken
//! (never NTLMv1 or LM); Kerberos / Negotiate is not offered.
//!
//! The hash functions come from the RustCrypto crates (`md4`, `md-5`,
//! `hmac`); this module only arranges the messages.

use base64::Engine as _;
use hmac::{Hmac, Mac};
use md4::{Digest as _, Md4};

type HmacMd5 = Hmac<md5::Md5>;

const SIGNATURE: &[u8; 8] = b"NTLMSSP\0";
const NEGOTIATE_UNICODE: u32 = 0x0000_0001;
const REQUEST_TARGET: u32 = 0x0000_0004;
const NEGOTIATE_NTLM: u32 = 0x0000_0200;
const NEGOTIATE_ALWAYS_SIGN: u32 = 0x0000_8000;
const NEGOTIATE_EXTENDED_SECURITY: u32 = 0x0008_0000;
const NEGOTIATE_TARGET_INFO: u32 = 0x0080_0000;
/// Largest challenge message accepted.
const MAX_CHALLENGE: usize = 16 * 1024;

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn hmac_md5(key: &[u8], parts: &[&[u8]]) -> [u8; 16] {
    // HMAC takes a key of any length.
    let Ok(mut mac) = HmacMd5::new_from_slice(key) else {
        return [0; 16];
    };
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

/// `DOMAIN\user` or `user@domain` → (domain, user); a bare name has no
/// domain.
pub(crate) fn split_user(username: &str) -> (String, String) {
    if let Some((domain, user)) = username.split_once('\\') {
        (domain.to_string(), user.to_string())
    } else {
        (String::new(), username.to_string())
    }
}

/// The NTLMv2 key of a user: HMAC-MD5 keyed with MD4(password) over the
/// upper-cased user name and the domain ([MS-NLMP] NTOWFv2).
fn ntowf_v2(user: &str, domain: &str, password: &str) -> [u8; 16] {
    let nt_hash = Md4::digest(utf16le(password));
    hmac_md5(
        &nt_hash,
        &[&utf16le(&user.to_uppercase()), &utf16le(domain)],
    )
}

/// The first message (`Authorization: NTLM <this>`): what the client can do.
pub(crate) fn negotiate() -> String {
    let flags = NEGOTIATE_UNICODE
        | REQUEST_TARGET
        | NEGOTIATE_NTLM
        | NEGOTIATE_ALWAYS_SIGN
        | NEGOTIATE_EXTENDED_SECURITY;
    let mut m = Vec::with_capacity(32);
    m.extend_from_slice(SIGNATURE);
    m.extend_from_slice(&1u32.to_le_bytes());
    m.extend_from_slice(&flags.to_le_bytes());
    // Empty domain and workstation fields (length, max length, offset).
    m.extend_from_slice(&[0u8; 16]);
    base64::engine::general_purpose::STANDARD.encode(m)
}

/// What the server's challenge message carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Challenge {
    server_challenge: [u8; 8],
    target_info: Vec<u8>,
}

/// Reads the second message (the value after `NTLM ` in the server's
/// `WWW-Authenticate`).
pub(crate) fn parse_challenge(base64_message: &str) -> Result<Challenge, String> {
    let m = base64::engine::general_purpose::STANDARD
        .decode(base64_message.trim())
        .map_err(|_| "the NTLM challenge is not base64")?;
    if m.len() > MAX_CHALLENGE {
        return Err("the NTLM challenge is too large".into());
    }
    if m.len() < 32 || &m[..8] != SIGNATURE || m[8..12] != 2u32.to_le_bytes() {
        return Err("not an NTLM challenge message".into());
    }
    let mut server_challenge = [0u8; 8];
    server_challenge.copy_from_slice(&m[24..32]);
    let flags = u32::from_le_bytes([m[20], m[21], m[22], m[23]]);
    let mut target_info = Vec::new();
    if flags & NEGOTIATE_TARGET_INFO != 0 && m.len() >= 48 {
        let len = usize::from(u16::from_le_bytes([m[40], m[41]]));
        let offset = u32::from_le_bytes([m[44], m[45], m[46], m[47]]) as usize;
        target_info = m
            .get(offset..offset.saturating_add(len))
            .ok_or("the NTLM challenge's target information is cut")?
            .to_vec();
    }
    Ok(Challenge {
        server_challenge,
        target_info,
    })
}

/// The NTLMv2 and LMv2 responses to a challenge.
fn responses(
    key: &[u8; 16],
    challenge: &Challenge,
    client_nonce: &[u8; 8],
    timestamp: u64,
) -> (Vec<u8>, Vec<u8>) {
    let mut blob = Vec::with_capacity(32 + challenge.target_info.len());
    blob.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0]);
    blob.extend_from_slice(&timestamp.to_le_bytes());
    blob.extend_from_slice(client_nonce);
    blob.extend_from_slice(&[0; 4]);
    blob.extend_from_slice(&challenge.target_info);
    blob.extend_from_slice(&[0; 4]);
    let proof = hmac_md5(key, &[&challenge.server_challenge, &blob]);
    let mut nt = proof.to_vec();
    nt.extend_from_slice(&blob);
    let mut lm = hmac_md5(key, &[&challenge.server_challenge, client_nonce]).to_vec();
    lm.extend_from_slice(client_nonce);
    (nt, lm)
}

/// The third message (`Authorization: NTLM <this>`): the answer to the
/// challenge for `username` (`DOMAIN\user` or `user`) and `password`.
/// `timestamp` is in 100 ns since 1601 (Windows FILETIME).
pub(crate) fn authenticate(
    username: &str,
    password: &str,
    challenge: &Challenge,
    client_nonce: &[u8; 8],
    timestamp: u64,
) -> String {
    let (domain, user) = split_user(username);
    let key = ntowf_v2(&user, &domain, password);
    let (nt, lm) = responses(&key, challenge, client_nonce, timestamp);
    let domain = utf16le(&domain);
    let user = utf16le(&user);
    let workstation = utf16le("CHOSPROXY");
    let flags = NEGOTIATE_UNICODE
        | REQUEST_TARGET
        | NEGOTIATE_NTLM
        | NEGOTIATE_ALWAYS_SIGN
        | NEGOTIATE_EXTENDED_SECURITY
        | NEGOTIATE_TARGET_INFO;
    // Header: signature, type, six fields (LM, NT, domain, user,
    // workstation, session key) and the flags = 64 bytes.
    const HEADER: usize = 64;
    let fields: [&[u8]; 5] = [&lm, &nt, &domain, &user, &workstation];
    let mut m = Vec::with_capacity(HEADER + fields.iter().map(|f| f.len()).sum::<usize>());
    m.extend_from_slice(SIGNATURE);
    m.extend_from_slice(&3u32.to_le_bytes());
    let mut offset = HEADER;
    let mut field = |m: &mut Vec<u8>, len: usize| {
        let short = u16::try_from(len).unwrap_or(u16::MAX);
        m.extend_from_slice(&short.to_le_bytes());
        m.extend_from_slice(&short.to_le_bytes());
        m.extend_from_slice(&u32::try_from(offset).unwrap_or(u32::MAX).to_le_bytes());
        offset += len;
    };
    for f in fields {
        field(&mut m, f.len());
    }
    field(&mut m, 0); // no session key
    m.extend_from_slice(&flags.to_le_bytes());
    for f in fields {
        m.extend_from_slice(f);
    }
    base64::engine::general_purpose::STANDARD.encode(m)
}

/// Now, as a Windows FILETIME.
pub(crate) fn filetime_now() -> u64 {
    const EPOCH_DIFFERENCE_SECS: u64 = 11_644_473_600;
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (unix.as_secs() + EPOCH_DIFFERENCE_SECS) * 10_000_000 + u64::from(unix.subsec_nanos()) / 100
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// The published example of [MS-NLMP] §4.2.4 (NTLMv2 authentication).
    #[test]
    fn matches_the_ms_nlmp_ntlmv2_example() {
        let key = ntowf_v2("User", "Domain", "Password");
        assert_eq!(hex(&key), "0c868a403bfd7a93a3001ef22ef02e3f");
        let challenge = Challenge {
            server_challenge: [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef],
            // Domain name "Domain", computer name "Server".
            target_info: unhex(
                "02000c0044006f006d00610069006e0001000c00530065007200760065007200\
                 00000000",
            ),
        };
        let (nt, lm) = responses(&key, &challenge, &[0xaa; 8], 0);
        assert_eq!(hex(&nt[..16]), "68cd0ab851e51c96aabc927bebef6a1c");
        assert_eq!(hex(&lm), "86c35097ac9cec102554764a57cccc19aaaaaaaaaaaaaaaa");
    }

    #[test]
    fn messages_have_the_wire_layout() {
        let n = base64::engine::general_purpose::STANDARD
            .decode(negotiate())
            .unwrap();
        assert_eq!(&n[..8], SIGNATURE);
        assert_eq!(n[8..12], 1u32.to_le_bytes());
        assert_eq!(n.len(), 32);

        // A challenge with target information after a 48-byte header.
        let info = unhex("02000200580000000000");
        let mut c = Vec::new();
        c.extend_from_slice(SIGNATURE);
        c.extend_from_slice(&2u32.to_le_bytes());
        c.extend_from_slice(&[0; 8]); // target name field
        c.extend_from_slice(&(NEGOTIATE_UNICODE | NEGOTIATE_TARGET_INFO).to_le_bytes());
        c.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        c.extend_from_slice(&[0; 8]); // reserved
        c.extend_from_slice(&(info.len() as u16).to_le_bytes());
        c.extend_from_slice(&(info.len() as u16).to_le_bytes());
        c.extend_from_slice(&48u32.to_le_bytes());
        c.extend_from_slice(&info);
        let encoded = base64::engine::general_purpose::STANDARD.encode(&c);
        let challenge = parse_challenge(&encoded).unwrap();
        assert_eq!(challenge.server_challenge, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(challenge.target_info, info);

        let a = base64::engine::general_purpose::STANDARD
            .decode(authenticate("LAB\\alice", "pw", &challenge, &[9; 8], 5))
            .unwrap();
        assert_eq!(a[8..12], 3u32.to_le_bytes());
        // Each field points inside the message; the user is "alice".
        let field = |at: usize| {
            let len = usize::from(u16::from_le_bytes([a[at], a[at + 1]]));
            let off = u32::from_le_bytes([a[at + 4], a[at + 5], a[at + 6], a[at + 7]]) as usize;
            a[off..off + len].to_vec()
        };
        assert_eq!(field(12).len(), 24, "LMv2 response");
        assert_eq!(field(28), utf16le("LAB"));
        assert_eq!(field(36), utf16le("alice"));
        let nt = field(20);
        let key = ntowf_v2("alice", "LAB", "pw");
        assert_eq!(nt, responses(&key, &challenge, &[9; 8], 5).0);

        assert_eq!(split_user("bob"), (String::new(), "bob".into()));
        assert!(parse_challenge("!!").is_err());
        assert!(parse_challenge(&negotiate()).is_err(), "a type 1 message");
        // A target-information field that points outside the message.
        c[44..48].copy_from_slice(&4000u32.to_le_bytes());
        let cut = base64::engine::general_purpose::STANDARD.encode(&c);
        assert!(parse_challenge(&cut).is_err());
        assert!(filetime_now() > 133_000_000_000_000_000);
    }
}
