use super::*;
use crate::inventory::limits::{cap_vec, MAP_SCHEMA};
use crate::inventory::schema::{CollectionError, InventoryNode};
use crate::inventory::{
    inventory_status, read_inventory_cache, InventoryCacheStatus, InventoryConfig,
};

impl CortexService {
    pub async fn homelab_map(&self, req: HomelabMapRequest) -> ServiceResult<HomelabMapResponse> {
        let host_limit = req.host_limit.unwrap_or(100).clamp(1, 500) as usize;
        let section_limit = req.section_limit.unwrap_or(100).clamp(1, 250) as usize;
        let sections = RequestedSections::new(req.include_sections.as_deref());
        let config = InventoryConfig::from_env();
        let (cache_status, raw_inventory) = tokio::task::spawn_blocking(move || {
            let status = inventory_status(&config);
            let inv = read_inventory_cache(&config).ok();
            (status, inv)
        })
        .await
        .map_err(|e| {
            ServiceError::Internal(anyhow::anyhow!("inventory cache read panicked: {e}"))
        })?;
        let mut inventory = raw_inventory;
        if let Some(inventory) = &mut inventory {
            inventory.freshness.is_stale = cache_status.is_stale;
            inventory.freshness.cache_status = cache_status.status.clone();
        }
        let (log_hosts, mut all_hostnames, mut nodes) = self.live_host_overlay(host_limit).await?;
        let fleet = self
            .fleet_state(FleetStateRequest {
                include_ok: Some(true),
                sort: Some("hostname".to_string()),
            })
            .await?;
        let heartbeat_hosts = fleet.summary.total;
        merge_heartbeat(&mut nodes, host_limit, fleet.hosts, &mut all_hostnames);

        if let Some(inventory) = &inventory {
            merge_inventory_nodes(&mut nodes, host_limit, &inventory.nodes, &mut all_hostnames);
        }

        let mut truncated = Vec::new();
        let services = section(
            &sections,
            "services",
            inventory.as_ref().map(|i| i.services.as_slice()),
            section_limit,
            &mut truncated,
        );
        let compose_projects = section(
            &sections,
            "compose_projects",
            inventory.as_ref().map(|i| i.compose_projects.as_slice()),
            section_limit,
            &mut truncated,
        );
        let reverse_proxies = section(
            &sections,
            "reverse_proxies",
            inventory.as_ref().map(|i| i.reverse_proxies.as_slice()),
            section_limit,
            &mut truncated,
        );
        let networks = section(
            &sections,
            "networks",
            inventory.as_ref().map(|i| i.networks.as_slice()),
            section_limit,
            &mut truncated,
        );
        let storage = section(
            &sections,
            "storage",
            inventory.as_ref().map(|i| i.storage.as_slice()),
            section_limit,
            &mut truncated,
        );
        let media_services = section(
            &sections,
            "media_services",
            inventory.as_ref().map(|i| i.media_services.as_slice()),
            section_limit,
            &mut truncated,
        );
        let projects = section(
            &sections,
            "projects",
            inventory.as_ref().map(|i| i.projects.as_slice()),
            section_limit,
            &mut truncated,
        );
        let artifact_refs = section(
            &sections,
            "artifact_refs",
            inventory.as_ref().map(|i| i.artifact_refs.as_slice()),
            section_limit,
            &mut truncated,
        );
        let mut collection_errors = if sections.includes("collection_errors") {
            merge_cache_warnings(
                cache_status.clone(),
                inventory
                    .as_ref()
                    .map(|i| i.collection_errors.as_slice())
                    .unwrap_or(&[]),
            )
        } else {
            Vec::new()
        };
        if req.per_host_limit.is_some() && sections.includes("collection_errors") {
            collection_errors.push(CollectionError {
                collector: "request".to_string(),
                phase: "map".to_string(),
                severity: "warning".to_string(),
                message: "per_host_limit is ignored by map v2; use host_limit and section_limit"
                    .to_string(),
                elapsed_ms: 0,
                truncated: false,
            });
        }
        if cap_vec(&mut collection_errors, section_limit)
            && !truncated.iter().any(|name| name == "collection_errors")
        {
            truncated.push("collection_errors".to_string());
        }

        let total_hosts = total_hosts_count(&all_hostnames, log_hosts, heartbeat_hosts);
        Ok(HomelabMapResponse {
            schema: MAP_SCHEMA.to_string(),
            generated_at: rfc3339_z(Utc::now()),
            cache_status: cache_status.status.clone(),
            freshness: sections
                .includes("freshness")
                .then(|| inventory.as_ref().map(|i| i.freshness.clone()))
                .flatten(),
            summary: HomelabMapSummary {
                hosts: total_hosts,
                returned_hosts: nodes.len(),
                services: services.len(),
                compose_projects: compose_projects.len(),
                reverse_proxies: reverse_proxies.len(),
                projects: projects.len(),
                artifacts: artifact_refs.len(),
                collection_errors: collection_errors.len(),
                heartbeat_hosts,
                truncated_hosts: total_hosts > nodes.len(),
                truncated_sections: truncated,
            },
            nodes,
            services,
            compose_projects,
            reverse_proxies,
            networks,
            storage,
            media_services,
            projects,
            artifact_refs,
            collection_errors,
            cortex_overlay: CortexOverlaySummary {
                log_hosts,
                heartbeat_hosts,
                overlay_status: "bounded_hosts_and_heartbeat".to_string(),
            },
        })
    }

    async fn live_host_overlay(
        &self,
        host_limit: usize,
    ) -> ServiceResult<(usize, HashSet<String>, Vec<HomelabMapNode>)> {
        self.run_db("homelab_map_hosts", move |pool| {
            let mut hosts = db::list_hosts(pool)?;
            let all_hostnames = hosts
                .iter()
                .map(|host| host.hostname.clone())
                .collect::<HashSet<_>>();
            let total = all_hostnames.len();
            hosts.truncate(host_limit);
            let nodes = hosts
                .into_iter()
                .map(|host| HomelabMapNode {
                    hostname: host.hostname,
                    first_seen: host.first_seen,
                    last_seen: host.last_seen,
                    log_count: host.log_count,
                    source_ips: Vec::new(),
                    apps: Vec::new(),
                    inventory_roles: Vec::new(),
                    inventory_ips: Vec::new(),
                    heartbeat: None,
                })
                .collect();
            Ok((total, all_hostnames, nodes))
        })
        .await
    }
}

struct RequestedSections {
    all: bool,
    names: HashSet<String>,
}

impl RequestedSections {
    fn new(input: Option<&[String]>) -> Self {
        let names = input
            .unwrap_or(&[])
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        Self {
            all: names.is_empty(),
            names,
        }
    }

    fn includes(&self, name: &str) -> bool {
        self.all || self.names.contains(name)
    }
}

fn section<T: Clone>(
    sections: &RequestedSections,
    name: &str,
    values: Option<&[T]>,
    limit: usize,
    truncated: &mut Vec<String>,
) -> Vec<T> {
    if !sections.includes(name) {
        return Vec::new();
    }
    let Some(values) = values else {
        return Vec::new();
    };
    let is_truncated = values.len() > limit;
    let values = values.iter().take(limit).cloned().collect::<Vec<_>>();
    if is_truncated {
        truncated.push(name.to_string());
    }
    values
}

fn total_hosts_count(
    all_hostnames: &HashSet<String>,
    log_hosts: usize,
    heartbeat_hosts: usize,
) -> usize {
    all_hostnames.len().max(log_hosts).max(heartbeat_hosts)
}

fn merge_heartbeat(
    nodes: &mut Vec<HomelabMapNode>,
    host_limit: usize,
    heartbeats: Vec<FleetStateHostRow>,
    all_hostnames: &mut HashSet<String>,
) {
    let mut index = nodes
        .iter()
        .enumerate()
        .map(|(idx, node)| (node.hostname.clone(), idx))
        .collect::<HashMap<_, _>>();
    for heartbeat in heartbeats {
        all_hostnames.insert(heartbeat.hostname.clone());
        if let Some(idx) = index.get(&heartbeat.hostname).copied() {
            nodes[idx].heartbeat = Some(heartbeat);
            continue;
        }
        if nodes.len() >= host_limit {
            continue;
        }
        index.insert(heartbeat.hostname.clone(), nodes.len());
        nodes.push(HomelabMapNode {
            hostname: heartbeat.hostname.clone(),
            first_seen: heartbeat.last_heartbeat_at.clone(),
            last_seen: heartbeat.last_heartbeat_at.clone(),
            log_count: 0,
            source_ips: Vec::new(),
            apps: Vec::new(),
            inventory_roles: Vec::new(),
            inventory_ips: Vec::new(),
            heartbeat: Some(heartbeat),
        });
    }
}

fn merge_inventory_nodes(
    nodes: &mut Vec<HomelabMapNode>,
    host_limit: usize,
    inventory_nodes: &[InventoryNode],
    all_hostnames: &mut HashSet<String>,
) {
    let mut index = nodes
        .iter()
        .enumerate()
        .map(|(idx, node)| (node.hostname.clone(), idx))
        .collect::<HashMap<_, _>>();
    for inventory in inventory_nodes {
        all_hostnames.insert(inventory.hostname.clone());
        if let Some(idx) = index.get(&inventory.hostname).copied() {
            nodes[idx].inventory_roles = inventory.roles.clone();
            nodes[idx].inventory_ips = inventory.ips.clone();
            continue;
        }
        if nodes.len() >= host_limit {
            continue;
        }
        index.insert(inventory.hostname.clone(), nodes.len());
        nodes.push(HomelabMapNode {
            hostname: inventory.hostname.clone(),
            first_seen: inventory.provenance.collected_at.clone(),
            last_seen: inventory.provenance.collected_at.clone(),
            log_count: 0,
            source_ips: Vec::new(),
            apps: Vec::new(),
            inventory_roles: inventory.roles.clone(),
            inventory_ips: inventory.ips.clone(),
            heartbeat: None,
        });
    }
}

fn merge_cache_warnings(
    cache_status: InventoryCacheStatus,
    collection_errors: &[CollectionError],
) -> Vec<CollectionError> {
    let mut collection_errors = collection_errors.to_vec();
    for warning in cache_status.warnings {
        collection_errors.push(CollectionError {
            collector: "cache".to_string(),
            phase: "read".to_string(),
            severity: "warning".to_string(),
            message: warning,
            elapsed_ms: 0,
            truncated: false,
        });
    }
    collection_errors
}

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
