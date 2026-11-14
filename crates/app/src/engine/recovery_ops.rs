//! Crash recovery and read-only project mode (M31 A, `PROJECT-004`, spec §118).
//!
//! While a writable project is open, the engine maintains a versioned session
//! marker (`session.json` in `paths.data_dir`, or `sessions/session-<id>.json`
//! when another instance or an unresolved crash marker already occupies
//! `session.json`). A clean `close_project()` or `shutdown()` removes only
//! this session's marker.
//!
//! On startup, [`Engine::recovery_status`] inspects any remaining markers:
//! - markers belonging to a live sibling process (verified by PID and process
//!   start timestamp via `sysinfo`, plus in-process session registration) are
//!   ignored and left untouched;
//! - stale markers from an unexpected exit are surfaced to the user with
//!   explicit `Restore`, `Open read-only`, and `Discard recovery` actions;
//! - active traffic-generating tasks (`crawl`, `scan`, `discovery`, `fuzz`,
//!   `sequencer`, `api`, `agent`) are never auto-restarted on restore; only
//!   passive `live` tasks resume.

use super::{Engine, display_name, lock, read};
use crate::AppError;
use crate::event_log::LogCategory;
use crate::types::{ProjectInfo, ProjectKind};
use chosproxy_core::event::NoticeLevel;
use chosproxy_core::time::now_ms;
use chosproxy_storage::{ProjectDb, ProjectKey, StorageError};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use ts_rs::TS;

/// Primary session marker filename in `paths.data_dir`.
pub const SESSION_MARKER_FILE: &str = "session.json";

/// Current schema version of [`SessionMarker`].
pub const SESSION_MARKER_VERSION: u32 = 1;

/// Process-local registry of live [`Engine`] session IDs so multiple engines
/// in the same process (e.g. integration tests) can distinguish live sibling
/// instances from dropped/crashed ones.
static ACTIVE_SESSIONS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Queries the OS start timestamp (seconds since Unix epoch) of `pid`, if it
/// is currently alive.
pub fn process_start_secs(pid: u32) -> Option<u64> {
    let sys_pid = Pid::from_u32(pid);
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );
    sys.refresh_processes(ProcessesToUpdate::Some(&[sys_pid]), true);
    sys.process(sys_pid).map(sysinfo::Process::start_time)
}

/// Start timestamp of the current OS process.
pub fn current_process_start_secs() -> Option<u64> {
    process_start_secs(std::process::id())
}

/// Registers a new engine session in this process and returns its unique ID.
pub(super) fn register_session() -> String {
    let pid = std::process::id();
    let start = current_process_start_secs().unwrap_or(0);
    let seq = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let id = format!("{pid}-{start}-{seq}");
    if let Ok(mut set) = ACTIVE_SESSIONS.lock() {
        set.insert(id.clone());
    }
    id
}

/// Unregisters an engine session in this process.
pub(super) fn unregister_session(id: &str) {
    if let Ok(mut set) = ACTIVE_SESSIONS.lock() {
        set.remove(id);
    }
}

/// A task that was running when the previous session exited unexpectedly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecoveryTask {
    /// Task ID in the project database.
    #[ts(type = "number")]
    pub id: i64,
    /// Task kind (`crawl`, `scan`, `fuzz`, `live`, `agent`, …).
    pub kind: String,
    /// Display name of the task.
    pub name: String,
}

/// On-disk session marker written while a writable project is open.
/// Never contains passwords or encryption keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMarker {
    /// Schema version (`SESSION_MARKER_VERSION`).
    pub version: u32,
    /// Unique identifier of the engine session that wrote this marker.
    pub session_id: String,
    /// OS process ID.
    pub pid: u32,
    /// OS process start time (seconds since epoch) to detect PID reuse.
    #[serde(default)]
    pub process_start_secs: Option<u64>,
    /// When the project was opened (ms since epoch).
    pub started_at: i64,
    /// When the marker was last updated (ms since epoch).
    pub updated_at: i64,
    /// Project display name.
    pub project_name: String,
    /// Project database file path.
    pub project_path: String,
    /// Temporary or persistent project.
    pub project_kind: ProjectKind,
    /// Whether the project is encrypted.
    pub encrypted: bool,
    /// True when the project is an encrypted temporary project whose random
    /// key lived only in memory.
    #[serde(default)]
    pub ephemeral_key: bool,
    /// Tasks running in the session when the marker was written.
    #[serde(default)]
    pub tasks: Vec<RecoveryTask>,
}

/// Crash recovery state reported at startup when a previous session did not
/// close cleanly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecoveryStatus {
    /// Path of the session marker file.
    pub marker_path: String,
    /// True if the marker JSON was corrupt or had an unsupported version.
    pub marker_corrupt: bool,
    /// Project display name.
    pub project_name: String,
    /// Project database file path.
    pub project_path: String,
    /// Temporary or persistent project.
    pub project_kind: ProjectKind,
    /// Whether the project database is encrypted.
    pub encrypted: bool,
    /// True if the project was an encrypted temporary project whose in-memory
    /// key is no longer available.
    pub ephemeral_key: bool,
    /// Whether the project file exists on disk.
    pub file_exists: bool,
    /// Size of the project file in bytes (`0` when missing).
    #[ts(type = "number")]
    pub file_bytes: u64,
    /// Whether the project can be restored or opened read-only.
    pub restorable: bool,
    /// Process ID that wrote the marker.
    pub pid: u32,
    /// When the project was opened in the crashed session (ms since epoch).
    #[ts(type = "number")]
    pub started_at: i64,
    /// When the marker was last updated (ms since epoch).
    #[ts(type = "number")]
    pub updated_at: i64,
    /// Tasks that were running when the session exited unexpectedly.
    pub interrupted_tasks: Vec<RecoveryTask>,
    /// Human-readable summary or warning for the recovery dialog.
    pub message: String,
}

enum PendingMarker {
    Valid {
        marker_path: PathBuf,
        marker: SessionMarker,
    },
    Corrupt {
        marker_path: PathBuf,
    },
}

impl PendingMarker {
    fn path(&self) -> &Path {
        match self {
            PendingMarker::Valid { marker_path, .. } | PendingMarker::Corrupt { marker_path } => {
                marker_path
            }
        }
    }
}

fn read_marker_file(path: &Path) -> Result<SessionMarker, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let marker: SessionMarker = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if marker.version != SESSION_MARKER_VERSION {
        return Err(format!(
            "unsupported session marker version {}",
            marker.version
        ));
    }
    Ok(marker)
}

impl Engine {
    /// Fails with [`StorageError::ReadOnly`] when the open project was opened
    /// in read-only mode, or [`AppError::NoProject`] when none is open.
    pub(crate) fn ensure_writable(&self) -> Result<(), AppError> {
        let info = read(&self.inner.project).ok_or(AppError::NoProject)?;
        if info.read_only {
            return Err(StorageError::ReadOnly.into());
        }
        Ok(())
    }

    /// Candidate session marker paths in priority order (`data_dir/session.json`
    /// followed by `data_dir/sessions/session-*.json`).
    fn marker_candidates(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let primary = self.inner.paths.data_dir.join(SESSION_MARKER_FILE);
        if primary.is_file() {
            out.push(primary);
        }
        let sessions_dir = self.inner.paths.data_dir.join("sessions");
        if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
            let mut extra: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("session-") && n.ends_with(".json"))
                })
                .collect();
            extra.sort();
            out.extend(extra);
        }
        out
    }

    /// True if `marker` belongs to this engine or to another live ChosProxy
    /// session whose process is still running.
    fn is_marker_active(&self, marker: &SessionMarker) -> bool {
        if marker.session_id == self.inner.session_id {
            return true;
        }
        let me = std::process::id();
        if marker.pid == me {
            let start_matches = match (marker.process_start_secs, current_process_start_secs()) {
                (Some(expected), Some(actual)) => actual.abs_diff(expected) <= 2,
                (None, _) | (_, None) => true,
            };
            if !start_matches {
                return false;
            }
            return ACTIVE_SESSIONS
                .lock()
                .map(|set| set.contains(&marker.session_id))
                .unwrap_or(false);
        }
        let Some(actual_start) = process_start_secs(marker.pid) else {
            return false;
        };
        match marker.process_start_secs {
            Some(expected) => actual_start.abs_diff(expected) <= 2,
            None => true,
        }
    }

    /// Writes or updates this session's marker atomically if a writable project
    /// is open.
    pub(super) fn write_session_marker(&self) {
        let Some(info) = read(&self.inner.project) else {
            return;
        };
        if info.read_only {
            return;
        }
        let mut tasks: Vec<RecoveryTask> = lock(&self.inner.tasks_running)
            .iter()
            .map(|(id, t)| RecoveryTask {
                id: *id,
                kind: t.kind.to_string(),
                name: t.name.clone(),
            })
            .collect();
        tasks.sort_by_key(|t| t.id);

        let now = now_ms();
        let started_at = {
            let s = self.inner.session_started_at.load(Ordering::Relaxed);
            if s > 0 { s } else { now }
        };
        let marker = SessionMarker {
            version: SESSION_MARKER_VERSION,
            session_id: self.inner.session_id.clone(),
            pid: std::process::id(),
            process_start_secs: current_process_start_secs(),
            started_at,
            updated_at: now,
            project_name: info.name,
            project_path: info.path,
            project_kind: info.kind,
            encrypted: info.encrypted,
            ephemeral_key: info.kind == ProjectKind::Temporary && info.encrypted,
            tasks,
        };
        let Ok(bytes) = serde_json::to_vec_pretty(&marker) else {
            return;
        };
        let path = self.choose_session_marker_path();
        if let Err(e) = chosproxy_core::fs::write_private_atomic(&path, &bytes) {
            tracing::warn!(path = %path.display(), error = %e, "could not write session marker");
        }
    }

    fn choose_session_marker_path(&self) -> PathBuf {
        let mut slot = lock(&self.inner.session_marker_path);
        if let Some(existing) = slot.as_ref() {
            return existing.clone();
        }
        let primary = self.inner.paths.data_dir.join(SESSION_MARKER_FILE);
        let can_use_primary = if !primary.exists() {
            true
        } else {
            read_marker_file(&primary)
                .ok()
                .is_some_and(|m| m.session_id == self.inner.session_id)
        };
        let chosen = if can_use_primary {
            primary
        } else {
            let dir = self.inner.paths.data_dir.join("sessions");
            if let Err(e) = chosproxy_core::fs::create_private_dir(&dir) {
                tracing::warn!(error = %e, "could not create sessions marker directory");
            }
            dir.join(format!("session-{}.json", self.inner.session_id))
        };
        *slot = Some(chosen.clone());
        chosen
    }

    /// Removes only this engine session's marker file (called on clean close
    /// or shutdown).
    pub(super) fn remove_own_session_marker(&self) {
        let recorded = lock(&self.inner.session_marker_path).take();
        if let Some(path) = recorded {
            remove_if_owned(&path, &self.inner.session_id);
        }
        let primary = self.inner.paths.data_dir.join(SESSION_MARKER_FILE);
        remove_if_owned(&primary, &self.inner.session_id);
    }

    /// Paths referenced by any pending crash marker or live sibling session
    /// marker so temporary file cleanup never deletes a recoverable or
    /// sibling-in-use temporary project.
    pub(super) fn temp_recovery_protected(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for candidate in self.marker_candidates() {
            if let Ok(marker) = read_marker_file(&candidate) {
                for suffix in ["", "-wal", "-shm"] {
                    out.push(PathBuf::from(format!("{}{suffix}", marker.project_path)));
                }
            }
        }
        out
    }

    fn find_pending_marker(&self) -> Option<PendingMarker> {
        for candidate in self.marker_candidates() {
            match read_marker_file(&candidate) {
                Ok(marker) => {
                    if !self.is_marker_active(&marker) {
                        return Some(PendingMarker::Valid {
                            marker_path: candidate,
                            marker,
                        });
                    }
                }
                Err(_) => {
                    return Some(PendingMarker::Corrupt {
                        marker_path: candidate,
                    });
                }
            }
        }
        None
    }

    /// Checks whether a previous session exited without closing its project.
    pub fn recovery_status(&self) -> Option<RecoveryStatus> {
        let pending = self.find_pending_marker()?;
        match pending {
            PendingMarker::Corrupt { marker_path } => Some(RecoveryStatus {
                marker_path: marker_path.display().to_string(),
                marker_corrupt: true,
                project_name: "Unreadable recovery marker".into(),
                project_path: String::new(),
                project_kind: ProjectKind::Persistent,
                encrypted: false,
                ephemeral_key: false,
                file_exists: false,
                file_bytes: 0,
                restorable: false,
                pid: 0,
                started_at: 0,
                updated_at: 0,
                interrupted_tasks: Vec::new(),
                message: "The session recovery marker is damaged or from an incompatible version. \
                          Discard the recovery marker to continue; your project files will not be \
                          deleted."
                    .into(),
            }),
            PendingMarker::Valid {
                marker_path,
                marker,
            } => {
                let path = Path::new(&marker.project_path);
                let meta = std::fs::metadata(path).ok().filter(|m| m.is_file());
                let file_exists = meta.is_some();
                let file_bytes = meta.map_or(0, |m| m.len());
                let disk_encrypted = file_exists
                    && !marker.ephemeral_key
                    && chosproxy_storage::is_encrypted_file(path).unwrap_or(marker.encrypted);
                let encrypted = marker.encrypted || disk_encrypted;

                let mut tasks = marker.tasks.clone();
                if file_exists
                    && !encrypted
                    && let Ok(db) = ProjectDb::open(
                        path,
                        chosproxy_storage::OpenOptions {
                            read_only: true,
                            ..Default::default()
                        },
                    )
                    && let Ok(stored_tasks) = db.tasks()
                {
                    for row in stored_tasks {
                        if !row.status.is_finished() && !tasks.iter().any(|t| t.id == row.id) {
                            tasks.push(RecoveryTask {
                                id: row.id,
                                kind: row.kind,
                                name: row.name,
                            });
                        }
                    }
                }
                tasks.sort_by_key(|t| t.id);

                let restorable = file_exists && !marker.ephemeral_key;
                let message = if !file_exists {
                    format!(
                        "The project file at {} is missing (it may have been moved or deleted). \
                         Discard the recovery marker or move the file back before restoring.",
                        marker.project_path
                    )
                } else if marker.ephemeral_key {
                    "This temporary project was encrypted with an ephemeral in-memory key that \
                     is never written to disk, so it cannot be unlocked after an unexpected exit. \
                     Discard the recovery marker to clean it up."
                        .into()
                } else if !tasks.is_empty() {
                    "ChosProxy exited unexpectedly while tasks were running. Restoring will \
                     reopen the project, mark active traffic tasks as interrupted without \
                     restarting them, and resume passive live tasks."
                        .into()
                } else {
                    "ChosProxy exited unexpectedly while this project was open. You can restore \
                     the project, open it in read-only mode to inspect it without modifying \
                     the file, or discard the recovery marker."
                        .into()
                };

                Some(RecoveryStatus {
                    marker_path: marker_path.display().to_string(),
                    marker_corrupt: false,
                    project_name: marker.project_name,
                    project_path: marker.project_path,
                    project_kind: marker.project_kind,
                    encrypted,
                    ephemeral_key: marker.ephemeral_key,
                    file_exists,
                    file_bytes,
                    restorable,
                    pid: marker.pid,
                    started_at: marker.started_at,
                    updated_at: marker.updated_at,
                    interrupted_tasks: tasks,
                    message,
                })
            }
        }
    }

    /// Restores the project from a pending crash recovery marker.
    ///
    /// Checks encryption and `PRAGMA quick_check` before closing any open
    /// project or removing the crash marker, so a wrong password or damaged
    /// database keeps the recovery marker intact.
    pub async fn recovery_restore(&self, password: Option<&str>) -> Result<ProjectInfo, AppError> {
        let pending = self
            .find_pending_marker()
            .ok_or_else(|| AppError::Invalid("no crash recovery is pending".into()))?;
        let PendingMarker::Valid {
            marker_path,
            marker,
        } = pending
        else {
            return Err(AppError::Invalid(
                "the recovery marker is corrupt; discard it instead".into(),
            ));
        };
        if marker.ephemeral_key {
            return Err(AppError::Invalid(
                "this encrypted temporary project used an ephemeral in-memory key and cannot be \
                 restored"
                    .into(),
            ));
        }
        let path = PathBuf::from(&marker.project_path);
        if !path.is_file() {
            return Err(AppError::Invalid(format!(
                "the project file {} was not found",
                path.display()
            )));
        }
        let key = password
            .filter(|p| !p.is_empty())
            .map(ProjectKey::password)
            .transpose()?;

        let info = self
            .activate_with(
                path,
                marker.project_kind,
                marker.project_name.clone(),
                key,
                false,
                true,
            )
            .await?;

        // Remove the old crash marker and promote our session marker to the
        // primary path when available.
        remove_if_owned(&marker_path, &marker.session_id);
        let current_marker = lock(&self.inner.session_marker_path).clone();
        let primary = self.inner.paths.data_dir.join(SESSION_MARKER_FILE);
        if current_marker.as_ref() != Some(&primary) && !primary.exists() {
            if let Some(old_own) = lock(&self.inner.session_marker_path).take() {
                remove_if_owned(&old_own, &self.inner.session_id);
            }
            self.write_session_marker();
        }
        self.audit("project.recovered", "restored after unexpected exit");
        Ok(info)
    }

    /// Opens an existing project file in read-only mode (no writer thread, no
    /// migrations, no listeners, no background tasks, and no file changes).
    pub async fn open_project_read_only(
        &self,
        path: &Path,
        password: Option<&str>,
    ) -> Result<ProjectInfo, AppError> {
        let key = password
            .filter(|p| !p.is_empty())
            .map(ProjectKey::password)
            .transpose()?;
        if !path.is_file() {
            return Err(AppError::Invalid(format!(
                "{} is not a file",
                path.display()
            )));
        }
        let name = display_name(path);
        self.activate_with(
            path.to_path_buf(),
            ProjectKind::Persistent,
            name,
            key,
            true,
            false,
        )
        .await
    }

    /// Opens the crashed session's project in read-only mode and clears the
    /// crash marker once opened.
    pub async fn recovery_open_read_only(
        &self,
        password: Option<&str>,
    ) -> Result<ProjectInfo, AppError> {
        let pending = self
            .find_pending_marker()
            .ok_or_else(|| AppError::Invalid("no crash recovery is pending".into()))?;
        let PendingMarker::Valid {
            marker_path,
            marker,
        } = pending
        else {
            return Err(AppError::Invalid(
                "the recovery marker is corrupt; discard it instead".into(),
            ));
        };
        if marker.ephemeral_key {
            return Err(AppError::Invalid(
                "this encrypted temporary project used an ephemeral in-memory key and cannot be \
                 opened"
                    .into(),
            ));
        }
        let path = PathBuf::from(&marker.project_path);
        if !path.is_file() {
            return Err(AppError::Invalid(format!(
                "the project file {} was not found",
                path.display()
            )));
        }
        let key = password
            .filter(|p| !p.is_empty())
            .map(ProjectKey::password)
            .transpose()?;

        let info = self
            .activate_with(
                path,
                marker.project_kind,
                marker.project_name.clone(),
                key,
                true,
                false,
            )
            .await?;
        remove_if_owned(&marker_path, &marker.session_id);
        self.inner.log.record(
            NoticeLevel::Info,
            LogCategory::Project,
            format!(
                "Opened {} ({}) in read-only mode from crash recovery",
                info.name, info.path
            ),
        );
        Ok(info)
    }

    /// Discards the pending crash recovery marker. Persistent project files are
    /// never deleted; an orphaned temporary project in `paths.temp_dir` is
    /// deleted only when `delete_orphan_temp` is `true`.
    pub fn recovery_discard(&self, delete_orphan_temp: bool) -> Result<(), AppError> {
        let pending = self
            .find_pending_marker()
            .ok_or_else(|| AppError::Invalid("no crash recovery is pending".into()))?;
        let marker_path = pending.path().to_path_buf();
        if let Err(e) = std::fs::remove_file(&marker_path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            return Err(AppError::Io(e));
        }

        let mut deleted_temp = false;
        if let PendingMarker::Valid { marker, .. } = &pending {
            if delete_orphan_temp && marker.project_kind == ProjectKind::Temporary {
                let p = Path::new(&marker.project_path);
                let in_temp_dir = p.parent() == Some(self.inner.paths.temp_dir.as_path());
                let valid_temp_name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("temp-") && n.ends_with(".chosproxy"));
                let currently_open = read(&self.inner.project)
                    .as_ref()
                    .is_some_and(|open| open.path == marker.project_path);
                if in_temp_dir && valid_temp_name && !currently_open {
                    for suffix in ["", "-wal", "-shm"] {
                        let target = PathBuf::from(format!("{}{suffix}", marker.project_path));
                        if let Err(e) = std::fs::remove_file(&target)
                            && e.kind() != std::io::ErrorKind::NotFound
                        {
                            tracing::warn!(
                                path = %target.display(),
                                error = %e,
                                "could not delete orphaned temporary project file"
                            );
                        }
                    }
                    deleted_temp = true;
                }
            }
            self.inner.log.record(
                NoticeLevel::Info,
                LogCategory::Project,
                format!(
                    "Discarded crash recovery for {}{}",
                    marker.project_name,
                    if deleted_temp {
                        " and removed orphaned temporary project"
                    } else {
                        ""
                    }
                ),
            );
            self.audit(
                "project.recovery.discarded",
                &format!(
                    "{}{}",
                    marker.project_path,
                    if deleted_temp {
                        " (temporary project deleted)"
                    } else {
                        ""
                    }
                ),
            );
        } else {
            self.inner.log.record(
                NoticeLevel::Info,
                LogCategory::Project,
                "Discarded corrupt crash recovery marker".into(),
            );
            self.audit("project.recovery.discarded", "corrupt session marker");
        }
        Ok(())
    }
}

fn remove_if_owned(path: &Path, session_id: &str) {
    if let Ok(marker) = read_marker_file(path)
        && marker.session_id == session_id
        && let Err(e) = std::fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(path = %path.display(), error = %e, "could not remove session marker");
    }
}
