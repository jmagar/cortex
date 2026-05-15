use serde_json::{Map, Value};

pub(crate) const MAX_METADATA_JSON_BYTES: usize = 64 * 1024;
pub(crate) const MAX_METADATA_STRING_CHARS: usize = 2048;
pub(crate) const MAX_METADATA_KEY_CHARS: usize = 128;
pub(crate) const MAX_METADATA_OBJECT_FIELDS: usize = 128;

const REDACTED: &str = "[REDACTED]";

pub(crate) fn bounded_metadata_json(value: Value) -> String {
    let value = sanitize_value(value, None);
    let encoded = value.to_string();
    if encoded.len() <= MAX_METADATA_JSON_BYTES {
        return encoded;
    }

    let source_type = value
        .get("source_type")
        .and_then(Value::as_str)
        .map(str::to_string);
    serde_json::json!({
        "source_type": source_type,
        "metadata_truncated": true,
        "metadata_original_bytes": encoded.len(),
    })
    .to_string()
}

pub(crate) fn attrs_to_metadata_object<'a, I>(attrs: I) -> Value
where
    I: IntoIterator<Item = (&'a str, Value)>,
{
    let mut object = Map::new();
    let mut omitted = 0usize;
    for (key, value) in attrs {
        if object.len() >= MAX_METADATA_OBJECT_FIELDS {
            omitted += 1;
            continue;
        }
        let stored_key = truncate_chars(key, MAX_METADATA_KEY_CHARS);
        object.insert(stored_key, sanitize_value(value, Some(key)));
    }
    if omitted > 0 {
        object.insert("_omitted_fields".to_string(), Value::Number(omitted.into()));
    }
    Value::Object(object)
}

fn sanitize_value(value: Value, key: Option<&str>) -> Value {
    if key.is_some_and(is_sensitive_key) {
        return Value::String(REDACTED.to_string());
    }

    match value {
        Value::String(value) => Value::String(truncate_chars(&value, MAX_METADATA_STRING_CHARS)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .take(MAX_METADATA_OBJECT_FIELDS)
                .map(|value| sanitize_value(value, key))
                .collect(),
        ),
        Value::Object(values) => sanitize_object(values),
        value => value,
    }
}

fn sanitize_object(values: Map<String, Value>) -> Value {
    let mut object = Map::new();
    let mut omitted = 0usize;
    for (key, value) in values {
        if object.len() >= MAX_METADATA_OBJECT_FIELDS {
            omitted += 1;
            continue;
        }
        let stored_key = truncate_chars(&key, MAX_METADATA_KEY_CHARS);
        object.insert(stored_key, sanitize_value(value, Some(&key)));
    }
    if omitted > 0 {
        object.insert("_omitted_fields".to_string(), Value::Number(omitted.into()));
    }
    Value::Object(object)
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    [
        "authorization",
        "bearer",
        "cookie",
        "credential",
        "password",
        "private_key",
        "secret",
        "set-cookie",
        "token",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
        || normalized.contains("api_key")
        || normalized.contains("apikey")
        || normalized.contains("access_key")
}

fn truncate_chars(value: &str, max: usize) -> String {
    let mut out = String::with_capacity(value.len().min(max));
    for (idx, ch) in value.chars().enumerate() {
        if idx >= max {
            out.push_str("...[truncated]");
            return out;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sensitive_keys_and_truncates_long_strings() {
        let value = attrs_to_metadata_object([
            ("Authorization", Value::String("Bearer secret".into())),
            (
                "normal",
                Value::String("x".repeat(MAX_METADATA_STRING_CHARS + 5)),
            ),
        ]);

        assert_eq!(value["Authorization"], REDACTED);
        assert!(value["normal"]
            .as_str()
            .unwrap()
            .ends_with("...[truncated]"));
    }

    #[test]
    fn redacts_sensitive_keys_before_key_truncation() {
        let long_sensitive_key = format!("{}token", "x".repeat(MAX_METADATA_KEY_CHARS + 20));
        let stored_key = truncate_chars(&long_sensitive_key, MAX_METADATA_KEY_CHARS);
        let value = attrs_to_metadata_object([(
            long_sensitive_key.as_str(),
            Value::String("secret".into()),
        )]);

        assert_eq!(value[stored_key], REDACTED);
    }

    #[test]
    fn caps_object_field_count() {
        let attrs = (0..(MAX_METADATA_OBJECT_FIELDS + 2))
            .map(|idx| (format!("key-{idx}"), Value::String("value".into())))
            .collect::<Vec<_>>();
        let value = attrs_to_metadata_object(
            attrs
                .iter()
                .map(|(key, value)| (key.as_str(), value.clone())),
        );

        assert_eq!(value["_omitted_fields"], 2);
    }

    #[test]
    fn bounded_metadata_remains_valid_json_when_payload_is_too_large() {
        let value = serde_json::json!({
            "source_type": "otlp",
            "huge": vec!["x".repeat(MAX_METADATA_STRING_CHARS + 20); MAX_METADATA_OBJECT_FIELDS],
        });
        let encoded = bounded_metadata_json(value);
        let parsed: Value = serde_json::from_str(&encoded).unwrap();

        assert_eq!(parsed["source_type"], "otlp");
        assert!(parsed["metadata_truncated"].as_bool().unwrap());
    }
}
