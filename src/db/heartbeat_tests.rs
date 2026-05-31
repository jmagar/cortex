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

/// Populate `host_heartbeats_latest` as the ingest path would.
fn seed_latest(
    pool: &DbPool,
    host_id: &str,
    heartbeat_id: i64,
    hostname: &str,
    sampled_at: &str,
    partial: bool,
) {
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO host_heartbeats_latest
             (host_id, heartbeat_id, hostname, sampled_at, received_at,
              partial, agent_version, os, architecture, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?4, ?5, '0.1.0', 'linux', 'x86_64',
                 '{\"agent\":{\"interval_secs\":30}}')
         ON CONFLICT(host_id) DO UPDATE SET
             heartbeat_id  = excluded.heartbeat_id,
             hostname      = excluded.hostname,
             sampled_at    = excluded.sampled_at,
             received_at   = excluded.received_at,
             partial       = excluded.partial,
             agent_version = excluded.agent_version,
             os            = excluded.os,
             architecture  = excluded.architecture,
             metadata_json = excluded.metadata_json
         WHERE excluded.sampled_at >= host_heartbeats_latest.sampled_at",
        params![host_id, heartbeat_id, hostname, sampled_at, partial as i64],
    )
    .unwrap();
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

// ── Fleet-state cache tests ───────────────────────────────────────────────

/// `heartbeat_latest_all` must use SCAN on the small cache table, not the
/// main `host_heartbeats` table. Verified via EXPLAIN QUERY PLAN.
#[test]
fn fleet_state_explain_does_not_scan_main_table() {
    let (pool, _dir) = test_pool();
    // Seed the cache with two hosts; main table is intentionally empty for
    // this test (the cache is populated by the ingest path, or migration 19).
    seed_latest(&pool, "host-a", 1, "tootie", "2026-05-25T00:01:00Z", false);
    seed_latest(&pool, "host-b", 2, "dookie", "2026-05-25T00:01:00Z", false);

    let conn = pool.get().unwrap();
    let plan: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "EXPLAIN QUERY PLAN
                 SELECT host_id, heartbeat_id, hostname, sampled_at, received_at,
                        partial, metadata_json
                 FROM host_heartbeats_latest
                 ORDER BY hostname ASC",
            )
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };

    let plan_text = plan.join("\n").to_lowercase();
    // Must see the cache table in the plan, not the main table.
    assert!(
        plan_text.contains("host_heartbeats_latest"),
        "EXPLAIN must reference host_heartbeats_latest; got: {plan_text}"
    );
    assert!(
        !plan_text.contains("scan host_heartbeats\n")
            && !plan_text.contains("scan host_heartbeats "),
        "EXPLAIN must NOT scan host_heartbeats; got: {plan_text}"
    );
}

#[test]
fn heartbeat_latest_all_returns_one_row_per_host_ordered_by_hostname() {
    let (pool, _dir) = test_pool();
    seed_latest(&pool, "host-b", 2, "zebra", "2026-05-25T00:01:00Z", false);
    seed_latest(&pool, "host-a", 1, "alpha", "2026-05-25T00:00:00Z", true);
    seed_latest(&pool, "host-c", 3, "midway", "2026-05-25T00:02:00Z", false);

    let entries = heartbeat_latest_all(&pool).unwrap();
    assert_eq!(entries.len(), 3);
    // Verify hostname ordering.
    assert_eq!(entries[0].hostname, "alpha");
    assert_eq!(entries[1].hostname, "midway");
    assert_eq!(entries[2].hostname, "zebra");
    // Partial flag is preserved.
    assert!(entries[0].partial);
    assert!(!entries[1].partial);
}

#[test]
fn cache_upsert_only_advances_on_newer_sampled_at() {
    let (pool, _dir) = test_pool();
    seed_latest(&pool, "host-a", 1, "tootie", "2026-05-25T00:01:00Z", false);
    // "Older" heartbeat: sampled_at is earlier, should NOT overwrite.
    seed_latest(&pool, "host-a", 99, "tootie", "2026-05-24T00:00:00Z", true);

    let entries = heartbeat_latest_all(&pool).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].heartbeat_id, 1,
        "older heartbeat must not overwrite newer cache entry"
    );
    assert!(
        !entries[0].partial,
        "partial flag must not be overwritten by older entry"
    );
}

#[test]
fn heartbeat_metric_snapshot_returns_aggregates() {
    let (pool, _dir) = test_pool();
    let hb_id = insert_heartbeat(&pool, "host-a", "tootie", 1, "2026-05-25T00:00:00Z", false);
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO heartbeat_cpu (heartbeat_id, load1, load5, load15, usage_percent)
         VALUES (?1, 1.0, 1.5, 2.0, 91.5)",
        [hb_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO heartbeat_memory
             (heartbeat_id, total_bytes, available_bytes, used_percent,
              swap_total_bytes, swap_used_bytes)
         VALUES (?1, 8000000000, 500000000, 87.5, 2000000000, 1900000000)",
        [hb_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO heartbeat_disks
             (heartbeat_id, mountpoint, filesystem, total_bytes, available_bytes, used_percent)
         VALUES (?1, '/', 'ext4', 1000000000, 50000000, 95.0)",
        [hb_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO heartbeat_network
             (heartbeat_id, interface, rx_bytes_per_sec, tx_bytes_per_sec, rx_errors, tx_errors)
         VALUES (?1, 'eth0', 1000, 500, 3, 1)",
        [hb_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO heartbeat_containers
             (heartbeat_id, runtime, running, stopped, restarting, unhealthy)
         VALUES (?1, 'docker', 5, 1, 0, 2)",
        [hb_id],
    )
    .unwrap();
    drop(conn);

    let snap = heartbeat_metric_snapshot(&pool, hb_id).unwrap();
    assert!(
        snap.cpu_usage_percent
            .is_some_and(|p| (p - 91.5).abs() < 0.01),
        "cpu_usage_percent"
    );
    assert!(
        snap.mem_used_percent
            .is_some_and(|p| (p - 87.5).abs() < 0.01),
        "mem_used_percent"
    );
    assert!(
        snap.swap_total_bytes == Some(2_000_000_000),
        "swap_total_bytes"
    );
    assert!(
        snap.max_disk_used_percent
            .is_some_and(|p| (p - 95.0).abs() < 0.01),
        "max_disk_used_percent"
    );
    assert_eq!(snap.total_network_errors, Some(4), "total_network_errors");
    assert_eq!(
        snap.container_unhealthy_count,
        Some(2),
        "container_unhealthy_count"
    );
}

fn insert_cpu(pool: &DbPool, heartbeat_id: i64, usage_percent: f64) {
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO heartbeat_cpu (heartbeat_id, usage_percent) VALUES (?1, ?2)",
        params![heartbeat_id, usage_percent],
    )
    .unwrap();
}

fn insert_memory(pool: &DbPool, heartbeat_id: i64, available_bytes: i64) {
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO heartbeat_memory (heartbeat_id, available_bytes) VALUES (?1, ?2)",
        params![heartbeat_id, available_bytes],
    )
    .unwrap();
}

/// Regression test for the `heartbeat_window_summaries` "misuse of aggregate"
/// SQL bug: `MAX(h.id)` referenced inside a correlated subquery under GROUP BY
/// is illegal in SQLite. The query must resolve the latest heartbeat id with a
/// scalar subquery, and the returned cpu/mem must come from that latest sample.
#[test]
fn heartbeat_window_summaries_resolves_latest_sample_metrics() {
    let (pool, _dir) = test_pool();
    // Two samples for host-a in the window; the second is the latest.
    let id1 = insert_heartbeat(&pool, "host-a", "tootie", 1, "2026-05-25T00:01:00Z", false);
    let id2 = insert_heartbeat(&pool, "host-a", "tootie", 2, "2026-05-25T00:02:00Z", true);
    insert_cpu(&pool, id1, 10.0);
    insert_cpu(&pool, id2, 80.0);
    insert_memory(&pool, id1, 8_000);
    insert_memory(&pool, id2, 2_000);
    // A second host to confirm per-group resolution and ordering.
    let id3 = insert_heartbeat(&pool, "host-b", "dookie", 1, "2026-05-25T00:01:30Z", false);
    insert_cpu(&pool, id3, 42.0);
    insert_memory(&pool, id3, 4_000);

    let from = "2026-05-25T00:00:00Z";
    let to = "2026-05-25T00:05:00Z";

    // All-hosts path (host omitted): bounded cross-host plan.
    let all = heartbeat_window_summaries(&pool, from, to, None).unwrap();
    assert_eq!(all.len(), 2, "expected one summary row per host");
    // Ordered by hostname ASC: dookie, tootie.
    assert_eq!(all[0].hostname, "dookie");
    assert_eq!(all[1].hostname, "tootie");
    // tootie's metrics come from the latest sample (id2), not the first.
    assert_eq!(all[1].samples, 2);
    assert_eq!(all[1].partial_samples, 1);
    assert_eq!(all[1].max_cpu_usage_percent, Some(80.0));
    assert_eq!(all[1].min_mem_available_bytes, Some(2_000));

    // Single-host path.
    let one = heartbeat_window_summaries(&pool, from, to, Some("host-a")).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].max_cpu_usage_percent, Some(80.0));
    assert_eq!(one[0].min_mem_available_bytes, Some(2_000));
}
