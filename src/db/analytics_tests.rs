use super::*;
use crate::config::StorageConfig;
use crate::db::{init_pool, insert_logs_batch, DbPool, LogBatchEntry};

fn test_pool() -> (DbPool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    (pool, dir)
}

fn entry(ts: &str, host: &str, severity: &str, app: Option<&str>, msg: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: host.to_string(),
        facility: None,
        severity: severity.to_string(),
        app_name: app.map(String::from),
        process_id: None,
        message: msg.to_string(),
        raw: format!("<14>{ts} {host} {}: {msg}", app.unwrap_or("test")),
        source_ip: "127.0.0.1:514".to_string(),
        docker_checkpoint: None,
    }
}

#[test]
fn template_normalises_numbers_ips_uuids() {
    let t = normalize_template(
        "connection refused from 10.0.0.5:42 (id b3a1c0de-1234-5678-9abc-def012345678)",
    );
    assert!(t.contains("<ip>:<n>"));
    assert!(t.contains("<uuid>"));
}

#[test]
fn template_preserves_non_ascii_codepoints() {
    // Multi-byte UTF-8 sequences must round-trip rather than getting split into
    // mojibake by `b as char`.
    let msg = "файл 1234 не найден";
    let t = normalize_template(msg);
    assert!(t.contains("файл"));
    assert!(t.contains("не найден"));
    assert!(t.contains("<n>"));
    assert!(t.is_char_boundary(t.len()));
}

#[test]
fn list_apps_returns_distinct_apps_with_counts() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry("2026-01-01T00:00:01Z", "h1", "info", Some("nginx"), "hello"),
            entry("2026-01-01T00:00:02Z", "h1", "info", Some("nginx"), "again"),
            entry(
                "2026-01-01T00:00:03Z",
                "h2",
                "info",
                Some("sshd"),
                "auth ok",
            ),
        ],
    )
    .unwrap();

    let apps = list_apps(&pool, None).unwrap();
    let nginx = apps.iter().find(|a| a.app_name == "nginx").unwrap();
    assert_eq!(nginx.log_count, 2);
    assert_eq!(nginx.host_count, 1);

    // Filter by hostname
    let only_h2 = list_apps(&pool, Some("h2")).unwrap();
    assert_eq!(only_h2.len(), 1);
    assert_eq!(only_h2[0].app_name, "sshd");
}

#[test]
fn timeline_buckets_by_hour() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry("2026-01-01T00:10:00Z", "h1", "info", None, "a"),
            entry("2026-01-01T00:50:00Z", "h1", "info", None, "b"),
            entry("2026-01-01T01:05:00Z", "h1", "info", None, "c"),
        ],
    )
    .unwrap();
    let pts = timeline(
        &pool,
        Bucket::Hour,
        TimelineGroupBy::None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(pts.len(), 2);
    assert_eq!(pts[0].count, 2);
    assert_eq!(pts[1].count, 1);
}

#[test]
fn patterns_clusters_by_template() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry(
                "2026-01-01T00:00:01Z",
                "h1",
                "err",
                None,
                "disk 1234 failed",
            ),
            entry(
                "2026-01-01T00:00:02Z",
                "h1",
                "err",
                None,
                "disk 9999 failed",
            ),
            entry("2026-01-01T00:00:03Z", "h2", "err", None, "disk 5 failed"),
            entry("2026-01-01T00:00:04Z", "h1", "info", None, "all good"),
        ],
    )
    .unwrap();
    let (pats, scanned, truncated) =
        patterns(&pool, None, None, None, None, None, 100, 10).unwrap();
    assert!(!truncated);
    assert_eq!(scanned, 4);
    let top = &pats[0];
    assert_eq!(top.count, 3);
    assert_eq!(top.host_count, 2);
}

#[test]
fn fetch_log_by_id_returns_raw() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[entry("2026-01-01T00:00:01Z", "h1", "info", None, "hello")],
    )
    .unwrap();
    let row = fetch_log_by_id(&pool, 1).unwrap().unwrap();
    assert_eq!(row.message, "hello");
    assert!(row.raw.contains("hello"));
}

#[test]
fn context_around_returns_neighbours() {
    let (pool, _d) = test_pool();
    let mut entries = Vec::new();
    for i in 0..10 {
        entries.push(entry(
            &format!("2026-01-01T00:00:{:02}Z", i),
            "h1",
            "info",
            None,
            &format!("msg {i}"),
        ));
    }
    insert_logs_batch(&pool, &entries).unwrap();
    let r = ContextRef {
        id: Some(5),
        hostname: "h1".to_string(),
        timestamp: "2026-01-01T00:00:04Z".to_string(),
    };
    let (before, after) = context_around(&pool, &r, 3, 3).unwrap();
    assert_eq!(before.len(), 3);
    assert_eq!(after.len(), 3);
    assert!(before.last().unwrap().timestamp.as_str() <= "2026-01-01T00:00:04Z");
    assert!(after.first().unwrap().timestamp.as_str() >= "2026-01-01T00:00:04Z");
}

#[test]
fn context_timestamp_only_anchor_splits_symmetrically() {
    // Two rows share the exact reference timestamp; with id=None they must not
    // all land on one side. The before/after split is strict on `< ts` / `> ts`,
    // so simultaneous rows are excluded from both — consistent regardless of id ordering.
    let (pool, _d) = test_pool();
    let mut entries = Vec::new();
    for i in 0..5 {
        entries.push(entry(
            &format!("2026-01-01T00:00:{:02}Z", i),
            "h1",
            "info",
            None,
            "msg",
        ));
    }
    // Two rows at the exact reference time.
    entries.push(entry("2026-01-01T00:00:05Z", "h1", "info", None, "ref-a"));
    entries.push(entry("2026-01-01T00:00:05Z", "h1", "info", None, "ref-b"));
    for i in 6..10 {
        entries.push(entry(
            &format!("2026-01-01T00:00:{:02}Z", i),
            "h1",
            "info",
            None,
            "msg",
        ));
    }
    insert_logs_batch(&pool, &entries).unwrap();

    let r = ContextRef {
        id: None,
        hostname: "h1".to_string(),
        timestamp: "2026-01-01T00:00:05Z".to_string(),
    };
    let (before, after) = context_around(&pool, &r, 10, 10).unwrap();
    // 5 strictly-less timestamps, 4 strictly-greater. Neither contains a row at 05.
    assert_eq!(before.len(), 5);
    assert_eq!(after.len(), 4);
    assert!(before
        .iter()
        .all(|r| r.timestamp.as_str() < "2026-01-01T00:00:05Z"));
    assert!(after
        .iter()
        .all(|r| r.timestamp.as_str() > "2026-01-01T00:00:05Z"));
}

#[test]
fn summarize_range_counts_errors() {
    let (pool, _d) = test_pool();
    insert_logs_batch(
        &pool,
        &[
            entry("2026-01-01T00:00:01Z", "h1", "err", Some("a"), "x"),
            entry("2026-01-01T00:00:02Z", "h1", "info", Some("a"), "y"),
            entry("2026-01-01T00:00:03Z", "h2", "warning", Some("b"), "z"),
        ],
    )
    .unwrap();
    let summary = summarize_range(&pool, "2026-01-01T00:00:00Z", "2026-01-01T00:00:10Z").unwrap();
    assert_eq!(summary.total_logs, 3);
    assert_eq!(summary.total_errors, 2);
    assert_eq!(summary.top_apps.len(), 2);
}

#[test]
fn list_source_ips_aggregates_hostnames() {
    let (pool, _d) = test_pool();
    let make = |ts: &str, host: &str, ip: &str| LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: host.to_string(),
        facility: None,
        severity: "info".to_string(),
        app_name: None,
        process_id: None,
        message: "x".to_string(),
        raw: "x".to_string(),
        source_ip: ip.to_string(),
        docker_checkpoint: None,
    };
    insert_logs_batch(
        &pool,
        &[
            make("2026-01-01T00:00:01Z", "h1", "10.0.0.1:514"),
            make("2026-01-01T00:00:02Z", "h2", "10.0.0.1:514"),
            make("2026-01-01T00:00:03Z", "h2", "10.0.0.1:514"),
            make("2026-01-01T00:00:04Z", "h3", "10.0.0.2:514"),
        ],
    )
    .unwrap();
    let ips = list_source_ips(&pool).unwrap();
    let first = ips.iter().find(|e| e.source_ip == "10.0.0.1:514").unwrap();
    assert_eq!(first.host_count, 2);
    assert_eq!(first.log_count, 3);
}
