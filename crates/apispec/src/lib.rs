//! API definitions (M27): OpenAPI 3.x, Swagger 2.0, Postman collections
//! v2.1 and WSDL 1.1 read into a list of endpoints, each with a request a
//! tester can send.
//!
//! Parsing only reads the text it is given: remote `$ref`s and imports are
//! never fetched, sizes and depths are bounded, and nothing is sent.

mod example;
pub mod graphql;
mod openapi;
mod postman;
mod wsdl;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Largest definition read.
pub const MAX_SPEC_BYTES: usize = 10 * 1024 * 1024;
/// Most endpoints kept from one definition.
pub const MAX_ENDPOINTS: usize = 5000;
/// Deepest `$ref` chain or example nesting followed.
pub(crate) const MAX_DEPTH: usize = 32;

/// What kind of definition a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SpecFormat {
    /// OpenAPI 3.0 or 3.1.
    OpenApi3,
    /// Swagger (OpenAPI) 2.0.
    Swagger2,
    /// Postman collection v2.0/v2.1.
    Postman,
    /// WSDL 1.1 (SOAP 1.1 and 1.2 bindings).
    Wsdl,
}

/// Where a parameter goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ApiParamLocation")]
pub enum ParamLocation {
    /// In the path (`/users/{id}`).
    Path,
    /// In the query string.
    Query,
    /// A header.
    Header,
    /// A cookie.
    Cookie,
    /// A field of the body.
    Body,
}

/// One parameter of an endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiParam {
    /// Name.
    pub name: String,
    /// Where it goes.
    pub location: ParamLocation,
    /// The definition says it is required.
    pub required: bool,
    /// The value used in the example request.
    pub example: String,
}

/// One operation of an API, with an example request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiEndpoint {
    /// HTTP method (upper case).
    pub method: String,
    /// Absolute URL of the example request (path and query filled in).
    pub url: String,
    /// The path as the definition writes it (`/users/{id}`).
    pub path: String,
    /// Operation id, summary or name.
    pub name: String,
    /// Group (tag, folder or port).
    pub group: String,
    /// Parameters.
    pub params: Vec<ApiParam>,
    /// Headers of the example request (besides Host and Content-Length).
    pub headers: Vec<(String, String)>,
    /// `Content-Type` of the body.
    pub content_type: Option<String>,
    /// Example body.
    pub body: Option<String>,
    /// Whether sending it only reads (GraphQL queries); `None`: decided by
    /// the method ([`is_safe_method`]).
    #[serde(default)]
    pub read_only: Option<bool>,
}

impl ApiEndpoint {
    /// Whether sending it should not change anything on the server.
    pub fn is_read_only(&self) -> bool {
        self.read_only
            .unwrap_or_else(|| is_safe_method(&self.method))
    }
}

/// A parsed definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiDefinition {
    /// Format read.
    pub format: SpecFormat,
    /// Title.
    pub title: String,
    /// Version of the API (not of the format).
    pub version: String,
    /// Endpoints, in definition order.
    pub endpoints: Vec<ApiEndpoint>,
    /// What could not be used (unresolved references, missing servers…).
    pub warnings: Vec<String>,
}

/// Reads a definition. `base_url` is used when the definition has no
/// absolute server (or only a relative one), e.g. `https://api.example`.
pub fn parse(text: &str, base_url: Option<&str>) -> Result<ApiDefinition, String> {
    if text.len() > MAX_SPEC_BYTES {
        return Err(format!(
            "the definition is larger than {} MB",
            MAX_SPEC_BYTES / 1024 / 1024
        ));
    }
    let base = match base_url.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => Some(
            url::Url::parse(b)
                .ok()
                .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
                .ok_or_else(|| format!("{b:?} is not an http(s) URL"))?,
        ),
        None => None,
    };
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('<') {
        return wsdl::parse(trimmed, base.as_ref());
    }
    let value: Value = if trimmed.starts_with('{') || trimmed.starts_with('[') {
        serde_json::from_str(trimmed).map_err(|e| format!("not valid JSON: {e}"))?
    } else {
        yaml_to_json(trimmed)?
    };
    if value
        .get("openapi")
        .and_then(Value::as_str)
        .is_some_and(|v| v.starts_with('3'))
    {
        openapi::parse_v3(&value, base.as_ref())
    } else if value.get("swagger").and_then(Value::as_str) == Some("2.0") {
        openapi::parse_v2(&value, base.as_ref())
    } else if value.get("info").and_then(|i| i.get("schema")).is_some()
        || value.get("item").is_some()
    {
        postman::parse(&value, base.as_ref())
    } else {
        Err("not an OpenAPI 3, Swagger 2.0, Postman v2 or WSDL 1.1 definition".into())
    }
}

/// The example request of an endpoint in the editors' text form
/// (`GET /path HTTP/1.1`, `Host`, headers, body with Content-Length).
pub fn request_text(endpoint: &ApiEndpoint) -> String {
    let parsed = url::Url::parse(&endpoint.url).ok();
    let (host, target) = match &parsed {
        Some(u) => {
            let host = match (u.host_str(), u.port()) {
                (Some(h), Some(p)) => format!("{h}:{p}"),
                (Some(h), None) => h.to_string(),
                _ => String::new(),
            };
            let mut target = u.path().to_string();
            if let Some(q) = u.query() {
                target.push('?');
                target.push_str(q);
            }
            (host, target)
        }
        None => (String::new(), endpoint.url.clone()),
    };
    let mut out = format!("{} {target} HTTP/1.1\r\nHost: {host}\r\n", endpoint.method);
    let mut has_type = false;
    for (name, value) in &endpoint.headers {
        if name.eq_ignore_ascii_case("host") || name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        has_type |= name.eq_ignore_ascii_case("content-type");
        out.push_str(&format!("{name}: {}\r\n", value.replace(['\r', '\n'], " ")));
    }
    match &endpoint.body {
        Some(body) => {
            if !has_type && let Some(ct) = &endpoint.content_type {
                out.push_str(&format!("Content-Type: {ct}\r\n"));
            }
            out.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
        }
        None => out.push_str("\r\n"),
    }
    out
}

/// Methods that do not change state on a well-behaved server.
pub fn is_safe_method(method: &str) -> bool {
    matches!(
        method.to_ascii_uppercase().as_str(),
        "GET" | "HEAD" | "OPTIONS"
    )
}

/// YAML (1.2) to JSON values; only the first document is read.
fn yaml_to_json(text: &str) -> Result<Value, String> {
    use yaml_rust2::{Yaml, YamlLoader};
    fn convert(y: &Yaml, depth: usize) -> Result<Value, String> {
        if depth > 200 {
            return Err("the YAML is nested too deeply".into());
        }
        Ok(match y {
            Yaml::Real(r) => r
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map_or_else(|| Value::String(r.clone()), Value::Number),
            Yaml::Integer(i) => Value::from(*i),
            Yaml::String(s) => Value::String(s.clone()),
            Yaml::Boolean(b) => Value::Bool(*b),
            Yaml::Array(a) => Value::Array(
                a.iter()
                    .map(|v| convert(v, depth + 1))
                    .collect::<Result<_, _>>()?,
            ),
            Yaml::Hash(h) => {
                let mut map = serde_json::Map::new();
                for (k, v) in h {
                    let key = match k {
                        Yaml::String(s) => s.clone(),
                        Yaml::Integer(i) => i.to_string(),
                        Yaml::Real(r) => r.clone(),
                        Yaml::Boolean(b) => b.to_string(),
                        _ => continue,
                    };
                    map.insert(key, convert(v, depth + 1)?);
                }
                Value::Object(map)
            }
            Yaml::Alias(_) | Yaml::Null | Yaml::BadValue => Value::Null,
        })
    }
    let docs = YamlLoader::load_from_str(text).map_err(|e| format!("not valid YAML: {e}"))?;
    let first = docs.first().ok_or("the YAML document is empty")?;
    convert(first, 0)
}

/// Joins a server URL (maybe relative, maybe with a trailing slash) and a
/// path, against `base` when the server is relative or missing.
pub(crate) fn join_url(
    server: Option<&str>,
    path: &str,
    base: Option<&url::Url>,
) -> Option<String> {
    let server = server.unwrap_or("").trim();
    let absolute = match url::Url::parse(server) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => u,
        _ => {
            let base = base?;
            base.join(server).ok()?
        }
    };
    let mut prefix = absolute.as_str().trim_end_matches('/').to_string();
    if absolute.query().is_some() || absolute.fragment().is_some() {
        prefix = format!(
            "{}://{}{}",
            absolute.scheme(),
            absolute.host_str()?,
            absolute.port().map(|p| format!(":{p}")).unwrap_or_default()
        );
    }
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    Some(format!("{prefix}{path}"))
}

/// Percent-encodes a value for a URL path segment or query value.
pub(crate) fn encode_component(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn detects_formats_and_refuses_others() {
        assert!(
            parse("{\"hello\": 1}", None)
                .unwrap_err()
                .contains("not an OpenAPI")
        );
        assert!(parse("key: [unclosed", None).unwrap_err().contains("YAML"));
        assert!(parse(&"x".repeat(MAX_SPEC_BYTES + 1), None).is_err());
        assert!(
            parse("openapi: 3.0.0", Some("ftp://x"))
                .unwrap_err()
                .contains("http")
        );
    }

    #[test]
    fn builds_request_text() {
        let e = ApiEndpoint {
            method: "POST".into(),
            url: "https://api.test:8443/v1/items?x=1".into(),
            path: "/v1/items".into(),
            name: "create".into(),
            group: String::new(),
            params: vec![],
            headers: vec![("X-Key".into(), "a\r\nb".into())],
            content_type: Some("application/json".into()),
            body: Some("{\"a\":1}".into()),
            read_only: None,
        };
        assert_eq!(
            request_text(&e),
            "POST /v1/items?x=1 HTTP/1.1\r\nHost: api.test:8443\r\nX-Key: a  b\r\n\
             Content-Type: application/json\r\nContent-Length: 7\r\n\r\n{\"a\":1}"
        );
        assert!(is_safe_method("get") && !is_safe_method("DELETE"));
    }

    #[test]
    fn joins_servers_and_paths() {
        let base = url::Url::parse("https://host.test/app/").unwrap();
        assert_eq!(
            join_url(Some("https://api.test/v1/"), "/users", None).unwrap(),
            "https://api.test/v1/users"
        );
        assert_eq!(
            join_url(Some("/api"), "users", Some(&base)).unwrap(),
            "https://host.test/api/users"
        );
        assert_eq!(
            join_url(None, "/x", Some(&base)).unwrap(),
            "https://host.test/app/x"
        );
        assert!(join_url(Some("/api"), "/x", None).is_none());
    }
}
