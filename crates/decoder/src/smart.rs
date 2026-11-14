//! Smart decode: repeatedly picks the most plausible decoding step for the
//! current value until nothing applies.

use crate::codecs::jwt_parts;
use crate::{Operation, apply, is_text};

/// Most steps proposed.
const MAX_STEPS: usize = 10;

fn has_percent_escape(s: &[u8]) -> bool {
    s.windows(3)
        .any(|w| w[0] == b'%' && w[1].is_ascii_hexdigit() && w[2].is_ascii_hexdigit())
}

fn has_html_entity(s: &[u8]) -> bool {
    let text = String::from_utf8_lossy(s);
    text.match_indices('&').any(|(i, _)| {
        let rest = &text[i + 1..];
        rest.find(';').is_some_and(|end| {
            end > 0
                && end <= 10
                && rest[..end]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '#')
        })
    })
}

fn has_unicode_escape(s: &[u8]) -> bool {
    s.windows(2)
        .any(|w| w[0] == b'\\' && matches!(w[1], b'u' | b'x'))
}

fn is_gzip(s: &[u8]) -> bool {
    s.starts_with(&[0x1f, 0x8b])
}

fn is_zlib(s: &[u8]) -> bool {
    s.len() > 2 && s[0] & 0x0f == 8 && (u16::from(s[0]) << 8 | u16::from(s[1])) % 31 == 0
}

/// A decoded value worth continuing with.
fn plausible(bytes: &[u8]) -> bool {
    !bytes.is_empty() && (is_text(bytes) || is_gzip(bytes) || is_zlib(bytes))
}

fn base64_shaped(s: &str, url: bool) -> bool {
    s.len() >= 8
        && s.len() % 4 != 1
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || b == b'='
                || if url {
                    matches!(b, b'-' | b'_')
                } else {
                    matches!(b, b'+' | b'/')
                }
        })
        // Plain words and numbers are valid Base64 too; require some mix.
        && s.bytes().any(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
        && s.bytes().any(|b| b.is_ascii_lowercase())
}

fn next_step(current: &[u8]) -> Option<Operation> {
    if jwt_parts(current).is_some() {
        return Some(Operation::JwtDecode);
    }
    if is_gzip(current) {
        return Some(Operation::GzipDecompress);
    }
    if is_zlib(current) && apply(Operation::DeflateDecompress, current).is_ok() {
        return Some(Operation::DeflateDecompress);
    }
    if !is_text(current) {
        return None;
    }
    let trimmed = String::from_utf8_lossy(current).trim().to_string();
    let changes = |op: Operation| -> bool {
        apply(op, current).is_ok_and(|out| out != current && plausible(&out))
    };
    if has_percent_escape(current) && changes(Operation::UrlDecode) {
        return Some(Operation::UrlDecode);
    }
    if has_html_entity(current) && changes(Operation::HtmlDecode) {
        return Some(Operation::HtmlDecode);
    }
    if has_unicode_escape(current) && changes(Operation::UnicodeUnescape) {
        return Some(Operation::UnicodeUnescape);
    }
    if trimmed.len() >= 8
        && trimmed.len().is_multiple_of(2)
        && trimmed.bytes().all(|b| b.is_ascii_hexdigit())
        && apply(Operation::HexDecode, trimmed.as_bytes()).is_ok_and(|o| plausible(&o))
    {
        return Some(Operation::HexDecode);
    }
    for (url, op) in [
        (false, Operation::Base64Decode),
        (true, Operation::Base64UrlDecode),
    ] {
        if base64_shaped(&trimmed, url)
            && apply(op, trimmed.as_bytes()).is_ok_and(|o| plausible(&o))
        {
            return Some(op);
        }
    }
    let looks_json = matches!(trimmed.as_bytes().first(), Some(b'{' | b'['));
    if looks_json && apply(Operation::JsonPretty, current).is_ok_and(|out| out != current) {
        return Some(Operation::JsonPretty);
    }
    None
}

/// Proposes a chain of decoding steps for `input` (possibly empty).
pub fn smart_decode(input: &[u8]) -> Vec<Operation> {
    let mut ops = Vec::new();
    let mut current = input.to_vec();
    while ops.len() < MAX_STEPS {
        let Some(op) = next_step(&current) else {
            break;
        };
        let Ok(next) = apply(op, &current) else {
            break;
        };
        ops.push(op);
        // These produce a final, human-readable form.
        if matches!(op, Operation::JwtDecode | Operation::JsonPretty) || next == current {
            break;
        }
        current = next;
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Operation as O, run_chain};

    fn chain_of(input: &[u8]) -> Vec<O> {
        smart_decode(input)
    }

    #[test]
    fn url_then_base64_then_json() {
        // URL-encoded (padding included) Base64 of a JSON object.
        let b64 = apply(O::Base64Encode, br#"{"user":"alice"}"#).unwrap();
        assert!(b64.ends_with(b"=="));
        let input = apply(O::UrlEncode, &b64).unwrap();
        let ops = chain_of(&input);
        assert_eq!(ops, vec![O::UrlDecode, O::Base64Decode, O::JsonPretty]);
        let out = run_chain(&input, &ops);
        let last = String::from_utf8(out.last().unwrap().clone().unwrap()).unwrap();
        assert!(last.contains("\"user\": \"alice\""));
    }

    #[test]
    fn gzip_hex_and_jwt() {
        let gz = apply(O::GzipCompress, b"secret text").unwrap();
        let b64 = apply(O::Base64Encode, &gz).unwrap();
        assert_eq!(chain_of(&b64), vec![O::Base64Decode, O::GzipDecompress]);
        assert_eq!(chain_of(b"68656c6c6f20776f726c64"), vec![O::HexDecode]);
        let jwt = b"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIn0.c2ln";
        assert_eq!(chain_of(jwt), vec![O::JwtDecode]);
    }

    #[test]
    fn entities_and_escapes() {
        assert_eq!(chain_of(b"&lt;script&gt;"), vec![O::HtmlDecode]);
        assert_eq!(chain_of(br"caf\u00e9"), vec![O::UnicodeUnescape]);
    }

    #[test]
    fn leaves_plain_values_alone() {
        for plain in [
            &b"hello world"[..],
            b"password",
            b"12345678",
            b"plain-text_value",
        ] {
            assert!(
                chain_of(plain).is_empty(),
                "{:?}",
                String::from_utf8_lossy(plain)
            );
        }
        assert!(chain_of(b"").is_empty());
    }
}
