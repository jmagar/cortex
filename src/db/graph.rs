#![allow(dead_code)]

//! Derived investigation graph schema vocabulary.
//!
//! The graph is a rebuildable projection over authoritative source tables
//! (`logs`, heartbeats, AI session rollups, source inventory, signatures). Keep
//! vocabulary constants here so schema, extraction, service, adapters, and docs
//! do not drift into hand-written string variants.

use std::time::Instant;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::pool::{write_lock, DbPool};

const GRAPH_REBUILD_CHUNK_SIZE: i64 = 10_000;
static GRAPH_REBUILD_LOCK: Mutex<()> = Mutex::new(());
#[cfg(test)]
pub(crate) static GRAPH_TEST_LOCK: Mutex<()> = Mutex::new(());

pub const ENTITY_TYPE_HOST: &str = "host";
pub const ENTITY_TYPE_CONTAINER: &str = "container";
pub const ENTITY_TYPE_SERVICE: &str = "service";
pub const ENTITY_TYPE_APP: &str = "app";
pub const ENTITY_TYPE_SOURCE_IP: &str = "source_ip";
pub const ENTITY_TYPE_AI_PROJECT: &str = "ai_project";
pub const ENTITY_TYPE_AI_SESSION: &str = "ai_session";
pub const ENTITY_TYPE_ERROR_SIGNATURE: &str = "error_signature";
pub const ENTITY_TYPE_COMPOSE_PROJECT: &str = "compose_project";
pub const ENTITY_TYPE_REVERSE_PROXY: &str = "reverse_proxy";
pub const ENTITY_TYPE_DOMAIN: &str = "domain";
pub const ENTITY_TYPE_NETWORK: &str = "network";
pub const ENTITY_TYPE_STORAGE: &str = "storage";
pub const ENTITY_TYPE_CONFIG_ARTIFACT: &str = "config_artifact";

pub const ENTITY_TYPES: &[&str] = &[
    ENTITY_TYPE_HOST,
    ENTITY_TYPE_CONTAINER,
    ENTITY_TYPE_SERVICE,
    ENTITY_TYPE_APP,
    ENTITY_TYPE_SOURCE_IP,
    ENTITY_TYPE_AI_PROJECT,
    ENTITY_TYPE_AI_SESSION,
    ENTITY_TYPE_ERROR_SIGNATURE,
    ENTITY_TYPE_COMPOSE_PROJECT,
    ENTITY_TYPE_REVERSE_PROXY,
    ENTITY_TYPE_DOMAIN,
    ENTITY_TYPE_NETWORK,
    ENTITY_TYPE_STORAGE,
    ENTITY_TYPE_CONFIG_ARTIFACT,
];

pub const REL_OBSERVED_AS: &str = "observed_as";
pub const REL_RUNS_ON: &str = "runs_on";
pub const REL_EMITTED_BY: &str = "emitted_by";
pub const REL_WORKED_ON: &str = "worked_on";
pub const REL_MATCHES_SIGNATURE: &str = "matches_signature";
pub const REL_DEFINES_SERVICE: &str = "defines_service";
pub const REL_ROUTES_TO: &str = "routes_to";
pub const REL_EXPOSES_DOMAIN: &str = "exposes_domain";
pub const REL_ATTACHED_TO: &str = "attached_to";
pub const REL_MOUNTS: &str = "mounts";
pub const REL_BACKED_BY: &str = "backed_by";
pub const REL_HAS_ARTIFACT: &str = "has_artifact";

pub const RELATIONSHIP_TYPES: &[&str] = &[
    REL_OBSERVED_AS,
    REL_RUNS_ON,
    REL_EMITTED_BY,
    REL_WORKED_ON,
    REL_MATCHES_SIGNATURE,
    REL_DEFINES_SERVICE,
    REL_ROUTES_TO,
    REL_EXPOSES_DOMAIN,
    REL_ATTACHED_TO,
    REL_MOUNTS,
    REL_BACKED_BY,
    REL_HAS_ARTIFACT,
];

pub const TRUST_VERIFIED: &str = "verified";
pub const TRUST_CLAIMED: &str = "claimed";
pub const TRUST_INFERRED: &str = "inferred";
pub const TRUST_CORRELATED: &str = "correlated";

pub const TRUST_LEVELS: &[&str] = &[
    TRUST_VERIFIED,
    TRUST_CLAIMED,
    TRUST_INFERRED,
    TRUST_CORRELATED,
];

pub const SOURCE_KIND_LOG: &str = "log";
pub const SOURCE_KIND_HEARTBEAT: &str = "heartbeat";
pub const SOURCE_KIND_AI_SESSION_ROLLUP: &str = "ai_session_rollup";
pub const SOURCE_KIND_SOURCE_INVENTORY: &str = "source_inventory";
pub const SOURCE_KIND_APP_INVENTORY: &str = "app_inventory";
pub const SOURCE_KIND_ERROR_SIGNATURE: &str = "error_signature";

pub const EVIDENCE_SOURCE_KINDS: &[&str] = &[
    SOURCE_KIND_LOG,
    SOURCE_KIND_HEARTBEAT,
    SOURCE_KIND_AI_SESSION_ROLLUP,
    SOURCE_KIND_SOURCE_INVENTORY,
    SOURCE_KIND_APP_INVENTORY,
    SOURCE_KIND_ERROR_SIGNATURE,
];

pub const REASON_SYSLOG_CLAIMED_HOSTNAME: &str = "syslog_claimed_hostname";
pub const REASON_LOG_APP_NAME: &str = "log_app_name";
pub const REASON_DOCKER_CONTAINER_ID: &str = "docker_container_id";
pub const REASON_DOCKER_SERVICE_LABEL: &str = "docker_service_label";
pub const REASON_AI_SESSION_PROJECT: &str = "ai_session_project";
pub const REASON_HEARTBEAT_HOST_STATE: &str = "heartbeat_host_state";
pub const REASON_ERROR_SIGNATURE_MATCH: &str = "error_signature_match";
pub const REASON_INVENTORY_NODE: &str = "inventory_node";
pub const REASON_INVENTORY_SERVICE: &str = "inventory_service";
pub const REASON_COMPOSE_CONFIG: &str = "compose_config";
pub const REASON_REVERSE_PROXY_CONFIG: &str = "reverse_proxy_config";
pub const REASON_DOCKER_NETWORK: &str = "docker_network";
pub const REASON_STORAGE_PROBE: &str = "storage_probe";
pub const REASON_CONFIG_ARTIFACT: &str = "config_artifact";

pub const REASON_CODES: &[&str] = &[
    REASON_SYSLOG_CLAIMED_HOSTNAME,
    REASON_LOG_APP_NAME,
    REASON_DOCKER_CONTAINER_ID,
    REASON_DOCKER_SERVICE_LABEL,
    REASON_AI_SESSION_PROJECT,
    REASON_HEARTBEAT_HOST_STATE,
    REASON_ERROR_SIGNATURE_MATCH,
    REASON_INVENTORY_NODE,
    REASON_INVENTORY_SERVICE,
    REASON_COMPOSE_CONFIG,
    REASON_REVERSE_PROXY_CONFIG,
    REASON_DOCKER_NETWORK,
    REASON_STORAGE_PROBE,
    REASON_CONFIG_ARTIFACT,
];

pub const PROJECTION_STATUS_NEVER_BUILT: &str = "never_built";
pub const PROJECTION_STATUS_BUILDING: &str = "building";
pub const PROJECTION_STATUS_READY: &str = "ready";
pub const PROJECTION_STATUS_STALE: &str = "stale";
pub const PROJECTION_STATUS_FAILED: &str = "failed";

pub const PROJECTION_STATUSES: &[&str] = &[
    PROJECTION_STATUS_NEVER_BUILT,
    PROJECTION_STATUS_BUILDING,
    PROJECTION_STATUS_READY,
    PROJECTION_STATUS_STALE,
    PROJECTION_STATUS_FAILED,
];

pub fn is_known_entity_type(value: &str) -> bool {
    ENTITY_TYPES.contains(&value)
}

pub fn is_known_relationship_type(value: &str) -> bool {
    RELATIONSHIP_TYPES.contains(&value)
}

pub fn is_known_reason_code(value: &str) -> bool {
    REASON_CODES.contains(&value)
}

pub fn is_known_trust_level(value: &str) -> bool {
    TRUST_LEVELS.contains(&value)
}

pub fn is_known_evidence_source_kind(value: &str) -> bool {
    EVIDENCE_SOURCE_KINDS.contains(&value)
}

pub fn canonical_graph_key(value: &str) -> Option<String> {
    normalized(value)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphProjectionStatus {
    pub projection_status: String,
    pub last_started_at: Option<String>,
    pub last_completed_at: Option<String>,
    pub source_watermark: String,
    pub source_row_count: i64,
    pub entity_count: i64,
    pub relationship_count: i64,
    pub evidence_count: i64,
    pub is_degraded: bool,
    pub last_error: Option<String>,
    pub last_runtime_ms: i64,
    pub last_chunk_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphRebuildStats {
    pub source_row_count: i64,
    pub entity_count: i64,
    pub relationship_count: i64,
    pub evidence_count: i64,
    pub source_watermark: String,
    pub runtime_ms: i64,
    pub chunk_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraphRebuildOutcome {
    Rebuilt(GraphRebuildStats),
    AlreadyRunning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphEntityRow {
    pub id: i64,
    pub entity_type: String,
    pub canonical_key: String,
    pub display_label: String,
    pub source_kind: String,
    pub source_id: String,
    pub trust_level: String,
    pub first_seen_at: Option<String>,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphEntityCandidateRow {
    pub entity: GraphEntityRow,
    pub match_reason: String,
    pub alias_type: Option<String>,
    pub alias_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphRelationshipRow {
    pub id: i64,
    pub relationship_key: String,
    pub src_entity_id: i64,
    pub dst_entity_id: i64,
    pub relationship_type: String,
    pub reason_code: String,
    pub trust_level: String,
    pub confidence: f64,
    pub evidence_count: i64,
    pub first_seen_at: Option<String>,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphEvidenceRow {
    pub id: i64,
    pub relationship_id: i64,
    pub evidence_key: String,
    pub source_kind: String,
    pub source_id: String,
    pub source_log_id: Option<i64>,
    pub source_heartbeat_id: Option<i64>,
    pub source_signature_hash: Option<String>,
    pub observed_at: String,
    pub reason_code: String,
    pub reason_text: Option<String>,
    pub confidence_delta: f64,
    pub trust_level: String,
    pub safe_excerpt: Option<String>,
    pub metadata_path: Option<String>,
    pub evidence_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphAroundRows {
    pub relationships: Vec<GraphRelationshipRow>,
    pub entities: Vec<GraphEntityRow>,
    pub evidence: Vec<GraphEvidenceRow>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphSourceLogSummaryRow {
    pub id: i64,
    pub timestamp: String,
    pub received_at: String,
    pub hostname: String,
    pub severity: String,
    pub app_name: Option<String>,
    pub process_id: Option<String>,
    pub source_ip: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphEvidenceLookupRows {
    pub evidence: GraphEvidenceRow,
    pub relationship: GraphRelationshipRow,
    pub src_entity: GraphEntityRow,
    pub dst_entity: GraphEntityRow,
    pub source_log_summary: Option<GraphSourceLogSummaryRow>,
}

#[derive(Debug)]
struct LogGraphRow {
    id: i64,
    timestamp: String,
    hostname: String,
    app_name: Option<String>,
    source_ip: String,
    ai_tool: Option<String>,
    ai_project: Option<String>,
    ai_session_id: Option<String>,
    metadata_json: Option<String>,
}

pub fn graph_projection_status(pool: &DbPool) -> Result<GraphProjectionStatus> {
    let conn = pool.get()?;
    conn.query_row(
        "SELECT projection_status, last_started_at, last_completed_at,
                source_watermark, source_row_count, entity_count,
                relationship_count, evidence_count, is_degraded, last_error,
                COALESCE(last_runtime_ms, 0), COALESCE(last_chunk_count, 0)
         FROM graph_projection_meta WHERE id = 1",
        [],
        |row| {
            Ok(GraphProjectionStatus {
                projection_status: row.get(0)?,
                last_started_at: row.get(1)?,
                last_completed_at: row.get(2)?,
                source_watermark: row.get(3)?,
                source_row_count: row.get(4)?,
                entity_count: row.get(5)?,
                relationship_count: row.get(6)?,
                evidence_count: row.get(7)?,
                is_degraded: row.get::<_, i64>(8)? != 0,
                last_error: row.get(9)?,
                last_runtime_ms: row.get(10)?,
                last_chunk_count: row.get(11)?,
            })
        },
    )
    .context("read graph projection status")
}

pub fn find_graph_entity_by_key(
    pool: &DbPool,
    entity_type: &str,
    canonical_key: &str,
) -> Result<Option<GraphEntityRow>> {
    let conn = pool.get()?;
    let key = canonical_graph_key(canonical_key).unwrap_or_else(|| canonical_key.to_string());
    conn.query_row(
        "SELECT id, entity_type, canonical_key, display_label, source_kind,
                source_id, trust_level, first_seen_at, last_seen_at
         FROM graph_entities
         WHERE entity_type = ?1 AND canonical_key = ?2",
        params![entity_type, key],
        graph_entity_from_row,
    )
    .optional()
    .map_err(Into::into)
}

pub fn find_graph_entity_by_id(pool: &DbPool, entity_id: i64) -> Result<Option<GraphEntityRow>> {
    let conn = pool.get()?;
    conn.query_row(
        "SELECT id, entity_type, canonical_key, display_label, source_kind,
                source_id, trust_level, first_seen_at, last_seen_at
         FROM graph_entities
         WHERE id = ?1",
        [entity_id],
        graph_entity_from_row,
    )
    .optional()
    .map_err(Into::into)
}

pub fn find_graph_entities_by_alias(
    pool: &DbPool,
    alias_type: &str,
    alias_key: &str,
    limit: u32,
) -> Result<Vec<GraphEntityCandidateRow>> {
    let conn = pool.get()?;
    let key = canonical_graph_key(alias_key).unwrap_or_else(|| alias_key.to_string());
    let limit = limit.clamp(1, 500);
    let mut stmt = conn.prepare(
        "SELECT e.id, e.entity_type, e.canonical_key, e.display_label, e.source_kind,
                e.source_id, e.trust_level, e.first_seen_at, e.last_seen_at,
                a.alias_type, a.alias_key
         FROM graph_entity_aliases a
         JOIN graph_entities e ON e.id = a.entity_id
         WHERE a.alias_type = ?1 AND a.alias_key = ?2
         ORDER BY e.last_seen_at DESC, e.id ASC
         LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![alias_type, key, limit], |row| {
            Ok(GraphEntityCandidateRow {
                entity: GraphEntityRow {
                    id: row.get(0)?,
                    entity_type: row.get(1)?,
                    canonical_key: row.get(2)?,
                    display_label: row.get(3)?,
                    source_kind: row.get(4)?,
                    source_id: row.get(5)?,
                    trust_level: row.get(6)?,
                    first_seen_at: row.get(7)?,
                    last_seen_at: row.get(8)?,
                },
                match_reason: "alias".to_string(),
                alias_type: row.get(9)?,
                alias_key: row.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn graph_around_entity(
    pool: &DbPool,
    entity_id: i64,
    limit: u32,
    evidence_sample_limit: u32,
) -> Result<GraphAroundRows> {
    let conn = pool.get()?;
    let limit = limit.clamp(1, 500);
    let evidence_sample_limit = evidence_sample_limit.clamp(0, 10);
    let fetch_limit = limit.saturating_add(1);

    let mut stmt = conn.prepare(
        "SELECT id, relationship_key, src_entity_id, dst_entity_id, relationship_type,
                reason_code, trust_level, confidence, evidence_count,
                first_seen_at, last_seen_at
         FROM graph_relationships
         WHERE src_entity_id = ?1 OR dst_entity_id = ?1
         ORDER BY last_seen_at DESC, id DESC
         LIMIT ?2",
    )?;
    let mut relationships = stmt
        .query_map(params![entity_id, fetch_limit], graph_relationship_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let truncated = relationships.len() > limit as usize;
    relationships.truncate(limit as usize);

    let mut entity_ids = Vec::with_capacity(relationships.len() * 2 + 1);
    entity_ids.push(entity_id);
    for rel in &relationships {
        entity_ids.push(rel.src_entity_id);
        entity_ids.push(rel.dst_entity_id);
    }
    entity_ids.sort_unstable();
    entity_ids.dedup();
    let entities = graph_entities_by_ids(&conn, &entity_ids)?;

    let relationship_ids: Vec<i64> = relationships.iter().map(|rel| rel.id).collect();
    let evidence =
        graph_evidence_for_relationships(&conn, &relationship_ids, evidence_sample_limit)?;

    Ok(GraphAroundRows {
        relationships,
        entities,
        evidence,
        truncated,
    })
}

pub fn graph_evidence_by_id(
    pool: &DbPool,
    evidence_id: i64,
) -> Result<Option<GraphEvidenceLookupRows>> {
    let conn = pool.get()?;
    let Some((evidence, relationship)) = conn
        .query_row(
            "SELECT
                e.id, e.relationship_id, e.evidence_key, e.source_kind, e.source_id,
                e.source_log_id, e.source_heartbeat_id, e.source_signature_hash,
                e.observed_at, e.reason_code, e.reason_text, e.confidence_delta,
                e.trust_level, e.safe_excerpt, e.metadata_path, e.evidence_count,
                r.id, r.relationship_key, r.src_entity_id, r.dst_entity_id,
                r.relationship_type, r.reason_code, r.trust_level, r.confidence,
                r.evidence_count, r.first_seen_at, r.last_seen_at
             FROM graph_relationship_evidence e
             JOIN graph_relationships r ON r.id = e.relationship_id
             WHERE e.id = ?1",
            [evidence_id],
            |row| {
                Ok((
                    graph_evidence_from_row(row)?,
                    GraphRelationshipRow {
                        id: row.get(16)?,
                        relationship_key: row.get(17)?,
                        src_entity_id: row.get(18)?,
                        dst_entity_id: row.get(19)?,
                        relationship_type: row.get(20)?,
                        reason_code: row.get(21)?,
                        trust_level: row.get(22)?,
                        confidence: row.get(23)?,
                        evidence_count: row.get(24)?,
                        first_seen_at: row.get(25)?,
                        last_seen_at: row.get(26)?,
                    },
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };

    let src_entity = conn.query_row(
        "SELECT id, entity_type, canonical_key, display_label, source_kind,
                source_id, trust_level, first_seen_at, last_seen_at
         FROM graph_entities
         WHERE id = ?1",
        [relationship.src_entity_id],
        graph_entity_from_row,
    )?;
    let dst_entity = conn.query_row(
        "SELECT id, entity_type, canonical_key, display_label, source_kind,
                source_id, trust_level, first_seen_at, last_seen_at
         FROM graph_entities
         WHERE id = ?1",
        [relationship.dst_entity_id],
        graph_entity_from_row,
    )?;
    let source_log_summary = match evidence.source_log_id {
        Some(source_log_id) => conn
            .query_row(
                "SELECT id, timestamp, received_at, hostname, severity, app_name,
                        process_id, source_ip, message
                 FROM logs
                 WHERE id = ?1",
                [source_log_id],
                |row| {
                    Ok(GraphSourceLogSummaryRow {
                        id: row.get(0)?,
                        timestamp: row.get(1)?,
                        received_at: row.get(2)?,
                        hostname: row.get(3)?,
                        severity: row.get(4)?,
                        app_name: row.get(5)?,
                        process_id: row.get(6)?,
                        source_ip: row.get(7)?,
                        message: row.get(8)?,
                    })
                },
            )
            .optional()?,
        None => None,
    };

    Ok(Some(GraphEvidenceLookupRows {
        evidence,
        relationship,
        src_entity,
        dst_entity,
        source_log_summary,
    }))
}

fn graph_entities_by_ids(conn: &rusqlite::Connection, ids: &[i64]) -> Result<Vec<GraphEntityRow>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT id, entity_type, canonical_key, display_label, source_kind,
                source_id, trust_level, first_seen_at, last_seen_at
         FROM graph_entities
         WHERE id IN ({placeholders})
         ORDER BY entity_type ASC, display_label ASC"
    );
    let params = ids.iter().copied().map(rusqlite::types::Value::Integer);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params), graph_entity_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn graph_evidence_for_relationships(
    conn: &rusqlite::Connection,
    relationship_ids: &[i64],
    evidence_sample_limit: u32,
) -> Result<Vec<GraphEvidenceRow>> {
    if relationship_ids.is_empty() || evidence_sample_limit == 0 {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", relationship_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT id, relationship_id, evidence_key, source_kind, source_id,
                source_log_id, source_heartbeat_id, source_signature_hash,
                observed_at, reason_code, reason_text, confidence_delta,
                trust_level, safe_excerpt, metadata_path, evidence_count
         FROM (
             SELECT e.*,
                    ROW_NUMBER() OVER (
                        PARTITION BY relationship_id
                        ORDER BY observed_at DESC, id DESC
                    ) AS rn
             FROM graph_relationship_evidence e
             WHERE relationship_id IN ({placeholders})
         )
         WHERE rn <= ?
         ORDER BY relationship_id ASC, observed_at DESC"
    );
    let mut values: Vec<rusqlite::types::Value> = relationship_ids
        .iter()
        .copied()
        .map(rusqlite::types::Value::Integer)
        .collect();
    values.push(rusqlite::types::Value::Integer(
        evidence_sample_limit as i64,
    ));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(values), graph_evidence_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn graph_entity_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GraphEntityRow> {
    Ok(GraphEntityRow {
        id: row.get(0)?,
        entity_type: row.get(1)?,
        canonical_key: row.get(2)?,
        display_label: row.get(3)?,
        source_kind: row.get(4)?,
        source_id: row.get(5)?,
        trust_level: row.get(6)?,
        first_seen_at: row.get(7)?,
        last_seen_at: row.get(8)?,
    })
}

fn graph_relationship_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GraphRelationshipRow> {
    Ok(GraphRelationshipRow {
        id: row.get(0)?,
        relationship_key: row.get(1)?,
        src_entity_id: row.get(2)?,
        dst_entity_id: row.get(3)?,
        relationship_type: row.get(4)?,
        reason_code: row.get(5)?,
        trust_level: row.get(6)?,
        confidence: row.get(7)?,
        evidence_count: row.get(8)?,
        first_seen_at: row.get(9)?,
        last_seen_at: row.get(10)?,
    })
}

fn graph_evidence_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GraphEvidenceRow> {
    Ok(GraphEvidenceRow {
        id: row.get(0)?,
        relationship_id: row.get(1)?,
        evidence_key: row.get(2)?,
        source_kind: row.get(3)?,
        source_id: row.get(4)?,
        source_log_id: row.get(5)?,
        source_heartbeat_id: row.get(6)?,
        source_signature_hash: row.get(7)?,
        observed_at: row.get(8)?,
        reason_code: row.get(9)?,
        reason_text: row.get(10)?,
        confidence_delta: row.get(11)?,
        trust_level: row.get(12)?,
        safe_excerpt: row.get(13)?,
        metadata_path: row.get(14)?,
        evidence_count: row.get(15)?,
    })
}

pub fn refresh_graph_projection(pool: &DbPool) -> Result<GraphRebuildOutcome> {
    let Some(_rebuild_guard) = GRAPH_REBUILD_LOCK.try_lock() else {
        return Ok(GraphRebuildOutcome::AlreadyRunning);
    };

    mark_graph_projection_building(pool)?;
    let started = Instant::now();
    match refresh_graph_projection_inner(pool, started) {
        Ok(stats) => Ok(GraphRebuildOutcome::Rebuilt(stats)),
        Err(err) => {
            let _ = mark_graph_projection_failed(pool, &err);
            Err(err)
        }
    }
}

fn refresh_graph_projection_inner(pool: &DbPool, started: Instant) -> Result<GraphRebuildStats> {
    let mut conn = pool.get()?;
    create_graph_staging_tables(&conn)?;

    let mut source_row_count = 0_i64;
    let mut chunk_count = 0_i64;
    let max_log_id: i64 =
        conn.query_row("SELECT COALESCE(MAX(id), 0) FROM logs", [], |r| r.get(0))?;
    let mut after_id = 0_i64;
    while after_id < max_log_id {
        let rows = fetch_log_graph_rows(&conn, after_id, GRAPH_REBUILD_CHUNK_SIZE)?;
        if rows.is_empty() {
            break;
        }
        chunk_count += 1;
        {
            let tx = conn.transaction()?;
            for row in &rows {
                after_id = after_id.max(row.id);
                source_row_count += 1;
                extract_log_row(&tx, row)?;
            }
            tx.commit()?;
        }
        mark_graph_projection_progress(&conn, source_row_count, chunk_count)?;
    }

    source_row_count += extract_heartbeat_latest(&conn)?;
    source_row_count += extract_error_signatures(&conn)?;

    let source_watermark = graph_source_watermark(&conn)?;
    let runtime_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
    let stats = swap_graph_projection(
        &mut conn,
        source_row_count,
        &source_watermark,
        runtime_ms,
        chunk_count,
    )?;
    let _ = conn.execute("DROP TABLE IF EXISTS _graph_entities_staging", []);
    let _ = conn.execute("DROP TABLE IF EXISTS _graph_aliases_staging", []);
    let _ = conn.execute("DROP TABLE IF EXISTS _graph_relationships_staging", []);
    let _ = conn.execute("DROP TABLE IF EXISTS _graph_evidence_staging", []);
    Ok(stats)
}

fn mark_graph_projection_building(pool: &DbPool) -> Result<()> {
    let conn = pool.get()?;
    let _guard = write_lock();
    conn.execute(
        "UPDATE graph_projection_meta
         SET projection_status = 'building',
             last_started_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             source_watermark = '',
             source_row_count = 0,
             entity_count = 0,
             relationship_count = 0,
             evidence_count = 0,
             is_degraded = 0,
             last_error = NULL,
             last_runtime_ms = 0,
             last_chunk_count = 0,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = 1",
        [],
    )?;
    Ok(())
}

fn mark_graph_projection_progress(
    conn: &rusqlite::Connection,
    source_row_count: i64,
    chunk_count: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE graph_projection_meta
         SET source_row_count = ?1,
             last_chunk_count = ?2,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = 1 AND projection_status = 'building'",
        params![source_row_count, chunk_count],
    )?;
    Ok(())
}

fn mark_graph_projection_failed(pool: &DbPool, err: &anyhow::Error) -> Result<()> {
    let conn = pool.get()?;
    let redacted = redact_error(&err.to_string());
    let _guard = write_lock();
    conn.execute(
        "UPDATE graph_projection_meta
         SET projection_status = 'failed',
             is_degraded = 1,
             last_error = ?1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = 1",
        [redacted],
    )?;
    Ok(())
}

fn create_graph_staging_tables(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS _graph_entities_staging;
         DROP TABLE IF EXISTS _graph_aliases_staging;
         DROP TABLE IF EXISTS _graph_relationships_staging;
         DROP TABLE IF EXISTS _graph_evidence_staging;

         CREATE TEMP TABLE _graph_entities_staging (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             entity_type TEXT NOT NULL,
             canonical_key TEXT NOT NULL,
             display_label TEXT NOT NULL,
             source_kind TEXT NOT NULL DEFAULT '',
             source_id TEXT NOT NULL DEFAULT '',
             trust_level TEXT NOT NULL,
             first_seen_at TEXT,
             last_seen_at TEXT,
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             UNIQUE(entity_type, canonical_key)
         );
         CREATE TEMP TABLE _graph_aliases_staging (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             entity_id INTEGER NOT NULL,
             alias_type TEXT NOT NULL,
             alias_key TEXT NOT NULL,
             alias_value TEXT NOT NULL,
             source_kind TEXT NOT NULL DEFAULT '',
             trust_level TEXT NOT NULL,
             first_seen_at TEXT,
             last_seen_at TEXT,
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             UNIQUE(entity_id, alias_type, alias_key, source_kind)
         );
         CREATE TEMP TABLE _graph_relationships_staging (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             relationship_key TEXT NOT NULL UNIQUE,
             src_entity_id INTEGER NOT NULL,
             dst_entity_id INTEGER NOT NULL,
             relationship_type TEXT NOT NULL,
             reason_code TEXT NOT NULL,
             trust_level TEXT NOT NULL,
             confidence REAL NOT NULL DEFAULT 0.0,
             evidence_count INTEGER NOT NULL DEFAULT 0,
             first_seen_at TEXT,
             last_seen_at TEXT,
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             UNIQUE(src_entity_id, dst_entity_id, relationship_type, relationship_key)
         );
         CREATE TEMP TABLE _graph_evidence_staging (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             relationship_id INTEGER NOT NULL,
             evidence_key TEXT NOT NULL,
             source_kind TEXT NOT NULL,
             source_id TEXT NOT NULL DEFAULT '',
             source_log_id INTEGER,
             source_heartbeat_id INTEGER,
             source_signature_hash TEXT,
             observed_at TEXT NOT NULL,
             reason_code TEXT NOT NULL,
             reason_text TEXT,
             confidence_delta REAL NOT NULL DEFAULT 0.0,
             trust_level TEXT NOT NULL,
             safe_excerpt TEXT,
             metadata_path TEXT,
             evidence_count INTEGER NOT NULL DEFAULT 1,
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             UNIQUE(relationship_id, evidence_key)
         );",
    )?;
    Ok(())
}

fn fetch_log_graph_rows(
    conn: &rusqlite::Connection,
    after_id: i64,
    limit: i64,
) -> Result<Vec<LogGraphRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, hostname, app_name, source_ip, ai_tool,
                ai_project, ai_session_id, metadata_json
         FROM logs
         WHERE id > ?1
         ORDER BY id ASC
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![after_id, limit], |row| {
            Ok(LogGraphRow {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                hostname: row.get(2)?,
                app_name: row.get(3)?,
                source_ip: row.get(4)?,
                ai_tool: row.get(5)?,
                ai_project: row.get(6)?,
                ai_session_id: row.get(7)?,
                metadata_json: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn extract_log_row(conn: &rusqlite::Connection, row: &LogGraphRow) -> Result<()> {
    let source_id = row.id.to_string();
    let source_entity = if let Some(key) = normalized(&row.source_ip) {
        Some(ensure_entity(
            conn,
            ENTITY_TYPE_SOURCE_IP,
            &key,
            &row.source_ip,
            SOURCE_KIND_LOG,
            &source_id,
            TRUST_VERIFIED,
            Some(&row.timestamp),
            Some(&row.timestamp),
        )?)
    } else {
        None
    };
    let host_entity = if let Some(key) = normalized(&row.hostname) {
        Some(ensure_entity(
            conn,
            ENTITY_TYPE_HOST,
            &key,
            &row.hostname,
            SOURCE_KIND_LOG,
            &source_id,
            TRUST_CLAIMED,
            Some(&row.timestamp),
            Some(&row.timestamp),
        )?)
    } else {
        None
    };

    if let Some(host_id) = host_entity {
        insert_alias(
            conn,
            host_id,
            "hostname",
            &normalize_key(&row.hostname),
            &row.hostname,
            SOURCE_KIND_LOG,
            TRUST_CLAIMED,
            Some(&row.timestamp),
            Some(&row.timestamp),
        )?;
    }
    if let (Some(source_id_entity), Some(host_id)) = (source_entity, host_entity) {
        ensure_relationship_with_evidence(
            conn,
            source_id_entity,
            host_id,
            REL_OBSERVED_AS,
            REASON_SYSLOG_CLAIMED_HOSTNAME,
            TRUST_CLAIMED,
            0.6,
            EvidenceInput {
                evidence_key: evidence_bucket_key(
                    "log",
                    row.id,
                    REASON_SYSLOG_CLAIMED_HOSTNAME,
                    &row.timestamp,
                ),
                source_kind: SOURCE_KIND_LOG,
                source_id: &source_id,
                source_log_id: Some(row.id),
                source_heartbeat_id: None,
                source_signature_hash: None,
                observed_at: &row.timestamp,
                reason_text: Some("syslog header hostname claimed by sender"),
                confidence_delta: 0.6,
                trust_level: TRUST_CLAIMED,
                safe_excerpt: Some(&row.hostname),
                metadata_path: None,
            },
        )?;
    }

    if let Some(app_name) = row.app_name.as_deref().and_then(normalized_value) {
        let app_id = ensure_entity(
            conn,
            ENTITY_TYPE_APP,
            &normalize_key(app_name),
            app_name,
            SOURCE_KIND_LOG,
            &source_id,
            TRUST_INFERRED,
            Some(&row.timestamp),
            Some(&row.timestamp),
        )?;
        if let Some(host_id) = host_entity {
            ensure_relationship_with_evidence(
                conn,
                app_id,
                host_id,
                REL_EMITTED_BY,
                REASON_LOG_APP_NAME,
                TRUST_INFERRED,
                0.5,
                EvidenceInput {
                    evidence_key: evidence_bucket_key(
                        "log",
                        row.id,
                        REASON_LOG_APP_NAME,
                        &row.timestamp,
                    ),
                    source_kind: SOURCE_KIND_LOG,
                    source_id: &source_id,
                    source_log_id: Some(row.id),
                    source_heartbeat_id: None,
                    source_signature_hash: None,
                    observed_at: &row.timestamp,
                    reason_text: Some("log app_name observed on claimed host"),
                    confidence_delta: 0.5,
                    trust_level: TRUST_INFERRED,
                    safe_excerpt: Some(app_name),
                    metadata_path: Some("logs.app_name"),
                },
            )?;
        }
    }

    extract_ai_log_row(conn, row)?;
    extract_docker_log_row(conn, row)?;
    Ok(())
}

fn extract_ai_log_row(conn: &rusqlite::Connection, row: &LogGraphRow) -> Result<()> {
    let Some(project) = row.ai_project.as_deref().and_then(normalized_value) else {
        return Ok(());
    };
    let Some(session) = row.ai_session_id.as_deref().and_then(normalized_value) else {
        return Ok(());
    };
    let tool = row
        .ai_tool
        .as_deref()
        .and_then(normalized_value)
        .unwrap_or("unknown");
    let source_id = row.id.to_string();
    let project_id = ensure_entity(
        conn,
        ENTITY_TYPE_AI_PROJECT,
        &normalize_key(project),
        project,
        SOURCE_KIND_LOG,
        &source_id,
        TRUST_VERIFIED,
        Some(&row.timestamp),
        Some(&row.timestamp),
    )?;
    let session_key = format!(
        "{}:{}:{}",
        normalize_key(project),
        normalize_key(tool),
        session
    );
    let session_label = format!("{project}/{tool}/{session}");
    let session_id = ensure_entity(
        conn,
        ENTITY_TYPE_AI_SESSION,
        &session_key,
        &session_label,
        SOURCE_KIND_LOG,
        &source_id,
        TRUST_VERIFIED,
        Some(&row.timestamp),
        Some(&row.timestamp),
    )?;
    ensure_relationship_with_evidence(
        conn,
        session_id,
        project_id,
        REL_WORKED_ON,
        REASON_AI_SESSION_PROJECT,
        TRUST_VERIFIED,
        0.9,
        EvidenceInput {
            evidence_key: evidence_bucket_key(
                "log",
                row.id,
                REASON_AI_SESSION_PROJECT,
                &row.timestamp,
            ),
            source_kind: SOURCE_KIND_LOG,
            source_id: &source_id,
            source_log_id: Some(row.id),
            source_heartbeat_id: None,
            source_signature_hash: None,
            observed_at: &row.timestamp,
            reason_text: Some("AI transcript metadata links session to project"),
            confidence_delta: 0.9,
            trust_level: TRUST_VERIFIED,
            safe_excerpt: Some(&session_label),
            metadata_path: Some("logs.ai_project/logs.ai_session_id"),
        },
    )?;
    Ok(())
}

fn extract_docker_log_row(conn: &rusqlite::Connection, row: &LogGraphRow) -> Result<()> {
    if !row.source_ip.starts_with("docker://") && !row.source_ip.starts_with("docker-event://") {
        return Ok(());
    }
    let meta = parse_metadata(row.metadata_json.as_deref());
    let parsed = parse_docker_source(&row.source_ip);
    let docker_host = metadata_text(&meta, &["docker_host", "docker.host"])
        .or(parsed.host)
        .and_then(normalized_value);
    let container = metadata_text(&meta, &["container_id", "docker.container_id"])
        .or_else(|| metadata_text(&meta, &["container_name", "docker.container_name"]))
        .or(parsed.container)
        .and_then(normalized_value);
    let Some(docker_host) = docker_host else {
        return Ok(());
    };
    let Some(container) = container else {
        return Ok(());
    };
    let source_id = row.id.to_string();
    let host_id = ensure_entity(
        conn,
        ENTITY_TYPE_HOST,
        &normalize_key(docker_host),
        docker_host,
        SOURCE_KIND_LOG,
        &source_id,
        TRUST_VERIFIED,
        Some(&row.timestamp),
        Some(&row.timestamp),
    )?;
    let container_key = format!(
        "{}:{}",
        normalize_key(docker_host),
        normalize_key(container)
    );
    let container_label = format!("{docker_host}/{container}");
    let container_id = ensure_entity(
        conn,
        ENTITY_TYPE_CONTAINER,
        &container_key,
        &container_label,
        SOURCE_KIND_LOG,
        &source_id,
        TRUST_VERIFIED,
        Some(&row.timestamp),
        Some(&row.timestamp),
    )?;
    ensure_relationship_with_evidence(
        conn,
        container_id,
        host_id,
        REL_RUNS_ON,
        REASON_DOCKER_CONTAINER_ID,
        TRUST_VERIFIED,
        0.9,
        EvidenceInput {
            evidence_key: evidence_bucket_key(
                "log",
                row.id,
                REASON_DOCKER_CONTAINER_ID,
                &row.timestamp,
            ),
            source_kind: SOURCE_KIND_LOG,
            source_id: &source_id,
            source_log_id: Some(row.id),
            source_heartbeat_id: None,
            source_signature_hash: None,
            observed_at: &row.timestamp,
            reason_text: Some("docker source identity links container to host"),
            confidence_delta: 0.9,
            trust_level: TRUST_VERIFIED,
            safe_excerpt: Some(&container_label),
            metadata_path: Some("logs.source_ip/metadata_json"),
        },
    )?;

    if let Some(service) = metadata_text(&meta, &["compose_service", "docker.compose_service"])
        .or_else(|| metadata_text(&meta, &["container_name", "docker.container_name"]))
        .and_then(normalized_value)
    {
        let project = metadata_text(&meta, &["compose_project", "docker.compose_project"])
            .and_then(normalized_value)
            .unwrap_or(docker_host);
        let service_key = format!(
            "{}:{}:{}",
            normalize_key(docker_host),
            normalize_key(project),
            normalize_key(service)
        );
        let service_label = format!("{docker_host}/{project}/{service}");
        let service_id = ensure_entity(
            conn,
            ENTITY_TYPE_SERVICE,
            &service_key,
            &service_label,
            SOURCE_KIND_LOG,
            &source_id,
            TRUST_INFERRED,
            Some(&row.timestamp),
            Some(&row.timestamp),
        )?;
        ensure_relationship_with_evidence(
            conn,
            container_id,
            service_id,
            REL_RUNS_ON,
            REASON_DOCKER_SERVICE_LABEL,
            TRUST_INFERRED,
            0.7,
            EvidenceInput {
                evidence_key: evidence_bucket_key(
                    "log",
                    row.id,
                    REASON_DOCKER_SERVICE_LABEL,
                    &row.timestamp,
                ),
                source_kind: SOURCE_KIND_LOG,
                source_id: &source_id,
                source_log_id: Some(row.id),
                source_heartbeat_id: None,
                source_signature_hash: None,
                observed_at: &row.timestamp,
                reason_text: Some("docker compose labels link container to service"),
                confidence_delta: 0.7,
                trust_level: TRUST_INFERRED,
                safe_excerpt: Some(&service_label),
                metadata_path: Some("metadata_json.compose_service"),
            },
        )?;
    }
    Ok(())
}

fn extract_heartbeat_latest(conn: &rusqlite::Connection) -> Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT heartbeat_id, host_id, hostname, sampled_at
         FROM host_heartbeats_latest
         ORDER BY hostname ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (heartbeat_id, host_id_value, hostname, sampled_at) in &rows {
        let Some(host_key) = normalized(hostname) else {
            continue;
        };
        let host_id = ensure_entity(
            conn,
            ENTITY_TYPE_HOST,
            &host_key,
            hostname,
            SOURCE_KIND_HEARTBEAT,
            &heartbeat_id.to_string(),
            TRUST_VERIFIED,
            Some(sampled_at),
            Some(sampled_at),
        )?;
        insert_alias(
            conn,
            host_id,
            "heartbeat_host_id",
            &normalize_key(host_id_value),
            host_id_value,
            SOURCE_KIND_HEARTBEAT,
            TRUST_VERIFIED,
            Some(sampled_at),
            Some(sampled_at),
        )?;
    }
    Ok(rows.len() as i64)
}

fn extract_error_signatures(conn: &rusqlite::Connection) -> Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT signature_hash, normalizer_version, template, sample_hostname,
                sample_app_name, first_seen_at, last_seen_at, total_count
         FROM error_signatures
         ORDER BY last_seen_at DESC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (hash, version, template, hostname, app_name, first_seen, last_seen, total_count) in &rows {
        let signature_key = format!("{hash}:{version}");
        let signature_id = ensure_entity(
            conn,
            ENTITY_TYPE_ERROR_SIGNATURE,
            &signature_key,
            &template.chars().take(120).collect::<String>(),
            SOURCE_KIND_ERROR_SIGNATURE,
            &signature_key,
            TRUST_INFERRED,
            Some(first_seen),
            Some(last_seen),
        )?;
        if let Some(app) = app_name.as_deref().and_then(normalized_value) {
            let app_id = ensure_entity(
                conn,
                ENTITY_TYPE_APP,
                &normalize_key(app),
                app,
                SOURCE_KIND_ERROR_SIGNATURE,
                &signature_key,
                TRUST_INFERRED,
                Some(first_seen),
                Some(last_seen),
            )?;
            ensure_relationship_with_evidence(
                conn,
                app_id,
                signature_id,
                REL_MATCHES_SIGNATURE,
                REASON_ERROR_SIGNATURE_MATCH,
                TRUST_INFERRED,
                0.7,
                EvidenceInput {
                    evidence_key: format!("signature:{signature_key}:app"),
                    source_kind: SOURCE_KIND_ERROR_SIGNATURE,
                    source_id: &signature_key,
                    source_log_id: None,
                    source_heartbeat_id: None,
                    source_signature_hash: Some(hash),
                    observed_at: last_seen,
                    reason_text: Some("error signature projection links app to template"),
                    confidence_delta: 0.7,
                    trust_level: TRUST_INFERRED,
                    safe_excerpt: Some(template),
                    metadata_path: Some("error_signatures"),
                },
            )?;
        }
        if let Some(host_key) = normalized(hostname) {
            let host_id = ensure_entity(
                conn,
                ENTITY_TYPE_HOST,
                &host_key,
                hostname,
                SOURCE_KIND_ERROR_SIGNATURE,
                &signature_key,
                TRUST_CLAIMED,
                Some(first_seen),
                Some(last_seen),
            )?;
            ensure_relationship_with_evidence(
                conn,
                host_id,
                signature_id,
                REL_MATCHES_SIGNATURE,
                REASON_ERROR_SIGNATURE_MATCH,
                TRUST_INFERRED,
                0.5,
                EvidenceInput {
                    evidence_key: format!("signature:{signature_key}:host"),
                    source_kind: SOURCE_KIND_ERROR_SIGNATURE,
                    source_id: &signature_key,
                    source_log_id: None,
                    source_heartbeat_id: None,
                    source_signature_hash: Some(hash),
                    observed_at: last_seen,
                    reason_text: Some("error signature projection links claimed host to template"),
                    confidence_delta: 0.5,
                    trust_level: TRUST_INFERRED,
                    safe_excerpt: Some(template),
                    metadata_path: Some("error_signatures"),
                },
            )?;
        }
        let _ = total_count;
    }
    Ok(rows.len() as i64)
}

#[allow(clippy::too_many_arguments)]
fn ensure_entity(
    conn: &rusqlite::Connection,
    entity_type: &str,
    canonical_key: &str,
    display_label: &str,
    source_kind: &str,
    source_id: &str,
    trust_level: &str,
    first_seen_at: Option<&str>,
    last_seen_at: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO _graph_entities_staging
             (entity_type, canonical_key, display_label, source_kind, source_id,
              trust_level, first_seen_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(entity_type, canonical_key) DO UPDATE SET
             display_label = CASE
                 WHEN _graph_entities_staging.display_label = '' THEN excluded.display_label
                 ELSE _graph_entities_staging.display_label END,
             first_seen_at = CASE
                 WHEN _graph_entities_staging.first_seen_at IS NULL THEN excluded.first_seen_at
                 WHEN excluded.first_seen_at IS NULL THEN _graph_entities_staging.first_seen_at
                 WHEN excluded.first_seen_at < _graph_entities_staging.first_seen_at THEN excluded.first_seen_at
                 ELSE _graph_entities_staging.first_seen_at END,
             last_seen_at = CASE
                 WHEN _graph_entities_staging.last_seen_at IS NULL THEN excluded.last_seen_at
                 WHEN excluded.last_seen_at IS NULL THEN _graph_entities_staging.last_seen_at
                 WHEN excluded.last_seen_at > _graph_entities_staging.last_seen_at THEN excluded.last_seen_at
                 ELSE _graph_entities_staging.last_seen_at END,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![
            entity_type,
            canonical_key,
            display_label,
            source_kind,
            source_id,
            trust_level,
            first_seen_at,
            last_seen_at
        ],
    )?;
    conn.query_row(
        "SELECT id FROM _graph_entities_staging
         WHERE entity_type = ?1 AND canonical_key = ?2",
        params![entity_type, canonical_key],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
fn insert_alias(
    conn: &rusqlite::Connection,
    entity_id: i64,
    alias_type: &str,
    alias_key: &str,
    alias_value: &str,
    source_kind: &str,
    trust_level: &str,
    first_seen_at: Option<&str>,
    last_seen_at: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO _graph_aliases_staging
             (entity_id, alias_type, alias_key, alias_value, source_kind,
              trust_level, first_seen_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(entity_id, alias_type, alias_key, source_kind) DO UPDATE SET
             last_seen_at = CASE
                 WHEN excluded.last_seen_at > _graph_aliases_staging.last_seen_at THEN excluded.last_seen_at
                 ELSE _graph_aliases_staging.last_seen_at END,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![
            entity_id,
            alias_type,
            alias_key,
            alias_value,
            source_kind,
            trust_level,
            first_seen_at,
            last_seen_at
        ],
    )?;
    Ok(())
}

struct EvidenceInput<'a> {
    evidence_key: String,
    source_kind: &'a str,
    source_id: &'a str,
    source_log_id: Option<i64>,
    source_heartbeat_id: Option<i64>,
    source_signature_hash: Option<&'a str>,
    observed_at: &'a str,
    reason_text: Option<&'a str>,
    confidence_delta: f64,
    trust_level: &'a str,
    safe_excerpt: Option<&'a str>,
    metadata_path: Option<&'a str>,
}

#[allow(clippy::too_many_arguments)]
fn ensure_relationship_with_evidence(
    conn: &rusqlite::Connection,
    src_entity_id: i64,
    dst_entity_id: i64,
    relationship_type: &str,
    reason_code: &str,
    trust_level: &str,
    confidence: f64,
    evidence: EvidenceInput<'_>,
) -> Result<()> {
    let relationship_key = format!("{src_entity_id}:{relationship_type}:{dst_entity_id}");
    conn.execute(
        "INSERT INTO _graph_relationships_staging
             (relationship_key, src_entity_id, dst_entity_id, relationship_type,
              reason_code, trust_level, confidence, evidence_count,
              first_seen_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8)
         ON CONFLICT(relationship_key) DO UPDATE SET
             confidence = MAX(_graph_relationships_staging.confidence, excluded.confidence),
             first_seen_at = CASE
                 WHEN excluded.first_seen_at < _graph_relationships_staging.first_seen_at THEN excluded.first_seen_at
                 ELSE _graph_relationships_staging.first_seen_at END,
             last_seen_at = CASE
                 WHEN excluded.last_seen_at > _graph_relationships_staging.last_seen_at THEN excluded.last_seen_at
                 ELSE _graph_relationships_staging.last_seen_at END,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![
            relationship_key,
            src_entity_id,
            dst_entity_id,
            relationship_type,
            reason_code,
            trust_level,
            confidence,
            evidence.observed_at
        ],
    )?;
    let relationship_id: i64 = conn.query_row(
        "SELECT id FROM _graph_relationships_staging WHERE relationship_key = ?1",
        [relationship_key],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO _graph_evidence_staging
             (relationship_id, evidence_key, source_kind, source_id, source_log_id,
              source_heartbeat_id, source_signature_hash, observed_at, reason_code,
              reason_text, confidence_delta, trust_level, safe_excerpt, metadata_path,
              evidence_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 1)
         ON CONFLICT(relationship_id, evidence_key) DO UPDATE SET
             evidence_count = _graph_evidence_staging.evidence_count + 1,
             observed_at = CASE
                 WHEN excluded.observed_at > _graph_evidence_staging.observed_at THEN excluded.observed_at
                 ELSE _graph_evidence_staging.observed_at END",
        params![
            relationship_id,
            evidence.evidence_key,
            evidence.source_kind,
            evidence.source_id,
            evidence.source_log_id,
            evidence.source_heartbeat_id,
            evidence.source_signature_hash,
            evidence.observed_at,
            reason_code,
            evidence.reason_text,
            evidence.confidence_delta,
            evidence.trust_level,
            evidence.safe_excerpt.map(truncate_safe_excerpt),
            evidence.metadata_path
        ],
    )?;
    conn.execute(
        "UPDATE _graph_relationships_staging
         SET evidence_count = (
             SELECT COALESCE(SUM(evidence_count), 0)
             FROM _graph_evidence_staging
             WHERE relationship_id = ?1
         )
         WHERE id = ?1",
        [relationship_id],
    )?;
    Ok(())
}

fn swap_graph_projection(
    conn: &mut rusqlite::Connection,
    source_row_count: i64,
    source_watermark: &str,
    runtime_ms: i64,
    chunk_count: i64,
) -> Result<GraphRebuildStats> {
    let entity_count = table_count(conn, "_graph_entities_staging")?;
    let relationship_count = table_count(conn, "_graph_relationships_staging")?;
    let evidence_count = table_count(conn, "_graph_evidence_staging")?;

    let _guard = write_lock();
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM graph_relationship_evidence", [])?;
    tx.execute("DELETE FROM graph_relationships", [])?;
    tx.execute("DELETE FROM graph_entity_aliases", [])?;
    tx.execute("DELETE FROM graph_entities", [])?;
    tx.execute(
        "INSERT INTO graph_entities
             (id, entity_type, canonical_key, display_label, source_kind, source_id,
              trust_level, first_seen_at, last_seen_at, created_at, updated_at)
         SELECT id, entity_type, canonical_key, display_label, source_kind, source_id,
                trust_level, first_seen_at, last_seen_at, created_at, updated_at
         FROM _graph_entities_staging",
        [],
    )?;
    tx.execute(
        "INSERT INTO graph_entity_aliases
             (id, entity_id, alias_type, alias_key, alias_value, source_kind,
              trust_level, first_seen_at, last_seen_at, created_at, updated_at)
         SELECT id, entity_id, alias_type, alias_key, alias_value, source_kind,
                trust_level, first_seen_at, last_seen_at, created_at, updated_at
         FROM _graph_aliases_staging",
        [],
    )?;
    tx.execute(
        "INSERT INTO graph_relationships
             (id, relationship_key, src_entity_id, dst_entity_id, relationship_type,
              reason_code, trust_level, confidence, evidence_count, first_seen_at,
              last_seen_at, created_at, updated_at)
         SELECT id, relationship_key, src_entity_id, dst_entity_id, relationship_type,
                reason_code, trust_level, confidence, evidence_count, first_seen_at,
                last_seen_at, created_at, updated_at
         FROM _graph_relationships_staging",
        [],
    )?;
    tx.execute(
        "INSERT INTO graph_relationship_evidence
             (id, relationship_id, evidence_key, source_kind, source_id, source_log_id,
              source_heartbeat_id, source_signature_hash, observed_at, reason_code,
              reason_text, confidence_delta, trust_level, safe_excerpt, metadata_path,
              evidence_count, created_at)
         SELECT id, relationship_id, evidence_key, source_kind, source_id, source_log_id,
                source_heartbeat_id, source_signature_hash, observed_at, reason_code,
                reason_text, confidence_delta, trust_level, safe_excerpt, metadata_path,
                evidence_count, created_at
         FROM _graph_evidence_staging",
        [],
    )?;
    tx.execute(
        "UPDATE graph_projection_meta
         SET projection_status = 'ready',
             last_completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             source_watermark = ?1,
             source_row_count = ?2,
             entity_count = ?3,
             relationship_count = ?4,
             evidence_count = ?5,
             is_degraded = 0,
             last_error = NULL,
             last_runtime_ms = ?6,
             last_chunk_count = ?7,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = 1",
        params![
            source_watermark,
            source_row_count,
            entity_count,
            relationship_count,
            evidence_count,
            runtime_ms,
            chunk_count
        ],
    )?;
    tx.commit()?;
    Ok(GraphRebuildStats {
        source_row_count,
        entity_count,
        relationship_count,
        evidence_count,
        source_watermark: source_watermark.to_string(),
        runtime_ms,
        chunk_count,
    })
}

fn graph_source_watermark(conn: &rusqlite::Connection) -> Result<String> {
    let max_log_id: i64 =
        conn.query_row("SELECT COALESCE(MAX(id), 0) FROM logs", [], |r| r.get(0))?;
    let max_heartbeat_id: i64 = conn.query_row(
        "SELECT COALESCE(MAX(heartbeat_id), 0) FROM host_heartbeats_latest",
        [],
        |r| r.get(0),
    )?;
    let signature_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM error_signatures", [], |r| r.get(0))?;
    Ok(format!(
        "logs:{max_log_id};heartbeats:{max_heartbeat_id};signatures:{signature_count}"
    ))
}

fn table_count(conn: &rusqlite::Connection, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(Into::into)
}

fn parse_metadata(input: Option<&str>) -> Option<Value> {
    input.and_then(|raw| serde_json::from_str::<Value>(raw).ok())
}

fn metadata_text<'a>(meta: &'a Option<Value>, paths: &[&str]) -> Option<&'a str> {
    let value = meta.as_ref()?;
    for path in paths {
        let mut current = value;
        let mut found = true;
        for segment in path.split('.') {
            if let Some(next) = current.get(segment) {
                current = next;
            } else {
                found = false;
                break;
            }
        }
        if found {
            if let Some(text) = current.as_str().and_then(normalized_value) {
                return Some(text);
            }
        }
    }
    None
}

#[derive(Debug, Default)]
struct DockerSourceParts<'a> {
    host: Option<&'a str>,
    container: Option<&'a str>,
}

fn parse_docker_source(source: &str) -> DockerSourceParts<'_> {
    let Some(rest) = source
        .strip_prefix("docker://")
        .or_else(|| source.strip_prefix("docker-event://"))
    else {
        return DockerSourceParts::default();
    };
    let mut parts = rest.split('/');
    DockerSourceParts {
        host: parts.next().and_then(normalized_value),
        container: parts.next().and_then(normalized_value),
    }
}

fn normalized(value: &str) -> Option<String> {
    normalized_value(value).map(normalize_key)
}

fn normalized_value(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn normalize_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn evidence_bucket_key(prefix: &str, source_id: i64, reason: &str, timestamp: &str) -> String {
    let _ = source_id;
    let bucket = timestamp.get(0..13).unwrap_or(timestamp);
    format!("{prefix}:{reason}:{bucket}")
}

fn truncate_safe_excerpt(value: &str) -> String {
    value.chars().take(512).collect()
}

fn redact_error(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(2048)
        .collect()
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
