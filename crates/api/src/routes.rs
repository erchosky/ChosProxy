//! The one table of the API's routes (M31b): the OpenAPI description is
//! generated from it, and a test checks it against the router.

use serde_json::{Map, Value, json};

/// One route under `/v1`.
#[derive(Debug, Clone, Copy)]
pub struct Route {
    /// HTTP method.
    pub method: &'static str,
    /// Path under `/v1`, with `{name}` for a path parameter.
    pub path: &'static str,
    /// What it does, in one line.
    pub summary: &'static str,
    /// The JSON body it takes, described in words (none: no body).
    pub body: Option<&'static str>,
    /// Query parameters: name and description.
    pub query: &'static [(&'static str, &'static str)],
}

const fn get(path: &'static str, summary: &'static str) -> Route {
    Route {
        method: "GET",
        path,
        summary,
        body: None,
        query: &[],
    }
}

const fn post(path: &'static str, summary: &'static str, body: Option<&'static str>) -> Route {
    Route {
        method: "POST",
        path,
        summary,
        body,
        query: &[],
    }
}

/// Every route, in the order of the documentation.
pub const ROUTES: &[Route] = &[
    get(
        "/status",
        "Engine version, the open project and the listeners.",
    ),
    get("/openapi.json", "This description (OpenAPI 3.1)."),
    get(
        "/events",
        "Engine events as Server-Sent Events, until the client disconnects.",
    ),
    post("/project/temporary", "Opens a new temporary project.", None),
    post(
        "/project/open",
        "Opens a project file.",
        Some("`path`; `password` for an encrypted project."),
    ),
    post(
        "/project/create",
        "Creates a project file and opens it.",
        Some("`path`; `password` to encrypt it."),
    ),
    post("/project/close", "Closes the open project.", None),
    get("/listeners", "The proxy listeners and their state."),
    post(
        "/listeners/{id}/{action}",
        "Starts or stops a listener (`action`: start, stop).",
        None,
    ),
    Route {
        method: "GET",
        path: "/history",
        summary: "A page of the history, newest last.",
        body: None,
        query: &[
            ("limit", "Rows to return (1-1000, default 100)."),
            ("offset", "Rows to skip."),
            ("search", "Text the URL must contain."),
            ("inScope", "`true`: only URLs in scope."),
        ],
    },
    post(
        "/history/query",
        "A page of the history with every filter of the UI.",
        Some("A history query (`limit`, `offset`, `search`, `sources`, `methods`…)."),
    ),
    get(
        "/history/{id}",
        "One exchange: request, response and notes.",
    ),
    get("/findings", "The findings."),
    post(
        "/findings/query",
        "The findings that match a filter.",
        Some("A finding query (`search`, `severities`, `statuses`…)."),
    ),
    get("/scope", "The scope rules."),
    Route {
        method: "PUT",
        path: "/scope",
        summary: "Replaces the scope rules.",
        body: Some("The scope, as `GET /scope` returns it."),
        query: &[],
    },
    post(
        "/scope/add",
        "Adds a URL to the scope (or excludes it).",
        Some("`url`; `exclude` to exclude it; `path` to keep its path as a prefix."),
    ),
    post(
        "/crawl",
        "Starts a crawl task (in scope, rate-limited).",
        Some("A crawl configuration (`startUrls`, limits)."),
    ),
    get("/scan/configs", "The scan configurations."),
    post(
        "/scan",
        "Starts a scan task: crawl, passive audit, JavaScript analysis.",
        Some("`startUrls` and `config` (one of `GET /scan/configs`)."),
    ),
    post(
        "/send",
        "Sends one request, like Repeater, and stores it in the history.",
        Some("`target` (`scheme`, `host`, `port`) and `request` (the raw request text)."),
    ),
    get("/api-definitions", "The imported API definitions."),
    post(
        "/api-definitions",
        "Imports an API definition (OpenAPI, Postman, WSDL).",
        Some("`name`, `text` (the file's content) and optionally `baseUrl`."),
    ),
    get(
        "/api-definitions/{id}",
        "One API definition with its endpoints.",
    ),
    get("/tasks", "The tasks of the project."),
    get("/tasks/{id}", "One task with its log."),
    post(
        "/tasks/{id}/{action}",
        "Controls a task (`action`: pause, resume, cancel).",
        None,
    ),
    post(
        "/report",
        "The report of the findings, as text.",
        Some("`format` (markdown, html, json), `title`, `includeEvidence`; all optional."),
    ),
];

/// Names of the `{…}` parameters of a path.
fn path_params(path: &str) -> Vec<&str> {
    path.split('/')
        .filter_map(|s| s.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
        .collect()
}

/// The OpenAPI 3.1 description of the API listening on `port`.
pub fn openapi(port: u16) -> Value {
    let mut paths: Map<String, Value> = Map::new();
    for r in ROUTES {
        let mut parameters: Vec<Value> = path_params(r.path)
            .into_iter()
            .map(|name| {
                json!({
                    "name": name, "in": "path", "required": true,
                    "schema": { "type": if name == "action" { "string" } else { "integer" } },
                })
            })
            .collect();
        // Listener ids are names, not numbers.
        if r.path.starts_with("/listeners/") {
            parameters[0]["schema"]["type"] = json!("string");
        }
        parameters.extend(r.query.iter().map(|(name, description)| {
            json!({
                "name": name, "in": "query", "required": false,
                "description": description, "schema": { "type": "string" },
            })
        }));
        let events = r.path == "/events";
        let mut operation = json!({
            "summary": r.summary,
            "operationId": format!(
                "{}{}",
                r.method.to_ascii_lowercase(),
                r.path.replace(['/', '{', '}', '.', '-'], "_")
            ),
            "parameters": parameters,
            "responses": {
                "200": {
                    "description": "OK",
                    "content": if events {
                        json!({ "text/event-stream": { "schema": { "type": "string" } } })
                    } else {
                        json!({ "application/json": { "schema": {} } })
                    },
                },
                "default": {
                    "description": "An error: `{\"error\": \"…\"}`.",
                    "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } },
                },
            },
        });
        if let Some(body) = r.body {
            operation["requestBody"] = json!({
                "description": body,
                "content": { "application/json": { "schema": { "type": "object" } } },
            });
        }
        let entry = paths
            .entry(format!("/v1{}", r.path))
            .or_insert_with(|| json!({}));
        entry[r.method.to_ascii_lowercase()] = operation;
    }
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "ChosProxy automation API",
            "version": format!("{}.0", crate::API_VERSION),
            "description": "A JSON API over the ChosProxy engine, on loopback only. Every \
                call needs the token; requests with an Origin header are refused. See docs/API.md.",
        },
        "servers": [{ "url": format!("http://127.0.0.1:{port}") }],
        "security": [{ "token": [] }],
        "paths": paths,
        "components": {
            "securitySchemes": { "token": { "type": "http", "scheme": "bearer" } },
            "schemas": {
                "Error": {
                    "type": "object",
                    "required": ["error"],
                    "properties": { "error": { "type": "string" } },
                },
            },
        },
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn describes_every_route_once() {
        let doc = openapi(8790);
        assert_eq!(doc["openapi"], "3.1.0");
        let mut operations = 0;
        for (path, item) in doc["paths"].as_object().unwrap() {
            assert!(path.starts_with("/v1/"), "{path}");
            for (method, op) in item.as_object().unwrap() {
                operations += 1;
                assert!(op["summary"].as_str().is_some_and(|s| !s.is_empty()));
                let declared = op["parameters"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|p| p["in"] == "path")
                    .count();
                assert_eq!(declared, path.matches('{').count(), "{method} {path}");
            }
        }
        assert_eq!(operations, ROUTES.len(), "a route is listed twice");
        let ids: std::collections::HashSet<String> = doc["paths"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|i| i.as_object().unwrap().values())
            .map(|op| op["operationId"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids.len(), ROUTES.len());
        assert_eq!(
            doc["paths"]["/v1/listeners/{id}/{action}"]["post"]["parameters"][0]["schema"]["type"],
            "string"
        );
        assert_eq!(doc["servers"][0]["url"], "http://127.0.0.1:8790");
    }

    /// The router has exactly the arms the table lists (the events stream
    /// and this description are answered before the router).
    #[test]
    fn the_table_matches_the_router() {
        let source = include_str!("lib.rs");
        let arms = source.matches("(&Method::").count();
        assert_eq!(arms, ROUTES.len() - 2, "update ROUTES in routes.rs");
        for r in ROUTES
            .iter()
            .filter(|r| r.path != "/events" && r.path != "/openapi.json")
        {
            let pattern: Vec<String> = r
                .path
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| {
                    if s.starts_with('{') {
                        s.trim_matches(['{', '}']).to_string()
                    } else {
                        format!("\"{s}\"")
                    }
                })
                .collect();
            let arm = format!("(&Method::{}, [{}])", r.method, pattern.join(", "));
            assert!(source.contains(&arm), "the router has no arm {arm}");
        }
    }
}
