//! Markdown report. Inline text is escaped so titles and notes cannot
//! inject structure; messages go in fenced blocks longer than any backtick
//! run they contain.

use crate::{Report, ReportMessage, format_time};
use std::fmt::Write as _;

/// Escapes Markdown punctuation (and `<`, `>`) in inline text.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '!' | '|'
            | '~' => {
                out.push('\\');
                out.push(c);
            }
            // Otherwise "&lt;" typed by the tester would render as "<".
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => {}
            '\n' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Neutralises what only has meaning at the start of a line: "-" (list
/// item, rule, heading underline), "=" (heading underline) and "1." (ordered
/// list). A backslash only escapes punctuation, so for "1." it goes before
/// the dot.
fn line_start(e: String) -> String {
    if e.starts_with(['-', '=']) {
        return format!("\\{e}");
    }
    let digits = e.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && e[digits..].starts_with('.') {
        return format!("{}\\{}", &e[..digits], &e[digits..]);
    }
    e
}

/// Multi-line text: paragraphs kept, each line escaped.
fn block(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let t = text.trim();
    if t.is_empty() {
        return "—".into();
    }
    t.split("\n\n")
        .map(|p| {
            p.lines()
                .map(|l| line_start(escape(l.trim())))
                .collect::<Vec<_>>()
                .join("  \n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn fence(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

fn message(out: &mut String, label: &str, m: &ReportMessage) {
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
        format!(" ({})", flags.join(", "))
    };
    let text = m.text.replace("\r\n", "\n");
    let f = fence(&text);
    let _ = write!(
        out,
        "{label}{flags}:\n\n{f}http\n{}\n{f}\n\n",
        text.trim_end()
    );
}

/// The report as Markdown.
pub fn render_markdown(r: &Report) -> String {
    let mut out = String::with_capacity(32 * 1024);
    let title = if r.meta.title.trim().is_empty() {
        "Security assessment report".to_string()
    } else {
        r.meta.title.clone()
    };
    let _ = writeln!(out, "# {}\n", escape(&title));
    if !r.meta.client.trim().is_empty() {
        let _ = writeln!(out, "Prepared for: {}  ", escape(&r.meta.client));
    }
    if !r.meta.author.trim().is_empty() {
        let _ = writeln!(out, "Prepared by: {}  ", escape(&r.meta.author));
    }
    let _ = writeln!(
        out,
        "Generated {} with {}\n",
        format_time(r.generated_at),
        escape(&r.generator)
    );

    let _ = writeln!(
        out,
        "## Executive summary\n\n{}\n",
        block(&r.meta.executive_summary)
    );
    out.push_str("| Severity | Findings |\n|---|---|\n");
    for (sev, n) in r.counts() {
        let _ = writeln!(out, "| {sev} | {n} |");
    }
    let _ = writeln!(out, "| **Total** | **{}** |\n", r.findings.len());

    out.push_str("## Target and scope\n\n");
    if r.targets.is_empty() {
        out.push_str("No hosts.\n\n");
    } else {
        out.push_str("Hosts with findings:\n\n");
        for t in &r.targets {
            let _ = writeln!(out, "- {}", escape(t));
        }
        out.push('\n');
    }
    if r.scope.is_empty() {
        out.push_str("No scope rules were defined.\n\n");
    } else {
        out.push_str("Scope rules in effect:\n\n");
        for s in &r.scope {
            let _ = writeln!(out, "- {}", escape(s));
        }
        out.push('\n');
    }

    let _ = writeln!(out, "## Methodology\n\n{}\n", block(&r.meta.methodology));

    out.push_str("## Findings overview\n\n");
    if r.findings.is_empty() {
        out.push_str("No findings were included.\n");
        return out;
    }
    out.push_str("| # | Severity | Title | Host | Status |\n|---|---|---|---|---|\n");
    for (i, f) in r.findings.iter().enumerate() {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            i + 1,
            escape(&f.severity),
            escape(&f.title),
            escape(&f.host),
            escape(&f.status)
        );
    }
    out.push_str("\n## Findings\n\n");
    for (i, f) in r.findings.iter().enumerate() {
        let _ = write!(
            out,
            "### {}. {}\n\n- Severity: **{}**\n- Confidence: {}\n- Status: {}\n- Type: {}\n- URL: {}\n\n\
             #### Description\n\n{}\n\n#### Remediation\n\n{}\n\n",
            i + 1,
            escape(&f.title),
            escape(&f.severity),
            escape(&f.confidence),
            escape(&f.status),
            escape(&f.kind),
            escape(&f.url),
            block(&f.description),
            block(&f.remediation),
        );
        if !f.notes.trim().is_empty() {
            let _ = write!(out, "#### Technical details\n\n{}\n\n", block(&f.notes));
        }
        for (n, e) in f.evidence.iter().enumerate() {
            let status = e.status.map(|s| format!(" → {s}")).unwrap_or_default();
            let _ = write!(
                out,
                "#### Evidence {}: {} {}{}\n\n",
                n + 1,
                escape(&e.method),
                escape(&e.url),
                status
            );
            if !e.note.trim().is_empty() {
                let _ = write!(out, "{}\n\n", block(&e.note));
            }
            message(&mut out, "Request", &e.request);
            if let Some(r) = &e.response {
                message(&mut out, "Response", r);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::sample;

    #[test]
    fn escapes_inline_text_and_fences_messages() {
        let mut r = sample();
        r.findings[0].title = "Title with [link](x) and | pipe".into();
        r.findings[0].evidence[0].response.as_mut().unwrap().text = "body with ``` fence".into();
        let md = render_markdown(&r);
        assert!(md.contains("Title with \\[link\\]\\(x\\) and \\| pipe"));
        assert!(
            md.contains("````http\nbody with ``` fence\n````"),
            "fence longer than content"
        );
        assert!(md.contains("# Assessment &lt;of&gt; lab"));
        assert!(md.contains("Fix \\*it\\*."));
        for section in [
            "## Executive summary",
            "## Target and scope",
            "## Methodology",
            "## Findings overview",
            "#### Remediation",
            "#### Technical details",
            "#### Evidence 1",
        ] {
            assert!(md.contains(section), "missing {section}");
        }
    }

    #[test]
    fn list_markers_in_text_are_neutralised() {
        assert_eq!(
            block("- not a list\n1. nor this\n10 requests\n==="),
            "\\- not a list  \n1\\. nor this  \n10 requests  \n\\==="
        );
        assert_eq!(block("a &lt;b&gt; & c"), "a &amp;lt;b&amp;gt; &amp; c");
    }
}
