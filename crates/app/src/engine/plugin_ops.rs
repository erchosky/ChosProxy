//! Plugins (spec §36–37, M14, ADR-0025): discovery in the plugins folder,
//! approval and grants (`plugins.toml`), and the host that runs each enabled
//! plugin as a separate process while a project is open, feeds it traffic
//! and checks every message it sends against the granted permissions.

use super::Engine;
use super::lock;
use crate::AppError;
use chosproxy_core::config::{ConfigSection, PluginGrant, PluginsConfig};
use chosproxy_core::event::EngineEvent;
use chosproxy_core::time::now_ms;
use chosproxy_http::content::{body_text, split_message};
use chosproxy_http::{RequestHead, ResponseHead};
use chosproxy_plugin::{
    Exchange, HostMessage, MANIFEST_FILE, MAX_BODY, MAX_LINE, Manifest, Permission, PluginFinding,
    PluginMessage, Query, Verified, hash_dir, key_fingerprint, verify_dir,
};
use chosproxy_storage::model::Highlight;
use chosproxy_storage::{Confidence, FindingSeverity, PassiveIssue, ProjectDb, StoredExchange};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, oneshot};
use ts_rs::TS;

/// Log lines kept per plugin.
const MAX_LOG: usize = 500;
/// Time a plugin has to answer `hello`.
const READY_TIMEOUT: Duration = Duration::from_secs(10);
/// Time a write to a plugin may take before it is considered stuck.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Findings a plugin may report per minute.
const FINDINGS_PER_MINUTE: usize = 120;
/// Invalid messages tolerated before the plugin is stopped.
const MAX_INVALID: u32 = 20;

/// Where a plugin is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PluginStatus {
    /// Not enabled.
    Disabled,
    /// Enabled; waiting for a project to be opened.
    Stopped,
    /// Starting.
    Starting,
    /// Running.
    Running,
    /// Exited or was stopped because of an error.
    Crashed,
    /// Its files changed since it was enabled: approve it again.
    NeedsApproval,
    /// Its manifest cannot be used.
    Invalid,
}

/// A permission as shown to the tester.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PermissionInfo {
    /// The permission.
    pub permission: Permission,
    /// What granting it allows.
    pub description: String,
}

/// An installed plugin.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginInfo {
    /// Id (also the folder name).
    pub id: String,
    /// Name.
    pub name: String,
    /// Version.
    pub version: String,
    /// What it does.
    pub description: String,
    /// Who made it.
    pub author: String,
    /// Its folder.
    pub dir: String,
    /// The command it runs.
    pub command: Vec<String>,
    /// Permissions it asks for.
    pub requested: Vec<PermissionInfo>,
    /// Permissions granted.
    pub granted: Vec<Permission>,
    /// Enabled by the tester.
    pub enabled: bool,
    /// Where it is.
    pub status: PluginStatus,
    /// Why (errors, changes).
    pub message: Option<String>,
    /// What its signature shows.
    pub signature: PluginSignature,
}

/// One line of a plugin's log.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginLogLine {
    /// When, Unix epoch ms.
    #[ts(type = "number")]
    pub at: i64,
    /// `info`, `warning` or `error`.
    pub level: String,
    /// Text.
    pub message: String,
}

/// What the host knows about a plugin in this session.
#[derive(Default)]
pub(crate) struct Runtime {
    status: Option<(PluginStatus, Option<String>)>,
    logs: VecDeque<PluginLogLine>,
    stop: Option<oneshot::Sender<()>>,
    /// API v2: actions the plugin added to request menus (id, label).
    actions: Vec<(String, String)>,
    /// API v2: messages to write to the running plugin.
    pub(super) calls: Option<tokio::sync::mpsc::Sender<HostMessage>>,
    /// API v2: action calls waiting for their output.
    pending: HashMap<u64, oneshot::Sender<String>>,
    /// API v2 (`proxy.intercept`): host patterns of its request hook, once
    /// registered (empty = every in-scope host).
    pub(super) hook: Option<Vec<String>>,
    /// The hook also gets the responses (M31b).
    pub(super) hook_responses: bool,
    /// API v2: hook calls waiting for the plugin's decision.
    pub(super) hook_pending: HashMap<u64, oneshot::Sender<super::plugin_hooks::HookAnswer>>,
}

/// An action a plugin added to request menus (API v2, `ui.extend`).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginAction {
    /// Plugin id.
    pub plugin: String,
    /// Plugin name.
    pub plugin_name: String,
    /// Action id.
    pub action: String,
    /// Menu label.
    pub label: String,
}

/// What a plugin's signature shows (ADR-0033).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginSignature {
    /// `unsigned`, `invalid`, `untrusted` (valid, unknown key) or `trusted`.
    pub state: String,
    /// Publisher named in the signature.
    pub publisher: Option<String>,
    /// The signing key, Base64 (to trust it).
    pub key: Option<String>,
    /// Short key fingerprint, for comparing with the publisher.
    pub fingerprint: Option<String>,
    /// Why it is invalid.
    pub problem: Option<String>,
}

fn signature_of(v: &Verified, config: &PluginsConfig) -> PluginSignature {
    let none = |state: &str, problem: Option<String>| PluginSignature {
        state: state.into(),
        publisher: None,
        key: None,
        fingerprint: None,
        problem,
    };
    match v {
        Verified::Unsigned => none("unsigned", None),
        Verified::Invalid(e) => none("invalid", Some(e.clone())),
        Verified::Valid { publisher, key } => {
            let trusted = config.trusted.iter().any(|t| &t.key == key);
            PluginSignature {
                state: if trusted { "trusted" } else { "untrusted" }.into(),
                publisher: Some(publisher.clone()),
                key: Some(key.clone()),
                fingerprint: Some(key_fingerprint(key)),
                problem: None,
            }
        }
    }
}

/// Requests one plugin may send per minute (API v2, `network.send`).
const SENDS_PER_MINUTE: usize = 300;
/// Requests one plugin may have in flight.
const SENDS_IN_FLIGHT: usize = 4;
/// Actions one plugin may register.
const MAX_ACTIONS: usize = 20;
/// How long an action may take to answer.
const ACTION_TIMEOUT: Duration = Duration::from_secs(30);

/// A plugin found in the plugins folder.
struct Found {
    id: String,
    dir: PathBuf,
    manifest: Result<Manifest, String>,
}

fn discover(dir: &Path) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Found> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| e.path().join(MANIFEST_FILE).is_file())
        .map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            let manifest = std::fs::read_to_string(path.join(MANIFEST_FILE))
                .map_err(|e| format!("cannot read {MANIFEST_FILE}: {e}"))
                .and_then(|t| Manifest::parse(&t))
                .and_then(|m| {
                    if m.id == id {
                        Ok(m)
                    } else {
                        Err(format!(
                            "the folder must be named after the plugin id ({})",
                            m.id
                        ))
                    }
                });
            Found {
                id,
                dir: path,
                manifest,
            }
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn text(bytes: &[u8]) -> String {
    let cut = &bytes[..bytes.len().min(MAX_BODY)];
    String::from_utf8_lossy(cut).into_owned()
}

fn headers_of(list: &[chosproxy_http::Header]) -> Vec<(String, String)> {
    list.iter()
        .map(|h| {
            (
                h.name.clone(),
                String::from_utf8_lossy(&h.value).into_owned(),
            )
        })
        .collect()
}

/// A stored exchange as plugins see it.
fn to_plugin(ex: &StoredExchange) -> Exchange {
    let (req_head, req_body) = split_message(&ex.request);
    let request_headers = RequestHead::parse(req_head.to_vec())
        .map(|h| headers_of(&h.headers))
        .unwrap_or_default();
    let (response_headers, response_body) = match ex.response.as_deref() {
        Some(raw) => {
            let (head, body) = split_message(raw);
            match ResponseHead::parse(head.to_vec()) {
                Ok(h) => {
                    let body = body_text(&h, body, MAX_BODY).unwrap_or_default();
                    (headers_of(&h.headers), body)
                }
                Err(_) => (Vec::new(), String::new()),
            }
        }
        None => (Vec::new(), String::new()),
    };
    Exchange {
        id: ex.row.id,
        source: ex.row.source.as_str().to_string(),
        method: ex.row.method.clone(),
        url: ex.row.url.clone(),
        host: ex.row.host.clone(),
        status: ex.row.status,
        request_headers,
        request_body: text(req_body),
        response_headers,
        response_body,
    }
}

impl Engine {
    /// The folder plugins are installed in (one sub-folder per plugin).
    pub fn plugins_dir(&self) -> PathBuf {
        self.inner.paths.data_dir.join("plugins")
    }

    fn plugins_config(&self) -> PluginsConfig {
        self.inner.store.load::<PluginsConfig>().value
    }

    pub(super) fn plugin_log(&self, id: &str, level: &str, message: impl Into<String>) {
        let message: String = message.into().chars().take(2000).collect();
        {
            let mut runtimes = lock(&self.inner.plugins);
            let rt = runtimes.entry(id.to_string()).or_default();
            if rt.logs.len() >= MAX_LOG {
                rt.logs.pop_front();
            }
            rt.logs.push_back(PluginLogLine {
                at: now_ms(),
                level: level.to_string(),
                message,
            });
        }
        let _ = self.inner.events.send(EngineEvent::PluginsChanged);
    }

    fn plugin_status(&self, id: &str, status: PluginStatus, message: Option<String>) {
        lock(&self.inner.plugins)
            .entry(id.to_string())
            .or_default()
            .status = Some((status, message));
        let _ = self.inner.events.send(EngineEvent::PluginsChanged);
    }

    /// Installed plugins with their grants and state.
    pub async fn plugins(&self) -> Result<Vec<PluginInfo>, AppError> {
        let dir = self.plugins_dir();
        let found = tokio::task::spawn_blocking(move || {
            discover(&dir)
                .into_iter()
                .map(|f| {
                    let hash = hash_dir(&f.dir);
                    let verified = f
                        .manifest
                        .as_ref()
                        .map(|m| verify_dir(&f.dir, m))
                        .unwrap_or(Verified::Unsigned);
                    (f, hash, verified)
                })
                .collect::<Vec<_>>()
        })
        .await?;
        let config = self.plugins_config();
        let runtimes = lock(&self.inner.plugins);
        Ok(found
            .into_iter()
            .map(|(f, hash, verified)| {
                let signature = signature_of(&verified, &config);
                let grant = config.get(&f.id).cloned().unwrap_or_default();
                let runtime = runtimes.get(&f.id).and_then(|r| r.status.clone());
                let dir = f.dir.display().to_string();
                match f.manifest {
                    Err(e) => PluginInfo {
                        id: f.id.clone(),
                        name: f.id,
                        version: String::new(),
                        description: String::new(),
                        author: String::new(),
                        dir,
                        command: Vec::new(),
                        requested: Vec::new(),
                        granted: Vec::new(),
                        enabled: false,
                        status: PluginStatus::Invalid,
                        message: Some(e),
                        signature,
                    },
                    Ok(m) => {
                        let requested = m.requested();
                        let granted: Vec<Permission> = grant
                            .granted
                            .iter()
                            .filter_map(|g| Permission::parse(g))
                            .filter(|p| requested.contains(p))
                            .collect();
                        let blocked = config.require_signed && signature.state != "trusted";
                        let (status, message) = match (&hash, grant.enabled) {
                            (Err(e), _) => (PluginStatus::Invalid, Some(e.clone())),
                            (_, false) => (PluginStatus::Disabled, None),
                            (_, true) if blocked => (
                                PluginStatus::NeedsApproval,
                                Some(
                                    "only plugins signed by a trusted publisher may run \
                                     (Plugins → Signatures)"
                                        .into(),
                                ),
                            ),
                            (Ok(h), true) if *h != grant.approved_hash => (
                                PluginStatus::NeedsApproval,
                                Some("its files changed since it was enabled".into()),
                            ),
                            _ => runtime.unwrap_or((PluginStatus::Stopped, None)),
                        };
                        PluginInfo {
                            id: m.id.clone(),
                            name: m.name.clone(),
                            version: m.version.clone(),
                            description: m.description.clone(),
                            author: m.author.clone(),
                            dir,
                            command: m.command.clone(),
                            requested: requested
                                .into_iter()
                                .map(|p| PermissionInfo {
                                    permission: p,
                                    description: p.description().to_string(),
                                })
                                .collect(),
                            granted,
                            enabled: grant.enabled,
                            status,
                            message,
                            signature,
                        }
                    }
                }
            })
            .collect())
    }

    /// Enables (approving its current files) or disables a plugin and sets
    /// the permissions it is granted. Restarts it if a project is open.
    pub async fn plugin_set(
        &self,
        id: &str,
        enabled: bool,
        granted: Vec<Permission>,
    ) -> Result<PluginInfo, AppError> {
        let dir = self.plugins_dir().join(id);
        let found = {
            let root = self.plugins_dir();
            let wanted = id.to_string();
            tokio::task::spawn_blocking(move || {
                discover(&root).into_iter().find(|f| f.id == wanted)
            })
            .await?
        }
        .ok_or_else(|| {
            AppError::Invalid(format!(
                "no plugin {id:?} in {}",
                self.plugins_dir().display()
            ))
        })?;
        let manifest = found.manifest.map_err(AppError::Invalid)?;
        let requested = manifest.requested();
        if let Some(extra) = granted.iter().find(|p| !requested.contains(p)) {
            return Err(AppError::Invalid(format!(
                "the plugin does not ask for {}",
                extra.as_str()
            )));
        }
        let hash = tokio::task::spawn_blocking(move || hash_dir(&dir))
            .await?
            .map_err(AppError::Invalid)?;
        let mut config = self.plugins_config();
        let prev_granted: Vec<String> = config
            .plugin
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.granted.clone())
            .unwrap_or_default();
        let next_granted: Vec<String> = granted.iter().map(|p| p.as_str().to_string()).collect();
        config.plugin.retain(|p| p.id != id);
        config.plugin.push(PluginGrant {
            id: id.to_string(),
            enabled,
            granted: next_granted.clone(),
            approved_hash: if enabled { hash } else { String::new() },
        });
        config.plugin.sort_by(|a, b| a.id.cmp(&b.id));
        self.inner.store.save(&config)?;
        let added: Vec<&str> = next_granted
            .iter()
            .filter(|p| !prev_granted.contains(p))
            .map(String::as_str)
            .collect();
        let removed: Vec<&str> = prev_granted
            .iter()
            .filter(|p| !next_granted.contains(p))
            .map(String::as_str)
            .collect();
        if !added.is_empty() || !removed.is_empty() {
            self.audit(
                "plugin.permissions.changed",
                &format!(
                    "{id}: granted=[{}], revoked=[{}]",
                    added.join(", "),
                    removed.join(", ")
                ),
            );
        }
        self.audit(
            if enabled {
                "plugin.enabled"
            } else {
                "plugin.disabled"
            },
            &format!(
                "{id} {} with: {}",
                manifest.version,
                if granted.is_empty() {
                    "no permissions".to_string()
                } else {
                    granted
                        .iter()
                        .map(|p| p.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ),
        );
        self.stop_plugin(id).await;
        if enabled && self.db().is_ok() {
            self.start_plugin(manifest, granted).await;
        } else {
            self.plugin_status(
                id,
                if enabled {
                    PluginStatus::Stopped
                } else {
                    PluginStatus::Disabled
                },
                None,
            );
        }
        self.plugins()
            .await?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::Invalid(format!("plugin {id} disappeared")))
    }

    /// A plugin's log (this session).
    pub fn plugin_logs(&self, id: &str) -> Vec<PluginLogLine> {
        lock(&self.inner.plugins)
            .get(id)
            .map(|r| r.logs.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Copies the HelloPlugin example into the plugins folder (not enabled).
    pub async fn plugins_install_example(&self) -> Result<PathBuf, AppError> {
        let dir = self.plugins_dir().join("hello-plugin");
        let target = dir.clone();
        tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&target)?;
            std::fs::write(
                target.join(MANIFEST_FILE),
                include_str!("../../../../examples/plugins/hello-plugin/plugin.toml"),
            )?;
            std::fs::write(
                target.join("hello.py"),
                include_str!("../../../../examples/plugins/hello-plugin/hello.py"),
            )?;
            Ok::<_, std::io::Error>(())
        })
        .await??;
        let _ = self.inner.events.send(EngineEvent::PluginsChanged);
        Ok(dir)
    }

    /// Starts every enabled, approved plugin (a project was opened).
    pub(crate) async fn start_plugins(&self) {
        let Ok(list) = self.plugins().await else {
            return;
        };
        let root = self.plugins_dir();
        for info in list.into_iter().filter(|p| p.enabled) {
            if info.status == PluginStatus::NeedsApproval || info.status == PluginStatus::Invalid {
                continue;
            }
            let manifest = std::fs::read_to_string(root.join(&info.id).join(MANIFEST_FILE))
                .map_err(|e| e.to_string())
                .and_then(|t| Manifest::parse(&t));
            if let Ok(m) = manifest {
                self.start_plugin(m, info.granted).await;
            }
        }
    }

    /// Stops every running plugin (the project is closing).
    pub(crate) async fn stop_plugins(&self) {
        let ids: Vec<String> = lock(&self.inner.plugins).keys().cloned().collect();
        for id in ids {
            self.stop_plugin(&id).await;
        }
    }

    async fn stop_plugin(&self, id: &str) {
        let stop = lock(&self.inner.plugins)
            .get_mut(id)
            .and_then(|r| r.stop.take());
        if let Some(stop) = stop {
            let _ = stop.send(());
            // Give it a moment to exit on its own.
            for _ in 0..30 {
                let running = lock(&self.inner.plugins)
                    .get(id)
                    .and_then(|r| r.status.clone())
                    .is_some_and(|(s, _)| {
                        s == PluginStatus::Running || s == PluginStatus::Starting
                    });
                if !running {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    async fn start_plugin(&self, manifest: Manifest, granted: Vec<Permission>) {
        let (tx, rx) = oneshot::channel();
        let id = manifest.id.clone();
        lock(&self.inner.plugins)
            .entry(id.clone())
            .or_default()
            .stop = Some(tx);
        self.plugin_status(&id, PluginStatus::Starting, None);
        let engine = self.internal_clone();
        tokio::spawn(async move {
            let id = manifest.id.clone();
            let result = engine.run_plugin(&manifest, &granted, rx).await;
            let (status, message) = match result {
                Ok(()) => (PluginStatus::Stopped, None),
                Err(e) => {
                    engine.plugin_log(&id, "error", e.clone());
                    (PluginStatus::Crashed, Some(e))
                }
            };
            lock(&engine.inner.plugins)
                .entry(id.clone())
                .or_default()
                .stop = None;
            engine.plugin_status(&id, status, message);
        });
    }

    fn spawn_process(&self, manifest: &Manifest) -> Result<Child, String> {
        let dir = self.plugins_dir().join(&manifest.id);
        let mut cmd = Command::new(manifest.program(&dir));
        cmd.args(manifest.command.iter().skip(1))
            .current_dir(&dir)
            // Only what a program needs to run: no inherited secrets.
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for key in [
            "PATH",
            "HOME",
            "LANG",
            "TMPDIR",
            "SystemRoot",
            "TEMP",
            "USERPROFILE",
        ] {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
        }
        cmd.env("CHOSPROXY_PLUGIN_API", manifest.api.to_string());
        cmd.spawn()
            .map_err(|e| format!("could not start {:?}: {e}", manifest.command.join(" ")))
    }

    /// Runs one plugin until it is stopped, exits or misbehaves.
    async fn run_plugin(
        &self,
        manifest: &Manifest,
        granted: &[Permission],
        mut stop: oneshot::Receiver<()>,
    ) -> Result<(), String> {
        let id = manifest.id.clone();
        let mut child = self.spawn_process(manifest)?;
        let mut stdin = child.stdin.take().ok_or("no standard input")?;
        let stdout = child.stdout.take().ok_or("no standard output")?;
        let stderr = child.stderr.take().ok_or("no standard error")?;
        let mut lines = BufReader::new(stdout);
        let mut errors = BufReader::new(stderr).lines();
        let mut events = self.inner.events.subscribe();

        let hello = HostMessage::Hello {
            api: manifest.api,
            plugin: id.clone(),
            granted: granted.iter().map(|p| p.as_str().to_string()).collect(),
        };
        write(&mut stdin, &hello).await?;
        match tokio::time::timeout(READY_TIMEOUT, read_line(&mut lines)).await {
            Err(_) => return Err("did not answer within 10 s (no \"ready\" message)".into()),
            Ok(Err(e)) => return Err(e),
            Ok(Ok(None)) => return Err(exit_reason(&mut child).await),
            Ok(Ok(Some(line))) => {
                if PluginMessage::parse(&line) != Ok(PluginMessage::Ready) {
                    return Err(format!(
                        "expected \"ready\", got: {}",
                        line.chars().take(200).collect::<String>()
                    ));
                }
            }
        }
        self.plugin_status(&id, PluginStatus::Running, None);
        self.plugin_log(
            &id,
            "info",
            format!("Started ({})", manifest.command.join(" ")),
        );

        let reads = granted.contains(&Permission::NetworkRead);
        let mut cursor = match self.db() {
            Ok(db) => tokio::task::spawn_blocking(move || db.last_exchange_id())
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or(0),
            Err(_) => 0,
        };
        // A task of its own reads the plugin's output: reading a line is not
        // cancel-safe, so it must not race with the other events below.
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Result<Option<String>, String>>(64);
        let reader = tokio::spawn(async move {
            loop {
                let line = read_line(&mut lines).await;
                let end = !matches!(line, Ok(Some(_)));
                if out_tx.send(line).await.is_err() || end {
                    return;
                }
            }
        });
        let _reader_guard = AbortOnDrop(reader);
        let (reply_tx, mut reply_rx) = tokio::sync::mpsc::channel::<HostMessage>(64);
        {
            let mut plugins = lock(&self.inner.plugins);
            let rt = plugins.entry(id.clone()).or_default();
            rt.calls = Some(reply_tx.clone());
            rt.actions.clear();
            rt.pending.clear();
        }
        let _calls_guard = ClearCalls {
            engine: self.clone(),
            id: id.clone(),
        };
        let budget = Arc::new(SendBudget::default());
        let mut recent_findings: VecDeque<Instant> = VecDeque::new();
        let mut denied: HashSet<Permission> = HashSet::new();
        let mut invalid: u32 = 0;
        let mut stderr_open = true;

        loop {
            tokio::select! {
                _ = &mut stop => {
                    let _ = write(&mut stdin, &HostMessage::Shutdown).await;
                    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                    let _ = child.kill().await;
                    self.plugin_log(&id, "info", "Stopped.");
                    return Ok(());
                }
                status = child.wait() => {
                    let code = status.map(|s| s.to_string()).unwrap_or_else(|e| e.to_string());
                    return Err(format!("the plugin exited ({code})"));
                }
                event = events.recv(), if reads => {
                    match event {
                        Ok(EngineEvent::HistoryAppended { .. }) | Err(broadcast::error::RecvError::Lagged(_)) => {
                            cursor = self.feed(&mut stdin, cursor).await?;
                        }
                        Err(broadcast::error::RecvError::Closed) => return Ok(()),
                        Ok(_) => {}
                    }
                }
                reply = reply_rx.recv() => {
                    if let Some(msg) = reply {
                        write(&mut stdin, &msg).await?;
                    }
                }
                line = errors.next_line(), if stderr_open => {
                    match line {
                        Ok(Some(l)) => self.plugin_log(&id, "warning", format!("stderr: {l}")),
                        _ => stderr_open = false,
                    }
                }
                line = out_rx.recv() => {
                    let line = match line {
                        Some(Ok(Some(l))) => l,
                        Some(Ok(None)) | None => return Err(exit_reason(&mut child).await),
                        Some(Err(e)) => return Err(e),
                    };
                    if line.trim().is_empty() {
                        continue;
                    }
                    match PluginMessage::parse(&line) {
                        Ok(msg) => {
                            let ctx = CallCtx {
                                reply: &reply_tx,
                                budget: &budget,
                            };
                            self.handle(&id, granted, msg, &mut recent_findings, &mut denied, &ctx)
                                .await;
                        }
                        Err(e) => {
                            invalid += 1;
                            self.plugin_log(&id, "warning", e);
                            if invalid >= MAX_INVALID {
                                return Err(format!("stopped after {MAX_INVALID} invalid messages"));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Sends the exchanges stored after `cursor`; returns the new cursor.
    async fn feed(&self, stdin: &mut ChildStdin, mut cursor: i64) -> Result<i64, String> {
        let Ok(db) = self.db() else {
            return Ok(cursor);
        };
        loop {
            let db2 = db.clone();
            let batch = tokio::task::spawn_blocking(move || {
                let ids = db2.exchange_ids_after(cursor, 50)?;
                db2.exchanges_bulk(&ids)
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            let Some(last) = batch.last().map(|e| e.row.id) else {
                return Ok(cursor);
            };
            for ex in &batch {
                // Requests plugins send are not fed back (no loops).
                if ex.row.source == chosproxy_core::Source::Plugin {
                    continue;
                }
                write(stdin, &HostMessage::Exchange(Box::new(to_plugin(ex)))).await?;
            }
            cursor = last;
        }
    }

    /// Carries out one message if the plugin has the permission for it.
    async fn handle(
        &self,
        id: &str,
        granted: &[Permission],
        msg: PluginMessage,
        recent: &mut VecDeque<Instant>,
        denied: &mut HashSet<Permission>,
        ctx: &CallCtx<'_>,
    ) {
        let need = match &msg {
            PluginMessage::Finding(_) => Some(Permission::ScannerPassive),
            PluginMessage::Annotate { .. } => Some(Permission::ProjectWrite),
            PluginMessage::Send { .. } => Some(Permission::NetworkSend),
            PluginMessage::Query { .. } => Some(Permission::ProjectRead),
            PluginMessage::RegisterAction { .. } | PluginMessage::Output { .. } => {
                Some(Permission::UiExtend)
            }
            PluginMessage::RegisterHook { .. } | PluginMessage::HookResult { .. } => {
                Some(Permission::ProxyIntercept)
            }
            _ => None,
        };
        if let Some(p) = need
            && !granted.contains(&p)
        {
            // Calls always get an answer, even when refused.
            if let PluginMessage::Send { id: call, .. } | PluginMessage::Query { id: call, .. } =
                &msg
            {
                let _ = ctx
                    .reply
                    .try_send(failed(*call, format!("{} not granted", p.as_str())));
            }
            if denied.insert(p) {
                self.plugin_log(
                    id,
                    "warning",
                    format!(
                        "Refused: needs the {} permission, which was not granted.",
                        p.as_str()
                    ),
                );
                self.audit(
                    "plugin.denied",
                    &format!("{id}: {} not granted", p.as_str()),
                );
            }
            return;
        }
        match msg {
            PluginMessage::Ready => {}
            PluginMessage::Log { level, message } => {
                let level = match level.as_str() {
                    "warning" | "error" => level,
                    _ => "info".into(),
                };
                self.plugin_log(id, &level, message);
            }
            PluginMessage::Finding(f) => {
                let now = Instant::now();
                while recent
                    .front()
                    .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
                {
                    recent.pop_front();
                }
                if recent.len() >= FINDINGS_PER_MINUTE {
                    self.plugin_log(id, "warning", "Finding ignored: more than 120 per minute.");
                    return;
                }
                recent.push_back(now);
                if let Err(e) = self.plugin_finding(id, f).await {
                    self.plugin_log(id, "warning", format!("Finding refused: {e}"));
                }
            }
            PluginMessage::Annotate {
                exchange_id,
                highlight,
                note,
            } => {
                if let Err(e) = self.plugin_annotate(exchange_id, highlight, note).await {
                    self.plugin_log(id, "warning", format!("Annotation refused: {e}"));
                }
            }
            PluginMessage::Send {
                id: call,
                scheme,
                host,
                port,
                request,
            } => {
                if let Err(e) = ctx.budget.take() {
                    let _ = ctx.reply.try_send(failed(call, e));
                    return;
                }
                let engine = self.clone();
                let reply = ctx.reply.clone();
                let budget = ctx.budget.clone();
                let plugin = id.to_string();
                tokio::spawn(async move {
                    let result = engine
                        .plugin_send(&plugin, scheme, host, port, &request)
                        .await;
                    budget.release();
                    let _ = reply.send(answer(call, result)).await;
                });
            }
            PluginMessage::Query { id: call, query } => {
                let result = self.plugin_query(query).await;
                let _ = ctx.reply.try_send(answer(call, result));
            }
            PluginMessage::RegisterAction { action, label } => {
                let valid =
                    (1..=64).contains(&action.len()) && (1..=60).contains(&label.chars().count());
                let mut plugins = lock(&self.inner.plugins);
                let rt = plugins.entry(id.to_string()).or_default();
                if !valid || rt.actions.len() >= MAX_ACTIONS {
                    drop(plugins);
                    self.plugin_log(
                        id,
                        "warning",
                        "Action refused: invalid or too many actions.",
                    );
                    return;
                }
                rt.actions.retain(|(a, _)| a != &action);
                rt.actions.push((action, label));
                drop(plugins);
                let _ = self.inner.events.send(EngineEvent::PluginsChanged);
            }
            PluginMessage::Output { call, text } => {
                let waiting = lock(&self.inner.plugins)
                    .get_mut(id)
                    .and_then(|rt| rt.pending.remove(&call));
                if let Some(tx) = waiting {
                    let _ = tx.send(text.chars().take(256 * 1024).collect());
                }
            }
            PluginMessage::RegisterHook { hosts, responses } => {
                let valid = hosts.len() <= super::plugin_hooks::MAX_HOOK_HOSTS
                    && hosts.iter().all(|h| (1..=253).contains(&h.len()));
                if !valid {
                    self.plugin_log(id, "warning", "Hook refused: invalid host patterns.");
                    return;
                }
                let described = if hosts.is_empty() {
                    "every in-scope host".to_string()
                } else {
                    hosts.join(", ")
                };
                if let Some(rt) = lock(&self.inner.plugins).get_mut(id) {
                    rt.hook = Some(hosts);
                    rt.hook_responses = responses;
                }
                self.plugin_log(
                    id,
                    "info",
                    format!(
                        "Request hook registered for {described} (in-scope proxy requests only, \
                         with a scope defined){}.",
                        if responses {
                            "; it also gets their responses"
                        } else {
                            ""
                        }
                    ),
                );
                self.audit(
                    "plugin.hook",
                    &format!(
                        "{id}: request{} hook for {described}",
                        if responses { " and response" } else { "" }
                    ),
                );
            }
            PluginMessage::HookResult {
                call,
                action,
                request,
                response,
                reason,
            } => {
                let waiting = lock(&self.inner.plugins)
                    .get_mut(id)
                    .and_then(|rt| rt.hook_pending.remove(&call));
                if let Some(tx) = waiting {
                    let _ = tx.send(super::plugin_hooks::HookAnswer {
                        action,
                        request,
                        response,
                        reason,
                    });
                }
            }
        }
    }

    /// API v2 `send`: one request through the Request Engine, scope-checked
    /// and stored in the history (source `plugin`).
    async fn plugin_send(
        &self,
        plugin: &str,
        scheme: String,
        host: String,
        port: u16,
        text: &str,
    ) -> Result<serde_json::Value, String> {
        let target = chosproxy_storage::validate_target(&chosproxy_storage::RepeaterTarget {
            scheme,
            host,
            port,
        })
        .map_err(|e| e.to_string())?;
        let path = chosproxy_storage::request_path(text);
        self.ensure_in_scope(&target, &path, "plugin")
            .map_err(|e| e.to_string())?;
        let request =
            crate::repeater::request_bytes(text, true, true, true).map_err(|e| e.to_string())?;
        let https = crate::repeater::scheme_of(&target) == chosproxy_http::Scheme::Https;
        let security = self.security_for_host(&target.host, https).await?;
        let destination = chosproxy_request::Destination {
            host: target.host.clone(),
            port: target.port,
            tls_name: target.host.clone(),
        };
        let limits = chosproxy_request::SendLimits {
            max_response_bytes: 4 * 1024 * 1024,
            ..chosproxy_request::SendLimits::default()
        };
        let started_at = now_ms();
        let outcome = self
            .session_send(
                "plugin",
                chosproxy_core::Source::Plugin,
                &destination,
                &security,
                &request,
                &limits,
            )
            .await;
        let mut exchange = crate::repeater::captured(
            chosproxy_core::Source::Plugin,
            &target,
            request,
            started_at,
            &outcome,
        );
        exchange.notes = Some(
            [
                Some(format!("Sent by the plugin {plugin}.")),
                exchange.notes.take(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" "),
        );
        let db = self.db().map_err(|e| e.to_string())?;
        let exchange_id = tokio::task::spawn_blocking(move || db.insert(exchange))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        Ok(match outcome {
            Ok(sent) => {
                let mut response = String::from_utf8_lossy(&sent.response).into_owned();
                if response.len() > MAX_BODY {
                    let mut end = MAX_BODY;
                    while !response.is_char_boundary(end) {
                        end -= 1;
                    }
                    response.truncate(end);
                }
                serde_json::json!({
                    "exchangeId": exchange_id,
                    "status": sent.head.status,
                    "response": response,
                })
            }
            Err(e) => serde_json::json!({ "exchangeId": exchange_id, "error": e.message }),
        })
    }

    /// API v2 `query`: project data, read-only.
    async fn plugin_query(&self, query: Query) -> Result<serde_json::Value, String> {
        let db = self.db().map_err(|e| e.to_string())?;
        let value = match query {
            Query::Exchange { exchange_id } => {
                let found = tokio::task::spawn_blocking(move || db.exchanges_bulk(&[exchange_id]))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())?;
                let ex = found
                    .first()
                    .ok_or_else(|| format!("no exchange {exchange_id}"))?;
                serde_json::to_value(to_plugin(ex))
            }
            Query::History { search, limit } => {
                let q = chosproxy_storage::HistoryQuery {
                    limit: limit.clamp(1, 200),
                    search,
                    ..Default::default()
                };
                let page = tokio::task::spawn_blocking(move || db.history_page(&q))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(page)
            }
            Query::Findings => {
                let rows = tokio::task::spawn_blocking(move || {
                    db.findings(&chosproxy_storage::FindingQuery::default())
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
                serde_json::to_value(rows)
            }
        };
        value.map_err(|e| e.to_string())
    }

    /// Trusts (or stops trusting) a publisher key. Running plugins are
    /// restarted so the change applies.
    pub async fn plugin_trust(
        &self,
        key: String,
        name: String,
        trust: bool,
    ) -> Result<(), AppError> {
        let mut config = self.plugins_config();
        config.trusted.retain(|t| t.key != key);
        if trust {
            config
                .trusted
                .push(chosproxy_core::config::TrustedPublisher {
                    name: name.trim().chars().take(100).collect(),
                    key: key.clone(),
                });
        }
        config.validate().map_err(AppError::Invalid)?;
        self.inner.store.save(&config)?;
        self.audit(
            if trust {
                "plugin.publisher.trusted"
            } else {
                "plugin.publisher.untrusted"
            },
            &format!("{name} ({})", key_fingerprint(&key)),
        );
        self.restart_plugins().await;
        Ok(())
    }

    /// Only run plugins signed by a trusted publisher (or allow any).
    pub async fn plugins_require_signed(&self, require: bool) -> Result<(), AppError> {
        let mut config = self.plugins_config();
        config.require_signed = require;
        self.inner.store.save(&config)?;
        self.audit("plugin.require_signed", if require { "on" } else { "off" });
        self.restart_plugins().await;
        Ok(())
    }

    /// Trusted publishers and whether signatures are required.
    pub fn plugins_trust_settings(&self) -> PluginsConfig {
        let mut c = self.plugins_config();
        c.plugin.clear();
        c
    }

    async fn restart_plugins(&self) {
        self.stop_plugins().await;
        if self.db().is_ok() {
            self.start_plugins().await;
        }
        let _ = self.inner.events.send(EngineEvent::PluginsChanged);
    }

    /// Installs a plugin by copying its folder (from a catalog directory,
    /// ChosHub) into the plugins directory. It arrives disabled, with
    /// nothing granted; an existing plugin with the same id is not replaced.
    pub async fn plugins_install_from(&self, source: &Path) -> Result<PluginInfo, AppError> {
        let manifest = Manifest::load(source).map_err(AppError::Invalid)?;
        hash_dir(source).map_err(AppError::Invalid)?;
        let target = self.plugins_dir().join(&manifest.id);
        if target.exists() {
            return Err(AppError::Invalid(format!(
                "a plugin with the id {:?} is already installed; remove it first",
                manifest.id
            )));
        }
        let (from, to) = (source.to_path_buf(), target.clone());
        tokio::task::spawn_blocking(move || copy_dir(&from, &to, 0))
            .await?
            .map_err(AppError::Invalid)?;
        self.audit(
            "plugin.installed",
            &format!(
                "{} {} from {}",
                manifest.id,
                manifest.version,
                source.display()
            ),
        );
        let _ = self.inner.events.send(EngineEvent::PluginsChanged);
        self.plugins()
            .await?
            .into_iter()
            .find(|p| p.id == manifest.id)
            .ok_or_else(|| AppError::Invalid("the plugin was copied but cannot be read".into()))
    }

    /// Actions plugins added to request menus (running plugins only).
    pub fn plugin_actions(&self) -> Vec<PluginAction> {
        let names: HashMap<String, String> = discover(&self.plugins_dir())
            .into_iter()
            .filter_map(|f| f.manifest.ok().map(|m| (m.id, m.name)))
            .collect();
        let plugins = lock(&self.inner.plugins);
        let mut out = Vec::new();
        for (id, rt) in plugins.iter() {
            if rt.calls.is_none() {
                continue;
            }
            for (action, label) in &rt.actions {
                out.push(PluginAction {
                    plugin: id.clone(),
                    plugin_name: names.get(id).cloned().unwrap_or_else(|| id.clone()),
                    action: action.clone(),
                    label: label.clone(),
                });
            }
        }
        out.sort_by(|a, b| (&a.plugin_name, &a.label).cmp(&(&b.plugin_name, &b.label)));
        out
    }

    /// Runs a plugin's action on a stored exchange and waits for its output.
    pub async fn plugin_run_action(
        &self,
        plugin: &str,
        action: &str,
        exchange_id: i64,
    ) -> Result<String, AppError> {
        static CALLS: AtomicU64 = AtomicU64::new(1);
        let db = self.db()?;
        let found =
            tokio::task::spawn_blocking(move || db.exchanges_bulk(&[exchange_id])).await??;
        let ex = found
            .first()
            .ok_or_else(|| AppError::Invalid(format!("no exchange {exchange_id}")))?;
        let call = CALLS.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let sender = {
            let mut plugins = lock(&self.inner.plugins);
            let rt = plugins
                .get_mut(plugin)
                .ok_or_else(|| AppError::Invalid(format!("plugin {plugin} is not running")))?;
            if !rt.actions.iter().any(|(a, _)| a == action) {
                return Err(AppError::Invalid(format!("no action {action:?}")));
            }
            rt.pending.insert(call, tx);
            rt.calls
                .clone()
                .ok_or_else(|| AppError::Invalid(format!("plugin {plugin} is not running")))?
        };
        sender
            .send(HostMessage::Action {
                call,
                action: action.to_string(),
                exchange: Box::new(to_plugin(ex)),
            })
            .await
            .map_err(|_| AppError::Invalid(format!("plugin {plugin} stopped")))?;
        self.audit(
            "plugin.action",
            &format!("{plugin}: {action} on exchange {exchange_id}"),
        );
        match tokio::time::timeout(ACTION_TIMEOUT, rx).await {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(_)) => Err(AppError::Invalid(format!("plugin {plugin} stopped"))),
            Err(_) => {
                if let Some(rt) = lock(&self.inner.plugins).get_mut(plugin) {
                    rt.pending.remove(&call);
                }
                Err(AppError::Invalid(format!(
                    "the plugin did not answer within {}s",
                    ACTION_TIMEOUT.as_secs()
                )))
            }
        }
    }

    async fn plugin_finding(&self, id: &str, f: PluginFinding) -> Result<(), String> {
        let severity = match f.severity.as_str() {
            "info" => FindingSeverity::Info,
            "low" => FindingSeverity::Low,
            "medium" => FindingSeverity::Medium,
            "high" => FindingSeverity::High,
            other => return Err(format!("unknown severity {other:?}")),
        };
        let confidence = match f.confidence.as_str() {
            "tentative" => Confidence::Tentative,
            "firm" => Confidence::Firm,
            "certain" => Confidence::Certain,
            other => return Err(format!("unknown confidence {other:?}")),
        };
        if f.key.is_empty() || f.key.len() > 300 {
            return Err("the key must be 1–300 characters".into());
        }
        let issue = PassiveIssue {
            source: "plugin",
            exchange_id: f.exchange_id,
            dedup_key: format!("plugin:{id}:{}", f.key),
            kind: f.kind,
            title: f.title.chars().take(300).collect(),
            severity,
            confidence,
            description: format!("{}\n\nReported by the plugin {id}.", f.description.trim())
                .trim_start()
                .to_string(),
            remediation: f.remediation,
            evidence_note: f.evidence.chars().take(2000).collect(),
        };
        let db = self.db().map_err(|e| e.to_string())?;
        let recorded = tokio::task::spawn_blocking(move || db.findings_record_auto(vec![issue]))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        if recorded.created + recorded.updated > 0 {
            let _ = self.inner.events.send(EngineEvent::FindingsChanged {
                created: recorded.created,
            });
        }
        Ok(())
    }

    async fn plugin_annotate(
        &self,
        exchange_id: i64,
        highlight: Option<String>,
        note: Option<String>,
    ) -> Result<(), String> {
        let color = match highlight.as_deref() {
            None => None,
            Some(name) => {
                Some(Highlight::parse(name).ok_or_else(|| format!("unknown highlight {name:?}"))?)
            }
        };
        let db: ProjectDb = self.db().map_err(|e| e.to_string())?;
        tokio::task::spawn_blocking(move || {
            let row = db
                .exchange(exchange_id)?
                .ok_or(chosproxy_storage::StorageError::NotFound(exchange_id))?
                .row;
            db.annotate(exchange_id, note.or(row.notes), color.or(row.highlight))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Aborts a task when dropped.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Copies a plugin folder (no links, bounded depth).
fn copy_dir(from: &Path, to: &Path, depth: u32) -> Result<(), String> {
    if depth > 8 {
        return Err("the plugin folder is nested too deeply".into());
    }
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if kind.is_symlink() {
            return Err("plugins cannot contain links".into());
        } else if kind.is_dir() {
            copy_dir(&entry.path(), &target, depth + 1)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// What a plugin call needs to answer.
struct CallCtx<'a> {
    reply: &'a tokio::sync::mpsc::Sender<HostMessage>,
    budget: &'a Arc<SendBudget>,
}

/// Rate and concurrency limit of one plugin's `send` calls.
#[derive(Default)]
struct SendBudget {
    state: Mutex<(VecDeque<Instant>, usize)>,
}

impl SendBudget {
    fn take(&self) -> Result<(), String> {
        let mut s = lock(&self.state);
        let now = Instant::now();
        while s
            .0
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
        {
            s.0.pop_front();
        }
        if s.0.len() >= SENDS_PER_MINUTE {
            return Err(format!("more than {SENDS_PER_MINUTE} requests per minute"));
        }
        if s.1 >= SENDS_IN_FLIGHT {
            return Err(format!("more than {SENDS_IN_FLIGHT} requests at once"));
        }
        s.0.push_back(now);
        s.1 += 1;
        Ok(())
    }

    fn release(&self) {
        let mut s = lock(&self.state);
        s.1 = s.1.saturating_sub(1);
    }
}

/// Forgets a plugin's actions and call channel when its run ends.
struct ClearCalls {
    engine: Engine,
    id: String,
}

impl Drop for ClearCalls {
    fn drop(&mut self) {
        if let Some(rt) = lock(&self.engine.inner.plugins).get_mut(&self.id) {
            rt.calls = None;
            rt.actions.clear();
            rt.pending.clear();
            rt.hook = None;
            rt.hook_responses = false;
            rt.hook_pending.clear();
        }
        let _ = self.engine.inner.events.send(EngineEvent::PluginsChanged);
    }
}

fn failed(id: u64, error: String) -> HostMessage {
    HostMessage::Result {
        id,
        ok: false,
        data: None,
        error: Some(error),
    }
}

fn answer(id: u64, result: Result<serde_json::Value, String>) -> HostMessage {
    match result {
        Ok(data) => HostMessage::Result {
            id,
            ok: true,
            data: Some(data),
            error: None,
        },
        Err(e) => failed(id, e),
    }
}

async fn write(stdin: &mut ChildStdin, msg: &HostMessage) -> Result<(), String> {
    let line = msg.to_line();
    let written = async {
        stdin.write_all(line.as_bytes()).await?;
        stdin.flush().await
    };
    match tokio::time::timeout(WRITE_TIMEOUT, written).await {
        Err(_) => Err("the plugin stopped reading its input".into()),
        Ok(Err(e)) => Err(format!("cannot write to the plugin: {e}")),
        Ok(Ok(())) => Ok(()),
    }
}

/// One line from the plugin (without the newline); `None` at end of output.
async fn read_line(
    reader: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<Option<String>, String> {
    let mut buf = Vec::new();
    let n = (&mut *reader)
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut buf)
        .await
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(None);
    }
    if buf.len() > MAX_LINE {
        return Err("a message is longer than 1 MiB".into());
    }
    Ok(Some(String::from_utf8_lossy(&buf).trim_end().to_string()))
}

async fn exit_reason(child: &mut Child) -> String {
    match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
        Ok(Ok(status)) => format!("the plugin exited ({status})"),
        _ => "the plugin closed its output".into(),
    }
}
