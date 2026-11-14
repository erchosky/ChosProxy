//! The sequencer's pure parts (spec §19): where a token is in a response
//! ([`TokenLocation`], [`suggest`]) and statistics on a token sample
//! ([`analyze`]). Capture, storage and the task live in the engine.
//!
//! The analysis describes a sample; it does not certify a generator. Each
//! measurement is reported as an observation with its explanation, and no
//! single metric decides "secure" or "insecure".

mod extract;
mod stats;

pub use extract::{MAX_TOKEN_CHARS, TokenCandidate, TokenLocation, suggest};
pub use stats::{
    BitStat, ChiSquare, LengthCount, MAX_TOKENS, Observation, ObservationLevel, PositionStat,
    TokenAnalysis, analyze,
};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most tokens one capture may collect.
pub const MAX_CAPTURE: u32 = 20_000;

/// A token capture: the request to repeat and where the token is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct CaptureConfig {
    /// `http` or `https`.
    pub scheme: String,
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Request text (`\n` line endings), sent as is each time.
    pub request: String,
    /// Send the head with CRLF line endings.
    pub head_crlf: bool,
    /// Send the body with CRLF line endings.
    pub body_crlf: bool,
    /// Where the token is in each response.
    pub location: TokenLocation,
    /// Tokens to collect.
    pub count: u32,
    /// Requests in flight at once.
    pub concurrency: u32,
    /// Requests started per second (0 = no limit).
    pub requests_per_second: u32,
    /// Seconds to wait for each response.
    pub timeout_secs: u32,
}

impl Default for CaptureConfig {
    fn default() -> CaptureConfig {
        CaptureConfig {
            scheme: "http".into(),
            host: "localhost".into(),
            port: 80,
            request: String::new(),
            head_crlf: true,
            body_crlf: true,
            location: TokenLocation::Cookie {
                name: String::new(),
            },
            count: 500,
            concurrency: 4,
            requests_per_second: 20,
            timeout_secs: 15,
        }
    }
}

impl CaptureConfig {
    /// Checks the limits, the request and the location.
    pub fn validate(&self) -> Result<(), String> {
        let ranges: [(&str, u32, u32, u32); 4] = [
            ("number of tokens", self.count, 2, MAX_CAPTURE),
            ("concurrency", self.concurrency, 1, 16),
            ("requests per second", self.requests_per_second, 0, 200),
            ("timeout", self.timeout_secs, 1, 300),
        ];
        for (name, value, min, max) in ranges {
            if !(min..=max).contains(&value) {
                return Err(format!("the {name} must be {min}–{max}"));
            }
        }
        if self.request.trim().is_empty() {
            return Err("the request is empty".into());
        }
        self.location.validate()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn capture_settings_are_checked() {
        let mut c = CaptureConfig {
            request: "GET / HTTP/1.1\n\n".into(),
            location: TokenLocation::Cookie { name: "sid".into() },
            ..CaptureConfig::default()
        };
        assert!(c.validate().is_ok());
        c.count = 1;
        assert!(c.validate().unwrap_err().contains("number of tokens"));
        c.count = 100;
        c.location = TokenLocation::Cookie {
            name: String::new(),
        };
        assert!(c.validate().is_err());
    }
}
