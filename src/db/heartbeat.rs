use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use tracing::warn;

use super::pool::DbPool;

#[derive(Debug, Clone)]
pub enum HeartbeatHostLookup {
    HostId(String),
    Hostname(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatHostState {
    pub host_id: String,
    pub hostname: String,
    pub total_samples: usize,
    pub truncated: bool,
    pub flags: HeartbeatStateFlags,
    pub latest: Option<HeartbeatSampleState>,
    pub samples: Vec<HeartbeatSampleState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatStateFlags {
    pub collector_partial: bool,
    pub heartbeat_late: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatSampleState {
    pub heartbeat_id: i64,
    pub host_id: String,
    pub hostname: String,
    pub sampled_at: String,
    pub received_at: String,
    pub source_ip: String,
    pub boot_id: String,
    pub sequence: i64,
    pub uptime_secs: i64,
    pub collection_ms: i64,
    pub partial: bool,
    pub agent_version: String,
    pub os: String,
    pub kernel: Option<String>,
    pub architecture: String,
    pub metadata: Option<Value>,
    pub cpu: Option<Value>,
    pub memory: Option<Value>,
    pub disks: Vec<Value>,
    pub network: Vec<Value>,
    pub processes: Option<Value>,
    pub containers: Vec<Value>,
}

#[derive(Debug)]
struct HeartbeatRow {
    id: i64,
    host_id: String,
    hostname: String,
    source_ip: String,
    sampled_at: String,
    received_at: String,
    boot_id: String,
    uptime_secs: i64,
    sequence: i64,
    collection_ms: i64,
    partial: bool,
    agent_version: String,
    os: String,
    kernel: Option<String>,
    architecture: String,
    metadata_json: Option<String>,
}

pub fn heartbeat_host_state(
    pool: &DbPool,
    lookup: HeartbeatHostLookup,
    since: Option<&str>,
    limit: usize,
) -> Result<HeartbeatHostState> {
    let conn = pool.get()?;
    let host_id = match lookup {
        HeartbeatHostLookup::HostId(host_id) => host_id,
        HeartbeatHostLookup::Hostname(hostname) => resolve_unique_hostname(&conn, &hostname)?,
    };

    let limit = limit.clamp(1, 100);
    let fetch_limit = limit + 1;
    let rows = if let Some(since) = since {
        let mut stmt = conn.prepare(
            "SELECT id, host_id, hostname, source_ip, sampled_at, received_at, boot_id,
                    uptime_secs, sequence, collection_ms, partial, agent_version,
                    os, kernel, architecture, metadata_json
             FROM host_heartbeats
             WHERE host_id = ?1 AND sampled_at >= ?2
             ORDER BY sampled_at DESC, id DESC
             LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(
                params![host_id, since, fetch_limit as i64],
                map_heartbeat_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, host_id, hostname, source_ip, sampled_at, received_at, boot_id,
                    uptime_secs, sequence, collection_ms, partial, agent_version,
                    os, kernel, architecture, metadata_json
             FROM host_heartbeats
             WHERE host_id = ?1
             ORDER BY sampled_at DESC, id DESC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![host_id, fetch_limit as i64], map_heartbeat_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    if rows.is_empty() {
        return Err(anyhow!("not_found"));
    }

    let truncated = rows.len() > limit;
    let mut samples = Vec::with_capacity(limit.min(rows.len()));
    for row in rows.into_iter().take(limit) {
        samples.push(sample_from_row(&conn, row)?);
    }
    let latest = samples.first().cloned();
    let host_id = samples[0].host_id.clone();
    let hostname = samples[0].hostname.clone();
    let flags = latest
        .as_ref()
        .map(heartbeat_flags)
        .unwrap_or(HeartbeatStateFlags {
            collector_partial: false,
            heartbeat_late: false,
        });

    Ok(HeartbeatHostState {
        host_id,
        hostname,
        total_samples: samples.len(),
        truncated,
        flags,
        latest,
        samples,
    })
}

fn heartbeat_flags(sample: &HeartbeatSampleState) -> HeartbeatStateFlags {
    let interval_secs = sample
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.pointer("/agent/interval_secs"))
        .and_then(Value::as_i64)
        .unwrap_or(30)
        .max(1);
    let received_at = match chrono::DateTime::parse_from_rfc3339(&sample.received_at) {
        Ok(dt) => Some(dt.with_timezone(&chrono::Utc)),
        Err(error) => {
            warn!(
                heartbeat_id = sample.heartbeat_id,
                received_at = %sample.received_at,
                error = %error,
                "heartbeat received_at timestamp failed to parse; heartbeat_late check skipped"
            );
            None
        }
    };
    let heartbeat_late = received_at.is_some_and(|received_at| {
        let elapsed = chrono::Utc::now().signed_duration_since(received_at);
        elapsed.num_milliseconds() > interval_secs * 2500
    });
    HeartbeatStateFlags {
        collector_partial: sample.partial,
        heartbeat_late,
    }
}

fn resolve_unique_hostname(conn: &rusqlite::Connection, hostname: &str) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT host_id
         FROM host_heartbeats
         WHERE hostname = ?1
         GROUP BY host_id
         ORDER BY MAX(received_at) DESC
         LIMIT 2",
    )?;
    let host_ids = stmt
        .query_map([hostname], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    match host_ids.as_slice() {
        [] => Err(anyhow!("not_found")),
        [host_id] => Ok(host_id.clone()),
        _ => Err(anyhow!("ambiguous_host")),
    }
}

fn map_heartbeat_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HeartbeatRow> {
    Ok(HeartbeatRow {
        id: row.get(0)?,
        host_id: row.get(1)?,
        hostname: row.get(2)?,
        source_ip: row.get(3)?,
        sampled_at: row.get(4)?,
        received_at: row.get(5)?,
        boot_id: row.get(6)?,
        uptime_secs: row.get(7)?,
        sequence: row.get(8)?,
        collection_ms: row.get(9)?,
        partial: row.get::<_, i64>(10)? != 0,
        agent_version: row.get(11)?,
        os: row.get(12)?,
        kernel: row.get(13)?,
        architecture: row.get(14)?,
        metadata_json: row.get(15)?,
    })
}

fn sample_from_row(conn: &rusqlite::Connection, row: HeartbeatRow) -> Result<HeartbeatSampleState> {
    Ok(HeartbeatSampleState {
        heartbeat_id: row.id,
        host_id: row.host_id,
        hostname: row.hostname,
        sampled_at: row.sampled_at,
        received_at: row.received_at,
        source_ip: row.source_ip,
        boot_id: row.boot_id,
        sequence: row.sequence,
        uptime_secs: row.uptime_secs,
        collection_ms: row.collection_ms,
        partial: row.partial,
        agent_version: row.agent_version,
        os: row.os,
        kernel: row.kernel,
        architecture: row.architecture,
        metadata: row
            .metadata_json
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok()),
        cpu: one_json(
            conn,
            "SELECT json_object(
                 'load1', load1, 'load5', load5, 'load15', load15,
                 'usage_percent', usage_percent, 'steal_percent', steal_percent,
                 'io_wait_percent', io_wait_percent
             ) FROM heartbeat_cpu WHERE heartbeat_id = ?1",
            row.id,
        )?,
        memory: one_json(
            conn,
            "SELECT json_object(
                 'total_bytes', total_bytes, 'available_bytes', available_bytes,
                 'used_percent', used_percent, 'swap_total_bytes', swap_total_bytes,
                 'swap_used_bytes', swap_used_bytes
             ) FROM heartbeat_memory WHERE heartbeat_id = ?1",
            row.id,
        )?,
        disks: many_json(
            conn,
            "SELECT json_object(
                 'mountpoint', mountpoint, 'filesystem', filesystem,
                 'total_bytes', total_bytes, 'available_bytes', available_bytes,
                 'used_percent', used_percent, 'read_bytes_per_sec', read_bytes_per_sec,
                 'write_bytes_per_sec', write_bytes_per_sec
             ) FROM heartbeat_disks WHERE heartbeat_id = ?1 ORDER BY id ASC",
            row.id,
        )?,
        network: many_json(
            conn,
            "SELECT json_object(
                 'interface', interface, 'rx_bytes_per_sec', rx_bytes_per_sec,
                 'tx_bytes_per_sec', tx_bytes_per_sec, 'rx_errors', rx_errors,
                 'tx_errors', tx_errors
             ) FROM heartbeat_network WHERE heartbeat_id = ?1 ORDER BY id ASC",
            row.id,
        )?,
        processes: one_json(
            conn,
            "SELECT json_object(
                 'total', total, 'running', running, 'sleeping', sleeping, 'zombie', zombie,
                 'top_cpu', json(top_cpu_json), 'top_memory', json(top_memory_json)
             ) FROM heartbeat_processes WHERE heartbeat_id = ?1",
            row.id,
        )?,
        containers: many_json(
            conn,
            "SELECT json_object(
                 'runtime', runtime, 'running', running, 'stopped', stopped,
                 'unhealthy', unhealthy, 'summary', json(summary_json)
             ) FROM heartbeat_containers WHERE heartbeat_id = ?1 ORDER BY id ASC",
            row.id,
        )?,
    })
}

fn one_json(conn: &rusqlite::Connection, sql: &str, heartbeat_id: i64) -> Result<Option<Value>> {
    let raw: Option<String> = conn
        .query_row(sql, [heartbeat_id], |row| row.get(0))
        .optional()?;
    Ok(raw.and_then(|raw| {
        serde_json::from_str(&raw)
            .map_err(|error| {
                warn!(heartbeat_id, error = %error, "failed to parse heartbeat JSON column");
            })
            .ok()
    }))
}

fn many_json(conn: &rusqlite::Connection, sql: &str, heartbeat_id: i64) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([heartbeat_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|raw| {
            serde_json::from_str(&raw)
                .map_err(|error| {
                    warn!(heartbeat_id, error = %error, "failed to parse heartbeat JSON row");
                })
                .ok()
        })
        .collect())
}

#[cfg(test)]
#[path = "heartbeat_tests.rs"]
mod tests;
