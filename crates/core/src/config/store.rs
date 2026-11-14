use super::ConfigSection;
use crate::fs::write_private_atomic;
use std::path::{Path, PathBuf};

/// Errors reading or writing configuration files.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Filesystem error.
    #[error("cannot access {path}: {source}")]
    Io {
        /// File involved.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The value failed semantic validation.
    #[error("invalid configuration: {0}")]
    Invalid(String),
    /// Serialization failed (a bug: all sections are serializable).
    #[error("cannot serialize configuration: {0}")]
    Serialize(String),
}

/// Result of loading a section: the effective value plus a warning when the
/// file existed but could not be used.
#[derive(Debug, Clone)]
pub struct Loaded<T> {
    /// Effective value (defaults when the file is missing or unusable).
    pub value: T,
    /// Why the file on disk was ignored, if it was.
    pub warning: Option<String>,
}

/// Reads and writes configuration sections in one directory.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
}

/// Configuration files larger than this are refused (they are tiny in practice).
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

impl ConfigStore {
    /// Creates a store rooted at `dir` (created on first save).
    pub fn new(dir: impl Into<PathBuf>) -> ConfigStore {
        ConfigStore { dir: dir.into() }
    }

    /// Directory holding the files.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_of<T: ConfigSection>(&self) -> PathBuf {
        self.dir.join(T::FILE_NAME)
    }

    /// Loads a section. A missing file yields defaults. A file that cannot be
    /// read, parsed or validated also yields defaults, with a warning; the
    /// file is left untouched so the user can fix it.
    pub fn load<T: ConfigSection>(&self) -> Loaded<T> {
        let path = self.path_of::<T>();
        match self.try_load::<T>(&path) {
            Ok(Some(value)) => Loaded {
                value,
                warning: None,
            },
            Ok(None) => Loaded {
                value: T::default(),
                warning: None,
            },
            Err(msg) => {
                tracing::warn!(file = %path.display(), error = %msg, "ignoring configuration file");
                Loaded {
                    value: T::default(),
                    warning: Some(format!("{}: {msg}; using defaults", path.display())),
                }
            }
        }
    }

    fn try_load<T: ConfigSection>(&self, path: &Path) -> Result<Option<T>, String> {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        if meta.len() > MAX_CONFIG_BYTES {
            return Err(format!("file is larger than {MAX_CONFIG_BYTES} bytes"));
        }
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let value: T = toml::from_str(&text).map_err(|e| e.to_string())?;
        value.validate()?;
        Ok(Some(value))
    }

    /// Validates and saves a section atomically with private permissions.
    /// If the existing file is unusable it is preserved as
    /// `<name>.invalid-<unix-seconds>` before being replaced.
    pub fn save<T: ConfigSection>(&self, value: &T) -> Result<(), ConfigError> {
        value.validate().map_err(ConfigError::Invalid)?;
        let text =
            toml::to_string_pretty(value).map_err(|e| ConfigError::Serialize(e.to_string()))?;
        crate::fs::create_private_dir(&self.dir).map_err(|source| ConfigError::Io {
            path: self.dir.clone(),
            source,
        })?;
        let path = self.path_of::<T>();
        if path.exists() && self.try_load::<T>(&path).is_err() {
            let backup =
                path.with_extension(format!("toml.invalid-{}", crate::time::now_ms() / 1000));
            std::fs::rename(&path, &backup).map_err(|source| ConfigError::Io {
                path: backup.clone(),
                source,
            })?;
            tracing::warn!(backup = %backup.display(), "preserved unusable configuration file");
        }
        write_private_atomic(&path, text.as_bytes())
            .map_err(|source| ConfigError::Io { path, source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppSettings, ProxyConfig, Theme};

    #[test]
    fn missing_file_gives_defaults_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path());
        let l = store.load::<ProxyConfig>();
        assert_eq!(l.value, ProxyConfig::default());
        assert!(l.warning.is_none());
    }

    #[test]
    fn save_then_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("cfg"));
        let mut s = AppSettings::default();
        s.ui.theme = Theme::System;
        store.save(&s).unwrap();
        assert_eq!(store.load::<AppSettings>().value, s);
    }

    #[test]
    fn invalid_file_warns_and_is_preserved_on_save() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path());
        std::fs::write(dir.path().join("settings.toml"), "this is = = not toml").unwrap();
        let l = store.load::<AppSettings>();
        assert!(l.warning.is_some());
        assert_eq!(l.value, AppSettings::default());
        // Still on disk, untouched.
        assert!(dir.path().join("settings.toml").exists());

        store.save(&AppSettings::default()).unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".invalid-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert!(store.load::<AppSettings>().warning.is_none());
    }

    #[test]
    fn invalid_values_not_saved() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path());
        let mut c = ProxyConfig::default();
        c.listeners[0].bind_address = "10.0.0.1".parse().unwrap();
        assert!(matches!(store.save(&c), Err(ConfigError::Invalid(_))));
        assert!(!dir.path().join("proxy.toml").exists());
    }
}
