//! Platform authentication (M24, matrix PROXY-017): a 401 challenge from a
//! host with a rule is answered once with Basic (RFC 7617) or Digest
//! (RFC 7616: MD5 or SHA-256, `qop=auth`) using a keyring credential. The
//! credentials are set process-wide by the engine, like the network
//! settings.

use base64::Engine as _;
use chosproxy_core::config::{AuthScheme, host_matches};
use chosproxy_http::ResponseHead;
use md5::Digest as _;
use ring::rand::{SecureRandom, SystemRandom};
use std::sync::{Arc, RwLock};

/// A rule with its secret resolved.
#[derive(Clone)]
pub struct PlatformCredential {
    /// `example.com` or `*.example.com`.
    pub host: String,
    /// Scheme it answers.
    pub scheme: AuthScheme,
    /// User name.
    pub username: String,
    /// Password.
    pub password: String,
}

impl std::fmt::Debug for PlatformCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformCredential")
            .field("host", &self.host)
            .field("scheme", &self.scheme)
            .finish_non_exhaustive()
    }
}

static AUTH: RwLock<Option<Arc<Vec<PlatformCredential>>>> = RwLock::new(None);

/// Replaces the platform authentication credentials.
pub fn set_platform_auth(credentials: Vec<PlatformCredential>) {
    let value = Some(Arc::new(credentials));
    match AUTH.write() {
        Ok(mut g) => *g = value,
        Err(p) => *p.into_inner() = value,
    }
}

fn current() -> Arc<Vec<PlatformCredential>> {
    let v = match AUTH.read() {
        Ok(g) => g.clone(),
        Err(p) => p.into_inner().clone(),
    };
    v.unwrap_or_default()
}

/// Splits `key=value, key="quoted, value"` parameters.
fn params(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq]
            .trim()
            .trim_start_matches(',')
            .trim()
            .to_ascii_lowercase();
        rest = rest[eq + 1..].trim_start();
        let value;
        if let Some(r) = rest.strip_prefix('"') {
            let mut v = String::new();
            let mut chars = r.char_indices();
            let mut end = r.len();
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => {
                        if let Some((_, n)) = chars.next() {
                            v.push(n);
                        }
                    }
                    '"' => {
                        end = i + 1;
                        break;
                    }
                    c => v.push(c),
                }
            }
            value = v;
            rest = r[end.min(r.len())..].trim_start();
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            value = rest[..end].trim().to_string();
            rest = &rest[end..];
        }
        rest = rest.trim_start_matches(',').trim_start();
        out.push((key, value));
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash(algorithm: &str, data: &str) -> String {
    if algorithm.eq_ignore_ascii_case("SHA-256") {
        hex(ring::digest::digest(&ring::digest::SHA256, data.as_bytes()).as_ref())
    } else {
        hex(&md5::Md5::digest(data.as_bytes()))
    }
}

fn request_line(request: &[u8]) -> Option<(String, String)> {
    let end = request.iter().position(|b| *b == b'\n')?;
    let line = String::from_utf8_lossy(&request[..end]);
    let mut parts = line.split_whitespace();
    Some((parts.next()?.to_string(), parts.next()?.to_string()))
}

fn has_authorization(request: &[u8]) -> bool {
    String::from_utf8_lossy(request)
        .split("\n")
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .any(|l| l.to_ascii_lowercase().starts_with("authorization:"))
}

/// The `Authorization` value answering `challenge` (one `WWW-Authenticate`
/// value) for this request, if the scheme matches.
pub(crate) fn authorization(
    cred: &PlatformCredential,
    challenge: &str,
    method: &str,
    uri: &str,
    cnonce: &str,
) -> Option<String> {
    let (scheme, rest) = challenge
        .trim()
        .split_once(' ')
        .unwrap_or((challenge.trim(), ""));
    match cred.scheme {
        AuthScheme::Basic if scheme.eq_ignore_ascii_case("basic") => {
            let token = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", cred.username, cred.password));
            Some(format!("Basic {token}"))
        }
        AuthScheme::Digest if scheme.eq_ignore_ascii_case("digest") => {
            let p = params(rest);
            let get = |k: &str| p.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
            let realm = get("realm").unwrap_or("");
            let nonce = get("nonce")?;
            let algorithm = get("algorithm").unwrap_or("MD5");
            if !(algorithm.eq_ignore_ascii_case("MD5") || algorithm.eq_ignore_ascii_case("SHA-256"))
            {
                return None;
            }
            let qop_auth = get("qop").is_some_and(|q| q.split(',').any(|x| x.trim() == "auth"));
            let ha1 = hash(
                algorithm,
                &format!("{}:{realm}:{}", cred.username, cred.password),
            );
            let ha2 = hash(algorithm, &format!("{method}:{uri}"));
            let nc = "00000001";
            let response = if qop_auth {
                hash(
                    algorithm,
                    &format!("{ha1}:{nonce}:{nc}:{cnonce}:auth:{ha2}"),
                )
            } else {
                hash(algorithm, &format!("{ha1}:{nonce}:{ha2}"))
            };
            let mut v = format!(
                "Digest username=\"{}\", realm=\"{realm}\", nonce=\"{nonce}\", uri=\"{uri}\", algorithm={algorithm}, response=\"{response}\"",
                cred.username.replace('"', "\\\"")
            );
            if qop_auth {
                v.push_str(&format!(", qop=auth, nc={nc}, cnonce=\"{cnonce}\""));
            }
            if let Some(opaque) = get("opaque") {
                v.push_str(&format!(", opaque=\"{opaque}\""));
            }
            Some(v)
        }
        _ => None,
    }
}

/// The request to send again after a 401 from `host`, with an
/// `Authorization` header added, and the scheme used. `None` when no rule
/// applies or the request already carried credentials.
pub(crate) fn retry_for(
    request: &[u8],
    response: &ResponseHead,
    host: &str,
) -> Option<(Vec<u8>, &'static str)> {
    if response.status != 401 || has_authorization(request) {
        return None;
    }
    let creds = current();
    let (method, uri) = request_line(request)?;
    let mut nonce = [0u8; 8];
    let _ = SystemRandom::new().fill(&mut nonce);
    let cnonce = hex(&nonce);
    // Several rules may cover the host (say Basic and Digest): the first
    // one that answers a challenge of the response is used.
    let (cred, value) = creds
        .iter()
        .filter(|c| host_matches(&c.host, host))
        .find_map(|cred| {
            response
                .headers
                .iter()
                .filter(|h| h.name.eq_ignore_ascii_case("www-authenticate"))
                .find_map(|h| {
                    authorization(
                        cred,
                        &String::from_utf8_lossy(&h.value),
                        &method,
                        &uri,
                        &cnonce,
                    )
                })
                .map(|v| (cred, v))
        })?;
    let out = with_authorization(request, &value, false)?;
    let scheme = match cred.scheme {
        AuthScheme::Basic => "Basic",
        AuthScheme::Digest => "Digest",
        AuthScheme::Ntlm => "NTLM",
    };
    Some((out, scheme))
}

/// `request` with an `Authorization` header after the request line.
/// `keep_open` also drops a `Connection: close` header, for an exchange
/// that needs the same connection again.
pub(crate) fn with_authorization(request: &[u8], value: &str, keep_open: bool) -> Option<Vec<u8>> {
    let line_end = request.iter().position(|b| *b == b'\n')? + 1;
    let eol: &[u8] = if request[..line_end].ends_with(b"\r\n") {
        b"\r\n"
    } else {
        b"\n"
    };
    let mut out = request[..line_end].to_vec();
    out.extend_from_slice(b"Authorization: ");
    out.extend_from_slice(value.as_bytes());
    out.extend_from_slice(eol);
    let rest = &request[line_end..];
    if !keep_open {
        out.extend_from_slice(rest);
        return Some(out);
    }
    // Header lines up to the blank line, without `Connection: close`.
    let mut at = 0;
    while at < rest.len() {
        let end = rest[at..]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(rest.len(), |i| at + i + 1);
        let line = &rest[at..end];
        if line == b"\r\n" || line == b"\n" {
            break;
        }
        let text = String::from_utf8_lossy(line).to_ascii_lowercase();
        let is_close = text
            .split_once(':')
            .is_some_and(|(n, v)| n.trim() == "connection" && v.trim() == "close");
        if !is_close {
            out.extend_from_slice(line);
        }
        at = end;
    }
    out.extend_from_slice(&rest[at..]);
    Some(out)
}

/// The NTLM credential for a 401 from `host` that offers NTLM, when the
/// request carried no credentials of its own.
pub(crate) fn ntlm_for(
    request: &[u8],
    response: &ResponseHead,
    host: &str,
) -> Option<PlatformCredential> {
    if response.status != 401 || has_authorization(request) || ntlm_challenge(response).is_none() {
        return None;
    }
    current()
        .iter()
        .find(|c| c.scheme == AuthScheme::Ntlm && host_matches(&c.host, host))
        .cloned()
}

/// What follows `NTLM` in a `WWW-Authenticate` header of the response:
/// empty for the bare offer, the challenge message in the second step.
pub(crate) fn ntlm_challenge(response: &ResponseHead) -> Option<String> {
    response
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("www-authenticate"))
        .find_map(|h| {
            let value = String::from_utf8_lossy(&h.value).trim().to_string();
            let (scheme, rest) = value.split_once(' ').unwrap_or((value.as_str(), ""));
            scheme
                .eq_ignore_ascii_case("ntlm")
                .then(|| rest.trim().to_string())
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn cred(scheme: AuthScheme) -> PlatformCredential {
        PlatformCredential {
            host: "lab.test".into(),
            scheme,
            username: "Mufasa".into(),
            password: "Circle of Life".into(),
        }
    }

    #[test]
    fn parses_challenge_parameters() {
        let p = params(r#"realm="a, b", qop="auth,auth-int", nonce=abc, opaque="x\"y""#);
        assert_eq!(p[0], ("realm".into(), "a, b".into()));
        assert_eq!(p[1], ("qop".into(), "auth,auth-int".into()));
        assert_eq!(p[2], ("nonce".into(), "abc".into()));
        assert_eq!(p[3], ("opaque".into(), "x\"y".into()));
    }

    #[test]
    fn digest_matches_rfc_7616_example() {
        // RFC 7616 §3.9.1 (MD5 and SHA-256), with its client nonce.
        let challenge = r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=MD5, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#;
        let cnonce = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";
        let v = authorization(
            &cred(AuthScheme::Digest),
            challenge,
            "GET",
            "/dir/index.html",
            cnonce,
        )
        .unwrap();
        assert!(
            v.contains(r#"response="8ca523f5e9506fed4657c9700eebdbec""#),
            "{v}"
        );
        let sha = challenge.replace("algorithm=MD5", "algorithm=SHA-256");
        let v = authorization(
            &cred(AuthScheme::Digest),
            &sha,
            "GET",
            "/dir/index.html",
            cnonce,
        )
        .unwrap();
        assert!(
            v.contains(
                r#"response="753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1""#
            ),
            "{v}"
        );
        assert!(v.contains("opaque=\"FQhe"));
    }

    #[test]
    fn basic_and_retry_request() {
        assert_eq!(
            authorization(
                &cred(AuthScheme::Basic),
                r#"Basic realm="x""#,
                "GET",
                "/",
                "c"
            )
            .unwrap(),
            "Basic TXVmYXNhOkNpcmNsZSBvZiBMaWZl"
        );
        assert!(
            authorization(&cred(AuthScheme::Basic), "Digest nonce=1", "GET", "/", "c").is_none()
        );
        set_platform_auth(vec![cred(AuthScheme::Basic)]);
        let head = ResponseHead::parse(
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"lab\"\r\n\r\n".to_vec(),
        )
        .unwrap();
        let (retry, scheme) = retry_for(
            b"GET /x HTTP/1.1\r\nHost: lab.test\r\n\r\n",
            &head,
            "lab.test",
        )
        .unwrap();
        assert_eq!(scheme, "Basic");
        assert!(String::from_utf8_lossy(&retry).starts_with(
            "GET /x HTTP/1.1\r\nAuthorization: Basic TXVmYXNhOkNpcmNsZSBvZiBMaWZl\r\nHost: lab.test"
        ));
        assert!(
            retry_for(&retry, &head, "lab.test").is_none(),
            "already has credentials"
        );
        assert!(retry_for(b"GET / HTTP/1.1\r\n\r\n", &head, "other.test").is_none());
        set_platform_auth(Vec::new());
    }
}
