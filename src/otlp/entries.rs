//! OTLP `ExportLogsServiceRequest` → [`LogBatchEntry`] conversion, plus the
//! `AnyValue` stringify/JSON helpers it depends on.

use std::collections::HashMap;
use std::net::SocketAddr;

use opentelemetry_proto::tonic::{
    collector::logs::v1::ExportLogsServiceRequest,
    common::v1::{AnyValue, any_value::Value as AnyValueKind},
};

use crate::db::LogBatchEntry;
use crate::enrich::{SourceKind, stamp_source_kind};
use crate::ingest_metadata::{attrs_to_metadata_object, bounded_metadata_json};

/// Walk the OTLP request and produce one `LogBatchEntry` per `LogRecord`.
pub(super) fn build_entries(
    req: &ExportLogsServiceRequest,
    peer: SocketAddr,
) -> Vec<LogBatchEntry> {
    let received_iso = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let source_ip = peer.to_string();
    let peer_ip = peer.ip().to_string();

    let mut out = Vec::new();
    for resource_logs in &req.resource_logs {
        let resource_attrs: HashMap<&str, &AnyValue> = resource_logs
            .resource
            .as_ref()
            .map(|r| {
                r.attributes
                    .iter()
                    .filter_map(|kv| kv.value.as_ref().map(|v| (kv.key.as_str(), v)))
                    .collect()
            })
            .unwrap_or_default();
        let hostname = resource_attrs
            .get("host.name")
            .and_then(|v| any_value_to_string(v))
            .unwrap_or_default();
        let service_name = resource_attrs
            .get("service.name")
            .and_then(|v| any_value_to_string(v));
        let service_version = resource_attrs
            .get("service.version")
            .and_then(|v| any_value_to_string(v));

        for scope_logs in &resource_logs.scope_logs {
            for log in &scope_logs.log_records {
                let log_attrs: HashMap<&str, &AnyValue> = log
                    .attributes
                    .iter()
                    .filter_map(|kv| kv.value.as_ref().map(|v| (kv.key.as_str(), v)))
                    .collect();

                let ai_session_id = log_attrs
                    .get("session.id")
                    .or_else(|| log_attrs.get("session_id"))
                    .or_else(|| resource_attrs.get("session.id"))
                    .or_else(|| resource_attrs.get("session_id"))
                    .and_then(|v| any_value_to_string(v))
                    .filter(|value| value.len() <= 128);

                let ai_project = log_attrs
                    .get("project.path")
                    .or_else(|| log_attrs.get("codebase.root_path"))
                    .or_else(|| log_attrs.get("session.cwd"))
                    .or_else(|| resource_attrs.get("project.path"))
                    .or_else(|| resource_attrs.get("codebase.root_path"))
                    .or_else(|| resource_attrs.get("session.cwd"))
                    .and_then(|v| any_value_to_string(v))
                    .filter(|value| value.len() <= 512);

                let timestamp = format_otlp_timestamp(log.time_unix_nano)
                    .unwrap_or_else(|| received_iso.clone());
                let severity = severity_from_number(log.severity_number).to_string();
                let message = log
                    .body
                    .as_ref()
                    .and_then(any_value_to_string)
                    .unwrap_or_default();
                let metadata_json = bounded_metadata_json(serde_json::json!({
                    "source_type": "otlp",
                    "peer_ip": peer_ip,
                    "peer_port": peer.port(),
                    "host_name": hostname,
                    "service_name": service_name,
                    "service_version": service_version,
                    "severity_number": log.severity_number,
                    "severity_text": log.severity_text,
                    "trace_id": hex_bytes(&log.trace_id),
                    "span_id": hex_bytes(&log.span_id),
                    "flags": log.flags,
                    "event_name": log.event_name,
                    "resource_attributes": attrs_to_json(&resource_attrs),
                    "log_attributes": attrs_to_json(&log_attrs),
                }));
                let mut entry = LogBatchEntry {
                    timestamp,
                    hostname: hostname.clone(),
                    facility: Some("otlp".to_string()),
                    severity,
                    app_name: service_name.clone(),
                    process_id: None,
                    message,
                    raw: metadata_json.clone(),
                    source_ip: source_ip.clone(),
                    docker_checkpoint: None,
                    ai_tool: extract_ai_tool(&log_attrs, &resource_attrs),
                    ai_project,
                    ai_session_id,
                    ai_transcript_path: None,
                    metadata_json: Some(metadata_json),
                    http_status: None,
                    auth_outcome: None,
                    dns_blocked: None,
                    event_action: None,
                    parse_error: None,
                };
                stamp_source_kind(&mut entry, SourceKind::Otlp);
                out.push(entry);
            }
        }
    }
    out
}

fn attrs_to_json(attrs: &HashMap<&str, &AnyValue>) -> serde_json::Value {
    attrs_to_metadata_object(
        attrs
            .iter()
            .map(|(key, value)| (*key, any_value_to_json(value))),
    )
}

fn any_value_to_json(v: &AnyValue) -> serde_json::Value {
    match v.value.as_ref() {
        Some(AnyValueKind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(AnyValueKind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(AnyValueKind::IntValue(i)) => serde_json::Value::Number((*i).into()),
        Some(AnyValueKind::DoubleValue(f)) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Some(AnyValueKind::BytesValue(b)) => serde_json::json!({"bytes_len": b.len()}),
        Some(AnyValueKind::ArrayValue(arr)) => serde_json::json!({"array_len": arr.values.len()}),
        Some(AnyValueKind::KvlistValue(kv)) => serde_json::json!({"kvlist_len": kv.values.len()}),
        Some(AnyValueKind::StringValueStrindex(idx)) => {
            serde_json::json!({"string_table_index": idx})
        }
        None => serde_json::Value::Null,
    }
}

fn hex_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn extract_ai_tool(
    log_attrs: &HashMap<&str, &AnyValue>,
    resource_attrs: &HashMap<&str, &AnyValue>,
) -> Option<String> {
    let raw = log_attrs
        .get("ai.tool")
        .or_else(|| log_attrs.get("ai_tool"))
        .or_else(|| resource_attrs.get("ai.tool"))
        .or_else(|| resource_attrs.get("ai_tool"))
        .and_then(|v| any_value_to_string(v))?;
    if raw.len() > 64 {
        return None;
    }
    match raw.to_ascii_lowercase().as_str() {
        "claude" | "codex" | "gemini" => Some(raw.to_ascii_lowercase()),
        _ => None,
    }
}

fn format_otlp_timestamp(time_unix_nano: u64) -> Option<String> {
    if time_unix_nano == 0 {
        return None;
    }
    let secs = (time_unix_nano / 1_000_000_000) as i64;
    let nanos = (time_unix_nano % 1_000_000_000) as u32;
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
}

/// OTLP `SeverityNumber` (0–24) → syslog severity string.
///
/// 0 (UNSPECIFIED) and any unrecognised value fall through to `info` rather
/// than dropping the record.
fn severity_from_number(n: i32) -> &'static str {
    match n {
        1..=8 => "debug", // OTLP TRACE (1..=4) and DEBUG (5..=8) both map here
        9..=12 => "info",
        13..=16 => "warning",
        17..=20 => "err",
        21..=24 => "crit",
        _ => "info", // 0=UNSPECIFIED and out-of-range fall back to info
    }
}

/// Stringify any `AnyValue` variant for storage in `message` / attribute fields.
/// Composite types render as a placeholder rather than expanding inline.
fn any_value_to_string(v: &AnyValue) -> Option<String> {
    match v.value.as_ref()? {
        AnyValueKind::StringValue(s) => Some(s.clone()),
        AnyValueKind::BoolValue(b) => Some(b.to_string()),
        AnyValueKind::IntValue(i) => Some(i.to_string()),
        AnyValueKind::DoubleValue(f) => Some(f.to_string()),
        AnyValueKind::BytesValue(b) => Some(format!("[{} bytes]", b.len())),
        AnyValueKind::ArrayValue(arr) => Some(format!("[array len={}]", arr.values.len())),
        AnyValueKind::KvlistValue(kv) => Some(format!("[kvlist len={}]", kv.values.len())),
        AnyValueKind::StringValueStrindex(idx) => Some(format!("[string_table_index={idx}]")),
    }
}

#[cfg(test)]
#[path = "entries_tests.rs"]
mod tests;
