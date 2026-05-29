use std::sync::Arc;

use crate::config::StorageConfig;
use crate::db::{init_pool, insert_logs_batch, DbPool, LogBatchEntry};

use super::*;

fn test_service() -> (SyslogService, Arc<DbPool>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("app-test.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    (SyslogService::new(Arc::clone(&pool), storage), pool, dir)
}

fn entry(ts: &str, host: &str, severity: &str, msg: &str, source_ip: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: host.to_string(),
        facility: None,
        severity: severity.to_string(),
        app_name: None,
        process_id: None,
        message: msg.to_string(),
        raw: msg.to_string(),
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

fn ai_entry(ts: &str, msg: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: "localhost".into(),
        facility: Some("local0".into()),
        severity: "info".into(),
        app_name: Some("codex-transcript".into()),
        process_id: None,
        message: msg.to_string(),
        raw: msg.to_string(),
        source_ip: "transcript://codex".into(),
        docker_checkpoint: None,
        ai_tool: Some("codex".into()),
        ai_project: Some("/tmp/project".into()),
        ai_session_id: Some("sess-1".into()),
        ai_transcript_path: Some("/tmp/project/sess-1.jsonl".into()),
        metadata_json: None,
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    }
}

#[test]
fn normalize_syslog_owned_service_rejects_arbitrary_units() {
    assert_eq!(
        normalize_syslog_owned_service("syslog-ai-watch").unwrap(),
        "syslog-ai-watch.service"
    );

    let err = normalize_syslog_owned_service("ssh").unwrap_err();
    assert!(err.to_string().contains("unsupported syslog-owned service"));
}

#[test]
fn parse_journal_json_lines_extracts_service_log_fields() {
    let raw = r#"{"__REALTIME_TIMESTAMP":"1780000000123456","_SYSTEMD_USER_UNIT":"syslog-ai-watch.service","PRIORITY":"3","SYSLOG_IDENTIFIER":"syslog","_PID":"42","MESSAGE":"AI transcript indexing failed","__CURSOR":"cursor-1"}"#;

    let (entries, dropped) = parse_journal_json_lines(raw);
    assert_eq!(dropped, 0);

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].unit.as_deref(), Some("syslog-ai-watch.service"));
    assert_eq!(entries[0].priority.as_deref(), Some("3"));
    assert_eq!(entries[0].syslog_identifier.as_deref(), Some("syslog"));
    assert_eq!(entries[0].pid.as_deref(), Some("42"));
    assert_eq!(
        entries[0].message.as_deref(),
        Some("AI transcript indexing failed")
    );
    assert_eq!(entries[0].cursor.as_deref(), Some("cursor-1"));
    assert!(entries[0].timestamp.is_some());
}

#[tokio::test]
async fn incident_returns_ordered_db_events_for_window() {
    let (service, pool, _dir) = test_service();
    insert_logs_batch(
        &pool,
        &[
            entry(
                "2026-01-01T00:00:00.000Z",
                "host-a",
                "info",
                "before",
                "10.0.0.1:514",
            ),
            entry(
                "2026-01-01T00:05:00.000Z",
                "host-a",
                "err",
                "middle",
                "10.0.0.1:514",
            ),
            entry(
                "2026-01-01T00:11:00.000Z",
                "host-a",
                "warning",
                "after",
                "10.0.0.1:514",
            ),
        ],
    )
    .unwrap();

    let response = service
        .incident(IncidentRequest {
            around: "2026-01-01T00:05:00Z".into(),
            minutes: Some(5),
            service: None,
            hostname: Some("host-a".into()),
            limit: Some(10),
        })
        .await
        .unwrap();

    assert_eq!(response.window_from, "2026-01-01T00:00:00.000Z");
    assert_eq!(response.window_to, "2026-01-01T00:10:00.000Z");
    assert_eq!(response.events.len(), 2);
    assert_eq!(response.events[0].message, "before");
    assert_eq!(response.events[1].message, "middle");
}

#[tokio::test]
async fn correlate_events_normalizes_window_groups_and_truncates() {
    let (service, pool, _dir) = test_service();
    insert_logs_batch(
        &pool,
        &[
            entry(
                "2026-01-01T00:00:00+00:00",
                "host-a",
                "err",
                "disk full",
                "10.0.0.1:514",
            ),
            entry(
                "2026-01-01T00:01:00+00:00",
                "host-b",
                "warning",
                "service slow",
                "10.0.0.2:514",
            ),
            entry(
                "2026-01-01T00:02:00+00:00",
                "host-b",
                "info",
                "ignored info",
                "10.0.0.2:514",
            ),
        ],
    )
    .unwrap();

    let response = service
        .correlate_events(CorrelateEventsRequest {
            reference_time: "2026-01-01T01:00:00+01:00".into(),
            window_minutes: Some(2),
            severity_min: Some("warning".into()),
            hostname: None,
            source_ip: None,
            query: None,
            limit: Some(1),
        })
        .await
        .unwrap();

    assert_eq!(response.window_from, "2025-12-31T23:58:00.000Z");
    assert_eq!(response.window_to, "2026-01-01T00:02:00.000Z");
    assert!(response.truncated);
    assert_eq!(response.total_events, 1);
    assert_eq!(response.hosts_count, 1);
}

#[tokio::test]
async fn correlate_ai_logs_cross_references_non_ai_logs_only() {
    let (service, pool, _dir) = test_service();
    insert_logs_batch(
        &pool,
        &[
            ai_entry("2026-01-01T00:00:00Z", "debug deployment failure"),
            entry(
                "2026-01-01T00:01:00Z",
                "host-a",
                "err",
                "container failed during deployment",
                "10.0.0.1:514",
            ),
            entry(
                "2026-01-01T00:02:00Z",
                "host-a",
                "info",
                "filtered by severity",
                "10.0.0.1:514",
            ),
            ai_entry("2026-01-01T00:03:00Z", "ai row should not be related"),
        ],
    )
    .unwrap();

    let response = service
        .correlate_ai_logs(AiCorrelateRequest {
            project: Some("/tmp/project".into()),
            tool: Some("codex".into()),
            ai_query: Some("deployment".into()),
            log_query: Some("container".into()),
            window_minutes: Some(5),
            severity_min: Some("warning".into()),
            limit: Some(1),
            events_per_anchor: Some(5),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(response.total_anchors, 1);
    assert_eq!(response.total_related_events, 1);
    assert_eq!(
        response.anchors[0].related[0].message,
        "container failed during deployment"
    );
    assert!(response.anchors[0].related[0].ai_project.is_none());
}

#[tokio::test]
async fn correlate_ai_logs_batches_related_windows_with_per_anchor_caps() {
    let (service, pool, _dir) = test_service();
    insert_logs_batch(
        &pool,
        &[
            ai_entry("2026-01-01T00:00:00Z", "deploy failure near host-a"),
            entry(
                "2026-01-01T00:00:10Z",
                "host-a",
                "err",
                "deploy failed on host-a",
                "10.0.0.1:514",
            ),
            entry(
                "2026-01-01T00:00:20Z",
                "host-a",
                "warning",
                "deploy warning on host-a",
                "10.0.0.1:514",
            ),
            ai_entry("2026-01-01T00:10:00Z", "deploy failure near host-b"),
            entry(
                "2026-01-01T00:10:10Z",
                "host-b",
                "err",
                "deploy failed on host-b",
                "10.0.0.2:514",
            ),
        ],
    )
    .unwrap();

    let response = service
        .correlate_ai_logs(AiCorrelateRequest {
            project: Some("/tmp/project".into()),
            tool: Some("codex".into()),
            ai_query: Some("deploy".into()),
            log_query: Some("deploy".into()),
            window_minutes: Some(1),
            severity_min: Some("warning".into()),
            limit: Some(2),
            events_per_anchor: Some(1),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(response.total_anchors, 2);
    assert_eq!(response.related_limit_per_anchor, 1);
    assert_eq!(response.total_related_events, 2);
    let truncated_count = response
        .anchors
        .iter()
        .filter(|anchor| anchor.related_truncated)
        .count();
    assert_eq!(truncated_count, 1);
    assert!(response
        .anchors
        .iter()
        .all(|anchor| anchor.related.len() == 1));
}

#[tokio::test]
async fn correlate_ai_logs_rest_policy_reports_service_owned_clamp() {
    let (service, pool, _dir) = test_service();
    insert_logs_batch(
        &pool,
        &[
            ai_entry("2026-01-01T00:00:00Z", "deploy failure near host-a"),
            entry(
                "2026-01-01T00:00:10Z",
                "host-a",
                "err",
                "deploy failed on host-a",
                "10.0.0.1:514",
            ),
        ],
    )
    .unwrap();

    let response = service
        .correlate_ai_logs_with_limit_policy(
            AiCorrelateRequest {
                project: Some("/tmp/project".into()),
                tool: Some("codex".into()),
                ai_query: Some("deploy".into()),
                window_minutes: Some(1),
                limit: Some(1),
                events_per_anchor: Some(10_000),
                ..Default::default()
            },
            AiCorrelateLimitPolicy::REST,
        )
        .await
        .unwrap();

    assert_eq!(response.related_limit_per_anchor, 50);
    assert_eq!(response.events_per_anchor_clamped_to, Some(50));
}

#[tokio::test]
async fn source_ip_filter_uses_network_sender_identity() {
    let (service, pool, _dir) = test_service();
    insert_logs_batch(
        &pool,
        &[
            entry(
                "2026-01-01T00:00:00Z",
                "spoofed-host",
                "err",
                "from one",
                "10.0.0.1:514",
            ),
            entry(
                "2026-01-01T00:00:01Z",
                "spoofed-host",
                "err",
                "from two",
                "10.0.0.2:514",
            ),
        ],
    )
    .unwrap();

    let response = service
        .search_logs(SearchLogsRequest {
            source_ip: Some("10.0.0.2:514".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(response.count, 1);
    assert_eq!(response.logs[0].message, "from two");
}

#[tokio::test]
async fn search_logs_rejects_invalid_severity() {
    let (service, _pool, _dir) = test_service();

    let err = service
        .search_logs(SearchLogsRequest {
            severity: Some("bogus".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Invalid severity 'bogus'"));
    assert!(err.to_string().contains("emerg, alert, crit"));
}

#[tokio::test]
async fn filter_logs_maps_docker_stream_alias_to_source_prefix() {
    let (service, pool, _dir) = test_service();
    let mut stdout = entry(
        "2026-01-01T00:00:00Z",
        "dookie",
        "info",
        "docker stdout",
        "docker://dookie/syslog-mcp/stdout",
    );
    stdout.app_name = Some("syslog-mcp".into());
    let mut other = entry(
        "2026-01-01T00:00:01Z",
        "dookie",
        "info",
        "other stdout",
        "docker://dookie/other/stdout",
    );
    other.app_name = Some("other".into());
    insert_logs_batch(&pool, &[stdout, other]).unwrap();

    let response = service
        .filter_logs(FilterLogsRequest {
            source_kind: Some("docker-stream".into()),
            docker_host: Some("dookie".into()),
            container: Some("syslog-mcp".into()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(response.count, 1);
    assert_eq!(response.logs[0].message, "docker stdout");
}

#[tokio::test]
async fn filter_logs_rejects_queryless_json_only_source_kind() {
    let (service, _pool, _dir) = test_service();

    let err = service
        .filter_logs(FilterLogsRequest {
            source_kind: Some("otlp".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();

    assert!(err.to_string().contains("not indexed separately in v1"));
}

#[tokio::test]
async fn filter_logs_rejects_conflicting_source_kind_tool_alias() {
    let (service, _pool, _dir) = test_service();

    let err = service
        .filter_logs(FilterLogsRequest {
            source_kind: Some("claude".into()),
            tool: Some("codex".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();

    assert!(err
        .to_string()
        .contains("source_kind=claude conflicts with tool=codex"));
}

#[tokio::test]
async fn filter_logs_transcript_source_kind_excludes_agent_commands() {
    let (service, pool, _dir) = test_service();

    let transcript = ai_entry("2026-01-01T00:00:00Z", "transcript row");
    let mut agent_command = entry(
        "2026-01-01T00:00:01Z",
        "localhost",
        "info",
        "agent command row",
        "agent-command://localhost/codex/sess-1",
    );
    agent_command.ai_tool = Some("codex".into());
    agent_command.ai_project = Some("/tmp/project".into());
    agent_command.ai_session_id = Some("sess-1".into());

    insert_logs_batch(&pool, &[transcript, agent_command]).unwrap();

    let response = service
        .filter_logs(FilterLogsRequest {
            source_kind: Some("transcript".into()),
            tool: Some("codex".into()),
            project: Some("/tmp/project".into()),
            session_id: Some("sess-1".into()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(response.count, 1);
    assert_eq!(response.logs[0].message, "transcript row");
    assert!(response.logs[0].source_ip.starts_with("transcript://"));
}

#[tokio::test]
async fn health_check_runs_simple_database_query() {
    let (service, _pool, _dir) = test_service();

    service.health_check().await.unwrap();
}

#[tokio::test]
async fn ai_service_methods_return_seeded_data() {
    let (service, pool, _dir) = test_service();
    insert_logs_batch(
        &pool,
        &[LogBatchEntry {
            timestamp: "2026-01-01T00:00:00Z".into(),
            hostname: "host-a".into(),
            facility: Some("local0".into()),
            severity: "info".into(),
            app_name: Some("claude".into()),
            process_id: None,
            message: "authentication bug fixed".into(),
            raw: "authentication bug fixed".into(),
            source_ip: "127.0.0.1:514".into(),
            docker_checkpoint: None,
            ai_tool: Some("claude".into()),
            ai_project: Some("/tmp/project".into()),
            ai_session_id: Some("sess-1".into()),
            ai_transcript_path: Some("/tmp/project/session.jsonl".into()),
            metadata_json: None,
            http_status: None,
            auth_outcome: None,
            dns_blocked: None,
            event_action: None,
            parse_error: None,
        }],
    )
    .unwrap();

    let search = service
        .search_sessions(SearchSessionsRequest {
            query: "authentication".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(search.sessions.len(), 1);

    let tools = service
        .list_ai_tools(ListAiToolsRequest::default())
        .await
        .unwrap();
    assert_eq!(tools.tools[0].tool, "claude");
}

// `tracing_test::traced_test` captures TRACE-level events by default, so the
// `tracing::debug!` calls emitted by `run_db` are visible to `logs_contain`.
// We verify both the message tag and the structured timing fields are present.
#[tokio::test]
#[tracing_test::traced_test]
async fn run_db_emits_timing_trace_on_success() {
    let (service, _pool, _dir) = test_service();

    service.health_check().await.unwrap();

    assert!(logs_contain("db op ok"));
    assert!(logs_contain("op=\"health_check\""));
    assert!(logs_contain("permit_ms"));
    assert!(logs_contain("exec_ms"));
}

#[tokio::test]
#[tracing_test::traced_test]
async fn run_db_emits_warn_on_slow_op() {
    use std::time::Duration;
    let (service, _pool, _dir) = test_service();

    service
        .run_db("slow_test", |_pool| {
            std::thread::sleep(Duration::from_millis(SLOW_DB_MS as u64 + 50));
            Ok(())
        })
        .await
        .unwrap();

    // Slow ops escalate to WARN level; message stays "db op ok" so aggregators
    // can filter a single message across all speeds, using exec_ms for the threshold.
    assert!(logs_contain("WARN"));
    assert!(logs_contain("db op ok"));
    assert!(logs_contain("op=\"slow_test\""));
    assert!(logs_contain("permit_ms"));
    assert!(logs_contain("exec_ms"));
}

#[tokio::test]
#[tracing_test::traced_test]
async fn run_db_emits_warn_on_semaphore_closed() {
    let (service, _pool, _dir) = test_service();
    service.db_permits.close();

    let err = service.run_db("closed_test", |_| Ok(())).await.unwrap_err();

    assert!(
        matches!(err, ServiceError::Busy(_)),
        "expected Busy, got {err:?}"
    );
    assert!(logs_contain("db semaphore closed"));
    assert!(logs_contain("op=\"closed_test\""));
}

#[tokio::test]
#[tracing_test::traced_test]
async fn run_db_emits_warn_on_slow_op_with_error() {
    use std::time::Duration;
    let (service, _pool, _dir) = test_service();

    let _: ServiceResult<()> = service
        .run_db("slow_err_test", |_pool| {
            std::thread::sleep(Duration::from_millis(SLOW_DB_MS as u64 + 50));
            Err(anyhow::anyhow!("simulated slow failure"))
        })
        .await;

    assert!(logs_contain("WARN"));
    assert!(logs_contain("db op err"));
    assert!(logs_contain("op=\"slow_err_test\""));
    assert!(logs_contain("error="));
}

#[tokio::test]
async fn timeline_applies_default_lookback_only_when_from_and_to_both_absent() {
    // Bead dyqw: the bucket-sized default lookback was centralized into
    // `SyslogService::timeline`. It must apply ONLY when both `from` and `to`
    // are absent (preventing an unbounded full-table scan), and must be SKIPPED
    // whenever `to` is supplied — preserving the zl9y guard against injecting a
    // `from` that would create an impossible range.
    let (service, pool, _dir) = test_service();
    let now = chrono::Utc::now();
    let fmt = |dt: chrono::DateTime<chrono::Utc>| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let recent = fmt(now - chrono::Duration::days(2));
    let old = fmt(now - chrono::Duration::days(400));
    insert_logs_batch(
        &pool,
        &[
            entry(&recent, "host-a", "info", "recent", "10.0.0.1:514"),
            entry(&old, "host-a", "info", "old", "10.0.0.1:514"),
        ],
    )
    .unwrap();

    // Both absent → day bucket default (30 days) excludes the 400-day-old log.
    let resp = service
        .timeline(TimelineRequest {
            bucket: Some("day".into()),
            group_by: None,
            from: None,
            to: None,
            hostname: None,
            app_name: None,
            severity_min: None,
        })
        .await
        .unwrap();
    let total: i64 = resp.points.iter().map(|p| p.count).sum();
    assert_eq!(
        total, 1,
        "default 30-day window must exclude the 400-day-old log"
    );

    // `to` set (1 day ago), `from` absent → the default MUST be skipped. The
    // 400-day-old log predates any 30-day default window; it is counted only if
    // no default `from` was injected. If the guard regressed (default applied
    // whenever from is None), the range would be [now-30d, now-1d] and the old
    // log would drop, yielding 1 instead of 2.
    let to = fmt(now - chrono::Duration::days(1));
    let resp2 = service
        .timeline(TimelineRequest {
            bucket: Some("day".into()),
            group_by: None,
            from: None,
            to: Some(to),
            hostname: None,
            app_name: None,
            severity_min: None,
        })
        .await
        .unwrap();
    let total2: i64 = resp2.points.iter().map(|p| p.count).sum();
    assert_eq!(
        total2, 2,
        "with `to` set and `from` omitted, the default must be skipped so both logs (<= to) are counted"
    );
}
