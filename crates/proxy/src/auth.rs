//! Proxy authentication for a listener (`Proxy-Authorization: Basic`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chosproxy_core::config::ProxyAuth;
use std::sync::Mutex;

/// Checks credentials for one running listener. The last accepted header
/// value is remembered so the (deliberately slow) hash runs once per
/// credential, not once per request.
#[derive(Debug)]
pub(crate) struct AuthGate {
    auth: ProxyAuth,
    accepted: Mutex<Option<Vec<u8>>>,
}

/// Outcome of a credential check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthResult {
    Ok,
    /// No credentials were sent (the normal first request of a client).
    Missing,
    /// Credentials were sent but are wrong or malformed.
    Rejected,
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Decodes `Basic <base64(user:password)>`.
fn parse_basic(value: &[u8]) -> Option<(String, String)> {
    let value = std::str::from_utf8(value).ok()?.trim();
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = STANDARD.decode(token.trim()).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (user, password) = text.split_once(':')?;
    Some((user.to_string(), password.to_string()))
}

impl AuthGate {
    pub(crate) fn new(auth: ProxyAuth) -> AuthGate {
        AuthGate {
            auth,
            accepted: Mutex::new(None),
        }
    }

    fn cached(&self) -> Option<Vec<u8>> {
        match self.accepted.lock() {
            Ok(g) => g.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Checks a `Proxy-Authorization` header value (blocking: may hash).
    pub(crate) fn check_blocking(&self, header: Option<&[u8]>) -> AuthResult {
        let Some(value) = header else {
            return AuthResult::Missing;
        };
        if self.cached().is_some_and(|c| constant_time_eq(&c, value)) {
            return AuthResult::Ok;
        }
        match parse_basic(value) {
            Some((user, password)) if self.auth.verify(&user, &password) => {
                if let Ok(mut g) = self.accepted.lock() {
                    *g = Some(value.to_vec());
                }
                AuthResult::Ok
            }
            _ => AuthResult::Rejected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(user: &str, pass: &str) -> Vec<u8> {
        format!("Basic {}", STANDARD.encode(format!("{user}:{pass}"))).into_bytes()
    }

    #[test]
    fn checks_basic_credentials() {
        let gate = AuthGate::new(ProxyAuth::new("u", "p:w").unwrap());
        assert_eq!(gate.check_blocking(None), AuthResult::Missing);
        assert_eq!(
            gate.check_blocking(Some(&header("u", "p:w"))),
            AuthResult::Ok
        );
        assert_eq!(
            gate.check_blocking(Some(&header("u", "p:w"))),
            AuthResult::Ok,
            "cached"
        );
        assert_eq!(
            gate.check_blocking(Some(&header("u", "x"))),
            AuthResult::Rejected
        );
        assert_eq!(
            gate.check_blocking(Some(b"Bearer abc")),
            AuthResult::Rejected
        );
        assert_eq!(
            gate.check_blocking(Some(b"Basic !!!")),
            AuthResult::Rejected
        );
        assert_eq!(
            gate.check_blocking(Some(
                format!("basic {}", STANDARD.encode("u:p:w")).as_bytes()
            )),
            AuthResult::Ok
        );
    }
}
