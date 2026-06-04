use regex::Regex;
use serde_json::{Map, Value};
use std::sync::OnceLock;

use crate::inventory::limits::{truncate_text, MAX_ARRAY_ENTRIES, MAX_JSON_DEPTH};
use crate::inventory::schema::RedactionStatus;

const REDACTED: &str = "[REDACTED]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedArtifact {
    body: String,
    status: RedactionStatus,
    truncated: bool,
}

impl RedactedArtifact {
    pub fn from_text(input: &str, max_bytes: usize) -> Self {
        let redacted = redact_text(input);
        let (body, truncated) = truncate_text(&redacted, max_bytes);
        let status = if redacted == input {
            RedactionStatus::NoSecretsDetected
        } else {
            RedactionStatus::Redacted
        };
        Self {
            body,
            status,
            truncated,
        }
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn status(&self) -> RedactionStatus {
        self.status.clone()
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }
}

pub fn redact_text(input: &str) -> String {
    let mut out = input.to_string();
    for regex in secret_regexes() {
        out = regex
            .replace_all(&out, |caps: &regex::Captures<'_>| {
                if caps.len() > 1 {
                    format!("{}{}", &caps[1], REDACTED)
                } else {
                    REDACTED.to_string()
                }
            })
            .to_string();
    }
    out
}

pub fn redact_error(input: impl AsRef<str>) -> (String, bool) {
    let redacted = redact_text(input.as_ref());
    let (truncated, did_truncate) = truncate_text(&redacted, 1024);
    (truncated, did_truncate)
}

pub fn redact_json(value: &Value) -> Value {
    redact_json_inner(value, 0)
}

fn redact_json_inner(value: &Value, depth: usize) -> Value {
    if depth >= MAX_JSON_DEPTH {
        return Value::String("[TRUNCATED_DEPTH]".to_string());
    }
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, value) in map {
                if sensitive_key(key)
                    || (key.eq_ignore_ascii_case("value") && sibling_name_is_sensitive(map))
                {
                    out.insert(key.clone(), Value::String(REDACTED.to_string()));
                } else {
                    out.insert(key.clone(), redact_json_inner(value, depth + 1));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            let truncated = items.len() > MAX_ARRAY_ENTRIES;
            let mut out = items
                .iter()
                .take(MAX_ARRAY_ENTRIES)
                .map(|item| redact_json_inner(item, depth + 1))
                .collect::<Vec<_>>();
            if truncated {
                out.push(Value::String("[TRUNCATED_ARRAY]".to_string()));
            }
            Value::Array(out)
        }
        Value::String(text) => Value::String(redact_text(text)),
        _ => value.clone(),
    }
}

pub fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "api_key",
        "apikey",
        "authorization",
        "bearer",
        "cookie",
        "client_secret",
        "key",
        "pass",
        "password",
        "passphrase",
        "private_key",
        "psk",
        "refresh_token",
        "secret",
        "session",
        "token",
        "x-api-key",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn sibling_name_is_sensitive(map: &Map<String, Value>) -> bool {
    map.iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("name") || key.eq_ignore_ascii_case("key"))
        .map(|(_, value)| value)
        .and_then(Value::as_str)
        .is_some_and(sensitive_key)
}

fn secret_regexes() -> &'static [Regex] {
    static REGEXES: OnceLock<Vec<Regex>> = OnceLock::new();
    REGEXES.get_or_init(|| {
        let private_key_label = ["PRI", "VATE ", "KEY"].concat();
        let pem_fence = "-".repeat(5);
        vec![
            Regex::new(&format!(
                r#"(?is){pem_fence}BEGIN [A-Z ]*{private_key_label}{pem_fence}.*?{pem_fence}END [A-Z ]*{private_key_label}{pem_fence}"#
            ))
            .unwrap(),
            Regex::new(r#"(?i)(\bcurl\b[^\n]*\s-u\s+)["']?[^"'\s]+["']?"#).unwrap(),
            Regex::new(r#"(?i)([a-z][a-z0-9+.-]*://)[^/@\s:]+(?::[^/@\s]*)?@"#).unwrap(),
            Regex::new(r#"(?i)(authorization\s*[:=]\s*(?:bearer\s+)?)[A-Za-z0-9._~+/=-]{12,}"#).unwrap(),
            Regex::new(r#"(?i)(cookie\s*[:=]\s*)[^\s;]{8,}"#).unwrap(),
            Regex::new(r#"(?i)((?:api[_-]?key|x[_-]?api[_-]?key|access[_-]?token|refresh[_-]?token|token|secret|password|passphrase|private[_-]?key|psk|client[_-]?secret)\s*[:=]\s*)(?:"[^"\n]{3,}"|'[^'\n]{3,}'|[^\s&,"']{3,})"#).unwrap(),
            Regex::new(r#"(?i)((?:api[_-]?key|x[_-]?api[_-]?key|access[_-]?token|refresh[_-]?token|token|secret|password|passphrase|private[_-]?key|psk|client[_-]?secret)=)(?:"[^"\n]{3,}"|'[^'\n]{3,}'|[^&\s"']+)"#).unwrap(),
            Regex::new(r#"[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{16,}"#).unwrap(),
            Regex::new(r#"\b[A-Za-z0-9+/]{32,}={0,2}\b"#).unwrap(),
        ]
    })
}

#[cfg(test)]
#[path = "redaction_tests.rs"]
mod tests;
