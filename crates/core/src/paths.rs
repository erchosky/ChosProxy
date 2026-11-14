//! Locations of configuration, data and log files.

use crate::brand;
use std::path::{Path, PathBuf};

/// Environment variable that relocates every ChosProxy directory under one
/// root (portable installs, tests).
pub const HOME_ENV: &str = "CHOSPROXY_HOME";

/// All directories the application writes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// Global configuration sections (`settings.toml`, `proxy.toml`, …).
    pub config_dir: PathBuf,
    /// Application data (default location for persistent projects).
    pub data_dir: PathBuf,
    /// Application log files.
    pub log_dir: PathBuf,
    /// Temporary project databases (deleted when closed).
    pub temp_dir: PathBuf,
}

impl AppPaths {
    /// Resolves paths from `CHOSPROXY_HOME` or the platform conventions
    /// (XDG on Linux: `~/.config/chosproxy`, `~/.local/share/chosproxy`).
    pub fn resolve() -> Option<AppPaths> {
        if let Some(home) = std::env::var_os(HOME_ENV) {
            return Some(AppPaths::under(Path::new(&home)));
        }
        let dirs = directories::ProjectDirs::from("", "", &brand().config_dir_name)?;
        let data = dirs.data_local_dir().to_path_buf();
        Some(AppPaths {
            config_dir: dirs.config_dir().to_path_buf(),
            log_dir: data.join("logs"),
            temp_dir: std::env::temp_dir().join(format!("{}-projects", brand().config_dir_name)),
            data_dir: data,
        })
    }

    /// All directories under a single root.
    pub fn under(root: &Path) -> AppPaths {
        AppPaths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            log_dir: root.join("logs"),
            temp_dir: root.join("tmp"),
        }
    }

    /// Creates every directory with private permissions.
    pub fn ensure(&self) -> std::io::Result<()> {
        for d in [
            &self.config_dir,
            &self.data_dir,
            &self.log_dir,
            &self.temp_dir,
        ] {
            crate::fs::create_private_dir(d)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_root_is_contained() {
        let p = AppPaths::under(Path::new("/x"));
        for d in [&p.config_dir, &p.data_dir, &p.log_dir, &p.temp_dir] {
            assert!(d.starts_with("/x"));
        }
    }
}
