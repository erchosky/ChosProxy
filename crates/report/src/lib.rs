//! Assessment reports built from documented findings.
//!
//! Every value that comes from the project (titles, notes, captured
//! requests) is untrusted: the HTML output escapes all of it, has no
//! scripts and no external resources, and evidence can be redacted and
//! shortened before it is written.

mod html;
mod markdown;
mod redact;

use serde::Serialize;

pub use html::render_html;
pub use markdown::render_markdown;
pub use redact::{prepare_message, redact_message};

/// Severities in report order (most severe first).
pub const SEVERITY_ORDER: [&str; 5] = ["critical", "high", "medium", "low", "info"];

/// Who the report is for and what it covers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportMeta {
    /// Report title.
    pub title: String,
    /// Client or system owner.
    pub client: String,
    /// Tester or team.
    pub author: String,
    /// Executive summary written by the tester.
    pub executive_summary: String,
    /// How the assessment was done.
    pub methodology: String,
}

/// One message of a piece of evidence, prepared for the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportMessage {
    /// The message as text (lossy UTF-8, possibly redacted and shortened).
    pub text: String,
    /// Values of sensitive headers were replaced.
    pub redacted: bool,
    /// The message was cut at the size limit.
    pub truncated: bool,
}

/// One piece of evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportEvidence {
    /// Request method.
    pub method: String,
    /// Request URL.
    pub url: String,
    /// Response status.
    pub status: Option<u16>,
    /// What it shows.
    pub note: String,
    /// The request.
    pub request: ReportMessage,
    /// The response.
    pub response: Option<ReportMessage>,
}

/// One finding in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportFinding {
    /// Finding id in the project.
    pub id: i64,
    /// Title.
    pub title: String,
    /// Type.
    pub kind: String,
    /// `critical` … `info`.
    pub severity: String,
    /// `certain`, `firm` or `tentative`.
    pub confidence: String,
    /// Review status.
    pub status: String,
    /// Host.
    pub host: String,
    /// Affected URL.
    pub url: String,
    /// What the problem is.
    pub description: String,
    /// How to fix it.
    pub remediation: String,
    /// The tester's notes (technical details).
    pub notes: String,
    /// Evidence (empty when evidence is left out).
    pub evidence: Vec<ReportEvidence>,
}

/// Everything a report contains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Format tag of the JSON output.
    pub format: &'static str,
    /// Product and version that generated it.
    pub generator: String,
    /// Unix epoch ms.
    pub generated_at: i64,
    /// Title, client, author, summary, methodology.
    pub meta: ReportMeta,
    /// Scope rules in effect, as readable lines.
    pub scope: Vec<String>,
    /// Hosts covered by the findings.
    pub targets: Vec<String>,
    /// Findings, most severe first.
    pub findings: Vec<ReportFinding>,
}

/// JSON format tag.
pub const JSON_FORMAT: &str = "chosproxy-report-v1";

impl Report {
    /// Builds a report; findings are sorted by severity, then title.
    pub fn new(
        meta: ReportMeta,
        scope: Vec<String>,
        mut findings: Vec<ReportFinding>,
        generated_at: i64,
    ) -> Report {
        findings.sort_by(|a, b| {
            severity_rank(&a.severity)
                .cmp(&severity_rank(&b.severity))
                .then_with(|| a.title.cmp(&b.title))
        });
        let mut targets: Vec<String> = findings.iter().map(|f| f.host.clone()).collect();
        targets.sort();
        targets.dedup();
        Report {
            format: JSON_FORMAT,
            generator: format!(
                "{} {}",
                chosproxy_core::brand().product_name,
                env!("CARGO_PKG_VERSION")
            ),
            generated_at,
            meta,
            scope,
            targets,
            findings,
        }
    }

    /// Findings per severity, in [`SEVERITY_ORDER`].
    pub fn counts(&self) -> [(&'static str, usize); 5] {
        SEVERITY_ORDER.map(|s| (s, self.findings.iter().filter(|f| f.severity == s).count()))
    }

    /// The report as pretty JSON.
    pub fn render_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// Position of a severity in [`SEVERITY_ORDER`] (unknown values last).
pub fn severity_rank(severity: &str) -> usize {
    SEVERITY_ORDER
        .iter()
        .position(|s| *s == severity)
        .unwrap_or(SEVERITY_ORDER.len())
}

/// `YYYY-MM-DD HH:MM UTC` for a Unix epoch ms.
pub(crate) fn format_time(ms: i64) -> String {
    let compact = chosproxy_core::time::compact_utc(ms);
    // compact_utc gives YYYYMMDD-HHMMSS.
    format!(
        "{}-{}-{} {}:{} UTC",
        &compact[0..4],
        &compact[4..6],
        &compact[6..8],
        &compact[9..11],
        &compact[11..13]
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> Report {
        let message = |text: &str| ReportMessage {
            text: text.into(),
            redacted: false,
            truncated: false,
        };
        let finding = |id, severity: &str, title: &str| ReportFinding {
            id,
            title: title.into(),
            kind: "Configuration".into(),
            severity: severity.into(),
            confidence: "firm".into(),
            status: "confirmed".into(),
            host: "lab.test".into(),
            url: "https://lab.test/admin".into(),
            description: "Line one.\n\nLine <two> & more.".into(),
            remediation: "Fix *it*.".into(),
            notes: "Checked twice.".into(),
            evidence: vec![ReportEvidence {
                method: "GET".into(),
                url: "https://lab.test/admin".into(),
                status: Some(200),
                note: "shows the page".into(),
                request: message("GET /admin HTTP/1.1\r\nHost: lab.test\r\n\r\n"),
                response: Some(message("HTTP/1.1 200 OK\r\n\r\n<script>alert(1)</script>")),
            }],
        };
        Report::new(
            ReportMeta {
                title: "Assessment <of> lab".into(),
                client: "Lab owner".into(),
                author: "Tester".into(),
                executive_summary: "Summary.".into(),
                methodology: "Manual review.".into(),
            },
            vec!["include: https://lab.test".into()],
            vec![
                finding(1, "low", "B low"),
                finding(2, "high", "A high"),
                finding(3, "low", "A low"),
            ],
            1_793_219_998_000,
        )
    }

    #[test]
    fn sorts_and_counts() {
        let r = sample();
        let titles: Vec<&str> = r.findings.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(titles, ["A high", "A low", "B low"]);
        assert_eq!(r.counts()[1], ("high", 1));
        assert_eq!(r.counts()[3], ("low", 2));
        assert_eq!(r.targets, ["lab.test"]);
        assert_eq!(format_time(1_793_219_998_000), "2026-10-28 20:39 UTC");
    }

    #[test]
    fn json_is_structured() {
        let v: serde_json::Value = serde_json::from_str(&sample().render_json()).unwrap();
        assert_eq!(v["format"], JSON_FORMAT);
        assert_eq!(v["findings"][0]["severity"], "high");
        assert_eq!(
            v["findings"][0]["evidence"][0]["response"]["text"],
            "HTTP/1.1 200 OK\r\n\r\n<script>alert(1)</script>"
        );
    }
}
