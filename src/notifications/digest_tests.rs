use super::*;

fn make_entries() -> Vec<HostDigestEntry> {
    vec![
        HostDigestEntry {
            hostname: "server1".to_string(),
            total_logs: 1000,
            error_count: 5,
            warning_count: 20,
            top_app: Some("nginx".to_string()),
        },
        HostDigestEntry {
            hostname: "server2".to_string(),
            total_logs: 500,
            error_count: 0,
            warning_count: 3,
            top_app: None,
        },
    ]
}

#[test]
fn build_digest_body_golden() {
    let entries = make_entries();
    let body = build_digest_body(&entries, 24);

    assert!(
        body.contains("Daily Digest — last 24h"),
        "should have header"
    );
    assert!(body.contains("server1"), "should contain host1");
    assert!(body.contains("server2"), "should contain host2");
    assert!(body.contains("nginx"), "should contain top app");
    assert!(body.contains("1000"), "should contain log count");
    assert!(body.contains("2 hosts"), "should have host count summary");
    assert!(body.contains("5 errors"), "should have error count");
}

#[test]
fn build_digest_body_empty() {
    let body = build_digest_body(&[], 24);
    assert!(
        body.contains("No log activity"),
        "empty input should say so"
    );
}

#[test]
fn build_digest_body_escapes_hostnames() {
    let entries = vec![HostDigestEntry {
        hostname: "admin@server".to_string(),
        total_logs: 10,
        error_count: 0,
        warning_count: 0,
        top_app: None,
    }];
    let body = build_digest_body(&entries, 24);
    assert!(!body.contains('@'), "@ should be escaped in hostname");
    assert!(body.contains('＠'), "should contain fullwidth @");
}

#[test]
fn fetch_host_stats_orders_by_volume_and_attaches_top_app() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE logs (
             hostname TEXT NOT NULL,
             severity TEXT NOT NULL,
             app_name TEXT,
             received_at TEXT NOT NULL
         );
         INSERT INTO logs (hostname, severity, app_name, received_at) VALUES
             ('host-a', 'info', 'nginx', strftime('%Y-%m-%dT%H:%M:%fZ','now')),
             ('host-a', 'warning', 'nginx', strftime('%Y-%m-%dT%H:%M:%fZ','now')),
             ('host-a', 'err', 'postgres', strftime('%Y-%m-%dT%H:%M:%fZ','now')),
             ('host-b', 'crit', 'worker', strftime('%Y-%m-%dT%H:%M:%fZ','now')),
             ('old-host', 'err', 'stale', strftime('%Y-%m-%dT%H:%M:%fZ','now','-48 hours'));",
    )
    .unwrap();

    let entries = fetch_host_stats(&conn, 24).unwrap();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].hostname, "host-a");
    assert_eq!(entries[0].total_logs, 3);
    assert_eq!(entries[0].error_count, 1);
    assert_eq!(entries[0].warning_count, 1);
    assert_eq!(entries[0].top_app.as_deref(), Some("nginx"));
    assert_eq!(entries[1].hostname, "host-b");
    assert_eq!(entries[1].error_count, 1);
    assert_eq!(entries[1].top_app.as_deref(), Some("worker"));
}

#[test]
fn parse_cron_hour_minute_standard() {
    assert_eq!(parse_cron_hour_minute("0 8 * * *"), (8, 0));
    assert_eq!(parse_cron_hour_minute("30 7 * * *"), (7, 30));
}

#[test]
fn parse_cron_hour_minute_defaults_on_bad_input() {
    assert_eq!(parse_cron_hour_minute(""), (8, 0));
    assert_eq!(parse_cron_hour_minute("bad input"), (8, 0));
}

#[test]
fn parse_cron_hour_minute_partial_failure_uses_per_field_defaults() {
    // minute parses (0), hour fails ("*") -> hour defaults to 8
    assert_eq!(parse_cron_hour_minute("0 * * * *"), (8, 0));
    // minute fails ("*"), hour parses (14) -> minute defaults to 0
    assert_eq!(parse_cron_hour_minute("* 14 * * *"), (14, 0));
}

#[test]
fn parse_cron_hour_minute_clamps_range() {
    // Out-of-range values should be clamped, not wrapped
    assert_eq!(parse_cron_hour_minute("99 25 * * *"), (23, 59));
}
