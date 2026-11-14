//! Self-contained HTML report. All project content is escaped; the page has
//! no scripts and a policy that blocks every external load.

use crate::{Report, ReportFinding, ReportMessage, format_time};
use std::fmt::Write as _;

/// Escapes text for HTML element content and attribute values.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Plain text as paragraphs (blank lines) with line breaks.
fn paragraphs(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let parts: Vec<String> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| format!("<p>{}</p>", escape(p).replace('\n', "<br>")))
        .collect();
    if parts.is_empty() {
        "<p class=\"empty\">—</p>".into()
    } else {
        parts.join("\n")
    }
}

const STYLE: &str = r#"
:root { color-scheme: light; --fg:#1b2430; --dim:#5b6676; --line:#d8dee6; --code:#f5f7fa; }
* { box-sizing: border-box; }
body { font: 14px/1.55 -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif; color: var(--fg); margin: 0 auto; max-width: 960px; padding: 32px 28px 64px; }
h1 { font-size: 26px; margin: 0 0 6px; }
h2 { font-size: 19px; margin: 36px 0 10px; padding-bottom: 6px; border-bottom: 2px solid var(--line); }
h3 { font-size: 16px; margin: 28px 0 8px; }
h4 { font-size: 13px; margin: 16px 0 6px; text-transform: uppercase; letter-spacing: .04em; color: var(--dim); }
.meta { color: var(--dim); margin: 0 0 4px; }
table { border-collapse: collapse; width: 100%; margin: 8px 0; }
th, td { text-align: left; padding: 6px 8px; border-bottom: 1px solid var(--line); vertical-align: top; }
th { font-size: 12px; color: var(--dim); text-transform: uppercase; }
.sev { display: inline-block; min-width: 68px; text-align: center; padding: 1px 8px; border-radius: 10px; font-size: 12px; font-weight: 700; color: #fff; text-transform: capitalize; }
.sev-critical { background: #7a1020; } .sev-high { background: #c0392b; } .sev-medium { background: #d68910; }
.sev-low { background: #2e86c1; } .sev-info { background: #7f8c8d; }
.finding { border: 1px solid var(--line); border-radius: 8px; padding: 4px 18px 14px; margin: 18px 0; page-break-inside: avoid; }
.facts { color: var(--dim); font-size: 13px; }
.facts span { margin-right: 14px; }
pre { background: var(--code); border: 1px solid var(--line); border-radius: 6px; padding: 10px; font: 12px/1.45 ui-monospace, Menlo, Consolas, monospace; white-space: pre-wrap; word-break: break-all; margin: 6px 0; }
.note { font-size: 12px; color: var(--dim); }
.empty { color: var(--dim); }
.url { font-family: ui-monospace, Menlo, Consolas, monospace; font-size: 12px; word-break: break-all; }
@media print { body { padding: 0; } h2 { page-break-after: avoid; } }
"#;

fn message(label: &str, m: &ReportMessage) -> String {
    let mut flags = Vec::new();
    if m.redacted {
        flags.push("credentials redacted");
    }
    if m.truncated {
        flags.push("shortened");
    }
    let flags = if flags.is_empty() {
        String::new()
    } else {
        format!(" <span class=\"note\">({})</span>", flags.join(", "))
    };
    format!(
        "<h4>{}{flags}</h4>\n<pre>{}</pre>\n",
        escape(label),
        escape(&m.text)
    )
}

fn finding(out: &mut String, n: usize, f: &ReportFinding) {
    let _ = write!(
        out,
        "<section class=\"finding\" id=\"finding-{id}\">\n<h3>{n}. {title}</h3>\n\
         <p class=\"facts\"><span class=\"sev sev-{sev}\">{sev}</span> \
         <span>Confidence: {conf}</span><span>Status: {status}</span><span>Type: {kind}</span></p>\n\
         <p class=\"url\">{url}</p>\n<h4>Description</h4>\n{desc}\n<h4>Remediation</h4>\n{fix}\n",
        id = f.id,
        title = escape(&f.title),
        sev = escape(&f.severity),
        conf = escape(&f.confidence),
        status = escape(&f.status),
        kind = escape(&f.kind),
        url = escape(&f.url),
        desc = paragraphs(&f.description),
        fix = paragraphs(&f.remediation),
    );
    if !f.notes.trim().is_empty() {
        let _ = write!(
            out,
            "<h4>Technical details</h4>\n{}\n",
            paragraphs(&f.notes)
        );
    }
    for (i, e) in f.evidence.iter().enumerate() {
        let status = e.status.map(|s| format!(" → {s}")).unwrap_or_default();
        let _ = writeln!(
            out,
            "<h4>Evidence {} — {} <span class=\"url\">{}</span>{}</h4>",
            i + 1,
            escape(&e.method),
            escape(&e.url),
            escape(&status)
        );
        if !e.note.trim().is_empty() {
            let _ = writeln!(out, "<p class=\"note\">{}</p>", escape(&e.note));
        }
        out.push_str(&message("Request", &e.request));
        if let Some(r) = &e.response {
            out.push_str(&message("Response", r));
        }
    }
    out.push_str("</section>\n");
}

/// The report as one HTML document.
pub fn render_html(r: &Report) -> String {
    let mut out = String::with_capacity(64 * 1024);
    let title = if r.meta.title.trim().is_empty() {
        "Security assessment report".to_string()
    } else {
        r.meta.title.clone()
    };
    let _ = write!(
        out,
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">\n\
         <meta name=\"referrer\" content=\"no-referrer\">\n<title>{t}</title>\n<style>{STYLE}</style>\n</head><body>\n\
         <h1>{t}</h1>\n",
        t = escape(&title)
    );
    if !r.meta.client.trim().is_empty() {
        let _ = writeln!(
            out,
            "<p class=\"meta\">Prepared for: {}</p>",
            escape(&r.meta.client)
        );
    }
    if !r.meta.author.trim().is_empty() {
        let _ = writeln!(
            out,
            "<p class=\"meta\">Prepared by: {}</p>",
            escape(&r.meta.author)
        );
    }
    let _ = writeln!(
        out,
        "<p class=\"meta\">Generated {} with {}</p>",
        escape(&format_time(r.generated_at)),
        escape(&r.generator)
    );

    out.push_str("<h2>Executive summary</h2>\n");
    out.push_str(&paragraphs(&r.meta.executive_summary));
    out.push_str("\n<table><thead><tr><th>Severity</th><th>Findings</th></tr></thead><tbody>\n");
    for (sev, n) in r.counts() {
        let _ = writeln!(
            out,
            "<tr><td><span class=\"sev sev-{sev}\">{sev}</span></td><td>{n}</td></tr>"
        );
    }
    let _ = writeln!(
        out,
        "<tr><td><strong>Total</strong></td><td><strong>{}</strong></td></tr></tbody></table>",
        r.findings.len()
    );

    out.push_str("<h2>Target and scope</h2>\n");
    if r.targets.is_empty() {
        out.push_str("<p class=\"empty\">No hosts.</p>\n");
    } else {
        out.push_str("<p>Hosts with findings:</p>\n<ul>\n");
        for t in &r.targets {
            let _ = writeln!(out, "<li class=\"url\">{}</li>", escape(t));
        }
        out.push_str("</ul>\n");
    }
    if r.scope.is_empty() {
        out.push_str("<p class=\"empty\">No scope rules were defined.</p>\n");
    } else {
        out.push_str("<p>Scope rules in effect:</p>\n<ul>\n");
        for s in &r.scope {
            let _ = writeln!(out, "<li class=\"url\">{}</li>", escape(s));
        }
        out.push_str("</ul>\n");
    }

    out.push_str("<h2>Methodology</h2>\n");
    out.push_str(&paragraphs(&r.meta.methodology));

    out.push_str("\n<h2>Findings overview</h2>\n");
    if r.findings.is_empty() {
        out.push_str("<p class=\"empty\">No findings were included.</p>\n");
    } else {
        out.push_str(
            "<table><thead><tr><th>#</th><th>Severity</th><th>Title</th><th>Host</th><th>Status</th></tr></thead><tbody>\n",
        );
        for (i, f) in r.findings.iter().enumerate() {
            let _ = writeln!(
                out,
                "<tr><td>{}</td><td><span class=\"sev sev-{sev}\">{sev}</span></td><td>{}</td><td class=\"url\">{}</td><td>{}</td></tr>",
                i + 1,
                escape(&f.title),
                escape(&f.host),
                escape(&f.status),
                sev = escape(&f.severity),
            );
        }
        out.push_str("</tbody></table>\n<h2>Findings</h2>\n");
        for (i, f) in r.findings.iter().enumerate() {
            finding(&mut out, i + 1, f);
        }
    }
    out.push_str("</body></html>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::sample;

    #[test]
    fn escapes_everything_and_loads_nothing() {
        let html = render_html(&sample());
        assert!(html.contains("default-src 'none'"));
        assert!(!html.contains("<script"), "evidence must be escaped");
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("<title>Assessment &lt;of&gt; lab</title>"));
        assert!(html.contains("<p>Line &lt;two&gt; &amp; more.</p>"));
        assert!(
            !html.contains("src=") && !html.contains("href="),
            "no external resources"
        );
        for section in [
            "Executive summary",
            "Target and scope",
            "Methodology",
            "Findings overview",
            "Remediation",
            "Technical details",
            "Evidence 1",
        ] {
            assert!(html.contains(section), "missing {section}");
        }
        assert!(html.find("1. A high").unwrap() < html.find("2. A low").unwrap());
    }

    #[test]
    fn empty_report_is_valid() {
        let mut r = sample();
        r.findings.clear();
        r.targets.clear();
        r.scope.clear();
        let html = render_html(&r);
        assert!(html.contains("No findings were included."));
        assert!(html.contains("No scope rules were defined."));
    }
}
