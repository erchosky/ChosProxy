//! Decoder results prepared for display (bytes travel as Base64).

use crate::AppError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chosproxy_decoder::{Operation, is_text, run_chain, smart_decode};
use serde::Serialize;
use ts_rs::TS;

/// Output shown as text at most this long (the Base64 copy is always full).
const TEXT_LIMIT: usize = 2 * 1024 * 1024;

/// The result of one step of a chain.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DecodeStep {
    /// The operation.
    pub op: Operation,
    /// Output bytes, Base64 (empty on error).
    pub output_base64: String,
    /// Output as text when it is text.
    pub output_text: Option<String>,
    /// Output size.
    #[ts(type = "number")]
    pub size: u64,
    /// Why the step failed (the chain stops there).
    pub error: Option<String>,
}

/// A chain proposed by smart decode, with its results.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SmartDecode {
    /// Proposed operations (empty: nothing recognised).
    pub ops: Vec<Operation>,
    /// Their results.
    pub steps: Vec<DecodeStep>,
}

fn input_bytes(input_base64: &str) -> Result<Vec<u8>, AppError> {
    STANDARD
        .decode(input_base64)
        .map_err(|e| AppError::Invalid(format!("invalid input encoding: {e}")))
}

/// Runs a chain on Base64 input.
pub(crate) fn decode(input_base64: &str, ops: &[Operation]) -> Result<Vec<DecodeStep>, AppError> {
    if ops.len() > 32 {
        return Err(AppError::Invalid("at most 32 steps".into()));
    }
    let input = input_bytes(input_base64)?;
    Ok(ops
        .iter()
        .zip(run_chain(&input, ops))
        .map(|(op, result)| match result {
            Ok(bytes) => DecodeStep {
                op: *op,
                output_text: (bytes.len() <= TEXT_LIMIT && is_text(&bytes))
                    .then(|| String::from_utf8_lossy(&bytes).into_owned()),
                size: bytes.len() as u64,
                output_base64: STANDARD.encode(&bytes),
                error: None,
            },
            Err(e) => DecodeStep {
                op: *op,
                output_base64: String::new(),
                output_text: None,
                size: 0,
                error: Some(e.to_string()),
            },
        })
        .collect())
}

/// Proposes and runs a decoding chain.
pub(crate) fn smart(input_base64: &str) -> Result<SmartDecode, AppError> {
    let ops = smart_decode(&input_bytes(input_base64)?);
    let steps = decode(input_base64, &ops)?;
    Ok(SmartDecode { ops, steps })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_chains_over_base64() {
        let steps = decode(
            &STANDARD.encode(b"a%2Bb"),
            &[Operation::UrlDecode, Operation::HexEncode],
        )
        .unwrap();
        assert_eq!(steps[0].output_text.as_deref(), Some("a+b"));
        assert_eq!(steps[1].output_text.as_deref(), Some("612b62"));
        let bin = decode(&STANDARD.encode(b"00ff"), &[Operation::HexDecode]).unwrap();
        assert_eq!(bin[0].output_text, None);
        assert_eq!(bin[0].output_base64, STANDARD.encode([0u8, 255]));
        let bad = decode(&STANDARD.encode(b"x"), &[Operation::HexDecode]).unwrap();
        assert!(bad[0].error.is_some());
        assert!(decode("***", &[]).is_err());
        let s = smart(&STANDARD.encode(b"&lt;b&gt;")).unwrap();
        assert_eq!(s.ops, vec![Operation::HtmlDecode]);
    }
}
