//! The sequencer (spec §19, M12): a capture task repeats one request and
//! collects the token each response carries; the analysis runs on the
//! stored tokens (or on tokens pasted by the tester).

use super::task_ops::{Control, LOGGED_ERRORS, PROGRESS_EVERY};
use super::{Engine, limits_of};
use crate::AppError;
use crate::repeater;
use chosproxy_core::Source;
use chosproxy_core::time::now_ms;
use chosproxy_request::{Destination, Limiter, SendError, SendLimits, Sent, UpstreamSecurity};
use chosproxy_sequencer::{CaptureConfig, TokenAnalysis, TokenCandidate, analyze, suggest};
use chosproxy_storage::{
    ProjectDb, RepeaterTarget, StorageError, TaskId, TaskProgress, TaskRow, TaskStatus,
    validate_target,
};
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use ts_rs::TS;

/// A capture prepared from a stored request.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SequencerPrepared {
    /// Settings with the request, its target and the first candidate.
    pub config: CaptureConfig,
    /// Tokens found in the stored response.
    pub candidates: Vec<TokenCandidate>,
}

/// What a capture's requests share.
struct Capture {
    config: CaptureConfig,
    target: RepeaterTarget,
    request: Vec<u8>,
    destination: Destination,
    security: UpstreamSecurity,
    limits: SendLimits,
    limiter: Limiter,
}

impl Engine {
    /// Default capture settings.
    pub fn sequencer_defaults(&self) -> CaptureConfig {
        CaptureConfig::default()
    }

    /// A capture of a stored request, with the tokens its response carries.
    pub async fn sequencer_prepare(&self, exchange_id: i64) -> Result<SequencerPrepared, AppError> {
        let db = self.db()?;
        let stored = tokio::task::spawn_blocking(move || db.exchange(exchange_id))
            .await??
            .ok_or(AppError::UnknownExchange(exchange_id))?;
        let text = repeater::editor_text(&stored.request)?;
        let candidates = stored.response.as_deref().map(suggest).unwrap_or_default();
        let mut config = CaptureConfig {
            scheme: stored.row.scheme.clone(),
            host: stored.row.host.clone(),
            port: stored.row.port,
            request: text.text,
            head_crlf: text.head_crlf,
            body_crlf: text.body_crlf,
            ..CaptureConfig::default()
        };
        if let Some(first) = candidates.first() {
            config.location = first.location.clone();
        }
        Ok(SequencerPrepared { config, candidates })
    }

    /// Statistics on tokens the tester provides.
    pub async fn sequencer_analyze(&self, tokens: Vec<String>) -> Result<TokenAnalysis, AppError> {
        tokio::task::spawn_blocking(move || analyze(&tokens))
            .await?
            .map_err(AppError::Invalid)
    }

    /// Statistics on the tokens of a capture.
    pub async fn sequencer_analysis(&self, task: TaskId) -> Result<TokenAnalysis, AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || {
            let tokens = db.sequencer_tokens(task)?;
            Ok::<_, StorageError>(analyze(&tokens))
        })
        .await??
        .map_err(AppError::Invalid)
    }

    /// The tokens of a capture, in order.
    pub async fn sequencer_tokens(&self, task: TaskId) -> Result<Vec<String>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.sequencer_tokens(task)).await??)
    }

    /// Starts a token capture.
    pub async fn sequencer_start(&self, config: CaptureConfig) -> Result<TaskRow, AppError> {
        config.validate().map_err(AppError::Invalid)?;
        let target = validate_target(&RepeaterTarget {
            scheme: config.scheme.clone(),
            host: config.host.clone(),
            port: config.port,
        })?;
        let request =
            repeater::request_bytes(&config.request, config.head_crlf, config.body_crlf, false)?;
        let full_path = config
            .request
            .lines()
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .nth(1)
            .unwrap_or("/");
        self.ensure_in_scope(&target, full_path, "sequencer")?;
        let path: String = full_path.chars().take(60).collect();
        let name = format!("Tokens from {}{path}", target.host);
        let json = serde_json::to_string(&config)
            .map_err(|e| AppError::Invalid(format!("invalid capture settings: {e}")))?;
        let security = self
            .tls_security(&target.host, target.scheme == "https")
            .await?;
        let timeout = Duration::from_secs(u64::from(config.timeout_secs));
        let capture = Capture {
            destination: Destination {
                host: target.host.clone(),
                port: target.port,
                tls_name: target.host.clone(),
            },
            limits: SendLimits {
                response_timeout: timeout,
                body_idle_timeout: timeout,
                max_response_bytes: limits_of(&self.settings()).max_message_bytes.max(64 * 1024),
            },
            limiter: Limiter::new(config.concurrency as usize, config.requests_per_second),
            config,
            target,
            request,
            security,
        };
        self.launch_task(
            "sequencer",
            "Sequencer",
            name,
            json,
            move |engine, db, id, rx| async move {
                engine.run_capture(&db, id, Arc::new(capture), rx).await
            },
        )
        .await
    }

    async fn capture_one(&self, capture: Arc<Capture>) -> (i64, Result<Sent, SendError>) {
        let _permit = capture.limiter.acquire().await;
        let started_at = now_ms();
        let outcome = self
            .session_send(
                "sequencer",
                chosproxy_core::Source::Sequencer,
                &capture.destination,
                &capture.security,
                &capture.request,
                &capture.limits,
            )
            .await;
        (started_at, outcome)
    }

    /// The capture loop: requests until `count` tokens are collected, the
    /// user stops it, or too many responses carry no token.
    async fn run_capture(
        &self,
        db: &ProjectDb,
        id: TaskId,
        capture: Arc<Capture>,
        mut control: watch::Receiver<Control>,
    ) -> Result<TaskStatus, AppError> {
        let wanted = u64::from(capture.config.count);
        // Give up when this many requests produced no token.
        let max_failures = wanted / 2 + 10;
        let mut log = vec![(
            "info".to_string(),
            format!(
                "Collecting {wanted} tokens from {}://{}:{}: {} at a time, {}. The request is sent \
                 unchanged each time (no cookies are added).",
                capture.target.scheme,
                capture.target.host,
                capture.target.port,
                capture.config.concurrency,
                if capture.config.requests_per_second == 0 {
                    "no rate limit".to_string()
                } else {
                    format!("{} per second", capture.config.requests_per_second)
                }
            ),
        )];
        self.task_log_lines(db, id, std::mem::take(&mut log)).await;
        self.set_status(db, id, TaskStatus::Running).await?;

        let mut inflight: tokio::task::JoinSet<(i64, Result<Sent, SendError>)> =
            tokio::task::JoinSet::new();
        let mut sent: u64 = 0;
        let mut tokens: u64 = 0;
        let mut failures: u64 = 0;
        let mut pending: Vec<(u64, String)> = Vec::new();
        let mut last_progress = Instant::now();
        let mut paused = false;
        let status = loop {
            let want = *control.borrow_and_update();
            if want == Control::Cancel {
                inflight.abort_all();
                log.push(("info".into(), "Cancelled.".into()));
                break TaskStatus::Cancelled;
            }
            if (want == Control::Pause) != paused {
                paused = want == Control::Pause;
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
            let enough = tokens + inflight.len() as u64 >= wanted || failures >= max_failures;
            while !paused
                && !enough
                && inflight.len() < capture.config.concurrency as usize
                && tokens + (inflight.len() as u64) < wanted
            {
                let engine = self.clone();
                let capture = capture.clone();
                inflight.spawn(async move { engine.capture_one(capture).await });
                sent += 1;
            }
            if inflight.is_empty() {
                if paused {
                    if control.changed().await.is_err() {
                        break TaskStatus::Cancelled;
                    }
                    continue;
                }
                if tokens >= wanted {
                    break TaskStatus::Completed;
                }
                self.save_capture(
                    db,
                    id,
                    &mut pending,
                    &mut log,
                    sent,
                    failures,
                    tokens,
                    wanted,
                )
                .await?;
                return Err(AppError::Invalid(format!(
                    "{failures} responses carried no token; check the token location"
                )));
            }
            let joined = tokio::select! {
                joined = inflight.join_next() => joined,
                changed = control.changed() => {
                    if changed.is_err() {
                        inflight.abort_all();
                        break TaskStatus::Cancelled;
                    }
                    continue;
                }
            };
            let Some(Ok((started_at, outcome))) = joined else {
                continue;
            };
            let token = match &outcome {
                Ok(s) => capture.config.location.extract(&s.response),
                Err(_) => None,
            };
            match (&outcome, token) {
                (Ok(_), Some(t)) if tokens < wanted => {
                    pending.push((tokens, t));
                    tokens += 1;
                }
                (Ok(_), Some(_)) => {}
                (Ok(s), None) => {
                    failures += 1;
                    if failures <= LOGGED_ERRORS {
                        log.push((
                            "warning".into(),
                            format!(
                                "Response {} (HTTP {}) carried no token.",
                                sent, s.head.status
                            ),
                        ));
                    }
                }
                (Err(e), _) => {
                    failures += 1;
                    if failures <= LOGGED_ERRORS {
                        log.push(("warning".into(), format!("No response: {}", e.message)));
                    }
                }
            }
            let exchange = repeater::captured(
                Source::Sequencer,
                &capture.target,
                capture.request.clone(),
                started_at,
                &outcome,
            );
            {
                let db = db.clone();
                tokio::task::spawn_blocking(move || db.insert(exchange)).await??;
            }
            if last_progress.elapsed() >= PROGRESS_EVERY {
                last_progress = Instant::now();
                self.save_capture(
                    db,
                    id,
                    &mut pending,
                    &mut log,
                    sent,
                    failures,
                    tokens,
                    wanted,
                )
                .await?;
            }
        };
        log.push((
            "info".into(),
            format!(
                "{} with {tokens} tokens after {sent} requests ({failures} without a token).",
                if status == TaskStatus::Cancelled {
                    "Stopped"
                } else {
                    "Finished"
                }
            ),
        ));
        self.save_capture(
            db,
            id,
            &mut pending,
            &mut log,
            sent,
            failures,
            tokens,
            wanted,
        )
        .await?;
        Ok(status)
    }

    #[allow(clippy::too_many_arguments)]
    async fn save_capture(
        &self,
        db: &ProjectDb,
        id: TaskId,
        pending: &mut Vec<(u64, String)>,
        log: &mut Vec<(String, String)>,
        sent: u64,
        failures: u64,
        tokens: u64,
        wanted: u64,
    ) -> Result<(), AppError> {
        let rows = std::mem::take(pending);
        let lines = std::mem::take(log);
        let progress = TaskProgress {
            requests: sent,
            errors: failures,
            discovered: tokens,
            queued: wanted.saturating_sub(tokens),
            progress: format!("{tokens} / {wanted} tokens"),
        };
        let db = db.clone();
        tokio::task::spawn_blocking(move || {
            db.sequencer_record(id, rows)?;
            db.task_progress(id, progress)?;
            db.task_log(id, lines)
        })
        .await??;
        let _ = self
            .inner
            .events
            .send(chosproxy_core::event::EngineEvent::TasksChanged);
        Ok(())
    }
}
