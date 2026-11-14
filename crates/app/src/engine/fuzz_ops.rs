//! The fuzzer (spec §25, M11): an attack is a task that sends one request
//! per payload combination, plus a baseline with the original values, and
//! records status, length, time, errors, grep matches and extracts.

use super::task_ops::{Control, LOGGED_ERRORS, PROGRESS_EVERY};
use super::{Engine, limits_of};
use crate::AppError;
use crate::cookies;
use crate::repeater;
use chosproxy_core::Source;
use chosproxy_core::time::now_ms;
use chosproxy_fuzzer::{
    BUILTIN, BuiltinList, FuzzConfig, MARKER, MAX_POSITIONS, Prepared, clear_marks,
};
use chosproxy_http::content::{body_text, split_message};
use chosproxy_request::{Destination, Limiter, SendError, SendLimits, Sent, UpstreamSecurity};
use chosproxy_scanner::insertion::{self, PointKind};
use chosproxy_storage::{
    FuzzResultQuery, FuzzResultRow, FuzzResultsPage, Highlight, NewFuzzResult, ProjectDb,
    RepeaterTarget, TaskId, TaskProgress, TaskRow, TaskStatus, validate_target,
};
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use ts_rs::TS;

/// Decoded response bytes grep looks at.
const GREP_LIMIT: usize = 2 * 1024 * 1024;

/// What an attack would do, before starting it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FuzzPreview {
    /// Payload positions marked.
    pub positions: u32,
    /// Requests the attack would send (plus one baseline).
    #[ts(type = "number")]
    pub requests: u64,
    /// The payloads of the first requests.
    pub samples: Vec<Vec<String>>,
    /// Why the attack cannot start, if it cannot.
    pub error: Option<String>,
}

/// One request's outcome.
struct Shot {
    idx: u64,
    payloads: Vec<String>,
    position: Option<u32>,
    request: Vec<u8>,
    started_at: i64,
    outcome: Result<Sent, SendError>,
}

/// What the requests of an attack share.
struct Attack {
    prepared: Prepared,
    config: FuzzConfig,
    target: RepeaterTarget,
    destination: Destination,
    security: UpstreamSecurity,
    limits: SendLimits,
    limiter: Limiter,
}

impl Engine {
    /// Default attack settings.
    pub fn fuzz_defaults(&self) -> FuzzConfig {
        FuzzConfig::default()
    }

    /// The payload lists that come with ChosProxy.
    pub fn fuzz_builtin_lists(&self) -> Vec<BuiltinList> {
        BUILTIN.to_vec()
    }

    /// The request with its insertion points of `kinds` marked as payload
    /// positions (at most [`MAX_POSITIONS`]); no kinds means the default
    /// ones (parameters, cookies and body values). Old markers are removed.
    pub fn fuzz_auto_mark(&self, text: &str, kinds: &[PointKind]) -> String {
        let clean = clear_marks(text);
        let kinds = if kinds.is_empty() {
            &PointKind::DEFAULT[..]
        } else {
            kinds
        };
        let mut points = insertion::insertion_points(&clean, kinds);
        points.truncate(MAX_POSITIONS);
        insertion::mark(&clean, &points, MARKER)
    }

    /// Positions, request count and first payloads of an attack.
    pub fn fuzz_preview(&self, config: &FuzzConfig) -> FuzzPreview {
        let positions = chosproxy_fuzzer::Template::parse(&config.template)
            .map_or(0, |t| u32::try_from(t.positions()).unwrap_or(0));
        let library = self.library();
        match config.prepare_with(&|id, v| library.run_payload(id, v)) {
            Ok(p) => FuzzPreview {
                positions,
                requests: p.plan.count(),
                samples: (0..p.plan.count().min(5))
                    .filter_map(|i| p.plan.attempt(i))
                    .map(|a| a.payloads)
                    .collect(),
                error: None,
            },
            Err(e) => FuzzPreview {
                positions,
                requests: 0,
                samples: Vec::new(),
                error: Some(e),
            },
        }
    }

    /// A page of an attack's results.
    pub async fn fuzz_results(
        &self,
        task: TaskId,
        query: FuzzResultQuery,
    ) -> Result<FuzzResultsPage, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.fuzz_results(task, &query)).await??)
    }

    /// Sets or clears a result's highlight colour.
    pub async fn fuzz_set_color(
        &self,
        id: i64,
        color: Option<Highlight>,
    ) -> Result<FuzzResultRow, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.fuzz_set_color(id, color)).await??)
    }

    /// Writes every result of an attack to `path` as CSV.
    pub async fn fuzz_export_csv(
        &self,
        task: TaskId,
        path: &std::path::Path,
    ) -> Result<std::path::PathBuf, AppError> {
        let db = self.db()?;
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let row = db
                .task(task)?
                .filter(|t| t.kind == "fuzz")
                .ok_or_else(|| AppError::Invalid(format!("attack {task} not found")))?;
            let config: FuzzConfig = serde_json::from_str(&row.config).unwrap_or_default();
            let csv = results_csv(&config, &db.fuzz_all_results(task)?);
            std::fs::write(&path, csv)?;
            Ok::<_, AppError>(path)
        })
        .await?
    }

    /// Starts an attack.
    pub async fn fuzz_start(&self, config: FuzzConfig) -> Result<TaskRow, AppError> {
        let library = self.library();
        let prepared = config
            .prepare_with(&|id, v| library.run_payload(id, v))
            .map_err(AppError::Invalid)?;
        let target = validate_target(&RepeaterTarget {
            scheme: config.target.scheme.clone(),
            host: config.target.host.clone(),
            port: config.target.port,
        })?;
        let first_line = config.template.lines().next().unwrap_or_default();
        let full_path = first_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .replace(chosproxy_fuzzer::MARKER, "");
        self.ensure_in_scope(&target, &full_path, "fuzzer")?;
        let path: String = full_path.chars().take(60).collect();
        let name = format!("Fuzz {}{path}", target.host);
        let json = serde_json::to_string(&config)
            .map_err(|e| AppError::Invalid(format!("invalid fuzzer settings: {e}")))?;
        let security = self
            .tls_security(&target.host, target.scheme == "https")
            .await?;
        let timeout = Duration::from_secs(u64::from(config.timeout_secs));
        let attack = Attack {
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
            prepared,
            config,
            target,
            security,
        };
        self.launch_task(
            "fuzz",
            "Fuzzer",
            name,
            json,
            move |engine, db, id, rx| async move {
                engine.run_fuzz(&db, id, Arc::new(attack), rx).await
            },
        )
        .await
    }

    /// Sends request `idx` (0 = baseline), retrying when no response comes.
    async fn shoot(&self, attack: Arc<Attack>, idx: u64, db: ProjectDb) -> Shot {
        let _permit = attack.limiter.acquire().await;
        let plan = &attack.prepared.plan;
        let (text, payloads, position) = if idx == 0 {
            (
                plan.template().render(&[]),
                plan.template().defaults().to_vec(),
                None,
            )
        } else {
            match plan.attempt(idx - 1) {
                Some(a) => (
                    plan.render(&a),
                    a.payloads,
                    a.position.and_then(|p| u32::try_from(p).ok()),
                ),
                None => (String::new(), Vec::new(), None),
            }
        };
        let started_at = now_ms();
        let config = &attack.config;
        let mut request = match repeater::request_bytes(
            &text,
            config.head_crlf,
            config.body_crlf,
            config.update_content_length,
        ) {
            Ok(r) => r,
            Err(e) => {
                return Shot {
                    idx,
                    payloads,
                    position,
                    request: text.into_bytes(),
                    started_at,
                    outcome: Err(SendError {
                        message: e.to_string(),
                        timing: Default::default(),
                        peer: None,
                    }),
                };
            }
        };
        if config.use_cookie_jar {
            let jar = tokio::task::spawn_blocking(move || db.cookie_jar_list())
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            let path = chosproxy_storage::request_path(&text);
            let matching = cookies::cookies_for(
                &jar,
                &attack.target.host,
                &path,
                attack.target.scheme == "https",
                now_ms(),
            );
            request = cookies::apply_cookies(&request, &matching).0;
        }
        let mut outcome = Err(SendError {
            message: "not sent".into(),
            timing: Default::default(),
            peer: None,
        });
        for _ in 0..=config.retries {
            outcome = self
                .session_send(
                    "fuzzer",
                    chosproxy_core::Source::Fuzzer,
                    &attack.destination,
                    &attack.security,
                    &request,
                    &attack.limits,
                )
                .await;
            if outcome.is_ok() {
                break;
            }
        }
        Shot {
            idx,
            payloads,
            position,
            request,
            started_at,
            outcome,
        }
    }

    /// The attack loop. Returns how it ended.
    async fn run_fuzz(
        &self,
        db: &ProjectDb,
        id: TaskId,
        attack: Arc<Attack>,
        mut control: watch::Receiver<Control>,
    ) -> Result<TaskStatus, AppError> {
        let total = attack.prepared.plan.count() + 1;
        let config = &attack.config;
        let mut log = vec![(
            "info".to_string(),
            format!(
                "Sending {} requests to {}://{}:{} ({:?}, {} position(s), baseline first): {} at a time, {}.",
                total,
                attack.target.scheme,
                attack.target.host,
                attack.target.port,
                config.mode,
                attack.prepared.plan.template().positions(),
                config.concurrency,
                if config.requests_per_second == 0 {
                    "no rate limit".to_string()
                } else {
                    format!("{} per second", config.requests_per_second)
                }
            ),
        )];
        self.task_log_lines(db, id, std::mem::take(&mut log)).await;
        self.set_status(db, id, TaskStatus::Running).await?;

        let mut inflight: tokio::task::JoinSet<Shot> = tokio::task::JoinSet::new();
        let mut next: u64 = 0;
        let mut done: u64 = 0;
        let mut errors: u64 = 0;
        let mut matched: u64 = 0;
        let mut pending: Vec<NewFuzzResult> = Vec::new();
        let mut last_progress = Instant::now();
        let mut paused = false;
        let mut stopped_early = false;
        let (mut stop_matched, mut stop_errors) = (0_u64, 0_u64);

        let status = loop {
            let wanted = *control.borrow_and_update();
            if wanted == Control::Cancel {
                inflight.abort_all();
                log.push(("info".into(), "Cancelled.".into()));
                break TaskStatus::Cancelled;
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
            while !paused && inflight.len() < config.concurrency as usize && next < total {
                let engine = self.clone();
                let attack = attack.clone();
                let db = db.clone();
                let idx = next;
                inflight.spawn(super::task_ops::in_task(
                    super::task_ops::current_task(),
                    async move { engine.shoot(attack, idx, db).await },
                ));
                next += 1;
            }
            if inflight.is_empty() {
                if paused {
                    if control.changed().await.is_err() {
                        break TaskStatus::Cancelled;
                    }
                    continue;
                }
                break TaskStatus::Completed;
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
            let Some(Ok(shot)) = joined else {
                continue;
            };
            done += 1;
            let (status, length, duration_ms, error, matches, extracts) = match &shot.outcome {
                Ok(sent) => {
                    let (head, raw_body) = split_message(&sent.response);
                    let mut text = String::from_utf8_lossy(head).into_owned();
                    text.push_str(&body_text(&sent.head, raw_body, GREP_LIMIT).unwrap_or_default());
                    (
                        Some(sent.head.status),
                        Some(sent.response_len),
                        sent.timing.total_ms,
                        sent.incomplete.clone(),
                        attack.prepared.grep.matches(&text),
                        attack.prepared.grep.extracts(&text),
                    )
                }
                Err(e) => {
                    errors += 1;
                    if errors <= LOGGED_ERRORS {
                        log.push((
                            "warning".into(),
                            format!("Request {}: {}", shot.idx, e.message),
                        ));
                    } else if errors == LOGGED_ERRORS + 1 {
                        log.push(("warning".into(), "More errors are only counted.".into()));
                    }
                    (
                        None,
                        None,
                        e.timing.total_ms,
                        Some(e.message.clone()),
                        Vec::new(),
                        Vec::new(),
                    )
                }
            };
            if matches.iter().any(|m| *m) {
                matched += 1;
            }
            // The baseline (request 0) does not count towards a stop.
            let stop_reason = if shot.idx > 0 {
                if matches.iter().any(|m| *m) {
                    stop_matched += 1;
                }
                if status.is_none() {
                    stop_errors += 1;
                }
                config.stop.reason(stop_matched, stop_errors, status)
            } else {
                None
            };
            let exchange = repeater::captured(
                Source::Fuzzer,
                &attack.target,
                shot.request,
                shot.started_at,
                &shot.outcome,
            );
            let exchange_id = {
                let db = db.clone();
                tokio::task::spawn_blocking(move || db.insert(exchange)).await??
            };
            pending.push(NewFuzzResult {
                idx: shot.idx,
                payloads: shot.payloads,
                position: shot.position,
                exchange_id: Some(exchange_id),
                status,
                length,
                duration_ms,
                error,
                matches,
                extracts,
            });
            if let Some(reason) = stop_reason {
                inflight.abort_all();
                stopped_early = true;
                log.push(("info".into(), format!("Stop condition met: {reason}.")));
                break TaskStatus::Completed;
            }
            if last_progress.elapsed() >= PROGRESS_EVERY {
                last_progress = Instant::now();
                self.save_fuzz(db, id, &mut pending, &mut log, done, errors, matched, total)
                    .await?;
            }
        };
        log.push((
            "info".into(),
            format!(
                "{} after {done} of {total} requests ({errors} without a response, {matched} matched).",
                if status == TaskStatus::Cancelled || stopped_early {
                    "Stopped"
                } else {
                    "Finished"
                }
            ),
        ));
        self.save_fuzz(db, id, &mut pending, &mut log, done, errors, matched, total)
            .await?;
        Ok(status)
    }

    #[allow(clippy::too_many_arguments)]
    async fn save_fuzz(
        &self,
        db: &ProjectDb,
        id: TaskId,
        pending: &mut Vec<NewFuzzResult>,
        log: &mut Vec<(String, String)>,
        done: u64,
        errors: u64,
        matched: u64,
        total: u64,
    ) -> Result<(), AppError> {
        let rows = std::mem::take(pending);
        let lines = std::mem::take(log);
        let progress = TaskProgress {
            requests: done,
            errors,
            discovered: matched,
            queued: total.saturating_sub(done),
            progress: format!("{done} / {total}"),
        };
        let db = db.clone();
        tokio::task::spawn_blocking(move || {
            db.fuzz_record(id, rows)?;
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

/// One CSV field: quoted when needed; a leading `=`, `+`, `-`, `@`, tab or
/// carriage return is prefixed with `'` so spreadsheets do not run it as a
/// formula (payloads and extracts come from the tested application).
fn csv_field(value: &str) -> String {
    let guarded = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// The results of an attack as CSV: request, payloads, status, length,
/// time, error, one column per match term and per extract, colour.
fn results_csv(config: &FuzzConfig, rows: &[FuzzResultRow]) -> String {
    let terms: Vec<&String> = config.grep_match.iter().filter(|m| !m.is_empty()).collect();
    let extracts: Vec<&String> = config
        .grep_extract
        .iter()
        .filter(|m| !m.is_empty())
        .collect();
    let mut header: Vec<String> = [
        "request", "payloads", "status", "length", "time_ms", "error",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    header.extend(terms.iter().map(|t| format!("match: {t}")));
    header.extend(extracts.iter().map(|x| format!("extract: {x}")));
    header.push("highlight".into());
    let mut out = header
        .iter()
        .map(|h| csv_field(h))
        .collect::<Vec<_>>()
        .join(",");
    out.push_str("\r\n");
    for r in rows {
        let mut fields = vec![
            if r.idx == 0 {
                "baseline".to_string()
            } else {
                r.idx.to_string()
            },
            r.payloads.join(" | "),
            r.status.map(|s| s.to_string()).unwrap_or_default(),
            r.length.map(|n| n.to_string()).unwrap_or_default(),
            r.duration_ms.to_string(),
            r.error.clone().unwrap_or_default(),
        ];
        fields.extend((0..terms.len()).map(|i| {
            if r.matches.get(i).copied().unwrap_or(false) {
                "yes".to_string()
            } else {
                String::new()
            }
        }));
        fields.extend((0..extracts.len()).map(|i| r.extracts.get(i).cloned().unwrap_or_default()));
        fields.push(r.color.map(|c| c.as_str().to_string()).unwrap_or_default());
        out.push_str(
            &fields
                .iter()
                .map(|f| csv_field(f))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn row(idx: u64, payload: &str) -> FuzzResultRow {
        FuzzResultRow {
            id: i64::try_from(idx).unwrap() + 1,
            idx,
            payloads: vec![payload.into()],
            position: Some(0),
            exchange_id: None,
            status: Some(200),
            length: Some(42),
            duration_ms: 7,
            error: None,
            matches: vec![idx == 1],
            extracts: vec!["tok,en".into()],
            color: (idx == 1).then_some(Highlight::Green),
        }
    }

    #[test]
    fn csv_quotes_and_guards_formulas() {
        let config = FuzzConfig {
            grep_match: vec!["admin".into(), String::new()],
            grep_extract: vec!["id=(\\d+)".into()],
            ..FuzzConfig::default()
        };
        let csv = results_csv(&config, &[row(0, "x"), row(1, "=cmd()"), row(2, "a \"b\"")]);
        let lines: Vec<&str> = csv.split("\r\n").collect();
        assert_eq!(
            lines[0],
            "request,payloads,status,length,time_ms,error,match: admin,extract: id=(\\d+),highlight"
        );
        assert_eq!(lines[1], "baseline,x,200,42,7,,,\"tok,en\",");
        assert_eq!(lines[2], "1,'=cmd(),200,42,7,,yes,\"tok,en\",green");
        assert_eq!(lines[3], "2,\"a \"\"b\"\"\",200,42,7,,,\"tok,en\",");
        assert_eq!(lines.len(), 5, "a final line break");
    }
}
