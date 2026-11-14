//! Insertion points: the places in a request where a tester's input goes.
//!
//! [`insertion_points`] reads a request in its text form (as the editors
//! show it) and lists every value a tester may want to vary: URL path
//! segments, query and form parameters, cookies, a few chosen headers, JSON
//! values at any depth, multipart fields and XML text and attributes. Each
//! point carries its byte range in the text, its current value and the
//! encoding its context needs, so a value can be put back without breaking
//! the request ([`replace`]), or the points can be turned into Fuzzer
//! positions ([`mark`]).
//!
//! This is parsing only: nothing here sends anything or chooses payloads.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most points listed for one request.
pub const MAX_POINTS: usize = 200;

/// Deepest JSON or XML nesting followed.
const MAX_DEPTH: usize = 32;

/// Headers whose value is offered as an insertion point.
const HEADERS: [&str; 5] = [
    "user-agent",
    "referer",
    "origin",
    "x-forwarded-for",
    "x-forwarded-host",
];

/// Where an insertion point is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PointKind {
    /// One segment of the URL path.
    PathSegment,
    /// A query string parameter value.
    Query,
    /// A URL-encoded form parameter value.
    Form,
    /// A cookie value.
    Cookie,
    /// The value of a chosen header (User-Agent, Referer, Origin, …).
    Header,
    /// A JSON value (string, number, boolean or null).
    Json,
    /// A multipart field value or file name.
    Multipart,
    /// XML element text or attribute value.
    Xml,
}

impl PointKind {
    /// Every kind.
    pub const ALL: [PointKind; 8] = [
        PointKind::PathSegment,
        PointKind::Query,
        PointKind::Form,
        PointKind::Cookie,
        PointKind::Header,
        PointKind::Json,
        PointKind::Multipart,
        PointKind::Xml,
    ];

    /// The kinds used when none are chosen: parameters and body values,
    /// not the path or headers.
    pub const DEFAULT: [PointKind; 6] = [
        PointKind::Query,
        PointKind::Form,
        PointKind::Cookie,
        PointKind::Json,
        PointKind::Multipart,
        PointKind::Xml,
    ];
}

/// How a value must be written at its point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PointEncoding {
    /// Percent-encoding (query, form).
    Url,
    /// Percent-encoding, `/` included (path segment).
    UrlPath,
    /// `;`, `,`, spaces and quotes percent-encoded (cookie).
    Cookie,
    /// Inside a JSON string: JSON escapes, no quotes.
    JsonString,
    /// A bare JSON scalar: kept if it is valid JSON, quoted otherwise.
    JsonScalar,
    /// XML entities for `& < > " '`.
    Xml,
    /// Line breaks removed (header value).
    HeaderValue,
    /// `"` and line breaks removed (multipart file name).
    Quoted,
    /// Written as is (multipart field content).
    Raw,
}

/// One place to put a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InsertionPoint {
    /// Where it is.
    pub kind: PointKind,
    /// The parameter name, JSON path (`user.tags[0]`), header name, …
    pub name: String,
    /// The value as written in the request.
    pub value: String,
    /// Start of the value, in bytes from the start of the text.
    pub start: usize,
    /// End of the value (exclusive), in bytes.
    pub end: usize,
    /// How a new value must be encoded here.
    pub encoding: PointEncoding,
}

/// Lists the insertion points of `kinds` in a request text, in the order
/// they appear, at most [`MAX_POINTS`].
pub fn insertion_points(text: &str, kinds: &[PointKind]) -> Vec<InsertionPoint> {
    let mut out = Points {
        text,
        kinds,
        list: Vec::new(),
    };
    let (head_end, body_start) = split_head(text);
    let head = &text[..head_end];
    let mut lines = line_spans(head);
    let Some((line_start, line_end)) = lines.next() else {
        return out.list;
    };
    out.request_line(line_start, line_end);
    let mut content_type = String::new();
    for (start, end) in lines {
        let line = &text[start..end];
        let Some(colon) = line.find(':') else {
            continue;
        };
        let name = line[..colon].trim().to_ascii_lowercase();
        let value_start = start + colon + 1;
        if name == "cookie" {
            out.pairs(value_start, end, b';', PointKind::Cookie);
        } else if name == "content-type" {
            content_type = text[value_start..end].trim().to_string();
        } else if HEADERS.contains(&name.as_str()) {
            let (s, e) = trim_range(text, value_start, end);
            out.push(
                PointKind::Header,
                line[..colon].trim(),
                s,
                e,
                PointEncoding::HeaderValue,
            );
        }
    }
    if body_start < text.len() {
        out.body(body_start, &content_type);
    }
    out.list.sort_by_key(|p| p.start);
    out.list
}

/// Writes `value` at `point` with the encoding its context needs.
pub fn replace(text: &str, point: &InsertionPoint, value: &str) -> String {
    let mut out = String::with_capacity(text.len() + value.len());
    out.push_str(&text[..point.start]);
    out.push_str(&encode(point.encoding, value));
    out.push_str(&text[point.end..]);
    out
}

/// Puts `marker` around each point (overlapping points are skipped; the
/// first one wins).
pub fn mark(text: &str, points: &[InsertionPoint], marker: char) -> String {
    let mut sorted: Vec<&InsertionPoint> = points.iter().collect();
    sorted.sort_by_key(|p| (p.start, p.end));
    let mut out = String::with_capacity(text.len() + points.len() * 4);
    let mut at = 0;
    for point in sorted {
        if point.start < at || point.end > text.len() {
            continue;
        }
        out.push_str(&text[at..point.start]);
        out.push(marker);
        out.push_str(&text[point.start..point.end]);
        out.push(marker);
        at = point.end;
    }
    out.push_str(&text[at..]);
    out
}

/// `value` encoded for a context.
pub fn encode(encoding: PointEncoding, value: &str) -> String {
    match encoding {
        PointEncoding::Url => percent(value, b"-._~!$'()*,;:@/?"),
        PointEncoding::UrlPath => percent(value, b"-._~!$&'()*+,;=:@"),
        PointEncoding::Cookie => percent(value, b"-._~!#$&'()*+/:<=>?@[]^`{|}"),
        PointEncoding::JsonString => {
            let quoted = serde_json::Value::String(value.to_string()).to_string();
            quoted[1..quoted.len() - 1].to_string()
        }
        PointEncoding::JsonScalar => {
            if serde_json::from_str::<serde_json::Value>(value)
                .is_ok_and(|v| !v.is_object() && !v.is_array())
            {
                value.to_string()
            } else {
                serde_json::Value::String(value.to_string()).to_string()
            }
        }
        PointEncoding::Xml => value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;"),
        PointEncoding::HeaderValue => value.replace(['\r', '\n'], " "),
        PointEncoding::Quoted => value.replace(['"', '\r', '\n'], ""),
        PointEncoding::Raw => value.to_string(),
    }
}

/// Percent-encodes every byte that is not alphanumeric or in `keep`.
fn percent(value: &str, keep: &[u8]) -> String {
    let mut out = String::with_capacity(value.len());
    for &b in value.as_bytes() {
        if b.is_ascii_alphanumeric() || keep.contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The end of the head and the start of the body (equal to the text length
/// when there is no body).
fn split_head(text: &str) -> (usize, usize) {
    let crlf = text.find("\r\n\r\n").map(|i| (i, i + 4));
    let lf = text.find("\n\n").map(|i| (i, i + 2));
    match (crlf, lf) {
        (Some(a), Some(b)) => {
            if a.0 <= b.0 {
                a
            } else {
                b
            }
        }
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => (text.len(), text.len()),
    }
}

/// The (start, end) of each line, without its line break.
fn line_spans(text: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut at = 0;
    std::iter::from_fn(move || {
        if at >= text.len() {
            return None;
        }
        let start = at;
        let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
        at = end + 1;
        let end = if text[start..end].ends_with('\r') {
            end - 1
        } else {
            end
        };
        Some((start, end))
    })
}

/// `[start, end)` without surrounding spaces.
fn trim_range(text: &str, start: usize, end: usize) -> (usize, usize) {
    let slice = &text[start..end];
    let lead = slice.len() - slice.trim_start().len();
    let trail = slice.len() - slice.trim_end().len();
    if lead == slice.len() {
        return (start, start);
    }
    (start + lead, end - trail)
}

struct Points<'a> {
    text: &'a str,
    kinds: &'a [PointKind],
    list: Vec<InsertionPoint>,
}

impl Points<'_> {
    fn wants(&self, kind: PointKind) -> bool {
        self.kinds.contains(&kind) && self.list.len() < MAX_POINTS
    }

    fn push(
        &mut self,
        kind: PointKind,
        name: &str,
        start: usize,
        end: usize,
        encoding: PointEncoding,
    ) {
        if !self.wants(kind) || start > end || end > self.text.len() {
            return;
        }
        self.list.push(InsertionPoint {
            kind,
            name: name.to_string(),
            value: self.text[start..end].to_string(),
            start,
            end,
            encoding,
        });
    }

    /// Path segments and query parameters of the request line.
    fn request_line(&mut self, start: usize, end: usize) {
        let line = &self.text[start..end];
        let Some(sp1) = line.find(' ') else {
            return;
        };
        let target_start = start + sp1 + 1;
        let target_end = self.text[target_start..end]
            .find(' ')
            .map_or(end, |i| target_start + i);
        let target = &self.text[target_start..target_end];
        // Absolute-form targets: skip the scheme and authority.
        let path_offset = if !target.starts_with('/')
            && let Some(i) = target.find("://")
        {
            target[i + 3..]
                .find('/')
                .map_or(target.len(), |j| i + 3 + j)
        } else {
            0
        };
        let path_start = target_start + path_offset;
        let rest = &self.text[path_start..target_end];
        let path_end = path_start + rest.find(['?', '#']).unwrap_or(rest.len());
        if self.wants(PointKind::PathSegment) {
            let mut at = path_start;
            let mut index = 0;
            for segment in self.text[path_start..path_end].split('/') {
                if !segment.is_empty() {
                    index += 1;
                    let name = format!("segment {index}");
                    self.push(
                        PointKind::PathSegment,
                        &name,
                        at,
                        at + segment.len(),
                        PointEncoding::UrlPath,
                    );
                }
                at += segment.len() + 1;
            }
        }
        if self.text[path_end..target_end].starts_with('?') {
            let query_end = self.text[path_end..target_end]
                .find('#')
                .map_or(target_end, |i| path_end + i);
            self.pairs(path_end + 1, query_end, b'&', PointKind::Query);
        }
    }

    /// `name=value` pairs separated by `separator` in `[start, end)`.
    fn pairs(&mut self, start: usize, end: usize, separator: u8, kind: PointKind) {
        if !self.wants(kind) {
            return;
        }
        let encoding = if kind == PointKind::Cookie {
            PointEncoding::Cookie
        } else {
            PointEncoding::Url
        };
        let mut at = start;
        for pair in self.text[start..end].split(char::from(separator)) {
            let lead = pair.len() - pair.trim_start().len();
            let trimmed = &pair[lead..];
            if let Some(eq) = trimmed.find('=') {
                let name = trimmed[..eq].trim().to_string();
                let value_start = at + lead + eq + 1;
                let value_end = at + lead + trimmed.trim_end().len();
                if !name.is_empty() {
                    self.push(
                        kind,
                        &name,
                        value_start,
                        value_end.max(value_start),
                        encoding,
                    );
                }
            }
            at += pair.len() + 1;
        }
    }

    fn body(&mut self, start: usize, raw_content_type: &str) {
        let content_type = raw_content_type.to_ascii_lowercase();
        let body = &self.text[start..];
        let trimmed = body.trim_start();
        if content_type.starts_with("multipart/") {
            if let Some(boundary) = boundary(raw_content_type) {
                self.multipart(start, &boundary);
            }
        } else if content_type.contains("json") || trimmed.starts_with(['{', '[']) {
            let lead = body.len() - trimmed.len();
            let mut parser = Json {
                points: self,
                at: start + lead,
            };
            let _ = parser.value("", 0);
        } else if content_type.contains("xml") || trimmed.starts_with('<') {
            self.xml(start);
        } else if content_type.contains("x-www-form-urlencoded")
            || (!body.is_empty()
                && !body.contains(['\n', ' ', '{', '<'])
                && body.split('&').all(|p| p.contains('=')))
        {
            self.pairs(start, self.text.len(), b'&', PointKind::Form);
        }
    }

    fn multipart(&mut self, start: usize, boundary: &str) {
        let delimiter = format!("--{boundary}");
        let text = self.text;
        let mut at = match text[start..].find(&delimiter) {
            Some(i) => start + i,
            None => return,
        };
        loop {
            let after = at + delimiter.len();
            if text[after..].starts_with("--") {
                return;
            }
            let Some(next) = text[after..].find(&delimiter).map(|i| after + i) else {
                return;
            };
            // The part: its headers, a blank line, its content, then the line
            // break before the next delimiter.
            let part_start = after + line_break_len(&text[after..]);
            let (head_end, content_start) = split_head(&text[part_start..next]);
            let head = &text[part_start..part_start + head_end];
            let content_start = part_start + content_start;
            let mut content_end = next;
            if text[..content_end].ends_with("\r\n") {
                content_end -= 2;
            } else if text[..content_end].ends_with('\n') {
                content_end -= 1;
            }
            let disposition = head
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("content-disposition:"));
            if let Some(line) = disposition {
                let line_start = part_start + (line.as_ptr() as usize - head.as_ptr() as usize);
                let name = quoted_param(line, "name").map(|(s, e)| line[s..e].to_string());
                if let Some(name) = name {
                    match quoted_param(line, "filename") {
                        Some((s, e)) => self.push(
                            PointKind::Multipart,
                            &format!("{name} (file name)"),
                            line_start + s,
                            line_start + e,
                            PointEncoding::Quoted,
                        ),
                        None if content_start <= content_end => self.push(
                            PointKind::Multipart,
                            &name,
                            content_start,
                            content_end,
                            PointEncoding::Raw,
                        ),
                        None => {}
                    }
                }
            }
            at = next;
        }
    }

    fn xml(&mut self, start: usize) {
        if !self.wants(PointKind::Xml) {
            return;
        }
        let text = self.text;
        let mut at = start;
        let mut depth = 0usize;
        while let Some(lt) = text[at..].find('<').map(|i| at + i) {
            let Some(gt) = text[lt..].find('>').map(|i| lt + i) else {
                return;
            };
            let tag = &text[lt + 1..gt];
            at = gt + 1;
            if tag.starts_with('/') {
                depth = depth.saturating_sub(1);
                continue;
            }
            if tag.starts_with(['?', '!']) {
                continue;
            }
            let self_closing = tag.ends_with('/');
            let name_len = tag
                .find(|c: char| c.is_whitespace() || c == '/')
                .unwrap_or(tag.len());
            let name = &tag[..name_len];
            if name.is_empty() || depth > MAX_DEPTH {
                continue;
            }
            self.xml_attributes(lt + 1 + name_len, gt, name);
            if self_closing {
                continue;
            }
            depth += 1;
            // Leaf text: `<name>text</name>`.
            if let Some(next_lt) = text[at..].find('<').map(|i| at + i)
                && text[next_lt..].starts_with(&format!("</{name}"))
            {
                let (s, e) = trim_range(text, at, next_lt);
                if s < e {
                    self.push(PointKind::Xml, name, s, e, PointEncoding::Xml);
                }
            }
        }
    }

    fn xml_attributes(&mut self, start: usize, end: usize, element: &str) {
        let text = self.text;
        let mut at = start;
        while let Some(eq) = text[at..end].find('=').map(|i| at + i) {
            let name = text[at..eq].trim();
            let Some(quote) = text[eq + 1..end].trim_start().chars().next() else {
                return;
            };
            if quote != '"' && quote != '\'' {
                return;
            }
            let open = eq + 1 + (text[eq + 1..end].len() - text[eq + 1..end].trim_start().len());
            let Some(close) = text[open + 1..end].find(quote).map(|i| open + 1 + i) else {
                return;
            };
            if !name.is_empty() {
                self.push(
                    PointKind::Xml,
                    &format!("{element}@{name}"),
                    open + 1,
                    close,
                    PointEncoding::Xml,
                );
            }
            at = close + 1;
        }
    }
}

/// Length of the line break at the start of `text` (0, 1 or 2).
fn line_break_len(text: &str) -> usize {
    if text.starts_with("\r\n") {
        2
    } else if text.starts_with('\n') {
        1
    } else {
        0
    }
}

/// The `boundary` parameter of a multipart content type.
fn boundary(content_type: &str) -> Option<String> {
    let at = content_type.to_ascii_lowercase().find("boundary=")? + "boundary=".len();
    let value = content_type[at..]
        .split(';')
        .next()?
        .trim()
        .trim_matches('"');
    (!value.is_empty()).then(|| value.to_string())
}

/// The range of a quoted `name="…"` parameter in a header line.
fn quoted_param(line: &str, name: &str) -> Option<(usize, usize)> {
    let lower = line.to_ascii_lowercase();
    let needle = format!("{name}=\"");
    let mut from = 0;
    while let Some(i) = lower[from..].find(&needle).map(|i| from + i) {
        // `name=` must not be the end of `filename=`.
        let before = lower[..i].chars().last();
        if before.is_none_or(|c| c == ';' || c.is_whitespace()) {
            let start = i + needle.len();
            let end = start + line[start..].find('"')?;
            return Some((start, end));
        }
        from = i + needle.len();
    }
    None
}

/// A small JSON reader that records the range of every scalar value.
struct Json<'p, 'a> {
    points: &'p mut Points<'a>,
    at: usize,
}

impl Json<'_, '_> {
    fn bytes(&self) -> &[u8] {
        self.points.text.as_bytes()
    }

    fn skip_space(&mut self) {
        while self
            .bytes()
            .get(self.at)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes().get(self.at).copied()
    }

    fn expect(&mut self, byte: u8) -> Result<(), ()> {
        self.skip_space();
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(())
        }
    }

    fn value(&mut self, path: &str, depth: usize) -> Result<(), ()> {
        if depth > MAX_DEPTH {
            return Err(());
        }
        self.skip_space();
        let name = if path.is_empty() { "body" } else { path };
        match self.peek().ok_or(())? {
            b'{' => {
                self.at += 1;
                self.skip_space();
                if self.peek() == Some(b'}') {
                    self.at += 1;
                    return Ok(());
                }
                loop {
                    self.skip_space();
                    let (ks, ke) = self.string()?;
                    let key = self.points.text[ks..ke].to_string();
                    self.expect(b':')?;
                    let child = if path.is_empty() {
                        key
                    } else {
                        format!("{path}.{key}")
                    };
                    self.value(&child, depth + 1)?;
                    self.skip_space();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(());
                        }
                        _ => return Err(()),
                    }
                }
            }
            b'[' => {
                self.at += 1;
                self.skip_space();
                if self.peek() == Some(b']') {
                    self.at += 1;
                    return Ok(());
                }
                let mut index = 0;
                loop {
                    self.value(&format!("{path}[{index}]"), depth + 1)?;
                    index += 1;
                    self.skip_space();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(());
                        }
                        _ => return Err(()),
                    }
                }
            }
            b'"' => {
                let (s, e) = self.string()?;
                self.points
                    .push(PointKind::Json, name, s, e, PointEncoding::JsonString);
                Ok(())
            }
            _ => {
                let start = self.at;
                while self
                    .peek()
                    .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'+' | b'.'))
                {
                    self.at += 1;
                }
                if self.at == start {
                    return Err(());
                }
                self.points.push(
                    PointKind::Json,
                    name,
                    start,
                    self.at,
                    PointEncoding::JsonScalar,
                );
                Ok(())
            }
        }
    }

    /// A JSON string at the cursor; returns the range inside the quotes.
    fn string(&mut self) -> Result<(usize, usize), ()> {
        if self.peek() != Some(b'"') {
            return Err(());
        }
        self.at += 1;
        let start = self.at;
        loop {
            match self.peek().ok_or(())? {
                b'\\' => self.at += 2,
                b'"' => {
                    let end = self.at;
                    self.at += 1;
                    return Ok((start, end));
                }
                _ => self.at += 1,
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn names(points: &[InsertionPoint]) -> Vec<(PointKind, &str, &str)> {
        points
            .iter()
            .map(|p| (p.kind, p.name.as_str(), p.value.as_str()))
            .collect()
    }

    #[test]
    fn finds_path_query_cookie_header_and_form_points() {
        let text = "POST /api/users/42?x=1&flag&q=a%20b#frag HTTP/1.1\nHost: lab\n\
                    User-Agent: test/1.0\nCookie: sid=abc; theme=dark\n\
                    Content-Type: application/x-www-form-urlencoded\n\nuser=bob&pass=";
        let points = insertion_points(text, &PointKind::ALL);
        assert_eq!(
            names(&points),
            [
                (PointKind::PathSegment, "segment 1", "api"),
                (PointKind::PathSegment, "segment 2", "users"),
                (PointKind::PathSegment, "segment 3", "42"),
                (PointKind::Query, "x", "1"),
                (PointKind::Query, "q", "a%20b"),
                (PointKind::Header, "User-Agent", "test/1.0"),
                (PointKind::Cookie, "sid", "abc"),
                (PointKind::Cookie, "theme", "dark"),
                (PointKind::Form, "user", "bob"),
                (PointKind::Form, "pass", ""),
            ]
        );
        for p in &points {
            assert_eq!(&text[p.start..p.end], p.value);
        }
        // The default kinds leave the path and headers alone.
        let default = insertion_points(text, &PointKind::DEFAULT);
        assert!(
            default
                .iter()
                .all(|p| !matches!(p.kind, PointKind::PathSegment | PointKind::Header))
        );
    }

    #[test]
    fn finds_json_values_at_any_depth() {
        let text = "POST /api HTTP/1.1\r\nContent-Type: application/json\r\n\r\n\
                    {\"user\": {\"name\": \"bob \\\"b\\\"\", \"age\": 30, \"tags\": [\"a\", true]}, \"x\": null}";
        let points = insertion_points(text, &PointKind::DEFAULT);
        assert_eq!(
            names(&points),
            [
                (PointKind::Json, "user.name", "bob \\\"b\\\""),
                (PointKind::Json, "user.age", "30"),
                (PointKind::Json, "user.tags[0]", "a"),
                (PointKind::Json, "user.tags[1]", "true"),
                (PointKind::Json, "x", "null"),
            ]
        );
        // Strings get JSON escapes; bare scalars stay valid JSON.
        let name = &points[0];
        let replaced = replace(text, name, "a\"b");
        assert!(replaced.contains(r#""name": "a\"b""#));
        let age = &points[1];
        assert!(replace(text, age, "31").contains("\"age\": 31,"));
        assert!(replace(text, age, "x y").contains("\"age\": \"x y\","));
        // Broken JSON yields what was read before the error, no panic.
        let broken = "POST / HTTP/1.1\n\n{\"a\": 1, \"b\": ";
        assert_eq!(
            names(&insertion_points(broken, &PointKind::DEFAULT)),
            [(PointKind::Json, "a", "1")]
        );
    }

    #[test]
    fn finds_multipart_fields_and_file_names() {
        let text = "POST /up HTTP/1.1\r\nContent-Type: multipart/form-data; boundary=XyZ\r\n\r\n\
                    --XyZ\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nhello world\r\n\
                    --XyZ\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.txt\"\r\n\
                    Content-Type: text/plain\r\n\r\nfile body\r\n--XyZ--\r\n";
        let points = insertion_points(text, &PointKind::DEFAULT);
        assert_eq!(
            names(&points),
            [
                (PointKind::Multipart, "title", "hello world"),
                (PointKind::Multipart, "file (file name)", "a.txt"),
            ]
        );
        assert!(replace(text, &points[1], "b\".txt").contains("filename=\"b.txt\""));
    }

    #[test]
    fn finds_xml_text_and_attributes() {
        let text = "POST /soap HTTP/1.1\nContent-Type: text/xml\n\n<?xml version=\"1.0\"?>\
                    <req id=\"7\"><user>bob</user><!-- c --><empty/><n>  5 </n></req>";
        let points = insertion_points(text, &PointKind::DEFAULT);
        assert_eq!(
            names(&points),
            [
                (PointKind::Xml, "req@id", "7"),
                (PointKind::Xml, "user", "bob"),
                (PointKind::Xml, "n", "5"),
            ]
        );
        assert!(replace(text, &points[1], "<a&b>").contains("<user>&lt;a&amp;b&gt;</user>"));
    }

    #[test]
    fn encodes_url_cookie_and_header_values() {
        assert_eq!(
            encode(PointEncoding::Url, "a b&c=d/é"),
            "a%20b%26c%3Dd/%C3%A9"
        );
        assert_eq!(encode(PointEncoding::UrlPath, "a/b"), "a%2Fb");
        assert_eq!(encode(PointEncoding::Cookie, "a; b,c"), "a%3B%20b%2Cc");
        assert_eq!(encode(PointEncoding::HeaderValue, "a\r\nb"), "a  b");
    }

    #[test]
    fn marks_points_without_overlaps() {
        let text = "GET /?a=1&b=2 HTTP/1.1\nHost: lab\n\n";
        let points = insertion_points(text, &PointKind::DEFAULT);
        assert_eq!(
            mark(text, &points, '§'),
            "GET /?a=§1§&b=§2§ HTTP/1.1\nHost: lab\n\n"
        );
        let mut overlapping = points.clone();
        overlapping.push(InsertionPoint {
            start: points[0].start,
            end: points[1].end,
            ..points[0].clone()
        });
        assert_eq!(
            mark(text, &overlapping, '§'),
            "GET /?a=§1§&b=§2§ HTTP/1.1\nHost: lab\n\n"
        );
    }

    #[test]
    fn absolute_targets_and_garbage_do_not_panic() {
        let text = "GET http://lab:9090/a/b?x=1 HTTP/1.1\n\n";
        let points = insertion_points(text, &PointKind::ALL);
        assert_eq!(
            names(&points),
            [
                (PointKind::PathSegment, "segment 1", "a"),
                (PointKind::PathSegment, "segment 2", "b"),
                (PointKind::Query, "x", "1"),
            ]
        );
        for garbage in [
            "",
            "GET",
            "\n\n{",
            "POST / HTTP/1.1\n\n<a b=\"",
            "x\n\n--",
            "é?é=é\n\n[1,",
        ] {
            let _ = insertion_points(garbage, &PointKind::ALL);
        }
        let many = format!("GET /?{} HTTP/1.1\n\n", "a=1&".repeat(500));
        assert_eq!(insertion_points(&many, &PointKind::ALL).len(), MAX_POINTS);
    }
}
