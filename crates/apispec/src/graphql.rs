//! GraphQL (M27, matrix SCANNER-015): recognizing GraphQL requests, the
//! standard introspection query, reading its answer into a schema, and
//! writing an operation for a field with example variables.
//!
//! The introspection query is the one every GraphQL tool sends: it reads
//! the schema and changes nothing.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use ts_rs::TS;

/// The standard introspection query (directives left out).
pub const INTROSPECTION_QUERY: &str = "query IntrospectionQuery { __schema { queryType { name } \
mutationType { name } subscriptionType { name } types { ...FullType } } } \
fragment FullType on __Type { kind name description fields(includeDeprecated: true) { name \
description args { ...InputValue } type { ...TypeRef } isDeprecated } inputFields { ...InputValue } \
interfaces { ...TypeRef } enumValues(includeDeprecated: true) { name } possibleTypes { ...TypeRef } } \
fragment InputValue on __InputValue { name description type { ...TypeRef } defaultValue } \
fragment TypeRef on __Type { kind name ofType { kind name ofType { kind name ofType { kind name \
ofType { kind name ofType { kind name ofType { kind name ofType { kind name } } } } } } } }";

/// Most types read from one schema.
const MAX_TYPES: usize = 5000;
/// Most fields listed per type.
const MAX_FIELDS: usize = 500;
/// Deepest selection written by the builder.
const MAX_SELECTION_DEPTH: usize = 3;
/// Most fields selected per object by the builder.
const MAX_SELECTED: usize = 20;

/// An argument or input field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "GraphqlArg")]
pub struct GqlArg {
    /// Name.
    pub name: String,
    /// Type as written in GraphQL (`[ID!]!`).
    pub type_ref: String,
    /// Default value, as GraphQL text.
    pub default_value: Option<String>,
}

/// A field of an object type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "GraphqlField")]
pub struct GqlField {
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Arguments.
    pub args: Vec<GqlArg>,
    /// Return type as written in GraphQL.
    pub type_ref: String,
    /// Marked deprecated.
    pub deprecated: bool,
}

/// A named type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "GraphqlType")]
pub struct GqlType {
    /// `OBJECT`, `INPUT_OBJECT`, `ENUM`, `SCALAR`, `INTERFACE` or `UNION`.
    pub kind: String,
    /// Name.
    pub name: String,
    /// Fields (objects and interfaces).
    pub fields: Vec<GqlField>,
    /// Input fields (input objects).
    pub input_fields: Vec<GqlArg>,
    /// Values (enums).
    pub enum_values: Vec<String>,
    /// Member types (unions and interfaces).
    pub possible_types: Vec<String>,
}

/// A schema read from an introspection answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "GraphqlSchema")]
pub struct GqlSchema {
    /// Name of the query root type.
    pub query_type: Option<String>,
    /// Name of the mutation root type.
    pub mutation_type: Option<String>,
    /// Name of the subscription root type.
    pub subscription_type: Option<String>,
    /// Types, without the introspection ones (`__…`).
    pub types: Vec<GqlType>,
}

/// An operation written by [`build_operation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "GraphqlOperation")]
pub struct GqlOperation {
    /// The operation text.
    pub query: String,
    /// Example variables (a JSON object as text).
    pub variables: String,
}

impl GqlSchema {
    /// A type by name.
    pub fn get(&self, name: &str) -> Option<&GqlType> {
        self.types.iter().find(|t| t.name == name)
    }
}

/// The JSON body of a GraphQL request.
pub fn request_body(query: &str, variables: &str, operation_name: Option<&str>) -> String {
    let vars: Value = serde_json::from_str(variables.trim())
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Map::new()));
    let mut body = json!({ "query": query, "variables": vars });
    if let Some(name) = operation_name.filter(|n| !n.is_empty()) {
        body["operationName"] = Value::String(name.to_string());
    }
    body.to_string()
}

/// Whether a request (text form) looks like a GraphQL call: a JSON body
/// with a `query` string that reads like GraphQL, or `?query=` on a path
/// mentioning graphql.
pub fn is_graphql_request(text: &str) -> bool {
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .unwrap_or((text, ""));
    let first = head.lines().next().unwrap_or_default().to_ascii_lowercase();
    if first.contains("graphql") && (first.contains("query=") || !body.trim().is_empty()) {
        return true;
    }
    let looks = |q: &str| {
        let q = q.trim_start();
        q.starts_with('{')
            || q.starts_with("query")
            || q.starts_with("mutation")
            || q.starts_with("subscription")
            || q.starts_with("fragment")
    };
    match serde_json::from_str::<Value>(body.trim()) {
        Ok(Value::Object(o)) => o.get("query").and_then(Value::as_str).is_some_and(looks),
        Ok(Value::Array(batch)) => batch
            .first()
            .and_then(|b| b.get("query"))
            .and_then(Value::as_str)
            .is_some_and(looks),
        _ => false,
    }
}

/// `{kind, name, ofType}` as GraphQL type text, and its named type.
fn type_text(t: &Value, depth: usize) -> (String, String) {
    if depth > 10 {
        return (String::new(), String::new());
    }
    let kind = t.get("kind").and_then(Value::as_str).unwrap_or_default();
    let of = t.get("ofType").unwrap_or(&Value::Null);
    match kind {
        "NON_NULL" => {
            let (inner, named) = type_text(of, depth + 1);
            (format!("{inner}!"), named)
        }
        "LIST" => {
            let (inner, named) = type_text(of, depth + 1);
            (format!("[{inner}]"), named)
        }
        _ => {
            let name = t
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            (name.clone(), name)
        }
    }
}

fn args_of(list: Option<&Value>) -> Vec<GqlArg> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_FIELDS)
        .map(|a| GqlArg {
            name: a
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            type_ref: type_text(a.get("type").unwrap_or(&Value::Null), 0).0,
            default_value: a
                .get("defaultValue")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
        .collect()
}

/// Reads an introspection answer (`{"data": {"__schema": …}}`, or the
/// `__schema` object itself).
pub fn parse_introspection(text: &str) -> Result<GqlSchema, String> {
    let value: Value =
        serde_json::from_str(text.trim()).map_err(|e| format!("the answer is not JSON: {e}"))?;
    let schema = value
        .pointer("/data/__schema")
        .or_else(|| value.get("__schema"))
        .ok_or_else(
            || match value.pointer("/errors/0/message").and_then(Value::as_str) {
                Some(m) => format!("the server refused introspection: {m}"),
                None => "the answer has no __schema (introspection may be disabled)".into(),
            },
        )?;
    let root = |key: &str| {
        schema
            .pointer(&format!("/{key}/name"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let types = schema
        .get("types")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_TYPES)
        .filter_map(|t| {
            let name = t.get("name").and_then(Value::as_str)?.to_string();
            if name.starts_with("__") {
                return None;
            }
            Some(GqlType {
                kind: t
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                fields: t
                    .get("fields")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .take(MAX_FIELDS)
                    .map(|f| GqlField {
                        name: f
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        description: f
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .chars()
                            .take(500)
                            .collect(),
                        args: args_of(f.get("args")),
                        type_ref: type_text(f.get("type").unwrap_or(&Value::Null), 0).0,
                        deprecated: f
                            .get("isDeprecated")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                    .collect(),
                input_fields: args_of(t.get("inputFields")),
                enum_values: t
                    .get("enumValues")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.get("name").and_then(Value::as_str).map(str::to_string))
                    .collect(),
                possible_types: t
                    .get("possibleTypes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.get("name").and_then(Value::as_str).map(str::to_string))
                    .collect(),
                name,
            })
        })
        .collect();
    Ok(GqlSchema {
        query_type: root("queryType"),
        mutation_type: root("mutationType"),
        subscription_type: root("subscriptionType"),
        types,
    })
}

/// The named type of a type text (`[User!]!` → `User`).
fn named(type_ref: &str) -> &str {
    type_ref.trim_matches(|c| c == '[' || c == ']' || c == '!')
}

/// An example JSON value for an input type text.
fn example_value(schema: &GqlSchema, type_ref: &str, depth: usize) -> Value {
    let inner = type_ref.trim_end_matches('!');
    if inner.starts_with('[') {
        let element = inner.trim_start_matches('[').trim_end_matches(']');
        return Value::Array(vec![example_value(schema, element, depth + 1)]);
    }
    match named(type_ref) {
        "Int" => Value::from(1),
        "Float" => Value::from(1.5),
        "Boolean" => Value::Bool(true),
        "ID" => Value::String("1".into()),
        "String" => Value::String("string".into()),
        other => match schema.get(other) {
            Some(t) if t.kind == "ENUM" => t
                .enum_values
                .first()
                .map_or(Value::Null, |v| Value::String(v.clone())),
            Some(t) if t.kind == "INPUT_OBJECT" && depth < MAX_SELECTION_DEPTH => {
                let mut map = Map::new();
                for f in t.input_fields.iter().take(MAX_SELECTED) {
                    map.insert(
                        f.name.clone(),
                        example_value(schema, &f.type_ref, depth + 1),
                    );
                }
                Value::Object(map)
            }
            _ => Value::String("string".into()),
        },
    }
}

/// The selection set for a type (scalars and enums; objects nested up to
/// a depth; unions and interfaces get `__typename`).
fn selection(schema: &GqlSchema, type_name: &str, depth: usize, indent: usize) -> String {
    let Some(t) = schema.get(type_name) else {
        return String::new();
    };
    if !matches!(t.kind.as_str(), "OBJECT" | "INTERFACE" | "UNION") {
        return String::new();
    }
    let pad = "  ".repeat(indent + 1);
    let mut lines = vec![format!("{pad}__typename")];
    for f in t
        .fields
        .iter()
        .filter(|f| f.args.iter().all(|a| !a.type_ref.ends_with('!')))
    {
        if lines.len() > MAX_SELECTED {
            break;
        }
        let inner = named(&f.type_ref);
        match schema.get(inner).map(|t| t.kind.as_str()) {
            Some("OBJECT" | "INTERFACE" | "UNION") => {
                if depth + 1 < MAX_SELECTION_DEPTH {
                    let nested = selection(schema, inner, depth + 1, indent + 1);
                    if !nested.is_empty() {
                        lines.push(format!("{pad}{} {nested}", f.name));
                    }
                }
            }
            _ => lines.push(format!("{pad}{}", f.name)),
        }
    }
    format!("{{\n{}\n{}}}", lines.join("\n"), "  ".repeat(indent))
}

/// Writes an operation that calls `field` of the query (`root` =
/// "query"), mutation or subscription type, with every argument as a
/// variable and example values for them.
pub fn build_operation(
    schema: &GqlSchema,
    root: &str,
    field: &str,
) -> Result<GqlOperation, String> {
    let root_name = match root {
        "query" => schema.query_type.as_deref(),
        "mutation" => schema.mutation_type.as_deref(),
        "subscription" => schema.subscription_type.as_deref(),
        _ => return Err(format!("unknown operation type {root:?}")),
    }
    .ok_or_else(|| format!("the schema has no {root} type"))?;
    let root_type = schema
        .get(root_name)
        .ok_or_else(|| format!("type {root_name} is missing"))?;
    let f = root_type
        .fields
        .iter()
        .find(|f| f.name == field)
        .ok_or_else(|| format!("{root_name} has no field {field}"))?;
    let mut variables = Map::new();
    let mut declared = Vec::new();
    let mut passed = Vec::new();
    for a in &f.args {
        declared.push(format!("${}: {}", a.name, a.type_ref));
        passed.push(format!("{}: ${}", a.name, a.name));
        variables.insert(a.name.clone(), example_value(schema, &a.type_ref, 0));
    }
    let op_name: String = {
        let mut chars = field.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().chain(chars).collect(),
            None => "Operation".into(),
        }
    };
    let header = if declared.is_empty() {
        format!("{root} {op_name}")
    } else {
        format!("{root} {op_name}({})", declared.join(", "))
    };
    let call = if passed.is_empty() {
        field.to_string()
    } else {
        format!("{field}({})", passed.join(", "))
    };
    let sel = selection(schema, named(&f.type_ref), 0, 1);
    let query = if sel.is_empty() {
        format!("{header} {{\n  {call}\n}}\n")
    } else {
        format!("{header} {{\n  {call} {sel}\n}}\n")
    };
    Ok(GqlOperation {
        query,
        variables: serde_json::to_string_pretty(&Value::Object(variables)).unwrap_or_default(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn tref(kind: &str, name: Option<&str>, of: Option<Value>) -> Value {
        json!({"kind": kind, "name": name, "ofType": of})
    }

    fn sample_answer() -> String {
        let non_null = |t: Value| tref("NON_NULL", None, Some(t));
        let named_t = |k: &str, n: &str| tref(k, Some(n), None);
        json!({"data": {"__schema": {
            "queryType": {"name": "Query"},
            "mutationType": {"name": "Mutation"},
            "subscriptionType": null,
            "types": [
                {"kind": "OBJECT", "name": "Query", "fields": [
                    {"name": "user", "description": "One user", "isDeprecated": false,
                     "args": [{"name": "id", "type": non_null(named_t("SCALAR", "ID")), "defaultValue": null}],
                     "type": named_t("OBJECT", "User")},
                    {"name": "users", "isDeprecated": false, "args": [],
                     "type": non_null(tref("LIST", None, Some(non_null(named_t("OBJECT", "User")))))}
                ]},
                {"kind": "OBJECT", "name": "Mutation", "fields": [
                    {"name": "addUser", "isDeprecated": false,
                     "args": [{"name": "input", "type": non_null(named_t("INPUT_OBJECT", "NewUser"))}],
                     "type": named_t("OBJECT", "User")}
                ]},
                {"kind": "OBJECT", "name": "User", "fields": [
                    {"name": "id", "args": [], "type": named_t("SCALAR", "ID")},
                    {"name": "role", "args": [], "type": named_t("ENUM", "Role")},
                    {"name": "friends", "args": [], "type": tref("LIST", None, Some(named_t("OBJECT", "User")))},
                    {"name": "posts", "args": [{"name": "first", "type": non_null(named_t("SCALAR", "Int"))}],
                     "type": named_t("OBJECT", "Post")}
                ]},
                {"kind": "INPUT_OBJECT", "name": "NewUser", "inputFields": [
                    {"name": "name", "type": non_null(named_t("SCALAR", "String"))},
                    {"name": "role", "type": named_t("ENUM", "Role"), "defaultValue": "USER"}
                ]},
                {"kind": "ENUM", "name": "Role", "enumValues": [{"name": "ADMIN"}, {"name": "USER"}]},
                {"kind": "OBJECT", "name": "__Type", "fields": []}
            ]
        }}})
        .to_string()
    }

    #[test]
    fn reads_introspection_answers() {
        let schema = parse_introspection(&sample_answer()).unwrap();
        assert_eq!(schema.query_type.as_deref(), Some("Query"));
        assert!(
            schema.get("__Type").is_none(),
            "introspection types are left out"
        );
        let users = &schema.get("Query").unwrap().fields[1];
        assert_eq!(users.type_ref, "[User!]!");
        assert_eq!(
            schema.get("NewUser").unwrap().input_fields[1]
                .default_value
                .as_deref(),
            Some("USER")
        );
        let refused = r#"{"errors":[{"message":"introspection is disabled"}]}"#;
        assert!(
            parse_introspection(refused)
                .unwrap_err()
                .contains("disabled")
        );
        assert!(parse_introspection("<html>").is_err());
    }

    #[test]
    fn builds_queries_and_mutations_with_variables() {
        let schema = parse_introspection(&sample_answer()).unwrap();
        let op = build_operation(&schema, "query", "user").unwrap();
        assert!(
            op.query
                .starts_with("query User($id: ID!) {\n  user(id: $id) {"),
            "{}",
            op.query
        );
        assert!(op.query.contains("      role\n"), "{}", op.query);
        assert!(
            op.query.contains("friends {"),
            "nested objects: {}",
            op.query
        );
        assert!(
            !op.query.contains("posts"),
            "fields with required arguments are left out"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&op.variables).unwrap(),
            json!({"id": "1"})
        );
        let m = build_operation(&schema, "mutation", "addUser").unwrap();
        assert!(m.query.starts_with("mutation AddUser($input: NewUser!) {"));
        assert_eq!(
            serde_json::from_str::<Value>(&m.variables).unwrap(),
            json!({"input": {"name": "string", "role": "ADMIN"}})
        );
        assert!(build_operation(&schema, "subscription", "x").is_err());
        assert!(build_operation(&schema, "query", "nope").is_err());
    }

    #[test]
    fn recognizes_graphql_requests_and_writes_bodies() {
        assert!(is_graphql_request(
            "POST /api HTTP/1.1\nContent-Type: application/json\n\n{\"query\":\"{ me { id } }\"}"
        ));
        assert!(is_graphql_request(
            "GET /graphql?query=%7Bme%7D HTTP/1.1\n\n"
        ));
        assert!(is_graphql_request(
            "POST /x HTTP/1.1\n\n[{\"query\":\"query A { a }\"}]"
        ));
        assert!(!is_graphql_request(
            "POST /x HTTP/1.1\n\n{\"query\":\"select * from t\"}"
        ));
        assert!(!is_graphql_request("GET / HTTP/1.1\n\n"));
        let body = request_body("{ a }", "{\"x\": 1}", Some("A"));
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({"query": "{ a }", "variables": {"x": 1}, "operationName": "A"})
        );
        assert_eq!(
            serde_json::from_str::<Value>(&request_body("{ a }", "not json", None)).unwrap(),
            json!({"query": "{ a }", "variables": {}})
        );
    }
}
