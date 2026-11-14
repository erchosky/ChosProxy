//! ChosOAST in the engine (spec addendum §68, ADR-0026).
//!
//! While a project is open, interactions arrive from the server inside
//! ChosProxy (Local mode) or are polled from the tester's own `chosoast`
//! server (Self-hosted mode). Only interactions whose identifier belongs to
//! a payload created in this project are stored. A linker scans new
//! traffic for those identifiers to know which request carried each one;
//! an identifier with both a carrying request and an interaction becomes a
//! finding: Finding → request → identifier → interaction → evidence.

use super::Engine;
use super::lock;
use crate::AppError;
use chosproxy_core::config::{ConfigSection, OastConfig, OastMode, SelfHostedOast};
use chosproxy_core::event::{EngineEvent, NoticeLevel};
use chosproxy_core::time::{compact_utc, now_ms};
use chosproxy_oast::api::{self, Remote};
use chosproxy_oast::dns::ZoneAddresses;
use chosproxy_oast::{Interaction, OastServer, ServerConfig, extract_ids, new_id};
use chosproxy_storage::{
    Confidence, FindingSeverity, HistoryRow, NewOastInteraction, OastInteractionRow,
    OastPayloadRow, PassiveIssue, ProjectDb,
};
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::task::JoinHandle;
use ts_rs::TS;

/// Requests scanned for identifiers per round.
const LINK_BATCH: u32 = 200;
/// Pause between linker rounds when nothing is waiting.
const LINK_IDLE: Duration = Duration::from_secs(2);
/// Interactions kept in the local server's memory.
const LOCAL_KEEP: usize = 10_000;

/// What runs for the open project; dropping it stops everything.
pub(crate) struct Runtime {
    _server: Option<OastServer>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

/// OAST state shown in the UI.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OastStatus {
    /// Configured mode.
    pub mode: OastMode,
    /// Receiving interactions now.
    pub running: bool,
    /// Base of HTTP payload URLs (Local mode).
    pub http_base: Option<String>,
    /// Zone of DNS payloads, if DNS is available.
    pub dns_zone: Option<String>,
    /// Where the local DNS listener runs.
    pub dns_listener: Option<String>,
    /// Where the local SMTP listener runs (it records mail, never relays).
    pub smtp_listener: Option<String>,
    /// Last successful poll of a self-hosted server, Unix epoch ms.
    #[ts(type = "number | null")]
    pub last_poll_at: Option<i64>,
    /// Why OAST is not working, if it is not.
    pub error: Option<String>,
    /// Interactions received for this project's payloads this session.
    #[ts(type = "number")]
    pub received: u64,
    /// Interactions for identifiers not created in this project (dropped).
    #[ts(type = "number")]
    pub ignored: u64,
}

/// One interaction with its raw data and the requests that carried it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OastInteractionDetail {
    /// The interaction.
    pub row: OastInteractionRow,
    /// Raw HTTP request as text, or the DNS question (packets are binary).
    pub text: String,
    /// Raw bytes, base64.
    pub raw_base64: String,
    /// Stored requests that carried the identifier, newest first.
    pub requests: Vec<HistoryRow>,
}

/// The OAST server as a request destination.
#[derive(Debug, Clone)]
struct OwnServer {
    zone: String,
    http: Option<(String, u16)>,
}

impl OwnServer {
    fn is_target(&self, host: &str, port: u16) -> bool {
        let host = host.to_ascii_lowercase();
        let in_zone = !self.zone.is_empty()
            && (host == self.zone || host.ends_with(&format!(".{}", self.zone)));
        in_zone
            || self
                .http
                .as_ref()
                .is_some_and(|(h, p)| *h == host && *p == port)
    }
}

fn group(protocol: &str) -> &'static str {
    match protocol {
        "dns" => "dns",
        "smtp" => "smtp",
        _ => "http",
    }
}

impl Engine {
    /// The OAST configuration.
    pub fn oast_config(&self) -> OastConfig {
        self.inner.store.load::<OastConfig>().value
    }

    /// Saves the configuration and restarts OAST for the open project.
    pub async fn oast_save_config(&self, config: OastConfig) -> Result<OastStatus, AppError> {
        config.validate().map_err(AppError::Invalid)?;
        self.inner.store.save(&config)?;
        self.stop_oast();
        if self.db().is_ok() {
            self.start_oast().await;
        }
        let _ = self.inner.events.send(EngineEvent::OastChanged);
        Ok(self.oast_status())
    }

    /// Current state.
    pub fn oast_status(&self) -> OastStatus {
        let mut s = lock(&self.inner.oast_status).clone();
        s.mode = self.oast_config().mode;
        s
    }

    /// Checks a self-hosted server: certificate pin, token and zone.
    pub async fn oast_test_server(&self, server: SelfHostedOast) -> Result<String, AppError> {
        let remote = Remote {
            host: server.api_host.clone(),
            port: server.api_port,
            token: server.token.clone(),
            pin: server.fingerprint.clone(),
        };
        let page = api::fetch(&remote, u64::MAX)
            .await
            .map_err(|e| AppError::Invalid(e.to_string()))?;
        if !page
            .zone
            .eq_ignore_ascii_case(server.zone.trim_end_matches('.'))
        {
            return Err(AppError::Invalid(format!(
                "the server answers for {}, not {}",
                page.zone, server.zone
            )));
        }
        Ok(format!(
            "Connected: zone {}, {} interactions recorded on the server.",
            page.zone, page.latest
        ))
    }

    /// Creates a payload for the open project.
    pub async fn oast_new_payload(&self, note: String) -> Result<OastPayloadRow, AppError> {
        self.ensure_writable()?;
        let db = self.db()?;
        let config = self.oast_config();
        let status = self.oast_status();
        if config.mode == OastMode::Off {
            return Err(AppError::Invalid(
                "choose an OAST mode first (OAST → Settings)".into(),
            ));
        }
        let id = new_id().map_err(|_| AppError::Internal("no secure random source".into()))?;
        let (dns_name, url) = match config.mode {
            OastMode::SelfHosted => {
                let zone = config
                    .self_hosted
                    .zone
                    .trim_end_matches('.')
                    .to_ascii_lowercase();
                (Some(format!("{id}.{zone}")), format!("http://{id}.{zone}/"))
            }
            _ => {
                let base = status.http_base.clone().unwrap_or_else(|| {
                    format!(
                        "http://{}:{}",
                        config.local.advertised_host, config.local.http_port
                    )
                });
                let dns = config
                    .local
                    .dns_enabled
                    .then(|| format!("{id}.{}", config.local.zone.trim_end_matches('.')));
                (dns, format!("{base}/{id}"))
            }
        };
        let note: String = note.trim().chars().take(500).collect();
        let row =
            tokio::task::spawn_blocking(move || db.oast_create_payload(id, note, dns_name, url))
                .await??;
        let _ = self.inner.events.send(EngineEvent::OastChanged);
        Ok(row)
    }

    /// Payloads of the open project, newest first.
    pub async fn oast_payloads(&self) -> Result<Vec<OastPayloadRow>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.oast_payloads()).await??)
    }

    /// Interactions, newest first (optionally of one identifier).
    pub async fn oast_interactions(
        &self,
        correlation_id: Option<String>,
    ) -> Result<Vec<OastInteractionRow>, AppError> {
        let db = self.db()?;
        Ok(
            tokio::task::spawn_blocking(move || db.oast_interactions(correlation_id, 1000))
                .await??,
        )
    }

    /// One interaction in full.
    pub async fn oast_interaction(&self, id: i64) -> Result<OastInteractionDetail, AppError> {
        use base64::Engine as _;
        let db = self.db()?;
        tokio::task::spawn_blocking(move || {
            let found = db
                .oast_interaction(id)?
                .ok_or_else(|| AppError::Invalid(format!("no OAST interaction {id}")))?;
            let mut requests = Vec::new();
            for ex in found.exchanges.iter().take(20) {
                if let Some(stored) = db.exchange(*ex)? {
                    requests.push(stored.row);
                }
            }
            let text = if found.row.protocol == "dns" {
                found.row.summary.clone()
            } else {
                String::from_utf8_lossy(&found.raw).into_owned()
            };
            Ok(OastInteractionDetail {
                text,
                raw_base64: base64::engine::general_purpose::STANDARD.encode(&found.raw),
                row: found.row,
                requests,
            })
        })
        .await?
    }

    /// Starts OAST for the open project according to the configuration.
    pub(crate) async fn start_oast(&self) {
        self.stop_oast();
        let config = self.oast_config();
        let mut status = OastStatus {
            mode: config.mode,
            ..OastStatus::default()
        };
        let mut runtime = Runtime {
            _server: None,
            tasks: Vec::new(),
        };
        match config.mode {
            OastMode::Off => {}
            OastMode::Local => match self.start_local(&config, &mut status).await {
                Ok(server) => {
                    runtime.tasks.push(self.spawn_local_consumer(&server));
                    runtime._server = Some(server);
                    status.running = true;
                }
                Err(e) => {
                    status.error = Some(e.clone());
                    self.notice(NoticeLevel::Error, format!("OAST: {e}"));
                }
            },
            OastMode::SelfHosted => {
                status.dns_zone = Some(config.self_hosted.zone.clone());
                status.running = true;
                runtime.tasks.push(self.spawn_poller(config.clone()));
            }
        }
        if config.mode != OastMode::Off {
            runtime.tasks.push(self.spawn_linker(config));
        }
        *lock(&self.inner.oast_status) = status;
        *lock(&self.inner.oast) = Some(runtime);
        let _ = self.inner.events.send(EngineEvent::OastChanged);
    }

    /// Stops OAST (the project is closing or the configuration changed).
    pub(crate) fn stop_oast(&self) {
        let runtime = lock(&self.inner.oast).take();
        drop(runtime);
        let mut status = lock(&self.inner.oast_status);
        status.running = false;
    }

    async fn start_local(
        &self,
        config: &OastConfig,
        status: &mut OastStatus,
    ) -> Result<OastServer, String> {
        let l = &config.local;
        let ip: IpAddr = l
            .bind
            .parse()
            .map_err(|_| "invalid bind address".to_string())?;
        let advertised_ip = l.advertised_host.parse::<IpAddr>().ok();
        let server = OastServer::start(ServerConfig {
            zone: l.zone.clone(),
            addresses: ZoneAddresses {
                ipv4: match advertised_ip.or(Some(ip)) {
                    Some(IpAddr::V4(v4)) => Some(v4),
                    _ => None,
                },
                ipv6: match advertised_ip {
                    Some(IpAddr::V6(v6)) => Some(v6),
                    _ => None,
                },
            },
            http: Some(SocketAddr::new(ip, l.http_port)),
            https: None,
            dns: l.dns_enabled.then(|| SocketAddr::new(ip, l.dns_port)),
            smtp: l.smtp_enabled.then(|| SocketAddr::new(ip, l.smtp_port)),
            keep: LOCAL_KEEP,
        })
        .await
        .map_err(|e| format!("cannot start the local OAST server on {}: {e}", l.bind))?;
        let port = server.http_addr().map_or(l.http_port, |a| a.port());
        status.http_base = Some(format!("http://{}:{port}", l.advertised_host));
        if let Some(dns) = server.dns_addr() {
            status.dns_zone = Some(l.zone.clone());
            status.dns_listener = Some(dns.to_string());
        }
        if let Some(smtp) = server.smtp_addr() {
            status.smtp_listener = Some(smtp.to_string());
        }
        if !ip.is_loopback() {
            self.notice(
                NoticeLevel::Warning,
                format!(
                    "OAST server listening on {} (reachable from the network)",
                    l.bind
                ),
            );
        }
        Ok(server)
    }

    fn spawn_local_consumer(&self, server: &OastServer) -> JoinHandle<()> {
        let mut rx = server.log().subscribe();
        let weak = std::sync::Arc::downgrade(&self.inner);
        let started = now_ms();
        tokio::spawn(async move {
            loop {
                let first = match rx.recv().await {
                    Ok(i) => i,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return,
                };
                let mut batch = vec![first];
                while let Ok(i) = rx.try_recv() {
                    batch.push(i);
                }
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                Engine::from_inner(inner)
                    .oast_store(batch, &format!("local:{started}"))
                    .await;
            }
        })
    }

    fn spawn_poller(&self, config: OastConfig) -> JoinHandle<()> {
        let weak = std::sync::Arc::downgrade(&self.inner);
        let s = config.self_hosted;
        let remote = Remote {
            host: s.api_host.clone(),
            port: s.api_port,
            token: s.token.clone(),
            pin: s.fingerprint.clone(),
        };
        let server_key = format!("{}:{}", s.api_host, s.api_port);
        let every = Duration::from_secs(u64::from(config.poll_seconds.max(1)));
        tokio::spawn(async move {
            loop {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let engine = Engine::from_inner(inner);
                engine.oast_poll_once(&remote, &server_key).await;
                drop(engine);
                tokio::time::sleep(every).await;
            }
        })
    }

    async fn oast_poll_once(&self, remote: &Remote, server_key: &str) {
        let Ok(db) = self.db() else {
            return;
        };
        let after = {
            let db = db.clone();
            let key = server_key.to_string();
            tokio::task::spawn_blocking(move || db.oast_remote_seq(&key))
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or(0)
        };
        let fetched = match api::fetch(remote, after).await {
            // The server restarted: its log starts again from 1.
            Ok(page) if page.latest < after => api::fetch(remote, 0).await,
            other => other,
        };
        match fetched {
            Ok(page) => {
                let newest = page
                    .interactions
                    .last()
                    .map_or(page.latest.min(after), |i| i.seq);
                self.oast_store(page.interactions, &format!("remote:{server_key}"))
                    .await;
                let key = server_key.to_string();
                let _ =
                    tokio::task::spawn_blocking(move || db.oast_set_remote_seq(key, newest)).await;
                let mut status = lock(&self.inner.oast_status);
                let was_failing = status.error.is_some();
                status.last_poll_at = Some(now_ms());
                status.error = None;
                drop(status);
                if was_failing {
                    let _ = self.inner.events.send(EngineEvent::OastChanged);
                }
            }
            Err(e) => {
                let message = e.to_string();
                let changed = {
                    let mut status = lock(&self.inner.oast_status);
                    let changed = status.error.as_deref() != Some(message.as_str());
                    status.error = Some(message.clone());
                    changed
                };
                if changed {
                    self.notice(NoticeLevel::Warning, format!("OAST server: {message}"));
                    let _ = self.inner.events.send(EngineEvent::OastChanged);
                }
            }
        }
    }

    /// Stores interactions for this project's payloads and raises findings.
    async fn oast_store(&self, interactions: Vec<Interaction>, origin: &str) {
        let Ok(db) = self.db() else {
            return;
        };
        let mut rows = Vec::new();
        let mut unmatched = 0u64;
        for i in interactions {
            if i.ids.is_empty() {
                unmatched += 1;
            }
            for id in &i.ids {
                rows.push(NewOastInteraction {
                    correlation_id: id.clone(),
                    received_at: i.at,
                    protocol: i.protocol.as_str().to_string(),
                    remote: i.remote.clone(),
                    summary: i.summary.clone(),
                    raw: i.raw.clone(),
                    server_key: format!("{origin}:{}:{}", i.at, i.seq),
                });
            }
        }
        let offered = rows.len() as u64;
        let touched = {
            let db = db.clone();
            match tokio::task::spawn_blocking(move || db.oast_record_interactions(rows)).await {
                Ok(Ok(t)) => t,
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "could not store OAST interactions");
                    return;
                }
                Err(_) => return,
            }
        };
        {
            let mut status = lock(&self.inner.oast_status);
            status.ignored += unmatched;
            if touched.is_empty() {
                status.ignored += offered;
            } else {
                status.received += offered;
            }
        }
        if !touched.is_empty() {
            self.oast_findings(&db, touched).await;
            let _ = self.inner.events.send(EngineEvent::OastChanged);
        }
    }

    fn spawn_linker(&self, config: OastConfig) -> JoinHandle<()> {
        let weak = std::sync::Arc::downgrade(&self.inner);
        // Requests sent to the OAST server itself (e.g. the tester opening
        // a payload URL) carry the identifier but prove nothing. The local
        // server is recognised by host and port: lab targets often share
        // its address.
        let own = match config.mode {
            OastMode::SelfHosted => OwnServer {
                zone: config.self_hosted.zone.to_ascii_lowercase(),
                http: None,
            },
            _ => OwnServer {
                zone: config.local.zone.to_ascii_lowercase(),
                http: Some((
                    config.local.advertised_host.to_ascii_lowercase(),
                    config.local.http_port,
                )),
            },
        };
        tokio::spawn(async move {
            loop {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let engine = Engine::from_inner(inner);
                let more = engine.oast_link_round(&own).await.unwrap_or(false);
                drop(engine);
                if !more {
                    tokio::time::sleep(LINK_IDLE).await;
                }
            }
        })
    }

    /// Scans the next stored requests for identifiers. True if more wait.
    async fn oast_link_round(&self, own: &OwnServer) -> Result<bool, AppError> {
        let db = self.db()?;
        let own = own.clone();
        let (touched, more) = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || {
                let cursor = db.oast_link_cursor()?;
                let ids = db.exchange_ids_after(cursor, LINK_BATCH)?;
                let Some(&last) = ids.last() else {
                    return Ok::<_, chosproxy_storage::StorageError>((Vec::new(), false));
                };
                let known = db.oast_known_ids()?;
                let mut links = Vec::new();
                if !known.is_empty() {
                    for ex in db.exchanges_bulk(&ids)? {
                        if own.is_target(&ex.row.host, ex.row.port) {
                            continue;
                        }
                        for id in extract_ids(&ex.request) {
                            if known.contains(&id) {
                                links.push((id, ex.row.id));
                            }
                        }
                    }
                }
                let touched = db.oast_record_links(links, last)?;
                Ok((touched, ids.len() == LINK_BATCH as usize))
            })
            .await??
        };
        if !touched.is_empty() {
            self.oast_findings(&db, touched).await;
            let _ = self.inner.events.send(EngineEvent::OastChanged);
        }
        Ok(more)
    }

    /// One finding per identifier and protocol family once an identifier
    /// has both a carrying request and an interaction.
    async fn oast_findings(&self, db: &ProjectDb, ids: Vec<String>) {
        let db = db.clone();
        let recorded = tokio::task::spawn_blocking(move || {
            let mut issues = Vec::new();
            for id in ids {
                let links = db.oast_links_for(&id)?;
                let Some(&exchange_id) = links.first() else {
                    continue;
                };
                let interactions = db.oast_interactions(Some(id.clone()), 200)?;
                for family in ["dns", "http", "smtp"] {
                    let of: Vec<&OastInteractionRow> = interactions
                        .iter()
                        .filter(|i| group(&i.protocol) == family)
                        .collect();
                    let Some(first) = of.last() else {
                        continue;
                    };
                    let (kind, what) = match family {
                        "dns" => ("External service interaction (DNS)", "looked up"),
                        "smtp" => ("External service interaction (SMTP)", "sent mail"),
                        _ => ("External service interaction (HTTP)", "fetched"),
                    };
                    issues.push(PassiveIssue {
                        source: "oast",
                        exchange_id,
                        dedup_key: format!("oast:{id}:{family}"),
                        kind: kind.into(),
                        title: format!("{kind} triggered by OAST payload {id}"),
                        severity: FindingSeverity::Medium,
                        confidence: Confidence::Firm,
                        description: format!(
                            "A unique OAST identifier sent in this request was later {what} by \
                             {} ({} interaction(s), first at {} UTC: {}). Something processed the \
                             value on the server side and contacted an external system, which can \
                             indicate server-side request forgery or a blind injection. Confirm \
                             which component made the contact and whether it can be steered to \
                             internal systems.",
                            first.remote,
                            of.len(),
                            compact_utc(first.received_at),
                            first.summary
                        ),
                        remediation: "Do not let user input decide which hosts the application \
                            contacts: validate and allow-list destinations, disable unused URL \
                            fetchers and external entity resolution, and restrict outbound \
                            network access from servers."
                            .into(),
                        evidence_note: format!(
                            "Carried OAST identifier {id}; {} {family} interaction(s) followed.",
                            of.len()
                        ),
                    });
                }
            }
            db.findings_record_auto(issues)
        })
        .await;
        if let Ok(Ok(r)) = recorded
            && r.created + r.updated > 0
        {
            let _ = self
                .inner
                .events
                .send(EngineEvent::FindingsChanged { created: r.created });
        }
    }
}
