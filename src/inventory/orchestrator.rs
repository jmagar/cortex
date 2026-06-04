use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use std::time::Instant;

use crate::inventory::collectors::CollectorOutput;
use crate::inventory::config::InventoryConfig;
use crate::inventory::limits::{cap_vec, INVENTORY_SCHEMA, MAX_SECTION_ITEMS};
use crate::inventory::schema::{
    CollectionState, CollectorState, GraphProjectionSummary, HomelabInventory,
};
use crate::inventory::storage::{write_json_private, InventoryPaths, RefreshLock};

#[derive(Debug, Clone, Serialize)]
pub struct InventoryRefreshReport {
    pub run_id: String,
    pub status: String,
    pub root: String,
    pub normalized_path: String,
    pub collection_state_path: String,
    pub started_at: String,
    pub finished_at: String,
    pub elapsed_ms: u128,
    pub collectors: Vec<CollectorState>,
    pub warnings: Vec<String>,
    pub artifact_paths: Vec<String>,
}

pub async fn refresh_inventory(config: InventoryConfig) -> Result<InventoryRefreshReport> {
    let paths = InventoryPaths::new(config.root.clone());
    paths.ensure_private_dirs()?;
    let _lock = RefreshLock::acquire(&paths.lock_file)?;
    let started = Utc::now().to_rfc3339();
    let run_id = run_id();
    let timer = Instant::now();
    let mut inventory = HomelabInventory::empty(run_id.clone(), started.clone());
    inventory.graph_projection = Some(GraphProjectionSummary {
        status: "not_projected".to_string(),
        source_kinds_reserved: vec!["source_inventory".to_string(), "app_inventory".to_string()],
        next_queries: vec!["cortex graph rebuild after explicit inventory projection".to_string()],
    });

    let mut states = Vec::new();
    let mut all_warnings = Vec::new();
    let mut collector_outputs = Vec::new();
    let collection_deadline = config.collection_deadline;

    macro_rules! run_collect {
        ($name:literal, $future:expr) => {{
            if timer.elapsed() > collection_deadline {
                skip_collector($name, &mut states, &mut all_warnings);
            } else {
                let collector_started = Utc::now().to_rfc3339();
                let collect_timer = Instant::now();
                let output = match tokio::time::timeout(config.collector_deadline, $future).await {
                    Ok(output) => output,
                    Err(_) => {
                        let mut output = CollectorOutput::new($name);
                        output.warn(
                            "timeout",
                            format!(
                                "collector {} exceeded {}ms",
                                $name,
                                config.collector_deadline.as_millis()
                            ),
                        );
                        output
                    }
                };
                run_collector(
                    $name,
                    output,
                    &mut states,
                    &mut all_warnings,
                    &mut collector_outputs,
                    collect_timer,
                    collector_started,
                );
            }
        }};
    }

    run_collect!(
        "raw_configs",
        crate::inventory::raw_configs::collect(
            &config.compose_paths,
            &config.proxy_paths,
            config.ssh_config.as_deref(),
            &config.ssh_hosts,
            &paths,
            &run_id,
            config.probe_deadline,
        )
    );
    run_collect!(
        "device",
        crate::inventory::device::collect(config.probe_deadline)
    );
    run_collect!(
        "docker",
        crate::inventory::docker::collect(&config.docker_hosts, config.collector_deadline)
    );
    run_collect!(
        "tailscale",
        crate::inventory::tailscale::collect(config.probe_deadline)
    );
    run_collect!(
        "unraid",
        crate::inventory::unraid::collect(
            config.unraid_url.as_deref(),
            config.unraid_api_key.as_deref(),
            config.collector_deadline,
        )
    );
    run_collect!(
        "unifi",
        crate::inventory::unifi::collect(
            config.unifi_url.as_deref(),
            config.unifi_api_key.as_deref(),
            config.collector_deadline,
        )
    );
    run_collect!(
        "media_stack",
        crate::inventory::media_stack::collect(&config.media_services, config.collector_deadline)
    );
    run_collect!(
        "projects",
        crate::inventory::projects::collect(&config.project_roots, config.probe_deadline)
    );

    for output in collector_outputs {
        inventory.nodes.extend(output.nodes);
        inventory.services.extend(output.services);
        inventory.compose_projects.extend(output.compose_projects);
        inventory.reverse_proxies.extend(output.reverse_proxies);
        inventory.networks.extend(output.networks);
        inventory.storage.extend(output.storage);
        inventory.media_services.extend(output.media_services);
        inventory.projects.extend(output.projects);
        inventory.artifact_refs.extend(output.artifacts);
        inventory.collection_errors.extend(output.errors);
    }
    apply_section_caps(&mut inventory);
    inventory.recompute_summary();
    let finished = Utc::now().to_rfc3339();
    let has_output = inventory_has_output(&inventory);
    let has_collection_errors = !inventory.collection_errors.is_empty();
    let status = if has_output && !has_collection_errors {
        "success"
    } else {
        "partial"
    }
    .to_string();
    let state = CollectionState {
        schema: INVENTORY_SCHEMA.to_string(),
        run_id: run_id.clone(),
        started_at: started.clone(),
        finished_at: finished.clone(),
        status: status.clone(),
        collectors: states.clone(),
        artifact_refs: inventory.artifact_refs.clone(),
        errors: inventory.collection_errors.clone(),
    };
    write_json_private(&paths.normalized_json, &inventory)
        .context("write normalized inventory cache")?;
    write_json_private(&paths.collection_state_json, &state).context("write collection state")?;
    Ok(InventoryRefreshReport {
        run_id,
        status,
        root: paths.root.display().to_string(),
        normalized_path: paths.normalized_json.display().to_string(),
        collection_state_path: paths.collection_state_json.display().to_string(),
        started_at: started,
        finished_at: finished,
        elapsed_ms: timer.elapsed().as_millis(),
        collectors: states,
        warnings: all_warnings,
        artifact_paths: inventory
            .artifact_refs
            .iter()
            .map(|artifact| artifact.cache_path.clone())
            .collect(),
    })
}

fn run_collector(
    name: &'static str,
    output: CollectorOutput,
    states: &mut Vec<CollectorState>,
    warnings: &mut Vec<String>,
    outputs: &mut Vec<CollectorOutput>,
    timer: Instant,
    started_at: String,
) {
    let finished = Utc::now().to_rfc3339();
    warnings.extend(output.warnings.iter().cloned());
    let status = if output.errors.iter().any(|e| e.severity == "error") {
        "failed"
    } else if output.warnings.is_empty() {
        "ok"
    } else {
        "partial"
    };
    states.push(CollectorState {
        name: name.to_string(),
        status: status.to_string(),
        started_at,
        finished_at: finished,
        elapsed_ms: timer.elapsed().as_millis(),
        warnings: output.warnings.clone(),
        artifacts: output.artifacts.iter().map(|a| a.id.clone()).collect(),
    });
    outputs.push(output);
}

fn skip_collector(
    name: &'static str,
    states: &mut Vec<CollectorState>,
    warnings: &mut Vec<String>,
) {
    let now = Utc::now().to_rfc3339();
    let warning = format!("collector {name} skipped after collection deadline");
    warnings.push(warning.clone());
    states.push(CollectorState {
        name: name.to_string(),
        status: "skipped".to_string(),
        started_at: now.clone(),
        finished_at: now,
        elapsed_ms: 0,
        warnings: vec![warning],
        artifacts: Vec::new(),
    });
}

fn apply_section_caps(inventory: &mut HomelabInventory) {
    cap_vec(&mut inventory.nodes, MAX_SECTION_ITEMS);
    cap_vec(&mut inventory.services, MAX_SECTION_ITEMS);
    cap_vec(&mut inventory.compose_projects, MAX_SECTION_ITEMS);
    cap_vec(&mut inventory.reverse_proxies, MAX_SECTION_ITEMS);
    cap_vec(&mut inventory.networks, MAX_SECTION_ITEMS);
    cap_vec(&mut inventory.storage, MAX_SECTION_ITEMS);
    cap_vec(&mut inventory.media_services, MAX_SECTION_ITEMS);
    cap_vec(&mut inventory.projects, MAX_SECTION_ITEMS);
    cap_vec(&mut inventory.artifact_refs, MAX_SECTION_ITEMS);
    cap_vec(&mut inventory.collection_errors, MAX_SECTION_ITEMS);
}

fn inventory_has_output(inventory: &HomelabInventory) -> bool {
    !inventory.nodes.is_empty()
        || !inventory.services.is_empty()
        || !inventory.compose_projects.is_empty()
        || !inventory.reverse_proxies.is_empty()
        || !inventory.networks.is_empty()
        || !inventory.storage.is_empty()
        || !inventory.media_services.is_empty()
        || !inventory.projects.is_empty()
}

fn run_id() -> String {
    format!("inv-{}-{}", Utc::now().timestamp(), std::process::id())
}

#[cfg(test)]
#[path = "orchestrator_tests.rs"]
mod tests;
