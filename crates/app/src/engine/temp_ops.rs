//! Temporary files (M31, spec §117): what ChosProxy keeps in its own
//! temporary directory, how much space it takes, and cleaning it.
//!
//! Rules, so nothing is deleted by mistake:
//! - only entries directly inside `paths.temp_dir`, and only names that
//!   ChosProxy itself creates (`temp-*.chosproxy[-wal|-shm]`, `browser-*`
//!   profile directories, `api-report-*.tmp`); anything else is listed as
//!   "other" and never touched;
//! - never the open project's files or the running lab browser's profile;
//! - leftovers of this process (its process id is in the name) are removed
//!   by "Clean now" and when the engine shuts down;
//! - leftovers of other runs — which might be another ChosProxy using the
//!   same directory right now — are never removed automatically, and only
//!   on request once untouched for a day;
//! - symbolic links are never followed.

use super::{Engine, read};
use crate::AppError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use ts_rs::TS;

/// An entry from another run may be removed on request after this long
/// without changes.
const OTHER_RUN_AGE: Duration = Duration::from_secs(24 * 3600);
/// Entries listed (the totals count every entry).
const MAX_LISTED: usize = 500;

/// What a temporary entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TempKind {
    /// A temporary project database (or its WAL / shared-memory file).
    TemporaryProject,
    /// A lab browser profile (also used by the DOM Inspector and browser
    /// crawls).
    BrowserProfile,
    /// A report written for the automation API before it is returned.
    ReportStaging,
    /// Not made by ChosProxy: never removed.
    Other,
}

/// One entry of the temporary directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TempEntry {
    /// File or directory name.
    pub name: String,
    /// What it is.
    pub kind: TempKind,
    /// Size in bytes (a directory: its files).
    #[ts(type = "number")]
    pub bytes: u64,
    /// Last change, ms since the epoch (0 when unknown).
    #[ts(type = "number")]
    pub modified_at: i64,
    /// Made by this run of ChosProxy.
    pub this_run: bool,
    /// The open project or the running lab browser uses it.
    pub in_use: bool,
    /// "Clean now" removes it.
    pub cleanable: bool,
    /// It can be removed on request (an old leftover of another run).
    pub removable: bool,
}

/// The temporary directory at a glance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TempStatus {
    /// The directory.
    pub dir: String,
    /// Total size.
    #[ts(type = "number")]
    pub total_bytes: u64,
    /// What "Clean now" would free.
    #[ts(type = "number")]
    pub cleanable_bytes: u64,
    /// Entries, largest first (at most 500).
    pub entries: Vec<TempEntry>,
    /// Entries not listed.
    #[ts(type = "number")]
    pub more: u64,
}

/// What a cleanup removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TempCleanup {
    /// Entries removed.
    #[ts(type = "number")]
    pub removed: u64,
    /// Bytes freed.
    #[ts(type = "number")]
    pub bytes: u64,
    /// Entries that could not be removed, with why.
    pub failed: Vec<String>,
}

/// The kind of an entry from its name, and the process id in the name.
pub(crate) fn classify(name: &str, is_dir: bool) -> (TempKind, Option<u32>) {
    let pid_after = |rest: &str| rest.split('-').next().and_then(|p| p.parse::<u32>().ok());
    if !is_dir
        && let Some(rest) = name.strip_prefix("temp-")
        && [".chosproxy", ".chosproxy-wal", ".chosproxy-shm"]
            .iter()
            .any(|s| rest.ends_with(s))
    {
        return (TempKind::TemporaryProject, pid_after(rest));
    }
    if is_dir && let Some(rest) = name.strip_prefix("browser-") {
        return (TempKind::BrowserProfile, pid_after(rest));
    }
    if !is_dir
        && let Some(rest) = name.strip_prefix("api-report-")
        && let Some(pid) = rest.strip_suffix(".tmp")
    {
        return (TempKind::ReportStaging, pid.parse().ok());
    }
    (TempKind::Other, None)
}

/// Size of a file, or of the files under a directory (links not followed).
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    let mut seen = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            seen += 1;
            if seen > 200_000 {
                return total;
            }
            let Ok(m) = std::fs::symlink_metadata(e.path()) else {
                continue;
            };
            if m.is_dir() {
                stack.push(e.path());
            } else if m.is_file() {
                total += m.len();
            }
        }
    }
    total
}

/// Lists the temporary directory. `protected`: paths in use.
pub(crate) fn scan(
    dir: &Path,
    protected: &[PathBuf],
    now: SystemTime,
) -> std::io::Result<Vec<TempEntry>> {
    let me = std::process::id();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let name = e.file_name().to_string_lossy().into_owned();
        let link = meta.file_type().is_symlink();
        let (kind, pid) = if link {
            (TempKind::Other, None)
        } else {
            classify(&name, meta.is_dir())
        };
        let modified = meta.modified().ok();
        let age = modified.and_then(|m| now.duration_since(m).ok());
        let in_use = protected.iter().any(|p| p == &path);
        let this_run = pid == Some(me);
        let ours = kind != TempKind::Other;
        out.push(TempEntry {
            bytes: if link { 0 } else { size_of(&path) },
            modified_at: modified
                .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX)),
            cleanable: ours && !in_use && this_run,
            removable: ours && !in_use && !this_run && age.is_some_and(|a| a >= OTHER_RUN_AGE),
            name,
            kind,
            this_run,
            in_use,
        });
    }
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));
    Ok(out)
}

/// Removes one entry of `dir` named `name` (never a link, never outside).
fn remove_entry(dir: &Path, name: &str) -> std::io::Result<u64> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(std::io::Error::other("not an entry name"));
    }
    let path = dir.join(name);
    let meta = std::fs::symlink_metadata(&path)?;
    let bytes = size_of(&path);
    if meta.file_type().is_symlink() {
        return Err(std::io::Error::other("links are not removed"));
    }
    if meta.is_dir() {
        std::fs::remove_dir_all(&path)?;
    } else {
        std::fs::remove_file(&path)?;
    }
    Ok(bytes)
}

/// Removes the entries `pick` chooses.
fn remove_where(
    dir: &Path,
    entries: &[TempEntry],
    pick: impl Fn(&TempEntry) -> bool,
) -> TempCleanup {
    let mut done = TempCleanup {
        removed: 0,
        bytes: 0,
        failed: Vec::new(),
    };
    for e in entries.iter().filter(|e| pick(e)) {
        match remove_entry(dir, &e.name) {
            Ok(b) => {
                done.removed += 1;
                done.bytes += b;
            }
            Err(err) => done.failed.push(format!("{}: {err}", e.name)),
        }
    }
    done
}

impl Engine {
    /// Paths in use: the open project's files, the lab browser profile, and
    /// any temporary project referenced by a pending crash recovery marker or
    /// live sibling session.
    async fn temp_protected(&self) -> Vec<PathBuf> {
        let mut out = self.temp_recovery_protected();
        if let Some(p) = read(&self.inner.project) {
            for suffix in ["", "-wal", "-shm"] {
                out.push(PathBuf::from(format!("{}{suffix}", p.path)));
            }
        }
        if let Some(profile) = self.inner.browsers.status().await.profile_dir {
            out.push(PathBuf::from(profile));
        }
        out
    }

    /// The temporary directory: size, entries and what can be cleaned.
    pub async fn temp_status(&self) -> Result<TempStatus, AppError> {
        let dir = self.inner.paths.temp_dir.clone();
        let protected = self.temp_protected().await;
        let scan_dir = dir.clone();
        let entries =
            tokio::task::spawn_blocking(move || scan(&scan_dir, &protected, SystemTime::now()))
                .await??;
        let total_bytes = entries.iter().map(|e| e.bytes).sum();
        let cleanable_bytes = entries
            .iter()
            .filter(|e| e.cleanable)
            .map(|e| e.bytes)
            .sum();
        let more = entries.len().saturating_sub(MAX_LISTED) as u64;
        Ok(TempStatus {
            dir: dir.display().to_string(),
            total_bytes,
            cleanable_bytes,
            entries: entries.into_iter().take(MAX_LISTED).collect(),
            more,
        })
    }

    /// "Clean now": removes this run's leftovers that nothing uses.
    pub async fn temp_cleanup(&self) -> Result<TempCleanup, AppError> {
        let dir = self.inner.paths.temp_dir.clone();
        let protected = self.temp_protected().await;
        let done = tokio::task::spawn_blocking(move || -> std::io::Result<TempCleanup> {
            let entries = scan(&dir, &protected, SystemTime::now())?;
            Ok(remove_where(&dir, &entries, |e| e.cleanable))
        })
        .await??;
        if done.removed > 0 {
            self.audit(
                "temp.cleaned",
                &format!("{} entries, {} bytes", done.removed, done.bytes),
            );
        }
        Ok(done)
    }

    /// Removes old leftovers of other runs that the tester picked by name.
    /// Each must still qualify (ChosProxy's own kind, not in use, unchanged
    /// for a day); others are refused.
    pub async fn temp_remove(&self, names: Vec<String>) -> Result<TempCleanup, AppError> {
        if names.is_empty() || names.len() > MAX_LISTED {
            return Err(AppError::Invalid("choose the entries to remove".into()));
        }
        let dir = self.inner.paths.temp_dir.clone();
        let protected = self.temp_protected().await;
        let done = tokio::task::spawn_blocking(move || -> std::io::Result<TempCleanup> {
            let entries = scan(&dir, &protected, SystemTime::now())?;
            let mut done = remove_where(&dir, &entries, |e| {
                names.contains(&e.name) && (e.removable || e.cleanable)
            });
            for n in &names {
                if !entries
                    .iter()
                    .any(|e| &e.name == n && (e.removable || e.cleanable))
                {
                    done.failed.push(format!(
                        "{n}: kept (in use, recent, or not a ChosProxy file)"
                    ));
                }
            }
            Ok(done)
        })
        .await??;
        if done.removed > 0 {
            self.audit(
                "temp.removed",
                &format!("{} entries, {} bytes", done.removed, done.bytes),
            );
        }
        Ok(done)
    }

    /// Removes this run's leftovers while shutting down (after the project
    /// and the lab browser are closed).
    pub(crate) fn temp_cleanup_on_exit(&self) {
        let dir = self.inner.paths.temp_dir.clone();
        let protected = self.temp_recovery_protected();
        match scan(&dir, &protected, SystemTime::now()) {
            Ok(entries) => {
                let done = remove_where(&dir, &entries, |e| e.cleanable);
                for f in done.failed {
                    tracing::warn!(entry = %f, "could not remove a temporary file at exit");
                }
            }
            Err(e) => tracing::warn!(error = %e, "could not read the temporary directory"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_chosproxy_names_are_classified() {
        assert_eq!(
            classify("temp-123-456-0.chosproxy", false),
            (TempKind::TemporaryProject, Some(123))
        );
        assert_eq!(
            classify("temp-123-456-0.chosproxy-wal", false).0,
            TempKind::TemporaryProject
        );
        assert_eq!(
            classify("browser-77-999", true),
            (TempKind::BrowserProfile, Some(77))
        );
        assert_eq!(
            classify("api-report-5.tmp", false),
            (TempKind::ReportStaging, Some(5))
        );
        for (name, dir) in [
            ("temp-1-2-3.chosproxy", true),
            ("browser-1-2", false),
            ("my-project.chosproxy", false),
            ("temp-notes.txt", false),
            ("api-report-x.pdf", false),
        ] {
            assert_eq!(classify(name, dir).0, TempKind::Other, "{name}");
        }
    }
}
