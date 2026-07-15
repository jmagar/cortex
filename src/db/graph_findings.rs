#[cfg(test)]
#[path = "graph_findings_tests.rs"]
mod tests;

use anyhow::Result;
use rusqlite::params;

use crate::db::DbPool;
use crate::db::graph;

#[derive(Debug, Clone, PartialEq)]
pub struct PublicRouteFindingRow {
    pub domain_key: String,
    pub domain_label: String,
    pub proxy_key: String,
    pub proxy_label: String,
    pub service_key: Option<String>,
    pub service_label: Option<String>,
    pub exposes_confidence: f64,
    pub routes_confidence: Option<f64>,
    pub exposes_evidence_id: Option<i64>,
    pub exposes_excerpt: Option<String>,
    pub routes_evidence_id: Option<i64>,
    pub routes_excerpt: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MountRelationshipFindingRow {
    pub service_key: String,
    pub service_label: String,
    pub storage_key: String,
    pub storage_label: String,
    pub confidence: f64,
    pub evidence_id: Option<i64>,
    pub safe_excerpt: Option<String>,
}

pub fn list_public_route_findings(pool: &DbPool, limit: u32) -> Result<Vec<PublicRouteFindingRow>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT
             domain.canonical_key,
             domain.display_label,
             proxy.canonical_key,
             proxy.display_label,
             service.canonical_key,
             service.display_label,
             exposes.confidence,
             routes.confidence,
             exposes_ev.id,
             exposes_ev.safe_excerpt,
             routes_ev.id,
             routes_ev.safe_excerpt
         FROM graph_relationships exposes INDEXED BY idx_graph_relationships_type_seen
         JOIN graph_entities proxy ON proxy.id = exposes.src_entity_id
         JOIN graph_entities domain ON domain.id = exposes.dst_entity_id
         LEFT JOIN graph_relationship_evidence exposes_ev
           ON exposes_ev.id = (
               SELECT id
                 FROM graph_relationship_evidence
                WHERE relationship_id = exposes.id
                ORDER BY observed_at DESC, id DESC
                LIMIT 1
           )
         LEFT JOIN graph_relationships routes
           ON routes.src_entity_id = proxy.id
          AND routes.relationship_type = ?2
         LEFT JOIN graph_entities service ON service.id = routes.dst_entity_id
         LEFT JOIN graph_relationship_evidence routes_ev
           ON routes_ev.id = (
               SELECT id
                 FROM graph_relationship_evidence
                WHERE relationship_id = routes.id
                ORDER BY observed_at DESC, id DESC
                LIMIT 1
           )
        WHERE exposes.relationship_type = ?1
          AND proxy.entity_type = ?3
          AND domain.entity_type = ?4
        ORDER BY exposes.confidence DESC, exposes.last_seen_at DESC, exposes.id DESC
        LIMIT ?5",
    )?;
    let rows = stmt
        .query_map(
            params![
                graph::REL_EXPOSES_DOMAIN,
                graph::REL_ROUTES_TO,
                graph::ENTITY_TYPE_REVERSE_PROXY,
                graph::ENTITY_TYPE_DOMAIN,
                i64::from(limit),
            ],
            |row| {
                Ok(PublicRouteFindingRow {
                    domain_key: row.get(0)?,
                    domain_label: row.get(1)?,
                    proxy_key: row.get(2)?,
                    proxy_label: row.get(3)?,
                    service_key: row.get(4)?,
                    service_label: row.get(5)?,
                    exposes_confidence: row.get(6)?,
                    routes_confidence: row.get(7)?,
                    exposes_evidence_id: row.get(8)?,
                    exposes_excerpt: row.get(9)?,
                    routes_evidence_id: row.get(10)?,
                    routes_excerpt: row.get(11)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list_mount_relationship_findings(
    pool: &DbPool,
    limit: u32,
) -> Result<Vec<MountRelationshipFindingRow>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT
             service.canonical_key,
             service.display_label,
             storage.canonical_key,
             storage.display_label,
             mounts.confidence,
             evidence.id,
             evidence.safe_excerpt
         FROM graph_relationships mounts INDEXED BY idx_graph_relationships_type_seen
         JOIN graph_entities service ON service.id = mounts.src_entity_id
         JOIN graph_entities storage ON storage.id = mounts.dst_entity_id
         LEFT JOIN graph_relationship_evidence evidence
           ON evidence.id = (
               SELECT id
                 FROM graph_relationship_evidence
                WHERE relationship_id = mounts.id
                ORDER BY observed_at DESC, id DESC
                LIMIT 1
           )
        WHERE mounts.relationship_type = ?1
          AND service.entity_type = ?2
          AND storage.entity_type = ?3
        ORDER BY mounts.confidence DESC, mounts.last_seen_at DESC, mounts.id DESC
        LIMIT ?4",
    )?;
    let rows = stmt
        .query_map(
            params![
                graph::REL_MOUNTS,
                graph::ENTITY_TYPE_SERVICE_INSTANCE,
                graph::ENTITY_TYPE_STORAGE,
                i64::from(limit),
            ],
            |row| {
                Ok(MountRelationshipFindingRow {
                    service_key: row.get(0)?,
                    service_label: row.get(1)?,
                    storage_key: row.get(2)?,
                    storage_label: row.get(3)?,
                    confidence: row.get(4)?,
                    evidence_id: row.get(5)?,
                    safe_excerpt: row.get(6)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
pub(crate) fn relationship_type_query_plan(
    pool: &DbPool,
    relationship_type: &str,
) -> Result<Vec<String>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "EXPLAIN QUERY PLAN
         SELECT id
           FROM graph_relationships INDEXED BY idx_graph_relationships_type_seen
          WHERE relationship_type = ?1
          ORDER BY last_seen_at DESC
          LIMIT 10",
    )?;
    let rows = stmt
        .query_map([relationship_type], |row| row.get::<_, String>(3))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
