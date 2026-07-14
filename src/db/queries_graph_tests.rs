//! Tests for graph-anchored traversal and log fan-out
//! (`graph_walk_n_hops`, `search_logs_from_graph_related_entities`).

use super::*;
use crate::db::graph::{
    self, GRAPH_WALK_MAX_DEPTH, REL_RUNS_ON, graph_walk_n_hops, refresh_graph_projection,
};
use crate::db::{LogBatchEntry, init_pool, insert_logs_batch};

fn test_pool(name: &str) -> (tempfile::TempDir, DbPool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(&StorageConfig::for_test(dir.path().join(name))).unwrap();
    (dir, pool)
}

fn insert_entity(conn: &rusqlite::Connection, entity_type: &str, key: &str) -> i64 {
    conn.execute(
        "INSERT INTO graph_entities
            (entity_type, canonical_key, display_label, trust_level)
         VALUES (?1, ?2, ?2, 'verified')",
        rusqlite::params![entity_type, key],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn insert_rel(conn: &rusqlite::Connection, src: i64, dst: i64, rel: &str) {
    conn.execute(
        "INSERT INTO graph_relationships
            (relationship_key, src_entity_id, dst_entity_id, relationship_type,
             reason_code, trust_level, confidence, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, 'log_app_name', 'inferred', 0.5,
                 '2026-01-01T00:00:00Z')",
        rusqlite::params![format!("{src}:{rel}:{dst}"), src, dst, rel],
    )
    .unwrap();
}

fn keys(entities: &[graph::GraphWalkEntity]) -> Vec<String> {
    let mut k: Vec<String> = entities.iter().map(|e| e.canonical_key.clone()).collect();
    k.sort();
    k
}

#[test]
fn graph_walk_single_hop_returns_seed_and_neighbour() {
    let (_d, pool) = test_pool("walk-1hop.db");
    let conn = pool.get().unwrap();
    let a = insert_entity(&conn, graph::ENTITY_TYPE_HOST, "host-a");
    let b = insert_entity(&conn, graph::ENTITY_TYPE_APP, "app-b");
    insert_rel(&conn, b, a, REL_RUNS_ON);

    let reached = graph_walk_n_hops(&conn, &["host-a".to_string()], 1).unwrap();
    assert_eq!(keys(&reached), vec!["app-b", "host-a"]);
}

#[test]
fn graph_walk_two_hops_respects_depth() {
    let (_d, pool) = test_pool("walk-2hop.db");
    let conn = pool.get().unwrap();
    let a = insert_entity(&conn, graph::ENTITY_TYPE_HOST, "a");
    let b = insert_entity(&conn, graph::ENTITY_TYPE_APP, "b");
    let c = insert_entity(&conn, graph::ENTITY_TYPE_APP, "c");
    insert_rel(&conn, a, b, REL_RUNS_ON);
    insert_rel(&conn, b, c, REL_RUNS_ON);

    // depth 1 reaches only the direct neighbour.
    let depth1 = graph_walk_n_hops(&conn, &["a".to_string()], 1).unwrap();
    assert_eq!(keys(&depth1), vec!["a", "b"]);

    // depth 2 reaches the far node too.
    let depth2 = graph_walk_n_hops(&conn, &["a".to_string()], 2).unwrap();
    assert_eq!(keys(&depth2), vec!["a", "b", "c"]);
}

#[test]
fn graph_walk_terminates_on_cycle() {
    let (_d, pool) = test_pool("walk-cycle.db");
    let conn = pool.get().unwrap();
    let a = insert_entity(&conn, graph::ENTITY_TYPE_HOST, "a");
    let b = insert_entity(&conn, graph::ENTITY_TYPE_APP, "b");
    let c = insert_entity(&conn, graph::ENTITY_TYPE_APP, "c");
    // 3-cycle: a → b → c → a
    insert_rel(&conn, a, b, REL_RUNS_ON);
    insert_rel(&conn, b, c, REL_RUNS_ON);
    insert_rel(&conn, c, a, REL_RUNS_ON);

    // UNION (not UNION ALL) dedups visited rows, so the walk converges.
    let reached = graph_walk_n_hops(&conn, &["a".to_string()], GRAPH_WALK_MAX_DEPTH).unwrap();
    assert_eq!(keys(&reached), vec!["a", "b", "c"]);
}

#[test]
fn graph_walk_clamps_depth_and_handles_empty_seed() {
    let (_d, pool) = test_pool("walk-clamp.db");
    let conn = pool.get().unwrap();
    let a = insert_entity(&conn, graph::ENTITY_TYPE_HOST, "a");
    let b = insert_entity(&conn, graph::ENTITY_TYPE_APP, "b");
    insert_rel(&conn, a, b, REL_RUNS_ON);

    // depth 0 is clamped up to 1 (still reaches the direct neighbour).
    let clamped_low = graph_walk_n_hops(&conn, &["a".to_string()], 0).unwrap();
    assert_eq!(keys(&clamped_low), vec!["a", "b"]);

    // Oversized depth is clamped to the ceiling without error.
    let clamped_high = graph_walk_n_hops(&conn, &["a".to_string()], 250).unwrap();
    assert_eq!(keys(&clamped_high), vec!["a", "b"]);

    // Empty seed set returns empty.
    assert!(graph_walk_n_hops(&conn, &[], 3).unwrap().is_empty());
}

#[test]
fn graph_walk_uses_relationship_indexes() {
    let (_d, pool) = test_pool("walk-plan.db");
    let conn = pool.get().unwrap();
    let plan: Vec<String> = conn
        .prepare(
            "EXPLAIN QUERY PLAN
             WITH RECURSIVE graph_walk(entity_id, depth) AS (
                 SELECT id, 0 FROM graph_entities WHERE canonical_key IN ('a')
                 UNION
                 SELECT CASE WHEN r.src_entity_id = gw.entity_id
                             THEN r.dst_entity_id ELSE r.src_entity_id END,
                        gw.depth + 1
                 FROM graph_relationships r
                 JOIN graph_walk gw
                   ON r.src_entity_id = gw.entity_id OR r.dst_entity_id = gw.entity_id
                 WHERE gw.depth < 6
             )
             SELECT DISTINCT e.entity_type, e.canonical_key
             FROM graph_entities e JOIN graph_walk gw ON e.id = gw.entity_id",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    // The recursive relationship join must be index-served on src/dst entity id,
    // never a full table scan of graph_relationships.
    assert!(
        !plan.iter().any(|p| p == "SCAN graph_relationships"),
        "recursive hop must not full-scan graph_relationships: {plan:?}"
    );
}

fn syslog_row(ts: &str, host: &str, app: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: host.to_string(),
        facility: None,
        severity: "info".to_string(),
        app_name: Some(app.to_string()),
        process_id: None,
        message: format!("{app} message"),
        raw: format!("{app} message"),
        source_ip: "10.0.0.5:514".to_string(),
        docker_checkpoint: None,
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        ai_transcript_path: None,
        metadata_json: Some(r#"{"source_kind":"syslog-udp"}"#.to_string()),
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    }
}

fn agent_command_row(ts: &str, host: &str, session: &str, cwd: &str) -> LogBatchEntry {
    let mut row = syslog_row(ts, host, "claude");
    row.source_ip = format!("agent-command://{host}/claude/{session}");
    row.ai_tool = Some("claude".to_string());
    row.ai_project = Some(cwd.to_string());
    row.ai_session_id = Some(session.to_string());
    row.metadata_json = Some(format!(
        r#"{{"source_kind":"agent-command","agent_command":{{"cwd":"{cwd}"}}}}"#
    ));
    row
}

#[test]
fn search_logs_from_graph_fans_out_from_session_to_host_logs() {
    let _guard = graph::GRAPH_TEST_LOCK.lock();
    let (_d, pool) = test_pool("fanout.db");

    insert_logs_batch(
        &pool,
        &[
            // Session sess-7 ran commands on dookie (links ai_session → host).
            agent_command_row(
                "2026-01-01T00:00:00Z",
                "dookie",
                "sess-7",
                "/home/jmagar/workspace/cortex",
            ),
            // Plain syslog on the same host — should be reached via the host edge.
            syslog_row("2026-01-01T00:01:00Z", "dookie", "swag"),
            // Unrelated host — must NOT be returned.
            syslog_row("2026-01-01T00:02:00Z", "squirts", "authelia"),
        ],
    )
    .unwrap();
    refresh_graph_projection(&pool).unwrap();

    // Seed from the AI session entity; traversal reaches host:dookie.
    let session_key = "cortex:claude:sess-7".to_string();
    let logs = search_logs_from_graph_related_entities(
        &pool,
        &[session_key],
        2,
        None,
        None,
        None,
        100,
        HostFanoutScope::WalkReached,
    )
    .unwrap();

    let hosts: std::collections::HashSet<&str> = logs.iter().map(|l| l.hostname.as_str()).collect();
    assert!(
        hosts.contains("dookie"),
        "must fan out to dookie logs: {hosts:?}"
    );
    assert!(
        !hosts.contains("squirts"),
        "unrelated host must be excluded"
    );
    assert!(
        logs.iter().any(|l| l.app_name.as_deref() == Some("swag")),
        "the swag syslog row on the related host must be returned"
    );
}

#[test]
fn search_logs_from_graph_respects_source_kind_filter() {
    let _guard = graph::GRAPH_TEST_LOCK.lock();
    let (_d, pool) = test_pool("fanout-source.db");

    insert_logs_batch(
        &pool,
        &[
            agent_command_row(
                "2026-01-01T00:00:00Z",
                "dookie",
                "sess-7",
                "/home/jmagar/workspace/cortex",
            ),
            syslog_row("2026-01-01T00:01:00Z", "dookie", "swag"),
        ],
    )
    .unwrap();
    refresh_graph_projection(&pool).unwrap();

    // Restrict to syslog-udp only → the agent-command row is filtered out.
    let logs = search_logs_from_graph_related_entities(
        &pool,
        &["cortex:claude:sess-7".to_string()],
        2,
        None,
        None,
        Some(&[SourceKind::SyslogUdp]),
        100,
        HostFanoutScope::WalkReached,
    )
    .unwrap();
    assert!(!logs.is_empty(), "syslog row should survive the filter");
    assert!(
        logs.iter()
            .all(|l| !l.source_ip.starts_with("agent-command://")),
        "agent-command rows must be excluded by the source_kind filter"
    );
}

#[test]
fn search_logs_from_graph_empty_for_unknown_seed() {
    let (_d, pool) = test_pool("fanout-empty.db");
    let logs = search_logs_from_graph_related_entities(
        &pool,
        &["does-not-exist".to_string()],
        2,
        None,
        None,
        None,
        100,
        HostFanoutScope::WalkReached,
    )
    .unwrap();
    assert!(logs.is_empty());
}

#[test]
fn topic_resolving_logical_service_with_no_instances_reports_degraded() {
    let (_dir, pool) = test_pool("topic-zero-instance-degraded.db");
    // Stale/unbuilt projection: the logical service entity exists but has no
    // `instance_of` service instances (e.g. right after migration 41 before
    // `cortex graph rebuild` runs). The resolved entity must surface as
    // degraded so the empty service timeline is explained, never silent.
    {
        let conn = pool.get().unwrap();
        insert_entity(&conn, graph::ENTITY_TYPE_LOGICAL_SERVICE, "plex");
    }
    let inputs =
        topic_correlate_inputs(&pool, &["plex".to_string()], 2, None, None, None, 100).unwrap();
    let entity = inputs
        .resolved
        .iter()
        .find(|e| e.entity_type == graph::ENTITY_TYPE_LOGICAL_SERVICE && e.canonical_key == "plex")
        .expect("logical service must resolve");
    assert_eq!(entity.resolver_status, ResolverStatus::Degraded);
    assert!(inputs.logs.is_empty(), "no instances → no service fan-out");
}

#[test]
fn topic_correlate_app_seed_does_not_fan_out_to_whole_host() {
    let (_dir, pool) = test_pool("topic-app-seed-no-host-fanout.db");
    // Bare `plex` app label (no agent-docker metadata) plus an unrelated
    // kernel row on the same host.
    insert_logs_batch(
        &pool,
        &[
            syslog_row("2026-01-01T00:00:00Z", "tootie", "plex"),
            syslog_row("2026-01-01T00:01:00Z", "tootie", "kernel"),
        ],
    )
    .unwrap();
    // Graph: app:plex —emitted_by→ host:tootie (log-identity edge).
    {
        let conn = pool.get().unwrap();
        let app = insert_entity(&conn, graph::ENTITY_TYPE_APP, "plex");
        let host = insert_entity(&conn, graph::ENTITY_TYPE_HOST, "tootie");
        insert_rel(&conn, app, host, graph::REL_EMITTED_BY);
    }

    let inputs =
        topic_correlate_inputs(&pool, &["plex".to_string()], 2, None, None, None, 100).unwrap();
    // The topic resolves to the raw app entity and the walk reaches
    // host:tootie, but the transitively reached host must never drive
    // host-wide log inclusion labelled `resolved`.
    assert!(
        inputs
            .resolved
            .iter()
            .any(|entity| entity.entity_type == graph::ENTITY_TYPE_APP
                && entity.canonical_key == "plex"),
        "topic must resolve the raw app entity: {:?}",
        inputs.resolved
    );
    assert!(
        !inputs.logs.iter().any(|row| {
            row.entry.app_name.as_deref() == Some("kernel")
                && row.resolver_status == ResolverStatus::Resolved
        }),
        "unrelated kernel row on the host must not be included as resolved: {:?}",
        inputs
            .logs
            .iter()
            .map(|row| (
                row.entry.app_name.clone(),
                row.resolver_status,
                row.fallback_kind.clone()
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn search_logs_for_service_instances_uses_service_predicates_not_host_fanout() {
    let (_dir, pool) = test_pool("service-instance-predicates.db");
    let mut plex = syslog_row("2026-01-01T00:01:00Z", "tootie", "plex/plex/plex");
    plex.metadata_json = Some(
        r#"{"source_kind":"agent-docker","agent_docker":{"host":"tootie","container_id":"abc","container_name":"plex","compose_service":"plex","stream":"stdout"}}"#
            .to_string(),
    );
    let mut exact = syslog_row("2026-01-01T00:02:00Z", "tootie", "plex");
    exact.metadata_json = None;
    insert_logs_batch(
        &pool,
        &[
            syslog_row("2026-01-01T00:00:00Z", "tootie", "kernel"),
            plex,
            exact,
            syslog_row("2026-01-01T00:03:00Z", "shart", "plex"),
        ],
    )
    .unwrap();

    let rows = search_logs_for_service_instances(
        &pool,
        &["tootie/plex".to_string()],
        None,
        None,
        None,
        50,
    )
    .unwrap();
    // Matches: exact app label, prefixed nested label, structured compose
    // service — all scoped to the instance's host. The kernel row on the
    // same host and the other host's plex row stay out.
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.entry.hostname == "tootie"));
    assert!(
        rows.iter()
            .all(|row| row.entry.app_name.as_deref() != Some("kernel"))
    );
    assert!(
        rows.iter()
            .all(|row| row.inclusion_reason == "service_instance")
    );
    assert!(
        rows.iter()
            .all(|row| row.resolver_status == ResolverStatus::Resolved)
    );
    assert!(rows.iter().all(|row| row.fallback_kind.is_none()));
}

#[test]
fn search_logs_for_service_instances_escapes_like_wildcards() {
    let (_dir, pool) = test_pool("service-instance-like-escape.db");
    insert_logs_batch(
        &pool,
        &[
            // `_` is a LIKE single-char wildcard; an unescaped pattern
            // `my_app/%` would match this row.
            syslog_row("2026-01-01T00:00:00Z", "tootie", "myxapp/x"),
            syslog_row("2026-01-01T00:01:00Z", "tootie", "my_app/x"),
        ],
    )
    .unwrap();
    let rows = search_logs_for_service_instances(
        &pool,
        &["tootie/my_app".to_string()],
        None,
        None,
        None,
        50,
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].entry.app_name.as_deref(), Some("my_app/x"));
}

#[test]
fn service_instance_fanout_arms_use_index_search_without_temp_btree() {
    let (_dir, pool) = test_pool("service-instance-plan.db");
    let conn = pool.get().unwrap();
    // Two-arm UNION ALL replica of the search_logs_for_service_instances
    // shape: every arm must be an index search (no `SCAN logs`) and no arm
    // may sort through a temp b-tree — LIMIT pushdown streams each arm off
    // idx_logs_host_time in timestamp order.
    let arm = "SELECT * FROM (SELECT l.id
          FROM logs l
         WHERE l.hostname = ? AND (l.app_name = ? OR l.app_name LIKE ? ESCAPE '\\' \
        OR json_extract(l.metadata_json, '$.agent_docker.compose_service') = ?)
         ORDER BY l.timestamp DESC, l.id DESC
         LIMIT ?)";
    let sql = format!("EXPLAIN QUERY PLAN {arm} UNION ALL {arm}");
    let plan: Vec<String> = conn
        .prepare(&sql)
        .unwrap()
        .query_map(
            rusqlite::params![
                "tootie", "plex", "plex/%", "plex", 100, "shart", "plex", "plex/%", "plex", 100
            ],
            |row| row.get::<_, String>(3),
        )
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|p| p.contains("USING INDEX idx_logs_host_time")),
        "arms must search idx_logs_host_time: {plan:?}"
    );
    assert!(
        !plan.iter().any(|p| p.starts_with("SCAN logs")),
        "no arm may full-scan logs: {plan:?}"
    );
    assert!(
        !plan.iter().any(|p| p.contains("TEMP B-TREE")),
        "no temp b-tree sort allowed: {plan:?}"
    );
}

#[test]
fn search_logs_for_service_instances_rejects_legacy_keys() {
    let (_dir, pool) = test_pool("service-instance-legacy-keys.db");
    insert_logs_batch(
        &pool,
        &[syslog_row("2026-01-01T00:00:00Z", "tootie", "plex")],
    )
    .unwrap();
    // Legacy shapes never split into (host, service) and yield no predicate.
    let rows = search_logs_for_service_instances(
        &pool,
        &["tootie:plex".to_string(), "plex/plex/plex".to_string()],
        None,
        None,
        None,
        50,
    )
    .unwrap();
    assert!(rows.is_empty());
}

#[test]
fn graph_walk_service_topic_traverses_proof_edges_and_caps_results() {
    let (_dir, pool) = test_pool("service-topic-walk.db");
    let conn = pool.get().unwrap();
    let logical = insert_entity(&conn, "logical_service", "plex");
    let instance = insert_entity(&conn, "service_instance", "tootie/plex");
    let host = insert_entity(&conn, "host", "tootie");
    let app = insert_entity(&conn, "app", "kernel");
    insert_rel(&conn, instance, logical, "instance_of");
    insert_rel(&conn, instance, host, "runs_on");
    // Broad log-identity edge from the host: must NOT be traversed.
    insert_rel(&conn, app, host, "emitted_by");

    let (entities, truncated) =
        graph::graph_walk_service_topic(&conn, &["plex".to_string()], 3).unwrap();
    let walked = keys(&entities);
    assert!(walked.contains(&"plex".to_string()));
    assert!(walked.contains(&"tootie/plex".to_string()));
    assert!(walked.contains(&"tootie".to_string()));
    assert!(
        !walked.contains(&"kernel".to_string()),
        "service-topic walk must not traverse broad log-identity edges: {walked:?}"
    );
    // Exact reach: seed + instance + host, and nothing else. (A `<= CAP`
    // assertion would be vacuous on a 4-node fixture.)
    assert_eq!(entities.len(), 3, "walk must reach exactly {walked:?}");
    assert!(!truncated, "small fixture must not report truncation");
}

#[test]
fn graph_walk_service_topic_reports_truncated_when_cap_hit() {
    let (_dir, pool) = test_pool("service-topic-walk-cap.db");
    let conn = pool.get().unwrap();
    let seed = insert_entity(&conn, "logical_service", "plex");
    // One more neighbor than GRAPH_SERVICE_TOPIC_ENTITY_CAP so the walk
    // (seed + neighbors) exceeds the cap and must report truncation.
    for i in 0..(graph::GRAPH_SERVICE_TOPIC_ENTITY_CAP + 1) {
        let neighbor = insert_entity(&conn, "host", &format!("host-{i}"));
        insert_rel(&conn, seed, neighbor, "runs_on");
    }

    let (entities, truncated) =
        graph::graph_walk_service_topic(&conn, &["plex".to_string()], 1).unwrap();
    assert_eq!(
        entities.len(),
        graph::GRAPH_SERVICE_TOPIC_ENTITY_CAP,
        "result must be capped at GRAPH_SERVICE_TOPIC_ENTITY_CAP"
    );
    assert!(
        truncated,
        "walk reaching more than the cap must report truncated=true"
    );
}

#[test]
fn ambiguous_prefix_candidates_surface_without_log_fanout() {
    let (_dir, pool) = test_pool("topic-ambiguous-prefix.db");
    insert_logs_batch(
        &pool,
        &[syslog_row(
            "2026-01-01T00:00:00Z",
            "tootie",
            "plexmediaserver",
        )],
    )
    .unwrap();
    {
        let conn = pool.get().unwrap();
        insert_entity(&conn, graph::ENTITY_TYPE_LOGICAL_SERVICE, "plexmediaserver");
    }
    // "plexmedia" prefix-matches logical_service:plexmediaserver: the
    // candidate must surface as ambiguous but contribute ZERO log fan-out.
    let inputs =
        topic_correlate_inputs(&pool, &["plexmedia".to_string()], 2, None, None, None, 100)
            .unwrap();
    let entity = inputs
        .resolved
        .iter()
        .find(|e| e.canonical_key == "plexmediaserver")
        .expect("prefix candidate must surface");
    assert_eq!(entity.match_kind, "prefix");
    assert_eq!(entity.resolver_status, ResolverStatus::Ambiguous);
    assert!(
        inputs.logs.is_empty(),
        "ambiguous candidates must contribute zero log fan-out"
    );
}

#[test]
fn service_instance_fanout_truncates_globally_newest_first_across_arms() {
    let (_dir, pool) = test_pool("service-instance-union-truncation.db");
    // Two instances on different hosts, limit + 2 rows each, sharing the
    // same timestamp set so the merge exercises the tie-break.
    let mut rows = Vec::new();
    for i in 0..8 {
        let ts = format!("2026-01-01T00:00:{i:02}Z");
        rows.push(syslog_row(&ts, "tootie", "plex"));
        rows.push(syslog_row(&ts, "shart", "plex"));
    }
    insert_logs_batch(&pool, &rows).unwrap();

    let limit = 6;
    let out = search_logs_for_service_instances(
        &pool,
        &["tootie/plex".to_string(), "shart/plex".to_string()],
        None,
        None,
        None,
        limit,
    )
    .unwrap();
    // UNION ALL arms each fetch up to `limit`; the Rust merge must truncate
    // to exactly `limit` rows globally.
    assert_eq!(out.len(), limit);
    // Global newest-first with the (timestamp DESC, id DESC) tie-break.
    for pair in out.windows(2) {
        let (a, b) = (&pair[0].entry, &pair[1].entry);
        assert!(
            a.timestamp > b.timestamp || (a.timestamp == b.timestamp && a.id > b.id),
            "rows must be (timestamp DESC, id DESC): {}#{} then {}#{}",
            a.timestamp,
            a.id,
            b.timestamp,
            b.id
        );
    }
    // The globally newest timestamp wins across both arms: both hosts'
    // :07 rows lead the merged result.
    assert!(out[0].entry.timestamp.ends_with(":07Z"));
    assert!(out[1].entry.timestamp.ends_with(":07Z"));
}

#[test]
fn mixed_case_hostname_does_not_match_canonical_instance_key() {
    let (_dir, pool) = test_pool("service-instance-case-miss.db");
    insert_logs_batch(
        &pool,
        &[syslog_row("2026-01-01T00:00:00Z", "Tootie", "plex")],
    )
    .unwrap();
    // Pins the case-sensitivity limitation: canonical keys are lowercase
    // and the log predicates compare with SQLite's default BINARY
    // collation, so a mixed-case syslog hostname ("Tootie") never matches
    // the canonical instance key ("tootie/plex"). Documented in
    // docs/contracts/investigation-graph.md; hostname case normalization
    // at ingest is tracked separately.
    let rows = search_logs_for_service_instances(
        &pool,
        &["tootie/plex".to_string()],
        None,
        None,
        None,
        50,
    )
    .unwrap();
    assert!(rows.is_empty(), "mixed-case hostname must miss (pinned)");
}

// -- syslog-mcp-csukc: resolve_topic_entities index-backed exact/prefix tier --

#[test]
fn glob_prefix_pattern_escapes_glob_metacharacters() {
    // '*', '?' and '[' are GLOB metacharacters; they must be wrapped in a
    // single-character bracket class so they match literally, not as
    // wildcards, before the trailing '*' prefix wildcard is appended.
    assert_eq!(super::glob_prefix_pattern("plex"), "plex*");
    assert_eq!(super::glob_prefix_pattern("a*b"), "a[*]b*");
    assert_eq!(super::glob_prefix_pattern("a?b"), "a[?]b*");
    assert_eq!(super::glob_prefix_pattern("a[b"), "a[[]b*");
    assert_eq!(super::glob_prefix_pattern("a*b?c[d"), "a[*]b[?]c[[]d*");
}

#[test]
fn resolve_topic_entities_exact_and_prefix_use_indexed_tier() {
    let (_dir, pool) = test_pool("resolve-topic-exact-prefix.db");
    let conn = pool.get().unwrap();
    insert_entity(&conn, graph::ENTITY_TYPE_HOST, "tootie");
    insert_entity(&conn, graph::ENTITY_TYPE_APP, "plex");
    insert_entity(&conn, graph::ENTITY_TYPE_APP, "plexmediaserver");

    let resolved = super::resolve_topic_entities(&conn, &["plex".to_string()]).unwrap();
    let exact = resolved
        .iter()
        .find(|e| e.canonical_key == "plex")
        .expect("exact canonical_key match must resolve");
    assert_eq!(exact.match_kind, "exact");
    assert_eq!(exact.resolver_status, ResolverStatus::Resolved);

    let prefix = resolved
        .iter()
        .find(|e| e.canonical_key == "plexmediaserver")
        .expect("prefix canonical_key match must resolve");
    assert_eq!(prefix.match_kind, "prefix");
    assert_eq!(prefix.resolver_status, ResolverStatus::Ambiguous);
}

#[test]
fn resolve_topic_entities_falls_back_to_label_substring_scan() {
    let (_dir, pool) = test_pool("resolve-topic-label-fallback.db");
    let conn = pool.get().unwrap();
    // canonical_key has no relationship to the search term at all; only the
    // human-readable display_label contains it as a mid-string substring.
    // This must still be found by the (unavoidable) label scan tier.
    conn.execute(
        "INSERT INTO graph_entities
            (entity_type, canonical_key, display_label, trust_level)
         VALUES (?1, ?2, ?3, 'verified')",
        rusqlite::params![
            graph::ENTITY_TYPE_APP,
            "backup-job-42",
            "nightly backup-job-42 (plex library)"
        ],
    )
    .unwrap();

    let resolved = super::resolve_topic_entities(&conn, &["plex".to_string()]).unwrap();
    let label = resolved
        .iter()
        .find(|e| e.canonical_key == "backup-job-42")
        .expect("label substring match must still resolve via fallback scan");
    assert_eq!(label.match_kind, "label");
    assert_eq!(label.resolver_status, ResolverStatus::Ambiguous);
}

#[test]
fn resolve_topic_entities_skips_label_scan_when_indexed_tier_fills_cap() {
    let (_dir, pool) = test_pool("resolve-topic-cap-skips-label.db");
    let conn = pool.get().unwrap();
    // 25 canonical_key prefix matches (== PER_TERM_CAP) plus one entity that
    // would only ever be found via the label substring scan. Once the
    // indexed tier alone fills the per-term cap, the label tier must not
    // contribute any additional candidates for this term.
    for i in 0..25 {
        conn.execute(
            "INSERT INTO graph_entities
                (entity_type, canonical_key, display_label, trust_level)
             VALUES (?1, ?2, ?2, 'verified')",
            rusqlite::params![graph::ENTITY_TYPE_APP, format!("svc-{i:02}")],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO graph_entities
            (entity_type, canonical_key, display_label, trust_level)
         VALUES (?1, ?2, ?3, 'verified')",
        rusqlite::params![
            graph::ENTITY_TYPE_APP,
            "unrelated-key",
            "totally unrelated but mentions svc- in passing"
        ],
    )
    .unwrap();

    let resolved = super::resolve_topic_entities(&conn, &["svc-".to_string()]).unwrap();
    assert_eq!(resolved.len(), 25, "per-term cap must still be honored");
    assert!(
        !resolved.iter().any(|e| e.canonical_key == "unrelated-key"),
        "label-only candidate must not appear once the indexed tier fills the cap: {:?}",
        resolved
    );
}
