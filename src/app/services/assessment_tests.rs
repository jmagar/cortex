use std::sync::Arc;

use crate::config::StorageConfig;
use crate::db::{DbPool, init_pool};

use super::*;
use crate::app::models::AbuseAssessRequest;

fn test_service() -> (CortexService, Arc<DbPool>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("assess-abuse-test.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    (CortexService::new(Arc::clone(&pool), storage), pool, dir)
}

#[tokio::test]
async fn assess_top_abuse_incident_errors_when_no_incidents_match() {
    let (service, _pool, _dir) = test_service();
    let req = AbuseAssessRequest {
        incident_id: None,
        model: None,
        project: Some("no-such-project-xyz".to_string()),
        tool: None,
        since: None,
        until: None,
        window_minutes: None,
        correlation_window_minutes: None,
        terms: vec![],
        limit: None,
    };
    let err = service
        .assess_top_abuse_incident_with_delta(req, false, |_| Ok(()))
        .await
        .unwrap_err();
    assert!(format!("{err}").contains("no abuse incident found"));
}

#[tokio::test]
async fn assess_top_abuse_incident_with_explicit_incident_id_bypasses_autopick() {
    let (service, _pool, _dir) = test_service();
    let req = AbuseAssessRequest {
        incident_id: Some("definitely-not-a-real-incident-id".to_string()),
        model: None,
        project: None,
        tool: None,
        since: None,
        until: None,
        window_minutes: None,
        correlation_window_minutes: None,
        terms: vec![],
        limit: None,
    };
    let err = service
        .assess_top_abuse_incident_with_delta(req, false, |_| Ok(()))
        .await
        .unwrap_err();
    assert!(format!("{err}").contains("no incident found with id"));
}

#[tokio::test]
async fn assess_top_abuse_incident_run_llm_false_writes_no_llm_invocation_row() {
    let (service, pool, _dir) = test_service();
    let req = AbuseAssessRequest {
        incident_id: None,
        model: None,
        project: None,
        tool: None,
        since: None,
        until: None,
        window_minutes: None,
        correlation_window_minutes: None,
        terms: vec![],
        limit: None,
    };
    let _ = service
        .assess_top_abuse_incident_with_delta(req, false, |_| Ok(()))
        .await;
    let conn = pool.get().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM llm_invocations WHERE action = 'ai_assess'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 0,
        "run_llm=false must never write an llm_invocations row (LlmRunner::run must not be called)"
    );
}
