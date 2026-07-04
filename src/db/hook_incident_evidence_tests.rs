use super::*;
use crate::config::StorageConfig;
use crate::db::pool::init_pool;
use crate::db::{DbPool, LogBatchEntry, insert_logs_batch};

fn test_pool() -> (DbPool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    (pool, dir)
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
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_hook_event_row(
    pool: &DbPool,
    log_id: Option<i64>,
    ai_tool: &str,
    ai_project: &str,
    ai_session_id: &str,
    hostname: &str,
    timestamp: &str,
    hook_event: &str,
    hook_name: &str,
    status: &str,
    evidence_kind: &str,
) {
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO ai_hook_events
            (log_id, ai_tool, ai_project, ai_session_id, hostname, timestamp,
             hook_event, hook_name, status, evidence_kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            log_id,
            ai_tool,
            ai_project,
            ai_session_id,
            hostname,
            timestamp,
            hook_event,
            hook_name,
            status,
            evidence_kind,
        ],
    )
    .unwrap();
}

#[test]
fn investigate_returns_bounded_evidence_bundle_with_findings_ready_data() {
    let (pool, _dir) = test_pool();
    let entries = vec![
        make_ai_entry(
            "2026-01-01T00:00:00.000Z",
            "dookie",
            "claude",
            "/home/jmagar/workspace/cortex",
            "sess-1",
            "starting work",
        ),
        make_ai_entry(
            "2026-01-01T00:00:10.000Z",
            "dookie",
            "claude",
            "/home/jmagar/workspace/cortex",
            "sess-1",
            "after hook context, exit code nonzero",
        ),
    ];
    insert_logs_batch(&pool, &entries).unwrap();
    let log_ids: Vec<i64> = {
        let conn = pool.get().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM logs ORDER BY id ASC").unwrap();
        stmt.query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };

    insert_hook_event_row(
        &pool,
        Some(log_ids[0]),
        "claude",
        "/home/jmagar/workspace/cortex",
        "sess-1",
        "dookie",
        "2026-01-01T00:00:05.000Z",
        "PostToolUse",
        "format-on-save",
        "failed",
        "runtime_transcript",
    );

    let result = investigate_ai_hook_incidents(
        &pool,
        &AiHookInvestigateParams {
            hook_name: Some("format-on-save".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.evidence.len(), 1);
    let bundle = &result.evidence[0];
    assert_eq!(bundle.incident.hook_name.as_deref(), Some("format-on-save"));
    assert_eq!(bundle.hook_events.len(), 1);
    assert!(!bundle.hook_events_truncated);
    assert!(!bundle.transcript_before.is_empty() || !bundle.transcript_after.is_empty());
}

#[test]
fn investigate_by_incident_id_narrows_to_one() {
    let (pool, _dir) = test_pool();
    insert_hook_event_row(
        &pool,
        None,
        "claude",
        "/home/jmagar/workspace/cortex",
        "sess-a",
        "dookie",
        "2026-01-01T00:00:00.000Z",
        "PostToolUse",
        "hook-a",
        "success",
        "runtime_transcript",
    );
    insert_hook_event_row(
        &pool,
        None,
        "claude",
        "/home/jmagar/workspace/cortex",
        "sess-b",
        "dookie",
        "2026-01-01T01:00:00.000Z",
        "PostToolUse",
        "hook-b",
        "failed",
        "runtime_transcript",
    );

    let all = search_ai_hook_incidents(&pool, &AiHookIncidentParams::default()).unwrap();
    assert_eq!(all.incidents.len(), 2);
    let target_id = all.incidents[0].incident_id.clone();

    let result = investigate_ai_hook_incidents(
        &pool,
        &AiHookInvestigateParams {
            incident_id: Some(target_id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.evidence.len(), 1);
    assert_eq!(result.evidence[0].incident.incident_id, target_id);
}

#[test]
fn investigate_with_no_matching_hook_returns_empty_evidence() {
    let (pool, _dir) = test_pool();
    let result = investigate_ai_hook_incidents(
        &pool,
        &AiHookInvestigateParams {
            hook_name: Some("nonexistent-hook".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(result.evidence.is_empty());
    assert_eq!(result.total_incidents, 0);
}
