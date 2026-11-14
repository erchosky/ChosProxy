//! Postman collections v2.0 and v2.1: folders of requests, with
//! collection variables (`{{name}}`) filled in.

use crate::example::as_text;
use crate::openapi::BOUNDARY;
use crate::{
    ApiDefinition, ApiEndpoint, ApiParam, MAX_DEPTH, MAX_ENDPOINTS, ParamLocation, SpecFormat,
    encode_component,
};
use serde_json::Value;
use std::collections::HashMap;

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or_default().to_string()
}

fn enabled(v: &Value) -> bool {
    !v.get("disabled").and_then(Value::as_bool).unwrap_or(false)
}

/// `{{name}}` replaced by collection variables (unknown ones are kept).
fn fill(text: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let name = after[..end].trim();
                match vars.get(name) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[start..start + 2 + end + 2]),
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The URL of a request (a string, or an object with `raw` or parts).
fn url_of(url: &Value) -> String {
    match url {
        Value::String(s) => s.clone(),
        Value::Object(o) => {
            let raw = text(o.get("raw"));
            if !raw.is_empty() {
                return raw;
            }
            let protocol = text(o.get("protocol"));
            let host = match o.get("host") {
                Some(Value::Array(parts)) => {
                    parts.iter().map(as_text).collect::<Vec<_>>().join(".")
                }
                Some(other) => as_text(other),
                None => String::new(),
            };
            let port = text(o.get("port"));
            let path = match o.get("path") {
                Some(Value::Array(parts)) => {
                    parts.iter().map(as_text).collect::<Vec<_>>().join("/")
                }
                Some(other) => as_text(other),
                None => String::new(),
            };
            let query: Vec<String> = o
                .get("query")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|q| enabled(q))
                .map(|q| format!("{}={}", text(q.get("key")), text(q.get("value"))))
                .collect();
            let mut out = format!(
                "{}{host}{}/{path}",
                if protocol.is_empty() {
                    String::new()
                } else {
                    format!("{protocol}://")
                },
                if port.is_empty() {
                    String::new()
                } else {
                    format!(":{port}")
                }
            );
            if !query.is_empty() {
                out.push('?');
                out.push_str(&query.join("&"));
            }
            out
        }
        _ => String::new(),
    }
}

struct Walk<'a> {
    vars: &'a HashMap<String, String>,
    base: Option<&'a url::Url>,
    endpoints: Vec<ApiEndpoint>,
    warnings: Vec<String>,
}

impl Walk<'_> {
    fn items(&mut self, items: &Value, group: &str, depth: usize) {
        if depth > MAX_DEPTH {
            self.warnings
                .push("Folders nested too deeply were skipped.".into());
            return;
        }
        for item in items.as_array().into_iter().flatten() {
            if self.endpoints.len() >= MAX_ENDPOINTS {
                self.warnings.push(format!(
                    "Only the first {MAX_ENDPOINTS} requests were kept."
                ));
                return;
            }
            let name = text(item.get("name"));
            if let Some(children) = item.get("item") {
                let folder = if group.is_empty() {
                    name
                } else {
                    format!("{group} / {name}")
                };
                self.items(children, &folder, depth + 1);
            } else if let Some(request) = item.get("request") {
                self.request(request, &name, group);
            }
        }
    }

    fn request(&mut self, request: &Value, name: &str, group: &str) {
        // v2 allows a request given as a bare URL string.
        let (method, url_value) = match request {
            Value::String(s) => ("GET".to_string(), Value::String(s.clone())),
            _ => (
                text(request.get("method")).to_ascii_uppercase(),
                request.get("url").cloned().unwrap_or(Value::Null),
            ),
        };
        let method = if method.is_empty() {
            "GET".to_string()
        } else {
            method
        };
        let raw_url = fill(&url_of(&url_value), self.vars);
        let with_scheme = if raw_url.contains("://") {
            raw_url.clone()
        } else {
            format!("https://{raw_url}")
        };
        let url = match url::Url::parse(&with_scheme) {
            Ok(u) if u.host_str().is_some_and(|h| !h.contains("{{")) => u.to_string(),
            _ => match self
                .base
                .and_then(|b| b.join(raw_url.trim_start_matches('/')).ok())
            {
                Some(u) => u.to_string(),
                None => {
                    self.warnings.push(format!(
                        "{name}: the URL {raw_url:?} has no usable host (unknown variable?); \
                         give a base URL"
                    ));
                    return;
                }
            },
        };
        let mut params = Vec::new();
        if let Ok(u) = url::Url::parse(&url) {
            for (k, v) in u.query_pairs() {
                params.push(ApiParam {
                    name: k.into_owned(),
                    location: ParamLocation::Query,
                    required: false,
                    example: v.into_owned(),
                });
            }
        }
        let mut headers = Vec::new();
        for h in request
            .get("header")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !enabled(h) {
                continue;
            }
            let (k, v) = (
                fill(&text(h.get("key")), self.vars),
                fill(&text(h.get("value")), self.vars),
            );
            if !k.is_empty() {
                params.push(ApiParam {
                    name: k.clone(),
                    location: ParamLocation::Header,
                    required: false,
                    example: v.clone(),
                });
                headers.push((k, v));
            }
        }
        let (content_type, body) = self.body(request.get("body"), &mut params);
        self.endpoints.push(ApiEndpoint {
            method,
            path: url::Url::parse(&url)
                .map(|u| u.path().to_string())
                .unwrap_or_default(),
            url,
            name: if name.is_empty() {
                "request".into()
            } else {
                name.to_string()
            },
            group: group.to_string(),
            params,
            headers,
            content_type,
            body,
            read_only: None,
        });
    }

    fn body(
        &self,
        body: Option<&Value>,
        params: &mut Vec<ApiParam>,
    ) -> (Option<String>, Option<String>) {
        let Some(body) = body else {
            return (None, None);
        };
        let pairs = |key: &str| -> Vec<(String, String)> {
            body.get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|p| enabled(p))
                .map(|p| {
                    (
                        fill(&text(p.get("key")), self.vars),
                        fill(&text(p.get("value")), self.vars),
                    )
                })
                .collect()
        };
        match text(body.get("mode")).as_str() {
            "raw" => {
                let raw = fill(&text(body.get("raw")), self.vars);
                let language = text(body.pointer("/options/raw/language"));
                let ct = match language.as_str() {
                    "json" => Some("application/json".to_string()),
                    "xml" => Some("application/xml".to_string()),
                    "html" => Some("text/html".to_string()),
                    "javascript" => Some("application/javascript".to_string()),
                    _ if raw.trim_start().starts_with(['{', '[']) => {
                        Some("application/json".to_string())
                    }
                    _ => Some("text/plain".to_string()),
                };
                (ct, Some(raw))
            }
            "urlencoded" => {
                let fields = pairs("urlencoded");
                for (k, v) in &fields {
                    params.push(ApiParam {
                        name: k.clone(),
                        location: ParamLocation::Body,
                        required: false,
                        example: v.clone(),
                    });
                }
                let body = fields
                    .iter()
                    .map(|(k, v)| format!("{}={}", encode_component(k), encode_component(v)))
                    .collect::<Vec<_>>()
                    .join("&");
                (Some("application/x-www-form-urlencoded".into()), Some(body))
            }
            "formdata" => {
                let fields = pairs("formdata");
                let mut out = String::new();
                for (k, v) in &fields {
                    params.push(ApiParam {
                        name: k.clone(),
                        location: ParamLocation::Body,
                        required: false,
                        example: v.clone(),
                    });
                    out.push_str(&format!(
                        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{v}\r\n",
                        k.replace('"', "")
                    ));
                }
                out.push_str(&format!("--{BOUNDARY}--\r\n"));
                (
                    Some(format!("multipart/form-data; boundary={BOUNDARY}")),
                    Some(out),
                )
            }
            "graphql" => {
                let query = text(body.pointer("/graphql/query"));
                let variables = text(body.pointer("/graphql/variables"));
                let vars: Value =
                    serde_json::from_str(&variables).unwrap_or(Value::Object(Default::default()));
                let payload = serde_json::json!({"query": query, "variables": vars});
                (Some("application/json".into()), Some(payload.to_string()))
            }
            _ => (None, None),
        }
    }
}

pub(crate) fn parse(doc: &Value, base: Option<&url::Url>) -> Result<ApiDefinition, String> {
    let items = doc.get("item").ok_or("no requests in the collection")?;
    let vars: HashMap<String, String> = doc
        .get("variable")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|v| enabled(v))
        .map(|v| {
            (
                text(v.get("key")),
                as_text(v.get("value").unwrap_or(&Value::Null)),
            )
        })
        .filter(|(k, _)| !k.is_empty())
        .collect();
    let mut walk = Walk {
        vars: &vars,
        base,
        endpoints: Vec::new(),
        warnings: Vec::new(),
    };
    walk.items(items, "", 0);
    Ok(ApiDefinition {
        format: SpecFormat::Postman,
        title: text(doc.pointer("/info/name")),
        version: text(doc.pointer("/info/version")),
        endpoints: walk.endpoints,
        warnings: walk.warnings,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use crate::{SpecFormat, parse};

    #[test]
    fn reads_folders_variables_and_bodies() {
        let collection = r#"{
          "info": {"name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
          "variable": [{"key": "base", "value": "https://shop.test"}, {"key": "token", "value": "t0k"}],
          "item": [
            {"name": "Users", "item": [
              {"name": "Get user", "request": {"method": "get",
                "url": {"raw": "{{base}}/users/7?full=1"},
                "header": [{"key": "Authorization", "value": "Bearer {{token}}"},
                           {"key": "X-Off", "value": "1", "disabled": true}]}},
              {"name": "Login", "request": {"method": "POST",
                "url": {"protocol": "https", "host": ["shop", "test"], "path": ["login"]},
                "body": {"mode": "urlencoded", "urlencoded": [{"key": "user", "value": "a b"}]}}}
            ]},
            {"name": "Create", "request": {"method": "POST", "url": "{{base}}/items",
              "body": {"mode": "raw", "raw": "{\"n\": 1}", "options": {"raw": {"language": "json"}}}}},
            {"name": "Unknown host", "request": {"method": "GET", "url": "{{missing}}/x"}},
            {"name": "GraphQL", "request": {"method": "POST", "url": "{{base}}/graphql",
              "body": {"mode": "graphql", "graphql": {"query": "{ me { id } }", "variables": ""}}}}
          ]
        }"#;
        let def = parse(collection, None).unwrap();
        assert_eq!(def.format, SpecFormat::Postman);
        assert_eq!(def.title, "Shop");
        assert_eq!(def.endpoints.len(), 4, "{:?}", def.warnings);
        assert_eq!(def.warnings.len(), 1);
        let get = &def.endpoints[0];
        assert_eq!(
            (get.method.as_str(), get.url.as_str()),
            ("GET", "https://shop.test/users/7?full=1")
        );
        assert_eq!(get.group, "Users");
        assert_eq!(
            get.headers,
            vec![("Authorization".into(), "Bearer t0k".into())]
        );
        let login = &def.endpoints[1];
        assert_eq!(login.url, "https://shop.test/login");
        assert_eq!(login.body.as_deref(), Some("user=a%20b"));
        let create = &def.endpoints[2];
        assert_eq!(create.content_type.as_deref(), Some("application/json"));
        let gql = &def.endpoints[3];
        assert_eq!(
            gql.body.as_deref(),
            Some("{\"query\":\"{ me { id } }\",\"variables\":{}}")
        );
        // With a base URL the unknown host is resolved against it.
        let def = parse(collection, Some("http://localhost:9090")).unwrap();
        assert_eq!(def.endpoints.len(), 5);
    }
}
