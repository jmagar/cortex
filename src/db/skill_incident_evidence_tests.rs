use super::*;
use crate::config::StorageConfig;
use crate::db::pool::init_pool;
use crate::db::skill_incidents::{AiSkillIncidentParams, search_ai_skill_incidents};
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

fn insert_skill_event(
    pool: &DbPool,
    log_id: i64,
    ai_tool: &str,
    ai_project: &str,
    ai_session_id: &str,
    hostname: &str,
    timestamp: &str,
    skill_name: &str,
    skill_plugin: Option<&str>,
) {
    // Note (PR 2 eng review sync): `skill_path`/`metadata_json` were dropped
    // from `ai_skill_events` before PR 2 shipped — neither extractor ever set
    // them. This INSERT reflects the shipped column set.
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO ai_skill_events
            (log_id, ai_tool, ai_project, ai_session_id, hostname, timestamp,
             skill_name, skill_plugin, event_kind, evidence_kind, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'skill_invoked', 'transcript', ?6)",
        rusqlite::params![
            log_id,
            ai_tool,
            ai_project,
            ai_session_id,
            hostname,
            timestamp,
            skill_name,
            skill_plugin,
        ],
    )
    .unwrap();
}

#[test]
fn investigate_ai_skill_incidents_bundle_has_bounded_collections_and_truncation_flags() {
    let (pool, _dir) = test_pool();

    let skill_log = make_ai_entry(
        "2026-01-01T00:00:00Z",
        "dookie",
        "codex",
        "/tmp/project-d",
        "sess-d",
        "loaded skill lavra:lavra-plan",
    );
    let before_log = make_ai_entry(
        "2026-01-01T00:00:00.000Z",
        "dookie",
        "codex",
        "/tmp/project-d",
        "sess-d",
        "user asked to plan the feature",
    );
    let correction_log = make_ai_entry(
        "2026-01-01T00:02:00Z",
        "dookie",
        "codex",
        "/tmp/project-d",
        "sess-d",
        "that's not what I asked, wrong file",
    );
    let failure_log = make_ai_entry(
        "2026-01-01T00:03:00Z",
        "dookie",
        "codex",
        "/tmp/project-d",
        "sess-d",
        "command exited with exit code 1",
    );
    insert_logs_batch(&pool, &[before_log, skill_log, correction_log, failure_log]).unwrap();

    let log_ids: Vec<i64> = {
        let conn = pool.get().unwrap();
        let mut stmt = conn
            .prepare("SELECT id FROM logs ORDER BY timestamp ASC, id ASC")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    // log_ids: [before, skill, correction, failure] in timestamp order.
    insert_skill_event(
        &pool,
        log_ids[1],
        "codex",
        "/tmp/project-d",
        "sess-d",
        "dookie",
        "2026-01-01T00:00:00Z",
        "lavra:lavra-plan",
        Some("lavra"),
    );

    let result = investigate_ai_skill_incidents(
        &pool,
        &AiSkillInvestigateParams {
            skill: Some("lavra:lavra-plan".into()),
            limit: Some(3),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.evidence.len(), 1);
    let bundle = &result.evidence[0];
    assert_eq!(bundle.incident.skill_name, "lavra:lavra-plan");
    assert!(!bundle.skill_events.is_empty());
    assert!(!bundle.skill_events_truncated);
    assert!(!bundle.signal_anchors.is_empty());
    assert!(!bundle.signal_anchors_truncated);
    // transcript_before should include the pre-skill "user asked to plan" row.
    assert!(
        bundle
            .transcript_before
            .iter()
            .any(|e| e.message.contains("user asked to plan"))
    );
    assert!(!bundle.transcript_before_truncated);
    assert!(!bundle.transcript_after_truncated);
    // The correction log should land in nearby_user_corrections; the failure
    // log should land in nearby_tool_failures.
    assert!(
        bundle
            .nearby_user_corrections
            .iter()
            .any(|e| e.message.contains("wrong file"))
    );
    assert!(
        bundle
            .nearby_tool_failures
            .iter()
            .any(|e| e.message.contains("exit code 1"))
    );
    assert!(!bundle.nearby_logs_truncated);
    assert!(!bundle.nearby_errors_truncated);
}

#[test]
fn investigate_ai_skill_incidents_exact_incident_id_can_target_outside_top_page() {
    let (pool, _dir) = test_pool();
    let mut entries = Vec::new();
    for i in 0..12 {
        entries.push(make_ai_entry(
            &format!("2026-01-01T00:{i:02}:00Z"),
            "host-a",
            "codex",
            "/tmp/project-e",
            &format!("sess-e-{i:02}"),
            "loaded skill lavra:lavra-plan",
        ));
    }
    insert_logs_batch(&pool, &entries).unwrap();
    let log_ids: Vec<i64> = {
        let conn = pool.get().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM logs ORDER BY id ASC").unwrap();
        stmt.query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    for (i, log_id) in log_ids.iter().enumerate() {
        insert_skill_event(
            &pool,
            *log_id,
            "codex",
            "/tmp/project-e",
            &format!("sess-e-{i:02}"),
            "host-a",
            &format!("2026-01-01T00:{i:02}:00Z"),
            "lavra:lavra-plan",
            Some("lavra"),
        );
    }

    let listed = search_ai_skill_incidents(
        &pool,
        &AiSkillIncidentParams {
            skill: Some("lavra:lavra-plan".into()),
            limit: Some(12),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(listed.incidents.len(), 12);
    let target_id = listed.incidents.last().unwrap().incident_id.clone();

    let top_page = investigate_ai_skill_incidents(
        &pool,
        &AiSkillInvestigateParams {
            skill: Some("lavra:lavra-plan".into()),
            limit: Some(3),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        !top_page
            .evidence
            .iter()
            .any(|b| b.incident.incident_id == target_id)
    );

    let exact = investigate_ai_skill_incidents(
        &pool,
        &AiSkillInvestigateParams {
            incident_id: Some(target_id.clone()),
            skill: Some("lavra:lavra-plan".into()),
            limit: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(exact.evidence.len(), 1);
    assert_eq!(exact.evidence[0].incident.incident_id, target_id);
}
