//! The crawler's pure parts (spec §13): what a crawl may do
//! ([`CrawlConfig`]), what a response points to ([`discover`]) and which URL
//! comes next ([`Frontier`]). Sending requests, scope and storage are the
//! engine's job, so this crate stays testable without a network.

mod discover;
mod frontier;

pub use discover::{Found, FoundKind, discover, robots_and_sitemap_paths};
pub use frontier::{Entry, Frontier, Push, normalize};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Words that mark links a crawler must not follow because they may change
/// state (log out, delete, buy…). Matched against the path and query.
pub const DEFAULT_AVOID: &[&str] = &[
    "logout",
    "log-out",
    "log_out",
    "signout",
    "sign-out",
    "sign_out",
    "logoff",
    "delete",
    "remove",
    "destroy",
    "unsubscribe",
    "deactivate",
    "reset",
    "checkout",
    "purchase",
    "payment",
];

/// What a crawl may do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct CrawlConfig {
    /// Where to start (absolute http/https URLs).
    pub start_urls: Vec<String>,
    /// Links followed from a start URL (0 = only the start URLs).
    pub max_depth: u32,
    /// Requests sent at most.
    pub max_requests: u32,
    /// Requests in flight at once.
    pub concurrency: u32,
    /// Requests started per second (0 = no limit).
    pub requests_per_second: u32,
    /// Seconds to wait for each response.
    pub timeout_secs: u32,
    /// Different query strings fetched per path (e.g. `/item?id=…`).
    pub max_variants_per_path: u32,
    /// URLs fetched per structural pattern: `/item/1` and `/item/2?x=…`
    /// share the pattern `/item/{n}?x` (0 = no limit).
    pub max_similar: u32,
    /// Submit forms that use GET, with default values.
    pub submit_get_forms: bool,
    /// Send matching cookies from the project cookie jar (crawl as the
    /// logged-in user) and keep cookies the crawl receives.
    pub use_cookie_jar: bool,
    /// Do not follow URLs whose path or query contains one of these words.
    pub avoid: Vec<String>,
    /// User-Agent header.
    pub user_agent: String,
}

impl Default for CrawlConfig {
    fn default() -> CrawlConfig {
        CrawlConfig {
            start_urls: Vec::new(),
            max_depth: 5,
            max_requests: 500,
            concurrency: 4,
            requests_per_second: 10,
            timeout_secs: 15,
            max_variants_per_path: 10,
            max_similar: 20,
            submit_get_forms: true,
            use_cookie_jar: true,
            avoid: DEFAULT_AVOID.iter().map(|s| (*s).to_string()).collect(),
            user_agent: "Mozilla/5.0 (compatible; ChosProxy Crawler)".into(),
        }
    }
}

impl CrawlConfig {
    /// Checks the limits and the start URLs; returns the start URLs parsed.
    pub fn validate(&self) -> Result<Vec<url::Url>, String> {
        let ranges: [(&str, u32, u32, u32); 7] = [
            ("depth", self.max_depth, 0, 50),
            ("maximum requests", self.max_requests, 1, 100_000),
            ("concurrency", self.concurrency, 1, 32),
            ("requests per second", self.requests_per_second, 0, 200),
            ("timeout", self.timeout_secs, 1, 300),
            ("variants per path", self.max_variants_per_path, 1, 1000),
            ("similar URLs per pattern", self.max_similar, 0, 10_000),
        ];
        for (name, value, min, max) in ranges {
            if !(min..=max).contains(&value) {
                return Err(format!("the {name} must be {min}–{max}"));
            }
        }
        if self.avoid.len() > 200 || self.avoid.iter().any(|w| w.len() > 100) {
            return Err("too many or too long words to avoid".into());
        }
        if self.user_agent.trim().is_empty()
            || self.user_agent.len() > 300
            || self.user_agent.contains(['\r', '\n'])
        {
            return Err("invalid User-Agent".into());
        }
        if self.start_urls.is_empty() {
            return Err("add at least one start URL".into());
        }
        if self.start_urls.len() > 100 {
            return Err("at most 100 start URLs".into());
        }
        self.start_urls
            .iter()
            .map(|u| normalize(u.trim()).ok_or_else(|| format!("invalid start URL {u:?}")))
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn defaults_and_validation() {
        let mut c = CrawlConfig {
            start_urls: vec![" https://Shop.Example/#top ".into()],
            ..CrawlConfig::default()
        };
        let urls = c.validate().unwrap();
        assert_eq!(urls[0].as_str(), "https://shop.example/");
        c.concurrency = 0;
        assert!(c.validate().unwrap_err().contains("concurrency"));
        c.concurrency = 4;
        c.start_urls = vec!["ftp://x".into()];
        assert!(c.validate().unwrap_err().contains("start URL"));
        c.start_urls.clear();
        assert!(c.validate().is_err());

        // Missing fields take their defaults.
        let partial: CrawlConfig =
            serde_json::from_str(r#"{"startUrls":["http://a.test"],"maxDepth":1}"#).unwrap();
        assert_eq!((partial.max_depth, partial.max_requests), (1, 500));
        assert!(partial.avoid.iter().any(|w| w == "logout"));
    }
}
