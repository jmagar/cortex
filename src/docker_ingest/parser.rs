use anyhow::Result;
use bollard::container::LogOutput;

use crate::db;

use super::models::ContainerMeta;

pub(super) fn log_output_to_entry(
    host_name: &str,
    container: &ContainerMeta,
    output: LogOutput,
) -> Result<Option<db::LogBatchEntry>> {
    let (stream, fallback_severity, bytes) = match output {
        LogOutput::StdOut { message } => ("stdout", "info", message),
        LogOutput::StdErr { message } => ("stderr", "warning", message),
        _ => return Ok(None),
    };

    let raw_line = String::from_utf8_lossy(&bytes)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if raw_line.is_empty() {
        return Ok(None);
    }

    let (timestamp, message) = split_docker_timestamp(&raw_line);
    let severity = infer_docker_severity(&message).unwrap_or(fallback_severity);
    let checkpoint_timestamp = timestamp.clone();
    Ok(Some(db::LogBatchEntry {
        timestamp,
        hostname: host_name.to_string(),
        facility: Some("local0".to_string()),
        severity: severity.to_string(),
        app_name: Some(container.app_name()),
        process_id: Some(container.short_id()),
        message,
        raw: raw_line,
        source_ip: format!("docker://{}/{}/{}", host_name, container.name, stream),
        docker_checkpoint: Some(db::DockerCheckpoint {
            host_name: host_name.to_string(),
            container_id: container.id.clone(),
            timestamp: checkpoint_timestamp,
        }),
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        ai_transcript_path: None,
    }))
}

fn split_docker_timestamp(raw: &str) -> (String, String) {
    match raw.split_once(' ') {
        Some((ts, rest)) if chrono::DateTime::parse_from_rfc3339(ts).is_ok() => {
            (ts.to_string(), rest.to_string())
        }
        _ => (
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            raw.to_string(),
        ),
    }
}

fn infer_docker_severity(message: &str) -> Option<&'static str> {
    infer_json_severity(message).or_else(|| infer_text_severity(&strip_ansi(message)))
}

fn infer_json_severity(message: &str) -> Option<&'static str> {
    let trimmed = message.trim_start();
    if !trimmed.starts_with('{') {
        return None;
    }

    let parsed: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    let candidates = [
        parsed.get("level"),
        parsed.get("severity"),
        parsed.get("severity_text"),
        parsed.get("log").and_then(|log| log.get("level")),
    ];
    let severity = candidates
        .into_iter()
        .flatten()
        .find_map(|value| value.as_str().and_then(normalize_level));
    severity
}

fn infer_text_severity(message: &str) -> Option<&'static str> {
    let normalized = message
        .split_whitespace()
        .take(24)
        .collect::<Vec<_>>()
        .join(" ");

    for marker in ["level", "lvl", "severity"] {
        if let Some(level) = extract_keyed_level(&normalized, marker) {
            return Some(level);
        }
    }

    for token in normalized.split(|c: char| !c.is_ascii_alphanumeric()) {
        if let Some(level) = normalize_level(token) {
            return Some(level);
        }
    }

    None
}

fn extract_keyed_level(message: &str, key: &str) -> Option<&'static str> {
    for separator in ['=', ':'] {
        let needle = format!("{key}{separator}");
        let Some(start) = message.to_ascii_lowercase().find(&needle) else {
            continue;
        };
        let value_start = start + needle.len();
        let value = message[value_start..]
            .trim_start_matches([' ', '"', '\''])
            .split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap_or_default();
        if let Some(level) = normalize_level(value) {
            return Some(level);
        }
    }
    None
}

fn normalize_level(level: &str) -> Option<&'static str> {
    match level.to_ascii_lowercase().as_str() {
        "trace" | "debug" => Some("debug"),
        "info" | "information" => Some("info"),
        "notice" => Some("notice"),
        "warn" | "warning" => Some("warning"),
        "error" | "err" => Some("err"),
        "fatal" | "panic" | "critical" | "crit" => Some("crit"),
        "alert" => Some("alert"),
        "emerg" | "emergency" => Some("emerg"),
        _ => None,
    }
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code_ch in chars.by_ref() {
                if code_ch.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
