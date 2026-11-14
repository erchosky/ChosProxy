//! DOM Inspector (spec addendum §69, ADR-0027): finds where values from
//! URL parts, the referrer, `window.name`, cookies or `postMessage` reach
//! dangerous DOM sinks, by instrumenting the lab browser (not by reading
//! page source).
//!
//! * [`INSPECTOR_JS`]: the instrumentation installed in every frame before
//!   the page's scripts; it only observes and reports.
//! * [`cdp`]: the DevTools client that installs it in Chromium-family lab
//!   browsers and receives its reports.
//! * [`report`]: the reports, parsed as untrusted input.
//! * [`driver`]: drives the lab browser for crawling (M24).

pub mod cdp;
pub mod driver;
pub mod report;

pub use report::{DomReport, Kind};

/// The instrumentation script (placeholders replaced per session by
/// [`cdp::Instrumentation::new`]).
pub const INSPECTOR_JS: &str = include_str!("inspector.js");
