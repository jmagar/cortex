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
    let payload = value.get("payload").unwrap_or(&value);
    let session_id = payload
        .get("id")
        .or_else(|| value.get("sessionId"))
        .or_else(|| value.get("session_id"))
        .or_else(|| value.pointer("/session/id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(ToString::to_string)
        });
    let ai_project = extract_project(&value);
    Ok(Some(ParsedTranscriptRecord {
        record_key: record_key_from_line(&value, line),
        timestamp: value
            .get("timestamp")
            .or_else(|| payload.get("timestamp"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        message,
        session_id,
        ai_project,
    }))
}

fn extract_project(value: &Value) -> Option<String> {
    let payload = value.get("payload").unwrap_or(value);
    payload
        .get("cwd")
        .or_else(|| value.get("cwd"))
        .or_else(|| value.pointer("/turn_context/cwd"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            payload
                .get("arguments")
                .and_then(Value::as_str)
                .and_then(|args| serde_json::from_str::<Value>(args).ok())
                .and_then(|args| {
                    args.get("workdir")
                        .or_else(|| args.get("cwd"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
        })
}

pub fn project_from_line(line: &str) -> Option<String> {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|value| extract_project(&value))
}

fn extract_message(value: &Value) -> String {
    if let Some(text) = value.get("content").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(text) = value.get("message").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(text) = value.pointer("/payload/text").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(text) = value.pointer("/payload/content").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(text) = value
        .pointer("/payload/message/content")
        .and_then(Value::as_str)
    {
        return text.to_string();
    }
    if let Some(items) = value.pointer("/payload/content").and_then(Value::as_array) {
        return join_content_items(items);
    }
    if let Some(items) = value.pointer("/message/content").and_then(Value::as_array) {
        return join_content_items(items);
    }
    String::new()
}

fn join_content_items(items: &[Value]) -> String {
    let parts: Vec<&str> = items
        .iter()
        .filter_map(|item| {
            item.as_str()
                .or_else(|| item.get("text").and_then(Value::as_str))
                .or_else(|| item.get("content").and_then(Value::as_str))
        })
        .collect();
    parts.join(" ")
}
