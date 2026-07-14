use super::MapFindingContext;
use super::finding_support::{entity, graph_row_evidence};
use crate::app::models::TopologyFinding;
use crate::app::topology_findings as finding_const;
use crate::app::topology_findings::reason as reason_const;
use crate::db;
use crate::inventory::schema::{HomelabInventory, InventoryService, MountRef};
use std::collections::BTreeMap;

pub(in crate::app::services::map_findings) fn risky_mount_findings(
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
    let mut service_entity = entity("service_instance", &row.service_key, &row.service_label);
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

/// Canonical `service_instance` key (`host/name`) matching the resolver's
/// graph projection; never the legacy `host:name` shape.
fn canonical_service_key(host: &str, name: &str) -> String {
    crate::db::entity_resolution::service_instance_key(host, name).unwrap_or_else(|| {
        format!(
            "{}/{}",
            canonical_component(host),
            canonical_component(name)
        )
    })
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

#[cfg(test)]
#[path = "risky_mounts_tests.rs"]
mod tests;
