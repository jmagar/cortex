use super::*;
use crate::inventory::schema::{Provenance, TrustLevel};
use std::collections::HashSet;

fn map_node(hostname: &str) -> HomelabMapNode {
    HomelabMapNode {
        hostname: hostname.to_string(),
        first_seen: "2026-06-03T00:00:00Z".to_string(),
        last_seen: "2026-06-03T00:00:00Z".to_string(),
        log_count: 7,
        source_ips: Vec::new(),
        apps: Vec::new(),
        inventory_roles: Vec::new(),
        inventory_ips: Vec::new(),
        heartbeat: None,
    }
}

fn inventory_node(hostname: &str, roles: &[&str], ips: &[&str]) -> InventoryNode {
    InventoryNode {
        id: format!("inventory:{hostname}"),
        hostname: hostname.to_string(),
        trust_level: TrustLevel::Observed,
        provenance: Provenance::new(
            "test",
            "source_inventory",
            "2026-06-03T00:00:00Z".to_string(),
        ),
        roles: roles.iter().map(|role| (*role).to_string()).collect(),
        ips: ips.iter().map(|ip| (*ip).to_string()).collect(),
        os: None,
        cpu: None,
        memory: None,
        listeners: Vec::new(),
        storage: Vec::new(),
        extras: Default::default(),
    }
}

#[test]
fn requested_sections_defaults_to_all_and_matches_case_insensitively() {
    let all = RequestedSections::new(None);
    assert!(all.includes("services"));
    assert!(all.includes("projects"));

    let filtered = RequestedSections::new(Some(&["Services".to_string()]));
    assert!(filtered.includes("services"));
    assert!(!filtered.includes("projects"));
}

#[test]
fn section_filters_and_records_truncation() {
    let sections = RequestedSections::new(Some(&["services".to_string()]));
    let mut truncated = Vec::new();
    let values = [1, 2, 3];

    let included = section(&sections, "services", Some(&values), 2, &mut truncated);
    let excluded = section(&sections, "projects", Some(&values), 2, &mut truncated);

    assert_eq!(included, vec![1, 2]);
    assert!(excluded.is_empty());
    assert_eq!(truncated, vec!["services".to_string()]);
}

#[test]
fn merge_inventory_nodes_enriches_existing_host_without_duplication() {
    let mut nodes = vec![map_node("host-a")];
    let mut all_hostnames = HashSet::from(["host-a".to_string()]);
    let inventory = vec![inventory_node("host-a", &["docker"], &["10.0.0.2"])];

    merge_inventory_nodes(&mut nodes, 10, &inventory, &mut all_hostnames);

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].inventory_roles, vec!["docker".to_string()]);
    assert_eq!(nodes[0].inventory_ips, vec!["10.0.0.2".to_string()]);
    assert_eq!(all_hostnames.len(), 1);
}

#[test]
fn merge_inventory_nodes_respects_host_limit() {
    let mut nodes = vec![map_node("host-a")];
    let mut all_hostnames = HashSet::from(["host-a".to_string()]);
    let inventory = vec![inventory_node("host-b", &["nas"], &["10.0.0.3"])];

    merge_inventory_nodes(&mut nodes, 1, &inventory, &mut all_hostnames);

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].hostname, "host-a");
    assert!(all_hostnames.contains("host-b"));
}

#[test]
fn total_hosts_count_uses_observed_union_with_summary_floor() {
    let all_hostnames = HashSet::from(["a".to_string(), "b".to_string(), "c".to_string()]);
    assert_eq!(total_hosts_count(&all_hostnames, 2, 1), 3);
    assert_eq!(total_hosts_count(&all_hostnames, 12, 40), 40);
}
