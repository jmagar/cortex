use super::*;
use crate::config::StorageConfig;
use crate::db::{init_pool, insert_logs_batch, DbPool, LogBatchEntry};

fn test_storage_config(db_path: std::path::PathBuf) -> StorageConfig {
    StorageConfig::for_test(db_path)
}

/// Create an isolated test pool using a temp file (not :memory: — FTS5 needs file)
fn test_pool() -> (DbPool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let config = test_storage_config(db_path);
    let pool = init_pool(&config).unwrap();
    (pool, dir) // keep dir alive for test duration
}

fn make_entry(ts: &str, host: &str, severity: &str, msg: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: host.to_string(),
        facility: None,
        severity: severity.to_string(),
        app_name: None,
        process_id: None,
        message: msg.to_string(),
        raw: msg.to_string(),
        source_ip: "127.0.0.1:514".to_string(),
        docker_checkpoint: None,
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        ai_transcript_path: None,
        metadata_json: None,
    }
}

#[test]
fn test_search_fts() {
    let (pool, _dir) = test_pool();
    let entries = vec![
        make_entry(
            "2026-01-01T00:00:01Z",
            "host-a",
            "err",
            "disk full on /dev/sda",
        ),
        make_entry(
            "2026-01-01T00:00:02Z",
            "host-b",
            "info",
            "connection established",
        ),
    ];
    insert_logs_batch(&pool, &entries).unwrap();

    let params = SearchParams {
        query: Some("disk".to_string()),
        ..Default::default()
    };
    let results = search_logs(&pool, &params).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].message.contains("disk full"));
}

#[test]
fn test_search_invalid_fts_returns_error() {
    let (pool, _dir) = test_pool();
    // FTS5 treats bare parentheses as a syntax error
    let params = SearchParams {
        query: Some("(invalid fts syntax".to_string()),
        ..Default::default()
    };
    let result = search_logs(&pool, &params);
    assert!(result.is_err(), "invalid FTS5 query should return Err");
    // Error message must be generic — no schema details leaked
    let msg = result.unwrap_err().to_string();
    assert_eq!(msg, "Search query failed", "error must be generic");
}

// --- validate_fts_query unit tests ---

#[test]
fn test_validate_fts_query_valid() {
    assert!(validate_fts_query("disk error").is_ok());
    assert!(validate_fts_query("nginx AND 502").is_ok());
    // Exactly 16 terms should pass
    let sixteen = (0..16)
        .map(|i| format!("term{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(validate_fts_query(&sixteen).is_ok());
    // Exactly 512 chars should pass
    let at_limit = "a".repeat(512);
    assert!(validate_fts_query(&at_limit).is_ok());
}

#[test]
fn test_validate_fts_query_too_long() {
    let long_query = "a".repeat(513);
    let result = validate_fts_query(&long_query);
    assert!(result.is_err(), "query > 512 chars should be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("513"), "error should mention actual length");
    assert!(msg.contains("512"), "error should mention the limit");
}

#[test]
fn test_validate_fts_query_too_many_terms() {
    let many_terms = (0..17)
        .map(|i| format!("term{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let result = validate_fts_query(&many_terms);
    assert!(result.is_err(), "query with 17 terms should be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("17"), "error should mention actual term count");
    assert!(msg.contains("16"), "error should mention the limit");
}

#[test]
fn test_get_stats_empty_db() {
    let (pool, dir) = test_pool();
    let stats = get_stats(&pool, &test_storage_config(dir.path().join("test.db"))).unwrap();
    assert_eq!(stats.total_logs, 0);
    assert_eq!(stats.total_hosts, 0);
    // oldest_log and newest_log should be None on empty DB
    assert!(stats.oldest_log.is_none());
    assert!(stats.newest_log.is_none());
    assert!(stats.free_disk_mb.is_some());
}

#[test]
fn test_tail_filter_by_host() {
    let (pool, _dir) = test_pool();
    let entries = vec![
        make_entry("2026-01-01T00:00:01Z", "host-a", "info", "from a"),
        make_entry("2026-01-01T00:00:02Z", "host-b", "info", "from b"),
    ];
    insert_logs_batch(&pool, &entries).unwrap();

    let rows = tail_logs(&pool, Some("host-a"), None, None, None, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].hostname, "host-a");
}

#[test]
fn test_search_timestamp_range_filtering() {
    let (pool, _dir) = test_pool();
    let entries = vec![
        make_entry("2026-01-01T00:00:00Z", "host-a", "info", "early message"),
        make_entry("2026-06-15T12:00:00Z", "host-a", "info", "mid message"),
        make_entry("2026-12-31T23:59:59Z", "host-a", "info", "late message"),
    ];
    insert_logs_batch(&pool, &entries).unwrap();

    // from only
    let params = SearchParams {
        from: Some("2026-06-01T00:00:00Z".into()),
        ..Default::default()
    };
    let results = search_logs(&pool, &params).unwrap();
    assert_eq!(results.len(), 2, "from filter should return mid + late");

    // to only
    let params = SearchParams {
        to: Some("2026-06-30T00:00:00Z".into()),
        ..Default::default()
    };
    let results = search_logs(&pool, &params).unwrap();
    assert_eq!(results.len(), 2, "to filter should return early + mid");

    // from + to (narrow window)
    let params = SearchParams {
        from: Some("2026-06-01T00:00:00Z".into()),
        to: Some("2026-06-30T00:00:00Z".into()),
        ..Default::default()
    };
    let results = search_logs(&pool, &params).unwrap();
    assert_eq!(results.len(), 1, "from+to filter should return only mid");
    assert_eq!(results[0].message, "mid message");
}

#[test]
fn test_severity_to_num() {
    assert_eq!(severity_to_num("emerg"), Some(0));
    assert_eq!(severity_to_num("alert"), Some(1));
    assert_eq!(severity_to_num("crit"), Some(2));
    assert_eq!(severity_to_num("err"), Some(3));
    assert_eq!(severity_to_num("warning"), Some(4));
    assert_eq!(severity_to_num("notice"), Some(5));
    assert_eq!(severity_to_num("info"), Some(6));
    assert_eq!(severity_to_num("debug"), Some(7));
    // Edge cases
    assert_eq!(severity_to_num(""), None);
    assert_eq!(severity_to_num("ERROR"), None, "case sensitive");
    assert_eq!(severity_to_num("critical"), None, "not a valid syslog name");
    assert_eq!(
        severity_to_num("warn"),
        None,
        "must be 'warning' not 'warn'"
    );
}

#[test]
fn test_error_summary_severity_filter() {
    let (pool, _dir) = test_pool();
    let entries = vec![
        make_entry("2026-01-01T00:00:00Z", "host-a", "err", "error msg"),
        make_entry("2026-01-01T00:00:01Z", "host-a", "warning", "warn msg"),
        make_entry("2026-01-01T00:00:02Z", "host-a", "info", "info msg"),
        make_entry("2026-01-01T00:00:03Z", "host-a", "debug", "debug msg"),
    ];
    insert_logs_batch(&pool, &entries).unwrap();

    let summary = get_error_summary(&pool, None, None, false).unwrap();
    // Only err and warning should appear (not info, debug)
    assert_eq!(summary.len(), 2);
    let severities: Vec<&str> = summary.iter().map(|e| e.severity.as_str()).collect();
    assert!(severities.contains(&"err"));
    assert!(severities.contains(&"warning"));
}

#[test]
fn test_search_severity_in_filter() {
    let (pool, _dir) = test_pool();
    let entries = vec![
        make_entry("2026-01-01T00:00:00Z", "host-a", "emerg", "emerg msg"),
        make_entry("2026-01-01T00:00:01Z", "host-a", "err", "err msg"),
        make_entry("2026-01-01T00:00:02Z", "host-a", "warning", "warn msg"),
        make_entry("2026-01-01T00:00:03Z", "host-a", "info", "info msg"),
        make_entry("2026-01-01T00:00:04Z", "host-a", "debug", "debug msg"),
    ];
    insert_logs_batch(&pool, &entries).unwrap();

    let params = SearchParams {
        severity_in: Some(vec!["emerg".into(), "err".into(), "warning".into()]),
        ..Default::default()
    };
    let results = search_logs(&pool, &params).unwrap();
    assert_eq!(results.len(), 3, "severity_in should match exactly 3");
    for r in &results {
        assert!(
            ["emerg", "err", "warning"].contains(&r.severity.as_str()),
            "unexpected severity: {}",
            r.severity
        );
    }
}

#[test]
fn tail_logs_filters_multiple_severities() {
    let (pool, _dir) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            make_entry("2026-01-01T00:00:00Z", "host-a", "err", "err msg"),
            make_entry("2026-01-01T00:00:01Z", "host-a", "warning", "warn msg"),
            make_entry("2026-01-01T00:00:02Z", "host-a", "info", "info msg"),
        ],
    )
    .unwrap();

    let severities = vec!["err".to_string(), "warning".to_string()];
    let rows = tail_logs(&pool, Some("host-a"), None, None, Some(&severities), 10).unwrap();

    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.hostname == "host-a"));
    assert!(rows
        .iter()
        .all(|row| ["err", "warning"].contains(&row.severity.as_str())));
}

#[test]
fn search_logs_ignores_deleted_fts_phantom_rows() {
    let (pool, _dir) = test_pool();
    insert_logs_batch(
        &pool,
        &[make_entry(
            "2026-01-01T00:00:00Z",
            "host-a",
            "info",
            "live message",
        )],
    )
    .unwrap();

    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO logs_fts(rowid, message) VALUES (?1, ?2)",
        rusqlite::params![999_999_i64, "phantom-token orphan row"],
    )
    .unwrap();
    drop(conn);

    let params = SearchParams {
        query: Some("\"phantom-token\"".to_string()),
        ..Default::default()
    };
    let results = search_logs(&pool, &params).unwrap();
    assert!(results.is_empty(), "FTS-only phantom rows must not leak");
}

fn make_ai_entry(
    ts: &str,
    host: &str,
    tool: &str,
    project: &str,
    session_id: &str,
    message: &str,
) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: host.to_string(),
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
    }
}

#[test]
fn search_logs_exclude_ai_filters_structured_and_transcript_app_rows() {
    let (pool, _dir) = test_pool();
    let mut legacy_transcript = make_entry(
        "2026-01-01T00:00:00Z",
        "localhost",
        "info",
        "legacy codex transcript event",
    );
    legacy_transcript.app_name = Some("codex-transcript".into());

    insert_logs_batch(
        &pool,
        &[
            legacy_transcript,
            make_ai_entry(
                "2026-01-01T00:00:01Z",
                "localhost",
                "codex",
                "/tmp/project",
                "sess-1",
                "structured ai transcript event",
            ),
            make_entry(
                "2026-01-01T00:00:02Z",
                "host-a",
                "warning",
                "real host event",
            ),
        ],
    )
    .unwrap();

    let rows = search_logs(
        &pool,
        &SearchParams {
            query: Some("event".into()),
            exclude_ai: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].message, "real host event");
}

#[test]
fn search_ai_sessions_groups_results() {
    let (pool, _dir) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            make_ai_entry(
                "2026-01-01T00:00:00Z",
                "host-a",
                "claude",
                "/tmp/project",
                "sess-1",
                "authentication bug fixed",
            ),
            make_ai_entry(
                "2026-01-01T00:01:00Z",
                "host-a",
                "claude",
                "/tmp/project",
                "sess-1",
                "authentication tests passing",
            ),
            make_ai_entry(
                "2026-01-01T00:02:00Z",
                "host-a",
                "claude",
                "/tmp/project",
                "sess-1",
                "unmatched context",
            ),
        ],
    )
    .unwrap();

    let result = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "authentication".into(),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.sessions.len(), 1);
    assert_eq!(result.sessions[0].match_count, 2);
    assert_eq!(result.sessions[0].event_count, 3);
}

#[test]
fn search_ai_sessions_candidate_cap_prefers_newer_rows() {
    let (pool, _dir) = test_pool();
    let mut entries = Vec::new();
    for i in 0..=5000 {
        entries.push(make_ai_entry(
            &format!("2026-01-01T00:{:02}:00Z", i % 60),
            "host-a",
            "claude",
            "/tmp/old",
            &format!("old-{i}"),
            "commontoken",
        ));
    }
    entries.push(make_ai_entry(
        "2026-01-02T00:00:00Z",
        "host-a",
        "claude",
        "/tmp/new",
        "newest",
        "commontoken",
    ));
    insert_logs_batch(&pool, &entries).unwrap();

    let result = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "commontoken".into(),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();

    assert!(result.truncated);
    assert_eq!(result.candidate_cap, 5000);
    assert_eq!(result.candidate_rows, 5000);
    assert!(result.candidate_window_truncated);
    assert_eq!(result.sessions[0].ai_session_id, "newest");
    assert_eq!(result.sessions[0].ai_project, "/tmp/new");
}

#[test]
fn search_ai_sessions_zero_limit_clamps_to_one_with_metadata() {
    let (pool, _dir) = test_pool();
    insert_logs_batch(
        &pool,
        &[make_ai_entry(
            "2026-01-01T00:00:00Z",
            "host-a",
            "claude",
            "/tmp/project",
            "sess-1",
            "zerolimit",
        )],
    )
    .unwrap();

    let result = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "zerolimit".into(),
            limit: Some(0),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.sessions.len(), 1);
    assert_eq!(result.total_candidates, 1);
    assert_eq!(result.candidate_rows, 1);
    assert!(!result.truncated);
}

#[test]
fn search_ai_abuse_returns_same_session_context() {
    let (pool, _dir) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            make_ai_entry(
                "2026-01-01T00:00:00Z",
                "host-a",
                "codex",
                "/tmp/project",
                "sess-1",
                "before row",
            ),
            make_ai_entry(
                "2026-01-01T00:01:00Z",
                "host-a",
                "codex",
                "/tmp/project",
                "sess-1",
                "this shit needs context",
            ),
            make_ai_entry(
                "2026-01-01T00:02:00Z",
                "host-a",
                "codex",
                "/tmp/project",
                "sess-1",
                "after row",
            ),
            make_ai_entry(
                "2026-01-01T00:03:00Z",
                "host-a",
                "codex",
                "/tmp/project",
                "sess-2",
                "other session row",
            ),
            make_ai_entry(
                "2026-01-01T00:04:00Z",
                "host-a",
                "codex",
                "/tmp/project",
                "sess-1",
                "assistant is not a abuse false positive",
            ),
        ],
    )
    .unwrap();

    let result = search_ai_abuse(
        &pool,
        &AiAbuseParams {
            ai_project: Some("/tmp/project".into()),
            ai_tool: Some("codex".into()),
            limit: Some(10),
            before: Some(1),
            after: Some(1),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.matches.len(), 1);
    let hit = &result.matches[0];
    assert_eq!(hit.term, "shit");
    assert_eq!(hit.entry.message, "this shit needs context");
    assert_eq!(hit.before.len(), 1);
    assert_eq!(hit.before[0].message, "before row");
    assert_eq!(hit.after.len(), 1);
    assert_eq!(hit.after[0].message, "after row");
}

#[test]
fn search_ai_abuse_truncates_only_when_additional_match_exists() {
    let (pool, _dir) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            make_ai_entry(
                "2026-01-01T00:00:00Z",
                "host-a",
                "codex",
                "/tmp/project",
                "sess-1",
                "one shit",
            ),
            make_ai_entry(
                "2026-01-01T00:01:00Z",
                "host-a",
                "codex",
                "/tmp/project",
                "sess-1",
                "plain row",
            ),
        ],
    )
    .unwrap();

    let exact = search_ai_abuse(
        &pool,
        &AiAbuseParams {
            ai_project: Some("/tmp/project".into()),
            ai_tool: Some("codex".into()),
            limit: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(exact.matches.len(), 1);
    assert!(!exact.truncated);

    insert_logs_batch(
        &pool,
        &[make_ai_entry(
            "2026-01-01T00:02:00Z",
            "host-a",
            "codex",
            "/tmp/project",
            "sess-1",
            "two shit",
        )],
    )
    .unwrap();
    let truncated = search_ai_abuse(
        &pool,
        &AiAbuseParams {
            ai_project: Some("/tmp/project".into()),
            ai_tool: Some("codex".into()),
            limit: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(truncated.matches.len(), 1);
    assert!(truncated.truncated);
}

#[test]
fn ai_session_queries_respect_filters() {
    let (pool, _dir) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            make_ai_entry(
                "2026-01-01T00:00:00Z",
                "host-a",
                "claude",
                "/tmp/a",
                "s1",
                "auth needle",
            ),
            make_ai_entry(
                "2026-01-01T01:00:00Z",
                "host-b",
                "codex",
                "/tmp/b",
                "s2",
                "auth needle",
            ),
            make_ai_entry(
                "2026-01-02T00:00:00Z",
                "host-a",
                "claude",
                "/tmp/a",
                "s3",
                "auth needle",
            ),
        ],
    )
    .unwrap();

    let listed = list_ai_sessions(
        &pool,
        &ListAiSessionsParams {
            ai_project: Some("/tmp/a".into()),
            ai_tool: Some("claude".into()),
            hostname: Some("host-a".into()),
            from: Some("2026-01-01T00:00:00Z".into()),
            to: Some("2026-01-01T23:59:59Z".into()),
            limit: Some(10),
        },
    )
    .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].ai_session_id, "s1");

    let searched = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "needle".into(),
            ai_project: Some("/tmp/b".into()),
            ai_tool: Some("codex".into()),
            from: Some("2026-01-01T00:30:00Z".into()),
            to: Some("2026-01-01T01:30:00Z".into()),
            limit: Some(10),
        },
    )
    .unwrap();
    assert_eq!(searched.sessions.len(), 1);
    assert_eq!(searched.sessions[0].ai_session_id, "s2");
    assert_eq!(searched.sessions[0].hostname, "host-b");
}

#[test]
fn list_ai_tool_and_project_inventory() {
    let (pool, _dir) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            make_ai_entry(
                "2026-01-01T00:00:00Z",
                "host-a",
                "claude",
                "/tmp/a",
                "s1",
                "one",
            ),
            make_ai_entry(
                "2026-01-01T00:01:00Z",
                "host-a",
                "codex",
                "/tmp/b",
                "s2",
                "two",
            ),
            make_ai_entry(
                "2026-01-01T00:02:00Z",
                "host-a",
                "claude",
                "/tmp/a",
                "s1",
                "three",
            ),
        ],
    )
    .unwrap();

    let tools = list_ai_tools(&pool, &ListAiToolsParams::default()).unwrap();
    assert_eq!(tools.tools.len(), 2);
    let projects = list_ai_projects(
        &pool,
        &ListAiProjectsParams {
            ai_tool: Some("claude".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(projects.projects.len(), 1);
    assert_eq!(projects.projects[0].project, "/tmp/a");
}

#[test]
fn list_ai_inventory_reports_true_totals_and_truncation() {
    let (pool, _dir) = test_pool();
    let mut entries = Vec::new();
    for i in 0..201 {
        entries.push(make_ai_entry(
            "2026-01-01T00:00:00Z",
            "host-a",
            &format!("tool-{i:03}"),
            &format!("/tmp/project-{i:03}"),
            &format!("session-{i:03}"),
            "inventory",
        ));
    }
    insert_logs_batch(&pool, &entries).unwrap();

    let tools = list_ai_tools(&pool, &ListAiToolsParams::default()).unwrap();
    assert_eq!(tools.tools.len(), 100);
    assert_eq!(tools.total_tools, 201);
    assert!(tools.truncated);

    let projects = list_ai_projects(&pool, &ListAiProjectsParams::default()).unwrap();
    assert_eq!(projects.projects.len(), 200);
    assert_eq!(projects.total_projects, 201);
    assert!(projects.truncated);
}

#[test]
fn list_ai_sessions_groups_by_project_tool_session_and_hostname() {
    let (pool, _dir) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            LogBatchEntry {
                timestamp: "2026-05-11T00:00:00Z".into(),
                hostname: "dookie".into(),
                facility: Some("local7".into()),
                severity: "info".into(),
                app_name: Some("codex-transcript".into()),
                process_id: None,
                message: "{}".into(),
                raw: "{}".into(),
                source_ip: "10.0.0.1:514".into(),
                docker_checkpoint: None,
                ai_tool: Some("codex".into()),
                ai_project: Some("/home/jmagar/workspace/syslog-mcp".into()),
                ai_session_id: Some("abc".into()),
                ai_transcript_path: Some(
                    "/home/jmagar/.codex/sessions/2026/05/11/rollout-abc.jsonl".into(),
                ),
                metadata_json: None,
            },
            LogBatchEntry {
                timestamp: "2026-05-11T00:01:00Z".into(),
                hostname: "dookie".into(),
                facility: Some("local7".into()),
                severity: "info".into(),
                app_name: Some("codex-transcript".into()),
                process_id: None,
                message: "{}".into(),
                raw: "{}".into(),
                source_ip: "10.0.0.1:514".into(),
                docker_checkpoint: None,
                ai_tool: Some("codex".into()),
                ai_project: Some("/home/jmagar/workspace/syslog-mcp".into()),
                ai_session_id: Some("abc".into()),
                ai_transcript_path: Some(
                    "/home/jmagar/.codex/sessions/2026/05/11/rollout-abc.jsonl".into(),
                ),
                metadata_json: None,
            },
        ],
    )
    .unwrap();

    let rows = list_ai_sessions(
        &pool,
        &ListAiSessionsParams {
            ai_project: Some("/home/jmagar/workspace/syslog-mcp".into()),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ai_tool, "codex");
    assert_eq!(rows[0].ai_session_id, "abc");
    assert_eq!(rows[0].event_count, 2);
    assert_eq!(rows[0].first_seen, "2026-05-11T00:00:00Z");
    assert_eq!(rows[0].last_seen, "2026-05-11T00:01:00Z");
}
