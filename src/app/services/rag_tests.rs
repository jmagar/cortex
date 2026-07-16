use super::*;
use crate::config::StorageConfig;
use crate::db::{LogBatchEntry, init_pool, insert_logs_batch};
use std::sync::Arc;

fn test_service() -> (CortexService, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("rag-service-test.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    insert_logs_batch(
        &pool,
        &[
            app_log("memory pressure detected"),
            app_log("connection pressure detected"),
        ],
    )
    .unwrap();
    (CortexService::new(pool, storage), dir)
}

fn app_log(message: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: "2026-01-01T00:30:00Z".into(),
        hostname: "db-01".into(),
        facility: None,
        severity: "err".into(),
        app_name: Some("postgres".into()),
        process_id: None,
        message: message.into(),
        raw: message.into(),
        source_ip: "10.0.0.1:514".into(),
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

#[tokio::test]
async fn incident_context_forwards_query_to_db_filtering() {
    let (service, _dir) = test_service();
    let request = |query: &str| IncidentContextRequest {
        since: Some("2026-01-01T00:00:00Z".into()),
        until: Some("2026-01-01T01:00:00Z".into()),
        query: Some(query.into()),
        ..Default::default()
    };

    let memory = service.incident_context(request("memory")).await.unwrap();
    let connection = service
        .incident_context(request("connection"))
        .await
        .unwrap();

    assert_eq!(memory.error_logs.len(), 1);
    assert_eq!(memory.error_logs[0].message, "memory pressure detected");
    assert_eq!(connection.error_logs.len(), 1);
    assert_eq!(
        connection.error_logs[0].message,
        "connection pressure detected"
    );
}
