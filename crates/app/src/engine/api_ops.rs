//! The API workspace (M27, matrix SCANNER-014): API definitions imported
//! into the project, their endpoints as requests for Repeater and the
//! Fuzzer, and a task that sends the endpoints' example requests.
//!
//! Sending is the tester's choice and is bounded: only hosts the tester
//! ticks (and, with a scope, only in-scope URLs), safe methods unless
//! others are asked for explicitly, rate-limited, through the resource
//! pool and session rules of the crawler. The requests are the
//! definition's examples — no attack payloads.

use super::Engine;
use super::task_ops::Control;
use crate::AppError;
use chosproxy_apispec::{ApiDefinition, ApiEndpoint, request_text};
use chosproxy_core::Source;
use chosproxy_core::time::now_ms;
use chosproxy_request::{Destination, Limiter, SendLimits};
use chosproxy_storage::{
    ApiDefinitionInfo, NewApiDefinition, ProjectDb, RepeaterTarget, StorageError, TaskId,
    TaskProgress, TaskRow, TaskStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::time::Duration;
use tokio::sync::watch;
use ts_rs::TS;

/// Sends logged one by one; later ones are only counted.
const LOGGED_SENDS: u64 = 200;

/// A definition with its endpoints.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiDefinitionDetail {
    /// The stored definition.
    pub info: ApiDefinitionInfo,
    /// Its endpoints.
    pub endpoints: Vec<ApiEndpoint>,
    /// The hosts its endpoints go to (`host:port`).
    pub hosts: Vec<String>,
}

/// An endpoint's example request, ready for Repeater or the Fuzzer.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiRequest {
    /// Where it goes.
    pub target: RepeaterTarget,
    /// The request text.
    pub request: String,
}

/// What an API send task does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiSendConfig {
    /// The definition.
    #[ts(type = "number")]
    pub definition: i64,
    /// Endpoint indexes to send (empty: all).
    pub endpoints: Vec<u32>,
    /// Hosts (`host:port`) the tester allows; endpoints on other hosts
    /// are skipped.
    pub hosts: Vec<String>,
    /// Also send methods other than GET, HEAD and OPTIONS (they may
    /// change data on the server).
    pub include_unsafe: bool,
    /// Requests per second (0 = no limit).
    pub requests_per_second: u32,
}

fn host_key(url: &url::Url) -> Option<String> {
    Some(format!(
        "{}:{}",
        url.host_str()?,
        url.port_or_known_default()?
    ))
}

fn target_of(endpoint: &ApiEndpoint) -> Option<(RepeaterTarget, url::Url)> {
    let url = url::Url::parse(&endpoint.url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    Some((
        RepeaterTarget {
            scheme: url.scheme().to_string(),
            host: url.host_str()?.to_string(),
            port: url.port_or_known_default()?,
        },
        url,
    ))
}

fn format_tag(def: &ApiDefinition) -> String {
    serde_json::to_value(def.format)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn storage_error(e: StorageError) -> AppError {
    match e {
        StorageError::InvalidQuery(m) => AppError::Invalid(m),
        other => other.into(),
    }
}

impl Engine {
    /// Reads a definition file and keeps it in the project.
    pub async fn api_import(
        &self,
        path: &std::path::Path,
        base_url: Option<String>,
    ) -> Result<ApiDefinitionInfo, AppError> {
        let meta = std::fs::metadata(path)
            .map_err(|e| AppError::Invalid(format!("{}: {e}", path.display())))?;
        if meta.len() > chosproxy_apispec::MAX_SPEC_BYTES as u64 {
            return Err(AppError::Invalid("the file is larger than 10 MB".into()));
        }
        let bytes = std::fs::read(path)
            .map_err(|e| AppError::Invalid(format!("{}: {e}", path.display())))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| AppError::Invalid("the file is not UTF-8 text".into()))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "definition".into());
        self.api_import_text(&name, &text, base_url).await
    }

    /// Keeps a definition given as text (the file name is a label).
    pub async fn api_import_text(
        &self,
        name: &str,
        text: &str,
        base_url: Option<String>,
    ) -> Result<ApiDefinitionInfo, AppError> {
        let base_url = base_url
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty());
        let def = tokio::task::spawn_blocking({
            let text = text.to_string();
            let base = base_url.clone();
            move || chosproxy_apispec::parse(&text, base.as_deref())
        })
        .await?
        .map_err(AppError::Invalid)?;
        if def.endpoints.is_empty() {
            let why = def
                .warnings
                .first()
                .cloned()
                .unwrap_or_else(|| "no operations".into());
            return Err(AppError::Invalid(format!("no endpoints found: {why}")));
        }
        let endpoints_json = serde_json::to_string(&def.endpoints)
            .map_err(|e| AppError::Invalid(format!("cannot store the endpoints: {e}")))?;
        let new = NewApiDefinition {
            name: name.to_string(),
            format: format_tag(&def),
            title: def.title.clone(),
            version: def.version.clone(),
            base_url: base_url.unwrap_or_default(),
            endpoint_count: i64::try_from(def.endpoints.len()).unwrap_or(i64::MAX),
            warnings: def.warnings.clone(),
            endpoints_json,
            schema_json: String::new(),
        };
        let db = self.db()?;
        let info = tokio::task::spawn_blocking(move || db.api_definition_add(new))
            .await?
            .map_err(storage_error)?;
        self.audit(
            "api.imported",
            &format!("{} ({} endpoints)", info.name, info.endpoint_count),
        );
        Ok(info)
    }

    /// The definitions of the project.
    pub async fn api_definitions(&self) -> Result<Vec<ApiDefinitionInfo>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.api_definitions()).await??)
    }

    /// A definition with its endpoints and hosts.
    pub async fn api_definition(&self, id: i64) -> Result<ApiDefinitionDetail, AppError> {
        let db = self.db()?;
        let (info, json) = tokio::task::spawn_blocking(move || db.api_definition(id))
            .await??
            .ok_or_else(|| AppError::Invalid(format!("no API definition {id}")))?;
        let endpoints: Vec<ApiEndpoint> = serde_json::from_str(&json)
            .map_err(|e| AppError::Invalid(format!("unreadable endpoints: {e}")))?;
        let hosts: BTreeSet<String> = endpoints
            .iter()
            .filter_map(|e| url::Url::parse(&e.url).ok())
            .filter_map(|u| host_key(&u))
            .collect();
        Ok(ApiDefinitionDetail {
            info,
            endpoints,
            hosts: hosts.into_iter().collect(),
        })
    }

    /// Removes a definition (traffic already sent stays in the history).
    pub async fn api_definition_delete(&self, id: i64) -> Result<(), AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.api_definition_delete(id))
            .await?
            .map_err(storage_error)?;
        self.audit("api.deleted", &format!("definition {id}"));
        Ok(())
    }

    /// The example request of one endpoint, for Repeater or the Fuzzer.
    pub async fn api_request(&self, id: i64, index: u32) -> Result<ApiRequest, AppError> {
        let detail = self.api_definition(id).await?;
        let endpoint = detail
            .endpoints
            .get(index as usize)
            .ok_or_else(|| AppError::Invalid(format!("no endpoint {index}")))?;
        let (target, _) = target_of(endpoint)
            .ok_or_else(|| AppError::Invalid(format!("{} is not an http(s) URL", endpoint.url)))?;
        Ok(ApiRequest {
            target,
            request: request_text(endpoint).replace("\r\n", "\n"),
        })
    }

    /// Starts a task that sends endpoints' example requests.
    pub async fn api_send_start(&self, config: ApiSendConfig) -> Result<TaskRow, AppError> {
        if config.hosts.is_empty() {
            return Err(AppError::Invalid(
                "tick at least one host you are allowed to test".into(),
            ));
        }
        if config.requests_per_second > 100 {
            return Err(AppError::Invalid("at most 100 requests per second".into()));
        }
        let detail = self.api_definition(config.definition).await?;
        let total = detail.endpoints.len();
        if config.endpoints.iter().any(|i| *i as usize >= total) {
            return Err(AppError::Invalid(
                "an endpoint index is out of range".into(),
            ));
        }
        let name = format!(
            "Send API {}",
            if detail.info.title.is_empty() {
                &detail.info.name
            } else {
                &detail.info.title
            }
        );
        let json = serde_json::to_string(&config)
            .map_err(|e| AppError::Invalid(format!("invalid settings: {e}")))?;
        self.launch_task(
            "apiSend",
            "API send",
            name,
            json,
            move |engine, db, id, rx| async move {
                engine.run_api_send(&db, id, config, detail, rx).await
            },
        )
        .await
    }

    async fn run_api_send(
        &self,
        db: &ProjectDb,
        id: TaskId,
        config: ApiSendConfig,
        detail: ApiDefinitionDetail,
        mut control: watch::Receiver<Control>,
    ) -> Result<TaskStatus, AppError> {
        let chosen: Vec<(usize, &ApiEndpoint)> = if config.endpoints.is_empty() {
            detail.endpoints.iter().enumerate().collect()
        } else {
            config
                .endpoints
                .iter()
                .filter_map(|i| detail.endpoints.get(*i as usize).map(|e| (*i as usize, e)))
                .collect()
        };
        let total = chosen.len() as u64;
        let limiter = Limiter::new(1, config.requests_per_second);
        let label = if detail.info.title.is_empty() {
            detail.info.name.clone()
        } else {
            detail.info.title.clone()
        };
        let mut log = vec![(
            "info".to_string(),
            format!(
                "Sending {total} endpoint(s) of {label} to {}; {}. Only the definition's example \
                 requests are sent.",
                config.hosts.join(", "),
                if config.include_unsafe {
                    "all methods"
                } else {
                    "safe methods only (GET, HEAD, OPTIONS)"
                }
            ),
        )];
        let (mut sent, mut errors, mut skipped, mut logged) = (0u64, 0u64, 0u64, 0u64);
        let mut paused = false;
        let mut status = TaskStatus::Completed;
        let mut note = |log: &mut Vec<(String, String)>, level: &str, message: String| {
            logged += 1;
            if logged <= LOGGED_SENDS {
                log.push((level.to_string(), message));
            } else if logged == LOGGED_SENDS + 1 {
                log.push(("info".into(), "More lines are only counted.".into()));
            }
        };
        for (done, (index, endpoint)) in chosen.into_iter().enumerate() {
            loop {
                let wanted = *control.borrow_and_update();
                match wanted {
                    Control::Cancel | Control::Suspend => {
                        status = TaskStatus::Cancelled;
                        break;
                    }
                    Control::Pause | Control::Run => {}
                }
                if (wanted == Control::Pause) != paused {
                    paused = wanted == Control::Pause;
                    self.set_status(
                        db,
                        id,
                        if paused {
                            TaskStatus::Paused
                        } else {
                            TaskStatus::Running
                        },
                    )
                    .await?;
                    log.push((
                        "info".into(),
                        if paused { "Paused." } else { "Resumed." }.into(),
                    ));
                }
                if !paused {
                    break;
                }
                if control.changed().await.is_err() {
                    status = TaskStatus::Cancelled;
                    break;
                }
            }
            if status == TaskStatus::Cancelled {
                log.push(("info".into(), "Stopped.".into()));
                break;
            }
            let what = format!("#{index} {} {}", endpoint.method, endpoint.url);
            if !config.include_unsafe && !endpoint.is_read_only() {
                skipped += 1;
                note(
                    &mut log,
                    "info",
                    format!("Skipped {what}: not a safe method."),
                );
                continue;
            }
            let Some((target, url)) = target_of(endpoint) else {
                skipped += 1;
                note(
                    &mut log,
                    "warning",
                    format!("Skipped {what}: not an http(s) URL."),
                );
                continue;
            };
            if !host_key(&url).is_some_and(|h| config.hosts.contains(&h)) {
                skipped += 1;
                note(
                    &mut log,
                    "info",
                    format!("Skipped {what}: its host was not ticked."),
                );
                continue;
            }
            let mut path = url.path().to_string();
            if let Some(q) = url.query() {
                path.push('?');
                path.push_str(q);
            }
            if let Err(e) = self.ensure_in_scope(&target, &path, "API send") {
                skipped += 1;
                note(&mut log, "info", format!("Skipped {what}: {e}"));
                continue;
            }
            let text = request_text(endpoint);
            let request = match crate::repeater::request_bytes(&text, true, true, true) {
                Ok(r) => r,
                Err(e) => {
                    skipped += 1;
                    note(&mut log, "warning", format!("Skipped {what}: {e}"));
                    continue;
                }
            };
            let https = target.scheme == "https";
            let security = match self.security_for_host(&target.host, https).await {
                Ok(s) => s,
                Err(e) => {
                    errors += 1;
                    note(&mut log, "warning", format!("{what}: {e}"));
                    continue;
                }
            };
            let _permit = limiter.acquire().await;
            let destination = Destination {
                host: target.host.clone(),
                port: target.port,
                tls_name: target.host.clone(),
            };
            let limits = SendLimits {
                response_timeout: Duration::from_secs(30),
                body_idle_timeout: Duration::from_secs(30),
                max_response_bytes: super::limits_of(&self.settings())
                    .max_message_bytes
                    .max(64 * 1024),
            };
            let started_at = now_ms();
            let outcome = self
                .session_send(
                    "crawler",
                    Source::Scanner,
                    &destination,
                    &security,
                    &request,
                    &limits,
                )
                .await;
            let mut exchange =
                crate::repeater::captured(Source::Scanner, &target, request, started_at, &outcome);
            let origin_note = format!("API definition {label}: {}.", endpoint.name);
            exchange.notes = Some(match exchange.notes.take() {
                Some(n) => format!("{origin_note} {n}"),
                None => origin_note,
            });
            match &outcome {
                Ok(s) => {
                    sent += 1;
                    note(&mut log, "info", format!("{what} → {}", s.head.status));
                }
                Err(e) => {
                    errors += 1;
                    note(&mut log, "warning", format!("{what}: {}", e.message));
                }
            }
            {
                let db = db.clone();
                tokio::task::spawn_blocking(move || db.insert(exchange)).await??;
            }
            let progress = TaskProgress {
                requests: sent + errors,
                errors,
                discovered: total,
                queued: total.saturating_sub(done as u64 + 1),
                progress: format!("{} of {total}", done + 1),
            };
            {
                let db = db.clone();
                tokio::task::spawn_blocking(move || db.task_progress(id, progress)).await??;
            }
            self.task_log_lines(db, id, std::mem::take(&mut log)).await;
        }
        log.push((
            "info".into(),
            format!("{sent} sent, {errors} failed, {skipped} skipped."),
        ));
        self.task_log_lines(db, id, log).await;
        Ok(status)
    }
}
