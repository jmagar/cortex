use std::path::Path;

use anyhow::Result;
use serde_json::Value;

pub fn parse_line(
    line: &str,
    path: &Path,
    line_no: usize,
) -> Result<Option<ParsedTranscriptRecord>> {
    let value: Value = serde_json::from_str(line)?;
    let message = extract_message(&value);
    if message.is_empty() {
        return Ok(None);
    }
    let session_id = value
        .get("sessionId")
        .or_else(|| value.get("session_id"))
        .or_else(|| value.pointer("/session/id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| Some(path.to_string_lossy().to_string()));
    Ok(Some(ParsedTranscriptRecord {
        record_key: format!("{line_no}"),
        timestamp: value
            .get("timestamp")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        message,
        session_id,
    }))
}

pub struct ParsedTranscriptRecord {
    pub record_key: String,
    pub timestamp: Option<String>,
    pub message: String,
    pub session_id: Option<String>,
}

fn extract_message(value: &Value) -> String {
    if let Some(content) = value.get("content").and_then(Value::as_str) {
        return content.to_string();
    }
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return message.to_string();
    }
    if let Some(text) = value.pointer("/message/content").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(items) = value.get("content").and_then(Value::as_array) {
        let parts: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
        if !parts.is_empty() {
            return parts.join(" ");
        }
    }
    String::new()
}
