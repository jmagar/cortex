use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::db::graph;
use crate::inventory::schema::{
    ArtifactRef, HomelabInventory, InventoryService, StorageSummary, TrustLevel,
};

use super::InventoryGraphStats;

#[derive(Debug, Clone)]
pub(super) struct EntityRef {
    pub(super) id: i64,
    pub(super) kind: &'static str,
    pub(super) key: String,
}

pub(super) fn prune_previous_inventory_projection(conn: &Connection) -> Result<()> {
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

pub(super) fn upsert_service(conn: &Connection, service: &InventoryService) -> Result<EntityRef> {
    upsert_entity(
        conn,
        graph::ENTITY_TYPE_SERVICE,
        &service_key(service),
        &service.name,
        graph::SOURCE_KIND_APP_INVENTORY,
        &service.id,
        trust(&service.trust_level),
        &service.provenance.collected_at,
    )
}

pub(super) fn upsert_artifact(
    conn: &Connection,
    artifact: &ArtifactRef,
    observed_at: &str,
) -> Result<EntityRef> {
    let display = artifact
        .source_path
        .as_deref()
        .unwrap_or(artifact.cache_path.as_str());
    let entity = upsert_entity(
        conn,
        graph::ENTITY_TYPE_CONFIG_ARTIFACT,
        &canonical_or_raw(&artifact.id),
        display,
        graph::SOURCE_KIND_APP_INVENTORY,
        &artifact.id,
        graph::TRUST_VERIFIED,
        observed_at,
    )?;
    if let Some(path) = &artifact.source_path {
        add_alias(
            conn,
            entity.id,
            "path",
            &canonical_or_raw(path),
            path,
            graph::SOURCE_KIND_APP_INVENTORY,
            graph::TRUST_VERIFIED,
            observed_at,
        )?;
    }
    Ok(entity)
}

pub(super) fn upsert_storage(
    conn: &Connection,
    storage: &StorageSummary,
    hosts: &BTreeMap<String, EntityRef>,
) -> Result<()> {
    let entity = upsert_entity(
        conn,
        graph::ENTITY_TYPE_STORAGE,
        &canonical_or_raw(&storage.id),
        &storage.mount,
        graph::SOURCE_KIND_SOURCE_INVENTORY,
        &storage.id,
        graph::TRUST_VERIFIED,
        &storage.provenance.collected_at,
    )?;
    if let Some(host) = storage
        .id
        .split(':')
        .nth(1)
        .and_then(|host| hosts.get(&canonical_or_raw(host)))
    {
        add_relationship(
            conn,
            host,
            &entity,
            graph::REL_BACKED_BY,
            graph::REASON_STORAGE_PROBE,
            graph::SOURCE_KIND_SOURCE_INVENTORY,
            &storage.id,
            &storage.provenance.collected_at,
            graph::TRUST_VERIFIED,
            0.75,
            &format!("{} storage mounted at {}", host.key, storage.mount),
        )?;
    }
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
             source_kind = excluded.source_kind,
             source_id = excluded.source_id,
             trust_level = excluded.trust_level,
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
    inventory: &HomelabInventory,
    counts: &InventoryGraphStats,
) -> Result<()> {
    conn.execute(
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
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = 1",
        params![
            format!("inventory:{}", inventory.run_id),
            inventory.summary.nodes as i64
                + inventory.summary.services as i64
                + inventory.summary.compose_projects as i64
                + inventory.summary.reverse_proxies as i64
                + inventory.summary.networks as i64
                + inventory.summary.storage as i64
                + inventory.summary.artifacts as i64,
            counts.entity_count,
            counts.relationship_count,
            counts.evidence_count
        ],
    )?;
    Ok(())
}

pub(super) fn graph_counts(conn: &Connection) -> Result<InventoryGraphStats> {
    Ok(InventoryGraphStats {
        source_row_count: 0,
        entity_count: table_count(conn, "graph_entities")?,
        relationship_count: table_count(conn, "graph_relationships")?,
        evidence_count: table_count(conn, "graph_relationship_evidence")?,
    })
}

fn table_count(conn: &Connection, table: &str) -> Result<i64> {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .with_context(|| format!("count {table}"))
}

pub(super) fn match_upstream<'a>(
    upstream: &str,
    services: &'a BTreeMap<String, EntityRef>,
) -> Option<&'a EntityRef> {
    let normalized = canonical_or_raw(upstream);
    let prefix = upstream
        .split([':', '/', '@'])
        .find(|part| !part.is_empty() && !part.starts_with("http"))
        .map(canonical_or_raw);
    prefix
        .and_then(|key| services.get(&key))
        .or_else(|| services.get(&normalized))
}

pub(super) fn service_key(service: &InventoryService) -> String {
    canonical_or_raw(&format!(
        "{}:{}",
        service.host.as_deref().unwrap_or("unknown"),
        service.name
    ))
}

pub(super) fn canonical_or_raw(value: &str) -> String {
    canonical(value).unwrap_or_else(|| value.trim().to_ascii_lowercase())
}

pub(super) fn canonical(value: &str) -> Option<String> {
    graph::canonical_graph_key(value)
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
