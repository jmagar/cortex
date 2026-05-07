//! OTLP/HTTP receiver — accepts OpenTelemetry log records over HTTP and feeds
//! them into the existing syslog-mcp ingest pipeline. Logs only — `/v1/traces`
//! returns 404 (deferred) and `/v1/metrics` returns 200 + discards.
//!
//! Mounted on the same axum server as MCP. Body limit: 4 MiB. Optional Bearer
//! auth via the same `SYSLOG_MCP_API_TOKEN` as MCP.

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
use subtle::ConstantTimeEq;
use tower_http::limit::RequestBodyLimitLayer;

use crate::db::LogBatchEntry;
use crate::ingest::IngestTx;

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

/// Build the OTLP router. Mounts `/v1/logs`, `/v1/metrics`, `/v1/traces` on the
/// same axum server as MCP.
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

    // try_send each entry. Any Full result aborts the request with 503 so
    // OTel exporters back off instead of growing the in-memory queue.
    for entry in entries {
        if state.ingest.try_send(entry).is_err() {
            tracing::warn!(
                source_ip = %peer,
                "OTLP write channel full — returning 503"
            );
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "channel_full"})),
            )
                .into_response();
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
    _body: Bytes,
) -> axum::response::Response {
    if !is_authorized(&state, &headers) {
        return unauthorized();
    }
    StatusCode::OK.into_response()
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
    let source_ip = peer.to_string();

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
                let timestamp = format_otlp_timestamp(log.time_unix_nano)
                    .unwrap_or_else(|| received_iso.clone());
                let severity = severity_from_number(log.severity_number).to_string();
                let message = log
                    .body
                    .as_ref()
                    .and_then(any_value_to_string)
                    .unwrap_or_default();
                let raw = service_version.clone().unwrap_or_default();

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
                });
            }
        }
    }
    out
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
        1..=4 => "debug",
        5..=8 => "debug",
        9..=12 => "info",
        13..=16 => "warning",
        17..=20 => "err",
        21..=24 => "crit",
        _ => "info",
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
    bearer_token(auth).is_some_and(|tok| token_matches(tok, expected))
}

fn bearer_token(auth: &str) -> Option<&str> {
    let mut parts = auth.split_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    if parts.next().is_some() || !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    Some(token)
}

fn token_matches(provided: &str, expected: &str) -> bool {
    const MAX_TOKEN_LEN: usize = 4096;
    if provided.len() > MAX_TOKEN_LEN || expected.len() > MAX_TOKEN_LEN {
        return false;
    }
    let mut a = [0_u8; MAX_TOKEN_LEN];
    let mut b = [0_u8; MAX_TOKEN_LEN];
    a[..provided.len()].copy_from_slice(provided.as_bytes());
    b[..expected.len()].copy_from_slice(expected.as_bytes());
    let bytes_match = a.ct_eq(&b).unwrap_u8() == 1;
    let lens_match = (provided.len() as u64)
        .ct_eq(&(expected.len() as u64))
        .unwrap_u8()
        == 1;
    bytes_match && lens_match
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
