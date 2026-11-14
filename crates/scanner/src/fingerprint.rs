//! Structural fingerprints: requests that differ only in their ids are the
//! same item.
//!
//! `/users/42/orders?page=2` and `/users/7/orders?page=9` reach the same
//! code; crawling or auditing both wastes requests. A fingerprint keeps the
//! method, the origin, the path with variable-looking segments folded
//! (numbers, UUIDs, hashes, long tokens) and the sorted parameter *names*.

use crate::insertion::{PointKind, insertion_points};

/// What a path segment looks like when it is a value, not a name.
fn fold_segment(segment: &str) -> Option<&'static str> {
    if segment.is_empty() {
        return None;
    }
    let (stem, _) = segment.rsplit_once('.').unwrap_or((segment, ""));
    if stem.bytes().all(|b| b.is_ascii_digit()) && !stem.is_empty() {
        return Some("{n}");
    }
    if is_uuid(stem) {
        return Some("{uuid}");
    }
    if stem.len() >= 16 && stem.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some("{hex}");
    }
    let digits = stem.bytes().filter(u8::is_ascii_digit).count();
    let letters = stem.bytes().filter(u8::is_ascii_alphabetic).count();
    if stem.len() >= 20
        && digits > 0
        && letters > 0
        && stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Some("{token}");
    }
    None
}

fn is_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// The path with its variable-looking segments replaced by placeholders
/// (`/users/{n}/orders`).
pub fn fold_path(path: &str) -> String {
    path.split('/')
        .map(|segment| fold_segment(segment).unwrap_or(segment))
        .collect::<Vec<_>>()
        .join("/")
}

/// The parameter names of a query string, sorted and without repeats.
pub fn query_names(query: &str) -> Vec<String> {
    let mut names: Vec<String> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| p.split_once('=').map_or(p, |(n, _)| n).to_string())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The fingerprint of a URL fetched with `method`. `origin` is
/// `scheme://host:port`.
pub fn url_fingerprint(method: &str, origin: &str, path: &str, query: Option<&str>) -> String {
    let names = query.map(query_names).unwrap_or_default();
    format!(
        "{} {}{}?{}",
        method.to_ascii_uppercase(),
        origin.to_ascii_lowercase(),
        fold_path(path),
        names.join("&")
    )
}

/// The fingerprint of a request in text form: method, `Host`, folded path,
/// and the names of its query, form, JSON, multipart and XML parameters.
pub fn request_fingerprint(text: &str) -> String {
    let first = text.lines().next().unwrap_or_default();
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or_default().to_ascii_uppercase();
    let target = parts.next().unwrap_or_default();
    let host = text
        .lines()
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("host")
                .then(|| value.trim().to_ascii_lowercase())
        })
        .unwrap_or_default();
    let path = target.split(['?', '#']).next().unwrap_or_default();
    let kinds = [
        PointKind::Query,
        PointKind::Form,
        PointKind::Json,
        PointKind::Multipart,
        PointKind::Xml,
    ];
    let mut names: Vec<String> = insertion_points(text, &kinds)
        .into_iter()
        .map(|p| {
            // `items[3].id` and `items[0].id` are the same field.
            let name = fold_indexes(&p.name);
            format!("{}:{name}", kind_tag(p.kind))
        })
        .collect();
    names.sort();
    names.dedup();
    format!("{method} {host}{}?{}", fold_path(path), names.join("&"))
}

fn kind_tag(kind: PointKind) -> &'static str {
    match kind {
        PointKind::PathSegment => "p",
        PointKind::Query => "q",
        PointKind::Form => "f",
        PointKind::Cookie => "c",
        PointKind::Header => "h",
        PointKind::Json => "j",
        PointKind::Multipart => "m",
        PointKind::Xml => "x",
    }
}

/// `a[12].b[3]` → `a[].b[]`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_ids_uuids_hashes_and_tokens() {
        assert_eq!(fold_path("/users/42/orders"), "/users/{n}/orders");
        assert_eq!(fold_path("/doc/7.pdf"), "/doc/{n}");
        assert_eq!(
            fold_path("/o/123e4567-e89b-12d3-a456-426614174000/x"),
            "/o/{uuid}/x"
        );
        assert_eq!(fold_path("/f/d41d8cd98f00b204e9800998ecf8427e"), "/f/{hex}");
        assert_eq!(fold_path("/s/AbCdEf0123456789xyzQ"), "/s/{token}");
        // Names stay: words, short hex-looking words, versions.
        assert_eq!(fold_path("/api/v2/beef/login"), "/api/v2/beef/login");
        assert_eq!(fold_path("/"), "/");
    }

    #[test]
    fn same_shape_same_fingerprint() {
        let a = url_fingerprint("get", "http://Lab:80", "/item/1", Some("id=1&b=2"));
        let b = url_fingerprint("GET", "http://lab:80", "/item/99", Some("b=x&id=7&id=8"));
        assert_eq!(a, b);
        assert_eq!(a, "GET http://lab:80/item/{n}?b&id");
        assert_ne!(
            a,
            url_fingerprint("POST", "http://lab:80", "/item/1", Some("id=1&b=2"))
        );
        assert_ne!(
            a,
            url_fingerprint("GET", "http://lab:80", "/item/1", Some("id=1"))
        );
    }

    #[test]
    fn request_fingerprints_use_body_parameter_names() {
        let a = "POST /api/u/5?x=1 HTTP/1.1\nHost: Lab\nContent-Type: application/json\n\n\
                 {\"name\":\"a\",\"tags\":[{\"id\":1},{\"id\":2}]}";
        let b = "POST /api/u/6?x=2 HTTP/1.1\nHost: lab\nContent-Type: application/json\n\n\
                 {\"tags\":[{\"id\":9}],\"name\":\"b\"}";
        assert_eq!(request_fingerprint(a), request_fingerprint(b));
        assert_eq!(
            request_fingerprint(a),
            "POST lab/api/u/{n}?j:name&j:tags[].id&q:x"
        );
        let c = "POST /api/u/6?x=2 HTTP/1.1\nHost: lab\n\n{\"other\":1}";
        assert_ne!(request_fingerprint(a), request_fingerprint(c));
    }
}
