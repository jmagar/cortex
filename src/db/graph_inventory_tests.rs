use super::*;
use crate::config::StorageConfig;
use crate::db::{LogBatchEntry, graph, init_pool, insert_logs_batch};
use crate::inventory::schema::{
    ArtifactRef, ComposeProject, HomelabInventory, InventoryNode, InventoryService, NetworkSegment,
    PortMapping, Provenance, RedactionStatus, ReverseProxyRoute, TrustLevel,
};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

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

fn basic_inventory() -> HomelabInventory {
    let mut inventory =
        HomelabInventory::empty("inv-test".to_string(), "2026-01-01T00:00:00Z".to_string());
    inventory.nodes.push(InventoryNode {
        id: "node:dookie".to_string(),
        hostname: "dookie".to_string(),
        trust_level: TrustLevel::Observed,
        provenance: provenance("ssh:dookie", "source_inventory"),
        roles: Vec::new(),
        ips: vec!["10.1.0.6".to_string()],
        os: Some("Ubuntu".to_string()),
        cpu: None,
        memory: None,
        listeners: Vec::new(),
        storage: Vec::new(),
        extras: Default::default(),
    });
    inventory
}

fn log_entry(message: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: "2026-01-01T00:00:00Z".to_string(),
        hostname: "writer-test".to_string(),
        facility: Some("daemon".to_string()),
        severity: "info".to_string(),
        app_name: Some("test".to_string()),
        process_id: None,
        message: message.to_string(),
        raw: format!("<14>{message}"),
        source_ip: "127.0.0.1:1514".to_string(),
        docker_checkpoint: None,
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        ai_transcript_path: None,
        metadata_json: None,
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    }
}

#[test]
fn project_inventory_does_not_hold_write_lock_while_preparing_projection() {
    let _guard = graph::GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&StorageConfig::for_test(
        dir.path().join("inventory-graph-lock-scope.db"),
    ))
    .unwrap();
    graph::refresh_graph_projection(&pool).unwrap();

    let inventory = basic_inventory();
    let projection_pool = pool.clone();
    let (prepared_tx, prepared_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let projection = thread::spawn(move || {
        project_inventory_with_apply_hook(&projection_pool, &inventory, || {
            prepared_tx.send(()).unwrap();
            continue_rx.recv().unwrap();
        })
        .unwrap();
    });

    prepared_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let writer_pool = pool.clone();
    let (insert_done_tx, insert_done_rx) = mpsc::channel();
    let writer = thread::spawn(move || {
        let started = Instant::now();
        insert_logs_batch(
            &writer_pool,
            &[log_entry("write while projection prepares")],
        )
        .unwrap();
        insert_done_tx.send(started.elapsed()).unwrap();
    });

    match insert_done_rx.recv_timeout(Duration::from_millis(200)) {
        Ok(elapsed) => assert!(
            elapsed < Duration::from_millis(200),
            "insert waited for projection preparation for {elapsed:?}"
        ),
        Err(error) => {
            continue_tx.send(()).unwrap();
            projection.join().unwrap();
            writer.join().unwrap();
            panic!("insert was blocked by projection preparation: {error}");
        }
    }

    continue_tx.send(()).unwrap();
    projection.join().unwrap();
    writer.join().unwrap();
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
    assert_eq!(stats.source_row_count, 0);
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
             WHERE entity_type = 'compose_project' AND canonical_key = 'squirts:edge'"
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

#[test]
fn project_inventory_preserves_existing_entity_ownership_and_hides_config_paths() {
    let _guard = graph::GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&StorageConfig::for_test(
        dir.path().join("inventory-graph-ownership.db"),
    ))
    .unwrap();
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO graph_entities
            (entity_type, canonical_key, display_label, source_kind, source_id, trust_level)
         VALUES ('host', 'squirts', 'squirts', 'log', '42', 'claimed')",
        [],
    )
    .unwrap();
    drop(conn);

    let mut inventory =
        HomelabInventory::empty("inv-test".to_string(), "2026-01-01T00:00:00Z".to_string());
    inventory.nodes.push(InventoryNode {
        id: "node:squirts".to_string(),
        hostname: "squirts".to_string(),
        trust_level: TrustLevel::Observed,
        provenance: provenance("ssh:squirts", "source_inventory"),
        roles: Vec::new(),
        ips: Vec::new(),
        os: None,
        cpu: None,
        memory: None,
        listeners: Vec::new(),
        storage: Vec::new(),
        extras: Default::default(),
    });
    inventory.artifact_refs.push(ArtifactRef {
        id: "artifact:compose:squirts:edge".to_string(),
        kind: "compose".to_string(),
        collector: "raw_configs".to_string(),
        source_host: Some("squirts".to_string()),
        source_path: Some("/opt/edge/compose.yaml".to_string()),
        cache_path: "/home/jmagar/.cortex/inventory/raw/inv/edge.txt".to_string(),
        redaction: RedactionStatus::Redacted,
        byte_len: 42,
        truncated: false,
    });
    inventory.compose_projects.push(ComposeProject {
        name: "edge".to_string(),
        provenance: provenance("compose:squirts:/opt/edge/compose.yaml", "app_inventory"),
        services: Vec::new(),
        compose_files: vec!["/opt/edge/compose.yaml".to_string()],
        domains: Vec::new(),
        ports: Vec::new(),
    });

    project_inventory(&pool, &inventory).unwrap();
    let conn = pool.get().unwrap();
    let (source_kind, source_id, trust_level): (String, String, String) = conn
        .query_row(
            "SELECT source_kind, source_id, trust_level
               FROM graph_entities
              WHERE entity_type = 'host' AND canonical_key = 'squirts'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(source_kind, "log");
    assert_eq!(source_id, "42");
    assert_eq!(trust_level, "claimed");
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities
              WHERE entity_type = 'config_artifact'
                AND (display_label LIKE '%/opt/%' OR display_label LIKE '%/.cortex/%')"
        ),
        0
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entity_aliases
              WHERE alias_type = 'path'
                 OR alias_value LIKE '%/opt/%'
                 OR alias_value LIKE '%/.cortex/%'"
        ),
        0
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_relationship_evidence
              WHERE source_id LIKE '%/opt/%'
                 OR safe_excerpt LIKE '%/opt/%'"
        ),
        0
    );
}

#[test]
fn project_inventory_does_not_route_to_ambiguous_service_name() {
    let _guard = graph::GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&StorageConfig::for_test(
        dir.path().join("inventory-graph-ambiguous.db"),
    ))
    .unwrap();
    graph::refresh_graph_projection(&pool).unwrap();

    let mut inventory =
        HomelabInventory::empty("inv-test".to_string(), "2026-01-01T00:00:00Z".to_string());
    for host in ["squirts", "tootie"] {
        inventory.nodes.push(InventoryNode {
            id: format!("node:{host}"),
            hostname: host.to_string(),
            trust_level: TrustLevel::Observed,
            provenance: provenance(&format!("ssh:{host}"), "source_inventory"),
            roles: Vec::new(),
            ips: Vec::new(),
            os: None,
            cpu: None,
            memory: None,
            listeners: Vec::new(),
            storage: Vec::new(),
            extras: Default::default(),
        });
        inventory.services.push(InventoryService {
            id: format!("container:{host}:swag"),
            name: "swag".to_string(),
            kind: "container".to_string(),
            trust_level: TrustLevel::Observed,
            provenance: provenance(&format!("docker:{host}"), "app_inventory"),
            host: Some(host.to_string()),
            image: None,
            status: Some("running".to_string()),
            domains: Vec::new(),
            ports: Vec::new(),
            mounts: Vec::new(),
            env_keys: Vec::new(),
            labels: Default::default(),
        });
    }
    inventory.reverse_proxies.push(ReverseProxyRoute {
        id: "proxy:ambiguous.tootie.tv".to_string(),
        server_names: vec!["ambiguous.tootie.tv".to_string()],
        upstreams: vec!["swag:443".to_string()],
        provenance: provenance("swag:/config/nginx/proxy.conf", "app_inventory"),
    });

    project_inventory(&pool, &inventory).unwrap();
    let conn = pool.get().unwrap();
    assert_eq!(relationship_count(&conn, "runs_on", "inventory_service"), 2);
    assert_eq!(
        relationship_count(&conn, "routes_to", "reverse_proxy_config"),
        0
    );
}

#[test]
fn project_inventory_routes_to_service_name_beginning_with_http() {
    let _guard = graph::GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&StorageConfig::for_test(
        dir.path().join("inventory-graph-http-prefix.db"),
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
        roles: Vec::new(),
        ips: Vec::new(),
        os: None,
        cpu: None,
        memory: None,
        listeners: Vec::new(),
        storage: Vec::new(),
        extras: Default::default(),
    });
    inventory.services.push(InventoryService {
        id: "container:squirts:http-api".to_string(),
        name: "http-api".to_string(),
        kind: "container".to_string(),
        trust_level: TrustLevel::Observed,
        provenance: provenance("docker:squirts", "app_inventory"),
        host: Some("squirts".to_string()),
        image: None,
        status: Some("running".to_string()),
        domains: Vec::new(),
        ports: Vec::new(),
        mounts: Vec::new(),
        env_keys: Vec::new(),
        labels: Default::default(),
    });
    inventory.reverse_proxies.push(ReverseProxyRoute {
        id: "proxy:http-api.tootie.tv".to_string(),
        server_names: vec!["http-api.tootie.tv".to_string()],
        upstreams: vec!["http://http-api:8080".to_string()],
        provenance: provenance("swag:squirts:/config/nginx/proxy.conf", "app_inventory"),
    });

    project_inventory(&pool, &inventory).unwrap();
    let conn = pool.get().unwrap();
    assert_eq!(
        relationship_count(&conn, "routes_to", "reverse_proxy_config"),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*)
               FROM graph_relationships rel
               JOIN graph_entities dst ON dst.id = rel.dst_entity_id
              WHERE rel.relationship_type = 'routes_to'
                AND dst.entity_type = 'service'
                AND dst.canonical_key = 'squirts:http-api'"
        ),
        1
    );
}

#[test]
fn project_inventory_scopes_compose_projects_and_networks_by_source_host() {
    let _guard = graph::GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&StorageConfig::for_test(
        dir.path().join("inventory-graph-scoped.db"),
    ))
    .unwrap();
    graph::refresh_graph_projection(&pool).unwrap();

    let mut inventory =
        HomelabInventory::empty("inv-test".to_string(), "2026-01-01T00:00:00Z".to_string());
    for host in ["squirts", "tootie"] {
        inventory.nodes.push(InventoryNode {
            id: format!("node:{host}"),
            hostname: host.to_string(),
            trust_level: TrustLevel::Observed,
            provenance: provenance(&format!("ssh:{host}"), "source_inventory"),
            roles: Vec::new(),
            ips: Vec::new(),
            os: None,
            cpu: None,
            memory: None,
            listeners: Vec::new(),
            storage: Vec::new(),
            extras: Default::default(),
        });
        inventory.services.push(InventoryService {
            id: format!("container:{host}:swag"),
            name: "swag".to_string(),
            kind: "container".to_string(),
            trust_level: TrustLevel::Observed,
            provenance: provenance(&format!("docker:{host}"), "app_inventory"),
            host: Some(host.to_string()),
            image: None,
            status: Some("running".to_string()),
            domains: Vec::new(),
            ports: Vec::new(),
            mounts: Vec::new(),
            env_keys: Vec::new(),
            labels: Default::default(),
        });
        inventory.compose_projects.push(ComposeProject {
            name: "edge".to_string(),
            provenance: provenance(
                &format!("compose:{host}:/opt/edge/compose.yaml"),
                "app_inventory",
            ),
            services: vec!["swag".to_string()],
            compose_files: Vec::new(),
            domains: Vec::new(),
            ports: Vec::new(),
        });
        inventory.networks.push(NetworkSegment {
            name: "bridge".to_string(),
            kind: "docker".to_string(),
            members: vec!["swag".to_string()],
            provenance: provenance(&format!("docker:{host}"), "app_inventory"),
        });
    }

    project_inventory(&pool, &inventory).unwrap();
    let conn = pool.get().unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'compose_project'"
        ),
        2
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'network'"
        ),
        2
    );
    assert_eq!(
        relationship_count(&conn, "defines_service", "compose_config"),
        2
    );
    assert_eq!(
        relationship_count(&conn, "attached_to", "docker_network"),
        2
    );
}
