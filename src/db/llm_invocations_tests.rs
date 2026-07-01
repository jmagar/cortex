use super::*;

fn test_conn() -> (
    r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let storage = crate::config::StorageConfig::for_test(db_path);
    let pool = crate::db::init_pool(&storage).unwrap();
    let conn = pool.get().unwrap();
    (conn, dir)
}

fn sample_params() -> LlmInvocationInsertParams {
    LlmInvocationInsertParams {
        caller_surface: "test".to_string(),
        action: "ai_assess".to_string(),
        provider: "gemini-cli".to_string(),
        model: Some("gemini-3.1-flash-lite-preview".to_string()),
        program: Some("gemini".to_string()),
        incident_id: Some("inc-42".to_string()),
        ai_tool: None,
        ai_project: Some("cortex".to_string()),
        ai_session_id: None,
        evidence_counts_json: Some(r#"{"total_incidents":1}"#.to_string()),
        prompt_bytes: Some(128),
        status: "running".to_string(),
        metadata_json: Some(r#"{"host":"dookie","pid":123}"#.to_string()),
    }
}

#[test]
fn insert_then_finish_round_trips() {
    let (conn, _dir) = test_conn();
    insert_llm_invocation_running(&conn, "llm-test-1", &sample_params()).unwrap();
    finish_llm_invocation(&conn, "llm-test-1", "success", None, 4200, Some(512)).unwrap();

    let rows = list_llm_invocations(&conn, 10, None, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, "llm-test-1");
    assert_eq!(row.status, "success");
    assert_eq!(row.duration_ms, Some(4200));
    assert_eq!(row.output_bytes, Some(512));
    assert_eq!(row.incident_id.as_deref(), Some("inc-42"));
    assert!(row.finished_at.is_some());
}

#[test]
fn list_filters_by_action_and_status_and_since() {
    let (conn, _dir) = test_conn();
    insert_llm_invocation_running(&conn, "llm-a", &sample_params()).unwrap();
    finish_llm_invocation(&conn, "llm-a", "success", None, 100, Some(10)).unwrap();

    let mut other = sample_params();
    other.action = "skill_assess".to_string();
    insert_llm_invocation_running(&conn, "llm-b", &other).unwrap();
    finish_llm_invocation(&conn, "llm-b", "error", Some("boom"), 50, None).unwrap();

    let ai_only = list_llm_invocations(&conn, 10, None, Some("ai_assess"), None).unwrap();
    assert_eq!(ai_only.len(), 1);
    assert_eq!(ai_only[0].id, "llm-a");

    let errors_only = list_llm_invocations(&conn, 10, None, None, Some("error")).unwrap();
    assert_eq!(errors_only.len(), 1);
    assert_eq!(errors_only[0].id, "llm-b");

    let future_since =
        list_llm_invocations(&conn, 10, Some("2999-01-01T00:00:00Z"), None, None).unwrap();
    assert!(future_since.is_empty());
}

#[test]
fn list_respects_limit_and_orders_newest_first() {
    let (conn, _dir) = test_conn();
    for i in 0..5 {
        let id = format!("llm-{i}");
        insert_llm_invocation_running(&conn, &id, &sample_params()).unwrap();
        finish_llm_invocation(&conn, &id, "success", None, 10, Some(1)).unwrap();
    }
    let rows = list_llm_invocations(&conn, 2, None, None, None).unwrap();
    assert_eq!(rows.len(), 2);
}
