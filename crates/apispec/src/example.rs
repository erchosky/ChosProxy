//! Example values from JSON Schemas (as used by OpenAPI and Swagger), with
//! local `$ref` resolution. Explicit examples win; otherwise a value of
//! the right type is made up. Bounded in depth and size; cycles stop.

use crate::MAX_DEPTH;
use serde_json::{Map, Value};

/// Deepest object nesting in a generated example.
const MAX_EXAMPLE_DEPTH: usize = 6;
/// Most properties written per generated object.
const MAX_PROPERTIES: usize = 50;

/// Resolves `#/…` references inside one document.
#[derive(Debug)]
pub(crate) struct Resolver<'a> {
    pub(crate) root: &'a Value,
}

impl<'a> Resolver<'a> {
    /// Follows `$ref`s (local only) until a value without one; `None` for a
    /// remote, missing or cyclic reference.
    pub(crate) fn resolve(&self, value: &'a Value) -> Option<&'a Value> {
        let mut current = value;
        for _ in 0..MAX_DEPTH {
            let Some(reference) = current.get("$ref").and_then(Value::as_str) else {
                return Some(current);
            };
            current = self.pointer(reference)?;
        }
        None
    }

    /// The value at a local reference (`#/components/schemas/User`).
    pub(crate) fn pointer(&self, reference: &str) -> Option<&'a Value> {
        // The fragment is a JSON pointer (`~0`/`~1` escapes are decoded by
        // `pointer`); `%xx` escapes are decoded first.
        let path = reference.strip_prefix('#')?;
        self.root.pointer(&percent_decode(path))
    }

    /// An example value for a schema.
    pub(crate) fn example(&self, schema: &'a Value) -> Value {
        self.example_at(schema, 0, &mut Vec::new())
    }

    fn example_at(&self, schema: &'a Value, depth: usize, seen: &mut Vec<String>) -> Value {
        let reference = schema
            .get("$ref")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(r) = &reference {
            if seen.contains(r) || seen.len() > MAX_DEPTH {
                return Value::Null;
            }
            seen.push(r.clone());
        }
        let value = match self.resolve(schema) {
            Some(s) => self.example_of(s, depth, seen),
            None => Value::Null,
        };
        if reference.is_some() {
            seen.pop();
        }
        value
    }

    fn example_of(&self, schema: &'a Value, depth: usize, seen: &mut Vec<String>) -> Value {
        for key in ["example", "default", "const"] {
            if let Some(v) = schema.get(key) {
                return v.clone();
            }
        }
        if let Some(v) = schema
            .get("examples")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
        {
            return v.clone();
        }
        if let Some(v) = schema
            .get("enum")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
        {
            return v.clone();
        }
        for combo in ["allOf", "oneOf", "anyOf"] {
            if let Some(list) = schema.get(combo).and_then(Value::as_array) {
                if combo == "allOf" {
                    let mut merged = Map::new();
                    for part in list {
                        if let Value::Object(o) = self.example_at(part, depth, seen) {
                            merged.extend(o);
                        }
                    }
                    return Value::Object(merged);
                }
                if let Some(first) = list.first() {
                    return self.example_at(first, depth, seen);
                }
            }
        }
        let kind = match schema.get("type") {
            Some(Value::String(t)) => t.as_str(),
            // OpenAPI 3.1: a list of types; the first that is not null.
            Some(Value::Array(types)) => types
                .iter()
                .filter_map(Value::as_str)
                .find(|t| *t != "null")
                .unwrap_or("null"),
            _ if schema.get("properties").is_some() => "object",
            _ if schema.get("items").is_some() => "array",
            _ => "string",
        };
        match kind {
            "object" => {
                if depth >= MAX_EXAMPLE_DEPTH {
                    return Value::Object(Map::new());
                }
                let mut map = Map::new();
                if let Some(props) = schema.get("properties").and_then(Value::as_object) {
                    for (name, prop) in props.iter().take(MAX_PROPERTIES) {
                        map.insert(name.clone(), self.example_at(prop, depth + 1, seen));
                    }
                }
                Value::Object(map)
            }
            "array" => {
                if depth >= MAX_EXAMPLE_DEPTH {
                    return Value::Array(Vec::new());
                }
                match schema.get("items") {
                    Some(items) => Value::Array(vec![self.example_at(items, depth + 1, seen)]),
                    None => Value::Array(Vec::new()),
                }
            }
            "integer" => Value::from(schema.get("minimum").and_then(Value::as_i64).unwrap_or(1)),
            "number" => schema
                .get("minimum")
                .cloned()
                .filter(Value::is_number)
                .unwrap_or_else(|| Value::from(1.5)),
            "boolean" => Value::Bool(true),
            "null" => Value::Null,
            _ => Value::String(string_example(schema)),
        }
    }
}

/// `%xx` escapes decoded (invalid ones kept as they are).
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A plausible string for a schema's `format`.
fn string_example(schema: &Value) -> String {
    match schema.get("format").and_then(Value::as_str).unwrap_or("") {
        "date" => "2024-01-31".into(),
        "date-time" => "2024-01-31T12:00:00Z".into(),
        "time" => "12:00:00".into(),
        "email" => "user@example.com".into(),
        "uuid" => "123e4567-e89b-12d3-a456-426614174000".into(),
        "uri" | "url" => "https://example.com/".into(),
        "hostname" => "example.com".into(),
        "ipv4" => "192.0.2.1".into(),
        "ipv6" => "2001:db8::1".into(),
        "byte" => "aGVsbG8=".into(),
        "binary" => "file contents".into(),
        "password" => "password".into(),
        _ => "string".into(),
    }
}

/// A value as text for a URL or header (strings unquoted).
pub(crate) fn as_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Array(a) => a.iter().map(as_text).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn examples_follow_schemas_refs_and_stop_on_cycles() {
        let doc = json!({
            "components": {"schemas": {
                "User": {"type": "object", "properties": {
                    "id": {"type": "integer", "minimum": 7},
                    "email": {"type": "string", "format": "email"},
                    "role": {"enum": ["admin", "user"]},
                    "tags": {"type": "array", "items": {"type": "string"}},
                    "friend": {"$ref": "#/components/schemas/User"},
                    "note": {"type": ["null", "string"], "example": "hi"}
                }},
                "Loop": {"$ref": "#/components/schemas/Loop"}
            }}
        });
        let r = Resolver { root: &doc };
        let user = r.example(&json!({"$ref": "#/components/schemas/User"}));
        assert_eq!(user["id"], 7);
        assert_eq!(user["email"], "user@example.com");
        assert_eq!(user["role"], "admin");
        assert_eq!(user["tags"], json!(["string"]));
        assert_eq!(user["note"], "hi");
        assert_eq!(user["friend"], Value::Null, "a cycle stops");
        assert_eq!(
            r.example(&json!({"$ref": "#/components/schemas/Loop"})),
            Value::Null
        );
        assert_eq!(
            r.example(&json!({"$ref": "http://remote/x.json"})),
            Value::Null
        );
        let merged = r.example(&json!({"allOf": [
            {"properties": {"a": {"type": "boolean"}}},
            {"properties": {"b": {"type": "number"}}}
        ]}));
        assert_eq!(merged, json!({"a": true, "b": 1.5}));
        assert_eq!(as_text(&json!(["a", 1])), "a,1");
    }
}
