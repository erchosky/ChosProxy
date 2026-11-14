//! Active audit checks (M13, spec §23, `SCANNER-002`, `SCANNER-004`,
//! `SCANNER-005`, `SCANNER-011`, `OAST-001`).
//!
//! Every check in this module is a pure, deterministic verification check:
//! - Probe generation is separated from network I/O (`plan_point_probes`,
//!   `plan_request_probes`).
//! - Evidence evaluation (`evaluate_point_probes`, `evaluate_request_probes`)
//!   compares the baseline response against controlled probe responses.
//! - Probes are non-destructive verification markers and boolean comparisons
//!   only: no shellcode, no data extraction, no credential lists, no DoS.

use crate::insertion::{InsertionPoint, PointKind, replace};
use crate::message::Message;
use crate::{Confidence, Issue, Severity};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Benign alphanumeric marker used to detect reflected input without side effects.
pub const XSS_MARKER: &str = "chosprobe7x9k";

/// Context-breaking characters appended after [`XSS_MARKER`] when the marker
/// is reflected in an HTML-capable response.
pub const XSS_CONTEXT_PROBE: &str = "chosprobe7x9k\"<>'";

/// Benign external target used to verify unvalidated redirects without
/// following out-of-scope redirects.
pub const REDIRECT_PROBE_URL: &str = "https://chosprobe.example/redirect-check";

/// Benign external origin used to verify CORS reflection.
pub const CORS_PROBE_ORIGIN: &str = "https://chosprobe.example";

/// Metadata for a built-in active check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ActiveCheck")]
pub struct ActiveCheckInfo {
    /// Stable identifier (e.g. `"active-reflected-xss"`).
    pub id: &'static str,
    /// Issue type shown on findings.
    pub kind: &'static str,
    /// One-line summary.
    pub summary: &'static str,
    /// Highest severity reported by this check.
    pub severity: Severity,
    /// Typical confidence of findings from this check.
    pub confidence: Confidence,
    /// Compatible insertion point kinds (empty for request-level checks such as CORS).
    pub point_kinds: &'static [PointKind],
    /// Maximum probes sent per insertion point (or per request when `point_kinds` is empty).
    pub max_requests_per_point: u32,
    /// True when the check requires ChosOAST to be enabled.
    pub requires_oast: bool,
    /// CWE reference.
    pub cwe: &'static str,
    /// OWASP category reference.
    pub owasp: &'static str,
    /// Explanation of the weakness.
    pub description: &'static str,
    /// Remediation guidance.
    pub remediation: &'static str,
}

const XSS_POINTS: &[PointKind] = &[
    PointKind::Query,
    PointKind::Form,
    PointKind::PathSegment,
    PointKind::Json,
    PointKind::Cookie,
    PointKind::Header,
];

const SQLI_POINTS: &[PointKind] = &[
    PointKind::Query,
    PointKind::Form,
    PointKind::PathSegment,
    PointKind::Json,
    PointKind::Cookie,
];

const REDIRECT_POINTS: &[PointKind] = &[PointKind::Query, PointKind::Form];

const OAST_POINTS: &[PointKind] = &[
    PointKind::Query,
    PointKind::Form,
    PointKind::Json,
    PointKind::Header,
    PointKind::Xml,
];

/// Built-in active checks catalog.
pub const ACTIVE_CHECKS: &[ActiveCheckInfo] = &[
    ActiveCheckInfo {
        id: "active-reflected-xss",
        kind: "Reflected Cross-Site Scripting (Active)",
        summary: "Probes parameters with a benign marker and checks whether HTML/script characters are reflected unescaped.",
        severity: Severity::High,
        confidence: Confidence::Tentative,
        point_kinds: XSS_POINTS,
        max_requests_per_point: 2,
        requires_oast: false,
        cwe: "CWE-79",
        owasp: "A03:2021 Injection",
        description: "The application reflects user-supplied input into an HTML response without context-appropriate encoding. Because HTTP-level active probing does not execute script in a browser, findings are reported with Tentative confidence alongside the detected reflection context.",
        remediation: "Encode untrusted output according to the HTML context (element text, attribute value, JavaScript string, or URL) and apply a strict Content-Security-Policy.",
    },
    ActiveCheckInfo {
        id: "active-sqli",
        kind: "SQL Injection (Active)",
        summary: "Compares controlled boolean conditions and checks for corroborated database syntax errors without extracting data.",
        severity: Severity::High,
        confidence: Confidence::Firm,
        point_kinds: SQLI_POINTS,
        max_requests_per_point: 3,
        requires_oast: false,
        cwe: "CWE-89",
        owasp: "A03:2021 Injection",
        description: "User input appears to be concatenated directly into a database query: either a controlled syntax probe triggered a database parser error not present in the baseline, or true/false boolean conditions produced a repeatable response differential.",
        remediation: "Use parameterized queries (prepared statements) for all database access and never concatenate untrusted input into SQL strings.",
    },
    ActiveCheckInfo {
        id: "active-open-redirect",
        kind: "Open Redirect (Active)",
        summary: "Checks whether redirect parameters can point the Location header to an external origin.",
        severity: Severity::Medium,
        confidence: Confidence::Certain,
        point_kinds: REDIRECT_POINTS,
        max_requests_per_point: 1,
        requires_oast: false,
        cwe: "CWE-601",
        owasp: "A01:2021 Broken Access Control",
        description: "The endpoint accepts a user-controlled URL parameter and returns a 3xx redirect whose Location header points to an arbitrary external domain.",
        remediation: "Allow only relative paths (starting with a single '/' not followed by '/' or '\\') or validate redirect targets against a strict allowlist of trusted origins.",
    },
    ActiveCheckInfo {
        id: "active-cors",
        kind: "CORS Misconfiguration (Active)",
        summary: "Probes the endpoint with an external Origin header and null Origin to check for unsafe Access-Control-Allow-Origin reflection.",
        severity: Severity::High,
        confidence: Confidence::Certain,
        point_kinds: &[],
        max_requests_per_point: 2,
        requires_oast: false,
        cwe: "CWE-942",
        owasp: "A05:2021 Security Misconfiguration",
        description: "The server dynamically reflects an arbitrary untrusted Origin (or the 'null' origin) in Access-Control-Allow-Origin, potentially exposing authenticated responses to third-party sites when combined with Access-Control-Allow-Credentials: true.",
        remediation: "Validate the Origin header against an explicit allowlist of trusted origins and never reflect arbitrary origins or 'null' with Access-Control-Allow-Credentials: true.",
    },
    ActiveCheckInfo {
        id: "active-oast",
        kind: "Out-of-Band Resource Interaction (OAST)",
        summary: "Injects a correlated ChosOAST callback URL to detect blind server-side interactions (DNS/HTTP).",
        severity: Severity::High,
        confidence: Confidence::Certain,
        point_kinds: OAST_POINTS,
        max_requests_per_point: 1,
        requires_oast: true,
        cwe: "CWE-918",
        owasp: "A10:2021 Server-Side Request Forgery",
        description: "The target server made an out-of-band network interaction (DNS or HTTP) to a unique ChosOAST correlation address supplied in a request parameter.",
        remediation: "Restrict outbound network access from application servers, validate and allowlist user-supplied URLs or hosts, and disable external entity/resource resolution.",
    },
];

/// A planned probe request generated by an active check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedProbe {
    /// Check id that planned this probe.
    pub check_id: &'static str,
    /// Short label for logs and notes (`"xss-marker"`, `"sqli-true"`, etc.).
    pub label: &'static str,
    /// Payload value placed into the insertion point (or header value).
    pub payload: String,
    /// Full modified request text (`\r\n` or `\n` framed; caller normalizes via `request_bytes`).
    pub request_text: String,
}

/// Completed probe observation passed back to the pure evaluator.
#[derive(Debug)]
pub struct ProbeObservation<'a> {
    /// Label matching [`PlannedProbe::label`].
    pub label: &'static str,
    /// Payload sent.
    pub payload: &'a str,
    /// Parsed message of the probe exchange.
    pub message: &'a Message,
}

/// An active finding produced by evaluating probes against a baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveFinding {
    /// Check metadata.
    pub check: &'static ActiveCheckInfo,
    /// Finding title.
    pub title: String,
    /// Severity.
    pub severity: Severity,
    /// Confidence.
    pub confidence: Confidence,
    /// Evidence summary naming the insertion point, payload, and response proof.
    pub evidence: String,
    /// Deduplication key (`check_id|origin|path|point_name`).
    pub dedup: String,
    /// Index of the probe in `observations` that proved the finding.
    pub probe_index: usize,
}

/// Lookup a built-in active check by id.
pub fn active_check(id: &str) -> Option<&'static ActiveCheckInfo> {
    ACTIVE_CHECKS.iter().find(|c| c.id == id)
}

/// Plans the probes for `check_id` at `point` on `base_request_text`,
/// respecting `budget` (at most `budget` probes returned).
pub fn plan_point_probes(
    check_id: &str,
    base_request_text: &str,
    point: &InsertionPoint,
    budget: u32,
    oast_url: Option<&str>,
) -> Vec<PlannedProbe> {
    if budget == 0 {
        return Vec::new();
    }
    let Some(info) = active_check(check_id) else {
        return Vec::new();
    };
    if !info.point_kinds.contains(&point.kind) {
        return Vec::new();
    }
    let max = budget.min(info.max_requests_per_point) as usize;
    let mut out = Vec::new();
    match check_id {
        "active-reflected-xss" => {
            let p1 = format!("{}{XSS_MARKER}", point.value);
            out.push(PlannedProbe {
                check_id: info.id,
                label: "xss-marker",
                request_text: replace(base_request_text, point, &p1),
                payload: p1,
            });
            if max >= 2 {
                let p2 = format!("{}{XSS_CONTEXT_PROBE}", point.value);
                out.push(PlannedProbe {
                    check_id: info.id,
                    label: "xss-context",
                    request_text: replace(base_request_text, point, &p2),
                    payload: p2,
                });
            }
        }
        "active-sqli" => {
            let is_numeric =
                !point.value.is_empty() && point.value.bytes().all(|b| b.is_ascii_digit());
            let (t_payload, f_payload, ctrl_payload) = if is_numeric {
                (
                    format!("{} OR 1=1", point.value),
                    format!("{} AND 1=2", point.value),
                    format!("{}'", point.value),
                )
            } else {
                (
                    format!("{}' OR '1'='1", point.value),
                    format!("{}' AND '1'='2", point.value),
                    format!("{}'\"\"", point.value),
                )
            };
            let candidates = [
                ("sqli-true", t_payload),
                ("sqli-false", f_payload),
                ("sqli-syntax", ctrl_payload),
            ];
            for (label, payload) in candidates.into_iter().take(max) {
                out.push(PlannedProbe {
                    check_id: info.id,
                    label,
                    request_text: replace(base_request_text, point, &payload),
                    payload,
                });
            }
        }
        "active-open-redirect" => {
            let name_lc = point.name.to_ascii_lowercase();
            let looks_like_redirect = [
                "url",
                "redirect",
                "next",
                "to",
                "return",
                "dest",
                "destination",
                "goto",
                "target",
                "continue",
                "uri",
                "callback",
                "out",
                "view",
            ]
            .iter()
            .any(|k| name_lc.contains(k))
                || point.value.starts_with('/')
                || point.value.starts_with("http://")
                || point.value.starts_with("https://");
            if looks_like_redirect {
                let payload = REDIRECT_PROBE_URL.to_string();
                out.push(PlannedProbe {
                    check_id: info.id,
                    label: "open-redirect",
                    request_text: replace(base_request_text, point, &payload),
                    payload,
                });
            }
        }
        "active-oast" => {
            if let Some(url) = oast_url {
                let payload = url.to_string();
                out.push(PlannedProbe {
                    check_id: info.id,
                    label: "oast-callback",
                    request_text: replace(base_request_text, point, &payload),
                    payload,
                });
            }
        }
        _ => {}
    }
    out.truncate(max);
    out
}

/// Plans request-level probes (such as `active-cors`) that modify headers on
/// the base request rather than a parameter insertion point.
pub fn plan_request_probes(
    check_id: &str,
    base_request_text: &str,
    budget: u32,
) -> Vec<PlannedProbe> {
    if budget == 0 || check_id != "active-cors" {
        return Vec::new();
    }
    let Some(info) = active_check("active-cors") else {
        return Vec::new();
    };
    let max = budget.min(info.max_requests_per_point) as usize;
    let mut out = Vec::new();
    let origins = [("cors-external", CORS_PROBE_ORIGIN), ("cors-null", "null")];
    for (label, origin) in origins.into_iter().take(max) {
        out.push(PlannedProbe {
            check_id: info.id,
            label,
            payload: origin.to_string(),
            request_text: set_or_replace_header(base_request_text, "Origin", origin),
        });
    }
    out
}

fn set_or_replace_header(request_text: &str, header_name: &str, value: &str) -> String {
    let sep = if request_text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let split_pat = if sep == "\r\n" { "\r\n\r\n" } else { "\n\n" };
    let (head, body) = match request_text.split_once(split_pat) {
        Some((h, b)) => (h, Some(b)),
        None => (request_text, None),
    };
    let mut lines: Vec<String> = head.lines().map(str::to_string).collect();
    let prefix = format!("{}:", header_name.to_ascii_lowercase());
    let mut replaced = false;
    for line in lines.iter_mut().skip(1) {
        if line.to_ascii_lowercase().starts_with(&prefix) {
            *line = format!("{header_name}: {value}");
            replaced = true;
            break;
        }
    }
    if !replaced {
        lines.push(format!("{header_name}: {value}"));
    }
    let new_head = lines.join(sep);
    match body {
        Some(b) => format!("{new_head}{split_pat}{b}"),
        None => format!("{new_head}{split_pat}"),
    }
}

/// Evaluates completed point probes against the `baseline` exchange.
pub fn evaluate_point_probes(
    check_id: &str,
    baseline: &Message,
    point: &InsertionPoint,
    observations: &[ProbeObservation<'_>],
) -> Option<ActiveFinding> {
    let info = active_check(check_id)?;
    match check_id {
        "active-reflected-xss" => evaluate_xss(info, baseline, point, observations),
        "active-sqli" => evaluate_sqli(info, baseline, point, observations),
        "active-open-redirect" => evaluate_open_redirect(info, baseline, point, observations),
        _ => None,
    }
}

/// Evaluates request-level probes (such as `active-cors`) against `baseline`.
pub fn evaluate_request_probes(
    check_id: &str,
    baseline: &Message,
    observations: &[ProbeObservation<'_>],
) -> Option<ActiveFinding> {
    let info = active_check(check_id)?;
    match check_id {
        "active-cors" => evaluate_cors(info, baseline, observations),
        _ => None,
    }
}

fn is_html_like_response(msg: &Message) -> bool {
    if msg.response.is_none() {
        return false;
    }
    let ct = msg
        .response_header("content-type")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ct.contains("text/html") || ct.contains("application/xhtml") || ct.contains("image/svg") {
        return true;
    }
    if ct.is_empty() {
        let trimmed = msg.body.trim_start().to_ascii_lowercase();
        return trimmed.starts_with("<!doctype html") || trimmed.starts_with("<html");
    }
    false
}

fn evaluate_xss(
    info: &'static ActiveCheckInfo,
    baseline: &Message,
    point: &InsertionPoint,
    observations: &[ProbeObservation<'_>],
) -> Option<ActiveFinding> {
    let origin = baseline.origin();
    let path = baseline.path_only();
    let dedup = format!("{}|{origin}{path}|{}", info.id, point.name);

    // Check context-breaking probe first if present.
    if let Some((idx, obs)) = observations
        .iter()
        .enumerate()
        .find(|(_, o)| o.label == "xss-context")
    {
        if !is_html_like_response(obs.message) {
            return None;
        }
        let body = &obs.message.body;
        if body.contains(XSS_CONTEXT_PROBE) {
            let in_script = is_inside_script_tag(body, XSS_CONTEXT_PROBE);
            return Some(ActiveFinding {
                check: info,
                title: format!(
                    "Unescaped input reflection in {} '{}' on {path}",
                    point_kind_label(point.kind),
                    point.name
                ),
                severity: Severity::High,
                confidence: Confidence::Tentative,
                evidence: format!(
                    "Parameter '{}' reflected unescaped HTML/script characters ({}) in {} response (tentative: browser execution not verified).",
                    point.name,
                    XSS_CONTEXT_PROBE,
                    if in_script {
                        "a <script> block of the"
                    } else {
                        "the"
                    }
                ),
                dedup,
                probe_index: idx,
            });
        }
        // Check unescaped attribute breakout (`chosprobe7x9k"` inside a quoted attribute)
        let unescaped_quote_marker = format!("{XSS_MARKER}\"");
        let unescaped_tag_marker = format!("{XSS_MARKER}\"<>");
        if body.contains(&unescaped_tag_marker)
            || (body.contains(&unescaped_quote_marker)
                && !body.contains(&format!("{XSS_MARKER}&quot;")))
        {
            return Some(ActiveFinding {
                check: info,
                title: format!(
                    "Unescaped input reflection in {} '{}' on {path}",
                    point_kind_label(point.kind),
                    point.name
                ),
                severity: Severity::High,
                confidence: Confidence::Tentative,
                evidence: format!(
                    "Parameter '{}' reflected unescaped quote/tag characters from '{}' in HTML response (tentative: browser execution not verified).",
                    point.name, obs.payload
                ),
                dedup,
                probe_index: idx,
            });
        }
        // Context probe was sent and properly escaped (`&lt;`, `&quot;`) -> not vulnerable.
        return None;
    }

    // If only the benign marker probe was sent (e.g. max_requests_per_point == 1),
    // report Tentative only when reflected inside a `<script>` block.
    if let Some((idx, obs)) = observations
        .iter()
        .enumerate()
        .find(|(_, o)| o.label == "xss-marker")
        && is_html_like_response(obs.message)
        && is_inside_script_tag(&obs.message.body, XSS_MARKER)
    {
        return Some(ActiveFinding {
            check: info,
            title: format!(
                "Reflected input in script context ('{}') on {path}",
                point.name
            ),
            severity: Severity::Medium,
            confidence: Confidence::Tentative,
            evidence: format!(
                "Parameter '{}' reflected marker '{}' directly inside a <script> element.",
                point.name, XSS_MARKER
            ),
            dedup,
            probe_index: idx,
        });
    }
    None
}

fn is_inside_script_tag(html: &str, marker: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    let marker_lower = marker.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find(&marker_lower) {
        let pos = search_from + rel;
        let before = &lower[..pos];
        if let Some(open_script) = before.rfind("<script") {
            let after_open = &before[open_script..];
            if !after_open.contains("</script") {
                return true;
            }
        }
        search_from = pos + marker_lower.len();
    }
    false
}

const SQL_ERROR_PATTERNS: &[(&str, &str)] = &[
    ("SQLITE_ERROR", "SQLite"),
    ("near \"", "SQLite syntax error"),
    ("unrecognized token:", "SQLite"),
    ("You have an error in your SQL syntax", "MySQL/MariaDB"),
    ("Warning: mysql_", "MySQL"),
    ("PG::SyntaxError", "PostgreSQL"),
    ("ERROR:  syntax error at or near", "PostgreSQL"),
    ("ERROR: syntax error at or near", "PostgreSQL"),
    ("unterminated quoted string at or near", "PostgreSQL"),
    ("ORA-00933", "Oracle"),
    ("ORA-01756", "Oracle"),
    (
        "Unclosed quotation mark after the character string",
        "Microsoft SQL Server",
    ),
    (
        "Microsoft OLE DB Provider for SQL Server",
        "Microsoft SQL Server",
    ),
];

fn detect_sql_error(body: &str) -> Option<(&'static str, &'static str)> {
    for &(pattern, db_name) in SQL_ERROR_PATTERNS {
        if body.contains(pattern) {
            return Some((pattern, db_name));
        }
    }
    None
}

fn evaluate_sqli(
    info: &'static ActiveCheckInfo,
    baseline: &Message,
    point: &InsertionPoint,
    observations: &[ProbeObservation<'_>],
) -> Option<ActiveFinding> {
    let origin = baseline.origin();
    let path = baseline.path_only();
    let dedup = format!("{}|{origin}{path}|{}", info.id, point.name);

    // 1. Corroborated SQL error signature not present in the baseline response.
    if detect_sql_error(&baseline.body).is_none() {
        for (idx, obs) in observations.iter().enumerate() {
            if let Some((pattern, db_name)) = detect_sql_error(&obs.message.body) {
                return Some(ActiveFinding {
                    check: info,
                    title: format!(
                        "SQL Injection ({db_name} error) in '{}' on {path}",
                        point.name
                    ),
                    severity: Severity::High,
                    confidence: Confidence::Firm,
                    evidence: format!(
                        "Probe '{}' in parameter '{}' triggered a {db_name} error signature ('{pattern}') not present in the baseline response.",
                        obs.payload, point.name
                    ),
                    dedup,
                    probe_index: idx,
                });
            }
        }
    }

    // 2. Controlled boolean differential: requires both sqli-true and sqli-false,
    // plus sqli-syntax as a negative control.
    let base_res = baseline.response.as_ref()?;
    if !(200..300).contains(&base_res.status) {
        return None;
    }
    let (t_idx, t_obs) = observations
        .iter()
        .enumerate()
        .find(|(_, o)| o.label == "sqli-true")?;
    let (_, f_obs) = observations
        .iter()
        .enumerate()
        .find(|(_, o)| o.label == "sqli-false")?;
    let t_res = t_obs.message.response.as_ref()?;
    let f_res = f_obs.message.response.as_ref()?;

    // Strip the reflected payload itself before comparing body lengths/structure so
    // ordinary echo/search pages are not mistaken for boolean SQL differentials.
    let base_norm = normalize_for_diff(&baseline.body, &point.value);
    let t_norm = normalize_for_diff(&t_obs.message.body, t_obs.payload);
    let f_norm = normalize_for_diff(&f_obs.message.body, f_obs.payload);

    // If a negative control probe (`sqli-syntax`) was sent and returned the exact same
    // response as `sqli-true` AND `sqli-false` only differed slightly, or if all three
    // responses differ wildly (unstable/dynamic page), do not report.
    if let Some((_, s_obs)) = observations
        .iter()
        .enumerate()
        .find(|(_, o)| o.label == "sqli-syntax")
    {
        let s_norm = normalize_for_diff(&s_obs.message.body, s_obs.payload);
        // Dynamic page check: if baseline, true, false, and syntax all have distinct dynamic content
        // where true does not match baseline and does not clearly exceed false, skip.
        if t_norm == f_norm || t_norm == s_norm {
            return None;
        }
        // Verify that the negative control (broken quote syntax) behaved like the false branch
        // or returned an error status, whereas the true branch succeeded (200) with substantially
        // more content or matched the baseline record set.
        let s_status = s_obs.message.status();
        let status_diff = t_res.status == 200 && f_res.status != 200 && s_status != 200;
        let body_diff = t_res.status == 200
            && f_res.status == 200
            && (t_norm == base_norm || t_norm.len() > f_norm.len() + 20)
            && f_norm == s_norm
            && base_norm != f_norm;
        if status_diff || body_diff {
            return Some(ActiveFinding {
                check: info,
                title: format!("Boolean SQL Injection in '{}' on {path}", point.name),
                severity: Severity::High,
                confidence: Confidence::Firm,
                evidence: format!(
                    "Boolean true probe ('{}') returned status {} ({} bytes normalized) while false probe ('{}') and syntax control returned status {} ({} bytes normalized).",
                    t_obs.payload,
                    t_res.status,
                    t_norm.len(),
                    f_obs.payload,
                    f_res.status,
                    f_norm.len()
                ),
                dedup,
                probe_index: t_idx,
            });
        }
    }
    None
}

fn normalize_for_diff(body: &str, payload: &str) -> String {
    let without_payload = if payload.is_empty() {
        body.to_string()
    } else {
        body.replace(payload, "")
    };
    without_payload
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn evaluate_open_redirect(
    info: &'static ActiveCheckInfo,
    baseline: &Message,
    point: &InsertionPoint,
    observations: &[ProbeObservation<'_>],
) -> Option<ActiveFinding> {
    let origin = baseline.origin();
    let path = baseline.path_only();
    let dedup = format!("{}|{origin}{path}|{}", info.id, point.name);

    for (idx, obs) in observations.iter().enumerate() {
        let status = obs.message.status();
        if !(300..400).contains(&status) {
            continue;
        }
        let Some(location) = obs.message.response_header("location") else {
            continue;
        };
        let loc_trimmed = location.trim();
        let redirects_to_probe = redirects_to_probe_host(loc_trimmed);
        if redirects_to_probe {
            return Some(ActiveFinding {
                check: info,
                title: format!("Open redirect via '{}' on {path}", point.name),
                severity: Severity::Medium,
                confidence: Confidence::Certain,
                evidence: format!(
                    "Supplying '{}' in '{}' caused a {status} redirect with Location: {loc_trimmed}.",
                    obs.payload, point.name
                ),
                dedup,
                probe_index: idx,
            });
        }
    }
    None
}

fn redirects_to_probe_host(location: &str) -> bool {
    let lower = location.trim().to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("https://") {
        r
    } else if let Some(r) = lower.strip_prefix("http://") {
        r
    } else if let Some(r) = lower.strip_prefix("//") {
        r
    } else {
        return false;
    };
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host_port
        .rsplit_once('@')
        .map_or(host_port, |(_, h)| h)
        .split(':')
        .next()
        .unwrap_or_default();
    host == "chosprobe.example"
}

fn evaluate_cors(
    info: &'static ActiveCheckInfo,
    baseline: &Message,
    observations: &[ProbeObservation<'_>],
) -> Option<ActiveFinding> {
    let origin = baseline.origin();
    let path = baseline.path_only();
    let dedup = format!("{}|{origin}{path}", info.id);

    for (idx, obs) in observations.iter().enumerate() {
        if obs.message.response.is_none() {
            continue;
        }
        let Some(acao) = obs.message.response_header("access-control-allow-origin") else {
            continue;
        };
        let acao = acao.trim();
        let acac = obs
            .message
            .response_header("access-control-allow-credentials")
            .unwrap_or_default()
            .trim()
            .eq_ignore_ascii_case("true");
        if obs.label == "cors-external" && acao.eq_ignore_ascii_case(CORS_PROBE_ORIGIN) {
            let severity = if acac { Severity::High } else { Severity::Low };
            return Some(ActiveFinding {
                check: info,
                title: format!("Arbitrary Origin reflected in CORS header on {path}"),
                severity,
                confidence: Confidence::Certain,
                evidence: format!(
                    "Sending 'Origin: {CORS_PROBE_ORIGIN}' returned 'Access-Control-Allow-Origin: {acao}'{}.",
                    if acac {
                        " with 'Access-Control-Allow-Credentials: true'"
                    } else {
                        ""
                    }
                ),
                dedup,
                probe_index: idx,
            });
        }
        if obs.label == "cors-null" && acao.eq_ignore_ascii_case("null") && acac {
            return Some(ActiveFinding {
                check: info,
                title: format!("Null Origin allowed with credentials in CORS header on {path}"),
                severity: Severity::High,
                confidence: Confidence::Certain,
                evidence: "Sending 'Origin: null' returned 'Access-Control-Allow-Origin: null' with 'Access-Control-Allow-Credentials: true'.".into(),
                dedup,
                probe_index: idx,
            });
        }
    }
    None
}

fn point_kind_label(kind: PointKind) -> &'static str {
    match kind {
        PointKind::PathSegment => "path segment",
        PointKind::Query => "query parameter",
        PointKind::Form => "form parameter",
        PointKind::Cookie => "cookie",
        PointKind::Header => "header",
        PointKind::Json => "JSON field",
        PointKind::Multipart => "multipart field",
        PointKind::Xml => "XML node",
    }
}

/// Converts an [`ActiveFinding`] into a scanner [`Issue`] (using a static [`CheckInfo`] view).
pub fn into_issue(finding: &ActiveFinding) -> Issue {
    let check_info = match finding.check.id {
        "active-reflected-xss" => &ACTIVE_XSS_CHECK_INFO,
        "active-sqli" => &ACTIVE_SQLI_CHECK_INFO,
        "active-open-redirect" => &ACTIVE_REDIRECT_CHECK_INFO,
        "active-cors" => &ACTIVE_CORS_CHECK_INFO,
        _ => &ACTIVE_OAST_CHECK_INFO,
    };
    Issue {
        check: check_info,
        title: finding.title.clone(),
        severity: finding.severity,
        confidence: finding.confidence,
        evidence: finding.evidence.clone(),
        dedup: finding.dedup.clone(),
    }
}

const ACTIVE_XSS_CHECK_INFO: crate::CheckInfo = crate::CheckInfo {
    id: ACTIVE_CHECKS[0].id,
    kind: ACTIVE_CHECKS[0].kind,
    summary: ACTIVE_CHECKS[0].summary,
    severity: ACTIVE_CHECKS[0].severity,
    description: ACTIVE_CHECKS[0].description,
    remediation: ACTIVE_CHECKS[0].remediation,
};

const ACTIVE_SQLI_CHECK_INFO: crate::CheckInfo = crate::CheckInfo {
    id: ACTIVE_CHECKS[1].id,
    kind: ACTIVE_CHECKS[1].kind,
    summary: ACTIVE_CHECKS[1].summary,
    severity: ACTIVE_CHECKS[1].severity,
    description: ACTIVE_CHECKS[1].description,
    remediation: ACTIVE_CHECKS[1].remediation,
};

const ACTIVE_REDIRECT_CHECK_INFO: crate::CheckInfo = crate::CheckInfo {
    id: ACTIVE_CHECKS[2].id,
    kind: ACTIVE_CHECKS[2].kind,
    summary: ACTIVE_CHECKS[2].summary,
    severity: ACTIVE_CHECKS[2].severity,
    description: ACTIVE_CHECKS[2].description,
    remediation: ACTIVE_CHECKS[2].remediation,
};

const ACTIVE_CORS_CHECK_INFO: crate::CheckInfo = crate::CheckInfo {
    id: ACTIVE_CHECKS[3].id,
    kind: ACTIVE_CHECKS[3].kind,
    summary: ACTIVE_CHECKS[3].summary,
    severity: ACTIVE_CHECKS[3].severity,
    description: ACTIVE_CHECKS[3].description,
    remediation: ACTIVE_CHECKS[3].remediation,
};

const ACTIVE_OAST_CHECK_INFO: crate::CheckInfo = crate::CheckInfo {
    id: ACTIVE_CHECKS[4].id,
    kind: ACTIVE_CHECKS[4].kind,
    summary: ACTIVE_CHECKS[4].summary,
    severity: ACTIVE_CHECKS[4].severity,
    description: ACTIVE_CHECKS[4].description,
    remediation: ACTIVE_CHECKS[4].remediation,
};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::insertion::insertion_points;

    fn make_msg(req_raw: &[u8], res_head: &[u8], body: &str) -> Message {
        let mut full_res = res_head.to_vec();
        full_res.extend_from_slice(body.as_bytes());
        Message::new(
            "http",
            "lab.test",
            8080,
            "/search?q=hello",
            req_raw,
            Some(&full_res),
        )
    }

    #[test]
    fn xss_detects_unescaped_html_and_ignores_escaped_or_json() {
        let req_text = "GET /search?q=hello HTTP/1.1\r\nHost: lab.test:8080\r\n\r\n";
        let pts = insertion_points(req_text, &[PointKind::Query]);
        let pt = &pts[0];
        let planned = plan_point_probes("active-reflected-xss", req_text, pt, 2, None);
        assert_eq!(planned.len(), 2);

        let base_res = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n";
        let m_base = make_msg(req_text.as_bytes(), base_res, "<p>hello</p>");

        // 1. Unescaped reflection -> High / Tentative (no browser execution)
        let unescaped_body = format!("<p>Result: {}</p>", planned[1].payload);
        let m_p2 = make_msg(req_text.as_bytes(), base_res, &unescaped_body);
        let obs = [ProbeObservation {
            label: "xss-context",
            payload: &planned[1].payload,
            message: &m_p2,
        }];
        let finding = evaluate_point_probes("active-reflected-xss", &m_base, pt, &obs).unwrap();
        assert_eq!(finding.severity, Severity::High);
        assert_eq!(finding.confidence, Confidence::Tentative);

        // 2. HTML-escaped reflection -> None
        let escaped_body = "<p>Result: hellochosprobe7x9k&quot;&lt;&gt;&#x27;</p>";
        let m_esc = make_msg(req_text.as_bytes(), base_res, escaped_body);
        let obs_esc = [ProbeObservation {
            label: "xss-context",
            payload: &planned[1].payload,
            message: &m_esc,
        }];
        assert!(evaluate_point_probes("active-reflected-xss", &m_base, pt, &obs_esc).is_none());

        // 3. JSON Content-Type with raw characters -> None (not HTML)
        let json_res = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n";
        let m_json = make_msg(req_text.as_bytes(), json_res, &unescaped_body);
        let obs_json = [ProbeObservation {
            label: "xss-context",
            payload: &planned[1].payload,
            message: &m_json,
        }];
        assert!(evaluate_point_probes("active-reflected-xss", &m_base, pt, &obs_json).is_none());
    }

    #[test]
    fn sqli_detects_boolean_diff_and_sql_errors_but_ignores_generic_500() {
        let req_text = "GET /items?id=1 HTTP/1.1\r\nHost: lab.test:8080\r\n\r\n";
        let pts = insertion_points(req_text, &[PointKind::Query]);
        let pt = &pts[0];
        let planned = plan_point_probes("active-sqli", req_text, pt, 3, None);
        assert_eq!(planned.len(), 3);

        let res_ok = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n";
        let res_500 = b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\n\r\n";

        let m_base = make_msg(
            req_text.as_bytes(),
            res_ok,
            r#"[{"id":1,"name":"Widget Alpha"},{"id":2,"name":"Widget Beta"}]"#,
        );

        // Generic 500 without SQL error signature -> None
        let m_500 = make_msg(req_text.as_bytes(), res_500, "Internal Server Error");
        let obs_500 = [ProbeObservation {
            label: "sqli-syntax",
            payload: &planned[2].payload,
            message: &m_500,
        }];
        assert!(evaluate_point_probes("active-sqli", &m_base, pt, &obs_500).is_none());

        // Corroborated SQLite error -> Firm finding
        let m_sqlerr = make_msg(
            req_text.as_bytes(),
            res_500,
            "SQLITE_ERROR: near \"'\": syntax error",
        );
        let obs_err = [ProbeObservation {
            label: "sqli-syntax",
            payload: &planned[2].payload,
            message: &m_sqlerr,
        }];
        let f_err = evaluate_point_probes("active-sqli", &m_base, pt, &obs_err).unwrap();
        assert_eq!(f_err.severity, Severity::High);
        assert!(f_err.title.contains("SQLite"));

        // Controlled boolean differential -> Firm finding
        let m_t = make_msg(
            req_text.as_bytes(),
            res_ok,
            r#"[{"id":1,"name":"Widget Alpha"},{"id":2,"name":"Widget Beta"}]"#,
        );
        let m_f = make_msg(req_text.as_bytes(), res_ok, "[]");
        let m_s = make_msg(req_text.as_bytes(), res_ok, "[]");
        let obs_bool = [
            ProbeObservation {
                label: "sqli-true",
                payload: &planned[0].payload,
                message: &m_t,
            },
            ProbeObservation {
                label: "sqli-false",
                payload: &planned[1].payload,
                message: &m_f,
            },
            ProbeObservation {
                label: "sqli-syntax",
                payload: &planned[2].payload,
                message: &m_s,
            },
        ];
        let f_bool = evaluate_point_probes("active-sqli", &m_base, pt, &obs_bool).unwrap();
        assert_eq!(f_bool.severity, Severity::High);
        assert!(f_bool.title.contains("Boolean SQL Injection"));
    }

    #[test]
    fn open_redirect_and_cors_evaluation() {
        let req_text = "GET /redirect?to=/home HTTP/1.1\r\nHost: lab.test:8080\r\n\r\n";
        let pts = insertion_points(req_text, &[PointKind::Query]);
        let pt = &pts[0];
        let planned = plan_point_probes("active-open-redirect", req_text, pt, 1, None);
        assert_eq!(planned.len(), 1);

        let res_base = b"HTTP/1.1 302 Found\r\nLocation: /home\r\n\r\n";
        let m_base = make_msg(req_text.as_bytes(), res_base, "");

        let res_redir =
            b"HTTP/1.1 302 Found\r\nLocation: https://chosprobe.example/redirect-check\r\n\r\n";
        let m_redir = make_msg(req_text.as_bytes(), res_redir, "");
        let obs = [ProbeObservation {
            label: "open-redirect",
            payload: &planned[0].payload,
            message: &m_redir,
        }];
        let f = evaluate_point_probes("active-open-redirect", &m_base, pt, &obs).unwrap();
        assert_eq!(f.severity, Severity::Medium);
        assert_eq!(f.confidence, Confidence::Certain);

        // CORS check
        let cors_probes = plan_request_probes("active-cors", req_text, 2);
        assert_eq!(cors_probes.len(), 2);
        let res_cors = b"HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: https://chosprobe.example\r\nAccess-Control-Allow-Credentials: true\r\n\r\n";
        let m_cors = make_msg(req_text.as_bytes(), res_cors, "{}");
        let cors_obs = [ProbeObservation {
            label: "cors-external",
            payload: &cors_probes[0].payload,
            message: &m_cors,
        }];
        let f_cors = evaluate_request_probes("active-cors", &m_base, &cors_obs).unwrap();
        assert_eq!(f_cors.severity, Severity::High);
        assert_eq!(f_cors.confidence, Confidence::Certain);
    }
}
