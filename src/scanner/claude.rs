use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use super::{record_key_from_line, ParsedTranscriptRecord};

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
        record_key: record_key_from_line(&value, line, line_no),
        timestamp: value
            .get("timestamp")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        message,
        session_id,
        ai_project: extract_project(&value),
    }))
}

fn extract_project(value: &Value) -> Option<String> {
    value
        .get("cwd")
        .or_else(|| value.get("projectPath"))
        .or_else(|| value.get("project_path"))
        .or_else(|| value.pointer("/message/cwd"))
        .or_else(|| value.pointer("/message/projectPath"))
        .or_else(|| value.pointer("/message/project_path"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
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
        if let Some(content) = join_content_items(items) {
            return content;
        }
    }
    if let Some(items) = value.pointer("/message/content").and_then(Value::as_array) {
        if let Some(content) = join_content_items(items) {
            return content;
        }
    }
    String::new()
}

fn join_content_items(items: &[Value]) -> Option<String> {
    let parts: Vec<&str> = items
        .iter()
        .filter_map(|item| {
            item.as_str()
                .or_else(|| item.get("text").and_then(Value::as_str))
                .or_else(|| item.get("content").and_then(Value::as_str))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;
