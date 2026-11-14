//! DOM Inspector in the engine (spec addendum §69, ADR-0027).
//!
//! When the lab browser is launched with the DOM Inspector, ChosProxy
//! connects to its DevTools endpoint (random loopback port, only for that
//! browser), installs the instrumentation in every frame and stores what it
//! reports. Observations whose page response is in the history become
//! findings with that response as evidence.

use super::Engine;
use super::lock;
use crate::AppError;
use chosproxy_core::event::{EngineEvent, NoticeLevel};
use chosproxy_dom::cdp::{self, Instrumentation};
use chosproxy_dom::{DomReport, Kind};
use chosproxy_storage::{
    Confidence, DomEventRow, FindingSeverity, NewDomEvent, PassiveIssue, without_fragment,
};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use ts_rs::TS;

/// Time the browser has to open its DevTools endpoint.
const ENDPOINT_WAIT: Duration = Duration::from_secs(20);

/// DOM Inspector state shown in the UI.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DomStatus {
    /// Instrumenting the lab browser now.
    pub active: bool,
    /// The session canary to put into sources.
    pub canary: Option<String>,
    /// Why the inspector stopped or could not start.
    pub error: Option<String>,
    /// Reports received this session.
    #[ts(type = "number")]
    pub observed: u64,
}

const HTML_SINKS: &[&str] = &[
    "element.innerHTML",
    "element.outerHTML",
    "element.insertAdjacentHTML",
    "document.write",
    "document.writeln",
    "range.createContextualFragment",
    "iframe.srcdoc",
    "script.text",
    "script.src",
    "setTimeout(string)",
    "setInterval(string)",
    "Function()",
    "element.setAttribute",
];

fn page_of(url: &str) -> &str {
    let url = without_fragment(url);
    url.split_once('?').map_or(url, |(u, _)| u)
}

fn to_new_event(r: DomReport) -> NewDomEvent {
    let location = r.location();
    let dedup_key = format!(
        "{}|{}|{}|{}|{}",
        r.kind.as_str(),
        r.sink,
        r.source.as_deref().unwrap_or(""),
        page_of(&r.frame_url),
        location.as_deref().unwrap_or("")
    );
    NewDomEvent {
        at: r.at,
        kind: r.kind.as_str().to_string(),
        sink: r.sink,
        source: r.source,
        source_value: r.source_value,
        matched: r.matched,
        transformation: r.transformation,
        value: r.value,
        canary: r.canary,
        checks_origin: r.checks_origin,
        frame_url: r.frame_url,
        top_frame: r.top_frame,
        location,
        stack: r.stack,
        dedup_key,
    }
}

/// The finding an observation stands for, if any.
pub(crate) fn finding_for(row: &DomEventRow) -> Option<PassiveIssue> {
    let exchange_id = row.exchange_id?;
    let kind = Kind::parse(&row.kind)?;
    let flow = format!(
        "{} → {} → {}",
        row.source.as_deref().unwrap_or("unknown source"),
        row.transformation.as_deref().unwrap_or("unchanged"),
        row.sink
    );
    let at = row
        .location
        .as_deref()
        .map(|l| format!(" at {l}"))
        .unwrap_or_default();
    let tester_input = if row.canary {
        " The session canary reached the sink, so input you control arrives there."
    } else {
        " A piece of the source value was found in the sink value; confirm it with the canary."
    };
    let (family, title, severity, confidence, description, remediation) = match kind {
        Kind::Flow if HTML_SINKS.contains(&row.sink.as_str()) => (
            "xss",
            format!("DOM-based cross-site scripting candidate: {}", row.sink),
            if row.canary {
                FindingSeverity::High
            } else {
                FindingSeverity::Medium
            },
            if row.canary {
                Confidence::Firm
            } else {
                Confidence::Tentative
            },
            format!(
                "Client-side code passes data from {flow}{at}.{tester_input} Script or HTML \
                 written through this sink runs in the page's origin."
            ),
            "Do not write untrusted data as HTML or code: use textContent or setAttribute on \
             safe attributes, encode for the context, or sanitize with a vetted library and a \
             Trusted Types policy."
                .to_string(),
        ),
        Kind::Flow | Kind::Redirect => (
            "redirect",
            format!(
                "DOM-based URL injection or open redirection candidate: {}",
                row.sink
            ),
            FindingSeverity::Medium,
            if row.canary {
                Confidence::Firm
            } else {
                Confidence::Tentative
            },
            format!(
                "Client-side code uses data from {flow}{at} as a URL.{tester_input} An attacker \
                 may send users to another site or, with javascript: URLs, run script."
            ),
            "Only navigate to URLs from an allow-list or to same-origin relative paths; reject \
             javascript: and data: URLs."
                .to_string(),
        ),
        Kind::MessageListener if row.checks_origin == Some(false) => (
            "postmessage",
            "postMessage listener without an origin check".to_string(),
            FindingSeverity::Low,
            Confidence::Tentative,
            format!(
                "The page registers a message listener{at} whose code never mentions the \
                 sender's origin, so any window that can reference it can send it data."
            ),
            "Check event.origin against the expected origins before using event.data, and \
             treat the data as untrusted."
                .to_string(),
        ),
        Kind::MessageListener => return None,
        Kind::PrototypePollution => (
            "pollution",
            format!("Client-side prototype pollution: {}", row.sink),
            FindingSeverity::Medium,
            if row.canary || row.source.is_some() {
                Confidence::Firm
            } else {
                Confidence::Tentative
            },
            format!(
                "Object.prototype gained a property ({}) while the page ran{}. Polluted \
                 prototypes can change application logic and lead to script execution through \
                 gadgets.",
                row.sink,
                row.source
                    .as_deref()
                    .map(|s| format!("; it came from {s}"))
                    .unwrap_or_default()
            ),
            "Do not merge untrusted keys into objects; reject __proto__, constructor and \
             prototype keys, or use Map / Object.create(null)."
                .to_string(),
        ),
        Kind::Clobbering => (
            "clobbering",
            format!("DOM clobbering: {}", row.sink),
            FindingSeverity::Info,
            Confidence::Tentative,
            format!(
                "An element's id or name shadows {} in this page. If scripts read that \
                 property, markup an attacker can inject changes what they get.",
                row.sink
            ),
            "Avoid reading globals and document properties that markup can shadow; check \
             types before use."
                .to_string(),
        ),
    };
    Some(PassiveIssue {
        source: "dom",
        exchange_id,
        dedup_key: format!("dom:{family}:{}:{}", row.sink, page_of(&row.frame_url)),
        kind: match family {
            "xss" => "DOM-based cross-site scripting".into(),
            "redirect" => "DOM-based open redirection".into(),
            "postmessage" => "postMessage without origin check".into(),
            "pollution" => "Client-side prototype pollution".into(),
            _ => "DOM clobbering".into(),
        },
        title,
        severity,
        confidence,
        description,
        remediation,
        evidence_note: format!(
            "Observed in the lab browser: {flow} in {} ({} time(s)).",
            row.frame_url, row.hits
        ),
    })
}

impl Engine {
    /// DOM Inspector state.
    pub fn dom_status(&self) -> DomStatus {
        lock(&self.inner.dom_status).clone()
    }

    /// Opens `url` in the instrumented lab browser (its first tab).
    pub async fn dom_navigate(&self, url: String) -> Result<(), AppError> {
        let url = url.trim().to_string();
        if !(url.starts_with("http://") || url.starts_with("https://")) || url.len() > 8192 {
            return Err(AppError::Invalid("enter an http:// or https:// URL".into()));
        }
        let tx = lock(&self.inner.dom_navigate)
            .clone()
            .ok_or_else(|| AppError::Invalid("the DOM Inspector is not running".into()))?;
        tx.send(url)
            .await
            .map_err(|_| AppError::Invalid("the DOM Inspector is not running".into()))
    }

    /// Observations of the open project, most recent first.
    pub async fn dom_events(&self) -> Result<Vec<DomEventRow>, AppError> {
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || db.dom_events(2000)).await??)
    }

    /// Deletes the project's observations (findings stay).
    pub async fn dom_clear(&self) -> Result<(), AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || db.dom_clear()).await??;
        let _ = self.inner.events.send(EngineEvent::DomChanged);
        Ok(())
    }

    /// Connects to the lab browser's DevTools endpoint and instruments it
    /// until the browser closes.
    pub(crate) fn start_dom_inspector(&self, profile: PathBuf, url: String) {
        let weak = std::sync::Arc::downgrade(&self.inner);
        let task = tokio::spawn(async move {
            let Some(engine) = weak.upgrade().map(Engine::from_inner) else {
                return;
            };
            let result = engine.dom_session(profile, url).await;
            *lock(&engine.inner.dom_navigate) = None;
            let mut status = lock(&engine.inner.dom_status);
            status.active = false;
            if let Err(e) = result {
                status.error = Some(e.clone());
                drop(status);
                engine.notice(NoticeLevel::Warning, format!("DOM Inspector: {e}"));
            } else {
                drop(status);
            }
            let _ = engine.inner.events.send(EngineEvent::DomChanged);
        });
        if let Some(old) = lock(&self.inner.dom_task).replace(task) {
            old.abort();
        }
    }

    async fn dom_session(&self, profile: PathBuf, url: String) -> Result<(), String> {
        let (port, path) = cdp::devtools_endpoint(&profile, ENDPOINT_WAIT)
            .await
            .map_err(|e| e.to_string())?;
        let inst = Instrumentation::new().map_err(|e| e.to_string())?;
        *lock(&self.inner.dom_status) = DomStatus {
            active: true,
            canary: Some(inst.canary.clone()),
            error: None,
            observed: 0,
        };
        let _ = self.inner.events.send(EngineEvent::DomChanged);
        let (tx, mut rx) = mpsc::channel::<DomReport>(1024);
        let (nav_tx, nav_rx) = mpsc::channel::<String>(8);
        *lock(&self.inner.dom_navigate) = Some(nav_tx);
        let session =
            tokio::spawn(async move { cdp::run(port, &path, inst, Some(url), nav_rx, tx).await });
        while let Some(first) = rx.recv().await {
            let mut batch = vec![first];
            while let Ok(r) = rx.try_recv() {
                batch.push(r);
                if batch.len() >= 200 {
                    break;
                }
            }
            lock(&self.inner.dom_status).observed += batch.len() as u64;
            self.dom_store(batch).await;
        }
        match session.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Ok(()),
        }
    }

    async fn dom_store(&self, reports: Vec<DomReport>) {
        let Ok(db) = self.db() else {
            return; // no project open: nothing to keep
        };
        let events: Vec<NewDomEvent> = reports.into_iter().map(to_new_event).collect();
        let stored = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || db.dom_record(events)).await
        };
        let rows = match stored {
            Ok(Ok(rows)) => rows,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "could not store DOM observations");
                return;
            }
            Err(_) => return,
        };
        let issues: Vec<PassiveIssue> = rows.iter().filter_map(finding_for).collect();
        if !issues.is_empty()
            && let Ok(Ok(r)) =
                tokio::task::spawn_blocking(move || db.findings_record_auto(issues)).await
            && r.created + r.updated > 0
        {
            let _ = self
                .inner
                .events
                .send(EngineEvent::FindingsChanged { created: r.created });
        }
        let _ = self.inner.events.send(EngineEvent::DomChanged);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn row(kind: &str, sink: &str, canary: bool) -> DomEventRow {
        DomEventRow {
            id: 1,
            first_at: 1,
            last_at: 1,
            hits: 2,
            kind: kind.into(),
            sink: sink.into(),
            source: Some("location.hash".into()),
            source_value: Some("x".into()),
            matched: Some("m".into()),
            transformation: Some("URL-decoded".into()),
            value: "v".into(),
            canary,
            checks_origin: None,
            frame_url: "http://lab.test/dom/hash?a=1#x".into(),
            top_frame: true,
            location: Some("http://lab.test/dom/hash:6:51".into()),
            stack: vec![],
            exchange_id: Some(9),
        }
    }

    #[test]
    fn observations_map_to_findings() {
        let xss = finding_for(&row("flow", "element.innerHTML", true)).unwrap();
        assert_eq!(
            (xss.severity, xss.confidence),
            (FindingSeverity::High, Confidence::Firm)
        );
        assert_eq!(
            xss.dedup_key,
            "dom:xss:element.innerHTML:http://lab.test/dom/hash"
        );
        assert!(xss.description.contains(
            "location.hash → URL-decoded → element.innerHTML at http://lab.test/dom/hash:6:51"
        ));
        let weak = finding_for(&row("flow", "element.innerHTML", false)).unwrap();
        assert_eq!(weak.confidence, Confidence::Tentative);
        let redirect = finding_for(&row("redirect", "navigation", true)).unwrap();
        assert_eq!(redirect.kind, "DOM-based open redirection");
        let mut listener = row(
            "messageListener",
            "window.addEventListener('message')",
            false,
        );
        listener.checks_origin = Some(true);
        assert!(finding_for(&listener).is_none());
        listener.checks_origin = Some(false);
        assert!(finding_for(&listener).is_some());
        let mut unlinked = row("flow", "element.innerHTML", true);
        unlinked.exchange_id = None;
        assert!(
            finding_for(&unlinked).is_none(),
            "findings need the page response as evidence"
        );
    }
}
