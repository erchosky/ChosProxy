//! Security analysis of HTTP traffic.
//!
//! The **passive checks** look at requests and responses that already
//! happened — proxy and Repeater traffic — and report what a tester should
//! look at: missing protections, information leaks, risky configurations.
//! They never send anything and never change traffic (spec §22). Each issue
//! carries an explanation of the problem and how to fix it, written for
//! someone learning web security.
//!
//! Most checks are pure functions of one request/response pair; the
//! cross-request checks ([`CrossAnalyzer`]) follow the traffic in order.
//! Repeated detections are merged by the caller through [`Issue::dedup`].

pub mod active;
mod checks;
pub mod cross;
pub mod fingerprint;
pub mod insertion;
pub mod javascript;
mod jwt;
mod message;

pub use active::{
    ACTIVE_CHECKS, ActiveCheckInfo, ActiveFinding, PlannedProbe, ProbeObservation, active_check,
    evaluate_point_probes, evaluate_request_probes, into_issue, plan_point_probes,
    plan_request_probes,
};
pub use checks::{CHECKS, analyze};
pub use cross::{CROSS_CHECKS, CrossAnalyzer};
pub use insertion::{InsertionPoint, PointEncoding, PointKind, insertion_points};
pub use message::Message;

use serde::Serialize;
use ts_rs::TS;

/// How serious an issue is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "PassiveSeverity")]
pub enum Severity {
    /// Worth knowing; rarely a problem alone.
    Info,
    /// Weakens defences.
    Low,
    /// Exploitable in some conditions.
    Medium,
    /// Likely exploitable.
    High,
}

/// How sure a check is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "PassiveConfidence")]
pub enum Confidence {
    /// A hint that needs manual confirmation.
    Tentative,
    /// Very likely right.
    Firm,
    /// Read directly from the message.
    Certain,
}

/// A passive check, as listed in the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "PassiveCheck")]
pub struct CheckInfo {
    /// Stable identifier (settings, dedup keys).
    pub id: &'static str,
    /// Issue type shown on its findings.
    pub kind: &'static str,
    /// What the check looks for, in one sentence.
    pub summary: &'static str,
    /// Highest severity it reports.
    pub severity: Severity,
    /// What the problem is and why it matters (Markdown-free text). Shown
    /// on findings and in the issue definitions catalogue.
    pub description: &'static str,
    /// How to fix it.
    pub remediation: &'static str,
}

/// One detection in one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// The check that found it.
    pub check: &'static CheckInfo,
    /// Title, naming the specific thing (cookie, header, parameter…).
    pub title: String,
    /// Severity of this detection (may be lower than the check's highest).
    pub severity: Severity,
    /// Confidence of this detection.
    pub confidence: Confidence,
    /// What in this message shows it (a header, a snippet…), short.
    pub evidence: String,
    /// What makes two detections the same issue, e.g. the origin and the
    /// cookie name. Combined by the caller with the check id.
    pub dedup: String,
}
