use super::*;
use crate::config::StorageConfig;
use crate::db::{init_pool, insert_logs_batch, LogBatchEntry};

fn test_storage_config(db_path: std::path::PathBuf) -> StorageConfig {
    StorageConfig::for_test(db_path)
}

fn make_entry(ts: &str, host: &str, app: Option<&str>, msg: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: host.to_string(),
        facility: None,
        severity: "info".to_string(),
        app_name: app.map(str::to_string),
        process_id: None,
        message: msg.to_string(),
        raw: msg.to_string(),
        source_ip: "10.0.0.1:514".to_string(),
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

fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

#[test]
fn refresh_graph_projection_builds_syslog_app_edges_and_is_idempotent() {
    let _guard = GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let config = test_storage_config(dir.path().join("graph-rebuild.db"));
    let pool = init_pool(&config).unwrap();

    insert_logs_batch(
        &pool,
        &[
            make_entry(
                "2026-01-01T00:00:00Z",
                "Claimed-Host",
                Some("sshd"),
                "accepted publickey",
            ),
            make_entry(
                "2026-01-01T00:12:00Z",
                "claimed-host",
                Some("sshd"),
                "session opened",
            ),
        ],
    )
    .unwrap();

    let first = match refresh_graph_projection(&pool).unwrap() {
        GraphRebuildOutcome::Rebuilt(stats) => stats,
        GraphRebuildOutcome::AlreadyRunning => panic!("unexpected single-flight skip"),
    };
    assert_eq!(first.source_row_count, 2);
    assert_eq!(first.chunk_count, 1);
    assert!(first.entity_count >= 3);
    assert!(first.relationship_count >= 2);

    let conn = pool.get().unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'source_ip'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'host'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'app'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_relationships WHERE relationship_type = 'observed_as'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT SUM(evidence_count) FROM graph_relationship_evidence
             WHERE reason_code = 'syslog_claimed_hostname'"
        ),
        2
    );
    drop(conn);

    let second = match refresh_graph_projection(&pool).unwrap() {
        GraphRebuildOutcome::Rebuilt(stats) => stats,
        GraphRebuildOutcome::AlreadyRunning => panic!("unexpected single-flight skip"),
    };
    assert_eq!(first.entity_count, second.entity_count);
    assert_eq!(first.relationship_count, second.relationship_count);
    assert_eq!(first.evidence_count, second.evidence_count);
}

#[test]
fn refresh_graph_projection_extracts_docker_from_metadata_and_source() {
    let _guard = GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let config = test_storage_config(dir.path().join("graph-docker.db"));
    let pool = init_pool(&config).unwrap();

    let mut metadata_row = make_entry(
        "2026-01-01T00:00:00Z",
        "docker-host",
        Some("cortex"),
        "container log",
    );
    metadata_row.source_ip = "docker://dookie/abcdef/stdout".to_string();
    metadata_row.metadata_json = Some(
        r#"{"docker_host":"dookie","container_id":"abcdef","container_name":"cortex","compose_project":"infra","compose_service":"cortex"}"#.to_string(),
    );
    let mut malformed_row = make_entry(
        "2026-01-01T00:01:00Z",
        "docker-host",
        Some("other"),
        "container log",
    );
    malformed_row.source_ip = "docker://dookie/bad-json/stderr".to_string();
    malformed_row.metadata_json = Some("{not-json".to_string());

    insert_logs_batch(&pool, &[metadata_row, malformed_row]).unwrap();
    refresh_graph_projection(&pool).unwrap();

    let conn = pool.get().unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'container'"
        ),
        2
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'service'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_relationships WHERE reason_code = 'docker_container_id'"
        ),
        2
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_relationships WHERE reason_code = 'docker_service_label'"
        ),
        1
    );
}

#[test]
fn refresh_graph_projection_extracts_ai_heartbeat_and_signature_sources() {
    let _guard = GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let config = test_storage_config(dir.path().join("graph-sources.db"));
    let pool = init_pool(&config).unwrap();

    let mut ai = make_entry(
        "2026-01-01T00:00:00Z",
        "agent-host",
        Some("codex"),
        "worked on cortex",
    );
    ai.ai_tool = Some("codex".to_string());
    ai.ai_project = Some("cortex".to_string());
    ai.ai_session_id = Some("sess-1".to_string());
    insert_logs_batch(&pool, &[ai]).unwrap();

    {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO host_heartbeats_latest
                (host_id, heartbeat_id, hostname, sampled_at, received_at,
                 partial, agent_version, os, architecture, metadata_json)
             VALUES ('host-1', 42, 'agent-host', '2026-01-01T00:00:00Z',
                     '2026-01-01T00:00:01Z', 0, '1.0.0', 'linux', 'x86_64', NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO error_signatures
                (signature_hash, normalizer_version, template, sample_message,
                 sample_hostname, sample_app_name, severity, first_seen_at,
                 last_seen_at, total_count)
             VALUES ('abc123', 1, 'error <id>', 'error 1', 'agent-host',
                     'codex', 'err', '2026-01-01T00:00:00Z',
                     '2026-01-01T00:05:00Z', 3)",
            [],
        )
        .unwrap();
    }

    refresh_graph_projection(&pool).unwrap();

    let conn = pool.get().unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'ai_project'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'ai_session'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'error_signature'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entity_aliases WHERE alias_type = 'heartbeat_host_id'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_relationships WHERE relationship_type = 'worked_on'"
        ),
        1
    );
    assert!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_relationships WHERE relationship_type = 'matches_signature'"
        ) >= 1
    );
}

#[test]
fn refresh_graph_projection_removes_deleted_source_log_evidence() {
    let _guard = GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let config = test_storage_config(dir.path().join("graph-ghost.db"));
    let pool = init_pool(&config).unwrap();

    insert_logs_batch(
        &pool,
        &[make_entry(
            "2026-01-01T00:00:00Z",
            "ghost-host",
            Some("sshd"),
            "temporary row",
        )],
    )
    .unwrap();
    refresh_graph_projection(&pool).unwrap();
    {
        let conn = pool.get().unwrap();
        assert!(count(&conn, "SELECT COUNT(*) FROM graph_relationship_evidence") > 0);
        conn.execute("DELETE FROM logs", []).unwrap();
    }

    refresh_graph_projection(&pool).unwrap();
    let conn = pool.get().unwrap();
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM graph_relationship_evidence"),
        0
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM graph_relationships"), 0);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM graph_entities"), 0);
}

#[test]
fn refresh_graph_projection_reports_status_failures_and_single_flight() {
    let _guard = GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let config = test_storage_config(dir.path().join("graph-status.db"));
    let pool = init_pool(&config).unwrap();

    let held = GRAPH_REBUILD_LOCK.try_lock().unwrap();
    assert_eq!(
        refresh_graph_projection(&pool).unwrap(),
        GraphRebuildOutcome::AlreadyRunning
    );
    drop(held);

    {
        let conn = pool.get().unwrap();
        conn.execute("DROP TABLE graph_relationships", []).unwrap();
    }
    let err = refresh_graph_projection(&pool).unwrap_err();
    assert!(err.to_string().contains("graph_relationships"));
    let status = graph_projection_status(&pool).unwrap();
    assert_eq!(status.projection_status, "failed");
    assert!(status.is_degraded);
    assert!(status.last_error.is_some());
}
