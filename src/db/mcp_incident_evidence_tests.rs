use super::*;
use crate::config::StorageConfig;
use crate::db::mcp_incidents::AiMcpIncidentParams;
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
fn insert_mcp_event(
    pool: &DbPool,
    call_log_id: i64,
    ai_tool: &str,
    ai_project: &str,
    ai_session_id: &str,
    hostname: &str,
    timestamp: &str,
    call_id: &str,
    tool_name: &str,
    mcp_server: Option<&str>,
    mcp_tool: Option<&str>,
    is_error: Option<bool>,
) {
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO ai_mcp_events
            (call_log_id, ai_tool, ai_project, ai_session_id, hostname, timestamp,
             call_id, tool_name, mcp_server, mcp_tool, event_kind, is_error, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'call', ?11, ?6)",
        rusqlite::params![
            call_log_id,
            ai_tool,
            ai_project,
            ai_session_id,
            hostname,
            timestamp,
            call_id,
            tool_name,
            mcp_server,
            mcp_tool,
            is_error.map(i64::from),
        ],
    )
    .unwrap();
}

#[test]
fn investigate_ai_mcp_incidents_bundle_has_bounded_collections_and_truncation_flags() {
    let (pool, _dir) = test_pool();

    let call_log = make_ai_entry(
        "2026-01-01T00:00:00Z",
        "dookie",
        "codex",
        "/tmp/project-d",
        "sess-d",
        "called mcp__labby__search",
    );
    let correction_log = make_ai_entry(
        "2026-01-01T00:01:00Z",
        "dookie",
        "codex",
        "/tmp/project-d",
        "sess-d",
        "no, that's the wrong tool",
    );
    insert_logs_batch(&pool, &[call_log, correction_log]).unwrap();
    let log_ids: Vec<i64> = {
        let conn = pool.get().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM logs ORDER BY id ASC").unwrap();
        stmt.query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    insert_mcp_event(
        &pool,
        log_ids[0],
        "codex",
        "/tmp/project-d",
        "sess-d",
        "dookie",
        "2026-01-01T00:00:00Z",
        "call_d",
        "mcp__labby__search",
        Some("labby"),
        Some("search"),
        Some(false),
    );

    let result = investigate_ai_mcp_incidents(
        &pool,
        &AiMcpInvestigateParams {
            mcp_server: Some("labby".into()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.evidence.len(), 1);
    let bundle = &result.evidence[0];
    assert_eq!(bundle.incident.mcp_server, "labby");
    assert_eq!(bundle.mcp_events.len(), 1);
    assert!(!bundle.mcp_events_truncated);
    assert!(
        bundle
            .signal_anchors
            .iter()
            .any(|e| e.message.contains("wrong tool"))
    );
}

#[test]
fn investigate_ai_mcp_incidents_filters_by_incident_id() {
    let (pool, _dir) = test_pool();
    let call_log = make_ai_entry(
        "2026-01-01T00:00:00Z",
        "dookie",
        "claude",
        "/tmp/project-f",
        "sess-f",
        "called mcp__gh__search",
    );
    insert_logs_batch(&pool, &[call_log]).unwrap();
    let log_id: i64 = {
        let conn = pool.get().unwrap();
        conn.query_row("SELECT id FROM logs LIMIT 1", [], |row| row.get(0))
            .unwrap()
    };
    insert_mcp_event(
        &pool,
        log_id,
        "claude",
        "/tmp/project-f",
        "sess-f",
        "dookie",
        "2026-01-01T00:00:00Z",
        "call_f",
        "mcp__gh__search",
        Some("gh"),
        Some("search"),
        Some(false),
    );

    let all = search_ai_mcp_incidents(
        &pool,
        &AiMcpIncidentParams {
            mcp_server: Some("gh".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(all.incidents.len(), 1);
    let incident_id = all.incidents[0].incident_id.clone();

    let result = investigate_ai_mcp_incidents(
        &pool,
        &AiMcpInvestigateParams {
            incident_id: Some(incident_id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.evidence.len(), 1);
    assert_eq!(result.evidence[0].incident.incident_id, incident_id);

    let none_result = investigate_ai_mcp_incidents(
        &pool,
        &AiMcpInvestigateParams {
            incident_id: Some("mcp-inc-doesnotexist".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(none_result.evidence.is_empty());
}
