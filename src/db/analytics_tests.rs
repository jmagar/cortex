use super::*;
use crate::app::error_detection::normalize::normalize_template;
use crate::config::StorageConfig;
use crate::db::{init_pool, insert_logs_batch, DbPool, LogBatchEntry};

fn test_pool() -> (DbPool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    (pool, dir)
}

fn entry(ts: &str, host: &str, severity: &str, app: Option<&str>, msg: &str) -> LogBatchEntry {
    entry_with_source_ip(ts, host, severity, app, msg, "127.0.0.1:514")
}

fn entry_with_source_ip(
    ts: &str,
    host: &str,
    severity: &str,
    app: Option<&str>,
    msg: &str,
    source_ip: &str,
) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: host.to_string(),
        facility: None,
        severity: severity.to_string(),
        app_name: app.map(String::from),
        process_id: None,
        message: msg.to_string(),
        raw: format!("<14>{ts} {host} {}: {msg}", app.unwrap_or("test")),
        source_ip: source_ip.to_string(),
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
fn template_normalises_numbers_ips_uuids() {
    let t = normalize_template(
        "connection refused from 10.0.0.5:42 (id b3a1c0de-1234-5678-9abc-def012345678)",
    );
    assert!(t.contains("<ip>:<n>"));
    assert!(t.contains("<uuid>"));
}

#[test]
fn template_preserves_non_ascii_codepoints() {
    // Multi-byte UTF-8 sequences must round-trip rather than getting split into
    // mojibake by `b as char`.
    let msg = "файл 1234 не найден";
    let t = normalize_template(msg);
    assert!(t.contains("файл"));
    assert!(t.contains("не найден"));
    assert!(t.contains("<n>"));
    assert!(t.is_char_boundary(t.len()));
}

#[test]
fn list_apps_returns_distinct_apps_with_counts() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry("2026-01-01T00:00:01Z", "h1", "info", Some("nginx"), "hello"),
            entry("2026-01-01T00:00:02Z", "h1", "info", Some("nginx"), "again"),
            entry(
                "2026-01-01T00:00:03Z",
                "h2",
                "info",
                Some("sshd"),
                "auth ok",
            ),
        ],
    )
    .unwrap();

    let apps = list_apps(
        &pool,
        &ListAppsParams {
            hostname: None,
            from: None,
            to: None,
            limit: 500,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(apps.total, 2);
    let nginx = apps.apps.iter().find(|a| a.app_name == "nginx").unwrap();
    assert_eq!(nginx.log_count, 2);
    assert_eq!(nginx.host_count, 1);

    // Filter by hostname
    let only_h2 = list_apps(
        &pool,
        &ListAppsParams {
            hostname: Some("h2"),
            from: None,
            to: None,
            limit: 500,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(only_h2.apps.len(), 1);
    assert_eq!(only_h2.apps[0].app_name, "sshd");
}

#[test]
fn unfiltered_list_apps_uses_inventory_stats_without_scanning_logs() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry("2026-01-01T00:00:01Z", "h1", "info", Some("nginx"), "hello"),
            entry("2026-01-01T00:00:02Z", "h2", "info", Some("nginx"), "again"),
            entry(
                "2026-01-01T00:00:03Z",
                "h2",
                "info",
                Some("sshd"),
                "auth ok",
            ),
        ],
    )
    .unwrap();
    pool.get()
        .unwrap()
        .execute(
            "UPDATE inventory_backfill_state
             SET completed_at = '2026-01-01T00:00:00Z'
             WHERE name = 'app_source_inventory'",
            [],
        )
        .unwrap();

    let conn = pool.get().unwrap();
    let plan = conn
        .prepare(
            "EXPLAIN QUERY PLAN
             WITH page AS (
                SELECT app_name, log_count, first_seen, last_seen
                FROM app_inventory_stats
                ORDER BY last_seen DESC, app_name ASC
                LIMIT 50 OFFSET 0
             )
             SELECT p.app_name, p.log_count, COUNT(h.hostname), p.first_seen, p.last_seen
             FROM page p
             LEFT JOIN app_host_inventory_stats h ON h.app_name = p.app_name
             GROUP BY p.app_name, p.log_count, p.first_seen, p.last_seen
             ORDER BY p.last_seen DESC, p.app_name ASC",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join("\n");
    assert!(
        !plan.contains("logs"),
        "unfiltered app inventory should not scan logs; got:\n{plan}"
    );
    drop(conn);

    let apps = list_apps(
        &pool,
        &ListAppsParams {
            hostname: None,
            from: None,
            to: None,
            limit: 50,
            offset: 0,
        },
    )
    .unwrap();
    let nginx = apps.apps.iter().find(|a| a.app_name == "nginx").unwrap();
    assert_eq!(nginx.log_count, 2);
    assert_eq!(nginx.host_count, 2);
}

#[test]
fn list_apps_to_filter_excludes_future_entries() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[entry(
            "2026-01-01T00:00:01Z",
            "h1",
            "info",
            Some("nginx"),
            "msg",
        )],
    )
    .unwrap();

    // All entries are inserted with received_at = now(). A `to` in the far past
    // should exclude them all; a `to` in the far future should include them.
    let none = list_apps(
        &pool,
        &ListAppsParams {
            hostname: None,
            from: None,
            to: Some("2000-01-01T00:00:00Z"),
            limit: 500,
            offset: 0,
        },
    )
    .unwrap();
    assert!(
        none.apps.is_empty(),
        "to=2000 should exclude all entries inserted now"
    );

    let all = list_apps(
        &pool,
        &ListAppsParams {
            hostname: None,
            from: None,
            to: Some("9999-01-01T00:00:00Z"),
            limit: 500,
            offset: 0,
        },
    )
    .unwrap();
    assert!(!all.apps.is_empty(), "to=9999 should include all entries");
}

#[test]
fn inventory_stats_decrement_when_logs_are_deleted() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry_with_source_ip(
                "2026-01-01T00:00:01Z",
                "h1",
                "info",
                Some("nginx"),
                "hello",
                "10.0.0.1:514",
            ),
            entry_with_source_ip(
                "2026-01-01T00:00:02Z",
                "h2",
                "info",
                Some("nginx"),
                "again",
                "10.0.0.1:514",
            ),
        ],
    )
    .unwrap();
    pool.get()
        .unwrap()
        .execute(
            "UPDATE inventory_backfill_state
             SET completed_at = '2026-01-01T00:00:00Z'
             WHERE name = 'app_source_inventory'",
            [],
        )
        .unwrap();

    let conn = pool.get().unwrap();
    conn.execute("DELETE FROM logs WHERE hostname = 'h1'", [])
        .unwrap();
    drop(conn);

    let apps = list_apps(
        &pool,
        &ListAppsParams {
            hostname: None,
            from: None,
            to: None,
            limit: 50,
            offset: 0,
        },
    )
    .unwrap();
    let nginx = apps.apps.iter().find(|a| a.app_name == "nginx").unwrap();
    assert_eq!(nginx.log_count, 1);
    assert_eq!(nginx.host_count, 1);

    let source_ips = list_source_ips(
        &pool,
        &ListSourceIpsParams {
            limit: 50,
            offset: 0,
        },
    )
    .unwrap();
    let ip = source_ips
        .source_ips
        .iter()
        .find(|entry| entry.source_ip == "10.0.0.1:514")
        .unwrap();
    assert_eq!(ip.log_count, 1);
    assert_eq!(ip.host_count, 1);
}

#[test]
fn list_source_ips_truncated_when_over_limit() {
    let (pool, _d) = test_pool();
    // Insert 3 entries with distinct source IPs; request limit=2 to force truncation.
    insert_logs_batch(
        &pool,
        &[
            entry_with_source_ip(
                "2026-01-01T00:00:01Z",
                "h1",
                "info",
                None,
                "a",
                "10.0.0.1:514",
            ),
            entry_with_source_ip(
                "2026-01-01T00:00:02Z",
                "h1",
                "info",
                None,
                "b",
                "10.0.0.2:514",
            ),
            entry_with_source_ip(
                "2026-01-01T00:00:03Z",
                "h1",
                "info",
                None,
                "c",
                "10.0.0.3:514",
            ),
        ],
    )
    .unwrap();

    let result = list_source_ips(
        &pool,
        &ListSourceIpsParams {
            limit: 2,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(result.total, 3, "total should reflect all 3 distinct IPs");
    assert_eq!(
        result.source_ips.len(),
        2,
        "page should contain only limit=2 IPs"
    );
}

#[test]
fn unfiltered_list_source_ips_uses_inventory_stats_without_scanning_logs() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry_with_source_ip("2026-01-01T00:00:01Z", "h1", "info", None, "a", "10.0.0.1"),
            entry_with_source_ip("2026-01-01T00:00:02Z", "h2", "info", None, "b", "10.0.0.1"),
            entry_with_source_ip("2026-01-01T00:00:03Z", "h3", "info", None, "c", "10.0.0.2"),
        ],
    )
    .unwrap();

    let conn = pool.get().unwrap();
    let plan = conn
        .prepare(
            "EXPLAIN QUERY PLAN
             WITH page AS (
                SELECT source_ip, log_count, first_seen, last_seen
                FROM source_ip_inventory_stats
                ORDER BY log_count DESC, source_ip ASC
                LIMIT 50 OFFSET 0
             )
             SELECT p.source_ip, p.log_count, p.first_seen, p.last_seen,
                    h.hostname, h.log_count, h.first_seen, h.last_seen
             FROM page p
             LEFT JOIN source_ip_host_inventory_stats h ON h.source_ip = p.source_ip
             ORDER BY p.log_count DESC, p.source_ip ASC, h.log_count DESC, h.hostname ASC",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join("\n");
    assert!(
        !plan.contains("logs"),
        "unfiltered source inventory should not scan logs; got:\n{plan}"
    );
    drop(conn);

    let result = list_source_ips(
        &pool,
        &ListSourceIpsParams {
            limit: 50,
            offset: 0,
        },
    )
    .unwrap();
    let ip = result
        .source_ips
        .iter()
        .find(|entry| entry.source_ip == "10.0.0.1")
        .unwrap();
    assert_eq!(ip.log_count, 2);
    assert_eq!(ip.host_count, 2);
}

#[test]
fn list_source_ips_chatty_ip_does_not_suppress_others() {
    // One IP with many hostnames must not crowd out other distinct IPs.
    let (pool, _d) = test_pool();
    let mut entries = vec![];
    // ip1 logs from 20 different hostnames
    for i in 0..20 {
        entries.push(entry_with_source_ip(
            "2026-01-01T00:00:01Z",
            &format!("host-{i}"),
            "info",
            None,
            "msg",
            "10.0.0.1:514",
        ));
    }
    // ip2 logs once
    entries.push(entry_with_source_ip(
        "2026-01-01T00:00:02Z",
        "h2",
        "info",
        None,
        "msg",
        "10.0.0.2:514",
    ));
    insert_logs_batch(&pool, &entries).unwrap();

    let result = list_source_ips(
        &pool,
        &ListSourceIpsParams {
            limit: 500,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(result.total, 2);
    assert!(
        result
            .source_ips
            .iter()
            .any(|e| e.source_ip == "10.0.0.2:514"),
        "ip2 must appear even though ip1 has many hostnames"
    );
}

#[test]
fn timeline_buckets_by_hour() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry("2026-01-01T00:10:00Z", "h1", "info", None, "a"),
            entry("2026-01-01T00:50:00Z", "h1", "info", None, "b"),
            entry("2026-01-01T01:05:00Z", "h1", "info", None, "c"),
        ],
    )
    .unwrap();
    let pts = timeline(
        &pool,
        Bucket::Hour,
        TimelineGroupBy::None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(pts.len(), 2);
    assert_eq!(pts[0].count, 2);
    assert_eq!(pts[1].count, 1);
}

#[test]
fn patterns_clusters_by_template() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry(
                "2026-01-01T00:00:01Z",
                "h1",
                "err",
                None,
                "disk 1234 failed",
            ),
            entry(
                "2026-01-01T00:00:02Z",
                "h1",
                "err",
                None,
                "disk 9999 failed",
            ),
            entry("2026-01-01T00:00:03Z", "h2", "err", None, "disk 5 failed"),
            entry("2026-01-01T00:00:04Z", "h1", "info", None, "all good"),
        ],
    )
    .unwrap();
    let (pats, scanned, truncated) =
        patterns(&pool, None, None, None, None, None, 100, 10).unwrap();
    assert!(!truncated);
    assert_eq!(scanned, 4);
    let top = &pats[0];
    assert_eq!(top.count, 3);
    assert_eq!(top.host_count, 2);
}

#[test]
fn fetch_log_by_id_returns_raw() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[entry("2026-01-01T00:00:01Z", "h1", "info", None, "hello")],
    )
    .unwrap();
    let row = fetch_log_by_id(&pool, 1).unwrap().unwrap();
    assert_eq!(row.message, "hello");
    assert!(row.raw.contains("hello"));
}

#[test]
fn context_around_returns_neighbours() {
    let (pool, _d) = test_pool();
    let mut entries = Vec::new();
    for i in 0..10 {
        entries.push(entry(
            &format!("2026-01-01T00:00:{:02}Z", i),
            "h1",
            "info",
            None,
            &format!("msg {i}"),
        ));
    }
    insert_logs_batch(&pool, &entries).unwrap();
    let r = ContextRef {
        id: Some(5),
        hostname: "h1".to_string(),
        timestamp: "2026-01-01T00:00:04Z".to_string(),
    };
    let (before, after) = context_around(&pool, &r, 3, 3).unwrap();
    assert_eq!(before.len(), 3);
    assert_eq!(after.len(), 3);
    assert!(before.last().unwrap().timestamp.as_str() < "2026-01-01T00:00:04Z");
    assert!(after.first().unwrap().timestamp.as_str() > "2026-01-01T00:00:04Z");
}

#[test]
fn context_timestamp_only_anchor_splits_symmetrically() {
    // Two rows share the exact reference timestamp; with id=None they must not
    // all land on one side. The before/after split is strict on `< ts` / `> ts`,
    // so simultaneous rows are excluded from both — consistent regardless of id ordering.
    let (pool, _d) = test_pool();
    let mut entries = Vec::new();
    for i in 0..5 {
        entries.push(entry(
            &format!("2026-01-01T00:00:{:02}Z", i),
            "h1",
            "info",
            None,
            "msg",
        ));
    }
    // Two rows at the exact reference time.
    entries.push(entry("2026-01-01T00:00:05Z", "h1", "info", None, "ref-a"));
    entries.push(entry("2026-01-01T00:00:05Z", "h1", "info", None, "ref-b"));
    for i in 6..10 {
        entries.push(entry(
            &format!("2026-01-01T00:00:{:02}Z", i),
            "h1",
            "info",
            None,
            "msg",
        ));
    }
    insert_logs_batch(&pool, &entries).unwrap();

    let r = ContextRef {
        id: None,
        hostname: "h1".to_string(),
        timestamp: "2026-01-01T00:00:05Z".to_string(),
    };
    let (before, after) = context_around(&pool, &r, 10, 10).unwrap();
    // 5 strictly-less timestamps, 4 strictly-greater. Neither contains a row at 05.
    assert_eq!(before.len(), 5);
    assert_eq!(after.len(), 4);
    assert!(before
        .iter()
        .all(|r| r.timestamp.as_str() < "2026-01-01T00:00:05Z"));
    assert!(after
        .iter()
        .all(|r| r.timestamp.as_str() > "2026-01-01T00:00:05Z"));
}

fn ai_entry(ts: &str, tool: &str, project: &str, session_id: &str, message: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: "host-a".to_string(),
        facility: Some("local0".to_string()),
        severity: "info".to_string(),
        app_name: Some("ai-transcript".to_string()),
        process_id: None,
        message: message.to_string(),
        raw: message.to_string(),
        source_ip: "127.0.0.1:514".to_string(),
        docker_checkpoint: None,
        ai_tool: Some(tool.to_string()),
        ai_project: Some(project.to_string()),
        ai_session_id: Some(session_id.to_string()),
        ai_transcript_path: Some(format!("{project}/{session_id}.jsonl")),
        metadata_json: None,
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    }
}

#[test]
fn usage_blocks_group_into_five_hour_windows() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            ai_entry(
                "2026-01-01T00:00:00Z",
                "claude",
                "/tmp/project",
                "sess-1",
                "one",
            ),
            ai_entry(
                "2026-01-01T04:59:59Z",
                "claude",
                "/tmp/project",
                "sess-1",
                "two",
            ),
            ai_entry(
                "2026-01-01T05:00:00Z",
                "claude",
                "/tmp/project",
                "sess-2",
                "three",
            ),
        ],
    )
    .unwrap();

    let result = get_ai_usage_blocks(
        &pool,
        &AiUsageBlocksParams {
            from: Some("2026-01-01T00:00:00Z".into()),
            to: Some("2026-01-01T06:00:00Z".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.blocks.len(), 2);
    assert_eq!(result.blocks[0].event_count, 1);
    assert_eq!(result.blocks[1].event_count, 2);
}

#[test]
fn usage_blocks_total_blocks_equals_len_when_truncated() {
    // When truncated, total_blocks == blocks.len() (the limit); truncated flag
    // is the authoritative indicator that more groups exist.
    let (pool, _d) = test_pool();
    let mut entries = Vec::new();
    for i in 0..1002 {
        entries.push(ai_entry(
            "2026-01-01T00:00:00Z",
            "claude",
            &format!("/tmp/project-{i}"),
            &format!("sess-{i}"),
            "usage block",
        ));
    }
    insert_logs_batch(&pool, &entries).unwrap();

    let result = get_ai_usage_blocks(
        &pool,
        &AiUsageBlocksParams {
            from: Some("2026-01-01T00:00:00Z".into()),
            to: Some("2026-07-31T00:00:00Z".into()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.blocks.len(), 1000);
    assert_eq!(result.total_blocks, 1000);
    assert!(result.truncated);
}

#[test]
fn project_context_returns_recent_entries() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            ai_entry(
                "2026-01-01T00:00:00Z",
                "claude",
                "/tmp/project",
                "sess-1",
                "one",
            ),
            ai_entry(
                "2026-01-01T00:01:00Z",
                "claude",
                "/tmp/project",
                "sess-2",
                "two",
            ),
        ],
    )
    .unwrap();
    let result = get_ai_project_context(
        &pool,
        &AiProjectContextParams {
            project: "/tmp/project".into(),
            limit: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.project, "/tmp/project");
    assert_eq!(result.event_count, 2);
    assert_eq!(result.recent_entries.len(), 1);
}

#[test]
fn project_context_snippets_are_bounded_to_256_chars() {
    let (pool, _d) = test_pool();
    let long_message = "a".repeat(300);
    insert_logs_batch(
        &pool,
        &[ai_entry(
            "2026-01-01T00:00:00Z",
            "claude",
            "/tmp/project",
            "sess-1",
            &long_message,
        )],
    )
    .unwrap();

    let result = get_ai_project_context(
        &pool,
        &AiProjectContextParams {
            project: "/tmp/project".into(),
            limit: Some(1),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.recent_entries[0].message.chars().count(), 256);
}

#[test]
fn clock_skew_plan_uses_received_at_range_index() {
    let (pool, _d) = test_pool();
    let conn = pool.get().unwrap();
    let sql = format!("EXPLAIN QUERY PLAN {CLOCK_SKEW_SQL}");
    let mut stmt = conn.prepare(&sql).unwrap();
    let rows = stmt
        .query_map(rusqlite::params!["2026-01-01T00:00:00Z", 100_i64], |row| {
            row.get::<_, String>(3)
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let plan = rows.join("\n");

    assert!(
        plan.contains("idx_logs_received_at"),
        "clock_skew must range-scan received_at; got:\n{plan}"
    );
    assert!(
        !plan.contains("SCAN logs USING INDEX idx_logs_hostname"),
        "clock_skew must not scan the hostname index for grouped recent windows; got:\n{plan}"
    );
}

#[test]
fn clock_skew_limits_hosts_in_skew_order() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry("2026-01-01T00:00:00Z", "h-low", "info", None, "low"),
            entry("2026-01-01T00:00:00Z", "h-high", "info", None, "high"),
            entry("2026-01-01T00:00:00Z", "h-mid", "info", None, "mid"),
        ],
    )
    .unwrap();

    let conn = pool.get().unwrap();
    for (host, received_at) in [
        ("h-low", "2026-01-01T00:00:10Z"),
        ("h-high", "2026-01-01T00:10:00Z"),
        ("h-mid", "2026-01-01T00:01:00Z"),
    ] {
        conn.execute(
            "UPDATE logs SET received_at = ?1 WHERE hostname = ?2",
            rusqlite::params![received_at, host],
        )
        .unwrap();
    }
    drop(conn);

    let result = clock_skew(&pool, "2026-01-01T00:00:00Z", Some(2)).unwrap();

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].hostname, "h-high");
    assert_eq!(result[1].hostname, "h-mid");
}

#[test]
fn summarize_range_counts_errors() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry("2026-01-01T00:00:01Z", "h1", "err", Some("a"), "x"),
            entry("2026-01-01T00:00:02Z", "h1", "info", Some("a"), "y"),
            entry("2026-01-01T00:00:03Z", "h2", "warning", Some("b"), "z"),
        ],
    )
    .unwrap();
    let summary = summarize_range(&pool, "2026-01-01T00:00:00Z", "2026-01-01T00:00:10Z").unwrap();
    assert_eq!(summary.total_logs, 3);
    assert_eq!(summary.total_errors, 2);
    assert_eq!(summary.top_apps.len(), 2);
}

#[test]
fn list_source_ips_aggregates_hostnames() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry_with_source_ip(
                "2026-01-01T00:00:01Z",
                "h1",
                "info",
                None,
                "x",
                "10.0.0.1:514",
            ),
            entry_with_source_ip(
                "2026-01-01T00:00:02Z",
                "h2",
                "info",
                None,
                "x",
                "10.0.0.1:514",
            ),
            entry_with_source_ip(
                "2026-01-01T00:00:03Z",
                "h2",
                "info",
                None,
                "x",
                "10.0.0.1:514",
            ),
            entry_with_source_ip(
                "2026-01-01T00:00:04Z",
                "h3",
                "info",
                None,
                "x",
                "10.0.0.2:514",
            ),
        ],
    )
    .unwrap();
    let result = list_source_ips(
        &pool,
        &ListSourceIpsParams {
            limit: 500,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(result.total, 2);
    let first = result
        .source_ips
        .iter()
        .find(|e| e.source_ip == "10.0.0.1:514")
        .unwrap();
    assert_eq!(first.host_count, 2);
    assert_eq!(first.log_count, 3);
}

#[test]
fn bucket_week_formats_correctly() {
    // Week bucket uses ISO week number format: "YYYY-WNN"
    assert_eq!(Bucket::Week.strftime_format(), "%Y-W%W");
    assert_eq!(Bucket::parse("week"), Some(Bucket::Week));
    assert_eq!(Bucket::parse("w"), Some(Bucket::Week));
}

#[test]
fn bucket_month_formats_correctly() {
    // Month bucket uses year-month format: "YYYY-MM"
    assert_eq!(Bucket::Month.strftime_format(), "%Y-%m");
    assert_eq!(Bucket::parse("month"), Some(Bucket::Month));
}

#[test]
fn bucket_default_lookback_days_scales_with_bucket_size() {
    assert!(Bucket::Minute.default_lookback_days() < Bucket::Hour.default_lookback_days());
    assert!(Bucket::Hour.default_lookback_days() < Bucket::Day.default_lookback_days());
    assert!(Bucket::Day.default_lookback_days() < Bucket::Week.default_lookback_days());
    assert!(Bucket::Week.default_lookback_days() < Bucket::Month.default_lookback_days());
    assert_eq!(Bucket::Week.default_lookback_days(), 180);
    assert_eq!(Bucket::Month.default_lookback_days(), 730);
}

#[test]
fn timeline_buckets_by_week() {
    let (pool, _d) = test_pool();
    // Two logs in the same week, one in a different week
    insert_logs_batch(
        &pool,
        &[
            entry("2026-01-05T00:00:00Z", "h1", "info", None, "a"), // week 1
            entry("2026-01-06T00:00:00Z", "h1", "info", None, "b"), // same week 1
            entry("2026-01-12T00:00:00Z", "h1", "info", None, "c"), // week 2
        ],
    )
    .unwrap();
    let pts = timeline(
        &pool,
        Bucket::Week,
        TimelineGroupBy::None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(pts.len(), 2);
    assert_eq!(pts[0].count, 2);
    assert_eq!(pts[1].count, 1);
    // Bucket labels should contain "W"
    assert!(
        pts[0].bucket.contains('W'),
        "week bucket label must contain 'W': {}",
        pts[0].bucket
    );
}

#[test]
fn timeline_buckets_by_month() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry("2026-01-01T00:00:00Z", "h1", "info", None, "a"),
            entry("2026-01-15T00:00:00Z", "h1", "info", None, "b"),
            entry("2026-02-01T00:00:00Z", "h1", "info", None, "c"),
        ],
    )
    .unwrap();
    let pts = timeline(
        &pool,
        Bucket::Month,
        TimelineGroupBy::None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(pts.len(), 2);
    assert_eq!(pts[0].count, 2);
    assert_eq!(pts[1].count, 1);
    // Bucket labels should look like "YYYY-MM"
    assert_eq!(pts[0].bucket, "2026-01");
    assert_eq!(pts[1].bucket, "2026-02");
}
