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
    }
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
            severity: Some("critical".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Invalid severity 'critical'"));
    assert!(err.to_string().contains("emerg, alert, crit"));
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
