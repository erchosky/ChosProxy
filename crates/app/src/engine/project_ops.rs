//! Engine operations on the whole project: Save As, export, backups and
//! restore, the project settings layer (scope override, notes), the audit
//! log and settings import/export.

use super::{Engine, display_name, read, with_extension, write};
use crate::AppError;
use crate::projects::{
    self, BackupInfo, BackupSettings, KEY_BACKUP, KEY_NOTES, KEY_SCOPE, MAX_NOTES_CHARS,
    ParsedSection, ProjectSettings, SectionPreview, SettingsImportPreview, SettingsSection,
};
use crate::types::{ProjectInfo, ProjectKind};
use chosproxy_core::event::{EngineEvent, NoticeLevel};
use chosproxy_core::{Scope, ScopeConfig};
use chosproxy_storage::{
    AuditEntry, AuditQuery, CopyKey, ProjectDb, ProjectFileInfo, ProjectKey, StorageError,
    inspect_project_file, inspect_project_file_with,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often the backup schedule is checked.
const BACKUP_CHECK: Duration = Duration::from_secs(60);

/// State of automatic backups for the open project.
#[derive(Debug, Default)]
pub(crate) struct BackupState {
    /// Backups are due from this moment on (project open or last backup).
    pub(crate) since: i64,
    pub(crate) last_at: Option<i64>,
    pub(crate) last_error: Option<String>,
    /// Project contents at the last backup (or when it was opened).
    pub(crate) fingerprint: Option<String>,
}

fn scope_summary(c: &ScopeConfig) -> String {
    let on =
        |rules: &[chosproxy_core::scope::ScopeRule]| rules.iter().filter(|r| r.enabled).count();
    format!(
        "{} include and {} exclude rule(s)",
        on(&c.include),
        on(&c.exclude)
    )
}

impl Engine {
    fn project_db(&self) -> Result<(ProjectDb, ProjectInfo), AppError> {
        let db = self.db()?;
        let info = read(&self.inner.project).ok_or(AppError::NoProject)?;
        Ok((db, info))
    }

    /// Records a security-relevant action in the open project's audit log.
    pub(crate) fn audit(&self, action: &str, detail: &str) {
        if let Ok(db) = self.db()
            && !db.is_read_only()
            && let Err(e) = db.audit(action, detail)
        {
            tracing::warn!(error = %e, action, "could not write the audit log");
        }
    }

    // ----------------------------------------------------------------------
    // Scope layers

    /// The scope in effect: the project's own, else the user's.
    pub(crate) fn effective_scope(&self) -> ScopeConfig {
        read(&self.inner.project_scope).unwrap_or_else(|| read(&self.inner.scope_config))
    }

    /// Recompiles the effective scope and applies it everywhere.
    pub(crate) fn apply_scope(&self) {
        let config = self.effective_scope();
        let scope = Scope::compile(&config).unwrap_or_default();
        let filter = (config.skip_out_of_scope_proxy && !scope.is_empty()).then(|| scope.clone());
        write(&self.inner.store_filter, filter);
        if let Ok(db) = self.db() {
            db.set_scope(scope.clone());
        }
        self.inner.proxy.intercept().set_scope(scope.clone());
        write(&self.inner.scope, scope);
        let _ = self.inner.events.send(EngineEvent::ScopeChanged);
    }

    /// Loads the project's scope override (if any) after opening a project.
    pub(crate) async fn load_project_layer(&self, db: &ProjectDb) {
        let db2 = db.clone();
        let raw = tokio::task::spawn_blocking(move || db2.setting(KEY_SCOPE))
            .await
            .ok()
            .and_then(Result::ok)
            .flatten();
        let parsed = raw.map(|text| {
            serde_json::from_str::<ScopeConfig>(&text)
                .map_err(|e| e.to_string())
                .and_then(|c| Scope::compile(&c).map(|_| c))
        });
        let project_scope = match parsed {
            Some(Ok(c)) => Some(c),
            Some(Err(e)) => {
                self.notice(
                    NoticeLevel::Warning,
                    format!("The project's own scope is invalid and was ignored: {e}"),
                );
                None
            }
            None => None,
        };
        write(&self.inner.project_scope, project_scope);
        self.apply_scope();
    }

    /// Whether the open project uses its own scope.
    pub fn scope_override(&self) -> bool {
        read(&self.inner.project_scope).is_some()
    }

    /// Makes the open project use its own scope (starting from the current
    /// one) or go back to the user's global scope.
    pub fn set_scope_override(&self, enabled: bool) -> Result<ScopeConfig, AppError> {
        let (db, _) = self.project_db()?;
        if enabled == self.scope_override() {
            return Ok(self.effective_scope());
        }
        if enabled {
            let config = self.effective_scope();
            let json =
                serde_json::to_string(&config).map_err(|e| AppError::Internal(e.to_string()))?;
            db.set_setting(KEY_SCOPE, Some(json))?;
            write(&self.inner.project_scope, Some(config));
            self.audit(
                "scope.override.enabled",
                "the project now uses its own scope",
            );
        } else {
            db.set_setting(KEY_SCOPE, None)?;
            write(&self.inner.project_scope, None);
            self.audit(
                "scope.override.disabled",
                "the project uses the global scope again",
            );
        }
        self.apply_scope();
        Ok(self.effective_scope())
    }

    /// Saves scope rules into the layer in effect (project or global).
    pub(crate) fn store_scope(&self, config: ScopeConfig) -> Result<(), AppError> {
        if read(&self.inner.project).is_some_and(|p| p.read_only) {
            return Err(StorageError::ReadOnly.into());
        }
        Scope::compile(&config).map_err(AppError::Invalid)?;
        let layer = if self.scope_override() {
            let (db, _) = self.project_db()?;
            let json =
                serde_json::to_string(&config).map_err(|e| AppError::Internal(e.to_string()))?;
            db.set_setting(KEY_SCOPE, Some(json))?;
            write(&self.inner.project_scope, Some(config.clone()));
            "project"
        } else {
            self.inner.store.save(&config)?;
            write(&self.inner.scope_config, config.clone());
            "global"
        };
        self.apply_scope();
        self.audit(
            "scope.changed",
            &format!("{layer} scope: {}", scope_summary(&config)),
        );
        Ok(())
    }

    // ----------------------------------------------------------------------
    // Save As, export

    /// Copies the project to `path` and continues working in the copy. A
    /// temporary project becomes a project file this way.
    pub async fn save_project_as(&self, path: &Path) -> Result<ProjectInfo, AppError> {
        self.save_project_as_with(path, ProjectEncryption::Same)
            .await
    }

    /// Save As, choosing the copy's encryption (M31): keep it, none, or a
    /// new password. A temporary project's random key is never reused for
    /// a file: "same" saves it without encryption unless a password is
    /// given.
    pub async fn save_project_as_with(
        &self,
        path: &Path,
        encryption: ProjectEncryption,
    ) -> Result<ProjectInfo, AppError> {
        let (db, info) = self.project_db()?;
        let (copy, key) = copy_key(&db, &info, encryption)?;
        let path = with_extension(path);
        if path.exists() {
            return Err(AppError::Invalid(format!(
                "{} already exists",
                path.display()
            )));
        }
        let dest = path.clone();
        let encrypted = key.is_some();
        tokio::task::spawn_blocking(move || db.copy_with(&dest, false, copy)).await??;
        self.audit(
            "project.saved_as",
            &format!(
                "{}{}",
                path.display(),
                if encrypted { " (encrypted)" } else { "" }
            ),
        );
        let name = display_name(&path);
        let new_info = self
            .activate(path.clone(), ProjectKind::Persistent, name, key)
            .await?;
        self.audit("project.created_from", &info.path);
        if encrypted != info.encrypted {
            self.audit(
                if encrypted {
                    "project.encrypted"
                } else {
                    "project.decrypted"
                },
                &format!("copy of {}", info.path),
            );
        }
        Ok(new_info)
    }

    /// Changes the password of the open encrypted project.
    pub async fn change_project_password(&self, password: &str) -> Result<(), AppError> {
        let (db, info) = self.project_db()?;
        if !info.encrypted || info.kind != ProjectKind::Persistent {
            return Err(AppError::Invalid(
                "only an encrypted project file has a password; use Save As to encrypt it".into(),
            ));
        }
        let key = ProjectKey::password(password)?;
        tokio::task::spawn_blocking(move || db.rekey(key)).await??;
        self.write_session_marker();
        self.audit("project.password.changed", &info.path);
        Ok(())
    }

    /// Writes a copy of the project to `path` (optionally only in-scope
    /// traffic) and keeps working in the current project.
    pub async fn export_project(
        &self,
        path: &Path,
        in_scope_only: bool,
    ) -> Result<ProjectFileInfo, AppError> {
        self.export_project_with(path, in_scope_only, ProjectEncryption::Same)
            .await
    }

    /// Export, choosing the copy's encryption (M31).
    pub async fn export_project_with(
        &self,
        path: &Path,
        in_scope_only: bool,
        encryption: ProjectEncryption,
    ) -> Result<ProjectFileInfo, AppError> {
        let (db, project) = self.project_db()?;
        let (copy, key) = copy_key(&db, &project, encryption)?;
        let path = with_extension(path);
        let dest = path.clone();
        let encrypted = key.is_some();
        let info = tokio::task::spawn_blocking(move || {
            db.copy_with(&dest, in_scope_only, copy)?;
            inspect_project_file_with(&dest, key.as_ref())
        })
        .await??;
        self.audit(
            "project.exported",
            &format!(
                "{}{}",
                path.display(),
                if in_scope_only {
                    " (in-scope traffic only)"
                } else {
                    ""
                }
            ),
        );
        if encrypted != project.encrypted {
            self.audit(
                if encrypted {
                    "project.encrypted"
                } else {
                    "project.decrypted"
                },
                &format!("exported copy {}", path.display()),
            );
        }
        Ok(info)
    }

    /// Checks a project file without opening it.
    pub async fn inspect_project(&self, path: &Path) -> Result<ProjectFileInfo, AppError> {
        self.inspect_project_with(path, None).await
    }

    /// Like [`Engine::inspect_project`], with an encrypted file's password.
    /// Without it, an encrypted file is only reported as encrypted.
    pub async fn inspect_project_with(
        &self,
        path: &Path,
        password: Option<&str>,
    ) -> Result<ProjectFileInfo, AppError> {
        let key = password
            .filter(|p| !p.is_empty())
            .map(ProjectKey::password)
            .transpose()?;
        let path = path.to_path_buf();
        Ok(tokio::task::spawn_blocking(move || {
            match inspect_project_file_with(&path, key.as_ref()) {
                Err(StorageError::Encrypted) => Ok(ProjectFileInfo {
                    schema: 0,
                    exchanges: 0,
                    size: std::fs::metadata(&path)?.len(),
                    created_at: None,
                    encrypted: true,
                }),
                other => other,
            }
        })
        .await??)
    }

    // ----------------------------------------------------------------------
    // Backups

    async fn backup_settings(&self, db: &ProjectDb) -> BackupSettings {
        let db = db.clone();
        tokio::task::spawn_blocking(move || db.setting(KEY_BACKUP))
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn backup_dir(&self, settings: &BackupSettings) -> PathBuf {
        settings
            .directory
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| projects::default_backup_dir(&self.inner.paths.data_dir))
    }

    /// Project-only settings.
    pub async fn project_settings(&self) -> Result<ProjectSettings, AppError> {
        let (db, info) = self.project_db()?;
        let backup = self.backup_settings(&db).await;
        let state = self
            .inner
            .backup_state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        Ok(ProjectSettings {
            scope_override: self.scope_override(),
            default_backup_directory: projects::default_backup_dir(&self.inner.paths.data_dir)
                .display()
                .to_string(),
            backup,
            backups_available: info.kind == ProjectKind::Persistent && !info.read_only,
            last_backup_at: state.last_at,
            last_backup_error: state.last_error.clone(),
        })
    }

    /// Saves the backup settings of the open project.
    pub async fn save_backup_settings(
        &self,
        settings: BackupSettings,
    ) -> Result<ProjectSettings, AppError> {
        settings.validate().map_err(AppError::Invalid)?;
        let (db, _) = self.project_db()?;
        let json =
            serde_json::to_string(&settings).map_err(|e| AppError::Internal(e.to_string()))?;
        let db2 = db.clone();
        tokio::task::spawn_blocking(move || db2.set_setting(KEY_BACKUP, Some(json))).await??;
        self.audit(
            "backup.settings.changed",
            &format!(
                "automatic backups {}, every {} min, keep {}",
                if settings.enabled { "on" } else { "off" },
                settings.interval_minutes,
                settings.max_backups
            ),
        );
        self.project_settings().await
    }

    /// Backups of the open project, newest first.
    pub async fn list_backups(&self) -> Result<Vec<BackupInfo>, AppError> {
        let (db, info) = self.project_db()?;
        let settings = self.backup_settings(&db).await;
        Ok(projects::list_backups(
            &self.backup_dir(&settings),
            &projects::backup_prefix(&info.name),
        ))
    }

    /// Writes a backup now: to a temporary name first, verified, then
    /// renamed; old backups are rotated only afterwards.
    pub async fn backup_now(&self) -> Result<BackupInfo, AppError> {
        let result = self.write_backup().await;
        let mut state = self
            .inner
            .backup_state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        match &result {
            Ok(b) => {
                state.last_at = Some(b.modified_at);
                state.last_error = None;
            }
            Err(e) => state.last_error = Some(e.to_string()),
        }
        drop(state);
        if let Err(e) = &result {
            self.notice(NoticeLevel::Error, format!("Project backup failed: {e}"));
        }
        result
    }

    async fn write_backup(&self) -> Result<BackupInfo, AppError> {
        let (db, info) = self.project_db()?;
        if info.read_only {
            return Err(StorageError::ReadOnly.into());
        }
        if info.kind != ProjectKind::Persistent {
            return Err(AppError::Invalid(
                "temporary projects are not backed up; save the project to a file first".into(),
            ));
        }
        let settings = self.backup_settings(&db).await;
        let dir = self.backup_dir(&settings);
        let prefix = projects::backup_prefix(&info.name);
        let now = chosproxy_core::time::now_ms();
        let path = dir.join(projects::backup_file_name(&prefix, now));
        let partial = PathBuf::from(format!("{}.partial", path.display()));
        let fingerprint = {
            let (d, p, part) = (dir.clone(), path.clone(), partial.clone());
            let db = db.clone();
            let in_scope = settings.in_scope_only;
            tokio::task::spawn_blocking(move || -> Result<String, AppError> {
                // Backups hold the project's traffic: a folder we create is
                // private; an existing folder the user chose is left as it is
                // (each backup file is private anyway).
                if !d.exists() {
                    chosproxy_core::fs::create_private_dir(&d)?;
                }
                if p.exists() {
                    return Err(AppError::Invalid(
                        "a backup was written less than a second ago".into(),
                    ));
                }
                let _ = std::fs::remove_file(&part);
                let fingerprint = db.fingerprint()?;
                db.copy_to(&part, in_scope)?;
                if let Err(e) = db.inspect_copy(&part) {
                    let _ = std::fs::remove_file(&part);
                    return Err(AppError::Invalid(format!(
                        "the new backup failed verification: {e}"
                    )));
                }
                std::fs::rename(&part, &p)?;
                Ok(fingerprint)
            })
            .await??
        };
        let removed = projects::rotate_backups(&dir, &prefix, settings.max_backups, &path);
        {
            let mut state = self
                .inner
                .backup_state
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            state.fingerprint = Some(fingerprint);
            state.since = now;
        }
        self.audit(
            "backup.created",
            &format!(
                "{}{}",
                path.display(),
                if removed > 0 {
                    format!(" ({removed} old removed)")
                } else {
                    String::new()
                }
            ),
        );
        projects::list_backups(&dir, &prefix)
            .into_iter()
            .find(|b| Path::new(&b.path) == path)
            .ok_or_else(|| AppError::Internal("the backup disappeared after writing".into()))
    }

    /// Takes a backup when automatic backups are on, the interval has
    /// passed and the project changed since the last one.
    async fn backup_if_due(&self) {
        let Ok((db, info)) = self.project_db() else {
            return;
        };
        if info.kind != ProjectKind::Persistent {
            return;
        }
        let settings = self.backup_settings(&db).await;
        if !settings.enabled {
            return;
        }
        let (since, last) = {
            let s = self
                .inner
                .backup_state
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            (s.since, s.fingerprint.clone())
        };
        let due =
            chosproxy_core::time::now_ms() - since >= i64::from(settings.interval_minutes) * 60_000;
        if !due {
            return;
        }
        let db2 = db.clone();
        let current = tokio::task::spawn_blocking(move || db2.fingerprint())
            .await
            .ok()
            .and_then(Result::ok);
        if current.is_some() && current == last {
            // Unchanged: wait another interval.
            let mut s = self
                .inner
                .backup_state
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            s.since = chosproxy_core::time::now_ms();
            return;
        }
        let _ = self.backup_now().await;
    }

    /// Starts the backup schedule for the newly opened project.
    pub(crate) async fn start_backups(&self, db: &ProjectDb) {
        let db2 = db.clone();
        let fingerprint = tokio::task::spawn_blocking(move || db2.fingerprint())
            .await
            .ok()
            .and_then(Result::ok);
        {
            let mut s = self
                .inner
                .backup_state
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            *s = super::project_ops::BackupState {
                since: chosproxy_core::time::now_ms(),
                fingerprint,
                ..Default::default()
            };
        }
        let engine = self.internal_clone();
        let task = tokio::spawn(async move {
            let mut tick = tokio::time::interval(BACKUP_CHECK);
            tick.tick().await;
            loop {
                tick.tick().await;
                engine.backup_if_due().await;
            }
        });
        if let Some(old) = self
            .inner
            .backup_task
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .replace(task)
        {
            old.abort();
        }
    }

    /// Stops the backup schedule (project closing).
    pub(crate) fn stop_backups(&self) {
        if let Some(task) = self
            .inner
            .backup_task
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            task.abort();
        }
    }

    /// Opens a copy of a backup as a new project file next to the current
    /// project. The backup itself and the current project are left as they
    /// are.
    pub async fn restore_backup(&self, backup: &Path) -> Result<ProjectInfo, AppError> {
        self.restore_backup_with(backup, None).await
    }

    /// Restores a backup; an encrypted one opens with `password`, or with
    /// the open project's key when none is given.
    pub async fn restore_backup_with(
        &self,
        backup: &Path,
        password: Option<&str>,
    ) -> Result<ProjectInfo, AppError> {
        let key = match password.filter(|p| !p.is_empty()) {
            Some(p) => Some(ProjectKey::password(p)?),
            None => self.db().ok().and_then(|db| db.current_key()),
        };
        let backup = backup.to_path_buf();
        let (checked, k) = (backup.clone(), key.clone());
        let encrypted = tokio::task::spawn_blocking(move || -> Result<bool, AppError> {
            let encrypted = chosproxy_storage::is_encrypted_file(&checked)?;
            if encrypted {
                inspect_project_file_with(&checked, k.as_ref())?;
            } else {
                inspect_project_file(&checked)?;
            }
            Ok(encrypted)
        })
        .await??;
        let key = if encrypted { key } else { None };
        let current = read(&self.inner.project);
        let dir = current
            .as_ref()
            .filter(|p| p.kind == ProjectKind::Persistent)
            .and_then(|p| Path::new(&p.path).parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.inner.paths.data_dir.clone());
        let stem = current
            .as_ref()
            .map(|p| projects::backup_prefix(&p.name))
            .unwrap_or_else(|| "project".into());
        let target = dir.join(format!(
            "{stem}-restored-{}.{}",
            chosproxy_core::time::compact_utc(chosproxy_core::time::now_ms()),
            super::PROJECT_EXTENSION
        ));
        if target.exists() {
            return Err(AppError::Invalid(format!(
                "{} already exists",
                target.display()
            )));
        }
        let (src, dst) = (backup.clone(), target.clone());
        tokio::task::spawn_blocking(move || -> Result<(), AppError> {
            std::fs::copy(&src, &dst)?;
            chosproxy_core::fs::restrict_file(&dst)?;
            Ok(())
        })
        .await??;
        self.audit(
            "backup.restored",
            &format!("{} → {}", backup.display(), target.display()),
        );
        let name = display_name(&target);
        let info = self
            .activate(target, ProjectKind::Persistent, name, key)
            .await?;
        self.audit("project.restored_from", &backup.display().to_string());
        Ok(info)
    }

    // ----------------------------------------------------------------------
    // Notes and audit log

    /// The project's notes (Markdown).
    pub async fn project_notes(&self) -> Result<String, AppError> {
        let (db, _) = self.project_db()?;
        let raw = tokio::task::spawn_blocking(move || db.setting(KEY_NOTES)).await??;
        Ok(raw
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default())
    }

    /// Saves the project's notes.
    pub async fn save_project_notes(&self, notes: String) -> Result<(), AppError> {
        if notes.chars().count() > MAX_NOTES_CHARS {
            return Err(AppError::Invalid(
                "notes are limited to 200 000 characters".into(),
            ));
        }
        let (db, _) = self.project_db()?;
        let json = serde_json::to_string(&notes).map_err(|e| AppError::Internal(e.to_string()))?;
        tokio::task::spawn_blocking(move || db.set_setting(KEY_NOTES, Some(json))).await??;
        Ok(())
    }

    /// The project's audit log, newest first.
    pub async fn audit_log(&self, limit: u32) -> Result<Vec<AuditEntry>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.audit_log(limit)).await??)
    }

    /// Queries the project's audit log with filtering and pagination (M31 F, §122).
    pub async fn audit_log_query(&self, query: AuditQuery) -> Result<Vec<AuditEntry>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.audit_log_query(&query)).await??)
    }

    // ----------------------------------------------------------------------
    // Settings import/export

    async fn current_section(
        &self,
        section: SettingsSection,
    ) -> Result<serde_json::Value, AppError> {
        let to = |v: Result<serde_json::Value, serde_json::Error>| {
            v.map_err(|e| AppError::Internal(e.to_string()))
        };
        Ok(match section {
            SettingsSection::App => to(serde_json::to_value(self.settings()))?,
            SettingsSection::Proxy => {
                let mut c = self.proxy_config().await;
                for l in &mut c.listeners {
                    l.auth = None;
                }
                to(serde_json::to_value(c))?
            }
            SettingsSection::Scope => to(serde_json::to_value(read(&self.inner.scope_config)))?,
            SettingsSection::ProjectScope => {
                self.project_db()?;
                match read(&self.inner.project_scope) {
                    Some(c) => to(serde_json::to_value(c))?,
                    None => serde_json::Value::Null,
                }
            }
            SettingsSection::ProjectBackup => {
                let (db, _) = self.project_db()?;
                to(serde_json::to_value(self.backup_settings(&db).await))?
            }
        })
    }

    /// Writes the chosen sections to `path` as JSON. Listener credentials
    /// are never included.
    pub async fn export_settings(
        &self,
        path: &Path,
        sections: &[SettingsSection],
    ) -> Result<PathBuf, AppError> {
        self.export_settings_with(path, sections, None).await
    }

    /// Settings export, encrypted with `password` when given (M31).
    pub async fn export_settings_with(
        &self,
        path: &Path,
        sections: &[SettingsSection],
        password: Option<&str>,
    ) -> Result<PathBuf, AppError> {
        if sections.is_empty() {
            return Err(AppError::Invalid(
                "choose at least one section to export".into(),
            ));
        }
        let mut values = serde_json::Map::new();
        for s in sections {
            let v = self.current_section(*s).await?;
            if !v.is_null() {
                values.insert(s.key().to_string(), v);
            }
        }
        let doc = projects::seal_if(projects::settings_document(values)?, password)?;
        chosproxy_core::fs::write_private_atomic(path, &doc)?;
        self.audit(
            "settings.exported",
            &format!(
                "{}{}",
                path.display(),
                if password.is_some_and(|p| !p.is_empty()) {
                    " (encrypted)"
                } else {
                    ""
                }
            ),
        );
        Ok(path.to_path_buf())
    }

    /// Reads and validates a settings file and shows what would change.
    /// Nothing is applied.
    pub async fn preview_settings_import(
        &self,
        path: &Path,
    ) -> Result<SettingsImportPreview, AppError> {
        self.preview_settings_import_with(path, None).await
    }

    /// Preview of an encrypted settings file (M31).
    pub async fn preview_settings_import_with(
        &self,
        path: &Path,
        password: Option<&str>,
    ) -> Result<SettingsImportPreview, AppError> {
        let incoming = projects::read_settings_file(path, password)?;
        let mut sections = Vec::new();
        let mut warnings = Vec::new();
        for key in incoming.keys() {
            if !SettingsSection::ALL.iter().any(|s| s.key() == key) {
                warnings.push(format!("unknown section \"{key}\" ignored"));
            }
        }
        for section in SettingsSection::ALL {
            let Some(value) = incoming.get(section.key()) else {
                continue;
            };
            let current = self.current_section(section).await;
            let (valid, error) = match (&current, projects::parse_section(section, value)) {
                (Err(e), _) => (false, Some(e.to_string())),
                (_, Err(e)) => (false, Some(e)),
                _ => (true, None),
            };
            // Compare normalized values (credentials removed, defaults filled).
            let normalized = match projects::parse_section(section, value) {
                Ok(p) => parsed_value(&p).unwrap_or_else(|| value.clone()),
                Err(_) => value.clone(),
            };
            let current = current.unwrap_or(serde_json::Value::Null);
            sections.push(SectionPreview {
                section,
                valid,
                error,
                changed: normalized != current,
                current: projects::pretty(&current),
                incoming: projects::pretty(&normalized),
            });
        }
        Ok(SettingsImportPreview { sections, warnings })
    }

    /// Applies the chosen sections of a settings file, validating them again.
    pub async fn apply_settings_import(
        &self,
        path: &Path,
        sections: &[SettingsSection],
    ) -> Result<Vec<SettingsSection>, AppError> {
        self.apply_settings_import_with(path, sections, None).await
    }

    /// Applies an encrypted settings file (M31).
    pub async fn apply_settings_import_with(
        &self,
        path: &Path,
        sections: &[SettingsSection],
        password: Option<&str>,
    ) -> Result<Vec<SettingsSection>, AppError> {
        let incoming = projects::read_settings_file(path, password)?;
        // Validate everything first: either all chosen sections apply or none.
        let mut parsed = Vec::new();
        for s in sections {
            let value = incoming.get(s.key()).ok_or_else(|| {
                AppError::Invalid(format!("the file has no \"{}\" section", s.key()))
            })?;
            let p = projects::parse_section(*s, value)
                .map_err(|e| AppError::Invalid(format!("{}: {e}", s.key())))?;
            if matches!(
                p,
                ParsedSection::ProjectScope(_) | ParsedSection::ProjectBackup(_)
            ) {
                self.project_db()?;
                self.ensure_writable()?;
            }
            parsed.push((*s, p));
        }
        for (_, p) in &parsed {
            match p.clone() {
                ParsedSection::App(s) => {
                    self.save_settings(s)?;
                }
                ParsedSection::Proxy(mut c) => {
                    // Keep existing credentials of listeners with the same id.
                    let current = self.proxy_config().await;
                    for l in &mut c.listeners {
                        l.auth = current.listener(&l.id).and_then(|x| x.auth.clone());
                    }
                    self.save_proxy_config(c).await?;
                }
                ParsedSection::Scope(c) => {
                    Scope::compile(&c).map_err(AppError::Invalid)?;
                    self.inner.store.save(&c)?;
                    write(&self.inner.scope_config, c);
                    self.apply_scope();
                }
                ParsedSection::ProjectScope(c) => {
                    let (db, _) = self.project_db()?;
                    let json =
                        serde_json::to_string(&c).map_err(|e| AppError::Internal(e.to_string()))?;
                    db.set_setting(KEY_SCOPE, Some(json))?;
                    write(&self.inner.project_scope, Some(c));
                    self.apply_scope();
                }
                ParsedSection::ProjectBackup(b) => {
                    self.save_backup_settings(b).await?;
                }
            }
        }
        let applied: Vec<SettingsSection> = parsed.iter().map(|(s, _)| *s).collect();
        let names: Vec<&str> = applied.iter().map(|s| s.key()).collect();
        self.audit(
            "settings.imported",
            &format!("{} from {}", names.join(", "), path.display()),
        );
        Ok(applied)
    }
}

fn parsed_value(p: &ParsedSection) -> Option<serde_json::Value> {
    match p {
        ParsedSection::App(v) => serde_json::to_value(v).ok(),
        ParsedSection::Proxy(v) => serde_json::to_value(v).ok(),
        ParsedSection::Scope(v) | ParsedSection::ProjectScope(v) => serde_json::to_value(v).ok(),
        ParsedSection::ProjectBackup(v) => serde_json::to_value(v).ok(),
    }
}

/// How a copy of the project (Save As, export) is encrypted (M31).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum ProjectEncryption {
    /// Like the project file (a temporary project's random key is not
    /// carried over: its copy is not encrypted).
    Same,
    /// Not encrypted.
    None,
    /// Encrypted with this password.
    Password {
        /// The password (not stored anywhere).
        password: String,
    },
}

/// The copy mode and the key the copy will open with.
fn copy_key(
    db: &ProjectDb,
    info: &ProjectInfo,
    encryption: ProjectEncryption,
) -> Result<(CopyKey, Option<ProjectKey>), AppError> {
    Ok(match encryption {
        ProjectEncryption::Same if info.kind == ProjectKind::Temporary && info.encrypted => {
            (CopyKey::Plain, None)
        }
        ProjectEncryption::Same => (CopyKey::Same, db.current_key()),
        ProjectEncryption::None => (CopyKey::Plain, None),
        ProjectEncryption::Password { password } => {
            let key = ProjectKey::password(&password)?;
            (CopyKey::Key(key.clone()), Some(key))
        }
    })
}
