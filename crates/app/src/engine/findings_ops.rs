//! Engine operations on findings.

use super::Engine;
use crate::AppError;
use crate::detail::message_view;
use crate::types::MessageView;
use chosproxy_core::event::EngineEvent;
use chosproxy_storage::{
    EvidenceRow, FindingId, FindingQuery, FindingRow, FindingUpdate, NewFinding, NewSuppression,
    StorageError, SuppressionRule,
};
use serde::Serialize;
use ts_rs::TS;

/// One piece of evidence with its messages prepared for display.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EvidenceView {
    /// Metadata.
    pub row: EvidenceRow,
    /// Request.
    pub request: MessageView,
    /// Response.
    pub response: Option<MessageView>,
}

/// A finding with its texts and evidence.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FindingDetail {
    /// Listing data.
    pub row: FindingRow,
    /// What the problem is.
    pub description: String,
    /// How to fix it.
    pub remediation: String,
    /// The tester's notes.
    pub notes: String,
    /// Evidence, oldest first.
    pub evidence: Vec<EvidenceView>,
}

fn not_found(e: StorageError, what: &str) -> AppError {
    match e {
        StorageError::NotFound(id) => AppError::Invalid(format!("{what} {id} not found")),
        e => e.into(),
    }
}

impl Engine {
    /// Findings matching the filter.
    pub async fn findings(&self, query: FindingQuery) -> Result<Vec<FindingRow>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.findings(&query)).await??)
    }

    /// One finding with its evidence.
    pub async fn finding_detail(&self, id: FindingId) -> Result<FindingDetail, AppError> {
        let db = self.db()?;
        let f = tokio::task::spawn_blocking(move || db.finding(id))
            .await??
            .ok_or_else(|| AppError::Invalid(format!("finding {id} not found")))?;
        Ok(FindingDetail {
            row: f.row,
            description: f.description,
            remediation: f.remediation,
            notes: f.notes,
            evidence: f
                .evidence
                .into_iter()
                .map(|e| EvidenceView {
                    request: message_view(&e.request, Some(e.request.len() as u64), false),
                    response: e
                        .response
                        .as_deref()
                        .map(|r| message_view(r, Some(r.len() as u64), false)),
                    row: e.row,
                })
                .collect(),
        })
    }

    /// Documents a new finding from a history entry.
    pub async fn finding_create(&self, new: NewFinding) -> Result<FindingId, AppError> {
        let db = self.db()?;
        let title = new.title.clone();
        let id = tokio::task::spawn_blocking(move || db.finding_create(new))
            .await?
            .map_err(|e| not_found(e, "request"))?;
        self.audit("finding.created", &format!("#{id} {title}"));
        Ok(id)
    }

    /// Adds a history entry as evidence of a finding.
    pub async fn finding_add_evidence(
        &self,
        finding_id: FindingId,
        exchange_id: i64,
        note: String,
    ) -> Result<(), AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || {
            db.finding_add_evidence(finding_id, exchange_id, &note)
        })
        .await?
        .map_err(|e| not_found(e, "finding or request"))?;
        Ok(())
    }

    /// Removes one piece of evidence.
    pub async fn finding_remove_evidence(&self, evidence_id: i64) -> Result<(), AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.finding_remove_evidence(evidence_id))
            .await?
            .map_err(|e| not_found(e, "evidence"))?;
        Ok(())
    }

    /// Saves a finding; status changes are recorded in the audit log.
    pub async fn finding_update(
        &self,
        id: FindingId,
        update: FindingUpdate,
    ) -> Result<(), AppError> {
        let db = self.db()?;
        let before = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || db.finding(id)).await??
        }
        .ok_or_else(|| AppError::Invalid(format!("finding {id} not found")))?;
        let (status, severity) = (update.status, update.severity);
        tokio::task::spawn_blocking(move || db.finding_update(id, update))
            .await?
            .map_err(|e| not_found(e, "finding"))?;
        if before.row.status != status {
            self.audit(
                "finding.status",
                &format!(
                    "#{id}: {} → {}",
                    before.row.status.as_str(),
                    status.as_str()
                ),
            );
        }
        if before.row.severity != severity {
            self.audit(
                "finding.severity",
                &format!(
                    "#{id}: {} → {}",
                    before.row.severity.as_str(),
                    severity.as_str()
                ),
            );
        }
        Ok(())
    }

    /// Deletes a finding (prefer marking it a false positive, which keeps a
    /// record).
    pub async fn finding_delete(&self, id: FindingId) -> Result<(), AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.finding_delete(id))
            .await?
            .map_err(|e| not_found(e, "finding"))?;
        self.audit("finding.deleted", &format!("#{id}"));
        Ok(())
    }

    /// The suppression rules of the project.
    pub async fn suppressions(&self) -> Result<Vec<SuppressionRule>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.suppressions()).await??)
    }

    /// Adds a suppression rule; with `mark_open`, the open findings it
    /// matches become false positives. Returns how many were marked.
    pub async fn suppression_add(
        &self,
        rule: NewSuppression,
        mark_open: bool,
    ) -> Result<u32, AppError> {
        let db = self.db()?;
        let (rule, marked) =
            tokio::task::spawn_blocking(move || db.suppression_add(rule, mark_open))
                .await?
                .map_err(|e| match e {
                    StorageError::InvalidQuery(m) => AppError::Invalid(m),
                    e => e.into(),
                })?;
        self.audit(
            "finding.suppression_added",
            &format!(
                "\"{}\" on {} {} ({marked} open finding(s) marked as false positives)",
                rule.kind,
                if rule.host.is_empty() {
                    "any host"
                } else {
                    &rule.host
                },
                if rule.path_prefix.is_empty() {
                    ""
                } else {
                    &rule.path_prefix
                },
            ),
        );
        if marked > 0 {
            let _ = self
                .inner
                .events
                .send(EngineEvent::FindingsChanged { created: 0 });
        }
        Ok(marked)
    }

    /// Removes a suppression rule (marked findings keep their status).
    pub async fn suppression_delete(&self, id: i64) -> Result<(), AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.suppression_delete(id))
            .await?
            .map_err(|e| match e {
                StorageError::InvalidQuery(m) => AppError::Invalid(m),
                e => e.into(),
            })?;
        self.audit("finding.suppression_deleted", &format!("rule {id}"));
        Ok(())
    }
}
