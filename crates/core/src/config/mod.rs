//! Typed configuration sections, each persisted in its own TOML file
//! (see ADR-0005).

mod agent;
mod ai;
mod api;
mod auth;
mod network;
mod oast;
mod plugins;
mod proxy;
mod scripts;
mod settings;
mod store;

pub use agent::{AgentConfig, AgentPermission};
pub use ai::{AiConfig, AiMode, AiProvider};
pub use api::ApiConfig;
pub use auth::{PBKDF2_ITERATIONS, ProxyAuth, validate_username};
pub use network::{
    AuthScheme, HostOverride, NetworkConfig, PlatformAuthRule, Timeouts, UpstreamKind, UpstreamRule,
};
pub use oast::{LocalOast, OastConfig, OastMode, SelfHostedOast};
pub use plugins::{PluginGrant, PluginsConfig, TrustedPublisher};
pub(crate) use proxy::valid_host_pattern;
pub use proxy::{
    InterceptRules, ListenerCertificate, ListenerConfig, MatchReplaceRule, MatchTarget,
    ProxyConfig, RequestFacts, STATIC_EXTENSIONS, TlsConfig, host_matches, rule_regex_matches,
    split_host_port,
};
pub use scripts::{ScriptEntry, ScriptKind, ScriptsConfig, valid_script_id};
pub use settings::{
    AppSettings, CaptureSettings, KeyBinding, LogLevel, LoggingSettings, PassiveSettings,
    SavedFilter, Theme, UiSettings,
};
pub use store::{ConfigError, ConfigStore, Loaded};

/// A configuration section stored in its own file.
pub trait ConfigSection:
    serde::Serialize + serde::de::DeserializeOwned + Default + Clone + Send + Sync + 'static
{
    /// File name inside the config directory, e.g. `"proxy.toml"`.
    const FILE_NAME: &'static str;

    /// Checks semantic constraints that the type system cannot express.
    fn validate(&self) -> Result<(), String>;
}
