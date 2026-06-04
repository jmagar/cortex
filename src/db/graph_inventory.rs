mod sql;
#[cfg(test)]
#[path = "graph_inventory_tests.rs"]
mod tests;

use std::collections::BTreeMap;

use anyhow::{Context, Result};

use crate::db::graph;
use crate::db::{write_lock, DbPool};
use crate::inventory::schema::HomelabInventory;

use self::sql::{
    add_alias, add_relationship, canonical, canonical_or_raw, graph_counts, match_upstream,
    prune_previous_inventory_projection, service_key, trust, update_projection_meta,
    upsert_artifact, upsert_entity, upsert_service, upsert_storage,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryGraphStats {
    pub source_row_count: i64,
    pub entity_count: i64,
    pub relationship_count: i64,
    pub evidence_count: i64,
}

pub fn project_inventory(
    pool: &DbPool,
    inventory: &HomelabInventory,
) -> Result<InventoryGraphStats> {
    let mut conn = pool.get().context("borrow sqlite connection")?;
    let _guard = write_lock();
    let tx = conn
        .transaction()
        .context("start inventory graph transaction")?;

    prune_previous_inventory_projection(&tx)?;

    let mut hosts = BTreeMap::new();
    for node in &inventory.nodes {
        let Some(key) = canonical(&node.hostname) else {
            continue;
        };
        let entity = upsert_entity(
            &tx,
            graph::ENTITY_TYPE_HOST,
            &key,
            &node.hostname,
            graph::SOURCE_KIND_SOURCE_INVENTORY,
            &node.id,
            trust(&node.trust_level),
            &node.provenance.collected_at,
        )?;
        add_alias(
            &tx,
            entity.id,
            "hostname",
            &key,
            &node.hostname,
            graph::SOURCE_KIND_SOURCE_INVENTORY,
            trust(&node.trust_level),
            &node.provenance.collected_at,
        )?;
        for ip in &node.ips {
            if let Some(alias_key) = canonical(ip) {
                add_alias(
                    &tx,
                    entity.id,
                    "ip",
                    &alias_key,
                    ip,
                    graph::SOURCE_KIND_SOURCE_INVENTORY,
                    trust(&node.trust_level),
                    &node.provenance.collected_at,
                )?;
            }
        }
        hosts.insert(key, entity);
    }

    let mut services = BTreeMap::new();
    for service in &inventory.services {
        let service_entity = upsert_service(&tx, service)?;
        services.insert(service_key(service), service_entity.clone());
        services.insert(canonical_or_raw(&service.name), service_entity.clone());

        for domain in &service.domains {
            if let Some(alias_key) = canonical(domain) {
                add_alias(
                    &tx,
                    service_entity.id,
                    "domain",
                    &alias_key,
                    domain,
                    graph::SOURCE_KIND_APP_INVENTORY,
                    trust(&service.trust_level),
                    &service.provenance.collected_at,
                )?;
            }
        }

        if let Some(host) = service
            .host
            .as_ref()
            .and_then(|h| hosts.get(&canonical_or_raw(h)))
        {
            add_relationship(
                &tx,
                &service_entity,
                host,
                graph::REL_RUNS_ON,
                graph::REASON_INVENTORY_SERVICE,
                graph::SOURCE_KIND_APP_INVENTORY,
                &service.id,
                &service.provenance.collected_at,
                trust(&service.trust_level),
                0.85,
                &format!("{} observed on {}", service.name, host.key),
            )?;
        }

        for mount in &service.mounts {
            let storage_key = canonical_or_raw(&format!(
                "{}:{}",
                service.host.as_deref().unwrap_or("unknown"),
                mount.target
            ));
            let storage = upsert_entity(
                &tx,
                graph::ENTITY_TYPE_STORAGE,
                &storage_key,
                &mount.target,
                graph::SOURCE_KIND_APP_INVENTORY,
                &storage_key,
                graph::TRUST_INFERRED,
                &service.provenance.collected_at,
            )?;
            add_relationship(
                &tx,
                &service_entity,
                &storage,
                graph::REL_MOUNTS,
                graph::REASON_STORAGE_PROBE,
                graph::SOURCE_KIND_APP_INVENTORY,
                &service.id,
                &service.provenance.collected_at,
                graph::TRUST_INFERRED,
                0.65,
                &format!("{} mounts {}", service.name, mount.target),
            )?;
        }
    }

    let mut artifacts = BTreeMap::new();
    for artifact in &inventory.artifact_refs {
        let entity = upsert_artifact(&tx, artifact, &inventory.generated_at)?;
        artifacts.insert(artifact.id.clone(), entity.clone());
        if let Some(path) = &artifact.source_path {
            artifacts.insert(path.clone(), entity);
        }
    }

    for project in &inventory.compose_projects {
        let project_key = canonical_or_raw(&project.name);
        let project_entity = upsert_entity(
            &tx,
            graph::ENTITY_TYPE_COMPOSE_PROJECT,
            &project_key,
            &project.name,
            graph::SOURCE_KIND_APP_INVENTORY,
            &project.provenance.source,
            graph::TRUST_VERIFIED,
            &project.provenance.collected_at,
        )?;
        for service_name in &project.services {
            if let Some(service_entity) = services.get(&canonical_or_raw(service_name)) {
                add_relationship(
                    &tx,
                    &project_entity,
                    service_entity,
                    graph::REL_DEFINES_SERVICE,
                    graph::REASON_COMPOSE_CONFIG,
                    graph::SOURCE_KIND_APP_INVENTORY,
                    &project.provenance.source,
                    &project.provenance.collected_at,
                    graph::TRUST_VERIFIED,
                    0.90,
                    &format!("compose project {} defines {}", project.name, service_name),
                )?;
            }
        }
        for compose_file in &project.compose_files {
            if let Some(artifact) = artifacts.get(compose_file) {
                add_relationship(
                    &tx,
                    &project_entity,
                    artifact,
                    graph::REL_HAS_ARTIFACT,
                    graph::REASON_CONFIG_ARTIFACT,
                    graph::SOURCE_KIND_APP_INVENTORY,
                    &project.provenance.source,
                    &project.provenance.collected_at,
                    graph::TRUST_VERIFIED,
                    0.95,
                    &format!("compose artifact {}", compose_file),
                )?;
            }
        }
    }

    for route in &inventory.reverse_proxies {
        let proxy_key = canonical_or_raw(&route.id);
        let proxy = upsert_entity(
            &tx,
            graph::ENTITY_TYPE_REVERSE_PROXY,
            &proxy_key,
            route.server_names.first().unwrap_or(&route.id),
            graph::SOURCE_KIND_APP_INVENTORY,
            &route.id,
            graph::TRUST_VERIFIED,
            &route.provenance.collected_at,
        )?;
        for domain in &route.server_names {
            let domain_key = canonical_or_raw(domain);
            let domain_entity = upsert_entity(
                &tx,
                graph::ENTITY_TYPE_DOMAIN,
                &domain_key,
                domain,
                graph::SOURCE_KIND_APP_INVENTORY,
                &route.id,
                graph::TRUST_VERIFIED,
                &route.provenance.collected_at,
            )?;
            add_relationship(
                &tx,
                &proxy,
                &domain_entity,
                graph::REL_EXPOSES_DOMAIN,
                graph::REASON_REVERSE_PROXY_CONFIG,
                graph::SOURCE_KIND_APP_INVENTORY,
                &route.id,
                &route.provenance.collected_at,
                graph::TRUST_VERIFIED,
                0.95,
                &format!("proxy exposes {}", domain),
            )?;
        }
        for upstream in &route.upstreams {
            if let Some(service) = match_upstream(upstream, &services) {
                add_relationship(
                    &tx,
                    &proxy,
                    service,
                    graph::REL_ROUTES_TO,
                    graph::REASON_REVERSE_PROXY_CONFIG,
                    graph::SOURCE_KIND_APP_INVENTORY,
                    &route.id,
                    &route.provenance.collected_at,
                    graph::TRUST_VERIFIED,
                    0.85,
                    &format!("proxy routes to {}", upstream),
                )?;
            }
        }
    }

    for network in &inventory.networks {
        let network_entity = upsert_entity(
            &tx,
            graph::ENTITY_TYPE_NETWORK,
            &canonical_or_raw(&network.name),
            &network.name,
            graph::SOURCE_KIND_APP_INVENTORY,
            &network.provenance.source,
            graph::TRUST_VERIFIED,
            &network.provenance.collected_at,
        )?;
        for member in &network.members {
            if let Some(service) = services.get(&canonical_or_raw(member)) {
                add_relationship(
                    &tx,
                    service,
                    &network_entity,
                    graph::REL_ATTACHED_TO,
                    graph::REASON_DOCKER_NETWORK,
                    graph::SOURCE_KIND_APP_INVENTORY,
                    &network.provenance.source,
                    &network.provenance.collected_at,
                    graph::TRUST_VERIFIED,
                    0.80,
                    &format!("{} attached to network {}", member, network.name),
                )?;
            }
        }
    }

    for storage in &inventory.storage {
        upsert_storage(&tx, storage, &hosts)?;
    }

    let stats = graph_counts(&tx)?;
    update_projection_meta(&tx, inventory, &stats)?;
    tx.commit().context("commit inventory graph projection")?;
    Ok(stats)
}
