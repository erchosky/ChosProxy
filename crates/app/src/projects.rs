//! Project-level settings (backups, notes, scope override), automatic
//! backups on disk, and settings import/export bundles.

use crate::AppError;
use chosproxy_core::config::{AppSettings, ConfigSection, ProxyConfig};
use chosproxy_core::{Scope, ScopeConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Project setting keys.
pub(crate) const KEY_SCOPE: &str = "scope";
pub(crate) const KEY_BACKUP: &str = "backup";
pub(crate) const KEY_NOTES: &str = "notes";

/// Longest project notes accepted.
pub const MAX_NOTES_CHARS: usize = 200_000;
/// Largest settings file read for import.
const MAX_IMPORT_BYTES: u64 = 2 * 1024 * 1024;
/// Format tag of exported settings.
const SETTINGS_FORMAT: &str = "chosproxy-settings-v1";

/// Automatic backups of the open project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct BackupSettings {
    /// Take backups automatically.
    pub enabled: bool,
    /// Minutes between backups (5–1440). Nothing is written while the
    /// project is unchanged.
    pub interval_minutes: u32,
    /// Backups kept (1–100); the oldest are deleted after a new one is
    /// written and verified.
    pub max_backups: u32,
    /// Leave out traffic outside the scope.
    pub in_scope_only: bool,
    /// Folder for the backups; `None` = the default backups folder.
    pub directory: Option<String>,
}

impl Default for BackupSettings {
    fn default() -> Self {
        BackupSettings {
            enabled: false,
            interval_minutes: 30,
            max_backups: 10,
            in_scope_only: false,
            directory: None,
        }
    }
}

impl BackupSettings {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !(5..=1440).contains(&self.interval_minutes) {
            return Err("the backup interval must be 5–1440 minutes".into());
        }
        if !(1..=100).contains(&self.max_backups) {
            return Err("keep 1–100 backups".into());
        }
        if let Some(d) = &self.directory
            && (d.trim().is_empty() || !Path::new(d).is_absolute())
        {
            return Err("the backup folder must be an absolute path".into());
        }
        Ok(())
    }
}

/// A backup file of the open project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupInfo {
    /// Full path.
    pub path: String,
    /// File name.
    pub file_name: String,
    /// Size in bytes.
    #[ts(type = "number")]
    pub size: u64,
    /// Last modified, Unix epoch ms.
    #[ts(type = "number")]
    pub modified_at: i64,
}

/// Project-only settings shown in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectSettings {
    /// The project uses its own scope instead of the global one.
    pub scope_override: bool,
    /// Automatic backups.
    pub backup: BackupSettings,
    /// Folder used when `backup.directory` is not set.
    pub default_backup_directory: String,
    /// Backups need a project file (temporary projects are not backed up).
    pub backups_available: bool,
    /// Time of the last automatic or manual backup in this session.
    #[ts(type = "number | null")]
    pub last_backup_at: Option<i64>,
    /// Why the last backup failed, if it did.
    pub last_backup_error: Option<String>,
}

/// Characters allowed in a backup file name prefix.
pub(crate) fn backup_prefix(project_name: &str) -> String {
    let cleaned: String = project_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "project".into()
    } else {
        cleaned
    }
}

/// The backup file name for a moment in time.
pub(crate) fn backup_file_name(prefix: &str, at_ms: i64) -> String {
    format!(
        "{prefix}-backup-{}.{}",
        chosproxy_core::time::compact_utc(at_ms),
        crate::engine::PROJECT_EXTENSION
    )
}

/// Backups of this project in `dir`, newest first.
pub(crate) fn list_backups(dir: &Path, prefix: &str) -> Vec<BackupInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let start = format!("{prefix}-backup-");
    let end = format!(".{}", crate::engine::PROJECT_EXTENSION);
    let mut out: Vec<BackupInfo> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with(&start) && name.ends_with(&end)) {
                return None;
            }
            let meta = e.metadata().ok()?;
            meta.is_file().then(|| BackupInfo {
                path: e.path().display().to_string(),
                file_name: name,
                size: meta.len(),
                modified_at: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
                    .unwrap_or(0),
            })
        })
        .collect();
    // Names embed a sortable UTC timestamp.
    out.sort_by(|a, b| b.file_name.cmp(&a.file_name));
    out
}

/// Deletes the oldest backups beyond `keep`, never touching `newest`.
pub(crate) fn rotate_backups(dir: &Path, prefix: &str, keep: u32, newest: &Path) -> u32 {
    let mut removed = 0;
    for b in list_backups(dir, prefix).into_iter().skip(keep as usize) {
        if Path::new(&b.path) == newest {
            continue;
        }
        match std::fs::remove_file(&b.path) {
            Ok(()) => removed += 1,
            Err(e) => tracing::warn!(path = %b.path, error = %e, "could not remove an old backup"),
        }
    }
    removed
}

// ---------------------------------------------------------------------------
// Settings import/export

/// Parts of the configuration that can be exported and imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SettingsSection {
    /// Application settings (theme, capture, logging).
    App,
    /// Proxy listeners, HTTPS policy and Intercept rules (credentials are
    /// never exported).
    Proxy,
    /// The user's (global) scope.
    Scope,
    /// The open project's own scope.
    ProjectScope,
    /// The open project's backup settings.
    ProjectBackup,
}

impl SettingsSection {
    pub(crate) fn key(self) -> &'static str {
        match self {
            SettingsSection::App => "app",
            SettingsSection::Proxy => "proxy",
            SettingsSection::Scope => "scope",
            SettingsSection::ProjectScope => "projectScope",
            SettingsSection::ProjectBackup => "projectBackup",
        }
    }

    pub(crate) const ALL: [SettingsSection; 5] = [
        SettingsSection::App,
        SettingsSection::Proxy,
        SettingsSection::Scope,
        SettingsSection::ProjectScope,
        SettingsSection::ProjectBackup,
    ];
}

/// One section of an import, validated, with what would change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SectionPreview {
    /// Section.
    pub section: SettingsSection,
    /// The incoming value is valid and can be applied.
    pub valid: bool,
    /// Why it is not valid (or cannot be applied now).
    pub error: Option<String>,
    /// Current value, pretty JSON (`null` when not set).
    pub current: String,
    /// Incoming value, pretty JSON.
    pub incoming: String,
    /// The incoming value differs from the current one.
    pub changed: bool,
}

/// What an import file would change. Nothing is applied yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SettingsImportPreview {
    /// Sections found in the file.
    pub sections: Vec<SectionPreview>,
    /// Problems with the file as a whole (unknown sections, …).
    pub warnings: Vec<String>,
}

/// A validated value ready to apply.
#[derive(Debug, Clone)]
pub(crate) enum ParsedSection {
    App(AppSettings),
    Proxy(ProxyConfig),
    Scope(ScopeConfig),
    ProjectScope(ScopeConfig),
    ProjectBackup(BackupSettings),
}

/// Opens `data` when it is sealed (M31): it needs `password`. Plain data
/// is returned as it is.
pub(crate) fn unseal(data: Vec<u8>, password: Option<&str>) -> Result<Vec<u8>, AppError> {
    use chosproxy_storage::sealed::{SealError, is_sealed, open};
    if !is_sealed(&data) {
        return Ok(data);
    }
    let password = password
        .filter(|p| !p.is_empty())
        .ok_or(chosproxy_storage::StorageError::Encrypted)?;
    open(password, &data).map_err(|e| match e {
        SealError::Rejected => chosproxy_storage::StorageError::WrongKey.into(),
        other => AppError::Invalid(other.to_string()),
    })
}

/// Seals `data` with `password` when one is given (M31).
pub(crate) fn seal_if(data: Vec<u8>, password: Option<&str>) -> Result<Vec<u8>, AppError> {
    match password.filter(|p| !p.is_empty()) {
        Some(p) => {
            chosproxy_storage::sealed::seal(p, &data).map_err(|e| AppError::Invalid(e.to_string()))
        }
        None => Ok(data),
    }
}

/// Reads a settings export from disk (size-limited); an encrypted one
/// needs its password.
pub(crate) fn read_settings_file(
    path: &Path,
    password: Option<&str>,
) -> Result<serde_json::Map<String, serde_json::Value>, AppError> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_IMPORT_BYTES {
        return Err(AppError::Invalid(
            "the settings file is larger than 2 MB".into(),
        ));
    }
    let data = unseal(std::fs::read(path)?, password)?;
    let value: serde_json::Value = serde_json::from_slice(&data)
        .map_err(|e| AppError::Invalid(format!("not a valid settings file: {e}")))?;
    if value.get("format").and_then(|f| f.as_str()) != Some(SETTINGS_FORMAT) {
        return Err(AppError::Invalid(format!(
            "not a ChosProxy settings export (expected format \"{SETTINGS_FORMAT}\")"
        )));
    }
    match value.get("sections") {
        Some(serde_json::Value::Object(m)) => Ok(m.clone()),
        _ => Err(AppError::Invalid(
            "the settings file has no sections".into(),
        )),
    }
}

fn parse<T: serde::de::DeserializeOwned>(v: &serde_json::Value) -> Result<T, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("invalid value: {e}"))
}

/// Parses and validates one section's value.
pub(crate) fn parse_section(
    section: SettingsSection,
    v: &serde_json::Value,
) -> Result<ParsedSection, String> {
    Ok(match section {
        SettingsSection::App => {
            let s: AppSettings = parse(v)?;
            s.validate()?;
            ParsedSection::App(s)
        }
        SettingsSection::Proxy => {
            let mut c: ProxyConfig = parse(v)?;
            // Credentials never travel in settings files.
            for l in &mut c.listeners {
                l.auth = None;
            }
            c.validate()?;
            ParsedSection::Proxy(c)
        }
        SettingsSection::Scope | SettingsSection::ProjectScope => {
            let c: ScopeConfig = parse(v)?;
            Scope::compile(&c)?;
            if section == SettingsSection::Scope {
                ParsedSection::Scope(c)
            } else {
                ParsedSection::ProjectScope(c)
            }
        }
        SettingsSection::ProjectBackup => {
            let b: BackupSettings = parse(v)?;
            b.validate()?;
            ParsedSection::ProjectBackup(b)
        }
    })
}

/// The settings file content for `values` (key → value).
pub(crate) fn settings_document(
    values: serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<u8>, AppError> {
    let doc = serde_json::json!({
        "format": SETTINGS_FORMAT,
        "exportedAt": chosproxy_core::time::now_ms(),
        "version": env!("CARGO_PKG_VERSION"),
        "sections": values,
    });
    serde_json::to_vec_pretty(&doc).map_err(|e| AppError::Internal(e.to_string()))
}

pub(crate) fn pretty(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// Default folder for backups of a project.
pub(crate) fn default_backup_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_settings_validation() {
        BackupSettings::default().validate().unwrap();
        for bad in [
            BackupSettings {
                interval_minutes: 1,
                ..Default::default()
            },
            BackupSettings {
                max_backups: 0,
                ..Default::default()
            },
            BackupSettings {
                directory: Some("relative/dir".into()),
                ..Default::default()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn backup_names_list_and_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = backup_prefix("juice shop!");
        assert_eq!(prefix, "juice_shop_");
        let mut paths = Vec::new();
        for (i, at) in [1_000_000_000_000i64, 1_000_000_060_000, 1_000_000_120_000]
            .iter()
            .enumerate()
        {
            let p = dir.path().join(backup_file_name(&prefix, *at));
            std::fs::write(&p, vec![0u8; i + 1]).unwrap();
            paths.push(p);
        }
        std::fs::write(dir.path().join("other-backup-x.chosproxy"), b"x").unwrap();
        let listed = list_backups(dir.path(), &prefix);
        assert_eq!(listed.len(), 3);
        assert_eq!(Path::new(&listed[0].path), paths[2], "newest first");
        assert_eq!(rotate_backups(dir.path(), &prefix, 2, &paths[2]), 1);
        assert!(!paths[0].exists() && paths[1].exists() && paths[2].exists());
        // The newest is protected even when keep = 0 would remove it.
        assert_eq!(rotate_backups(dir.path(), &prefix, 0, &paths[2]), 1);
        assert!(paths[2].exists());
    }

    #[test]
    fn sections_are_validated_and_credentials_dropped() {
        let mut proxy = serde_json::to_value(ProxyConfig::default()).unwrap();
        proxy["listeners"][0]["auth"] = serde_json::json!({
            "username": "u", "password_hash": "00", "salt": "00", "iterations": 1
        });
        match parse_section(SettingsSection::Proxy, &proxy).unwrap() {
            ParsedSection::Proxy(c) => assert!(c.listeners[0].auth.is_none()),
            other => panic!("{other:?}"),
        }
        let bad_scope = serde_json::json!({ "include": [{ "host": "(", "regex": true }] });
        assert!(parse_section(SettingsSection::Scope, &bad_scope).is_err());
        assert!(parse_section(SettingsSection::App, &serde_json::json!({ "nope": 1 })).is_err());
        let backup = serde_json::json!({ "enabled": true, "intervalMinutes": 2 });
        assert!(parse_section(SettingsSection::ProjectBackup, &backup).is_err());
    }

    #[test]
    fn settings_files_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("s.json");
        let mut m = serde_json::Map::new();
        m.insert(
            "scope".into(),
            serde_json::to_value(ScopeConfig::default()).unwrap(),
        );
        std::fs::write(&ok, settings_document(m).unwrap()).unwrap();
        assert!(read_settings_file(&ok, None).unwrap().contains_key("scope"));
        let bad = dir.path().join("b.json");
        std::fs::write(&bad, br#"{"format":"other","sections":{}}"#).unwrap();
        assert!(read_settings_file(&bad, None).is_err());
    }
}
