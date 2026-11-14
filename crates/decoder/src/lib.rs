//! Decoder: reversible encodings, compression, hashes and JWT inspection,
//! applied one by one or as a chain, plus "smart decode" that proposes a
//! chain for an unknown value. Everything works on bytes and never touches
//! the network.

mod codecs;
mod smart;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use smart::smart_decode;

/// Largest input accepted by one operation.
pub const MAX_INPUT: usize = 16 * 1024 * 1024;
/// Largest output a decompression may produce (bomb guard).
pub const MAX_DECOMPRESSED: usize = 32 * 1024 * 1024;

/// One transformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Operation {
    /// `%XX` and `+` → bytes.
    UrlDecode,
    /// Percent-encode everything except unreserved characters.
    UrlEncode,
    /// Percent-encode every byte.
    UrlEncodeAll,
    /// Standard Base64 → bytes (padding optional, whitespace ignored).
    Base64Decode,
    /// Bytes → standard Base64.
    Base64Encode,
    /// URL-safe Base64 → bytes.
    Base64UrlDecode,
    /// Bytes → URL-safe Base64 without padding.
    Base64UrlEncode,
    /// Hex digits → bytes (whitespace, `:` and `0x` ignored).
    HexDecode,
    /// Bytes → lowercase hex.
    HexEncode,
    /// HTML entities → characters.
    HtmlDecode,
    /// `& < > " '` → entities.
    HtmlEncode,
    /// `\uXXXX`, `\u{…}`, `\xHH` and C escapes → characters.
    UnicodeUnescape,
    /// Non-ASCII and control characters → `\uXXXX`.
    UnicodeEscape,
    /// gzip → bytes.
    GzipDecompress,
    /// Bytes → gzip.
    GzipCompress,
    /// zlib or raw deflate → bytes.
    DeflateDecompress,
    /// Bytes → zlib.
    DeflateCompress,
    /// Re-indent JSON (key order kept).
    JsonPretty,
    /// Show a JWT's header and payload (the signature is not verified).
    JwtDecode,
    /// MD5 digest, hex.
    Md5,
    /// SHA-1 digest, hex.
    Sha1,
    /// SHA-256 digest, hex.
    Sha256,
    /// SHA-512 digest, hex.
    Sha512,
}

impl Operation {
    /// Every operation, in display order.
    pub const ALL: [Operation; 23] = [
        Operation::UrlDecode,
        Operation::UrlEncode,
        Operation::UrlEncodeAll,
        Operation::Base64Decode,
        Operation::Base64Encode,
        Operation::Base64UrlDecode,
        Operation::Base64UrlEncode,
        Operation::HexDecode,
        Operation::HexEncode,
        Operation::HtmlDecode,
        Operation::HtmlEncode,
        Operation::UnicodeUnescape,
        Operation::UnicodeEscape,
        Operation::GzipDecompress,
        Operation::GzipCompress,
        Operation::DeflateDecompress,
        Operation::DeflateCompress,
        Operation::JsonPretty,
        Operation::JwtDecode,
        Operation::Md5,
        Operation::Sha1,
        Operation::Sha256,
        Operation::Sha512,
    ];
}

/// Why an operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The input is not valid for this operation.
    #[error("{0}")]
    Invalid(String),
    /// Input or output exceeds the limits.
    #[error("{0}")]
    TooLarge(String),
}

/// Applies one operation.
pub fn apply(op: Operation, input: &[u8]) -> Result<Vec<u8>, DecodeError> {
    if input.len() > MAX_INPUT {
        return Err(DecodeError::TooLarge(format!(
            "input larger than {} MiB",
            MAX_INPUT / (1024 * 1024)
        )));
    }
    codecs::apply(op, input)
}

/// Applies `ops` in order. Returns one result per operation that ran; the
/// chain stops at the first failure.
pub fn run_chain(input: &[u8], ops: &[Operation]) -> Vec<Result<Vec<u8>, DecodeError>> {
    let mut out = Vec::with_capacity(ops.len());
    let mut current = input.to_vec();
    for op in ops {
        match apply(*op, &current) {
            Ok(next) => {
                current = next.clone();
                out.push(Ok(next));
            }
            Err(e) => {
                out.push(Err(e));
                break;
            }
        }
    }
    out
}

/// True when `bytes` is UTF-8 text with few control characters.
pub fn is_text(bytes: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(bytes) else {
        return false;
    };
    if s.is_empty() {
        return true;
    }
    let total = s.chars().count();
    let controls = s
        .chars()
        .filter(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        .count();
    controls * 20 <= total
}
