use super::*;
use crate::config::StorageConfig;
use crate::db::{graph, init_pool};
use crate::inventory::schema::{
    ArtifactRef, ComposeProject, HomelabInventory, InventoryNode, InventoryService, PortMapping,
    Provenance, RedactionStatus, ReverseProxyRoute, TrustLevel,
};

fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn relationship_count(conn: &rusqlite::Connection, rel_type: &str, reason: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*)
           FROM graph_relationships r
           JOIN graph_entities src ON src.id = r.src_entity_id
           JOIN graph_entities dst ON dst.id = r.dst_entity_id
          WHERE r.relationship_type = ?1
            AND r.reason_code = ?2
            AND src.canonical_key <> ''
            AND dst.canonical_key <> ''",
        rusqlite::params![rel_type, reason],
        |row| row.get(0),
    )
    .unwrap()
}

fn provenance(source: &str, kind: &str) -> Provenance {
    Provenance::new(source, kind, "2026-01-01T00:00:00Z".to_string())
}

#[test]
fn project_inventory_adds_topology_entities_relationships_and_evidence() {
    let _guard = graph::GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&StorageConfig::for_test(
        dir.path().join("inventory-graph.db"),
    ))
    .unwrap();
    graph::refresh_graph_projection(&pool).unwrap();

    let mut inventory =
        HomelabInventory::empty("inv-test".to_string(), "2026-01-01T00:00:00Z".to_string());
    inventory.nodes.push(InventoryNode {
        id: "node:squirts".to_string(),
        hostname: "squirts".to_string(),
        trust_level: TrustLevel::Observed,
        provenance: provenance("ssh:squirts", "source_inventory"),
        roles: vec!["edge".to_string()],
        ips: vec!["10.1.0.8".to_string()],
        os: Some("Ubuntu".to_string()),
        cpu: None,
        memory: None,
        listeners: Vec::new(),
        storage: Vec::new(),
        extras: Default::default(),
    });
    inventory.services.push(InventoryService {
        id: "container:squirts:swag".to_string(),
        name: "swag".to_string(),
        kind: "container".to_string(),
        trust_level: TrustLevel::Observed,
        provenance: provenance("docker:squirts", "app_inventory"),
        host: Some("squirts".to_string()),
        image: Some("lscr.io/linuxserver/swag:latest".to_string()),
        status: Some("running".to_string()),
        domains: vec!["example.tootie.tv".to_string()],
        ports: vec![PortMapping {
            host_ip: Some("0.0.0.0".to_string()),
            host_port: Some(443),
            container_port: Some(443),
            protocol: "tcp".to_string(),
        }],
        mounts: Vec::new(),
        env_keys: vec!["URL".to_string()],
        labels: Default::default(),
    });
    inventory.compose_projects.push(ComposeProject {
        name: "edge".to_string(),
        provenance: provenance("compose:squirts:/opt/edge/compose.yaml", "app_inventory"),
        services: vec!["swag".to_string()],
        compose_files: vec!["/opt/edge/compose.yaml".to_string()],
        domains: vec!["example.tootie.tv".to_string()],
        ports: Vec::new(),
    });
    inventory.reverse_proxies.push(ReverseProxyRoute {
        id: "proxy:example.tootie.tv".to_string(),
        server_names: vec!["example.tootie.tv".to_string()],
        upstreams: vec!["swag:443".to_string()],
        provenance: provenance("swag:squirts:/config/nginx/proxy.conf", "app_inventory"),
    });
    inventory.artifact_refs.push(ArtifactRef {
        id: "artifact:compose:squirts:edge".to_string(),
        kind: "compose".to_string(),
        collector: "raw_configs".to_string(),
        source_host: Some("squirts".to_string()),
        source_path: Some("/opt/edge/compose.yaml".to_string()),
        cache_path: "/home/jmagar/.cortex/inventory/artifacts/edge.yaml".to_string(),
        redaction: RedactionStatus::Redacted,
        byte_len: 42,
        truncated: false,
    });

    let stats = project_inventory(&pool, &inventory).unwrap();
    assert!(stats.entity_count >= 6);
    assert!(stats.relationship_count >= 4);
    assert!(stats.evidence_count >= 4);

    let conn = pool.get().unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities
             WHERE entity_type = 'host' AND canonical_key = 'squirts'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entity_aliases
             WHERE alias_type = 'ip' AND alias_key = '10.1.0.8'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entity_aliases
             WHERE alias_type = 'domain' AND alias_key = 'example.tootie.tv'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities
             WHERE entity_type = 'compose_project' AND canonical_key = 'edge'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities
             WHERE entity_type = 'reverse_proxy' AND canonical_key = 'proxy:example.tootie.tv'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities
             WHERE entity_type = 'domain' AND canonical_key = 'example.tootie.tv'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities
             WHERE entity_type = 'config_artifact'
               AND canonical_key = 'artifact:compose:squirts:edge'"
        ),
        1
    );
    assert_eq!(relationship_count(&conn, "runs_on", "inventory_service"), 1);
    assert_eq!(
        relationship_count(&conn, "defines_service", "compose_config"),
        1
    );
    assert_eq!(
        relationship_count(&conn, "routes_to", "reverse_proxy_config"),
        1
    );
    assert_eq!(
        relationship_count(&conn, "exposes_domain", "reverse_proxy_config"),
        1
    );
    assert_eq!(
        relationship_count(&conn, "has_artifact", "config_artifact"),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_relationship_evidence
             WHERE source_kind IN ('source_inventory', 'app_inventory')"
        ),
        5
    );
}
