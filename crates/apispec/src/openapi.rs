//! OpenAPI 3.x and Swagger 2.0.

use crate::example::{Resolver, as_text};
use crate::{
    ApiDefinition, ApiEndpoint, ApiParam, MAX_ENDPOINTS, ParamLocation, SpecFormat,
    encode_component, join_url,
};
use serde_json::{Map, Value};

const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];
/// Boundary of generated multipart bodies.
pub(crate) const BOUNDARY: &str = "----ChosProxyBoundary7MA4YWxkTrZu0gW";

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Parameters of a path item and an operation, the operation's winning
/// over the path's with the same name and location, references resolved.
fn merged_params<'a>(
    r: &Resolver<'a>,
    item: &'a Value,
    op: &'a Value,
    warnings: &mut Vec<String>,
) -> Vec<&'a Value> {
    let mut out: Vec<&'a Value> = Vec::new();
    for list in [item.get("parameters"), op.get("parameters")] {
        for p in list.and_then(Value::as_array).into_iter().flatten() {
            let Some(p) = r.resolve(p) else {
                warnings.push(format!(
                    "Unresolved parameter reference {}",
                    text(p.get("$ref"))
                ));
                continue;
            };
            let key = (text(p.get("name")), text(p.get("in")));
            out.retain(|q| (text(q.get("name")), text(q.get("in"))) != key);
            out.push(p);
        }
    }
    out
}

/// The example of a parameter (OpenAPI 3: `example`, `examples`, schema;
/// Swagger 2: `x-example`, `default`, `enum`, its own type).
fn param_example(r: &Resolver<'_>, p: &Value) -> String {
    if let Some(v) = p.get("example").or_else(|| p.get("x-example")) {
        return as_text(v);
    }
    if let Some(first) = p
        .get("examples")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(|e| r.resolve(e))
        .and_then(|e| e.get("value"))
    {
        return as_text(first);
    }
    match p.get("schema") {
        Some(schema) => as_text(&r.example(schema)),
        // Swagger 2 keeps the schema keywords on the parameter itself.
        None => as_text(&r.example(p)),
    }
}

fn location(name: &str) -> Option<ParamLocation> {
    Some(match name {
        "path" => ParamLocation::Path,
        "query" => ParamLocation::Query,
        "header" => ParamLocation::Header,
        "cookie" => ParamLocation::Cookie,
        "body" | "formData" => ParamLocation::Body,
        _ => return None,
    })
}

/// Fills `{name}` in a path and builds the query, headers and cookies.
struct Built {
    path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    cookies: Vec<(String, String)>,
}

fn build(path: &str, params: &[ApiParam]) -> Built {
    let mut filled = path.to_string();
    let mut built = Built {
        path: String::new(),
        query: Vec::new(),
        headers: Vec::new(),
        cookies: Vec::new(),
    };
    for p in params {
        match p.location {
            ParamLocation::Path => {
                filled = filled.replace(&format!("{{{}}}", p.name), &encode_component(&p.example));
            }
            ParamLocation::Query => built.query.push((p.name.clone(), p.example.clone())),
            ParamLocation::Header => built.headers.push((p.name.clone(), p.example.clone())),
            ParamLocation::Cookie => built.cookies.push((p.name.clone(), p.example.clone())),
            ParamLocation::Body => {}
        }
    }
    built.path = filled;
    built
}

fn finish_url(url: String, query: &[(String, String)]) -> String {
    if query.is_empty() {
        return url;
    }
    let q: Vec<String> = query
        .iter()
        .map(|(k, v)| format!("{}={}", encode_component(k), encode_component(v)))
        .collect();
    format!(
        "{url}{}{}",
        if url.contains('?') { '&' } else { '?' },
        q.join("&")
    )
}

/// A body for a media type from an example value.
fn body_for(media_type: &str, value: &Value) -> String {
    let mt = media_type.to_ascii_lowercase();
    if mt.contains("json") {
        serde_json::to_string_pretty(value).unwrap_or_default()
    } else if mt.contains("x-www-form-urlencoded") {
        match value {
            Value::Object(map) => map
                .iter()
                .map(|(k, v)| format!("{}={}", encode_component(k), encode_component(&as_text(v))))
                .collect::<Vec<_>>()
                .join("&"),
            other => as_text(other),
        }
    } else if mt.starts_with("multipart/") {
        multipart(value)
    } else if mt.contains("xml") {
        match value {
            Value::Object(map) => xml_of("root", map),
            other => as_text(other),
        }
    } else {
        as_text(value)
    }
}

fn multipart(value: &Value) -> String {
    let mut out = String::new();
    if let Value::Object(map) = value {
        for (k, v) in map {
            out.push_str(&format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{}\r\n",
                k.replace('"', ""),
                as_text(v)
            ));
        }
    }
    out.push_str(&format!("--{BOUNDARY}--\r\n"));
    out
}

fn xml_of(name: &str, map: &Map<String, Value>) -> String {
    let mut out = format!("<{name}>");
    for (k, v) in map {
        match v {
            Value::Object(inner) => out.push_str(&xml_of(k, inner)),
            other => out.push_str(&format!(
                "<{k}>{}</{k}>",
                as_text(other)
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
            )),
        }
    }
    out.push_str(&format!("</{name}>"));
    out
}

/// Picks the media type to use from a content map: JSON first, then form,
/// multipart, XML, then whatever comes first.
fn pick_media(content: &Map<String, Value>) -> Option<(&String, &Value)> {
    let order = [
        "json",
        "x-www-form-urlencoded",
        "multipart/form-data",
        "xml",
        "text/plain",
    ];
    for want in order {
        if let Some(found) = content
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase().contains(want))
        {
            return Some(found);
        }
    }
    content.iter().next()
}

/// Server URL of OpenAPI 3 with its variables at their defaults.
fn server_url(server: &Value) -> Option<String> {
    let mut url = server.get("url")?.as_str()?.to_string();
    if let Some(vars) = server.get("variables").and_then(Value::as_object) {
        for (name, var) in vars {
            url = url.replace(&format!("{{{name}}}"), &text(var.get("default")));
        }
    }
    Some(url)
}

/// Headers and query parameters the security requirements ask for, with
/// placeholders the tester replaces.
fn security_params(r: &Resolver<'_>, doc: &Value, op: &Value, v3: bool) -> Vec<ApiParam> {
    let requirements = op.get("security").or_else(|| doc.get("security"));
    let Some(first) = requirements
        .and_then(Value::as_array)
        .and_then(|a| a.first())
    else {
        return Vec::new();
    };
    let schemes = if v3 {
        doc.pointer("/components/securitySchemes")
    } else {
        doc.get("securityDefinitions")
    };
    let mut out = Vec::new();
    for name in first
        .as_object()
        .map(|o| o.keys().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        let Some(scheme) = schemes.and_then(|s| s.get(name)).and_then(|s| r.resolve(s)) else {
            continue;
        };
        let kind = text(scheme.get("type"));
        let sub = text(scheme.get("scheme")).to_ascii_lowercase();
        let param = match kind.as_str() {
            "apiKey" => location(&text(scheme.get("in"))).map(|loc| ApiParam {
                name: text(scheme.get("name")),
                location: loc,
                required: true,
                example: "API_KEY".into(),
            }),
            "http" if sub == "bearer" => Some(ApiParam {
                name: "Authorization".into(),
                location: ParamLocation::Header,
                required: true,
                example: "Bearer TOKEN".into(),
            }),
            "http" if sub == "basic" => Some(ApiParam {
                name: "Authorization".into(),
                location: ParamLocation::Header,
                required: true,
                example: "Basic dXNlcjpwYXNzd29yZA==".into(),
            }),
            "basic" => Some(ApiParam {
                name: "Authorization".into(),
                location: ParamLocation::Header,
                required: true,
                example: "Basic dXNlcjpwYXNzd29yZA==".into(),
            }),
            "oauth2" | "openIdConnect" => Some(ApiParam {
                name: "Authorization".into(),
                location: ParamLocation::Header,
                required: true,
                example: "Bearer TOKEN".into(),
            }),
            _ => None,
        };
        out.extend(param.filter(|p| !p.name.is_empty()));
    }
    out
}

fn endpoint_name(op: &Value, method: &str, path: &str) -> String {
    let id = text(op.get("operationId"));
    if !id.is_empty() {
        return id;
    }
    let summary = text(op.get("summary"));
    if !summary.is_empty() {
        return summary.chars().take(200).collect();
    }
    format!("{} {path}", method.to_ascii_uppercase())
}

/// OpenAPI 3.0 / 3.1.
pub(crate) fn parse_v3(doc: &Value, base: Option<&url::Url>) -> Result<ApiDefinition, String> {
    let r = Resolver { root: doc };
    let mut warnings = Vec::new();
    let mut endpoints = Vec::new();
    let root_server = doc
        .get("servers")
        .and_then(Value::as_array)
        .and_then(|s| s.first())
        .and_then(server_url);
    let paths = doc
        .get("paths")
        .and_then(Value::as_object)
        .ok_or("no paths in the definition")?;
    for (path, item) in paths {
        let Some(item) = r.resolve(item) else {
            warnings.push(format!("Unresolved path item {path}"));
            continue;
        };
        let path_server = item
            .get("servers")
            .and_then(Value::as_array)
            .and_then(|s| s.first())
            .and_then(server_url);
        for method in METHODS {
            let Some(op) = item.get(method) else {
                continue;
            };
            if endpoints.len() >= MAX_ENDPOINTS {
                warnings.push(format!(
                    "Only the first {MAX_ENDPOINTS} endpoints were kept."
                ));
                break;
            }
            let server = op
                .get("servers")
                .and_then(Value::as_array)
                .and_then(|s| s.first())
                .and_then(server_url)
                .or_else(|| path_server.clone())
                .or_else(|| root_server.clone());
            let mut params: Vec<ApiParam> = merged_params(&r, item, op, &mut warnings)
                .into_iter()
                .filter_map(|p| {
                    Some(ApiParam {
                        name: text(p.get("name")),
                        location: location(&text(p.get("in")))?,
                        required: p.get("required").and_then(Value::as_bool).unwrap_or(false),
                        example: param_example(&r, p),
                    })
                })
                .collect();
            params.extend(security_params(&r, doc, op, true));
            let mut content_type = None;
            let mut body = None;
            if let Some(rb) = op.get("requestBody").and_then(|b| r.resolve(b))
                && let Some((media_type, media)) = rb
                    .get("content")
                    .and_then(Value::as_object)
                    .and_then(pick_media)
            {
                let value = media
                    .get("example")
                    .cloned()
                    .or_else(|| {
                        media
                            .get("examples")
                            .and_then(Value::as_object)
                            .and_then(|m| m.values().next())
                            .and_then(|e| r.resolve(e))
                            .and_then(|e| e.get("value"))
                            .cloned()
                    })
                    .or_else(|| media.get("schema").map(|s| r.example(s)))
                    .unwrap_or(Value::Null);
                if let Value::Object(map) = &value {
                    for name in map.keys() {
                        params.push(ApiParam {
                            name: name.clone(),
                            location: ParamLocation::Body,
                            required: false,
                            example: as_text(&map[name]),
                        });
                    }
                }
                let ct = if media_type.to_ascii_lowercase().starts_with("multipart/") {
                    format!("{media_type}; boundary={BOUNDARY}")
                } else {
                    media_type.clone()
                };
                body = Some(body_for(media_type, &value));
                content_type = Some(ct);
            }
            let built = build(path, &params);
            let Some(url) = join_url(server.as_deref(), &built.path, base) else {
                return Err(
                    "the definition has no absolute server URL: give a base URL (for example \
                     https://api.example.com)"
                        .into(),
                );
            };
            let mut headers = built.headers;
            if !built.cookies.is_empty() {
                let cookie = built
                    .cookies
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                headers.push(("Cookie".into(), cookie));
            }
            endpoints.push(ApiEndpoint {
                method: method.to_ascii_uppercase(),
                url: finish_url(url, &built.query),
                path: path.clone(),
                name: endpoint_name(op, method, path),
                group: op
                    .get("tags")
                    .and_then(Value::as_array)
                    .and_then(|t| t.first())
                    .map(as_text)
                    .unwrap_or_default(),
                params,
                headers,
                content_type,
                body,
                read_only: None,
            });
        }
    }
    Ok(ApiDefinition {
        format: SpecFormat::OpenApi3,
        title: text(doc.pointer("/info/title")),
        version: text(doc.pointer("/info/version")),
        endpoints,
        warnings,
    })
}

/// Swagger 2.0.
pub(crate) fn parse_v2(doc: &Value, base: Option<&url::Url>) -> Result<ApiDefinition, String> {
    let r = Resolver { root: doc };
    let mut warnings = Vec::new();
    let mut endpoints = Vec::new();
    let server = doc.get("host").and_then(Value::as_str).map(|host| {
        let scheme = doc
            .get("schemes")
            .and_then(Value::as_array)
            .and_then(|s| {
                s.iter()
                    .filter_map(Value::as_str)
                    .find(|x| *x == "https")
                    .or_else(|| s.iter().filter_map(Value::as_str).next())
            })
            .unwrap_or("https");
        format!("{scheme}://{host}{}", text(doc.get("basePath")))
    });
    let server = server.or_else(|| {
        let bp = text(doc.get("basePath"));
        (!bp.is_empty()).then_some(bp)
    });
    let root_consumes = doc.get("consumes").and_then(Value::as_array);
    let paths = doc
        .get("paths")
        .and_then(Value::as_object)
        .ok_or("no paths in the definition")?;
    for (path, item) in paths {
        let Some(item) = r.resolve(item) else {
            warnings.push(format!("Unresolved path item {path}"));
            continue;
        };
        for method in METHODS {
            let Some(op) = item.get(method) else {
                continue;
            };
            if endpoints.len() >= MAX_ENDPOINTS {
                warnings.push(format!(
                    "Only the first {MAX_ENDPOINTS} endpoints were kept."
                ));
                break;
            }
            let raw_params = merged_params(&r, item, op, &mut warnings);
            let consumes: Vec<String> = op
                .get("consumes")
                .and_then(Value::as_array)
                .or(root_consumes)
                .map(|c| {
                    c.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let mut params = Vec::new();
            let mut body_value = None;
            let mut form = Map::new();
            let mut has_file = false;
            for p in raw_params {
                let kind = text(p.get("in"));
                if kind == "body" {
                    body_value = Some(p.get("schema").map(|s| r.example(s)).unwrap_or(Value::Null));
                    continue;
                }
                let example = param_example(&r, p);
                if kind == "formData" {
                    has_file |= text(p.get("type")) == "file";
                    form.insert(text(p.get("name")), Value::String(example.clone()));
                }
                let Some(loc) = location(&kind) else {
                    continue;
                };
                params.push(ApiParam {
                    name: text(p.get("name")),
                    location: loc,
                    required: p.get("required").and_then(Value::as_bool).unwrap_or(false),
                    example,
                });
            }
            params.extend(security_params(&r, doc, op, false));
            let (content_type, body) = if let Some(value) = body_value {
                let ct = consumes
                    .iter()
                    .find(|c| c.contains("json"))
                    .or_else(|| consumes.first())
                    .cloned()
                    .unwrap_or_else(|| "application/json".into());
                if let Value::Object(map) = &value {
                    for (name, v) in map {
                        params.push(ApiParam {
                            name: name.clone(),
                            location: ParamLocation::Body,
                            required: false,
                            example: as_text(v),
                        });
                    }
                }
                (Some(ct.clone()), Some(body_for(&ct, &value)))
            } else if !form.is_empty() {
                let multipart = has_file || consumes.iter().any(|c| c.starts_with("multipart/"));
                if multipart {
                    (
                        Some(format!("multipart/form-data; boundary={BOUNDARY}")),
                        Some(body_for("multipart/form-data", &Value::Object(form))),
                    )
                } else {
                    let ct = "application/x-www-form-urlencoded";
                    (Some(ct.into()), Some(body_for(ct, &Value::Object(form))))
                }
            } else {
                (None, None)
            };
            let built = build(path, &params);
            let Some(url) = join_url(server.as_deref(), &built.path, base) else {
                return Err("the definition has no host: give a base URL (for example \
                     https://api.example.com)"
                    .into());
            };
            let mut headers = built.headers;
            if !built.cookies.is_empty() {
                let cookie = built
                    .cookies
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                headers.push(("Cookie".into(), cookie));
            }
            endpoints.push(ApiEndpoint {
                method: method.to_ascii_uppercase(),
                url: finish_url(url, &built.query),
                path: path.clone(),
                name: endpoint_name(op, method, path),
                group: op
                    .get("tags")
                    .and_then(Value::as_array)
                    .and_then(|t| t.first())
                    .map(as_text)
                    .unwrap_or_default(),
                params,
                headers,
                content_type,
                body,
                read_only: None,
            });
        }
    }
    Ok(ApiDefinition {
        format: SpecFormat::Swagger2,
        title: text(doc.pointer("/info/title")),
        version: text(doc.pointer("/info/version")),
        endpoints,
        warnings,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use crate::{ParamLocation, SpecFormat, parse, request_text};

    const PETSTORE_YAML: &str = r#"
openapi: 3.0.3
info:
  title: Pets
  version: "1.2"
servers:
  - url: https://{region}.pets.test/v1
    variables:
      region:
        default: eu
security:
  - key: []
components:
  securitySchemes:
    key:
      type: apiKey
      in: header
      name: X-Api-Key
  parameters:
    Limit:
      name: limit
      in: query
      schema: {type: integer, minimum: 10}
  schemas:
    Pet:
      type: object
      required: [name]
      properties:
        name: {type: string, example: Rex}
        tag: {type: string}
paths:
  /pets:
    get:
      operationId: listPets
      tags: [pets]
      parameters:
        - $ref: '#/components/parameters/Limit'
    post:
      summary: Add a pet
      requestBody:
        content:
          application/json:
            schema: {$ref: '#/components/schemas/Pet'}
  /pets/{petId}:
    parameters:
      - {name: petId, in: path, required: true, schema: {type: string, example: "a b"}}
    delete:
      security: []
      parameters:
        - {name: session, in: cookie, schema: {type: string}}
"#;

    #[test]
    fn reads_openapi_3_yaml() {
        let def = parse(PETSTORE_YAML, None).unwrap();
        assert_eq!(def.format, SpecFormat::OpenApi3);
        assert_eq!((def.title.as_str(), def.version.as_str()), ("Pets", "1.2"));
        assert_eq!(def.endpoints.len(), 3, "{:?}", def.warnings);
        let list = &def.endpoints[0];
        assert_eq!(list.method, "GET");
        assert_eq!(list.url, "https://eu.pets.test/v1/pets?limit=10");
        assert_eq!(list.name, "listPets");
        assert_eq!(list.group, "pets");
        assert!(
            list.headers
                .contains(&("X-Api-Key".into(), "API_KEY".into()))
        );
        let add = &def.endpoints[1];
        assert_eq!(add.name, "Add a pet");
        assert_eq!(add.content_type.as_deref(), Some("application/json"));
        assert!(add.body.as_deref().unwrap().contains("\"name\": \"Rex\""));
        assert!(
            add.params
                .iter()
                .any(|p| p.name == "tag" && p.location == ParamLocation::Body)
        );
        let del = &def.endpoints[2];
        assert_eq!(del.url, "https://eu.pets.test/v1/pets/a%20b");
        assert!(
            del.headers
                .contains(&("Cookie".into(), "session=string".into()))
        );
        assert!(
            !del.headers.iter().any(|(k, _)| k == "X-Api-Key"),
            "security: [] turns it off"
        );
        let text = request_text(add);
        assert!(text.starts_with("POST /v1/pets HTTP/1.1\r\nHost: eu.pets.test\r\n"));
    }

    #[test]
    fn relative_servers_need_a_base_url() {
        let spec = r#"{"openapi":"3.1.0","info":{"title":"t","version":"1"},
            "servers":[{"url":"/api"}],"paths":{"/x":{"get":{}}}}"#;
        assert!(parse(spec, None).unwrap_err().contains("base URL"));
        let def = parse(spec, Some("http://localhost:9090/")).unwrap();
        assert_eq!(def.endpoints[0].url, "http://localhost:9090/api/x");
        assert_eq!(def.endpoints[0].name, "GET /x");
    }

    #[test]
    fn reads_swagger_2_with_body_form_and_files() {
        let spec = r##"{
          "swagger": "2.0", "info": {"title": "Old", "version": "0.1"},
          "host": "old.test", "basePath": "/api", "schemes": ["http", "https"],
          "securityDefinitions": {"b": {"type": "basic"}}, "security": [{"b": []}],
          "definitions": {"Item": {"type": "object", "properties": {"n": {"type": "integer"}}}},
          "paths": {
            "/items/{id}": {
              "put": {"parameters": [
                {"name": "id", "in": "path", "required": true, "type": "integer", "x-example": 5},
                {"name": "body", "in": "body", "schema": {"$ref": "#/definitions/Item"}}
              ]},
              "post": {"consumes": ["application/x-www-form-urlencoded"], "parameters": [
                {"name": "id", "in": "path", "type": "integer"},
                {"name": "q", "in": "formData", "type": "string", "default": "a&b"}
              ]}
            },
            "/upload": {"post": {"parameters": [
              {"name": "file", "in": "formData", "type": "file"}
            ]}}
          }
        }"##;
        let def = parse(spec, None).unwrap();
        assert_eq!(def.format, SpecFormat::Swagger2);
        let put = &def.endpoints[0];
        assert_eq!(put.url, "https://old.test/api/items/5");
        assert_eq!(put.body.as_deref(), Some("{\n  \"n\": 1\n}"));
        assert!(
            put.headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v.starts_with("Basic "))
        );
        let post = &def.endpoints[1];
        assert_eq!(post.body.as_deref(), Some("q=a%26b"));
        assert_eq!(post.url, "https://old.test/api/items/1");
        let upload = &def.endpoints[2];
        assert!(
            upload
                .content_type
                .as_deref()
                .unwrap()
                .starts_with("multipart/form-data; boundary=")
        );
        assert!(upload.body.as_deref().unwrap().contains("name=\"file\""));
    }
}
