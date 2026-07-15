use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::db::graph;
use crate::inventory::schema::TrustLevel;

use super::InventoryGraphStats;

#[derive(Debug, Clone)]
pub(super) struct EntityRef {
    pub(super) id: i64,
    pub(super) kind: &'static str,
    pub(super) key: String,
}

pub(super) fn prune_previous_inventory_projection(conn: &Connection) -> Result<()> {
    // Resolver-vocabulary edges (`instance_of` with reason
    // `resolver_instance_of`) are shared with the log-driven projection, so
    // they are pruned symmetrically with the evidence criteria: only rows
    // backed by inventory-sourced evidence are inventory-owned. This must run
    // before the evidence delete below, which removes the identifying rows.
    conn.execute(
        "DELETE FROM graph_relationships
          WHERE reason_code = ?1
            AND id IN (
                SELECT relationship_id FROM graph_relationship_evidence
                 WHERE source_kind IN ('source_inventory', 'app_inventory')
            )",
        [graph::REASON_RESOLVER_INSTANCE_OF],
    )?;
    conn.execute(
        "DELETE FROM graph_relationship_evidence
          WHERE source_kind IN ('source_inventory', 'app_inventory')",
        [],
    )?;
    conn.execute(
        "DELETE FROM graph_relationships
          WHERE reason_code IN (
              'inventory_node', 'inventory_service', 'compose_config',
              'reverse_proxy_config', 'docker_network', 'storage_probe',
              'config_artifact'
          )",
        [],
    )?;
    conn.execute(
        "DELETE FROM graph_entity_aliases
          WHERE source_kind IN ('source_inventory', 'app_inventory')",
        [],
    )?;
    conn.execute(
        "DELETE FROM graph_entities
          WHERE source_kind IN ('source_inventory', 'app_inventory')
            AND id NOT IN (SELECT src_entity_id FROM graph_relationships)
            AND id NOT IN (SELECT dst_entity_id FROM graph_relationships)",
        [],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn upsert_entity(
    conn: &Connection,
    entity_type: &'static str,
    canonical_key: &str,
    display_label: &str,
    source_kind: &str,
    source_id: &str,
    trust_level: &str,
    observed_at: &str,
) -> Result<EntityRef> {
    conn.execute(
        "INSERT INTO graph_entities
             (entity_type, canonical_key, display_label, source_kind, source_id,
              trust_level, first_seen_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(entity_type, canonical_key) DO UPDATE SET
             display_label = excluded.display_label,
             last_seen_at = excluded.last_seen_at,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![
            entity_type,
            canonical_key,
            display_label,
            source_kind,
            source_id,
            trust_level,
            observed_at
        ],
    )?;
    let id = conn.query_row(
        "SELECT id FROM graph_entities WHERE entity_type = ?1 AND canonical_key = ?2",
        params![entity_type, canonical_key],
        |row| row.get(0),
    )?;
    Ok(EntityRef {
        id,
        kind: entity_type,
        key: canonical_key.to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn add_alias(
    conn: &Connection,
    entity_id: i64,
    alias_type: &str,
    alias_key: &str,
    alias_value: &str,
    source_kind: &str,
    trust_level: &str,
    observed_at: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO graph_entity_aliases
             (entity_id, alias_type, alias_key, alias_value, source_kind,
              trust_level, first_seen_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(entity_id, alias_type, alias_key, source_kind) DO UPDATE SET
             alias_value = excluded.alias_value,
             trust_level = excluded.trust_level,
             last_seen_at = excluded.last_seen_at,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![
            entity_id,
            alias_type,
            alias_key,
            alias_value,
            source_kind,
            trust_level,
            observed_at
        ],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn add_relationship(
    conn: &Connection,
    src: &EntityRef,
    dst: &EntityRef,
    relationship_type: &str,
    reason_code: &str,
    source_kind: &str,
    source_id: &str,
    observed_at: &str,
    trust_level: &str,
    confidence: f64,
    safe_excerpt: &str,
) -> Result<()> {
    let key = format!(
        "{}:{}->{}:{}:{}",
        src.kind, src.key, dst.kind, dst.key, relationship_type
    );
    conn.execute(
        "INSERT INTO graph_relationships
             (relationship_key, src_entity_id, dst_entity_id, relationship_type,
              reason_code, trust_level, confidence, evidence_count, first_seen_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8)
         ON CONFLICT(src_entity_id, dst_entity_id, relationship_type, relationship_key)
         DO UPDATE SET
             reason_code = excluded.reason_code,
             trust_level = excluded.trust_level,
             confidence = MAX(graph_relationships.confidence, excluded.confidence),
             last_seen_at = excluded.last_seen_at,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![
            key,
            src.id,
            dst.id,
            relationship_type,
            reason_code,
            trust_level,
            confidence,
            observed_at
        ],
    )?;
    let rel_id: i64 = conn.query_row(
        "SELECT id FROM graph_relationships WHERE relationship_key = ?1",
        [&key],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO graph_relationship_evidence
             (relationship_id, evidence_key, source_kind, source_id, observed_at,
              reason_code, reason_text, confidence_delta, trust_level, safe_excerpt,
              evidence_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1)
         ON CONFLICT(relationship_id, evidence_key) DO UPDATE SET
             observed_at = excluded.observed_at,
             reason_text = excluded.reason_text,
             confidence_delta = excluded.confidence_delta,
             trust_level = excluded.trust_level,
             safe_excerpt = excluded.safe_excerpt,
             evidence_count = excluded.evidence_count",
        params![
            rel_id,
            format!(
                "{source_kind}:{source_id}:{reason_code}:{}:{}",
                src.key, dst.key
            ),
            source_kind,
            source_id,
            observed_at,
            reason_code,
            reason_code.replace('_', " "),
            confidence,
            trust_level,
            truncate_excerpt(safe_excerpt)
        ],
    )?;
    conn.execute(
        "UPDATE graph_relationships
            SET evidence_count = (
                SELECT COALESCE(SUM(evidence_count), 0)
                  FROM graph_relationship_evidence
                 WHERE relationship_id = ?1
            )
          WHERE id = ?1",
        [rel_id],
    )?;
    Ok(())
}

pub(super) fn update_projection_meta(
    conn: &Connection,
    counts: &InventoryGraphStats,
) -> Result<()> {
    conn.execute(
        "UPDATE graph_projection_meta
            SET entity_count = ?1,
                relationship_count = ?2,
                evidence_count = ?3,
                is_degraded = CASE
                    WHEN projection_status = 'failed' THEN is_degraded
                    ELSE 0
                END,
                last_error = CASE
                    WHEN projection_status = 'failed' THEN last_error
                    ELSE NULL
                END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = 1",
        params![
            counts.entity_count,
            counts.relationship_count,
            counts.evidence_count
        ],
    )?;
    Ok(())
}

pub(super) fn graph_counts(conn: &Connection) -> Result<InventoryGraphStats> {
    Ok(InventoryGraphStats {
        source_row_count: conn
            .query_row(
                "SELECT source_row_count FROM graph_projection_meta WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0),
        entity_count: table_count(conn, "graph_entities")?,
        relationship_count: table_count(conn, "graph_relationships")?,
        evidence_count: table_count(conn, "graph_relationship_evidence")?,
    })
}

pub(super) fn mark_projection_degraded(conn: &Connection, error: &str) -> Result<()> {
    conn.execute(
        "UPDATE graph_projection_meta
            SET is_degraded = 1,
                last_error = ?1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = 1",
        [truncate_excerpt(error)],
    )?;
    Ok(())
}

fn table_count(conn: &Connection, table: &str) -> Result<i64> {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .with_context(|| format!("count {table}"))
}

pub(super) fn scoped_inventory_key(source: &str, name: &str) -> String {
    let scope = source_host(source).unwrap_or("unknown");
    canonical_or_raw(&format!("{scope}:{name}"))
}

pub(super) fn safe_inventory_source_id(source: &str) -> String {
    match source_host(source) {
        Some(host) => format!(
            "{}:{}",
            source.split(':').next().unwrap_or("inventory").trim(),
            host
        ),
        None => source
            .split(':')
            .next()
            .filter(|collector| !collector.trim().is_empty())
            .unwrap_or("inventory")
            .to_string(),
    }
}

pub(super) fn canonical_or_raw(value: &str) -> String {
    canonical(value).unwrap_or_else(|| value.trim().to_ascii_lowercase())
}

pub(super) fn canonical(value: &str) -> Option<String> {
    graph::canonical_graph_key(value)
}

fn source_host(source: &str) -> Option<&str> {
    let mut parts = source.split(':');
    let _collector = parts.next()?;
    let host = parts.next()?.trim();
    if host.is_empty() || host.starts_with('/') {
        None
    } else {
        Some(host)
    }
}

pub(super) fn trust(value: &TrustLevel) -> &'static str {
    match value {
        TrustLevel::Verified | TrustLevel::Observed => graph::TRUST_VERIFIED,
        TrustLevel::Claimed => graph::TRUST_CLAIMED,
        TrustLevel::Inferred => graph::TRUST_INFERRED,
    }
}

fn truncate_excerpt(value: &str) -> String {
    const MAX: usize = 512;
    if value.len() <= MAX {
        return value.to_string();
    }
    value.chars().take(MAX).collect()
}
