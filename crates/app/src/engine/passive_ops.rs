//! Passive analysis (M9): a background task reads the history in id order,
//! runs the passive checks on proxy (and Repeater) traffic and records what
//! they find as findings. It only reads stored traffic; nothing is sent.

use super::{Engine, Inner, read};
use crate::AppError;
use chosproxy_core::Source;
use chosproxy_core::event::EngineEvent;
use chosproxy_core::scope::UrlParts;
use chosproxy_http::content::split_message;
use chosproxy_scanner::javascript::JS_CHECKS;
use chosproxy_scanner::{
    CHECKS, CROSS_CHECKS, CheckInfo, Confidence, CrossAnalyzer, Message, Severity, analyze,
};
use chosproxy_storage::{FindingSeverity, PassiveIssue, StoredExchange};
use serde::Serialize;
use std::sync::{Arc, Weak};
use std::time::Duration;
use ts_rs::TS;

/// Exchanges analyzed per round.
const BATCH: u32 = 100;
/// Pause between rounds when the analyzer has caught up.
const IDLE: Duration = Duration::from_millis(1500);

/// Where passive analysis stands, for the Findings view.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PassiveStatus {
    /// Analysis of new traffic is on.
    pub enabled: bool,
    /// Only in-scope traffic is analyzed.
    pub in_scope_only: bool,
    /// A scope is defined (without one, all traffic is analyzed).
    pub scope_defined: bool,
    /// Exchanges up to this id have been looked at.
    #[ts(type = "number")]
    pub analyzed_up_to: i64,
    /// Highest exchange id stored.
    #[ts(type = "number")]
    pub last_exchange_id: i64,
    /// Checks turned on / available.
    pub checks_enabled: u32,
    /// Checks available.
    pub checks_total: u32,
}

fn severity(s: Severity) -> FindingSeverity {
    match s {
        Severity::Info => FindingSeverity::Info,
        Severity::Low => FindingSeverity::Low,
        Severity::Medium => FindingSeverity::Medium,
        Severity::High => FindingSeverity::High,
    }
}

fn confidence(c: Confidence) -> chosproxy_storage::Confidence {
    match c {
        Confidence::Tentative => chosproxy_storage::Confidence::Tentative,
        Confidence::Firm => chosproxy_storage::Confidence::Firm,
        Confidence::Certain => chosproxy_storage::Confidence::Certain,
    }
}

/// The detections in one stored exchange (no filtering by source or scope).
pub(crate) fn issues_in(exchange: &StoredExchange, disabled: &[String]) -> Vec<PassiveIssue> {
    let row = &exchange.row;
    let message = Message::new(
        &row.scheme,
        &row.host,
        row.port,
        &row.path,
        &exchange.request,
        exchange.response.as_deref(),
    );
    analyze(&message, |id| !disabled.iter().any(|d| d == id))
        .into_iter()
        .map(|issue| passive_issue(row.id, issue))
        .collect()
}

/// The message the checks read, for a stored exchange.
pub(crate) fn message_of(exchange: &StoredExchange) -> Message {
    let row = &exchange.row;
    Message::new(
        &row.scheme,
        &row.host,
        row.port,
        &row.path,
        &exchange.request,
        exchange.response.as_deref(),
    )
}

/// Every passive check: single message, JavaScript, cross-request.
pub(crate) fn all_checks() -> impl Iterator<Item = &'static CheckInfo> {
    CHECKS
        .iter()
        .copied()
        .chain(JS_CHECKS.iter().copied())
        .chain(CROSS_CHECKS.iter().copied())
}

/// Feeds one exchange to a cross-request analyzer; the findings it
/// completes (on this exchange or an earlier one).
pub(crate) fn cross_issues(
    cross: &mut CrossAnalyzer,
    exchange: &StoredExchange,
    disabled: &[String],
) -> Vec<PassiveIssue> {
    let message = message_of(exchange);
    let (_, body) = split_message(&exchange.request);
    cross
        .observe(exchange.row.id, &message, body, |id| {
            !disabled.iter().any(|d| d == id)
        })
        .into_iter()
        .map(|(id, issue)| passive_issue(id, issue))
        .collect()
}

/// A detection as a finding to record (merged by its dedup key).
pub(crate) fn passive_issue(exchange_id: i64, issue: chosproxy_scanner::Issue) -> PassiveIssue {
    let mut dedup_key = format!("passive:{}:{}", issue.check.id, issue.dedup);
    if dedup_key.len() > 1000 {
        dedup_key.truncate(
            (0..=1000)
                .rev()
                .find(|i| dedup_key.is_char_boundary(*i))
                .unwrap_or(0),
        );
    }
    PassiveIssue {
        source: "passive",
        exchange_id,
        dedup_key,
        kind: issue.check.kind.to_string(),
        title: issue.title,
        severity: severity(issue.severity),
        confidence: confidence(issue.confidence),
        description: issue.check.description.to_string(),
        remediation: issue.check.remediation.to_string(),
        evidence_note: issue.evidence,
    }
}

/// An active audit finding recorded with both baseline and probe exchanges as evidence.
pub(crate) fn active_issue_pair(
    baseline_id: i64,
    probe_id: i64,
    finding: &chosproxy_scanner::ActiveFinding,
) -> Vec<PassiveIssue> {
    let issue = chosproxy_scanner::into_issue(finding);
    let mut dedup_key = format!("scanner:{}:{}", issue.check.id, issue.dedup);
    if dedup_key.len() > 1000 {
        dedup_key.truncate(
            (0..=1000)
                .rev()
                .find(|i| dedup_key.is_char_boundary(*i))
                .unwrap_or(0),
        );
    }
    let base = PassiveIssue {
        source: "scanner",
        exchange_id: baseline_id,
        dedup_key: dedup_key.clone(),
        kind: issue.check.kind.to_string(),
        title: issue.title.clone(),
        severity: severity(issue.severity),
        confidence: confidence(issue.confidence),
        description: issue.check.description.to_string(),
        remediation: issue.check.remediation.to_string(),
        evidence_note: format!("Baseline exchange #{baseline_id} before active probe."),
    };
    let probe = PassiveIssue {
        source: "scanner",
        exchange_id: probe_id,
        dedup_key,
        kind: issue.check.kind.to_string(),
        title: issue.title,
        severity: severity(issue.severity),
        confidence: confidence(issue.confidence),
        description: issue.check.description.to_string(),
        remediation: issue.check.remediation.to_string(),
        evidence_note: issue.evidence,
    };
    if baseline_id == probe_id {
        vec![probe]
    } else {
        vec![base, probe]
    }
}

impl Engine {
    /// The passive checks, for the settings (single message, JavaScript
    /// and cross-request).
    pub fn passive_checks(&self) -> Vec<CheckInfo> {
        all_checks().copied().collect()
    }

    /// The built-in active audit checks (M13).
    pub fn active_checks(&self) -> Vec<chosproxy_scanner::ActiveCheckInfo> {
        chosproxy_scanner::ACTIVE_CHECKS.to_vec()
    }

    /// Where passive analysis of the open project stands.
    pub async fn passive_status(&self) -> Result<PassiveStatus, AppError> {
        let db = self.db()?;
        let read_only = db.is_read_only();
        let (analyzed_up_to, last_exchange_id) = tokio::task::spawn_blocking(move || {
            Ok::<_, chosproxy_storage::StorageError>((db.passive_cursor()?, db.last_exchange_id()?))
        })
        .await??;
        let settings = self.settings().passive;
        let total = u32::try_from(all_checks().count()).unwrap_or(u32::MAX);
        let disabled = all_checks()
            .filter(|c| settings.disabled_checks.iter().any(|d| d == c.id))
            .count();
        Ok(PassiveStatus {
            enabled: settings.enabled && !read_only,
            in_scope_only: settings.in_scope_only,
            scope_defined: !read(&self.inner.scope).is_empty(),
            analyzed_up_to,
            last_exchange_id,
            checks_enabled: total.saturating_sub(u32::try_from(disabled).unwrap_or(0)),
            checks_total: total,
        })
    }

    /// Analyzes all traffic of the project again (findings already recorded
    /// are merged, not duplicated).
    pub async fn passive_reanalyze(&self) -> Result<PassiveStatus, AppError> {
        self.ensure_writable()?;
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.passive_set_cursor(0)).await??;
        self.passive_reset_cross();
        self.inner.passive_wake.notify_one();
        self.passive_status().await
    }

    /// Forgets what the cross-request checks remember (new project,
    /// analysis from the start).
    pub(crate) fn passive_reset_cross(&self) {
        *super::lock(&self.inner.passive_cross) = CrossAnalyzer::new();
    }

    /// Starts the analyzer task once (it follows whichever project is open
    /// and stops when the engine is dropped).
    pub(crate) fn ensure_passive_task(&self) {
        let mut task = match self.inner.passive_task.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if task.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        let weak: Weak<Inner> = Arc::downgrade(&self.inner);
        *task = Some(tokio::spawn(async move {
            loop {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let engine = Engine::from_inner(inner);
                let more = match engine.passive_round().await {
                    Ok(more) => more,
                    Err(AppError::NoProject) => false,
                    Err(e) => {
                        tracing::warn!(error = %e, "passive analysis round failed");
                        false
                    }
                };
                if !more {
                    // Holds the engine for at most IDLE after it is dropped.
                    let notified = engine.inner.passive_wake.notified();
                    let _ = tokio::time::timeout(IDLE, notified).await;
                }
            }
        }));
    }

    /// Analyzes the next batch of stored traffic. True if more is waiting.
    async fn passive_round(&self) -> Result<bool, AppError> {
        let settings = self.settings().passive;
        if !settings.enabled {
            return Ok(false);
        }
        let db = self.db()?;
        if db.is_read_only() {
            return Ok(false);
        }
        let ids = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || {
                let cursor = db.passive_cursor()?;
                db.exchange_ids_after(cursor, BATCH)
            })
            .await??
        };
        let Some(&last) = ids.last() else {
            return Ok(false);
        };
        let scope = read(&self.inner.scope);
        let checks = self.library().checks();
        let issues = {
            let db = db.clone();
            let ids = ids.clone();
            let engine = self.clone();
            tokio::task::spawn_blocking(move || {
                let exchanges = db.exchanges_bulk(&ids)?;
                let mut cross = super::lock(&engine.inner.passive_cross);
                let mut issues = Vec::new();
                for ex in &exchanges {
                    let wanted = match ex.row.source {
                        Source::Proxy | Source::Crawler | Source::Fuzzer | Source::Scanner => true,
                        Source::Repeater | Source::Agent => settings.repeater,
                        _ => false,
                    };
                    if !wanted || ex.response.is_none() {
                        continue;
                    }
                    if settings.in_scope_only && !scope.is_empty() {
                        let parts = UrlParts {
                            scheme: ex.row.scheme.clone(),
                            host: ex.row.host.clone(),
                            port: ex.row.port,
                            path: ex.row.path.clone(),
                        };
                        if !scope.contains(&parts.target()) {
                            continue;
                        }
                    }
                    issues.extend(issues_in(ex, &settings.disabled_checks));
                    issues.extend(super::scan_ops::javascript_issues(
                        ex,
                        &settings.disabled_checks,
                    ));
                    issues.extend(cross_issues(&mut cross, ex, &settings.disabled_checks));
                    issues.extend(super::script_ops::script_issues(ex, &checks));
                }
                Ok::<_, chosproxy_storage::StorageError>(issues)
            })
            .await??
        };
        let recorded =
            tokio::task::spawn_blocking(move || db.passive_record(issues, last)).await??;
        if recorded.created + recorded.updated > 0 {
            let _ = self.inner.events.send(EngineEvent::FindingsChanged {
                created: recorded.created,
            });
        }
        Ok(ids.len() == BATCH as usize)
    }
}
