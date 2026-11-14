//! Reports sent by the instrumentation script. They come from JavaScript
//! running next to the page's own code, so they are treated as untrusted:
//! strict shape, known kinds only, every string cut to a limit.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Longest report accepted from the browser.
pub const MAX_REPORT: usize = 64 * 1024;
const MAX_TEXT: usize = 2000;
const MAX_STACK: usize = 20;

/// What was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "DomEventKind")]
pub enum Kind {
    /// A source value reached a sink.
    Flow,
    /// A navigation whose destination came from a source.
    Redirect,
    /// A `message` listener was added to the window.
    MessageListener,
    /// `Object.prototype` gained a property.
    PrototypePollution,
    /// A document property is shadowed by an element.
    Clobbering,
}

impl Kind {
    /// Lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Flow => "flow",
            Kind::Redirect => "redirect",
            Kind::MessageListener => "messageListener",
            Kind::PrototypePollution => "prototypePollution",
            Kind::Clobbering => "clobbering",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(s: &str) -> Option<Kind> {
        [
            Kind::Flow,
            Kind::Redirect,
            Kind::MessageListener,
            Kind::PrototypePollution,
            Kind::Clobbering,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

/// One observation from a frame of the lab browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomReport {
    /// What was observed.
    pub kind: Kind,
    /// The sink (e.g. `element.innerHTML`).
    pub sink: String,
    /// Value that reached the sink.
    pub value: String,
    /// The source it came from (e.g. `location.hash`).
    pub source: Option<String>,
    /// The source's value.
    pub source_value: Option<String>,
    /// The part of the source found in the sink value.
    pub matched: Option<String>,
    /// How the matched part changed on the way (`unchanged`, `URL-decoded`).
    pub transformation: Option<String>,
    /// The session canary reached the sink.
    #[serde(default)]
    pub canary: bool,
    /// For message listeners: the code mentions `origin`.
    #[serde(default)]
    pub checks_origin: Option<bool>,
    /// URL of the frame.
    pub frame_url: String,
    /// The frame is the top-level page.
    #[serde(default)]
    pub top_frame: bool,
    /// Unix epoch ms (browser clock).
    pub at: i64,
    /// JavaScript stack at the sink (`at fn (file:line:col)` lines).
    #[serde(default)]
    pub stack: Vec<String>,
}

fn cut(s: &mut String, max: usize) {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
}

impl DomReport {
    /// Parses and bounds one report.
    pub fn parse(payload: &str) -> Result<DomReport, String> {
        if payload.len() > MAX_REPORT {
            return Err("report too large".into());
        }
        let mut r: DomReport =
            serde_json::from_str(payload).map_err(|e| format!("invalid report: {e}"))?;
        cut(&mut r.sink, 200);
        cut(&mut r.value, MAX_TEXT);
        cut(&mut r.frame_url, MAX_TEXT);
        for s in [&mut r.source, &mut r.matched, &mut r.transformation]
            .into_iter()
            .flatten()
        {
            cut(s, 200);
        }
        if let Some(v) = r.source_value.as_mut() {
            cut(v, MAX_TEXT);
        }
        r.stack.truncate(MAX_STACK);
        for line in &mut r.stack {
            cut(line, 300);
        }
        if r.sink.is_empty() {
            return Err("report without a sink".into());
        }
        Ok(r)
    }

    /// File, line and column of the innermost page frame of the stack.
    pub fn location(&self) -> Option<String> {
        self.stack.iter().find_map(|l| {
            let inner = l.trim().rsplit_once('(').map_or(l.trim(), |(_, r)| r);
            let inner = inner.trim_start_matches("at ").trim_end_matches(')');
            (inner.contains("://") && inner.matches(':').count() >= 3).then(|| inner.to_string())
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn parses_bounds_and_locates() {
        let r = DomReport::parse(&format!(
            r#"{{"kind":"flow","sink":"element.innerHTML","value":"{}","source":"location.hash",
            "sourceValue":"x","matched":"chosabc","transformation":"unchanged","canary":true,
            "frameUrl":"https://lab.test/p","topFrame":true,"at":5,
            "stack":["    at render (https://lab.test/app.js:12:7)","    at <anonymous>"]}}"#,
            "v".repeat(5000)
        ))
        .unwrap();
        assert_eq!(r.kind, Kind::Flow);
        assert_eq!(r.value.len(), MAX_TEXT);
        assert_eq!(
            r.location().as_deref(),
            Some("https://lab.test/app.js:12:7")
        );
        assert!(DomReport::parse(r#"{"kind":"run","sink":"x"}"#).is_err());
        assert!(DomReport::parse(&"x".repeat(MAX_REPORT + 1)).is_err());
    }
}
