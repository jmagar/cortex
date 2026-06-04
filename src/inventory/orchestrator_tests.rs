use super::*;
use crate::inventory::schema::{MediaService, Provenance};
use std::collections::BTreeMap;
use std::time::Duration;

#[tokio::test]
async fn refresh_writes_cache_and_state() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = InventoryConfig {
        root: dir.path().join("inventory"),
        compose_paths: vec![],
        proxy_paths: vec![],
        ssh_config: None,
        ssh_hosts: vec![],
        project_roots: vec![],
        docker_hosts: vec![],
        unraid_url: None,
        unraid_api_key: None,
        unifi_url: None,
        unifi_api_key: None,
        media_services: vec![],
        collection_deadline: Duration::from_secs(2),
        collector_deadline: Duration::from_millis(50),
        probe_deadline: Duration::from_millis(50),
    };
    let report = refresh_inventory(cfg).await.unwrap();
    assert!(std::path::Path::new(&report.normalized_path).exists());
    assert!(std::path::Path::new(&report.collection_state_path).exists());
}

#[tokio::test]
async fn refresh_skips_collectors_after_collection_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = InventoryConfig {
        root: dir.path().join("inventory"),
        compose_paths: vec![],
        proxy_paths: vec![],
        ssh_config: None,
        ssh_hosts: vec![],
        project_roots: vec![],
        docker_hosts: vec![],
        unraid_url: None,
        unraid_api_key: None,
        unifi_url: None,
        unifi_api_key: None,
        media_services: vec![],
        collection_deadline: Duration::ZERO,
        collector_deadline: Duration::from_millis(50),
        probe_deadline: Duration::from_millis(50),
    };
    let report = refresh_inventory(cfg).await.unwrap();
    assert_eq!(report.status, "partial");
    assert!(!report.collectors.is_empty());
    assert!(report
        .collectors
        .iter()
        .all(|collector| collector.status == "skipped"));
}

#[test]
fn inventory_has_output_counts_media_only_inventory() {
    let mut inventory = HomelabInventory::empty("run".to_string(), Utc::now().to_rfc3339());
    inventory.media_services.push(MediaService {
        service: "radarr".to_string(),
        base_url: "http://radarr.test".to_string(),
        status: "ok".to_string(),
        version: None,
        topology: BTreeMap::new(),
        provenance: Provenance::new("test", "source_inventory", Utc::now().to_rfc3339()),
    });

    assert!(inventory_has_output(&inventory));
}
