use super::ConfigSection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Application-wide settings (`settings.toml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct AppSettings {
    /// Schema version of this file.
    pub version: u32,
    /// Look and feel.
    pub ui: UiSettings,
    /// Application log output.
    pub logging: LoggingSettings,
    /// Limits for traffic capture.
    pub capture: CaptureSettings,
    /// Passive analysis of captured traffic.
    pub passive: PassiveSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            version: 1,
            ui: UiSettings::default(),
            logging: LoggingSettings::default(),
            capture: CaptureSettings::default(),
            passive: PassiveSettings::default(),
        }
    }
}

/// Colour theme. Dark is the default (Kali).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Theme {
    /// Dark theme.
    #[default]
    Dark,
    /// Light theme.
    Light,
    /// Follow the operating system.
    System,
}

/// UI settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct UiSettings {
    /// Colour theme.
    pub theme: Theme,
    /// Named HTTP history filters.
    pub saved_filters: Vec<SavedFilter>,
    /// Keyboard shortcuts changed by the user (others keep their default).
    pub key_bindings: Vec<KeyBinding>,
}

/// A named history filter (the UI's filter, as JSON).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct SavedFilter {
    /// Name.
    pub name: String,
    /// The filter, as the UI stores it (JSON).
    pub filter: String,
}

/// A shortcut chosen by the user for an action.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct KeyBinding {
    /// Action id (e.g. `palette`, `view:history`).
    pub action: String,
    /// Keys, e.g. `Mod+Shift+H` (empty = no shortcut).
    pub keys: String,
}

/// Log verbosity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LogLevel {
    /// Errors only.
    Error,
    /// Errors and warnings.
    Warn,
    /// Normal operation (default).
    #[default]
    Info,
    /// Developer diagnostics.
    Debug,
    /// Everything.
    Trace,
}

impl LogLevel {
    /// Directive understood by `tracing_subscriber::EnvFilter`.
    pub fn as_filter(self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

/// Logging settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct LoggingSettings {
    /// Minimum level written to the log file.
    pub level: LogLevel,
}

/// Capture limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct CaptureSettings {
    /// Maximum bytes of each request/response stored in the project.
    /// Larger messages are still forwarded in full; the stored copy is
    /// truncated and flagged.
    #[ts(type = "number")]
    pub max_message_bytes: u64,
}

impl CaptureSettings {
    /// Lower bound accepted by validation (4 KiB).
    pub const MIN_MESSAGE_BYTES: u64 = 4 * 1024;
    /// Upper bound accepted by validation (256 MiB).
    pub const MAX_MESSAGE_BYTES: u64 = 256 * 1024 * 1024;
}

impl Default for CaptureSettings {
    fn default() -> Self {
        CaptureSettings {
            max_message_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Passive analysis: checks run on traffic that already happened (they never
/// send requests).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct PassiveSettings {
    /// Analyze new traffic automatically.
    pub enabled: bool,
    /// Only traffic in scope (all traffic while no scope is defined).
    pub in_scope_only: bool,
    /// Analyze Repeater traffic too (proxy traffic always is).
    pub repeater: bool,
    /// Ids of checks turned off.
    pub disabled_checks: Vec<String>,
}

impl Default for PassiveSettings {
    fn default() -> Self {
        PassiveSettings {
            enabled: true,
            in_scope_only: true,
            repeater: true,
            disabled_checks: Vec::new(),
        }
    }
}

impl ConfigSection for AppSettings {
    const FILE_NAME: &'static str = "settings.toml";

    fn validate(&self) -> Result<(), String> {
        let m = self.capture.max_message_bytes;
        if !(CaptureSettings::MIN_MESSAGE_BYTES..=CaptureSettings::MAX_MESSAGE_BYTES).contains(&m) {
            return Err(format!(
                "capture.max_message_bytes must be between {} and {}",
                CaptureSettings::MIN_MESSAGE_BYTES,
                CaptureSettings::MAX_MESSAGE_BYTES
            ));
        }
        if self.passive.disabled_checks.len() > 200
            || self.passive.disabled_checks.iter().any(|c| c.len() > 64)
        {
            return Err("passive.disabled_checks has too many or too long entries".into());
        }
        if self.ui.saved_filters.len() > 50
            || self
                .ui
                .saved_filters
                .iter()
                .any(|f| f.name.trim().is_empty() || f.name.len() > 100 || f.filter.len() > 8192)
        {
            return Err("ui.saved_filters: at most 50, each named, filters up to 8 KiB".into());
        }
        if self.ui.key_bindings.len() > 60
            || self
                .ui
                .key_bindings
                .iter()
                .any(|k| k.action.is_empty() || k.action.len() > 40 || k.keys.len() > 40)
        {
            return Err("ui.key_bindings has too many or invalid entries".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(AppSettings::default().validate().is_ok());
        assert_eq!(AppSettings::default().ui.theme, Theme::Dark);
    }

    #[test]
    fn partial_files_fill_defaults() {
        let s: AppSettings = toml::from_str("[ui]\ntheme = \"light\"\n").unwrap();
        assert_eq!(s.ui.theme, Theme::Light);
        assert_eq!(s.logging.level, LogLevel::Info);
        assert!(s.passive.enabled && s.passive.in_scope_only);
        let p: AppSettings =
            toml::from_str("[passive]\nenabled = false\ndisabled_checks = [\"csp\"]\n").unwrap();
        assert!(!p.passive.enabled && p.passive.repeater);
        assert_eq!(p.passive.disabled_checks, vec!["csp"]);
    }

    #[test]
    fn unknown_fields_rejected() {
        assert!(toml::from_str::<AppSettings>("bogus = 1\n").is_err());
    }

    #[test]
    fn capture_limit_validated() {
        let mut s = AppSettings::default();
        s.capture.max_message_bytes = 1;
        assert!(s.validate().is_err());
    }
}
