//! Data types stored in and returned from the project database.

use chosproxy_core::Source;
pub use chosproxy_core::exchange::{CapturedExchange as NewExchange, flags};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Row id of an exchange.
pub type ExchangeId = i64;

/// Lightweight history row (no bodies).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryRow {
    /// Exchange id.
    #[ts(type = "number")]
    pub id: ExchangeId,
    /// Start time, Unix epoch ms.
    #[ts(type = "number")]
    pub started_at: i64,
    /// Duration in ms.
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    /// Producing module.
    pub source: Source,
    /// Scheme.
    pub scheme: String,
    /// Method.
    pub method: String,
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Path + query.
    pub path: String,
    /// Lowercase extension of the path.
    pub extension: Option<String>,
    /// Full URL.
    pub url: String,
    /// Request version token.
    pub http_version: String,
    /// Response status.
    pub status: Option<u16>,
    /// MIME category.
    pub mime: Option<String>,
    /// Request bytes on the wire.
    #[ts(type = "number")]
    pub request_len: u64,
    /// Response bytes on the wire.
    #[ts(type = "number | null")]
    pub response_len: Option<u64>,
    /// Upstream server IP.
    pub server_ip: Option<String>,
    /// Upstream used TLS.
    pub tls: bool,
    /// Error text.
    pub error: Option<String>,
    /// Notes.
    pub notes: Option<String>,
    /// Stored request was truncated.
    pub request_truncated: bool,
    /// Stored response was truncated.
    pub response_truncated: bool,
    /// Request had both Content-Length and Transfer-Encoding.
    pub cl_te_conflict: bool,
    /// Connection was upgraded after this exchange.
    pub upgraded: bool,
    /// Request or response was edited during interception.
    pub edited: bool,
    /// Request or response was dropped during interception.
    pub dropped: bool,
    /// Row highlight colour.
    pub highlight: Option<Highlight>,
    /// The URL is in the current scope.
    pub in_scope: bool,
}

/// Full stored exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredExchange {
    /// Metadata.
    pub row: HistoryRow,
    /// Client address.
    pub client_addr: Option<String>,
    /// Listener id.
    pub listener: Option<String>,
    /// Reason phrase.
    pub reason: Option<String>,
    /// Raw request.
    pub request: Vec<u8>,
    /// Raw response.
    pub response: Option<Vec<u8>>,
    /// Request before it was edited during interception.
    pub original_request: Option<Vec<u8>>,
    /// Response before it was edited during interception.
    pub original_response: Option<Vec<u8>>,
}

/// Sort order for history listings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum HistoryOrder {
    /// Oldest first.
    Asc,
    /// Newest first.
    #[default]
    Desc,
}

/// History column to sort by (ties keep capture order).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HistorySort {
    /// Capture order.
    #[default]
    Id,
    /// Method.
    Method,
    /// Host.
    Host,
    /// Path and query.
    Path,
    /// Response status.
    Status,
    /// Response size.
    Length,
    /// MIME category.
    Mime,
    /// Duration.
    Duration,
}

impl HistorySort {
    /// The column it sorts on (fixed SQL fragment).
    pub fn column(self) -> &'static str {
        match self {
            HistorySort::Id => "id",
            HistorySort::Method => "method",
            HistorySort::Host => "host",
            HistorySort::Path => "path",
            HistorySort::Status => "status",
            HistorySort::Length => "response_len",
            HistorySort::Mime => "mime",
            HistorySort::Duration => "duration_ms",
        }
    }
}

/// Row highlight colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Highlight {
    /// Red.
    Red,
    /// Orange.
    Orange,
    /// Yellow.
    Yellow,
    /// Green.
    Green,
    /// Cyan.
    Cyan,
    /// Blue.
    Blue,
    /// Purple.
    Purple,
    /// Gray.
    Gray,
}

impl Highlight {
    /// All colours, in display order.
    pub const ALL: [Highlight; 8] = [
        Highlight::Red,
        Highlight::Orange,
        Highlight::Yellow,
        Highlight::Green,
        Highlight::Cyan,
        Highlight::Blue,
        Highlight::Purple,
        Highlight::Gray,
    ];

    /// Stable identifier (used in storage).
    pub fn as_str(self) -> &'static str {
        match self {
            Highlight::Red => "red",
            Highlight::Orange => "orange",
            Highlight::Yellow => "yellow",
            Highlight::Green => "green",
            Highlight::Cyan => "cyan",
            Highlight::Blue => "blue",
            Highlight::Purple => "purple",
            Highlight::Gray => "gray",
        }
    }

    /// Parses [`Highlight::as_str`].
    pub fn parse(s: &str) -> Option<Highlight> {
        Highlight::ALL.into_iter().find(|h| h.as_str() == s)
    }
}

/// Restricts results to one origin and, optionally, a path prefix (a site
/// map node).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SiteFilter {
    /// `http` or `https`.
    pub scheme: String,
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Path prefix (`/` = whole origin). A prefix ending without `/` matches
    /// that endpoint exactly (with any query string).
    pub path: String,
}

/// History filter and paging. The filter fields are shared by HTTP history,
/// the site map and global search (FilterEngine, ARCHITECTURE §11).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryQuery {
    /// Rows to skip.
    pub offset: u32,
    /// Rows to return (1..=1000).
    pub limit: u32,
    /// Sort direction.
    pub order: HistoryOrder,
    /// Sort column (capture order by default).
    pub sort: HistorySort,
    /// Matched against the URL: a case-insensitive substring, or a regular
    /// expression when `search_regex` is set.
    pub search: Option<String>,
    /// `search` is a regular expression (case-insensitive unless it starts
    /// with `(?-i)`).
    pub search_regex: bool,
    /// Only URLs in the current scope.
    pub in_scope_only: bool,
    /// Only these MIME categories (e.g. `HTML`, `JSON`). Empty = all.
    pub mimes: Vec<String>,
    /// Only traffic from these tools. Empty = all.
    pub sources: Vec<Source>,
    /// Only rows with one of these highlights. Empty = all.
    pub highlights: Vec<Highlight>,
    /// Only rows with a note or a highlight.
    pub annotated_only: bool,
    /// Only one site map node.
    pub site: Option<SiteFilter>,
    /// Only these methods (empty = all).
    pub methods: Vec<String>,
    /// Status classes 1-5 (`0` = no response). Empty = all.
    pub status_classes: Vec<u8>,
    /// Hide these extensions (lowercase, without dot).
    pub hide_extensions: Vec<String>,
}

impl HistoryQuery {
    /// Largest page size.
    pub const MAX_LIMIT: u32 = 1000;
}

/// A page of history rows plus the total matching the filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryPage {
    /// Rows matching the filter in total.
    #[ts(type = "number")]
    pub total: u64,
    /// Requested rows.
    pub rows: Vec<HistoryRow>,
}

/// Aggregate counters for the dashboard.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectStats {
    /// Stored exchanges.
    #[ts(type = "number")]
    pub exchanges: u64,
    /// Distinct hosts.
    #[ts(type = "number")]
    pub hosts: u64,
    /// Distinct host + path endpoints (path without query).
    #[ts(type = "number")]
    pub endpoints: u64,
    /// Exchanges that ended in an error.
    #[ts(type = "number")]
    pub errors: u64,
}

/// Marks on site map nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SiteMark {
    /// Worth a closer look.
    Interesting,
    /// Already looked at.
    Reviewed,
    /// Not relevant.
    Ignore,
    /// Pinned by the user.
    Favorite,
}

impl SiteMark {
    /// Stable identifier (used in storage).
    pub fn as_str(self) -> &'static str {
        match self {
            SiteMark::Interesting => "interesting",
            SiteMark::Reviewed => "reviewed",
            SiteMark::Ignore => "ignore",
            SiteMark::Favorite => "favorite",
        }
    }

    /// Parses [`SiteMark::as_str`].
    pub fn parse(s: &str) -> Option<SiteMark> {
        Some(match s {
            "interesting" => SiteMark::Interesting,
            "reviewed" => SiteMark::Reviewed,
            "ignore" => SiteMark::Ignore,
            "favorite" => SiteMark::Favorite,
            _ => return None,
        })
    }
}

/// A mark on a site map node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SiteMarkEntry {
    /// Origin scheme.
    pub scheme: String,
    /// Origin host.
    pub host: String,
    /// Origin port.
    pub port: u16,
    /// Endpoint (`/a/b`) or folder (`/a/`).
    pub path: String,
    /// The mark.
    pub mark: SiteMark,
}

/// One endpoint (origin + path without query) seen in the traffic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SiteMapEntry {
    /// Scheme.
    pub scheme: String,
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Path without the query string.
    pub path: String,
    /// Exchanges for this endpoint.
    #[ts(type = "number")]
    pub count: u64,
    /// Distinct methods.
    pub methods: Vec<String>,
    /// Distinct response statuses.
    pub statuses: Vec<u16>,
    /// Distinct MIME categories.
    pub mimes: Vec<String>,
    /// Tools that produced the traffic.
    pub sources: Vec<Source>,
    /// Some request had a query string.
    pub has_params: bool,
    /// First seen, Unix epoch ms.
    #[ts(type = "number")]
    pub first_seen: i64,
    /// Last seen, Unix epoch ms.
    #[ts(type = "number")]
    pub last_seen: i64,
    /// Newest exchange id.
    #[ts(type = "number")]
    pub last_id: ExchangeId,
    /// In the current scope.
    pub in_scope: bool,
}

/// The site map: endpoints matching a filter plus the stored marks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SiteMap {
    /// Endpoints, ordered by host, port, scheme, path.
    pub entries: Vec<SiteMapEntry>,
    /// More endpoints matched than were returned.
    pub truncated: bool,
    /// Marks on nodes.
    pub marks: Vec<SiteMarkEntry>,
}

impl SiteMap {
    /// Most endpoints returned at once.
    pub const MAX_ENTRIES: u32 = 20_000;
}
