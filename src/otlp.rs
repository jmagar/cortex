//! OTLP/HTTP receiver — accepts OpenTelemetry log records over HTTP and feeds
//! them into the existing syslog-mcp ingest pipeline. Logs only — `/v1/traces`
//! returns 404 (deferred) and `/v1/metrics` returns 404 (deferred).
//!
//! Mounted on the same axum server as MCP. Body limit: 4 MiB. Optional Bearer
//! auth via the same `SYSLOG_MCP_TOKEN` as MCP (`SYSLOG_MCP_API_TOKEN` is
//! accepted as a deprecated alias).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, State},
    http::{header::RETRY_AFTER, HeaderMap, HeaderValue, StatusCode},
    middleware::{from_fn, Next},
    response::{IntoResponse, Json},
    routing::post,
    Router,
};
use bytes::Bytes;
use opentelemetry_proto::tonic::{
    collector::logs::v1::ExportLogsServiceRequest,
    common::v1::{any_value::Value as AnyValueKind, AnyValue},
};
use prost::Message;
use serde_json::json;
use tower_http::limit::RequestBodyLimitLayer;

use crate::db::LogBatchEntry;
use crate::ingest::IngestTx;
use lab_auth::middleware::{parse_bearer_token, tokens_equal};

/// Per-request body cap. Matches the OpenTelemetry Collector default for
/// HTTP receivers. Larger payloads receive 413 + `Retry-After: 86400`.
pub const OTLP_BODY_LIMIT_BYTES: usize = 4 * 1024 * 1024;

/// Atomic counters for the OTLP receiver, surfaced via `/health`.
#[derive(Debug, Default)]
pub struct OtlpCounters {
    pub logs_received: AtomicU64,
    pub decode_errors: AtomicU64,
}

/// Shared state for every OTLP route.
#[derive(Clone)]
pub struct OtlpState {
    pub(crate) ingest: IngestTx,
    pub api_token: Option<String>,
    pub counters: Arc<OtlpCounters>,
}

impl OtlpState {
    pub(crate) fn new(
        ingest: IngestTx,
        api_token: Option<String>,
        counters: Arc<OtlpCounters>,
    ) -> Self {
        Self {
            ingest,
            api_token,
            counters,
        }
    }
}

/// Build the OTLP router. Mounts `/v1/logs` (functional ingest),
/// `/v1/metrics` (404 — deferred), `/v1/traces` (404 — deferred) on the same
/// axum server as MCP.
pub fn router(state: OtlpState) -> Router {
    Router::new()
        .route("/v1/logs", post(logs_handler))
        .route("/v1/metrics", post(metrics_handler))
        .route("/v1/traces", post(traces_handler))
        .layer(RequestBodyLimitLayer::new(OTLP_BODY_LIMIT_BYTES))
        .layer(from_fn(add_retry_after_on_413))
        .with_state(state)
}

/// Tower middleware: attach `Retry-After: 86400` to any 413 response so OTLP
/// exporters back off instead of hammering the endpoint on retry.
async fn add_retry_after_on_413(
    req: axum::extract::Request,
    next: Next,
) -> axum::response::Response {
    let mut response = next.run(req).await;
    if response.status() == StatusCode::PAYLOAD_TOO_LARGE {
        response
            .headers_mut()
            .insert(RETRY_AFTER, HeaderValue::from_static("86400"));
    }
    response
}

async fn logs_handler(
    State(state): State<OtlpState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if !is_authorized(&state, &headers) {
        tracing::warn!(
            source_ip = %peer,
            has_auth = headers.contains_key(axum::http::header::AUTHORIZATION),
            "OTLP /v1/logs unauthorized"
        );
        return unauthorized();
    }

    // prost decode is CPU-bound; isolate from the runtime via spawn_blocking.
    let body_for_decode = body.clone();
    let decoded =
        tokio::task::spawn_blocking(move || ExportLogsServiceRequest::decode(body_for_decode))
            .await;

    let req = match decoded {
        Ok(Ok(req)) => req,
        Ok(Err(err)) => {
            state.counters.decode_errors.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(error = %err, source_ip = %peer, "OTLP /v1/logs decode failed");
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "decode_failed", "message": err.to_string()})),
            )
                .into_response();
        }
        Err(err) => {
            state.counters.decode_errors.fetch_add(1, Ordering::Relaxed);
            tracing::error!(error = %err, "OTLP decode task panicked");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal"})),
            )
                .into_response();
        }
    };

    let entries = build_entries(&req, peer);
    let count = entries.len();

    // Pre-flight capacity check: reject the WHOLE request with 503 if the
    // channel can't fit it. Without this, partial accept (entries 0..N
    // queued, N+1 hits Full, return 503) leads to OTel exporter retry of
    // the full batch — duplicating the rows already accepted. See review
    // threads PRRT_*ALWfA / *ALb_p / *ALYDZ.
    if state.ingest.capacity() < count {
        tracing::warn!(
            source_ip = %peer,
            requested = count,
            available = state.ingest.capacity(),
            "OTLP write channel insufficient capacity — returning 503 (no partial accept)"
        );
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "channel_full"})),
        )
            .into_response();
    }

    // Capacity reservation is best-effort (concurrent senders may consume
    // slots between check and send). On Full mid-loop we still 503, which
    // can in the worst case duplicate a few records on retry, but the
    // pre-flight makes the common case clean.
    for entry in entries {
        match state.ingest.try_send(entry) {
            Ok(()) => {}
            Err(crate::ingest::TrySendErr::Full) => {
                tracing::warn!(
                    source_ip = %peer,
                    "OTLP write channel filled mid-batch — returning 503"
                );
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error": "channel_full"})),
                )
                    .into_response();
            }
            Err(crate::ingest::TrySendErr::Closed) => {
                tracing::error!(
                    source_ip = %peer,
                    "OTLP write channel CLOSED — batch writer task is dead"
                );
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "writer_unavailable"})),
                )
                    .into_response();
            }
        }
    }

    state
        .counters
        .logs_received
        .fetch_add(count as u64, Ordering::Relaxed);
    tracing::info!(records = count, source_ip = %peer, "OTLP logs ingested");
    StatusCode::OK.into_response()
}

async fn metrics_handler(
    State(state): State<OtlpState>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if !is_authorized(&state, &headers) {
        return unauthorized();
    }
    tracing::warn!(
        bytes = body.len(),
        "OTLP metrics received but metrics ingestion is not supported"
    );
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "metrics_not_supported",
            "message": "OTLP metrics deferred. Use /v1/logs only."
        })),
    )
        .into_response()
}

async fn traces_handler() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "traces_not_supported",
            "message": "OTLP traces deferred. Use /v1/logs only."
        })),
    )
        .into_response()
}

/// Walk the OTLP request and produce one `LogBatchEntry` per `LogRecord`.
fn build_entries(req: &ExportLogsServiceRequest, peer: SocketAddr) -> Vec<LogBatchEntry> {
    let received_iso = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let source_ip = peer.ip().to_string();

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
                let metadata_json = serde_json::json!({
                    "source_type": "otlp",
                    "peer_ip": source_ip,
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
                })
                .to_string();
                let raw = metadata_json.clone();

                out.push(LogBatchEntry {
                    timestamp,
                    hostname: hostname.clone(),
                    facility: Some("otlp".to_string()),
                    severity,
                    app_name: service_name.clone(),
                    process_id: None,
                    message,
                    raw,
                    source_ip: source_ip.clone(),
                    docker_checkpoint: None,
                    ai_tool: extract_ai_tool(&log_attrs, &resource_attrs),
                    ai_project,
                    ai_session_id,
                    ai_transcript_path: None,
                    metadata_json: Some(metadata_json),
                });
            }
        }
    }
    out
}

fn attrs_to_json(attrs: &HashMap<&str, &AnyValue>) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    for (key, value) in attrs {
        object.insert((*key).to_string(), any_value_to_json(value));
    }
    serde_json::Value::Object(object)
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
    }
}

fn is_authorized(state: &OtlpState, headers: &HeaderMap) -> bool {
    let Some(expected) = state.api_token.as_deref() else {
        return true;
    };
    let Some(auth) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    parse_bearer_token(auth).is_some_and(|tok| tokens_equal(&tok, expected))
}

fn unauthorized() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "unauthorized"})),
    )
        .into_response()
}

#[cfg(test)]
#[path = "otlp_tests.rs"]
mod tests;
