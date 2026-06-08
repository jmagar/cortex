use super::graph_safety::*;
use super::*;
use crate::app::topology_findings as finding_const;
use crate::app::topology_findings::reason as reason_const;
use crate::inventory::schema::{CollectionError, HomelabInventory, InventoryService, MountRef};
use crate::inventory::{
    inventory_status, is_not_found_error, read_inventory_cache, InventoryCacheStatus,
    InventoryConfig,
};
use std::collections::{BTreeMap, HashSet};

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

async fn read_map_finding_context() -> ServiceResult<MapFindingContext> {
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

fn public_route_findings(
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

fn risky_mount_findings(
    rows: Vec<db::MountRelationshipFindingRow>,
    evidence_limit: usize,
    context: &MapFindingContext,
    graph_status: &db::graph::GraphProjectionStatus,
) -> Vec<TopologyFinding> {
    let services = context
        .inventory
        .as_ref()
        .map(service_mount_index)
        .unwrap_or_default();
    let mut findings = Vec::new();
    for row in rows {
        let Some(mounts) = services.get(&row.service_key) else {
            continue;
        };
        for (service, mount) in mounts {
            if !mount_target_matches(&row, mount) {
                continue;
            }
            if let Some(risk) = classify_mount(mount) {
                findings.push(risky_mount_finding(
                    &row,
                    service,
                    mount,
                    risk,
                    graph_status,
                    evidence_limit,
                ));
            }
        }
    }
    findings
}

fn risky_mount_finding(
    row: &db::MountRelationshipFindingRow,
    service: &InventoryService,
    mount: &MountRef,
    risk: MountRisk,
    graph_status: &db::graph::GraphProjectionStatus,
    evidence_limit: usize,
) -> TopologyFinding {
    let mut service_entity = entity("service", &row.service_key, &row.service_label);
    service_entity
        .details
        .insert("kind".to_string(), service.kind.clone());
    if let Some(host) = &service.host {
        service_entity
            .details
            .insert("host".to_string(), host.clone());
    }
    let mut mount_entity = entity("storage", &row.storage_key, &row.storage_label);
    mount_entity.details.insert(
        "mount_source_kind".to_string(),
        risk.source_kind.to_string(),
    );
    mount_entity
        .details
        .insert("mount_target".to_string(), safe_mount_target(&mount.target));
    mount_entity
        .details
        .insert("read_only".to_string(), mount.read_only.to_string());
    let evidence = graph_row_evidence(row.evidence_id, row.safe_excerpt.clone(), evidence_limit);
    TopologyFinding {
        finding_type: finding_const::TYPE_RISKY_MOUNTS.to_string(),
        severity: risk.severity.to_string(),
        confidence: mount_confidence(row.confidence, mount.read_only, graph_status.is_degraded),
        reason_code: risk.reason_code.to_string(),
        affected_entities: vec![service_entity, mount_entity],
        evidence: evidence.items,
        evidence_total: evidence.total,
        evidence_truncated: evidence.truncated,
        evidence_omitted: evidence.omitted,
        remediation: risk.remediation.to_string(),
        degraded_reason: graph_status
            .is_degraded
            .then(|| "graph_projection_degraded".to_string()),
        confidence_context: Some("Mount source details come from normalized inventory; graph mount relationships provide positive service-to-storage proof.".to_string()),
    }
}

fn collector_health_findings(
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

fn service_mount_index(
    inventory: &HomelabInventory,
) -> BTreeMap<String, Vec<(&InventoryService, &MountRef)>> {
    let mut services = BTreeMap::new();
    for service in &inventory.services {
        let Some(host) = service.host.as_deref() else {
            continue;
        };
        let key = canonical_service_key(host, &service.name);
        services.insert(
            key,
            service
                .mounts
                .iter()
                .map(|mount| (service, mount))
                .collect::<Vec<_>>(),
        );
    }
    services
}

#[derive(Debug, Clone, Copy)]
struct MountRisk {
    severity: &'static str,
    reason_code: &'static str,
    source_kind: &'static str,
    remediation: &'static str,
}

fn classify_mount(mount: &MountRef) -> Option<MountRisk> {
    let source = mount.source.as_deref().unwrap_or_default();
    let target = mount.target.as_str();
    if source == "/var/run/docker.sock" || target == "/var/run/docker.sock" {
        return Some(MountRisk {
            severity: if mount.read_only {
                finding_const::SEVERITY_MEDIUM
            } else {
                finding_const::SEVERITY_HIGH
            },
            reason_code: reason_const::DOCKER_SOCKET_MOUNT,
            source_kind: "docker_socket",
            remediation: "Review whether this service needs Docker daemon access; prefer a narrow proxy or read-only control path when possible.",
        });
    }
    if source == "/" {
        return Some(MountRisk {
            severity: if mount.read_only {
                finding_const::SEVERITY_MEDIUM
            } else {
                finding_const::SEVERITY_HIGH
            },
            reason_code: reason_const::HOST_ROOT_MOUNT,
            source_kind: "host_root",
            remediation: "Replace host-root binds with the smallest required host path and keep the mount read-only when writes are not required.",
        });
    }
    if source.contains("/appdata")
        || target.contains("/appdata")
        || source.starts_with("/mnt/user/appdata")
    {
        return Some(MountRisk {
            severity: if mount.read_only {
                finding_const::SEVERITY_LOW
            } else {
                finding_const::SEVERITY_MEDIUM
            },
            reason_code: reason_const::APPDATA_ROOT_MOUNT,
            source_kind: "appdata_root",
            remediation: "Scope appdata binds to the service-specific directory and keep secrets outside broadly shared mounts.",
        });
    }
    if mount.source.is_none() {
        return Some(MountRisk {
            severity: finding_const::SEVERITY_LOW,
            reason_code: reason_const::MOUNT_MISSING_SOURCE_DETAIL,
            source_kind: "unknown_source",
            remediation: "Refresh Docker or compose inventory so mount source details can be classified before making risk decisions.",
        });
    }
    None
}

fn mount_target_matches(row: &db::MountRelationshipFindingRow, mount: &MountRef) -> bool {
    row.storage_label == mount.target
        || row
            .storage_key
            .ends_with(&canonical_component(&mount.target))
}

fn route_confidence(row: &db::PublicRouteFindingRow, degraded: bool) -> f64 {
    let mut confidence = row
        .routes_confidence
        .map(|route| row.exposes_confidence.min(route))
        .unwrap_or(row.exposes_confidence * 0.75);
    if degraded {
        confidence *= 0.8;
    }
    confidence.clamp(0.0, 1.0)
}

fn mount_confidence(confidence: f64, read_only: bool, graph_degraded: bool) -> f64 {
    let mut value = confidence.max(0.70);
    if read_only {
        value *= 0.9;
    }
    if graph_degraded {
        value *= 0.8;
    }
    value.clamp(0.0, 1.0)
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

fn graph_projection_not_ready(graph_status: &db::graph::GraphProjectionStatus) -> bool {
    graph_status.projection_status != "ready" || graph_status.last_completed_at.is_none()
}

fn graph_projection_health_finding(
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

fn has_core_collector_degradation(context: &MapFindingContext) -> bool {
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

fn apply_findings_payload_budget(findings: &mut Vec<TopologyFinding>, payload_budget: u32) -> bool {
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

fn finding_next_queries(findings: &[TopologyFinding]) -> Vec<HomelabMapNextQuery> {
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

fn finding_proof_queries(findings: &[TopologyFinding]) -> Vec<HomelabMapProofQuery> {
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

fn finding_sort_key(finding: &TopologyFinding) -> (u8, std::cmp::Reverse<i64>, String) {
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

fn canonical_service_key(host: &str, name: &str) -> String {
    format!(
        "{}:{}",
        canonical_component(host),
        canonical_component(name)
    )
}

fn canonical_component(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn safe_mount_target(target: &str) -> String {
    match target {
        "/var/run/docker.sock" => "/var/run/docker.sock".to_string(),
        "/" => "/".to_string(),
        value if value.contains("/appdata") => "appdata_path".to_string(),
        value if value.starts_with('/') => value.to_string(),
        _ => "relative_mount_target".to_string(),
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

fn is_core_collector(name: &str) -> bool {
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

struct RequestedFindingTypes {
    values: HashSet<String>,
}

impl RequestedFindingTypes {
    fn new(values: Option<&[String]>) -> ServiceResult<Self> {
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

    fn includes(&self, value: &str) -> bool {
        self.values.contains(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mount(source: Option<&str>, target: &str, read_only: bool) -> MountRef {
        MountRef {
            source: source.map(str::to_string),
            target: target.to_string(),
            read_only,
        }
    }

    #[test]
    fn classify_mount_covers_risky_source_branches() {
        let docker_rw = classify_mount(&mount(
            Some("/var/run/docker.sock"),
            "/var/run/docker.sock",
            false,
        ))
        .unwrap();
        assert_eq!(docker_rw.reason_code, reason_const::DOCKER_SOCKET_MOUNT);
        assert_eq!(docker_rw.severity, finding_const::SEVERITY_HIGH);

        let docker_ro = classify_mount(&mount(
            Some("/var/run/docker.sock"),
            "/var/run/docker.sock",
            true,
        ))
        .unwrap();
        assert_eq!(docker_ro.severity, finding_const::SEVERITY_MEDIUM);

        let root_rw = classify_mount(&mount(Some("/"), "/host", false)).unwrap();
        assert_eq!(root_rw.reason_code, reason_const::HOST_ROOT_MOUNT);
        assert_eq!(root_rw.severity, finding_const::SEVERITY_HIGH);

        let root_ro = classify_mount(&mount(Some("/"), "/host", true)).unwrap();
        assert_eq!(root_ro.severity, finding_const::SEVERITY_MEDIUM);

        let appdata_rw =
            classify_mount(&mount(Some("/mnt/user/appdata"), "/config", false)).unwrap();
        assert_eq!(appdata_rw.reason_code, reason_const::APPDATA_ROOT_MOUNT);
        assert_eq!(appdata_rw.severity, finding_const::SEVERITY_MEDIUM);

        let appdata_ro =
            classify_mount(&mount(Some("/mnt/user/appdata"), "/config", true)).unwrap();
        assert_eq!(appdata_ro.severity, finding_const::SEVERITY_LOW);

        let missing = classify_mount(&mount(None, "/config", false)).unwrap();
        assert_eq!(
            missing.reason_code,
            reason_const::MOUNT_MISSING_SOURCE_DETAIL
        );
        assert_eq!(missing.severity, finding_const::SEVERITY_LOW);
    }

    #[test]
    fn safe_mount_target_renders_sensitive_targets_safely() {
        assert_eq!(
            safe_mount_target("/var/run/docker.sock"),
            "/var/run/docker.sock"
        );
        assert_eq!(safe_mount_target("/"), "/");
        assert_eq!(safe_mount_target("/mnt/user/appdata/app"), "appdata_path");
        assert_eq!(safe_mount_target("relative"), "relative_mount_target");
    }
}
