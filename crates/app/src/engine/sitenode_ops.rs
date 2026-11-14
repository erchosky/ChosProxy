//! Site map node details (M27, matrix TARGET-003): the parameters seen in
//! a node's requests (from the insertion-point parser) and its latest
//! requests, for the site map's detail panel. Reads stored traffic only.

use super::Engine;
use crate::AppError;
use chosproxy_scanner::PointKind;
use chosproxy_scanner::insertion::insertion_points;
use chosproxy_storage::{HistoryOrder, HistoryQuery, HistoryRow, SiteFilter};
use serde::Serialize;
use std::collections::BTreeMap;
use ts_rs::TS;

/// Requests read to collect parameter names.
const SAMPLED: u32 = 200;
/// Requests listed inline.
const LISTED: usize = 50;
/// Parameters listed at most.
const MAX_PARAMS: usize = 300;
/// Longest example value shown.
const EXAMPLE_CHARS: usize = 120;

/// A parameter name seen under a node.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NodeParam {
    /// Name (JSON paths with indexes as `[]`).
    pub name: String,
    /// Where it was seen.
    pub kinds: Vec<PointKind>,
    /// In how many of the sampled requests.
    pub count: u32,
    /// One value seen (shortened).
    pub example: String,
}

/// What the site map shows for a node.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SiteNodeDetails {
    /// Parameters, most frequent first.
    pub params: Vec<NodeParam>,
    /// The latest requests under the node.
    pub requests: Vec<HistoryRow>,
    /// Requests read to collect the parameters.
    pub sampled: u32,
    /// Requests under the node in all.
    #[ts(type = "number")]
    pub total: u64,
}

/// `items[3].id` → `items[].id`.
fn fold_indexes(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut in_index = false;
    for c in name.chars() {
        match c {
            '[' => {
                in_index = true;
                out.push('[');
            }
            ']' => {
                in_index = false;
                out.push(']');
            }
            c if in_index && c.is_ascii_digit() => {}
            c => out.push(c),
        }
    }
    out
}

impl Engine {
    /// Parameters and latest requests of a site map node.
    pub async fn site_node_details(&self, site: SiteFilter) -> Result<SiteNodeDetails, AppError> {
        let page = self
            .history_page(HistoryQuery {
                limit: SAMPLED,
                order: HistoryOrder::Desc,
                site: Some(site),
                ..Default::default()
            })
            .await?;
        let ids: Vec<i64> = page.rows.iter().map(|r| r.id).collect();
        let sampled = u32::try_from(ids.len()).unwrap_or(u32::MAX);
        let db = self.db()?;
        let params = tokio::task::spawn_blocking(move || {
            let exchanges = db.exchanges_bulk(&ids)?;
            let kinds = [
                PointKind::Query,
                PointKind::Form,
                PointKind::Cookie,
                PointKind::Json,
                PointKind::Multipart,
                PointKind::Xml,
            ];
            let mut seen: BTreeMap<String, NodeParam> = BTreeMap::new();
            for ex in &exchanges {
                let text = String::from_utf8_lossy(&ex.request);
                let mut in_this: Vec<String> = Vec::new();
                for p in insertion_points(&text, &kinds) {
                    let name = fold_indexes(&p.name);
                    let entry = seen.entry(name.clone()).or_insert_with(|| NodeParam {
                        name: name.clone(),
                        kinds: Vec::new(),
                        count: 0,
                        example: p.value.chars().take(EXAMPLE_CHARS).collect(),
                    });
                    if !entry.kinds.contains(&p.kind) {
                        entry.kinds.push(p.kind);
                    }
                    if !in_this.contains(&name) {
                        entry.count += 1;
                        in_this.push(name);
                    }
                }
            }
            let mut list: Vec<NodeParam> = seen.into_values().collect();
            list.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
            list.truncate(MAX_PARAMS);
            Ok::<_, chosproxy_storage::StorageError>(list)
        })
        .await??;
        let mut requests = page.rows;
        requests.truncate(LISTED);
        Ok(SiteNodeDetails {
            params,
            requests,
            sampled,
            total: page.total,
        })
    }
}
