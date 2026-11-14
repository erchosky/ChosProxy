//! `ai.toml`: ChosAI (M29, ADR-0039). Off by default. A local model is
//! first-class; an external provider is an explicit choice.

use super::ConfigSection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Whether and where ChosAI runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AiMode {
    /// No model is ever called.
    #[default]
    Disabled,
    /// A model on this machine (the URL must be loopback).
    Local,
    /// A provider elsewhere: every call needs a previewed context.
    External,
}

/// How to talk to the model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AiProvider {
    /// Ollama's `/api/chat`.
    #[default]
    Ollama,
    /// `/v1/chat/completions` (LM Studio, llama.cpp server, vLLM and
    /// hosted services with that API).
    OpenAiCompatible,
}

/// ChosAI settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct AiConfig {
    /// Off, local or external.
    pub mode: AiMode,
    /// API style.
    pub provider: AiProvider,
    /// Base URL, e.g. `http://127.0.0.1:11434` (Ollama) or
    /// `http://127.0.0.1:1234` (LM Studio).
    pub base_url: String,
    /// Model name.
    pub model: String,
    /// Keyring credential holding the API key (its secret), if any.
    pub api_key_credential: Option<String>,
    /// Seconds to wait for an answer.
    pub timeout_secs: u32,
    /// Most characters of context sent per question.
    pub max_context_chars: u32,
    /// Extra regular expressions whose matches are redacted.
    pub redact_patterns: Vec<String>,
}

impl Default for AiConfig {
    fn default() -> AiConfig {
        AiConfig {
            mode: AiMode::Disabled,
            provider: AiProvider::Ollama,
            base_url: "http://127.0.0.1:11434".into(),
            model: "llama3.1".into(),
            api_key_credential: None,
            timeout_secs: 120,
            max_context_chars: 24_000,
            redact_patterns: Vec::new(),
        }
    }
}

/// Whether a URL's host is this machine.
fn loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

impl AiConfig {
    /// `(https, host, port, base path)` of the base URL.
    pub fn endpoint(&self) -> Result<(bool, String, u16, String), String> {
        let url = self.base_url.trim();
        let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = url.strip_prefix("http://") {
            (false, r)
        } else {
            return Err("the model URL must start with http:// or https://".into());
        };
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], rest[i..].trim_end_matches('/').to_string()),
            None => (rest, String::new()),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) if !p.contains(']') => (
                h.to_string(),
                p.parse()
                    .map_err(|_| "the model URL has a bad port".to_string())?,
            ),
            _ => (authority.to_string(), if https { 443 } else { 80 }),
        };
        if host.is_empty() {
            return Err("the model URL has no host".into());
        }
        Ok((
            https,
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .to_string(),
            port,
            path,
        ))
    }

    /// Whether the configured URL points at this machine.
    pub fn is_loopback(&self) -> bool {
        self.endpoint()
            .is_ok_and(|(_, host, _, _)| loopback_host(&host))
    }
}

impl ConfigSection for AiConfig {
    const FILE_NAME: &'static str = "ai.toml";

    fn validate(&self) -> Result<(), String> {
        let (https, _, _, _) = self.endpoint()?;
        if self.mode == AiMode::Local && !self.is_loopback() {
            return Err(
                "local mode needs a model on this machine (127.0.0.1, ::1 or localhost); \
                 choose external mode for other hosts"
                    .into(),
            );
        }
        if self.mode == AiMode::External && !https && !self.is_loopback() {
            return Err("an external provider must be reached over https".into());
        }
        if self.model.trim().is_empty() || self.model.len() > 200 {
            return Err("give a model name".into());
        }
        if !(5..=900).contains(&self.timeout_secs) {
            return Err("the timeout must be between 5 and 900 seconds".into());
        }
        if !(1000..=400_000).contains(&self.max_context_chars) {
            return Err("the context limit must be between 1000 and 400000 characters".into());
        }
        if self.redact_patterns.len() > 50 {
            return Err("at most 50 redaction patterns".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_by_default_and_local_means_loopback() {
        let c = AiConfig::default();
        assert_eq!(c.mode, AiMode::Disabled);
        assert!(c.validate().is_ok());
        assert!(c.is_loopback());
        let remote = AiConfig {
            mode: AiMode::Local,
            base_url: "http://10.0.0.5:11434".into(),
            ..AiConfig::default()
        };
        assert!(remote.validate().unwrap_err().contains("local mode"));
        let plain = AiConfig {
            mode: AiMode::External,
            base_url: "http://api.example.com".into(),
            ..AiConfig::default()
        };
        assert!(plain.validate().unwrap_err().contains("https"));
        let ok = AiConfig {
            mode: AiMode::External,
            base_url: "https://api.example.com/v1/".into(),
            provider: AiProvider::OpenAiCompatible,
            ..AiConfig::default()
        };
        assert!(ok.validate().is_ok());
        assert_eq!(
            ok.endpoint().unwrap(),
            (true, "api.example.com".into(), 443, "/v1".into())
        );
        let v6 = AiConfig {
            mode: AiMode::Local,
            base_url: "http://[::1]:1234".into(),
            ..AiConfig::default()
        };
        assert!(v6.validate().is_ok(), "{:?}", v6.endpoint());
        assert!(
            AiConfig {
                base_url: "ftp://x".into(),
                ..AiConfig::default()
            }
            .validate()
            .is_err()
        );
    }
}
