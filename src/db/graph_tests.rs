use super::*;
use crate::config::StorageConfig;
use crate::db::{LogBatchEntry, init_pool, insert_logs_batch};

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
fn graph_evidence_by_id_returns_relationship_entities_and_source_log_summary() {
    let _guard = GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let config = test_storage_config(dir.path().join("graph-evidence-lookup.db"));
    let pool = init_pool(&config).unwrap();

    insert_logs_batch(
        &pool,
        &[make_entry(
            "2026-01-01T00:00:00Z",
            "proof-host",
            Some("sshd"),
            "proof row",
        )],
    )
    .unwrap();
    refresh_graph_projection(&pool).unwrap();

    let conn = pool.get().unwrap();
    let evidence_id: i64 = conn
        .query_row(
            "SELECT id FROM graph_relationship_evidence
             WHERE source_log_id IS NOT NULL
             ORDER BY id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(conn);

    let rows = graph_evidence_by_id(&pool, evidence_id).unwrap().unwrap();
    assert_eq!(rows.evidence.id, evidence_id);
    assert_eq!(rows.evidence.relationship_id, rows.relationship.id);
    assert!(rows.evidence.source_log_id.is_some());
    assert!(rows.source_log_summary.is_some());
    assert_eq!(
        rows.source_log_summary.as_ref().unwrap().message,
        "proof row"
    );
    assert_eq!(rows.src_entity.id, rows.relationship.src_entity_id);
    assert_eq!(rows.dst_entity.id, rows.relationship.dst_entity_id);
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

#[test]
fn parse_log_watermark_extracts_log_cursor() {
    assert_eq!(
        parse_log_watermark("logs:42;heartbeats:3;signatures:7"),
        Some(42)
    );
    // Order-independent.
    assert_eq!(
        parse_log_watermark("heartbeats:3;logs:99;signatures:7"),
        Some(99)
    );
    assert_eq!(parse_log_watermark("logs:0"), Some(0));
    assert_eq!(parse_log_watermark(""), None);
    assert_eq!(parse_log_watermark("heartbeats:3;signatures:7"), None);
    assert_eq!(parse_log_watermark("logs:notanumber"), None);
}

#[test]
fn incremental_projection_falls_back_to_full_build_when_unbuilt() {
    let _guard = GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&test_storage_config(
        dir.path().join("graph-inc-fallback.db"),
    ))
    .unwrap();

    insert_logs_batch(
        &pool,
        &[make_entry(
            "2026-01-01T00:00:00Z",
            "host-a",
            Some("sshd"),
            "accepted publickey",
        )],
    )
    .unwrap();

    // No prior projection: incremental must perform a full build.
    let stats = match refresh_graph_projection_incremental(&pool).unwrap() {
        GraphRebuildOutcome::Rebuilt(stats) => stats,
        GraphRebuildOutcome::AlreadyRunning => panic!("unexpected single-flight skip"),
    };
    assert!(stats.entity_count >= 3);
    let status = graph_projection_status(&pool).unwrap();
    assert_eq!(status.projection_status, "ready");
    assert!(!status.is_degraded);
}

/// The gold-standard correctness check: a full build followed by an incremental
/// delta must yield the same graph (entity/relationship/evidence counts and
/// accumulated evidence totals) as a single full rebuild over all the same logs.
#[test]
fn incremental_projection_matches_full_rebuild() {
    let _guard = GRAPH_TEST_LOCK.lock();

    let batch1 = || {
        vec![
            make_entry("2026-01-01T00:00:00Z", "host-a", Some("sshd"), "login a"),
            make_entry("2026-01-01T00:05:00Z", "host-a", Some("sshd"), "login b"),
        ]
    };
    // batch2 reuses host-a/sshd (accumulates evidence on existing edges) and
    // introduces host-b/nginx (new entities + edges discovered incrementally).
    let batch2 = || {
        vec![
            make_entry("2026-01-01T01:00:00Z", "host-a", Some("sshd"), "login c"),
            make_entry("2026-01-01T01:05:00Z", "host-b", Some("nginx"), "GET /"),
        ]
    };

    // DB A: full build over batch1, then an incremental pass over batch2.
    let dir_a = tempfile::tempdir().unwrap();
    let pool_a = init_pool(&test_storage_config(dir_a.path().join("graph-inc-a.db"))).unwrap();
    insert_logs_batch(&pool_a, &batch1()).unwrap();
    refresh_graph_projection(&pool_a).unwrap();
    insert_logs_batch(&pool_a, &batch2()).unwrap();
    let incremental = match refresh_graph_projection_incremental(&pool_a).unwrap() {
        GraphRebuildOutcome::Rebuilt(stats) => stats,
        GraphRebuildOutcome::AlreadyRunning => panic!("unexpected single-flight skip"),
    };

    // DB B: a single full rebuild over batch1 + batch2.
    let dir_b = tempfile::tempdir().unwrap();
    let pool_b = init_pool(&test_storage_config(dir_b.path().join("graph-inc-b.db"))).unwrap();
    insert_logs_batch(&pool_b, &batch1()).unwrap();
    insert_logs_batch(&pool_b, &batch2()).unwrap();
    let full = match refresh_graph_projection(&pool_b).unwrap() {
        GraphRebuildOutcome::Rebuilt(stats) => stats,
        GraphRebuildOutcome::AlreadyRunning => panic!("unexpected single-flight skip"),
    };

    assert_eq!(
        incremental.entity_count, full.entity_count,
        "entity count must match a full rebuild"
    );
    assert_eq!(
        incremental.relationship_count, full.relationship_count,
        "relationship count must match a full rebuild"
    );
    assert_eq!(
        incremental.evidence_count, full.evidence_count,
        "evidence row count must match a full rebuild"
    );

    let conn_a = pool_a.get().unwrap();
    let conn_b = pool_b.get().unwrap();
    // Accumulated evidence totals must match (guards against double-counting or
    // dropped evidence in the incremental merge).
    let ev_sum = "SELECT COALESCE(SUM(evidence_count), 0) FROM graph_relationship_evidence";
    assert_eq!(
        count(&conn_a, ev_sum),
        count(&conn_b, ev_sum),
        "summed evidence_count must match a full rebuild"
    );
    let rel_ev_sum = "SELECT COALESCE(SUM(evidence_count), 0) FROM graph_relationships";
    assert_eq!(
        count(&conn_a, rel_ev_sum),
        count(&conn_b, rel_ev_sum),
        "relationship evidence_count rollups must match a full rebuild"
    );
    // host-b only appears in batch2, so the incremental pass must have created it.
    assert_eq!(
        count(
            &conn_a,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type='host' AND canonical_key='host-b'"
        ),
        1,
        "incremental pass must create entities first seen in the delta"
    );
}

/// A second incremental pass with no new logs must be a no-op for counts (the
/// bounded snapshot re-projection stays idempotent).
#[test]
fn incremental_projection_is_idempotent_without_new_logs() {
    let _guard = GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&test_storage_config(dir.path().join("graph-inc-idem.db"))).unwrap();
    insert_logs_batch(
        &pool,
        &[
            make_entry("2026-01-01T00:00:00Z", "host-a", Some("sshd"), "login a"),
            make_entry("2026-01-01T00:05:00Z", "host-a", Some("sshd"), "login b"),
        ],
    )
    .unwrap();
    refresh_graph_projection(&pool).unwrap();

    let first = match refresh_graph_projection_incremental(&pool).unwrap() {
        GraphRebuildOutcome::Rebuilt(stats) => stats,
        GraphRebuildOutcome::AlreadyRunning => panic!("unexpected single-flight skip"),
    };
    let second = match refresh_graph_projection_incremental(&pool).unwrap() {
        GraphRebuildOutcome::Rebuilt(stats) => stats,
        GraphRebuildOutcome::AlreadyRunning => panic!("unexpected single-flight skip"),
    };
    assert_eq!(first.entity_count, second.entity_count);
    assert_eq!(first.relationship_count, second.relationship_count);
    assert_eq!(first.evidence_count, second.evidence_count);
}

/// Build a log entry shaped like `command_log::agent_record_to_entry` output:
/// `agent-command://` source_ip, agent in ai_tool/app_name, raw cwd in
/// ai_project, session id in ai_session_id.
fn make_agent_command_entry(
    ts: &str,
    host: &str,
    agent: &str,
    session: &str,
    cwd: &str,
) -> LogBatchEntry {
    let mut entry = make_entry(ts, host, Some(agent), "cargo test");
    entry.source_ip = format!("agent-command://{host}/{agent}/{session}");
    entry.ai_tool = Some(agent.to_string());
    entry.ai_project = Some(cwd.to_string());
    entry.ai_session_id = Some(session.to_string());
    entry.metadata_json = Some(format!(
        r#"{{"source_kind":"agent-command","agent_command":{{"cwd":"{cwd}","session_id":"{session}"}}}}"#
    ));
    entry
}

#[test]
fn infer_project_from_cwd_prefers_workspace_segment() {
    assert_eq!(
        infer_project_from_cwd("/home/jmagar/workspace/cortex"),
        Some("cortex".to_string())
    );
    // Deep worktree path still resolves to the repo under workspace/.
    assert_eq!(
        infer_project_from_cwd("/home/jmagar/workspace/cortex/.claude/worktrees/foo"),
        Some("cortex".to_string())
    );
    // No workspace component → final segment.
    assert_eq!(
        infer_project_from_cwd("/srv/projects/axon/"),
        Some("axon".to_string())
    );
    assert_eq!(infer_project_from_cwd("/"), None);
    assert_eq!(infer_project_from_cwd(""), None);
}

#[test]
fn agent_command_row_creates_verified_session_host_and_inferred_project_edges() {
    let _guard = GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&test_storage_config(dir.path().join("graph-agent-cmd.db"))).unwrap();

    insert_logs_batch(
        &pool,
        &[make_agent_command_entry(
            "2026-01-01T00:00:00Z",
            "dookie",
            "claude",
            "sess-7",
            "/home/jmagar/workspace/cortex",
        )],
    )
    .unwrap();
    refresh_graph_projection(&pool).unwrap();

    let conn = pool.get().unwrap();

    // Exactly one ai_session, keyed by the INFERRED project (not the raw cwd path).
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'ai_session'"
        ),
        1
    );
    let session_key: String = conn
        .query_row(
            "SELECT canonical_key FROM graph_entities WHERE entity_type = 'ai_session'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(session_key, "cortex:claude:sess-7");

    // Verified session→host edge (agent_command_session, 0.95).
    let (trust, conf): (String, f64) = conn
        .query_row(
            "SELECT trust_level, confidence FROM graph_relationships
             WHERE relationship_type = 'worked_on' AND reason_code = 'agent_command_session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(trust, "verified");
    assert!((conf - 0.95).abs() < 1e-9, "confidence was {conf}");

    // Inferred session→project edge (agent_command_cwd_infer, 0.7) to ai_project:cortex.
    let project_key: String = conn
        .query_row(
            "SELECT e.canonical_key FROM graph_relationships r
             JOIN graph_entities e ON e.id = r.dst_entity_id
             WHERE r.reason_code = 'agent_command_cwd_infer'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(project_key, "cortex");
}

#[test]
fn agent_command_session_converges_with_transcript_session_entity() {
    let _guard = GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&test_storage_config(dir.path().join("graph-converge.db"))).unwrap();

    // Transcript event: clean project "cortex", same tool + session id.
    let mut transcript = make_entry("2026-01-01T00:00:00Z", "dookie", Some("claude"), "thinking");
    transcript.ai_tool = Some("claude".to_string());
    transcript.ai_project = Some("cortex".to_string());
    transcript.ai_session_id = Some("sess-9".to_string());
    transcript.source_ip = "agent://dookie".to_string();

    // Agent-command row: raw cwd, same session id → must converge on one entity.
    let cmd = make_agent_command_entry(
        "2026-01-01T00:01:00Z",
        "dookie",
        "claude",
        "sess-9",
        "/home/jmagar/workspace/cortex",
    );

    insert_logs_batch(&pool, &[transcript, cmd]).unwrap();
    refresh_graph_projection(&pool).unwrap();

    let conn = pool.get().unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM graph_entities WHERE entity_type = 'ai_session'"
        ),
        1,
        "transcript and agent-command rows for the same session must share one ai_session entity"
    );
}

#[test]
fn agent_command_incremental_rebuild_adds_no_duplicate_edges() {
    let _guard = GRAPH_TEST_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&test_storage_config(dir.path().join("graph-agent-inc.db"))).unwrap();

    insert_logs_batch(
        &pool,
        &[make_agent_command_entry(
            "2026-01-01T00:00:00Z",
            "dookie",
            "claude",
            "sess-3",
            "/home/jmagar/workspace/cortex",
        )],
    )
    .unwrap();
    refresh_graph_projection(&pool).unwrap();
    let before: i64 = count(
        &pool.get().unwrap(),
        "SELECT COUNT(*) FROM graph_relationships WHERE reason_code LIKE 'agent_command%'",
    );

    // Second command in the same session → incremental rebuild, no new edges.
    insert_logs_batch(
        &pool,
        &[make_agent_command_entry(
            "2026-01-01T00:02:00Z",
            "dookie",
            "claude",
            "sess-3",
            "/home/jmagar/workspace/cortex",
        )],
    )
    .unwrap();
    refresh_graph_projection_incremental(&pool).unwrap();
    let after: i64 = count(
        &pool.get().unwrap(),
        "SELECT COUNT(*) FROM graph_relationships WHERE reason_code LIKE 'agent_command%'",
    );

    assert_eq!(
        before, after,
        "incremental rebuild must not duplicate agent-command edges"
    );
    assert_eq!(
        before, 2,
        "one verified host edge + one inferred project edge"
    );
}
