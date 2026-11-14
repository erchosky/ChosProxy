//! The GraphQL workspace (M27, matrix SCANNER-015).
//!
//! From a GraphQL request in the history, the tester asks for the schema:
//! the same request (headers, cookies) is sent once with the standard
//! introspection query, like a Repeater send (scope-checked, stored in the
//! history). The schema becomes an API definition whose endpoints are the
//! schema's queries and mutations, each with an example operation, so the
//! API workspace can open them in Repeater or the Fuzzer and send them.
//! Queries count as read-only; mutations are only sent in bulk when the
//! tester asks for methods that change data.

use super::Engine;
use crate::AppError;
use chosproxy_apispec::graphql::{
    GqlOperation, GqlSchema, INTROSPECTION_QUERY, build_operation, parse_introspection,
    request_body,
};
use chosproxy_apispec::{ApiEndpoint, ApiParam, ParamLocation};
use chosproxy_core::Source;
use chosproxy_core::time::now_ms;
use chosproxy_http::ResponseHead;
use chosproxy_http::content::{body_text, split_message};
use chosproxy_request::{Destination, SendLimits};
use chosproxy_storage::{ApiDefinitionInfo, NewApiDefinition, RepeaterTarget, StorageError};
use std::time::Duration;

/// Most root fields turned into endpoints.
const MAX_OPERATIONS: usize = 2000;
/// Largest introspection answer read.
const MAX_ANSWER: usize = 32 * 1024 * 1024;
/// Headers of the original request not carried over.
const DROPPED: [&str; 6] = [
    "host",
    "content-length",
    "content-type",
    "transfer-encoding",
    "connection",
    "accept-encoding",
];

/// The parts of a stored GraphQL request reused for new operations.
struct Base {
    target: RepeaterTarget,
    /// `scheme://host:port/path` without the query.
    url: String,
    path: String,
    headers: Vec<(String, String)>,
}

fn storage_error(e: StorageError) -> AppError {
    match e {
        StorageError::InvalidQuery(m) => AppError::Invalid(m),
        other => other.into(),
    }
}

/// The request text of a GraphQL operation built on `base`.
fn operation_text(base: &Base, body: &str) -> String {
    let host = match (base.target.scheme.as_str(), base.target.port) {
        ("https", 443) | ("http", 80) => base.target.host.clone(),
        (_, port) => format!("{}:{port}", base.target.host),
    };
    let mut out = format!("POST {} HTTP/1.1\nHost: {host}\n", base.path);
    for (k, v) in &base.headers {
        out.push_str(&format!("{k}: {v}\n"));
    }
    out.push_str(&format!(
        "Content-Type: application/json\nContent-Length: {}\n\n{body}",
        body.len()
    ));
    out
}

impl Engine {
    async fn graphql_base(&self, exchange_id: i64) -> Result<Base, AppError> {
        let db = self.db()?;
        let ex = tokio::task::spawn_blocking(move || db.exchanges_bulk(&[exchange_id]))
            .await??
            .pop()
            .ok_or_else(|| AppError::Invalid(format!("no exchange {exchange_id}")))?;
        let row = &ex.row;
        let text = String::from_utf8_lossy(&ex.request);
        if !chosproxy_apispec::graphql::is_graphql_request(&text) {
            return Err(AppError::Invalid(
                "this request does not look like a GraphQL call (a JSON body with a \"query\")"
                    .into(),
            ));
        }
        let (head, _) = text
            .split_once("\r\n\r\n")
            .or_else(|| text.split_once("\n\n"))
            .unwrap_or((&text, ""));
        let headers = head
            .lines()
            .skip(1)
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .filter(|(k, _)| !k.is_empty() && !DROPPED.contains(&k.to_ascii_lowercase().as_str()))
            .collect();
        let path = row.path.split(['?', '#']).next().unwrap_or("/").to_string();
        let path = if path.starts_with('/') {
            path
        } else {
            format!("/{path}")
        };
        let target = RepeaterTarget {
            scheme: row.scheme.clone(),
            host: row.host.clone(),
            port: row.port,
        };
        Ok(Base {
            url: format!("{}://{}:{}{path}", row.scheme, row.host, row.port),
            target,
            path,
            headers,
        })
    }

    /// Sends the introspection query on the endpoint of a stored GraphQL
    /// request and keeps the schema as an API definition.
    pub async fn graphql_introspect(
        &self,
        exchange_id: i64,
    ) -> Result<ApiDefinitionInfo, AppError> {
        self.ensure_writable()?;
        let base = self.graphql_base(exchange_id).await?;
        self.ensure_in_scope(&base.target, &base.path, "GraphQL workspace")?;
        let body = request_body(INTROSPECTION_QUERY, "{}", Some("IntrospectionQuery"));
        let text = operation_text(&base, &body);
        let request = crate::repeater::request_bytes(&text, true, true, true)
            .map_err(|e| AppError::Invalid(e.to_string()))?;
        let security = self
            .security_for_host(&base.target.host, base.target.scheme == "https")
            .await
            .map_err(AppError::Invalid)?;
        let destination = Destination {
            host: base.target.host.clone(),
            port: base.target.port,
            tls_name: base.target.host.clone(),
        };
        let limits = SendLimits {
            response_timeout: Duration::from_secs(30),
            body_idle_timeout: Duration::from_secs(30),
            max_response_bytes: MAX_ANSWER,
        };
        let started_at = now_ms();
        let outcome = self
            .session_send(
                "repeater",
                Source::Repeater,
                &destination,
                &security,
                &request,
                &limits,
            )
            .await;
        let mut exchange = crate::repeater::captured(
            Source::Repeater,
            &base.target,
            request,
            started_at,
            &outcome,
        );
        exchange.notes = Some(match exchange.notes.take() {
            Some(n) => format!("GraphQL introspection (GraphQL workspace). {n}"),
            None => "GraphQL introspection (GraphQL workspace).".into(),
        });
        {
            let db = self.db()?;
            tokio::task::spawn_blocking(move || db.insert(exchange)).await??;
        }
        let sent = outcome
            .map_err(|e| AppError::Invalid(format!("introspection failed: {}", e.message)))?;
        let (head, raw_body) = split_message(&sent.response);
        let parsed = ResponseHead::parse(head.to_vec())
            .map_err(|e| AppError::Invalid(format!("unreadable answer: {e}")))?;
        let answer = body_text(&parsed, raw_body, MAX_ANSWER)
            .ok_or_else(|| AppError::Invalid("the answer has no readable body".into()))?;
        let schema = parse_introspection(&answer).map_err(AppError::Invalid)?;
        self.graphql_store(&base, &schema).await
    }

    /// Keeps a schema from a saved introspection answer (a JSON file, for
    /// servers with introspection turned off) for the endpoint of a stored
    /// GraphQL request. Nothing is sent.
    pub async fn graphql_import_schema(
        &self,
        exchange_id: i64,
        path: &std::path::Path,
    ) -> Result<ApiDefinitionInfo, AppError> {
        let meta = std::fs::metadata(path)
            .map_err(|e| AppError::Invalid(format!("{}: {e}", path.display())))?;
        if meta.len() > MAX_ANSWER as u64 {
            return Err(AppError::Invalid("the file is larger than 32 MB".into()));
        }
        let answer = std::fs::read_to_string(path)
            .map_err(|e| AppError::Invalid(format!("{}: {e}", path.display())))?;
        let base = self.graphql_base(exchange_id).await?;
        let schema = parse_introspection(&answer).map_err(AppError::Invalid)?;
        self.graphql_store(&base, &schema).await
    }

    async fn graphql_store(
        &self,
        base: &Base,
        schema: &GqlSchema,
    ) -> Result<ApiDefinitionInfo, AppError> {
        let mut endpoints = Vec::new();
        let mut warnings = Vec::new();
        for (root, read_only) in [("query", true), ("mutation", false)] {
            let root_name = match root {
                "query" => schema.query_type.as_deref(),
                _ => schema.mutation_type.as_deref(),
            };
            let Some(root_type) = root_name.and_then(|n| schema.get(n)) else {
                continue;
            };
            for field in &root_type.fields {
                if endpoints.len() >= MAX_OPERATIONS {
                    warnings.push(format!(
                        "Only the first {MAX_OPERATIONS} operations were kept."
                    ));
                    break;
                }
                let op = match build_operation(schema, root, &field.name) {
                    Ok(op) => op,
                    Err(e) => {
                        warnings.push(e);
                        continue;
                    }
                };
                endpoints.push(ApiEndpoint {
                    method: "POST".into(),
                    url: base.url.clone(),
                    path: base.path.clone(),
                    name: field.name.clone(),
                    group: if read_only {
                        "Query".into()
                    } else {
                        "Mutation".into()
                    },
                    params: field
                        .args
                        .iter()
                        .map(|a| ApiParam {
                            name: a.name.clone(),
                            location: ParamLocation::Body,
                            required: a.type_ref.ends_with('!'),
                            example: a.type_ref.clone(),
                        })
                        .collect(),
                    headers: base.headers.clone(),
                    content_type: Some("application/json".into()),
                    body: Some(request_body(&op.query, &op.variables, None)),
                    read_only: Some(read_only),
                });
            }
        }
        if endpoints.is_empty() {
            return Err(AppError::Invalid(
                "the schema has no queries or mutations".into(),
            ));
        }
        if schema.subscription_type.is_some() {
            warnings.push("Subscriptions are listed in the schema but not as endpoints.".into());
        }
        let endpoints_json = serde_json::to_string(&endpoints)
            .map_err(|e| AppError::Invalid(format!("cannot store the operations: {e}")))?;
        let schema_json = serde_json::to_string(schema)
            .map_err(|e| AppError::Invalid(format!("cannot store the schema: {e}")))?;
        let new = NewApiDefinition {
            name: format!("GraphQL {}", base.url),
            format: "graphql".into(),
            title: format!("GraphQL at {}{}", base.target.host, base.path),
            version: String::new(),
            base_url: base.url.clone(),
            endpoint_count: i64::try_from(endpoints.len()).unwrap_or(i64::MAX),
            warnings,
            endpoints_json,
            schema_json,
        };
        let db = self.db()?;
        let info = tokio::task::spawn_blocking(move || db.api_definition_add(new))
            .await?
            .map_err(storage_error)?;
        self.audit(
            "graphql.schema",
            &format!("{} ({} operations)", base.url, info.endpoint_count),
        );
        Ok(info)
    }

    /// The schema of a GraphQL definition.
    pub async fn graphql_schema(&self, id: i64) -> Result<GqlSchema, AppError> {
        let db = self.db()?;
        let json = tokio::task::spawn_blocking(move || db.api_definition_schema(id))
            .await??
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::Invalid(format!("definition {id} has no GraphQL schema")))?;
        serde_json::from_str(&json)
            .map_err(|e| AppError::Invalid(format!("unreadable schema: {e}")))
    }

    /// An example operation for a field of the query or mutation type.
    pub async fn graphql_build(
        &self,
        id: i64,
        root: &str,
        field: &str,
    ) -> Result<GqlOperation, AppError> {
        let schema = self.graphql_schema(id).await?;
        build_operation(&schema, root, field).map_err(AppError::Invalid)
    }

    /// A request carrying an operation, built on the definition's endpoint
    /// and headers, for Repeater or the Fuzzer.
    pub async fn graphql_request(
        &self,
        id: i64,
        query: &str,
        variables: &str,
    ) -> Result<super::ApiRequest, AppError> {
        let detail = self.api_definition(id).await?;
        let first = detail
            .endpoints
            .first()
            .ok_or_else(|| AppError::Invalid("the definition has no endpoint".into()))?;
        let url = url::Url::parse(&first.url).map_err(|e| AppError::Invalid(e.to_string()))?;
        let target = RepeaterTarget {
            scheme: url.scheme().to_string(),
            host: url.host_str().unwrap_or_default().to_string(),
            port: url.port_or_known_default().unwrap_or(443),
        };
        let base = Base {
            url: first.url.clone(),
            path: url.path().to_string(),
            headers: first.headers.clone(),
            target: target.clone(),
        };
        Ok(super::ApiRequest {
            target,
            request: operation_text(&base, &request_body(query, variables, None)),
        })
    }
}
