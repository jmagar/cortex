mod finding_support;
use super::graph_safety::redact_graph_text;
use super::*;
use crate::app::topology_findings as finding_const;
use crate::app::topology_findings::reason as reason_const;
use crate::inventory::{InventoryCacheStatus, schema::HomelabInventory};

#[derive(Debug, Clone)]
struct MapFindingContext {
    inventory: Option<HomelabInventory>,
    inventory_read_issue: Option<InventoryReadIssue>,
    cache_status: InventoryCacheStatus,
}

#[derive(Debug, Clone)]
enum InventoryReadIssue {
    NotFound,
    Unreadable,
}

#[derive(Debug, Clone, Copy)]
struct FindingLimits {
    finding_limit: u32,
    evidence_limit: usize,
    payload_budget: u32,
}

impl FindingLimits {
    fn from_request(req: &HomelabMapRequest) -> Self {
        Self {
            finding_limit: req.finding_limit.unwrap_or(25).clamp(1, 100),
            evidence_limit: req.evidence_per_finding.unwrap_or(2).clamp(1, 5) as usize,
            payload_budget: req.payload_budget.unwrap_or(32_768).clamp(4_096, 65_536),
        }
    }

    fn db_limit(self) -> u32 {
        self.finding_limit.saturating_mul(4).clamp(1, 400)
    }

    fn evidence_sample_limit(self) -> u32 {
        self.evidence_limit as u32
    }
}

impl CortexService {
    pub(super) async fn homelab_map_findings_answer(
        &self,
        req: &HomelabMapRequest,
    ) -> ServiceResult<HomelabMapGraphAnswer> {
        let limits = FindingLimits::from_request(req);
        let requested = RequestedFindingTypes::new(req.finding_types.as_deref())?;
        let context = read_map_finding_context().await?;

        let db_limit = limits.db_limit();
        let (public_routes, mount_relationships, graph_status) = self
            .run_db("homelab_map_findings", move |pool| {
                Ok((
                    db::list_public_route_findings(pool, db_limit)?,
                    db::list_mount_relationship_findings(pool, db_limit)?,
                    db::graph::graph_projection_status(pool)?,
                ))
            })
            .await?;

        let mut findings = Vec::new();
        if let Some(finding) = graph_projection_health_finding(&graph_status, limits.evidence_limit)
        {
            findings.push(finding);
        }
        if requested.includes(finding_const::TYPE_POTENTIAL_PUBLIC_ROUTE) {
            findings.extend(public_route_findings(
                public_routes,
                limits.evidence_limit,
                &context,
                &graph_status,
            ));
        }
        if requested.includes(finding_const::TYPE_RISKY_MOUNTS) {
            findings.extend(risky_mount_findings(
                mount_relationships,
                limits.evidence_limit,
                &context,
                &graph_status,
            ));
        }
        if requested.includes(finding_const::TYPE_COLLECTOR_HEALTH) {
            findings.extend(collector_health_findings(&context, limits.evidence_limit));
        }
        findings.sort_by_key(finding_sort_key);
        let mut truncated = findings.len() > limits.finding_limit as usize;
        if truncated {
            findings.truncate(limits.finding_limit as usize);
        }
        let mut truncated_reason = truncated.then(|| "finding_limit".to_string());
        if apply_findings_payload_budget(&mut findings, limits.payload_budget) {
            truncated = true;
            truncated_reason = Some("payload_budget".to_string());
        }

        let projection_not_ready = graph_projection_not_ready(&graph_status);
        let context_degraded = projection_not_ready
            || graph_status.is_degraded
            || context.cache_status.is_stale
            || context.inventory_read_issue.is_some()
            || has_core_collector_degradation(&context);

        let metadata = GraphResponseMetadata {
            projection_status: graph_status.projection_status,
            last_completed_at: graph_status.last_completed_at,
            source_watermark: graph_status.source_watermark,
            is_degraded: context_degraded,
            last_error: graph_status.last_error.map(redact_graph_text),
            limit: limits.finding_limit,
            depth: 1,
            truncated,
            truncated_reason: truncated_reason.clone(),
            evidence_sample_limit: limits.evidence_sample_limit(),
            payload_budget: limits.payload_budget,
        };

        let answer_status = if metadata.is_degraded {
            "degraded"
        } else {
            "ok"
        }
        .to_string();
        let degraded_reason = metadata.is_degraded.then(|| {
            if projection_not_ready {
                reason_const::GRAPH_PROJECTION_NOT_READY.to_string()
            } else if context.inventory_read_issue.is_some() {
                match context.inventory_read_issue {
                    Some(InventoryReadIssue::NotFound) => {
                        reason_const::INVENTORY_CACHE_MISSING.to_string()
                    }
                    Some(InventoryReadIssue::Unreadable) => {
                        reason_const::INVENTORY_CACHE_UNREADABLE.to_string()
                    }
                    None => "inventory_context_unavailable".to_string(),
                }
            } else if context.cache_status.is_stale {
                reason_const::INVENTORY_CACHE_STALE.to_string()
            } else {
                "graph_degraded".to_string()
            }
        });
        Ok(HomelabMapGraphAnswer {
            mode: "findings".to_string(),
            answer_status,
            target: HomelabMapGraphTarget {
                entity_type: "topology".to_string(),
                key: "findings".to_string(),
            },
            rows: Vec::new(),
            candidates: Vec::new(),
            evidence: Vec::new(),
            metadata,
            truncation: HomelabMapAnswerTruncation {
                truncated,
                reason: truncated_reason,
                limit: limits.finding_limit,
                evidence_sample_limit: limits.evidence_sample_limit(),
                payload_budget: limits.payload_budget,
            },
            degraded_reason,
            next_queries: finding_next_queries(&findings),
            proof_queries: finding_proof_queries(&findings),
            findings,
        })
    }
}

#[cfg(test)]
use crate::inventory::schema::MountRef;
use finding_support::{
    RequestedFindingTypes, apply_findings_payload_budget, collector_health_findings,
    finding_next_queries, finding_proof_queries, finding_sort_key, graph_projection_health_finding,
    graph_projection_not_ready, has_core_collector_degradation, public_route_findings,
    read_map_finding_context, risky_mount_findings,
};
#[cfg(test)]
use finding_support::{classify_mount, safe_mount_target};

#[cfg(test)]
#[path = "map_findings_tests.rs"]
mod tests;
