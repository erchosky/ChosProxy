//! Target analyzer (matrix TARGET-010, ENGAGE-003): what a site exposes,
//! from the traffic already stored — URLs, parameters and where they
//! appear, HTML comments and scripts. Nothing is sent.

use chosproxy_http::ResponseHead;
use chosproxy_http::content::{body_text, split_message};
use chosproxy_storage::StoredExchange;
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;
use ts_rs::TS;

/// Exchanges examined per analysis (most recent first).
pub const MAX_ANALYZED: u32 = 5000;
const MAX_LIST: usize = 500;
const MAX_TEXT: usize = 300;

/// A parameter name seen on the site.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzedParameter {
    /// Name.
    pub name: String,
    /// `query`, `body`, `json`, `cookie`.
    pub locations: Vec<String>,
    /// Distinct URLs (without query) using it.
    #[ts(type = "number")]
    pub urls: u64,
    /// A few values seen.
    pub samples: Vec<String>,
}

/// Something found in a response, with where.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzedItem {
    /// The comment text or script reference.
    pub text: String,
    /// First URL it was seen on.
    pub url: String,
    /// The exchange.
    #[ts(type = "number")]
    pub exchange_id: i64,
    /// Pages it appears on.
    #[ts(type = "number")]
    pub pages: u64,
}

/// What a site exposes.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TargetAnalysis {
    /// Exchanges examined.
    #[ts(type = "number")]
    pub exchanges: u64,
    /// Distinct URLs (without query).
    #[ts(type = "number")]
    pub urls: u64,
    /// URLs taking parameters (query or body).
    #[ts(type = "number")]
    pub dynamic_urls: u64,
    /// URLs of static files (images, styles, scripts, fonts…).
    #[ts(type = "number")]
    pub static_urls: u64,
    /// Parameters, most used first.
    pub parameters: Vec<AnalyzedParameter>,
    /// HTML comments.
    pub comments: Vec<AnalyzedItem>,
    /// External scripts (`src`) and inline scripts (start of the code).
    pub scripts: Vec<AnalyzedItem>,
    /// Lists were cut at their limit.
    pub truncated: bool,
}

static COMMENT: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?s)<!--(.*?)-->").ok());
static SCRIPT_SRC: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"(?i)<script\b[^>]*\bsrc\s*=\s*["']?([^"'\s>]+)"#).ok());
static INLINE_SCRIPT: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?is)<script\b([^>]*)>(.*?)</script>").ok());

const STATIC_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "ico", "webp", "css", "js", "mjs", "woff", "woff2", "ttf",
    "eot", "map", "mp4", "mp3", "pdf",
];

fn cut(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().take(MAX_TEXT).collect()
}

fn decode(s: &str) -> String {
    let bytes = s.replace('+', " ");
    let mut out = Vec::new();
    let b = bytes.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&bytes[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Default)]
struct ParamAcc {
    locations: BTreeSet<&'static str>,
    urls: BTreeSet<String>,
    samples: Vec<String>,
}

fn add_param(
    params: &mut HashMap<String, ParamAcc>,
    name: &str,
    value: &str,
    location: &'static str,
    url: &str,
) {
    if name.is_empty() || name.len() > 100 || params.len() >= 5000 && !params.contains_key(name) {
        return;
    }
    let p = params.entry(name.to_string()).or_default();
    p.locations.insert(location);
    p.urls.insert(url.to_string());
    let v = cut(value);
    if !v.is_empty() && p.samples.len() < 3 && !p.samples.contains(&v) {
        p.samples.push(v);
    }
}

fn pairs(text: &str) -> impl Iterator<Item = (String, String)> + '_ {
    text.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((n, v)) => (decode(n), decode(v)),
            None => (decode(p), String::new()),
        })
}

fn add_item(
    map: &mut BTreeMap<String, (AnalyzedItem, BTreeSet<String>)>,
    text: String,
    url: &str,
    id: i64,
) {
    if text.is_empty() {
        return;
    }
    if map.len() >= MAX_LIST && !map.contains_key(&text) {
        return;
    }
    let entry = map.entry(text.clone()).or_insert_with(|| {
        (
            AnalyzedItem {
                text,
                url: url.to_string(),
                exchange_id: id,
                pages: 0,
            },
            BTreeSet::new(),
        )
    });
    entry.1.insert(url.to_string());
}

/// Analyzes the exchanges of one site.
pub fn analyze(exchanges: &[StoredExchange]) -> TargetAnalysis {
    let mut urls: BTreeSet<String> = BTreeSet::new();
    let mut dynamic: BTreeSet<String> = BTreeSet::new();
    let mut statics: BTreeSet<String> = BTreeSet::new();
    let mut params: HashMap<String, ParamAcc> = HashMap::new();
    let mut comments = BTreeMap::new();
    let mut scripts = BTreeMap::new();
    for ex in exchanges {
        let row = &ex.row;
        let (path, query) = row.path.split_once('?').unwrap_or((row.path.as_str(), ""));
        let url = format!("{}://{}:{}{}", row.scheme, row.host, row.port, path);
        urls.insert(url.clone());
        let ext = path
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if STATIC_EXT.contains(&ext.as_str()) {
            statics.insert(url.clone());
        }
        let (req_head, req_body) = split_message(&ex.request);
        let head_text = String::from_utf8_lossy(req_head);
        let mut has_params = false;
        for (n, v) in pairs(query) {
            has_params = true;
            add_param(&mut params, &n, &v, "query", &url);
        }
        let content_type = head_text
            .lines()
            .filter_map(|l| l.split_once(':'))
            .find(|(n, _)| n.trim().eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.trim().to_ascii_lowercase())
            .unwrap_or_default();
        if !req_body.is_empty() {
            let body = String::from_utf8_lossy(req_body);
            if content_type.starts_with("application/x-www-form-urlencoded") {
                for (n, v) in pairs(body.trim()) {
                    has_params = true;
                    add_param(&mut params, &n, &v, "body", &url);
                }
            } else if content_type.contains("json")
                && let Ok(serde_json::Value::Object(map)) =
                    serde_json::from_str::<serde_json::Value>(&body)
            {
                for (n, v) in map {
                    has_params = true;
                    let v = v.as_str().map_or_else(|| v.to_string(), str::to_string);
                    add_param(&mut params, &n, &v, "json", &url);
                }
            }
        }
        for line in head_text.lines() {
            if let Some((n, v)) = line.split_once(':')
                && n.trim().eq_ignore_ascii_case("cookie")
            {
                for c in v.split(';') {
                    if let Some((cn, cv)) = c.trim().split_once('=') {
                        add_param(&mut params, cn.trim(), cv.trim(), "cookie", &url);
                    }
                }
            }
        }
        if has_params {
            dynamic.insert(url.clone());
        }
        let Some(response) = &ex.response else {
            continue;
        };
        let (head, body) = split_message(response);
        let Ok(head) = ResponseHead::parse(head.to_vec()) else {
            continue;
        };
        let is_markup = row
            .mime
            .as_deref()
            .is_some_and(|m| m == "HTML" || m == "XML" || m == "SVG");
        if !is_markup {
            continue;
        }
        let Some(text) = body_text(&head, body, 2 * 1024 * 1024) else {
            continue;
        };
        if let Some(re) = COMMENT.as_ref() {
            for c in re.captures_iter(&text).take(200) {
                if let Some(m) = c.get(1) {
                    add_item(&mut comments, cut(m.as_str()), &row.url, row.id);
                }
            }
        }
        if let Some(re) = SCRIPT_SRC.as_ref() {
            for c in re.captures_iter(&text).take(200) {
                if let Some(m) = c.get(1) {
                    add_item(
                        &mut scripts,
                        format!("src: {}", cut(m.as_str())),
                        &row.url,
                        row.id,
                    );
                }
            }
        }
        if let Some(re) = INLINE_SCRIPT.as_ref() {
            for c in re.captures_iter(&text).take(200) {
                let attrs = c.get(1).map_or("", |m| m.as_str());
                let code = c.get(2).map_or("", |m| m.as_str()).trim();
                if !attrs.to_ascii_lowercase().contains("src") && !code.is_empty() {
                    add_item(
                        &mut scripts,
                        format!("inline: {}", cut(code)),
                        &row.url,
                        row.id,
                    );
                }
            }
        }
    }
    let truncated = comments.len() >= MAX_LIST || scripts.len() >= MAX_LIST || params.len() >= 5000;
    let mut parameters: Vec<AnalyzedParameter> = params
        .into_iter()
        .map(|(name, p)| AnalyzedParameter {
            name,
            locations: p.locations.into_iter().map(str::to_string).collect(),
            urls: p.urls.len() as u64,
            samples: p.samples,
        })
        .collect();
    parameters.sort_by(|a, b| b.urls.cmp(&a.urls).then_with(|| a.name.cmp(&b.name)));
    parameters.truncate(MAX_LIST);
    let finish = |m: BTreeMap<String, (AnalyzedItem, BTreeSet<String>)>| -> Vec<AnalyzedItem> {
        let mut v: Vec<AnalyzedItem> = m
            .into_values()
            .map(|(mut item, pages)| {
                item.pages = pages.len() as u64;
                item
            })
            .collect();
        v.sort_by(|a, b| b.pages.cmp(&a.pages).then_with(|| a.text.cmp(&b.text)));
        v
    };
    TargetAnalysis {
        exchanges: exchanges.len() as u64,
        urls: urls.len() as u64,
        dynamic_urls: dynamic.len() as u64,
        static_urls: statics.len() as u64,
        parameters,
        comments: finish(comments),
        scripts: finish(scripts),
        truncated,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use chosproxy_core::Source;
    use chosproxy_storage::HistoryRow;

    fn ex(id: i64, path: &str, request: &str, response: &str, mime: &str) -> StoredExchange {
        StoredExchange {
            row: HistoryRow {
                id,
                started_at: 1,
                duration_ms: None,
                source: Source::Proxy,
                scheme: "https".into(),
                method: "GET".into(),
                host: "shop.test".into(),
                port: 443,
                path: path.into(),
                extension: None,
                url: format!("https://shop.test{path}"),
                http_version: "HTTP/1.1".into(),
                status: Some(200),
                mime: Some(mime.into()),
                request_len: 0,
                response_len: None,
                server_ip: None,
                tls: true,
                error: None,
                notes: None,
                request_truncated: false,
                response_truncated: false,
                cl_te_conflict: false,
                upgraded: false,
                edited: false,
                dropped: false,
                highlight: None,
                in_scope: false,
            },
            client_addr: None,
            listener: None,
            reason: None,
            request: request.as_bytes().to_vec(),
            response: Some(response.as_bytes().to_vec()),
            original_request: None,
            original_response: None,
        }
    }

    #[test]
    fn finds_parameters_comments_and_scripts() {
        let page = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><!-- TODO: remove debug=1 -->\
            <script src=\"/js/app.js\"></script><script>var apiKey = 'x';</script></html>";
        let exchanges = vec![
            ex(
                1,
                "/search?q=shoes&page=2",
                "GET /search?q=shoes&page=2 HTTP/1.1\r\nCookie: sid=abc\r\n\r\n",
                page,
                "HTML",
            ),
            ex(
                2,
                "/search?q=hats",
                "GET /search?q=hats HTTP/1.1\r\n\r\n",
                page,
                "HTML",
            ),
            ex(
                3,
                "/api/cart",
                "POST /api/cart HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{\"item\":7,\"qty\":\"2\"}",
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}",
                "JSON",
            ),
            ex(
                4,
                "/img/logo.png",
                "GET /img/logo.png HTTP/1.1\r\n\r\n",
                "HTTP/1.1 200 OK\r\n\r\n",
                "image",
            ),
        ];
        let a = analyze(&exchanges);
        assert_eq!(
            (a.exchanges, a.urls, a.dynamic_urls, a.static_urls),
            (4, 3, 2, 1)
        );
        let q = a.parameters.iter().find(|p| p.name == "q").unwrap();
        assert_eq!(
            (q.locations.clone(), q.urls, q.samples.len()),
            (vec!["query".to_string()], 1, 2)
        );
        assert!(
            a.parameters
                .iter()
                .any(|p| p.name == "qty" && p.locations == vec!["json".to_string()])
        );
        assert!(
            a.parameters
                .iter()
                .any(|p| p.name == "sid" && p.locations == vec!["cookie".to_string()])
        );
        assert_eq!(a.comments[0].text, "TODO: remove debug=1");
        assert_eq!(a.comments[0].pages, 2);
        assert!(a.scripts.iter().any(|s| s.text == "src: /js/app.js"));
        assert!(
            a.scripts
                .iter()
                .any(|s| s.text.starts_with("inline: var apiKey"))
        );
    }
}
