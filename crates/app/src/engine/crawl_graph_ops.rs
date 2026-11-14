//! The crawl graph (M31b, matrix TARGET-007): the requests of a crawl as
//! nodes, joined by where each URL was found. It is read from the notes the
//! crawler writes on its requests ("Crawler: a link on #12 (depth 2).").

use super::Engine;
use crate::AppError;
use chosproxy_storage::TaskId;
use serde::Serialize;
use ts_rs::TS;

/// Most requests of one graph.
pub const MAX_GRAPH_NODES: u32 = 1_500;

/// A request the crawl made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CrawlNode {
    /// History id.
    #[ts(type = "number")]
    pub id: i64,
    /// URL requested.
    pub url: String,
    /// Response status, if any.
    pub status: Option<u16>,
    /// Links followed from a start URL to get here.
    pub depth: u32,
    /// Why it was requested ("start URL", "a link", "a GET form"…).
    pub reason: String,
}

/// "`to` was found on `from`".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CrawlEdge {
    /// The page it was found on (history id).
    #[ts(type = "number")]
    pub from: i64,
    /// The request made because of it (history id).
    #[ts(type = "number")]
    pub to: i64,
}

/// The requests of a crawl and how they led to each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CrawlGraph {
    /// Requests, oldest first.
    pub nodes: Vec<CrawlNode>,
    /// Found-on links between them.
    pub edges: Vec<CrawlEdge>,
    /// There were more requests than a graph holds.
    pub truncated: bool,
}

/// What a crawler note says: the reason, the page it was found on and the
/// depth. `None` for notes the crawler did not write.
pub(super) fn parse_crawl_note(notes: &str) -> Option<(String, Option<i64>, u32)> {
    let line = notes
        .lines()
        .find_map(|l| l.trim().strip_prefix("Crawler: "))?;
    let number_after = |marker: &str| {
        let rest = &line[line.find(marker)? + marker.len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<i64>().ok()
    };
    let via = number_after("#");
    let depth = number_after("(depth ")
        .and_then(|d| u32::try_from(d).ok())
        .unwrap_or(0);
    let reason = line
        .split(" on #")
        .next()
        .unwrap_or(line)
        .split(" from #")
        .next()
        .unwrap_or(line)
        .trim_end_matches('.')
        .to_string();
    Some((reason, via, depth))
}

impl Engine {
    /// The graph of a crawl or scan task: its requests (made while the
    /// task ran) and where each was found.
    pub async fn crawl_graph(&self, task: TaskId) -> Result<CrawlGraph, AppError> {
        let db = self.db()?;
        tokio::task::spawn_blocking(move || {
            let row = db
                .task(task)?
                .ok_or_else(|| AppError::Invalid(format!("no task #{task}")))?;
            let from = row.started_at.unwrap_or(row.created_at);
            let to = row.finished_at.unwrap_or(i64::MAX);
            let rows = db.crawl_exchanges(from, to, MAX_GRAPH_NODES + 1)?;
            let truncated = rows.len() > MAX_GRAPH_NODES as usize;
            let mut nodes = Vec::new();
            let mut edges = Vec::new();
            for (id, url, status, notes) in rows.into_iter().take(MAX_GRAPH_NODES as usize) {
                let Some((reason, via, depth)) = parse_crawl_note(&notes) else {
                    continue;
                };
                if let Some(from) = via {
                    edges.push(CrawlEdge { from, to: id });
                }
                nodes.push(CrawlNode {
                    id,
                    url,
                    status,
                    depth,
                    reason,
                });
            }
            // Only links between requests of this graph.
            let known: std::collections::HashSet<i64> = nodes.iter().map(|n| n.id).collect();
            edges.retain(|e| known.contains(&e.from));
            Ok(CrawlGraph {
                nodes,
                edges,
                truncated,
            })
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::parse_crawl_note;

    #[test]
    fn reads_the_crawler_notes() {
        assert_eq!(
            parse_crawl_note("Crawler: start URL."),
            Some(("start URL".into(), None, 0))
        );
        assert_eq!(
            parse_crawl_note("Something else.\nCrawler: a link on #12 (depth 2)."),
            Some(("a link".into(), Some(12), 2))
        );
        assert_eq!(
            parse_crawl_note("Crawler: redirected from #7 (depth 1)."),
            Some(("redirected".into(), Some(7), 1))
        );
        assert_eq!(
            parse_crawl_note("Crawler: robots.txt or sitemap of a start site."),
            Some(("robots.txt or sitemap of a start site".into(), None, 0))
        );
        assert_eq!(parse_crawl_note("Sent from Repeater."), None);
    }
}
