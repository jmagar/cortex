use super::graph_safety::*;
use super::*;
use crate::inventory::schema::{CollectionError, HomelabInventory, InventoryService, MountRef};
use crate::inventory::{
    inventory_status, read_inventory_cache, InventoryCacheStatus, InventoryConfig,
};
use std::collections::{BTreeMap, HashSet};

const FINDING_POTENTIAL_PUBLIC_ROUTE: &str = "potential_public_route";
const FINDING_RISKY_MOUNTS: &str = "risky_mounts";
const FINDING_COLLECTOR_HEALTH: &str = "collector_health";

#[derive(Debug, Clone)]
struct MapFindingContext {
    inventory: Option<HomelabInventory>,
    cache_status: InventoryCacheStatus,
}

impl CortexService {
    pub(super) async fn homelab_map_findings_answer(
        &self,
        req: &HomelabMapRequest,
    ) -> ServiceResult<HomelabMapGraphAnswer> {
        let finding_limit = req.finding_limit.unwrap_or(25).clamp(1, 100);
        let evidence_limit = req.evidence_per_finding.unwrap_or(2).clamp(1, 5) as usize;
        let requested = RequestedFindingTypes::new(req.finding_types.as_deref())?;
        let context = read_map_finding_context().await?;

        let db_limit = finding_limit.saturating_mul(4).clamp(1, 400);
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
        if requested.includes(FINDING_POTENTIAL_PUBLIC_ROUTE) {
            findings.extend(public_route_findings(
                public_routes,
                evidence_limit,
                &context,
                &graph_status,
            ));
        }
        if requested.includes(FINDING_RISKY_MOUNTS) {
            findings.extend(risky_mount_findings(
                mount_relationships,
                evidence_limit,
                &context,
                &graph_status,
            ));
        }
        if requested.includes(FINDING_COLLECTOR_HEALTH) {
            findings.extend(collector_health_findings(&context, evidence_limit));
        }
        findings.sort_by_key(finding_sort_key);
        let truncated = findings.len() > finding_limit as usize;
        findings.truncate(finding_limit as usize);

        let metadata = GraphResponseMetadata {
            projection_status: graph_status.projection_status,
            last_completed_at: graph_status.last_completed_at,
            source_watermark: graph_status.source_watermark,
            is_degraded: graph_status.is_degraded || context.cache_status.is_stale,
            last_error: graph_status.last_error.map(redact_graph_text),
            limit: finding_limit,
            depth: 1,
            truncated,
            truncated_reason: truncated.then(|| "finding_limit".to_string()),
            evidence_sample_limit: req.evidence_per_finding.unwrap_or(2).clamp(1, 5),
            payload_budget: req.payload_budget.unwrap_or(32_768).clamp(4_096, 65_536),
        };

        let degraded_reason = metadata.is_degraded.then(|| {
            if context.cache_status.status == "missing" {
                "inventory_cache_missing".to_string()
            } else if context.cache_status.is_stale {
                "inventory_cache_stale".to_string()
            } else {
                "graph_degraded".to_string()
            }
        });
        Ok(HomelabMapGraphAnswer {
            mode: "findings".to_string(),
            answer_status: "ok".to_string(),
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
                reason: truncated.then(|| "finding_limit".to_string()),
                limit: finding_limit,
                evidence_sample_limit: req.evidence_per_finding.unwrap_or(2).clamp(1, 5),
                payload_budget: req.payload_budget.unwrap_or(32_768).clamp(4_096, 65_536),
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
        let inventory = read_inventory_cache(&config).ok();
        MapFindingContext {
            inventory,
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
            TopologyFinding {
                finding_type: FINDING_POTENTIAL_PUBLIC_ROUTE.to_string(),
                severity: if has_route_target { "medium" } else { "low" }.to_string(),
                confidence,
                reason_code: if has_route_target {
                    "reverse_proxy_route_configured"
                } else {
                    "reverse_proxy_domain_without_target_proof"
                }
                .to_string(),
                affected_entities: affected,
                evidence: public_route_evidence(&row, evidence_limit),
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
    rows.into_iter()
        .flat_map(|row| {
            services
                .get(&row.service_key)
                .into_iter()
                .flat_map(move |mounts| {
                    mounts
                        .iter()
                        .filter(|(_, mount)| mount_target_matches(&row, mount))
                        .filter_map(|(service, mount)| {
                            classify_mount(mount).map(|risk| {
                                risky_mount_finding(
                                    &row,
                                    service,
                                    mount,
                                    risk,
                                    graph_status,
                                    evidence_limit,
                                )
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect()
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
    TopologyFinding {
        finding_type: FINDING_RISKY_MOUNTS.to_string(),
        severity: risk.severity.to_string(),
        confidence: mount_confidence(row.confidence, mount.read_only, graph_status.is_degraded),
        reason_code: risk.reason_code.to_string(),
        affected_entities: vec![service_entity, mount_entity],
        evidence: graph_row_evidence(row.evidence_id, row.safe_excerpt.clone(), evidence_limit),
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
    if context.cache_status.status == "missing" || context.inventory.is_none() {
        findings.push(collector_health_finding(
            "inventory_cache_missing",
            "warning",
            0.95,
            "inventory_cache",
            "Inventory cache is unavailable, so absence of topology findings is unknown.",
            evidence_limit,
        ));
    } else if context.cache_status.is_stale {
        findings.push(collector_health_finding(
            "inventory_cache_stale",
            "info",
            0.90,
            "inventory_cache",
            "Inventory cache is stale; findings are based on the last available normalized snapshot.",
            evidence_limit,
        ));
    }

    if let Some(state) = &context.cache_status.collection_state {
        for collector in &state.collectors {
            if collector.status == "ok" && collector.warnings.is_empty() {
                continue;
            }
            let severity = if is_core_collector(&collector.name) {
                "warning"
            } else {
                "info"
            };
            findings.push(collector_health_finding(
                "collector_degraded",
                severity,
                if is_core_collector(&collector.name) { 0.85 } else { 0.70 },
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
    let mut details = BTreeMap::new();
    details.insert("collector".to_string(), sanitize_token(collector));
    TopologyFinding {
        finding_type: FINDING_COLLECTOR_HEALTH.to_string(),
        severity: severity.to_string(),
        confidence,
        reason_code: reason_code.to_string(),
        affected_entities: vec![TopologyFindingEntity {
            entity_type: "collector".to_string(),
            key: sanitize_token(collector),
            label: sanitize_token(collector),
            details,
        }],
        evidence: bounded_evidence(
            vec![TopologyFindingEvidence {
                evidence_id: None,
                source_kind: "collection_state".to_string(),
                safe_excerpt: Some(context.to_string()),
            }],
            evidence_limit,
        ),
        remediation: "Refresh inventory collection and inspect collector-specific logs before treating missing topology as absence.".to_string(),
        degraded_reason: Some(reason_code.to_string()),
        confidence_context: Some(context.to_string()),
    }
}

fn collection_error_finding(error: &CollectionError, evidence_limit: usize) -> TopologyFinding {
    let severity = if is_core_collector(&error.collector) {
        "warning"
    } else {
        "info"
    };
    let mut details = BTreeMap::new();
    details.insert("collector".to_string(), sanitize_token(&error.collector));
    details.insert("phase".to_string(), sanitize_token(&error.phase));
    details.insert("truncated".to_string(), error.truncated.to_string());
    TopologyFinding {
        finding_type: FINDING_COLLECTOR_HEALTH.to_string(),
        severity: severity.to_string(),
        confidence: if is_core_collector(&error.collector) { 0.85 } else { 0.65 },
        reason_code: "collector_partial".to_string(),
        affected_entities: vec![TopologyFindingEntity {
            entity_type: "collector".to_string(),
            key: sanitize_token(&error.collector),
            label: sanitize_token(&error.collector),
            details,
        }],
        evidence: bounded_evidence(
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
        ),
        remediation: "Refresh or repair the collector, then rebuild the graph projection before relying on absence-oriented topology conclusions.".to_string(),
        degraded_reason: Some("collector_partial".to_string()),
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
            severity: if mount.read_only { "medium" } else { "high" },
            reason_code: "docker_socket_mount",
            source_kind: "docker_socket",
            remediation: "Review whether this service needs Docker daemon access; prefer a narrow proxy or read-only control path when possible.",
        });
    }
    if source == "/" {
        return Some(MountRisk {
            severity: if mount.read_only { "medium" } else { "high" },
            reason_code: "host_root_mount",
            source_kind: "host_root",
            remediation: "Replace host-root binds with the smallest required host path and keep the mount read-only when writes are not required.",
        });
    }
    if source.contains("/appdata")
        || target.contains("/appdata")
        || source.starts_with("/mnt/user/appdata")
    {
        return Some(MountRisk {
            severity: if mount.read_only { "low" } else { "medium" },
            reason_code: "appdata_root_mount",
            source_kind: "appdata_root",
            remediation: "Scope appdata binds to the service-specific directory and keep secrets outside broadly shared mounts.",
        });
    }
    if mount.source.is_none() {
        return Some(MountRisk {
            severity: "low",
            reason_code: "mount_missing_source_detail",
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
    if graph_status.is_degraded {
        Some("graph_projection_degraded".to_string())
    } else if context.cache_status.is_stale {
        Some("inventory_cache_stale".to_string())
    } else if context.inventory.as_ref().is_some_and(|inventory| {
        inventory
            .collection_errors
            .iter()
            .any(|e| is_core_collector(&e.collector))
    }) {
        Some("core_collector_partial".to_string())
    } else {
        None
    }
}

fn public_route_evidence(
    row: &db::PublicRouteFindingRow,
    evidence_limit: usize,
) -> Vec<TopologyFindingEvidence> {
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
) -> Vec<TopologyFindingEvidence> {
    bounded_evidence(
        vec![TopologyFindingEvidence {
            evidence_id,
            source_kind: "app_inventory".to_string(),
            safe_excerpt: safe_excerpt.map(redact_graph_text),
        }],
        evidence_limit,
    )
}

fn bounded_evidence(
    evidence: Vec<TopologyFindingEvidence>,
    limit: usize,
) -> Vec<TopologyFindingEvidence> {
    evidence
        .into_iter()
        .filter(|item| item.evidence_id.is_some() || item.safe_excerpt.is_some())
        .take(limit)
        .collect()
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
        "critical" => 0,
        "high" => 1,
        "medium" | "warning" => 2,
        "low" => 3,
        "info" => 4,
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
                vec![
                    FINDING_POTENTIAL_PUBLIC_ROUTE.to_string(),
                    FINDING_RISKY_MOUNTS.to_string(),
                    FINDING_COLLECTOR_HEALTH.to_string(),
                ]
            });
        for value in &values {
            if !matches!(
                value.as_str(),
                FINDING_POTENTIAL_PUBLIC_ROUTE | FINDING_RISKY_MOUNTS | FINDING_COLLECTOR_HEALTH
            ) {
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
