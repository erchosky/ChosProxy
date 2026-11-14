//! Lifecycle of the (single) running lab browser.

use crate::AppError;
use crate::browser::{self, BrowserInfo, BrowserKind, LaunchSpec};
use chosproxy_core::event::EngineEvent;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast, oneshot};
use ts_rs::TS;

/// State of the lab browser.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrowserStatus {
    /// A lab browser started by this app is running.
    pub running: bool,
    /// Which browser.
    pub browser: Option<BrowserInfo>,
    /// Proxy address it uses.
    pub proxy: Option<String>,
    /// Temporary profile (deleted when the browser exits).
    pub profile_dir: Option<String>,
    /// Limitations of this session (e.g. CA not imported in Firefox).
    pub warning: Option<String>,
}

struct Session {
    id: u64,
    status: BrowserStatus,
    kill: Option<oneshot::Sender<()>>,
    exited: Option<oneshot::Receiver<()>>,
}

pub(crate) struct BrowserSessions {
    temp_root: PathBuf,
    current: Arc<Mutex<Option<Session>>>,
    events: broadcast::Sender<EngineEvent>,
    next_id: std::sync::atomic::AtomicU64,
}

/// Removes a profile directory, retrying briefly while the browser releases it.
async fn remove_profile(dir: &Path) {
    for _ in 0..10 {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(300)).await,
        }
    }
    tracing::warn!(dir = %dir.display(), "could not delete lab browser profile");
}

/// Lab profiles older than this are leftovers of a crash and are removed at
/// startup (a running lab browser never outlives its app session that long
/// without the app noticing it).
const STALE_PROFILE_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// Deletes `browser-*` profile directories older than [`STALE_PROFILE_AGE`].
fn remove_stale_profiles(temp_root: &Path) {
    let Ok(entries) = std::fs::read_dir(temp_root) else {
        return;
    };
    for entry in entries.flatten() {
        let is_profile = entry.file_name().to_string_lossy().starts_with("browser-");
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > STALE_PROFILE_AGE);
        if is_profile && old && std::fs::remove_dir_all(entry.path()).is_ok() {
            tracing::info!(dir = %entry.path().display(), "removed stale lab browser profile");
        }
    }
}

impl BrowserSessions {
    pub(crate) fn new(temp_root: PathBuf, events: broadcast::Sender<EngineEvent>) -> Self {
        remove_stale_profiles(&temp_root);
        BrowserSessions {
            temp_root,
            current: Arc::new(Mutex::new(None)),
            events,
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    pub(crate) async fn status(&self) -> BrowserStatus {
        self.current
            .lock()
            .await
            .as_ref()
            .map(|s| s.status.clone())
            .unwrap_or_default()
    }

    /// Starts `info` with a fresh temporary profile.
    pub(crate) async fn launch(
        &self,
        info: BrowserInfo,
        proxy: std::net::SocketAddr,
        spki_pin: String,
        ca_pem: String,
        url: String,
        devtools: bool,
    ) -> Result<BrowserStatus, AppError> {
        if devtools && info.kind != BrowserKind::Chromium {
            return Err(AppError::Invalid(
                "the DOM Inspector needs a Chromium-family browser (Chromium, Chrome, Edge or Brave)"
                    .into(),
            ));
        }
        // Checked explicitly: the browser may have been uninstalled since it
        // was detected, and some environments only report exec failures in
        // the child process.
        if !browser::is_executable(Path::new(&info.id)) {
            return Err(AppError::Invalid(format!(
                "{} is no longer available at {}",
                info.name, info.id
            )));
        }
        let mut current = self.current.lock().await;
        if current.is_some() {
            return Err(AppError::Invalid(
                "a lab browser is already running; close it first".into(),
            ));
        }
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let profile_dir = self.temp_root.join(format!(
            "browser-{}-{}",
            std::process::id(),
            chosproxy_core::time::now_ms()
        ));
        chosproxy_core::fs::create_private_dir(&profile_dir)?;
        let spec = LaunchSpec {
            proxy,
            spki_pin,
            ca_pem,
            url,
            profile_dir: profile_dir.clone(),
            devtools,
        };
        let mut warning = None;
        let args = match info.kind {
            BrowserKind::Chromium => {
                let root = browser::running_as_root();
                if root {
                    warning = Some(
                        "Running as root: Chromium needs --no-sandbox, which weakens the \
                         browser's isolation. Prefer running ChosProxy as a normal user."
                            .to_string(),
                    );
                }
                browser::chromium_args(&spec, root)
            }
            BrowserKind::Firefox => {
                warning = browser::prepare_firefox_profile(&spec)?;
                browser::firefox_args(&spec)
            }
        };
        let child = tokio::process::Command::new(&info.id)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                remove_profile(&profile_dir).await;
                return Err(AppError::Invalid(format!(
                    "cannot start {}: {e}",
                    info.name
                )));
            }
        };
        tracing::info!(browser = %info.name, %proxy, "lab browser started");
        let status = BrowserStatus {
            running: true,
            browser: Some(info),
            proxy: Some(proxy.to_string()),
            profile_dir: Some(profile_dir.display().to_string()),
            warning,
        };
        let (kill_tx, kill_rx) = oneshot::channel();
        let (exit_tx, exit_rx) = oneshot::channel();
        *current = Some(Session {
            id,
            status: status.clone(),
            kill: Some(kill_tx),
            exited: Some(exit_rx),
        });
        drop(current);
        let _ = self.events.send(EngineEvent::BrowserChanged);

        let slot = self.current.clone();
        let events = self.events.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = child.wait() => {}
                _ = kill_rx => {
                    let _ = child.kill().await;
                }
            }
            remove_profile(&profile_dir).await;
            {
                let mut s = slot.lock().await;
                if s.as_ref().is_some_and(|s| s.id == id) {
                    *s = None;
                }
            }
            tracing::info!("lab browser exited; temporary profile removed");
            let _ = events.send(EngineEvent::BrowserChanged);
            let _ = exit_tx.send(());
        });
        Ok(status)
    }

    /// Closes the lab browser (if any) and waits until its profile is gone.
    pub(crate) async fn close(&self) {
        let exited = {
            let mut current = self.current.lock().await;
            let Some(session) = current.as_mut() else {
                return;
            };
            if let Some(kill) = session.kill.take() {
                let _ = kill.send(());
            }
            session.exited.take()
        };
        if let Some(rx) = exited {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(10), rx).await;
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in "browser" that records its arguments and waits.
    fn fake_browser(dir: &Path) -> BrowserInfo {
        let exe = dir.join("fake-chromium");
        let log = dir.join("args.txt");
        std::fs::write(
            &exe,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nexec sleep 30\n",
                log.display()
            ),
        )
        .expect("write fake browser script");
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
            .expect("make fake browser executable");
        BrowserInfo {
            id: exe.display().to_string(),
            name: "Fake".into(),
            kind: BrowserKind::Chromium,
        }
    }

    #[tokio::test]
    async fn launches_once_closes_and_removes_profile() {
        let dir = tempfile::tempdir().unwrap();
        let (events, mut rx) = broadcast::channel(16);
        let sessions = BrowserSessions::new(dir.path().join("tmp"), events);
        let info = fake_browser(dir.path());
        let status = sessions
            .launch(
                info.clone(),
                "127.0.0.1:8080".parse().unwrap(),
                "PIN".into(),
                String::new(),
                "http://chosproxy/".into(),
                false,
            )
            .await
            .unwrap();
        assert!(status.running);
        let profile = PathBuf::from(status.profile_dir.unwrap());
        assert!(profile.is_dir());
        assert!(matches!(
            rx.recv().await.unwrap(),
            EngineEvent::BrowserChanged
        ));

        // Arguments reached the process.
        let args_file = dir.path().join("args.txt");
        // Generous: the fake browser is slow to start when the machine is busy.
        // The file exists as soon as the shell opens it, before the
        // arguments are written: wait for the content, not for the file.
        let mut args = String::new();
        for _ in 0..500 {
            args = std::fs::read_to_string(&args_file).unwrap_or_default();
            if args.contains("--ignore-certificate-errors-spki-list") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(args.contains("--proxy-server=http://127.0.0.1:8080"));
        assert!(args.contains("--ignore-certificate-errors-spki-list=PIN"));

        // Only one at a time.
        let second = sessions
            .launch(
                info,
                "127.0.0.1:8080".parse().unwrap(),
                "P".into(),
                String::new(),
                "http://x/".into(),
                false,
            )
            .await;
        assert!(second.is_err());

        sessions.close().await;
        assert!(!sessions.status().await.running);
        assert!(!profile.exists(), "temporary profile deleted");
    }

    #[tokio::test]
    async fn missing_executable_is_an_error_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (events, _) = broadcast::channel(16);
        let sessions = BrowserSessions::new(dir.path().join("tmp"), events);
        let info = BrowserInfo {
            id: dir.path().join("nope").display().to_string(),
            name: "Missing".into(),
            kind: BrowserKind::Chromium,
        };
        let r = sessions
            .launch(
                info,
                "127.0.0.1:1".parse().unwrap(),
                String::new(),
                String::new(),
                "http://x/".into(),
                false,
            )
            .await;
        assert!(r.is_err());
        let leftovers = std::fs::read_dir(dir.path().join("tmp")).map_or(0, |d| d.count());
        assert_eq!(leftovers, 0, "no profile directory left behind");
        assert!(!sessions.status().await.running);
    }
}
