use super::*;

use crate::config::StorageConfig;

fn test_pool() -> (DbPool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("heartbeat-state.db"));
    let pool = crate::db::init_pool(&storage).unwrap();
    (pool, dir)
}

fn insert_heartbeat(
    pool: &DbPool,
    host_id: &str,
    hostname: &str,
    sequence: i64,
    sampled_at: &str,
    partial: bool,
) -> i64 {
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO host_heartbeats (
             host_id, hostname, source_ip, sampled_at, received_at, boot_id,
             uptime_secs, sequence, collection_ms, partial, agent_version,
             os, architecture, metadata_json
         ) VALUES (?1, ?2, '10.0.0.1:41000', ?4, ?4, 'boot-a', 60, ?3, 5, ?5,
                   '0.1.0', 'linux', 'x86_64', '{\"agent\":{\"interval_secs\":30}}')",
        params![host_id, hostname, sequence, sampled_at, partial as i64],
    )
    .unwrap();
    conn.last_insert_rowid()
}

#[test]
fn host_state_returns_latest_by_host_id() {
    let (pool, _dir) = test_pool();
    let older = insert_heartbeat(&pool, "host-a", "tootie", 1, "2026-05-25T00:00:00Z", false);
    let latest = insert_heartbeat(&pool, "host-a", "tootie", 2, "2026-05-25T00:01:00Z", true);
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO heartbeat_cpu (heartbeat_id, load1, load5, load15)
         VALUES (?1, 0.1, 0.2, 0.3)",
        [latest],
    )
    .unwrap();
    drop(conn);

    let state = heartbeat_host_state(
        &pool,
        HeartbeatHostLookup::HostId("host-a".into()),
        None,
        10,
    )
    .unwrap();
    assert_eq!(state.host_id, "host-a");
    assert_eq!(state.latest.as_ref().unwrap().heartbeat_id, latest);
    assert!(state.flags.collector_partial);
    assert_eq!(state.samples.len(), 2);
    assert!(state
        .samples
        .iter()
        .any(|sample| sample.heartbeat_id == older));
    assert!(state.latest.as_ref().unwrap().cpu.is_some());
}

#[test]
fn host_state_unique_hostname_fallback_and_ambiguous_hostname() {
    let (pool, _dir) = test_pool();
    insert_heartbeat(&pool, "host-a", "unique", 1, "2026-05-25T00:00:00Z", false);
    let state = heartbeat_host_state(
        &pool,
        HeartbeatHostLookup::Hostname("unique".into()),
        None,
        1,
    )
    .unwrap();
    assert_eq!(state.host_id, "host-a");

    insert_heartbeat(&pool, "host-b", "shared", 1, "2026-05-25T00:00:00Z", false);
    insert_heartbeat(&pool, "host-c", "shared", 1, "2026-05-25T00:01:00Z", false);
    let error = heartbeat_host_state(
        &pool,
        HeartbeatHostLookup::Hostname("shared".into()),
        None,
        1,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "ambiguous_host");
}

#[test]
fn host_state_caps_limit_and_filters_since() {
    let (pool, _dir) = test_pool();
    for sequence in 0..105 {
        insert_heartbeat(
            &pool,
            "host-a",
            "tootie",
            sequence,
            &format!("2026-05-25T00:{sequence:03}:00Z"),
            false,
        );
    }

    let capped = heartbeat_host_state(
        &pool,
        HeartbeatHostLookup::HostId("host-a".into()),
        None,
        500,
    )
    .unwrap();
    assert_eq!(capped.samples.len(), 100);
    assert!(capped.truncated);

    let since = heartbeat_host_state(
        &pool,
        HeartbeatHostLookup::HostId("host-a".into()),
        Some("2026-05-25T00:099:00Z"),
        100,
    )
    .unwrap();
    assert_eq!(since.samples.len(), 6);
}
