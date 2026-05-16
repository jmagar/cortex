//! End-to-end enrichment pipeline test.
//! Pushes fixture rows through EnrichmentPipeline::dispatch → insert_logs_batch
//! and asserts the enrichment columns appear in the DB.

use std::sync::Arc;

use syslog_mcp::config::StorageConfig;
use syslog_mcp::enrich::{EnrichmentPipeline, SourceKind};
use syslog_mcp::testing::{init_pool, insert_logs_batch, DbPool, LogBatchEntry};
use tempfile::TempDir;

/// Create an isolated SQLite pool backed by a temp directory.
/// Returns both the pool and the TempDir so the directory is not cleaned up
/// before the pool is dropped.
fn make_pool() -> (DbPool, TempDir) {
    let dir = TempDir::new().unwrap();
    let config = StorageConfig {
        db_path: dir.path().join("test.db"),
        pool_size: 1,
        retention_days: 0,
        wal_mode: false,
        max_db_size_mb: 0,
        recovery_db_size_mb: 0,
        min_free_disk_mb: 0,
        recovery_free_disk_mb: 0,
        cleanup_interval_secs: 60,
        cleanup_chunk_size: 1,
    };
    let pool = init_pool(&config).expect("test db pool should init");
    (pool, dir)
}

/// Build a `LogBatchEntry` with the source_kind and docker container_name
/// pre-stamped into metadata_json so the dispatcher can route it.
fn make_entry(
    app: Option<&str>,
    container: Option<&str>,
    message: &str,
    source_kind: SourceKind,
) -> LogBatchEntry {
    let metadata = if let Some(c) = container {
        format!(
            r#"{{"source_kind":"{}","docker":{{"container_name":"{}"}}}}"#,
            source_kind.as_str(),
            c
        )
    } else {
        format!(r#"{{"source_kind":"{}"}}"#, source_kind.as_str())
    };
    LogBatchEntry {
        timestamp: "2026-05-16T10:00:00Z".into(),
        hostname: "h".into(),
        facility: None,
        severity: "info".into(),
        app_name: app.map(str::to_string),
        process_id: None,
        message: message.into(),
        raw: message.into(),
        source_ip: "udp://127.0.0.1:514".into(),
        docker_checkpoint: None,
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        ai_transcript_path: None,
        metadata_json: Some(metadata),
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    }
}

#[test]
fn swag_row_lands_with_http_status() {
    let (pool, _dir) = make_pool();
    let pipeline = Arc::new(EnrichmentPipeline::new());

    // Routed via container_name "swag" → SwagParser.
    let mut entry = make_entry(
        Some("nginx"),
        Some("swag"),
        r#"192.0.2.55 - - [15/May/2026:14:22:11 +0000] "GET /api HTTP/1.1" 404 87 "-" "ua""#,
        SourceKind::DockerStream,
    );

    pipeline.dispatch(&mut entry);
    insert_logs_batch(&pool, &[entry]).unwrap();

    let conn = pool.get().unwrap();
    let (status, action): (Option<i32>, Option<String>) = conn
        .query_row(
            "SELECT http_status, event_action FROM logs LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, Some(404));
    assert_eq!(action.as_deref(), Some("http_request"));
}

#[test]
fn adguard_row_lands_with_dns_blocked() {
    let (pool, _dir) = make_pool();
    let pipeline = Arc::new(EnrichmentPipeline::new());

    // Routed via app_name "adguard-query" → AdguardParser.
    let mut entry = make_entry(
        Some("adguard-query"),
        None,
        r#"{"T":"2026-05-16T14:00:00Z","QH":"ads.example","QT":"A","Client":"192.168.0.10","Result":{"IsFiltered":true,"Reason":"FilteredBlackList"}}"#,
        SourceKind::AdguardApi,
    );

    pipeline.dispatch(&mut entry);
    insert_logs_batch(&pool, &[entry]).unwrap();

    let conn = pool.get().unwrap();
    let blocked: Option<i64> = conn
        .query_row("SELECT dns_blocked FROM logs LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(blocked, Some(1));
}

#[test]
fn unknown_source_writes_row_unchanged() {
    let (pool, _dir) = make_pool();
    let pipeline = Arc::new(EnrichmentPipeline::new());

    // No parser registered for "unknown-app" → no enrichment, no parse_error.
    let mut entry = make_entry(
        Some("unknown-app"),
        None,
        "random log line",
        SourceKind::SyslogTcp,
    );
    pipeline.dispatch(&mut entry);
    insert_logs_batch(&pool, &[entry]).unwrap();

    let conn = pool.get().unwrap();
    let (status, pe): (Option<i32>, Option<String>) = conn
        .query_row(
            "SELECT http_status, parse_error FROM logs LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, None);
    assert_eq!(pe, None);
}

#[test]
fn parser_failure_records_parse_error_but_persists_row() {
    let (pool, _dir) = make_pool();
    let pipeline = Arc::new(EnrichmentPipeline::new());

    // AdguardParser will fail to parse invalid JSON → sets parse_error.
    let mut entry = make_entry(
        Some("adguard-query"),
        None,
        "{ bad json",
        SourceKind::AdguardApi,
    );
    pipeline.dispatch(&mut entry);
    insert_logs_batch(&pool, &[entry]).unwrap();

    let conn = pool.get().unwrap();
    let (count, pe): (i64, Option<String>) = conn
        .query_row(
            "SELECT COUNT(*), MAX(parse_error) FROM logs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 1);
    assert!(
        pe.as_ref()
            .map(|s| s.starts_with("adguard:"))
            .unwrap_or(false),
        "expected parse_error to start with 'adguard:', got: {:?}",
        pe
    );
}
