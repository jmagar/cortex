use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use super::{ParsedTranscriptRecord, hash_text, record_key_from_line};

pub fn is_chat_file(path: &Path) -> bool {
    let file_name = path.file_name().and_then(|name| name.to_str());
    let parent_name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    matches!(parent_name, Some("chats"))
        && file_name.is_some_and(|name| name.starts_with("session-") && name.ends_with(".json"))
}

pub fn parse_file(raw: &str, path: &Path) -> Result<Vec<ParsedTranscriptRecord>> {
    let value: Value = serde_json::from_str(raw)?;
    let session_id = value
        .get("sessionId")
        .or_else(|| value.get("session_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(ToString::to_string)
        });
    let ai_project = value
        .get("cwd")
        .or_else(|| value.get("projectPath"))
        .or_else(|| value.get("project_path"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            value
                .get("projectHash")
                .or_else(|| value.get("project_hash"))
                .and_then(Value::as_str)
                .map(|hash| format!("gemini://project/{hash}"))
        });
    let default_timestamp = value
        .get("startTime")
        .or_else(|| value.get("started_at"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut records = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let content = extract_message(message);
        if content.is_empty() {
            continue;
        }
        let serialized = serde_json::to_string(message)?;
        let record_key = message
            .get("id")
            .or_else(|| message.get("uuid"))
            .and_then(Value::as_str)
            .map(|id| format!("id:{id}"))
            .unwrap_or_else(|| format!("message:{index}:hash:{}", hash_text(&serialized)));
        let timestamp = message
            .get("timestamp")
            .or_else(|| message.get("created_at"))
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| default_timestamp.clone());
        records.push(ParsedTranscriptRecord {
            record_key,
            timestamp,
            message: content,
            session_id: session_id.clone(),
            ai_project: ai_project.clone(),
        });
    }
    Ok(records)
}

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
    Ok(Some(ParsedTranscriptRecord {
        record_key: record_key_from_line(&value, line, line_no),
        timestamp: value
            .get("timestamp")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        message,
        session_id: value
            .get("sessionId")
            .or_else(|| value.get("session_id"))
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| Some(path.to_string_lossy().to_string())),
        ai_project: value
            .get("cwd")
            .or_else(|| value.get("projectPath"))
            .or_else(|| value.get("project_path"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
    }))
}

fn extract_message(value: &Value) -> String {
    if let Some(content) = value.get("content").and_then(Value::as_str) {
        return content.to_string();
    }
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return message.to_string();
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
        return parts.join(" ");
    }
    String::new()
}

#[cfg(test)]
#[path = "gemini_tests.rs"]
mod tests;
