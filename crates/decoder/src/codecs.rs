//! The individual transformations.

use crate::{DecodeError, MAX_DECOMPRESSED, Operation};
use base64::Engine as _;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use md5::Digest as _;
use std::io::{Read, Write};

fn invalid(msg: impl Into<String>) -> DecodeError {
    DecodeError::Invalid(msg.into())
}

const fn lenient(alphabet: &alphabet::Alphabet) -> GeneralPurpose {
    GeneralPurpose::new(
        alphabet,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    )
}
const B64: GeneralPurpose = lenient(&alphabet::STANDARD);
const B64URL: GeneralPurpose = lenient(&alphabet::URL_SAFE);
const B64URL_NOPAD: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_encode_padding(false),
);

fn without_whitespace(input: &[u8]) -> Vec<u8> {
    input
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect()
}

pub(crate) fn hex(bytes: &[u8]) -> Vec<u8> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)]);
        out.push(DIGITS[usize::from(b & 0xf)]);
    }
    out
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn hex_decode(input: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let mut digits: Vec<u8> = input
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace() && *b != b':')
        .collect();
    if digits.starts_with(b"0x") || digits.starts_with(b"0X") {
        digits.drain(..2);
    }
    if !digits.len().is_multiple_of(2) {
        return Err(invalid("hex input has an odd number of digits"));
    }
    digits
        .chunks(2)
        .map(|p| match (hex_value(p[0]), hex_value(p[1])) {
            (Some(h), Some(l)) => Ok(h << 4 | l),
            _ => Err(invalid("hex input contains non-hex characters")),
        })
        .collect()
}

fn url_decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            b'%' if i + 2 < input.len() => {
                match (hex_value(input[i + 1]), hex_value(input[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push(h << 4 | l);
                        i += 3;
                        continue;
                    }
                    _ => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    out
}

fn url_encode(input: &[u8], all: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() * 3);
    for &b in input {
        if !all && (b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~')) {
            out.push(b);
        } else {
            out.push(b'%');
            out.extend(hex(&[b]).to_ascii_uppercase());
        }
    }
    out
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "reg" => '®',
        "euro" => '€',
        "hellip" => '…',
        "ndash" => '–',
        "mdash" => '—',
        "laquo" => '«',
        "raquo" => '»',
        _ => return None,
    })
}

fn html_decode(input: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(input);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_ref();
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let decoded = after.find(';').filter(|end| *end <= 10).and_then(|end| {
            let entity = &after[..end];
            let c = if let Some(num) = entity.strip_prefix('#') {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(h) => u32::from_str_radix(h, 16).ok(),
                    None => num.parse::<u32>().ok(),
                };
                code.and_then(char::from_u32)
            } else {
                named_entity(entity)
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.into_bytes()
}

fn html_encode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    for &b in input {
        match b {
            b'&' => out.extend(b"&amp;"),
            b'<' => out.extend(b"&lt;"),
            b'>' => out.extend(b"&gt;"),
            b'"' => out.extend(b"&quot;"),
            b'\'' => out.extend(b"&#39;"),
            b => out.push(b),
        }
    }
    out
}

fn unicode_unescape(input: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(input);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let hex_at = |from: usize, n: usize| -> Option<u32> {
        let s: String = chars.get(from..from + n)?.iter().collect();
        if s.chars().all(|c| c.is_ascii_hexdigit()) {
            u32::from_str_radix(&s, 16).ok()
        } else {
            None
        }
    };
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' || i + 1 >= chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let (c, used) = match chars[i + 1] {
            'u' if chars.get(i + 2) == Some(&'{') => {
                let end = chars[i + 3..].iter().position(|c| *c == '}');
                match end.and_then(|e| hex_at(i + 3, e).map(|v| (v, e))) {
                    Some((v, e)) => (char::from_u32(v), e + 4),
                    None => (None, 0),
                }
            }
            'u' => match hex_at(i + 2, 4) {
                Some(hi @ 0xD800..=0xDBFF) => {
                    let lo = (chars.get(i + 6) == Some(&'\\') && chars.get(i + 7) == Some(&'u'))
                        .then(|| hex_at(i + 8, 4))
                        .flatten()
                        .filter(|lo| (0xDC00..=0xDFFF).contains(lo));
                    match lo {
                        Some(lo) => (
                            char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)),
                            12,
                        ),
                        None => (None, 0),
                    }
                }
                Some(v) => (char::from_u32(v), 6),
                None => (None, 0),
            },
            'x' => (hex_at(i + 2, 2).and_then(char::from_u32), 4),
            'n' => (Some('\n'), 2),
            'r' => (Some('\r'), 2),
            't' => (Some('\t'), 2),
            '0' => (Some('\0'), 2),
            '\\' => (Some('\\'), 2),
            '"' => (Some('"'), 2),
            '\'' => (Some('\''), 2),
            '/' => (Some('/'), 2),
            _ => (None, 0),
        };
        match c {
            Some(c) if used > 0 => {
                out.push(c);
                i += used;
            }
            _ => {
                out.push('\\');
                i += 1;
            }
        }
    }
    out.into_bytes()
}

fn unicode_escape(input: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(input);
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() && !c.is_ascii_control() {
            out.push(c);
            continue;
        }
        let mut units = [0u16; 2];
        for u in c.encode_utf16(&mut units) {
            out.push_str(&format!("\\u{u:04x}"));
        }
    }
    out.into_bytes()
}

fn read_limited(mut reader: impl Read, what: &str) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::new();
    let limit = u64::try_from(MAX_DECOMPRESSED).unwrap_or(u64::MAX);
    (&mut reader)
        .take(limit + 1)
        .read_to_end(&mut out)
        .map_err(|e| invalid(format!("not valid {what}: {e}")))?;
    if out.len() > MAX_DECOMPRESSED {
        return Err(DecodeError::TooLarge(format!(
            "decompressed data exceeds {} MiB",
            MAX_DECOMPRESSED / (1024 * 1024)
        )));
    }
    Ok(out)
}

fn compress(mut encoder: impl Write, input: &[u8]) -> Result<(), DecodeError> {
    encoder
        .write_all(input)
        .map_err(|e| invalid(format!("compression failed: {e}")))
}

fn json_pretty(input: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let value: serde_json::Value =
        serde_json::from_slice(input).map_err(|e| invalid(format!("not valid JSON: {e}")))?;
    serde_json::to_vec_pretty(&value).map_err(|e| invalid(e.to_string()))
}

pub(crate) fn jwt_parts(input: &[u8]) -> Option<(serde_json::Value, serde_json::Value, usize)> {
    let text = std::str::from_utf8(input).ok()?.trim();
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let decode = |p: &str| -> Option<serde_json::Value> {
        serde_json::from_slice(&B64URL.decode(p).ok()?).ok()
    };
    let header = decode(parts[0]).filter(|h| h.get("alg").is_some())?;
    let payload = decode(parts[1])?;
    let signature = B64URL.decode(parts[2]).ok()?.len();
    Some((header, payload, signature))
}

fn jwt_decode(input: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let (header, payload, signature) =
        jwt_parts(input).ok_or_else(|| invalid("not a JWT (header.payload.signature)"))?;
    let pretty = |v: &serde_json::Value| serde_json::to_string_pretty(v).unwrap_or_default();
    let alg = header.get("alg").and_then(|a| a.as_str()).unwrap_or("?");
    let note = if alg.eq_ignore_ascii_case("none") {
        "alg is \"none\": the token is not signed"
    } else {
        "signature not verified"
    };
    Ok(format!(
        "Header:\n{}\n\nPayload:\n{}\n\nSignature: {signature} bytes ({note})\n",
        pretty(&header),
        pretty(&payload)
    )
    .into_bytes())
}

pub(crate) fn apply(op: Operation, input: &[u8]) -> Result<Vec<u8>, DecodeError> {
    use Operation as O;
    Ok(match op {
        O::UrlDecode => url_decode(input),
        O::UrlEncode => url_encode(input, false),
        O::UrlEncodeAll => url_encode(input, true),
        O::Base64Decode => B64
            .decode(without_whitespace(input))
            .map_err(|e| invalid(format!("not valid Base64: {e}")))?,
        O::Base64Encode => B64.encode(input).into_bytes(),
        O::Base64UrlDecode => B64URL
            .decode(without_whitespace(input))
            .map_err(|e| invalid(format!("not valid URL-safe Base64: {e}")))?,
        O::Base64UrlEncode => B64URL_NOPAD.encode(input).into_bytes(),
        O::HexDecode => hex_decode(input)?,
        O::HexEncode => hex(input),
        O::HtmlDecode => html_decode(input),
        O::HtmlEncode => html_encode(input),
        O::UnicodeUnescape => unicode_unescape(input),
        O::UnicodeEscape => unicode_escape(input),
        O::GzipDecompress => read_limited(flate2::read::MultiGzDecoder::new(input), "gzip")?,
        O::GzipCompress => {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            compress(&mut e, input)?;
            e.finish().map_err(|e| invalid(e.to_string()))?
        }
        O::DeflateDecompress => read_limited(flate2::read::ZlibDecoder::new(input), "zlib")
            .or_else(|_| read_limited(flate2::read::DeflateDecoder::new(input), "deflate"))?,
        O::DeflateCompress => {
            let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            compress(&mut e, input)?;
            e.finish().map_err(|e| invalid(e.to_string()))?
        }
        O::JsonPretty => json_pretty(input)?,
        O::JwtDecode => jwt_decode(input)?,
        O::Md5 => hex(&md5::Md5::digest(input)),
        O::Sha1 => {
            hex(ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, input).as_ref())
        }
        O::Sha256 => hex(ring::digest::digest(&ring::digest::SHA256, input).as_ref()),
        O::Sha512 => hex(ring::digest::digest(&ring::digest::SHA512, input).as_ref()),
    })
}

#[cfg(test)]
mod tests {
    use crate::{Operation as O, apply, run_chain};

    fn ok(op: O, input: &[u8]) -> String {
        String::from_utf8(apply(op, input).unwrap()).unwrap()
    }

    #[test]
    fn url() {
        assert_eq!(ok(O::UrlDecode, b"a%20b+c%2Fd%zz%4"), "a b c/d%zz%4");
        assert_eq!(ok(O::UrlEncode, "a b/é~".as_bytes()), "a%20b%2F%C3%A9~");
        assert_eq!(ok(O::UrlEncodeAll, b"ab"), "%61%62");
    }

    #[test]
    fn base64_and_hex() {
        assert_eq!(ok(O::Base64Encode, b"hello?>"), "aGVsbG8/Pg==");
        assert_eq!(ok(O::Base64Decode, b"aGVsbG8/Pg"), "hello?>");
        assert_eq!(ok(O::Base64UrlEncode, b"hello?>"), "aGVsbG8_Pg");
        assert_eq!(ok(O::Base64UrlDecode, b"aGVsbG8_Pg=="), "hello?>");
        assert!(apply(O::Base64Decode, b"***").is_err());
        assert_eq!(ok(O::HexEncode, b"\x00\xffA"), "00ff41");
        assert_eq!(ok(O::HexDecode, b"0x48 65:6c6C6f"), "Hello");
        assert!(apply(O::HexDecode, b"abc").is_err());
        assert!(apply(O::HexDecode, b"zz").is_err());
    }

    #[test]
    fn html_and_unicode() {
        assert_eq!(
            ok(
                O::HtmlDecode,
                b"&lt;a href=&quot;x&quot;&gt;&#65;&#x42;&amp;&bogus; & done"
            ),
            "<a href=\"x\">AB&&bogus; & done"
        );
        assert_eq!(ok(O::HtmlEncode, b"<'&\">"), "&lt;&#39;&amp;&quot;&gt;");
        assert_eq!(
            ok(
                O::UnicodeUnescape,
                br#"caf\u00e9 \ud83d\ude00 \u{1F600} \x41\n\q"#
            ),
            "café 😀 😀 A\n\\q"
        );
        assert_eq!(
            ok(O::UnicodeEscape, "é😀\n".as_bytes()),
            "\\u00e9\\ud83d\\ude00\\u000a"
        );
        assert_eq!(
            ok(
                O::UnicodeUnescape,
                ok(O::UnicodeEscape, "é😀".as_bytes()).as_bytes()
            ),
            "é😀"
        );
    }

    #[test]
    fn compression_round_trips() {
        for (c, d) in [
            (O::GzipCompress, O::GzipDecompress),
            (O::DeflateCompress, O::DeflateDecompress),
        ] {
            let out = run_chain(b"payload payload payload", &[c, d]);
            assert_eq!(out[1].as_ref().unwrap(), b"payload payload payload");
        }
        assert!(apply(O::GzipDecompress, b"not gzip").is_err());
    }

    #[test]
    fn json_keeps_key_order() {
        assert_eq!(
            ok(O::JsonPretty, br#"{"z":1,"a":[true]}"#),
            "{\n  \"z\": 1,\n  \"a\": [\n    true\n  ]\n}"
        );
        assert!(apply(O::JsonPretty, b"{").is_err());
    }

    #[test]
    fn jwt() {
        // {"alg":"HS256","typ":"JWT"} . {"sub":"1234567890","name":"lab"} . sig
        let token = b"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6ImxhYiJ9.c2lnbmF0dXJl";
        let out = ok(O::JwtDecode, token);
        assert!(out.contains("\"alg\": \"HS256\""));
        assert!(out.contains("\"name\": \"lab\""));
        assert!(out.contains("Signature: 9 bytes (signature not verified)"));
        assert!(apply(O::JwtDecode, b"a.b").is_err());
    }

    #[test]
    fn hashes() {
        assert_eq!(ok(O::Md5, b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            ok(O::Sha1, b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            ok(O::Sha256, b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(ok(O::Sha512, b"").len(), 128);
    }

    #[test]
    fn chain_stops_at_first_error() {
        let out = run_chain(b"%7A%7A", &[O::UrlDecode, O::HexDecode, O::Base64Encode]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].as_ref().unwrap(), b"zz");
        assert!(out[1].is_err());
    }
}
