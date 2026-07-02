//! MCP/tool-call event extraction from AI transcript records.
//!
//! Two independent extractors feed the same [`ExtractedMcpEvent`] shape:
//! - Claude: `message.content[]` items with `type = "tool_use"` (call) or
//!   `type = "tool_result"` (result, linked back to the call via
//!   `tool_use_id`).
//! - Codex: `response_item` rows with `payload.type = "function_call"`
//!   (call) or `payload.type = "function_call_output"` (result, linked via
//!   `payload.call_id`).
//!
//! Both extractors operate on the already-parsed raw JSON `Value` for a
//! single transcript line (never re-parse `line_text`), and return zero or
//! more events per line — unlike skill-event extraction, a single Claude
//! assistant turn can contain multiple `tool_use` blocks, so this is a
//! `Vec`, not an `Option`.
//!
//! `tool_name` classification: a name is classified as MCP
//! (`mcp_server`/`mcp_tool` populated) only when it matches the
//! `mcp__<server>__<tool>` naming convention emitted by both Claude and
//! Codex's MCP tool-call surface (GH #94's Lavra research: "Codex MCP/tool
//! names may be exposed as plain function names ... or namespaced tool
//! names ... Implement parser normalization by shape, not by one exact
//! prefix" — the `mcp__` double-underscore shape is the one safe prefix
//! both platforms actually emit). Builtin tool names (`Bash`, `Read`,
//! `shell`, `exec_command`, ...) are still recorded as general tool-call
//! rows with `mcp_server = NULL` per the schema note in GH #94's "MCP
//! assessment design" section — `cortex assess mcp` filters to
//! MCP-classified rows at query time.

use serde_json::Value;

use crate::assessment::redact_secrets;

/// Bound on stored preview/argument fields — mirrors
/// `ingest_metadata::MAX_METADATA_STRING_CHARS` so a single oversized tool
/// payload cannot bloat `ai_mcp_events` rows or leak a full secret/log dump.
const MAX_PREVIEW_CHARS: usize = 2048;
const MAX_NAME_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpEventKind {
    Call,
    Result,
}

impl McpEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Result => "result",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedMcpEvent {
    pub call_id: String,
    pub tool_name: String,
    pub mcp_server: Option<String>,
    pub mcp_tool: Option<String>,
    pub event_kind: McpEventKind,
    pub turn_id: Option<String>,
    pub status: Option<String>,
    pub is_error: Option<bool>,
    pub arguments_json: Option<String>,
    pub output_preview: Option<String>,
    pub error_text: Option<String>,
}

fn clamp_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        value.chars().take(max_chars).collect()
    }
}

/// Classify `name` as MCP-style (`mcp__<server>__<tool>`) or leave it as a
/// general/builtin tool call. Only the double-underscore `mcp__` prefix is
/// treated as authoritative — anything else (bare builtins, single-underscore
/// wrapper-local names like `exec_command`) is left unclassified rather than
/// guessed at, per GH #94's "normalize by shape, not by one exact prefix"
/// guidance (shape here means: exactly the `mcp__` delimiter convention both
/// platforms are observed to emit for MCP-routed tools).
fn classify_tool_name(name: &str) -> (Option<String>, Option<String>) {
    let Some(rest) = name.strip_prefix("mcp__") else {
        return (None, None);
    };
    match rest.split_once("__") {
        Some((server, tool)) if !server.is_empty() && !tool.is_empty() => {
            (Some(server.to_string()), Some(tool.to_string()))
        }
        _ => (None, None),
    }
}

fn bounded_preview(value: &str) -> String {
    clamp_chars(&redact_secrets(value), MAX_PREVIEW_CHARS)
}

/// Extract Claude `tool_use`/`tool_result` events from one transcript line's
/// raw JSON value. Scans `message.content[]` (assistant `tool_use` rows) or
/// `message.content[]` (user `tool_result` rows) — both shapes live under
/// the same pointer, so a single scan covers both.
pub fn extract_claude_mcp_events(value: &Value) -> Vec<ExtractedMcpEvent> {
    let Some(items) = value.pointer("/message/content").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut events = Vec::new();
    for item in items {
        let Some(item_type) = item.get("type").and_then(Value::as_str) else {
            continue;
        };
        match item_type {
            "tool_use" => {
                let Some(call_id) = item.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(name) = item.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let (mcp_server, mcp_tool) = classify_tool_name(name);
                let arguments_json = item.get("input").map(|input| {
                    let encoded = input.to_string();
                    bounded_preview(&encoded)
                });
                events.push(ExtractedMcpEvent {
                    call_id: clamp_chars(call_id, MAX_NAME_CHARS),
                    tool_name: clamp_chars(name, MAX_NAME_CHARS),
                    mcp_server,
                    mcp_tool,
                    event_kind: McpEventKind::Call,
                    turn_id: None,
                    status: None,
                    is_error: None,
                    arguments_json,
                    output_preview: None,
                    error_text: None,
                });
            }
            "tool_result" => {
                let Some(call_id) = item.get("tool_use_id").and_then(Value::as_str) else {
                    continue;
                };
                let is_error = item.get("is_error").and_then(Value::as_bool);
                let content_text = match item.get("content") {
                    Some(Value::String(text)) => Some(text.clone()),
                    Some(Value::Array(parts)) => {
                        let joined: Vec<&str> = parts
                            .iter()
                            .filter_map(|part| {
                                part.as_str()
                                    .or_else(|| part.get("text").and_then(Value::as_str))
                            })
                            .collect();
                        (!joined.is_empty()).then(|| joined.join(" "))
                    }
                    _ => None,
                };
                let (output_preview, error_text) = match (is_error, &content_text) {
                    (Some(true), Some(text)) => (None, Some(bounded_preview(text))),
                    (_, Some(text)) => (Some(bounded_preview(text)), None),
                    _ => (None, None),
                };
                events.push(ExtractedMcpEvent {
                    call_id: clamp_chars(call_id, MAX_NAME_CHARS),
                    // Result rows don't carry the tool name in Claude's
                    // shape; the DB layer resolves it from the paired call
                    // row when persisting (see `db::mcp_events`). Kept empty
                    // here rather than guessed.
                    tool_name: String::new(),
                    mcp_server: None,
                    mcp_tool: None,
                    event_kind: McpEventKind::Result,
                    turn_id: None,
                    status: Some(
                        if is_error == Some(true) {
                            "error"
                        } else {
                            "ok"
                        }
                        .to_string(),
                    ),
                    is_error,
                    arguments_json: None,
                    output_preview,
                    error_text,
                });
            }
            _ => {}
        }
    }
    events
}

/// Extract Codex `function_call`/`function_call_output` events from one
/// transcript line's raw JSON value (`{"type": "response_item", "payload":
/// {...}}`).
pub fn extract_codex_mcp_events(value: &Value) -> Vec<ExtractedMcpEvent> {
    if value.get("type").and_then(Value::as_str) != Some("response_item") {
        return Vec::new();
    }
    let Some(payload) = value.get("payload") else {
        return Vec::new();
    };
    let payload_type = payload.get("type").and_then(Value::as_str);
    let turn_id = payload
        .pointer("/metadata/turn_id")
        .and_then(Value::as_str)
        .map(|s| clamp_chars(s, MAX_NAME_CHARS));

    match payload_type {
        Some("function_call") => {
            let Some(call_id) = payload.get("call_id").and_then(Value::as_str) else {
                return Vec::new();
            };
            let Some(name) = payload.get("name").and_then(Value::as_str) else {
                return Vec::new();
            };
            let (mcp_server, mcp_tool) = classify_tool_name(name);
            let arguments_json = payload
                .get("arguments")
                .and_then(Value::as_str)
                .map(bounded_preview);
            vec![ExtractedMcpEvent {
                call_id: clamp_chars(call_id, MAX_NAME_CHARS),
                tool_name: clamp_chars(name, MAX_NAME_CHARS),
                mcp_server,
                mcp_tool,
                event_kind: McpEventKind::Call,
                turn_id,
                status: None,
                is_error: None,
                arguments_json,
                output_preview: None,
                error_text: None,
            }]
        }
        Some("function_call_output") => {
            let Some(call_id) = payload.get("call_id").and_then(Value::as_str) else {
                return Vec::new();
            };
            let raw_output = payload.get("output").and_then(Value::as_str);
            // Codex wraps output as a JSON-encoded string:
            // `{"output": "...", "metadata": {"exit_code": N, ...}}"`.
            // Best-effort parse it to recover exit_code/is_error without
            // requiring it — plain string output is also accepted.
            let (output_text, is_error, status) =
                match raw_output.and_then(|s| serde_json::from_str::<Value>(s).ok()) {
                    Some(parsed) => {
                        let text = parsed
                            .get("output")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .or_else(|| raw_output.map(str::to_string));
                        let exit_code = parsed
                            .pointer("/metadata/exit_code")
                            .and_then(Value::as_i64);
                        let is_error = exit_code.map(|code| code != 0);
                        let status = exit_code.map(|code| code.to_string());
                        (text, is_error, status)
                    }
                    None => (raw_output.map(str::to_string), None, None),
                };
            let (output_preview, error_text) = match (is_error, &output_text) {
                (Some(true), Some(text)) => (None, Some(bounded_preview(text))),
                (_, Some(text)) => (Some(bounded_preview(text)), None),
                _ => (None, None),
            };
            vec![ExtractedMcpEvent {
                call_id: clamp_chars(call_id, MAX_NAME_CHARS),
                tool_name: String::new(),
                mcp_server: None,
                mcp_tool: None,
                event_kind: McpEventKind::Result,
                turn_id,
                status,
                is_error,
                arguments_json: None,
                output_preview,
                error_text,
            }]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "mcp_events_tests.rs"]
mod tests;
