use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use super::{record_key_from_line, ParsedTranscriptRecord};

pub fn parse_line(
    line: &str,
    path: &Path,
    _line_no: usize,
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
        record_key: record_key_from_line(&value, line),
        timestamp: value
            .get("timestamp")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        message,
        session_id,
        ai_project: None,
    }))
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
        let parts: Vec<&str> = items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .or_else(|| item.get("text").and_then(Value::as_str))
                    .or_else(|| item.get("content").and_then(Value::as_str))
            })
            .collect();
        if !parts.is_empty() {
            return parts.join(" ");
        }
    }
    if let Some(items) = value.pointer("/message/content").and_then(Value::as_array) {
        let parts: Vec<&str> = items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .or_else(|| item.get("text").and_then(Value::as_str))
                    .or_else(|| item.get("content").and_then(Value::as_str))
            })
            .collect();
        if !parts.is_empty() {
            return parts.join(" ");
        }
    }
    String::new()
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;
