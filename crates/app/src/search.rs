//! Global search over stored traffic: URLs, request and response headers and
//! bodies (after removing chunked framing and compression), and notes.

use crate::AppError;
use crate::detail::decode_body;
use chosproxy_http::content::split_message;
use chosproxy_storage::{HistoryQuery, HistoryRow, ProjectDb, StoredExchange, WsMessageRow};
use regex::bytes::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use ts_rs::TS;

/// Most exchanges examined by one search (newest first).
pub const MAX_SCANNED: u32 = 100_000;
/// Most hits returned by one search.
pub const MAX_HITS: u32 = 1_000;
/// Bytes of context on each side of a match.
const CONTEXT_BYTES: usize = 60;

/// What to search for and where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct SearchQuery {
    /// Text or regular expression.
    pub text: String,
    /// `text` is a regular expression.
    pub regex: bool,
    /// Match case.
    pub case_sensitive: bool,
    /// Search URLs.
    pub in_urls: bool,
    /// Search request headers.
    pub in_request_headers: bool,
    /// Search request bodies.
    pub in_request_bodies: bool,
    /// Search response headers.
    pub in_response_headers: bool,
    /// Search response bodies.
    pub in_response_bodies: bool,
    /// Search notes.
    pub in_notes: bool,
    /// Search WebSocket message payloads (only "in scope only" of the
    /// filter applies to them).
    pub in_websockets: bool,
    /// Restricts which exchanges are searched (paging fields are ignored).
    pub filter: HistoryQuery,
    /// Most hits to return (1..=1000).
    pub max_hits: u32,
}

impl Default for SearchQuery {
    fn default() -> Self {
        SearchQuery {
            text: String::new(),
            regex: false,
            case_sensitive: false,
            in_urls: true,
            in_request_headers: true,
            in_request_bodies: true,
            in_response_headers: true,
            in_response_bodies: true,
            in_notes: true,
            in_websockets: true,
            filter: HistoryQuery::default(),
            max_hits: 500,
        }
    }
}

/// Where a match was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SearchLocation {
    /// The URL.
    Url,
    /// Request line and headers.
    RequestHeaders,
    /// Request body.
    RequestBody,
    /// Status line and response headers.
    ResponseHeaders,
    /// Response body.
    ResponseBody,
    /// The user's notes.
    Notes,
}

/// The first match of one exchange in one location.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchHit {
    /// The exchange.
    pub row: HistoryRow,
    /// Where the match is.
    pub location: SearchLocation,
    /// Text before the match.
    pub before: String,
    /// The matched text.
    pub matched: String,
    /// Text after the match.
    pub after: String,
    /// Matches in this location.
    pub count: u32,
}

/// The first match in one WebSocket message.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WsSearchHit {
    /// The message.
    pub message: WsMessageRow,
    /// Its connection's URL.
    pub url: String,
    /// Text before the match.
    pub before: String,
    /// The matched text.
    pub matched: String,
    /// Text after the match.
    pub after: String,
    /// Matches in this message.
    pub count: u32,
}

/// Search outcome.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchResults {
    /// Hits, newest exchange first.
    pub hits: Vec<SearchHit>,
    /// Exchanges examined.
    pub scanned: u32,
    /// Exchanges with at least one hit.
    pub matched_exchanges: u32,
    /// WebSocket message hits, newest first.
    pub ws_hits: Vec<WsSearchHit>,
    /// WebSocket messages examined.
    pub ws_scanned: u32,
    /// The hit or scan limit stopped the search early.
    pub truncated: bool,
    /// A newer search replaced this one before it finished.
    pub superseded: bool,
    /// Time taken.
    #[ts(type = "number")]
    pub elapsed_ms: u64,
}

fn build(q: &SearchQuery) -> Result<Regex, AppError> {
    let text = q.text.trim();
    if text.is_empty() {
        return Err(AppError::Invalid("enter something to search for".into()));
    }
    if text.len() > 4096 {
        return Err(AppError::Invalid("the search text is too long".into()));
    }
    let pattern = if q.regex {
        text.to_string()
    } else {
        regex::escape(text)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!q.case_sensitive)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| AppError::Invalid(format!("invalid regular expression: {e}")))
}

fn clean(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn find(re: &Regex, hay: &[u8]) -> Option<(usize, usize, u32)> {
    let mut iter = re.find_iter(hay);
    let first = iter.next()?;
    // Count a bounded number of further matches; huge bodies stay cheap.
    let more = iter.take(9_999).count();
    Some((
        first.start(),
        first.end(),
        u32::try_from(more + 1).unwrap_or(u32::MAX),
    ))
}

fn hit(row: &HistoryRow, location: SearchLocation, re: &Regex, hay: &[u8]) -> Option<SearchHit> {
    let (start, end, count) = find(re, hay)?;
    let from = start.saturating_sub(CONTEXT_BYTES);
    let to = (end + CONTEXT_BYTES).min(hay.len());
    // Keep a matched span readable even when the expression matched a lot.
    let shown_end = end.min(start + 400);
    Some(SearchHit {
        row: row.clone(),
        location,
        before: clean(&hay[from..start]),
        matched: clean(&hay[start..shown_end]),
        after: clean(&hay[shown_end..to.max(shown_end)]),
        count,
    })
}

fn search_exchange(q: &SearchQuery, re: &Regex, ex: &StoredExchange, out: &mut Vec<SearchHit>) {
    let row = &ex.row;
    if q.in_urls
        && let Some(h) = hit(row, SearchLocation::Url, re, row.url.as_bytes())
    {
        out.push(h);
    }
    let mut message = |raw: &[u8], head_loc, body_loc, want_head: bool, want_body: bool| {
        if !want_head && !want_body {
            return;
        }
        let (head, body) = split_message(raw);
        if want_head && let Some(h) = hit(row, head_loc, re, head) {
            out.push(h);
        }
        if want_body && !body.is_empty() {
            let decoded = decode_body(head, body);
            if let Some(h) = hit(row, body_loc, re, &decoded.data) {
                out.push(h);
            }
        }
    };
    message(
        &ex.request,
        SearchLocation::RequestHeaders,
        SearchLocation::RequestBody,
        q.in_request_headers,
        q.in_request_bodies,
    );
    if let Some(response) = &ex.response {
        message(
            response,
            SearchLocation::ResponseHeaders,
            SearchLocation::ResponseBody,
            q.in_response_headers,
            q.in_response_bodies,
        );
    }
    if q.in_notes
        && let Some(notes) = &row.notes
        && let Some(h) = hit(row, SearchLocation::Notes, re, notes.as_bytes())
    {
        out.push(h);
    }
}

/// Exchanges handed to each worker at a time. Batches are processed in
/// order, so results stay newest first and the hit limit stops early.
const BATCH_PER_WORKER: usize = 128;

/// Searches loaded exchanges on several threads; one hit list per exchange,
/// in order.
fn search_batch(
    q: &SearchQuery,
    re: &Regex,
    exchanges: &[StoredExchange],
    workers: usize,
) -> Result<Vec<Vec<SearchHit>>, AppError> {
    let chunk = exchanges.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = exchanges
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|ex| {
                            let mut hits = Vec::new();
                            search_exchange(q, re, ex, &mut hits);
                            hits
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut all = Vec::with_capacity(exchanges.len());
        for h in handles {
            all.extend(
                h.join()
                    .map_err(|_| AppError::Internal("a search worker panicked".into()))?,
            );
        }
        Ok(all)
    })
}

/// Runs a search (blocking). `generation` is bumped by every new search; when
/// it moves past `mine` this search stops and reports itself superseded.
pub(crate) fn run(
    db: &ProjectDb,
    q: &SearchQuery,
    generation: &AtomicU64,
    mine: u64,
) -> Result<SearchResults, AppError> {
    let started = Instant::now();
    let re = build(q)?;
    let max_hits = q.max_hits.clamp(1, MAX_HITS) as usize;
    let ids = db.matching_ids(&q.filter, MAX_SCANNED + 1)?;
    let mut truncated = ids.len() > MAX_SCANNED as usize;
    let ids = &ids[..ids.len().min(MAX_SCANNED as usize)];
    let workers = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4)
        .clamp(1, 8);
    let mut results = SearchResults {
        hits: Vec::new(),
        scanned: 0,
        matched_exchanges: 0,
        ws_hits: Vec::new(),
        ws_scanned: 0,
        truncated: false,
        superseded: false,
        elapsed_ms: 0,
    };
    'batches: for batch in ids.chunks(workers * BATCH_PER_WORKER) {
        if generation.load(Ordering::Relaxed) != mine {
            results.superseded = true;
            break;
        }
        // One read transaction per batch; matching runs in parallel.
        let loaded = db.exchanges_bulk(batch)?;
        for hits in search_batch(q, &re, &loaded, workers)? {
            results.scanned += 1;
            if !hits.is_empty() {
                results.matched_exchanges += 1;
                results.hits.extend(hits);
            }
            if results.hits.len() >= max_hits {
                results.hits.truncate(max_hits);
                truncated = true;
                break 'batches;
            }
        }
    }
    if q.in_websockets && !results.superseded {
        truncated |= search_websockets(db, q, &re, max_hits, generation, mine, &mut results)?;
    }
    results.truncated = truncated;
    results.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(results)
}

/// WebSocket messages read per page.
const WS_PAGE: usize = 2_000;

/// Searches WebSocket payloads newest first; true if a limit stopped it.
fn search_websockets(
    db: &ProjectDb,
    q: &SearchQuery,
    re: &Regex,
    max_hits: usize,
    generation: &AtomicU64,
    mine: u64,
    results: &mut SearchResults,
) -> Result<bool, AppError> {
    let mut before = None;
    loop {
        if generation.load(Ordering::Relaxed) != mine {
            results.superseded = true;
            return Ok(false);
        }
        let page = db.ws_messages_for_search(before, WS_PAGE, q.filter.in_scope_only)?;
        let Some(last) = page.last() else {
            return Ok(false);
        };
        before = Some(last.0.id);
        for (message, url, payload) in page {
            results.ws_scanned += 1;
            if let Some((start, end, count)) = find(re, &payload) {
                let from = start.saturating_sub(CONTEXT_BYTES);
                let to = (end + CONTEXT_BYTES).min(payload.len());
                let shown_end = end.min(start + 400);
                results.ws_hits.push(WsSearchHit {
                    message,
                    url,
                    before: clean(&payload[from..start]),
                    matched: clean(&payload[start..shown_end]),
                    after: clean(&payload[shown_end..to.max(shown_end)]),
                    count,
                });
                if results.ws_hits.len() >= max_hits {
                    return Ok(true);
                }
            }
            if results.ws_scanned >= MAX_SCANNED {
                return Ok(true);
            }
        }
    }
}
