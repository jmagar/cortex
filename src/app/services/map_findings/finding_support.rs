use super::super::graph_safety::*;
use super::super::*;
use super::{InventoryReadIssue, MapFindingContext};
use crate::app::topology_findings as finding_const;
use crate::app::topology_findings::reason as reason_const;
use crate::inventory::{
    InventoryConfig, inventory_status, is_not_found_error, read_inventory_cache,
};
use std::collections::{BTreeMap, HashSet};

#[path = "collector_health.rs"]
mod collector_health;
#[path = "risky_mounts.rs"]
mod risky_mounts;

pub(super) use collector_health::{
    collector_health_findings, graph_projection_health_finding, graph_projection_not_ready,
    has_core_collector_degradation,
};
pub(super) use risky_mounts::risky_mount_findings;
#[cfg(test)]
pub(super) use risky_mounts::{classify_mount, safe_mount_target};
pub(in crate::app::services::map_findings) async fn read_map_finding_context()
-> ServiceResult<MapFindingContext> {
    let config = InventoryConfig::from_env();
    tokio::task::spawn_blocking(move || {
        let cache_status = inventory_status(&config);
        let (inventory, inventory_read_issue) = match read_inventory_cache(&config) {
            Ok(inventory) => (Some(inventory), None),
            Err(error) if is_not_found_error(&error) => (None, Some(InventoryReadIssue::NotFound)),
            Err(_) => (None, Some(InventoryReadIssue::Unreadable)),
        };
        MapFindingContext {
            inventory,
            inventory_read_issue,
            cache_status,
        }
    })
    .await
    .map_err(|e| ServiceError::Internal(anyhow::anyhow!("inventory findings read panicked: {e}")))
}

pub(in crate::app::services::map_findings) fn public_route_findings(
    rows: Vec<db::PublicRouteFindingRow>,
    evidence_limit: usize,
    context: &MapFindingContext,
    graph_status: &db::graph::GraphProjectionStatus,
) -> Vec<TopologyFinding> {
    let degraded = route_confidence_context(context, graph_status);
    rows.into_iter()
        .map(|row| {
            let mut affected = vec![
                entity("domain", &row.domain_key, &row.domain_label),
                entity("reverse_proxy", &row.proxy_key, &row.proxy_label),
            ];
            if let (Some(key), Some(label)) = (&row.service_key, &row.service_label) {
                affected.push(entity("service", key, label));
            }
            let has_route_target = row.service_key.is_some();
            let confidence = route_confidence(&row, degraded.is_some());
            let evidence = public_route_evidence(&row, evidence_limit);
            TopologyFinding {
                finding_type: finding_const::TYPE_POTENTIAL_PUBLIC_ROUTE.to_string(),
                severity: if has_route_target {
                    finding_const::SEVERITY_MEDIUM
                } else {
                    finding_const::SEVERITY_LOW
                }
                .to_string(),
                confidence,
                reason_code: if has_route_target {
                    reason_const::REVERSE_PROXY_ROUTE_CONFIGURED
                } else {
                    reason_const::REVERSE_PROXY_DOMAIN_WITHOUT_TARGET_PROOF
                }
                .to_string(),
                affected_entities: affected,
                evidence: evidence.items,
                evidence_total: evidence.total,
                evidence_truncated: evidence.truncated,
                evidence_omitted: evidence.omitted,
                remediation: "Review whether the routed domain should remain reachable through the reverse proxy, and verify proxy authentication/TLS policy separately from topology data.".to_string(),
                degraded_reason: degraded.clone(),
                confidence_context: degraded.clone().map(|reason| {
                    format!("Confidence reduced because {reason}; this finding proves configured routing, not unauthenticated internet exposure.")
                }).or_else(|| Some("Reverse-proxy graph proof indicates a configured route; listener and perimeter exposure require separate validation.".to_string())),
            }
        })
        .collect()
}

pub(super) fn route_confidence(row: &db::PublicRouteFindingRow, degraded: bool) -> f64 {
    let mut confidence = row
        .routes_confidence
        .map(|route| row.exposes_confidence.min(route))
        .unwrap_or(row.exposes_confidence * 0.75);
    if degraded {
        confidence *= 0.8;
    }
    confidence.clamp(0.0, 1.0)
}

fn route_confidence_context(
    context: &MapFindingContext,
    graph_status: &db::graph::GraphProjectionStatus,
) -> Option<String> {
    if graph_projection_not_ready(graph_status) {
        Some(reason_const::GRAPH_PROJECTION_NOT_READY.to_string())
    } else if graph_status.is_degraded {
        Some("graph_projection_degraded".to_string())
    } else if context.cache_status.is_stale {
        Some(reason_const::INVENTORY_CACHE_STALE.to_string())
    } else if context.inventory_read_issue.is_some() {
        Some("inventory_context_unavailable".to_string())
    } else if has_core_collector_degradation(context) {
        Some("core_collector_partial".to_string())
    } else {
        None
    }
}

fn public_route_evidence(
    row: &db::PublicRouteFindingRow,
    evidence_limit: usize,
) -> BoundedEvidence {
    bounded_evidence(
        vec![
            TopologyFindingEvidence {
                evidence_id: row.exposes_evidence_id,
                source_kind: "app_inventory".to_string(),
                safe_excerpt: row.exposes_excerpt.clone().map(redact_graph_text),
            },
            TopologyFindingEvidence {
                evidence_id: row.routes_evidence_id,
                source_kind: "app_inventory".to_string(),
                safe_excerpt: row.routes_excerpt.clone().map(redact_graph_text),
            },
        ],
        evidence_limit,
    )
}

fn graph_row_evidence(
    evidence_id: Option<i64>,
    safe_excerpt: Option<String>,
    evidence_limit: usize,
) -> BoundedEvidence {
    bounded_evidence(
        vec![TopologyFindingEvidence {
            evidence_id,
            source_kind: "app_inventory".to_string(),
            safe_excerpt: safe_excerpt.map(redact_graph_text),
        }],
        evidence_limit,
    )
}

fn bounded_evidence(evidence: Vec<TopologyFindingEvidence>, limit: usize) -> BoundedEvidence {
    let candidates = evidence
        .into_iter()
        .filter(|item| item.evidence_id.is_some() || item.safe_excerpt.is_some())
        .collect::<Vec<_>>();
    let total = candidates.len();
    let items = candidates.into_iter().take(limit).collect::<Vec<_>>();
    let omitted = total.saturating_sub(items.len());
    BoundedEvidence {
        items,
        total,
        truncated: omitted > 0,
        omitted,
    }
}

#[derive(Debug)]
struct BoundedEvidence {
    items: Vec<TopologyFindingEvidence>,
    total: usize,
    truncated: bool,
    omitted: usize,
}

pub(in crate::app::services::map_findings) fn apply_findings_payload_budget(
    findings: &mut Vec<TopologyFinding>,
    payload_budget: u32,
) -> bool {
    let budget = payload_budget as usize;
    let mut truncated = false;
    while findings_payload_size(findings) > budget {
        if let Some(finding) = findings
            .iter_mut()
            .rev()
            .find(|finding| !finding.evidence.is_empty())
        {
            finding.evidence.clear();
            finding.evidence_truncated = finding.evidence_total > 0;
            finding.evidence_omitted = finding.evidence_total;
            truncated = true;
            continue;
        }
        if findings.pop().is_some() {
            truncated = true;
        } else {
            break;
        }
    }
    truncated
}

fn findings_payload_size(findings: &[TopologyFinding]) -> usize {
    serde_json::to_vec(findings)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}

fn entity(entity_type: &str, key: &str, label: &str) -> TopologyFindingEntity {
    TopologyFindingEntity {
        entity_type: entity_type.to_string(),
        key: key.to_string(),
        label: label.to_string(),
        details: BTreeMap::new(),
    }
}

pub(in crate::app::services::map_findings) fn finding_next_queries(
    findings: &[TopologyFinding],
) -> Vec<HomelabMapNextQuery> {
    let mut seen = HashSet::new();
    findings
        .iter()
        .flat_map(|finding| &finding.affected_entities)
        .filter_map(|entity| match entity.entity_type.as_str() {
            "domain" => Some(HomelabMapNextQuery {
                action: "map".to_string(),
                mode: "domain_routes".to_string(),
                reason: "Inspect route proof for this finding domain".to_string(),
                host: None,
                domain: Some(entity.key.clone()),
                service: None,
            }),
            "service" => Some(HomelabMapNextQuery {
                action: "map".to_string(),
                mode: "service_dependencies".to_string(),
                reason: "Inspect service dependencies for this finding service".to_string(),
                host: None,
                domain: None,
                service: Some(entity.key.clone()),
            }),
            _ => None,
        })
        .filter(|query| {
            seen.insert(format!(
                "{}:{:?}:{:?}",
                query.mode, query.domain, query.service
            ))
        })
        .take(10)
        .collect()
}

pub(in crate::app::services::map_findings) fn finding_proof_queries(
    findings: &[TopologyFinding],
) -> Vec<HomelabMapProofQuery> {
    let mut seen = HashSet::new();
    findings
        .iter()
        .flat_map(|finding| &finding.evidence)
        .filter_map(|evidence| evidence.evidence_id)
        .filter(|id| seen.insert(*id))
        .take(10)
        .map(|id| HomelabMapProofQuery {
            action: "graph".to_string(),
            mode: "evidence".to_string(),
            label: "Inspect finding evidence".to_string(),
            entity_id: None,
            evidence_id: Some(id),
        })
        .collect()
}

pub(in crate::app::services::map_findings) fn finding_sort_key(
    finding: &TopologyFinding,
) -> (u8, std::cmp::Reverse<i64>, String) {
    (
        severity_rank(&finding.severity),
        std::cmp::Reverse((finding.confidence * 1000.0) as i64),
        finding.finding_type.clone(),
    )
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        finding_const::SEVERITY_CRITICAL => 0,
        finding_const::SEVERITY_HIGH => 1,
        finding_const::SEVERITY_MEDIUM => 2,
        finding_const::SEVERITY_LOW => 3,
        finding_const::SEVERITY_INFO => 4,
        _ => 5,
    }
}

fn sanitize_token(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

pub(in crate::app::services::map_findings) struct RequestedFindingTypes {
    values: HashSet<String>,
}

impl RequestedFindingTypes {
    pub(in crate::app::services::map_findings) fn new(
        values: Option<&[String]>,
    ) -> ServiceResult<Self> {
        let values = values
            .map(|values| {
                values
                    .iter()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|values| !values.is_empty())
            .unwrap_or_else(|| {
                finding_const::TYPES
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect()
            });
        for value in &values {
            if !finding_const::TYPES.contains(&value.as_str()) {
                return Err(ServiceError::InvalidInput(format!(
                    "unsupported finding type `{value}`; expected potential_public_route, risky_mounts, or collector_health"
                )));
            }
        }
        Ok(Self {
            values: values.into_iter().collect(),
        })
    }

    pub(in crate::app::services::map_findings) fn includes(&self, value: &str) -> bool {
        self.values.contains(value)
    }
}
