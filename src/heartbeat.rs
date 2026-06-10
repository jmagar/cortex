//! Heartbeat telemetry ingest (`POST /v1/heartbeats`) — server side of the
//! fleet inventory/graph sub-product's host-state pipeline.
//!
//! Receives bounded JSON snapshots (load, memory, disk, top processes) from
//! the host-local agent in `heartbeat_agent.rs`, mounted on the shared HTTP
//! listener (port 3100) next to MCP and OTLP. Rows back the `host_state`,
//! `fleet_state`, and `correlate_state` actions and are retained 14 days.
//!
//! Invariants: request bodies are capped at 256 KiB; auth mirrors MCP — the
//! static `CORTEX_TOKEN` bearer when configured, with non-loopback
//! unauthenticated exposure rejected at startup by config validation.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    middleware::{Next, from_fn},
    response::{IntoResponse, Json},
    routing::post,
};
use bytes::Bytes;
use lab_auth::middleware::{parse_bearer_token, tokens_equal};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::limit::RequestBodyLimitLayer;

use crate::db::DbPool;
use crate::mcp::AuthPolicy;

pub const HEARTBEAT_BODY_LIMIT_BYTES: usize = 256 * 1024;

#[derive(Clone)]
pub struct HeartbeatState {
    pool: Arc<DbPool>,
    api_token: Option<String>,
    auth_policy: AuthPolicy,
}

impl HeartbeatState {
    pub fn new(pool: Arc<DbPool>, api_token: Option<String>, auth_policy: AuthPolicy) -> Self {
        Self {
            pool,
            api_token,
            auth_policy,
        }
    }
}

pub fn router(state: HeartbeatState) -> Router {
    Router::new()
        .route("/v1/heartbeats", post(heartbeat_handler))
        .layer(RequestBodyLimitLayer::new(HEARTBEAT_BODY_LIMIT_BYTES))
        .layer(from_fn(json_payload_too_large))
        .with_state(state)
}

async fn json_payload_too_large(
    req: axum::extract::Request,
    next: Next,
) -> axum::response::Response {
    let response = next.run(req).await;
    if response.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({"error": "payload_too_large"})),
        )
            .into_response();
    }
    response
}

async fn heartbeat_handler(
    State(state): State<HeartbeatState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if !is_authorized(&state, &peer, &headers) {
        return unauthorized();
    }

    let request: HeartbeatRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid_payload", "message": error.to_string()})),
            )
                .into_response();
        }
    };

    let pool = Arc::clone(&state.pool);
    let source_ip = peer.to_string();
    let exec_start = Instant::now();
    let join_result =
        tokio::task::spawn_blocking(move || insert_heartbeat(&pool, request, &source_ip)).await;
    let exec_ms = exec_start.elapsed().as_millis();
    let result = join_result
        .map_err(|error| anyhow::anyhow!("heartbeat insert task failed: {error}"))
        .and_then(|result| result);
    // Two-tier: heartbeat INSERTs target <5ms; warn only above 500ms to avoid noise.
    if exec_ms > 500 {
        match &result {
            Ok(_) => tracing::warn!(op = "heartbeat.insert", exec_ms, "db op ok"),
            Err(e) => tracing::warn!(op = "heartbeat.insert", exec_ms, error = %e, "db op err"),
        }
    } else {
        match &result {
            Ok(_) => tracing::debug!(op = "heartbeat.insert", exec_ms, "db op ok"),
            Err(e) => tracing::debug!(op = "heartbeat.insert", exec_ms, error = %e, "db op err"),
        }
    }

    match result {
        Ok(response) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "heartbeat ingest failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal_error"})),
            )
                .into_response()
        }
    }
}

fn is_authorized(state: &HeartbeatState, peer: &SocketAddr, headers: &HeaderMap) -> bool {
    if matches!(state.auth_policy, AuthPolicy::LoopbackDev) {
        return peer.ip().is_loopback();
    }
    let Some(expected) = state.api_token.as_deref() else {
        return false;
    };
    let Some(auth) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    parse_bearer_token(auth).is_some_and(|token| tokens_equal(&token, expected))
}

fn unauthorized() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "unauthorized"})),
    )
        .into_response()
}

fn insert_heartbeat(
    pool: &DbPool,
    request: HeartbeatRequest,
    source_ip: &str,
) -> anyhow::Result<HeartbeatIngestResponse> {
    let received_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut conn = pool.get()?;
    let _write_guard = crate::db::write_lock();
    let tx = conn.transaction()?;
    let metadata_json = heartbeat_metadata_json(&request)?;

    tx.execute(
        "INSERT OR IGNORE INTO host_heartbeats (
             host_id, hostname, source_ip, sampled_at, received_at, boot_id,
             uptime_secs, sequence, collection_ms, push_latency_ms, partial,
             agent_version, os, kernel, architecture, metadata_json
         ) VALUES (
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
         )",
        params![
            request.host.host_id,
            request.host.hostname,
            source_ip,
            request.sample.sampled_at,
            received_at,
            request.host.boot_id,
            request.sample.uptime_secs,
            request.sample.sequence,
            request.sample.collection_ms,
            request.agent.push_latency_ms,
            request.sample.partial as i64,
            request.agent.version,
            request.host.os,
            request.host.kernel,
            request.host.architecture,
            metadata_json,
        ],
    )?;

    let heartbeat_id = if tx.changes() == 0 {
        let id: i64 = tx.query_row(
            "SELECT id FROM host_heartbeats
             WHERE host_id = ?1 AND boot_id = ?2 AND sequence = ?3",
            params![
                request.host.host_id,
                request.host.boot_id,
                request.sample.sequence
            ],
            |row| row.get(0),
        )?;
        tx.commit()?;
        return Ok(HeartbeatIngestResponse {
            accepted: 0,
            heartbeat_id: id,
            received_at,
        });
    } else {
        tx.last_insert_rowid()
    };

    insert_metric_rows(&tx, heartbeat_id, &request)?;

    // Keep the fleet-state cache in sync. Only runs for accepted (non-duplicate)
    // heartbeats. The WHERE guard on sampled_at ensures out-of-order retries
    // never overwrite a newer entry with an older one.
    tx.execute(
        "INSERT INTO host_heartbeats_latest
             (host_id, heartbeat_id, hostname, sampled_at, received_at,
              partial, agent_version, os, architecture, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(host_id) DO UPDATE SET
             heartbeat_id  = excluded.heartbeat_id,
             hostname      = excluded.hostname,
             sampled_at    = excluded.sampled_at,
             received_at   = excluded.received_at,
             partial       = excluded.partial,
             agent_version = excluded.agent_version,
             os            = excluded.os,
             architecture  = excluded.architecture,
             metadata_json = excluded.metadata_json
         WHERE excluded.sampled_at >= host_heartbeats_latest.sampled_at",
        params![
            request.host.host_id,
            heartbeat_id,
            request.host.hostname,
            request.sample.sampled_at,
            received_at,
            request.sample.partial as i64,
            request.agent.version,
            request.host.os,
            request.host.architecture,
            metadata_json,
        ],
    )?;

    tx.commit()?;

    Ok(HeartbeatIngestResponse {
        accepted: 1,
        heartbeat_id,
        received_at,
    })
}

fn heartbeat_metadata_json(request: &HeartbeatRequest) -> anyhow::Result<String> {
    Ok(serde_json::to_string(&json!({
        "schema_version": request.schema_version,
        "host": {
            "timezone": request.host.timezone,
        },
        "sample": {
            "monotonic_ms": request.sample.monotonic_ms,
            "probe_errors": request.sample.probe_errors,
            "skipped_probes": request.sample.skipped_probes,
        },
        "agent": {
            "mode": request.agent.mode,
            "interval_secs": request.agent.interval_secs,
            "retry_backlog": request.agent.retry_backlog,
        },
        "gpu": request.gpu,
        "cpu": request.cpu,
        "memory": request.memory,
        "disks": request.disks,
        "networks": request.networks,
        "processes": request.processes,
        "containers": request.containers,
    }))?)
}

fn insert_metric_rows(
    tx: &rusqlite::Transaction<'_>,
    heartbeat_id: i64,
    request: &HeartbeatRequest,
) -> rusqlite::Result<()> {
    if let Some(cpu) = &request.cpu {
        tx.execute(
            "INSERT INTO heartbeat_cpu (
                 heartbeat_id, load1, load5, load15, usage_percent, steal_percent, io_wait_percent
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                heartbeat_id,
                cpu.load1,
                cpu.load5,
                cpu.load15,
                cpu.usage_pct,
                cpu.steal_pct,
                cpu.iowait_pct,
            ],
        )?;
    }

    if let Some(memory) = &request.memory {
        let used_percent = if memory.mem_total_bytes > 0 {
            let used = memory
                .mem_total_bytes
                .saturating_sub(memory.mem_available_bytes);
            Some((used as f64 / memory.mem_total_bytes as f64) * 100.0)
        } else {
            None
        };
        tx.execute(
            "INSERT INTO heartbeat_memory (
                 heartbeat_id, total_bytes, available_bytes, used_percent,
                 swap_total_bytes, swap_used_bytes
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                heartbeat_id,
                memory.mem_total_bytes,
                memory.mem_available_bytes,
                used_percent,
                memory.swap_total_bytes,
                memory.swap_used_bytes,
            ],
        )?;
    }

    for disk in &request.disks {
        tx.execute(
            "INSERT INTO heartbeat_disks (
                 heartbeat_id, mountpoint, filesystem, total_bytes, available_bytes,
                 used_percent, read_bytes_per_sec, write_bytes_per_sec
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                heartbeat_id,
                disk.name,
                disk.fs_type,
                disk.bytes_total,
                disk.bytes_free,
                disk.used_percent(),
                disk.read_bytes_per_sec,
                disk.write_bytes_per_sec,
            ],
        )?;
    }

    for net in &request.networks {
        tx.execute(
            "INSERT INTO heartbeat_network (
                 heartbeat_id, interface, rx_bytes_per_sec, tx_bytes_per_sec, rx_errors, tx_errors
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                heartbeat_id,
                net.interface,
                net.rx_bytes_per_sec,
                net.tx_bytes_per_sec,
                net.rx_errors_per_sec.map(|value| value.round() as i64),
                net.tx_errors_per_sec.map(|value| value.round() as i64),
            ],
        )?;
    }

    if let Some(processes) = &request.processes {
        tx.execute(
            "INSERT INTO heartbeat_processes (
                 heartbeat_id, total, running, sleeping, zombie, top_cpu_json, top_memory_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            params![
                heartbeat_id,
                processes.total,
                processes.running,
                processes.sleeping,
                processes.zombies,
                Some(
                    serde_json::to_string(&processes.top)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                ),
            ],
        )?;
    }

    if let Some(containers) = &request.containers {
        tx.execute(
            "INSERT INTO heartbeat_containers (
                 heartbeat_id, runtime, running, stopped, restarting, unhealthy, summary_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                heartbeat_id,
                containers.runtime.as_deref().unwrap_or("docker"),
                containers.running,
                containers.exited,
                containers.restarting,
                containers.unhealthy,
                Some(
                    serde_json::to_string(&containers.details)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                ),
            ],
        )?;
    }

    Ok(())
}

#[derive(Debug, Serialize)]
struct HeartbeatIngestResponse {
    accepted: u32,
    heartbeat_id: i64,
    received_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatRequest {
    #[serde(default = "default_schema_version")]
    schema_version: u8,
    host: HeartbeatHost,
    sample: HeartbeatSample,
    agent: HeartbeatAgent,
    #[serde(default)]
    cpu: Option<HeartbeatCpu>,
    #[serde(default)]
    memory: Option<HeartbeatMemory>,
    #[serde(default)]
    disks: Vec<HeartbeatDisk>,
    #[serde(default, alias = "network")]
    networks: Vec<HeartbeatNetwork>,
    #[serde(default)]
    processes: Option<HeartbeatProcesses>,
    #[serde(default)]
    containers: Option<HeartbeatContainers>,
    #[serde(default)]
    gpu: Option<serde_json::Value>,
}

fn default_schema_version() -> u8 {
    1
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatHost {
    host_id: String,
    hostname: String,
    os: String,
    #[serde(default)]
    kernel: Option<String>,
    architecture: String,
    boot_id: String,
    #[serde(default)]
    timezone: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatSample {
    sequence: i64,
    sampled_at: String,
    uptime_secs: i64,
    #[serde(default)]
    monotonic_ms: Option<i64>,
    collection_ms: i64,
    partial: bool,
    #[serde(default)]
    probe_errors: Vec<String>,
    #[serde(default)]
    skipped_probes: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatAgent {
    version: String,
    mode: String,
    interval_secs: i64,
    #[serde(default)]
    push_latency_ms: Option<i64>,
    #[serde(default)]
    retry_backlog: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatCpu {
    load1: f64,
    load5: f64,
    load15: f64,
    #[serde(default)]
    usage_pct: Option<f64>,
    #[serde(default)]
    user_pct: Option<f64>,
    #[serde(default)]
    system_pct: Option<f64>,
    #[serde(default)]
    iowait_pct: Option<f64>,
    #[serde(default)]
    steal_pct: Option<f64>,
    core_count: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatMemory {
    mem_total_bytes: i64,
    mem_available_bytes: i64,
    #[serde(default)]
    mem_used_bytes: Option<i64>,
    swap_total_bytes: i64,
    swap_used_bytes: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatDisk {
    kind: String,
    name: String,
    #[serde(default)]
    fs_type: Option<String>,
    #[serde(default)]
    bytes_total: Option<i64>,
    #[serde(default)]
    bytes_free: Option<i64>,
    #[serde(default)]
    bytes_used: Option<i64>,
    #[serde(default)]
    read_bytes_per_sec: Option<f64>,
    #[serde(default)]
    write_bytes_per_sec: Option<f64>,
}

impl HeartbeatDisk {
    fn used_percent(&self) -> Option<f64> {
        let total = self.bytes_total?;
        if total <= 0 {
            return None;
        }
        let used = self
            .bytes_used
            .or_else(|| self.bytes_free.map(|free| total.saturating_sub(free)))?;
        Some((used as f64 / total as f64) * 100.0)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatNetwork {
    interface: String,
    #[serde(default)]
    rx_bytes_per_sec: Option<f64>,
    #[serde(default)]
    tx_bytes_per_sec: Option<f64>,
    #[serde(default)]
    rx_errors_per_sec: Option<f64>,
    #[serde(default)]
    tx_errors_per_sec: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatProcesses {
    total: i64,
    #[serde(default)]
    running: Option<i64>,
    #[serde(default)]
    sleeping: Option<i64>,
    zombies: i64,
    #[serde(default)]
    top: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatContainers {
    #[serde(default)]
    runtime: Option<String>,
    reachable: bool,
    running: i64,
    exited: i64,
    restarting: i64,
    unhealthy: i64,
    #[serde(default)]
    details: Vec<serde_json::Value>,
}

#[cfg(test)]
#[path = "heartbeat_tests.rs"]
mod tests;
