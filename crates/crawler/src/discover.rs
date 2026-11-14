//! What a response points to: links, scripts, forms, redirects, URLs in
//! JavaScript, robots.txt and sitemap entries.
//!
//! HTML is read with tolerant patterns rather than a full parser: a crawler
//! needs the URLs, and real pages are often not well formed.

use chosproxy_http::ResponseHead;
use chosproxy_http::head::Headers;
use regex::Regex;
use std::sync::LazyLock;
use url::Url;

/// What kind of reference a URL came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FoundKind {
    /// A link or frame to navigate to.
    Link,
    /// A stylesheet, image or media file (not crawled).
    Resource,
    /// A script (crawled to look for more URLs in it).
    Script,
    /// A form; `method` says how it is submitted.
    Form,
    /// A redirect (Location, Refresh, meta refresh).
    Redirect,
    /// A path or URL written in JavaScript.
    JsEndpoint,
    /// An entry of robots.txt or a sitemap.
    Robots,
}

impl FoundKind {
    /// Short label for logs.
    pub fn label(self) -> &'static str {
        match self {
            FoundKind::Link => "link",
            FoundKind::Resource => "resource",
            FoundKind::Script => "script",
            FoundKind::Form => "form",
            FoundKind::Redirect => "redirect",
            FoundKind::JsEndpoint => "JavaScript",
            FoundKind::Robots => "robots/sitemap",
        }
    }
}

/// A URL found in a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Absolute URL (fragment removed).
    pub url: Url,
    /// Where it came from.
    pub kind: FoundKind,
    /// `GET`, or the method of a form.
    pub method: String,
}

/// Paths fetched at the start of a crawl of an origin, for discovery.
pub fn robots_and_sitemap_paths() -> &'static [&'static str] {
    &["/robots.txt", "/sitemap.xml"]
}

/// Most URLs taken from one response.
const MAX_FOUND: usize = 2000;

fn re(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

static BASE: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r#"(?is)<base\b[^>]*?\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#));
static TAG: LazyLock<Option<Regex>> = LazyLock::new(|| {
    re(r"(?is)<(a|area|link|script|img|iframe|frame|source|embed|video|audio)\b([^>]*)>")
});
static URL_ATTR: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r#"(?is)\b(?:href|src)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#));
static META_REFRESH: LazyLock<Option<Regex>> = LazyLock::new(|| {
    re(
        r#"(?is)<meta\b[^>]*http-equiv\s*=\s*["']?refresh["']?[^>]*content\s*=\s*["']?\s*\d*\s*;?\s*url\s*=\s*([^"'>\s]+)"#,
    )
});
static FORM: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"(?is)<form\b([^>]*)>(.*?)</form>"));
static FIELD: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"(?is)<(input|textarea|select)\b([^>]*)>"));
static INLINE_SCRIPT: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"(?is)<script\b[^>]*>(.*?)</script>"));
static JS_PATH: LazyLock<Option<Regex>> = LazyLock::new(|| {
    re(r#"["'`](/[A-Za-z0-9_\-.~%]+(?:/[A-Za-z0-9_\-.~%]*)*(?:\?[A-Za-z0-9_\-.~%=&]*)?)["'`]"#)
});
static JS_URL: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r#"["'`](https?://[^\s"'`<>\\]+)["'`]"#));
static ROBOTS_LINE: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"(?im)^\s*(allow|disallow|sitemap)\s*:\s*(\S+)"));
static LOC: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"(?is)<loc>\s*([^<\s]+)\s*</loc>"));

/// The value of `name` in an HTML tag's attribute text.
fn attr(attrs: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"(?is)\b{name}\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#);
    let c = Regex::new(&pattern).ok()?.captures(attrs)?;
    let v = c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3))?.as_str();
    Some(unescape(v))
}

/// The few HTML entities that appear in URLs.
fn unescape(v: &str) -> String {
    v.trim()
        .replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&#x2F;", "/")
        .replace("&#47;", "/")
        .replace("&quot;", "\"")
}

/// Resolves a reference against `base`; `None` for non-navigable schemes.
fn resolve(base: &Url, reference: &str) -> Option<Url> {
    let r = reference.trim();
    let lower = r.to_ascii_lowercase();
    if r.is_empty()
        || r.starts_with('#')
        || [
            "javascript:",
            "mailto:",
            "tel:",
            "data:",
            "about:",
            "blob:",
            "sms:",
        ]
        .iter()
        .any(|s| lower.starts_with(s))
    {
        return None;
    }
    let mut url = base.join(r).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }
    url.set_fragment(None);
    Some(url)
}

fn push(out: &mut Vec<Found>, base: &Url, reference: &str, kind: FoundKind, method: &str) {
    if out.len() >= MAX_FOUND {
        return;
    }
    if let Some(url) = resolve(base, reference) {
        out.push(Found {
            url,
            kind,
            method: method.to_string(),
        });
    }
}

/// A form's fields with values a browser would send by default (or a
/// harmless placeholder for empty text fields).
fn form_query(body: &str) -> Vec<(String, String)> {
    let Some(field) = FIELD.as_ref() else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    for c in field.captures_iter(body).take(100) {
        let tag = c.get(1).map_or("", |m| m.as_str()).to_ascii_lowercase();
        let attrs = c.get(2).map_or("", |m| m.as_str());
        let Some(name) = attr(attrs, "name").filter(|n| !n.is_empty()) else {
            continue;
        };
        let kind = attr(attrs, "type").unwrap_or_default().to_ascii_lowercase();
        let value = attr(attrs, "value").unwrap_or_default();
        let value = match (tag.as_str(), kind.as_str()) {
            ("input", "submit" | "button" | "image" | "reset" | "file" | "password") => continue,
            ("input", "checkbox" | "radio") => {
                if !attrs.to_ascii_lowercase().contains("checked") {
                    continue;
                }
                if value.is_empty() { "on".into() } else { value }
            }
            ("input", "email") if value.is_empty() => "test@example.com".into(),
            ("input", "number" | "range") if value.is_empty() => "1".into(),
            ("input", _) if value.is_empty() && kind != "hidden" => "test".into(),
            ("textarea", _) => "test".into(),
            _ => value,
        };
        pairs.push((name, value));
    }
    pairs
}

fn query_string(pairs: &[(String, String)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .finish()
}

fn javascript(out: &mut Vec<Found>, base: &Url, code: &str) {
    if let Some(r) = JS_PATH.as_ref() {
        for c in r.captures_iter(code).take(500) {
            let path = c.get(1).map_or("", |m| m.as_str());
            // Skip regular expressions and comments that look like paths.
            if path.len() > 1 && !path.starts_with("//") && !path.contains("/*") {
                push(out, base, path, FoundKind::JsEndpoint, "GET");
            }
        }
    }
    if let Some(r) = JS_URL.as_ref() {
        for c in r.captures_iter(code).take(500) {
            let u = c.get(1).map_or("", |m| m.as_str());
            if !u.contains("${") {
                push(out, base, u, FoundKind::JsEndpoint, "GET");
            }
        }
    }
}

fn html(out: &mut Vec<Found>, page: &Url, body: &str, submit_get_forms: bool) {
    let base = BASE
        .as_ref()
        .and_then(|r| r.captures(body))
        .and_then(|c| c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)))
        .and_then(|m| resolve(page, &unescape(m.as_str())))
        .unwrap_or_else(|| page.clone());
    if let (Some(tag), Some(url_attr)) = (TAG.as_ref(), URL_ATTR.as_ref()) {
        for c in tag.captures_iter(body) {
            let name = c.get(1).map_or("", |m| m.as_str()).to_ascii_lowercase();
            let attrs = c.get(2).map_or("", |m| m.as_str());
            let Some(value) = url_attr
                .captures(attrs)
                .and_then(|a| a.get(1).or_else(|| a.get(2)).or_else(|| a.get(3)))
            else {
                continue;
            };
            let kind = match name.as_str() {
                "a" | "area" | "iframe" | "frame" => FoundKind::Link,
                "script" => FoundKind::Script,
                _ => FoundKind::Resource,
            };
            push(out, &base, &unescape(value.as_str()), kind, "GET");
        }
    }
    if let Some(m) = META_REFRESH
        .as_ref()
        .and_then(|r| r.captures(body))
        .and_then(|c| c.get(1))
    {
        push(
            out,
            &base,
            &unescape(m.as_str()),
            FoundKind::Redirect,
            "GET",
        );
    }
    if let Some(form) = FORM.as_ref() {
        for c in form.captures_iter(body).take(100) {
            let attrs = c.get(1).map_or("", |m| m.as_str());
            let inner = c.get(2).map_or("", |m| m.as_str());
            let method = attr(attrs, "method")
                .unwrap_or_else(|| "get".into())
                .to_ascii_uppercase();
            let action = attr(attrs, "action").unwrap_or_default();
            let action = if action.is_empty() {
                base.as_str().to_string()
            } else {
                action
            };
            let Some(mut target) = resolve(&base, &action) else {
                continue;
            };
            if method == "GET" {
                if !submit_get_forms {
                    continue;
                }
                target.set_query(Some(&query_string(&form_query(inner))));
            }
            if out.len() < MAX_FOUND {
                out.push(Found {
                    url: target,
                    kind: FoundKind::Form,
                    method,
                });
            }
        }
    }
    if let Some(script) = INLINE_SCRIPT.as_ref() {
        for c in script.captures_iter(body).take(200) {
            javascript(out, &base, c.get(1).map_or("", |m| m.as_str()));
        }
    }
}

/// The URLs a response points to. `page` is the URL that was requested;
/// `body` is the decoded response body ("" if none or binary).
pub fn discover(page: &Url, head: &ResponseHead, body: &str, submit_get_forms: bool) -> Vec<Found> {
    let mut out = Vec::new();
    if (300..400).contains(&head.status)
        && let Some(location) = head.header_str("location")
    {
        push(&mut out, page, &location, FoundKind::Redirect, "GET");
    }
    if let Some(refresh) = head.header_str("refresh")
        && let Some((_, target)) = refresh.to_ascii_lowercase().split_once("url=")
    {
        // Keep the original case of the URL.
        let start = refresh.len() - target.len();
        push(
            &mut out,
            page,
            refresh[start..].trim(),
            FoundKind::Redirect,
            "GET",
        );
    }
    if body.is_empty() {
        return out;
    }
    let media = head
        .header_str("content-type")
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let path = page.path().to_ascii_lowercase();
    if path == "/robots.txt" {
        if let Some(r) = ROBOTS_LINE.as_ref() {
            for c in r.captures_iter(body).take(500) {
                let field = c.get(1).map_or("", |m| m.as_str()).to_ascii_lowercase();
                let value = c.get(2).map_or("", |m| m.as_str());
                // Patterns: keep the literal prefix before any wildcard.
                let literal = value.split(['*', '$']).next().unwrap_or_default();
                if field == "sitemap" || literal.len() > 1 {
                    push(&mut out, page, literal, FoundKind::Robots, "GET");
                }
            }
        }
    } else if media.contains("xml") && (body.contains("<urlset") || body.contains("<sitemapindex"))
    {
        if let Some(r) = LOC.as_ref() {
            for c in r.captures_iter(body).take(MAX_FOUND) {
                push(
                    &mut out,
                    page,
                    &unescape(c.get(1).map_or("", |m| m.as_str())),
                    FoundKind::Robots,
                    "GET",
                );
            }
        }
    } else if media == "text/html" || media == "application/xhtml+xml" {
        html(&mut out, page, body, submit_get_forms);
    } else if media.contains("javascript") || path.ends_with(".js") {
        javascript(&mut out, page, body);
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn head(raw: &str) -> ResponseHead {
        ResponseHead::parse(raw.as_bytes().to_vec()).unwrap()
    }

    fn urls(found: &[Found], kind: FoundKind) -> Vec<String> {
        found
            .iter()
            .filter(|f| f.kind == kind)
            .map(|f| f.url.to_string())
            .collect()
    }

    #[test]
    fn links_scripts_forms_and_inline_javascript() {
        let page = Url::parse("https://shop.example/catalog/index.html").unwrap();
        let body = r#"<html><head><base href="/app/">
            <script src="js/main.js"></script><link rel=stylesheet href='/s.css'></head>
            <a href="item?id=1&amp;x=2#top">Item</a> <a href="mailto:a@b.c">m</a>
            <a href=javascript:void(0)>j</a> <a href="https://other.example/x">o</a>
            <iframe src="/frame"></iframe>
            <form action="/search" method="get"><input name="q"><input type="hidden" name="cat" value="7">
              <input type="checkbox" name="new" checked><input type="submit" name="go"></form>
            <form method="POST" action="/login"><input name="user"><input type="password" name="p"></form>
            <script>fetch('/api/v1/cart?full=1'); const u = "https://cdn.example/lib.js"; x.replace(/\/\//g, '')</script>
            <meta http-equiv="refresh" content="5; url=/next">"#;
        let found = discover(
            &page,
            &head("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n"),
            body,
            true,
        );
        assert_eq!(
            urls(&found, FoundKind::Link),
            vec![
                "https://shop.example/app/item?id=1&x=2",
                "https://other.example/x",
                "https://shop.example/frame"
            ]
        );
        assert_eq!(
            urls(&found, FoundKind::Script),
            vec!["https://shop.example/app/js/main.js"]
        );
        assert_eq!(
            urls(&found, FoundKind::Resource),
            vec!["https://shop.example/s.css"]
        );
        let forms: Vec<(String, String)> = found
            .iter()
            .filter(|f| f.kind == FoundKind::Form)
            .map(|f| (f.method.clone(), f.url.to_string()))
            .collect();
        assert_eq!(
            forms,
            vec![
                (
                    "GET".into(),
                    "https://shop.example/search?q=test&cat=7&new=on".into()
                ),
                ("POST".into(), "https://shop.example/login".into()),
            ]
        );
        assert_eq!(
            urls(&found, FoundKind::JsEndpoint),
            vec![
                "https://shop.example/api/v1/cart?full=1",
                "https://cdn.example/lib.js"
            ]
        );
        assert_eq!(
            urls(&found, FoundKind::Redirect),
            vec!["https://shop.example/next"]
        );

        let without_forms = discover(
            &page,
            &head("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n"),
            body,
            false,
        );
        assert_eq!(
            without_forms
                .iter()
                .filter(|f| f.kind == FoundKind::Form)
                .count(),
            1,
            "POST forms are always reported, GET forms only when submitted"
        );
    }

    #[test]
    fn redirects_robots_sitemaps_and_scripts() {
        let page = Url::parse("http://lab.test/old").unwrap();
        let found = discover(
            &page,
            &head("HTTP/1.1 302 Found\r\nLocation: /new?a=1\r\n\r\n"),
            "",
            true,
        );
        assert_eq!(
            urls(&found, FoundKind::Redirect),
            vec!["http://lab.test/new?a=1"]
        );

        let robots = Url::parse("http://lab.test/robots.txt").unwrap();
        let found = discover(
            &robots,
            &head("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n"),
            "User-agent: *\nDisallow: /admin/\nDisallow: /*.php$\nAllow: /public\nDisallow: /\nSitemap: http://lab.test/sm.xml\n",
            true,
        );
        assert_eq!(
            urls(&found, FoundKind::Robots),
            vec![
                "http://lab.test/admin/",
                "http://lab.test/public",
                "http://lab.test/sm.xml"
            ]
        );

        let sitemap = Url::parse("http://lab.test/sm.xml").unwrap();
        let found = discover(
            &sitemap,
            &head("HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\n\r\n"),
            "<urlset><url><loc> http://lab.test/a?x=1&amp;y=2 </loc></url></urlset>",
            true,
        );
        assert_eq!(
            urls(&found, FoundKind::Robots),
            vec!["http://lab.test/a?x=1&y=2"]
        );

        let script = Url::parse("http://lab.test/static/app.js").unwrap();
        let found = discover(
            &script,
            &head("HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\n\r\n"),
            "api.get(`/api/users`); const re = '/'; load(\"/static/chunk.js\")",
            true,
        );
        assert_eq!(
            urls(&found, FoundKind::JsEndpoint),
            vec![
                "http://lab.test/api/users",
                "http://lab.test/static/chunk.js"
            ]
        );
    }
}
