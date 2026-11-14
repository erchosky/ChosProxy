//! Engine operations on reports: building a report from chosen findings,
//! previewing it and exporting it as HTML, Markdown or JSON.

use super::Engine;
use crate::AppError;
use chosproxy_core::ScopeConfig;
use chosproxy_core::scope::{ScopeProtocol, ScopeRule};
use chosproxy_report::{
    Report, ReportEvidence, ReportFinding, ReportMeta, prepare_message, render_html,
    render_markdown,
};
use chosproxy_storage::{FindingId, StoredFinding};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Evidence size limits per message, in KiB.
const MIN_EVIDENCE_KIB: u32 = 1;
const MAX_EVIDENCE_KIB: u32 = 1024;

/// Report output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ReportFormat {
    /// Self-contained HTML page.
    Html,
    /// Markdown.
    Markdown,
    /// Structured JSON (`chosproxy-report-v1`).
    Json,
}

impl ReportFormat {
    fn name(self) -> &'static str {
        match self {
            ReportFormat::Html => "HTML",
            ReportFormat::Markdown => "Markdown",
            ReportFormat::Json => "JSON",
        }
    }
}

/// What goes into a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReportOptions {
    /// Report title.
    pub title: String,
    /// Client or system owner.
    pub client: String,
    /// Tester or team.
    pub author: String,
    /// Executive summary.
    pub executive_summary: String,
    /// Methodology.
    pub methodology: String,
    /// Findings to include, in any order (the report sorts them).
    #[ts(type = "number[]")]
    pub finding_ids: Vec<FindingId>,
    /// Include the captured requests and responses.
    pub include_evidence: bool,
    /// Replace credential header values in the evidence.
    pub redact_secrets: bool,
    /// Size limit per evidence message, in KiB.
    pub max_evidence_kib: u32,
}

/// `camelCase` status values as words ("falsePositive" → "false positive").
fn words(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 4);
    for c in value.chars() {
        if c.is_ascii_uppercase() {
            out.push(' ');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn scope_line(kind: &str, r: &ScopeRule) -> String {
    let protocol = match r.protocol {
        ScopeProtocol::Any => "http(s)",
        ScopeProtocol::Http => "http",
        ScopeProtocol::Https => "https",
    };
    let host = match (r.host.is_empty(), r.regex, r.include_subdomains) {
        (true, _, _) => "any host".to_string(),
        (false, true, _) => format!("host matching /{}/", r.host),
        (false, false, true) => format!("{} and subdomains", r.host),
        (false, false, false) => r.host.clone(),
    };
    let mut line = format!("{kind}: {protocol} {host}");
    if let Some(p) = r.port {
        line.push_str(&format!(", port {p}"));
    }
    if !r.path.is_empty() {
        if r.regex {
            line.push_str(&format!(", path matching /{}/", r.path));
        } else {
            line.push_str(&format!(", path {}*", r.path));
        }
    }
    if !r.comment.trim().is_empty() {
        line.push_str(&format!(" — {}", r.comment.trim()));
    }
    line
}

/// Enabled scope rules as readable lines.
fn scope_lines(c: &ScopeConfig) -> Vec<String> {
    let include = c.include.iter().filter(|r| r.enabled);
    let exclude = c.exclude.iter().filter(|r| r.enabled);
    include
        .map(|r| scope_line("Include", r))
        .chain(exclude.map(|r| scope_line("Exclude", r)))
        .collect()
}

fn report_finding(f: StoredFinding, options: &ReportOptions) -> ReportFinding {
    let limit = options
        .max_evidence_kib
        .clamp(MIN_EVIDENCE_KIB, MAX_EVIDENCE_KIB) as usize
        * 1024;
    let evidence = if options.include_evidence {
        f.evidence
            .into_iter()
            .map(|e| ReportEvidence {
                method: e.row.method,
                url: e.row.url,
                status: e.row.status_code,
                note: e.row.note,
                request: prepare_message(&e.request, options.redact_secrets, limit),
                response: e
                    .response
                    .map(|r| prepare_message(&r, options.redact_secrets, limit)),
            })
            .collect()
    } else {
        Vec::new()
    };
    ReportFinding {
        id: f.row.id,
        title: f.row.title,
        kind: f.row.kind,
        severity: f.row.severity.as_str().to_string(),
        confidence: f.row.confidence.as_str().to_string(),
        status: words(f.row.status.as_str()),
        host: f.row.host,
        url: f.row.url,
        description: f.description,
        remediation: f.remediation,
        notes: f.notes,
        evidence,
    }
}

impl Engine {
    async fn build_report(&self, options: &ReportOptions) -> Result<Report, AppError> {
        let db = self.db()?;
        // A finding listed twice is reported once.
        let mut ids = options.finding_ids.clone();
        ids.sort_unstable();
        ids.dedup();
        let stored = tokio::task::spawn_blocking(move || {
            ids.into_iter()
                .map(|id| {
                    db.finding(id)?
                        .ok_or_else(|| AppError::Invalid(format!("finding {id} not found")))
                })
                .collect::<Result<Vec<_>, AppError>>()
        })
        .await??;
        let findings = stored
            .into_iter()
            .map(|f| report_finding(f, options))
            .collect();
        let meta = ReportMeta {
            title: options.title.trim().to_string(),
            client: options.client.trim().to_string(),
            author: options.author.trim().to_string(),
            executive_summary: options.executive_summary.clone(),
            methodology: options.methodology.clone(),
        };
        Ok(Report::new(
            meta,
            scope_lines(&self.effective_scope()),
            findings,
            chosproxy_core::time::now_ms(),
        ))
    }

    /// The report as HTML, for the preview.
    pub async fn report_preview(&self, options: ReportOptions) -> Result<String, AppError> {
        Ok(render_html(&self.build_report(&options).await?))
    }

    /// Writes the report to `path` and records the export in the audit log.
    pub async fn export_report(
        &self,
        options: ReportOptions,
        format: ReportFormat,
        path: &Path,
    ) -> Result<PathBuf, AppError> {
        let report = self.build_report(&options).await?;
        let text = match format {
            ReportFormat::Html => render_html(&report),
            ReportFormat::Markdown => render_markdown(&report),
            ReportFormat::Json => report.render_json(),
        };
        std::fs::write(path, text)?;
        self.audit(
            "report.exported",
            &format!(
                "{} report, {} finding(s), evidence {}{}: {}",
                format.name(),
                report.findings.len(),
                if options.include_evidence {
                    "included"
                } else {
                    "left out"
                },
                if options.include_evidence && !options.redact_secrets {
                    " without redaction"
                } else {
                    ""
                },
                path.display()
            ),
        );
        Ok(path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_rules_read_as_text() {
        let config = ScopeConfig {
            include: vec![ScopeRule {
                host: "lab.test".into(),
                include_subdomains: true,
                path: "/api/".into(),
                comment: "staging".into(),
                ..ScopeRule::default()
            }],
            exclude: vec![
                ScopeRule {
                    protocol: ScopeProtocol::Https,
                    host: "^cdn\\.".into(),
                    regex: true,
                    port: Some(8443),
                    ..ScopeRule::default()
                },
                ScopeRule {
                    enabled: false,
                    ..ScopeRule::default()
                },
            ],
            ..ScopeConfig::default()
        };
        assert_eq!(
            scope_lines(&config),
            [
                "Include: http(s) lab.test and subdomains, path /api/* — staging",
                "Exclude: https host matching /^cdn\\./, port 8443",
            ]
        );
    }

    #[test]
    fn status_values_read_as_words() {
        assert_eq!(words("falsePositive"), "false positive");
        assert_eq!(words("open"), "open");
    }
}
