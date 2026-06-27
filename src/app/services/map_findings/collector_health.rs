use super::super::super::*;
use super::super::{InventoryReadIssue, MapFindingContext};
use super::{bounded_evidence, sanitize_token};
use crate::app::topology_findings as finding_const;
use crate::app::topology_findings::reason as reason_const;
use crate::inventory::schema::CollectionError;
use std::collections::BTreeMap;

pub(in crate::app::services::map_findings) fn collector_health_findings(
    context: &MapFindingContext,
    evidence_limit: usize,
) -> Vec<TopologyFinding> {
    let mut findings = Vec::new();
    match context.inventory_read_issue {
        Some(InventoryReadIssue::NotFound) => {
            findings.push(collector_health_finding(
                reason_const::INVENTORY_CACHE_MISSING,
                finding_const::SEVERITY_MEDIUM,
                0.95,
                "inventory_cache",
                "Inventory cache is unavailable, so absence of topology findings is unknown.",
                evidence_limit,
            ));
        }
        Some(InventoryReadIssue::Unreadable) => {
            findings.push(collector_health_finding(
                reason_const::INVENTORY_CACHE_UNREADABLE,
                finding_const::SEVERITY_HIGH,
                0.95,
                "inventory_cache",
                "Inventory cache could not be read safely; absence-oriented topology findings are incomplete.",
                evidence_limit,
            ));
        }
        None => {}
    }

    if context.cache_status.is_stale {
        findings.push(collector_health_finding(
            reason_const::INVENTORY_CACHE_STALE,
            finding_const::SEVERITY_INFO,
            0.90,
            "inventory_cache",
            "Inventory cache is stale; findings are based on the last available normalized snapshot.",
            evidence_limit,
        ));
    }

    for warning in &context.cache_status.warnings {
        let reason_code = if warning.starts_with("collection-state unavailable") {
            reason_const::COLLECTION_STATE_UNAVAILABLE
        } else {
            reason_const::INVENTORY_CACHE_UNREADABLE
        };
        findings.push(collector_health_finding(
            reason_code,
            finding_const::SEVERITY_MEDIUM,
            0.90,
            "collection_state",
            "Inventory cache status reported a read warning; raw warning text is withheld.",
            evidence_limit,
        ));
    }

    if let Some(state) = &context.cache_status.collection_state {
        for collector in &state.collectors {
            if collector.status == "ok" && collector.warnings.is_empty() {
                continue;
            }
            let profile = collector_profile(&collector.name);
            findings.push(collector_health_finding(
                reason_const::COLLECTOR_DEGRADED,
                profile.degraded_severity,
                profile.degraded_confidence,
                &collector.name,
                "Collector did not complete cleanly; related topology conclusions have reduced confidence.",
                evidence_limit,
            ));
        }
    }

    if let Some(inventory) = &context.inventory {
        for error in &inventory.collection_errors {
            findings.push(collection_error_finding(error, evidence_limit));
        }
    }
    findings
}

fn collector_health_finding(
    reason_code: &str,
    severity: &str,
    confidence: f64,
    collector: &str,
    context: &str,
    evidence_limit: usize,
) -> TopologyFinding {
    let evidence = bounded_evidence(
        vec![TopologyFindingEvidence {
            evidence_id: None,
            source_kind: "collection_state".to_string(),
            safe_excerpt: Some(context.to_string()),
        }],
        evidence_limit,
    );
    TopologyFinding {
        finding_type: finding_const::TYPE_COLLECTOR_HEALTH.to_string(),
        severity: severity.to_string(),
        confidence,
        reason_code: reason_code.to_string(),
        affected_entities: vec![collector_entity(collector)],
        evidence: evidence.items,
        evidence_total: evidence.total,
        evidence_truncated: evidence.truncated,
        evidence_omitted: evidence.omitted,
        remediation: "Refresh inventory collection and inspect collector-specific logs before treating missing topology as absence.".to_string(),
        degraded_reason: Some(reason_code.to_string()),
        confidence_context: Some(context.to_string()),
    }
}

fn collection_error_finding(error: &CollectionError, evidence_limit: usize) -> TopologyFinding {
    let profile = collector_profile(&error.collector);
    let mut entity = collector_entity(&error.collector);
    entity
        .details
        .insert("phase".to_string(), sanitize_token(&error.phase));
    entity
        .details
        .insert("truncated".to_string(), error.truncated.to_string());
    let evidence = bounded_evidence(
        vec![TopologyFindingEvidence {
            evidence_id: None,
            source_kind: "collection_error".to_string(),
            safe_excerpt: Some(format!(
                "{} collector reported a {} during {}",
                sanitize_token(&error.collector),
                sanitize_token(&error.severity),
                sanitize_token(&error.phase)
            )),
        }],
        evidence_limit,
    );
    TopologyFinding {
        finding_type: finding_const::TYPE_COLLECTOR_HEALTH.to_string(),
        severity: profile.partial_severity.to_string(),
        confidence: profile.partial_confidence,
        reason_code: reason_const::COLLECTOR_PARTIAL.to_string(),
        affected_entities: vec![entity],
        evidence: evidence.items,
        evidence_total: evidence.total,
        evidence_truncated: evidence.truncated,
        evidence_omitted: evidence.omitted,
        remediation: "Refresh or repair the collector, then rebuild the graph projection before relying on absence-oriented topology conclusions.".to_string(),
        degraded_reason: Some(reason_const::COLLECTOR_PARTIAL.to_string()),
        confidence_context: Some("Raw warning text is intentionally withheld; collector class and phase are enough to scope follow-up.".to_string()),
    }
}

pub(in crate::app::services::map_findings) fn graph_projection_not_ready(
    graph_status: &db::graph::GraphProjectionStatus,
) -> bool {
    graph_status.projection_status != "ready" || graph_status.last_completed_at.is_none()
}

pub(in crate::app::services::map_findings) fn graph_projection_health_finding(
    graph_status: &db::graph::GraphProjectionStatus,
    evidence_limit: usize,
) -> Option<TopologyFinding> {
    if !graph_projection_not_ready(graph_status) {
        return None;
    }
    Some(collector_health_finding(
        reason_const::GRAPH_PROJECTION_NOT_READY,
        finding_const::SEVERITY_MEDIUM,
        0.98,
        "graph_projection",
        "Graph projection is not ready, so absence of topology findings is incomplete.",
        evidence_limit,
    ))
}

pub(in crate::app::services::map_findings) fn has_core_collector_degradation(
    context: &MapFindingContext,
) -> bool {
    let state_degraded = context
        .cache_status
        .collection_state
        .as_ref()
        .is_some_and(|state| {
            state.collectors.iter().any(|collector| {
                is_core_collector(&collector.name)
                    && (collector.status != "ok" || !collector.warnings.is_empty())
            })
        });
    let errors_degraded = context.inventory.as_ref().is_some_and(|inventory| {
        inventory
            .collection_errors
            .iter()
            .any(|error| is_core_collector(&error.collector))
    });
    state_degraded || errors_degraded
}

pub(in crate::app::services::map_findings) fn is_core_collector(name: &str) -> bool {
    matches!(
        name,
        "raw_configs" | "docker" | "remote_docker" | "reverse_proxy" | "compose"
    )
}

#[derive(Debug, Clone, Copy)]
struct CollectorProfile {
    degraded_severity: &'static str,
    partial_severity: &'static str,
    degraded_confidence: f64,
    partial_confidence: f64,
}

fn collector_profile(name: &str) -> CollectorProfile {
    if is_core_collector(name) {
        CollectorProfile {
            degraded_severity: finding_const::SEVERITY_MEDIUM,
            partial_severity: finding_const::SEVERITY_MEDIUM,
            degraded_confidence: 0.85,
            partial_confidence: 0.85,
        }
    } else {
        CollectorProfile {
            degraded_severity: finding_const::SEVERITY_INFO,
            partial_severity: finding_const::SEVERITY_INFO,
            degraded_confidence: 0.70,
            partial_confidence: 0.65,
        }
    }
}

fn collector_entity(collector: &str) -> TopologyFindingEntity {
    let mut details = BTreeMap::new();
    details.insert("collector".to_string(), sanitize_token(collector));
    TopologyFindingEntity {
        entity_type: "collector".to_string(),
        key: sanitize_token(collector),
        label: sanitize_token(collector),
        details,
    }
}
