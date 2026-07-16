use super::*;
use crate::config::StorageConfig;
use crate::db::init_pool;
use std::sync::Arc;

fn test_service() -> (CortexService, Arc<db::DbPool>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("logs-service-test.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    (CortexService::new(Arc::clone(&pool), storage), pool, dir)
}

fn insert_heartbeat(
    pool: &db::DbPool,
    host_id: &str,
    hostname: &str,
    sequence: i64,
    sampled_at: &str,
) -> i64 {
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO host_heartbeats (
             host_id, hostname, source_ip, sampled_at, received_at, boot_id,
             uptime_secs, sequence, collection_ms, partial, agent_version,
             os, architecture, metadata_json
         ) VALUES (?1, ?2, '10.0.0.1:41000', ?4, ?4, ?1,
                   60, ?3, 5, 0, '0.1.0', 'linux', 'x86_64', '{}')",
        rusqlite::params![host_id, hostname, sequence, sampled_at],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn seed_latest(
    pool: &db::DbPool,
    host_id: &str,
    heartbeat_id: i64,
    hostname: &str,
    sampled_at: &str,
) {
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO host_heartbeats_latest
             (host_id, heartbeat_id, hostname, sampled_at, received_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        rusqlite::params![host_id, heartbeat_id, hostname, sampled_at],
    )
    .unwrap();
}

fn insert_error_log(pool: &db::DbPool, hostname: &str, severity: &str, timestamp: &str) {
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO logs
             (timestamp, received_at, hostname, severity, app_name, message, raw, source_ip)
         VALUES (?1, ?1, ?2, ?3, 'app', 'boom', 'boom', '10.0.0.1:514')",
        rusqlite::params![timestamp, hostname, severity],
    )
    .unwrap();
}

// Regression: with only `until` supplied (no `since`), the last-hour default
// must anchor to `until` (until - 1h). A pre-fix build suppressed the default
// and scanned from the epoch, so the out-of-window row leaked into the summary.
#[tokio::test]
async fn get_errors_until_only_bounds_window_to_one_hour_before_until() {
    let (service, pool, _dir) = test_service();
    // Two hours before `until` — outside the 1h window, must be excluded.
    insert_error_log(&pool, "old-host", "err", "2026-01-01T10:00:00Z");
    // Half an hour before `until` — inside the 1h window, must be included.
    insert_error_log(&pool, "recent-host", "err", "2026-01-01T11:30:00Z");

    let response = service
        .get_errors(models::GetErrorsRequest {
            since: None,
            until: Some("2026-01-01T12:00:00Z".to_string()),
            group_by: None,
            limit: None,
        })
        .await
        .unwrap();

    assert_eq!(
        response.summary.len(),
        1,
        "only the in-window row should be summarized; got {:?}",
        response.summary
    );
    assert_eq!(response.summary[0].hostname, "recent-host");
    assert_eq!(response.summary[0].count, 1);
}

#[tokio::test]
async fn host_state_default_uses_authoritative_freshest_heartbeat() {
    let (service, pool, _dir) = test_service();
    let projected = insert_heartbeat(
        &pool,
        "projected-host",
        "projected",
        1,
        "2026-01-01T02:00:00Z",
    );
    seed_latest(
        &pool,
        "projected-host",
        projected,
        "projected",
        "2026-01-01T02:00:00Z",
    );
    insert_heartbeat(&pool, "fresh-host", "fresh", 1, "2026-01-01T03:00:00Z");

    let response = service
        .host_state(models::HostStateRequest::default())
        .await
        .unwrap()
        .expect("authoritative heartbeat exists");

    assert_eq!(response.host_id, "fresh-host");
    assert_eq!(response.hostname, "fresh");
}
