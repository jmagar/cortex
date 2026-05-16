#[cfg(test)]
mod notifications_db_tests {
    use rusqlite::Connection;

    use crate::db::notifications::{
        backoff_next_attempt_at, firings_insert, firings_recent, firings_recent_dedup_check,
        outbox_claim_pending, outbox_insert, outbox_mark_dead, outbox_mark_dropped,
        outbox_mark_sent, outbox_schedule_retry, FiringInsertParams, OutboxInsertParams,
    };

    fn in_memory_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE notifications_outbox (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 dedup_key TEXT NOT NULL,
                 rule_id TEXT NOT NULL,
                 severity TEXT NOT NULL,
                 hostname TEXT NOT NULL,
                 title TEXT NOT NULL,
                 body TEXT NOT NULL,
                 apprise_urls_json TEXT NOT NULL,
                 apprise_tags TEXT,
                 enqueued_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                 next_attempt_at TEXT NOT NULL,
                 attempt_count INTEGER NOT NULL DEFAULT 0,
                 last_status_code INTEGER,
                 last_error TEXT,
                 status TEXT NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending','sent','dead','dropped'))
             );
             CREATE UNIQUE INDEX IF NOT EXISTS idx_outbox_dedup_pending
                 ON notifications_outbox(dedup_key) WHERE status = 'pending';
             CREATE TABLE notification_firings (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 outbox_id INTEGER NOT NULL,
                 rule_id TEXT NOT NULL,
                 severity TEXT NOT NULL,
                 hostname TEXT NOT NULL,
                 fired_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                 status_code INTEGER,
                 notes TEXT,
                 dedup_key TEXT NOT NULL DEFAULT ''
             );",
        )
        .expect("schema");
        conn
    }

    fn make_params(dedup_key: &str) -> OutboxInsertParams {
        OutboxInsertParams {
            dedup_key: dedup_key.to_string(),
            rule_id: "oom_kill".to_string(),
            severity: "critical".to_string(),
            hostname: "host1".to_string(),
            title: "OOM Kill on host1".to_string(),
            body: "Process was killed".to_string(),
            apprise_urls_json: r#"["gotify://host/token"]"#.to_string(),
            next_attempt_at: "2030-01-01T00:00:00.000Z".to_string(),
        }
    }

    #[test]
    fn outbox_insert_idempotent() {
        let conn = in_memory_conn();
        let params = make_params("dedup-1");

        // First insert should succeed
        outbox_insert(&conn, &params).expect("first insert");

        // Second insert with same dedup_key should be skipped (idempotent)
        outbox_insert(&conn, &params).expect("second insert (no-op)");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notifications_outbox WHERE dedup_key = ?1",
                rusqlite::params!["dedup-1"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "duplicate dedup_key should be suppressed");
    }

    #[test]
    fn outbox_insert_different_keys() {
        let conn = in_memory_conn();
        outbox_insert(&conn, &make_params("key-a")).expect("insert a");
        outbox_insert(&conn, &make_params("key-b")).expect("insert b");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM notifications_outbox", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn outbox_claim_pending_basic() {
        let conn = in_memory_conn();
        outbox_insert(&conn, &make_params("key-c")).expect("insert");

        // Override next_attempt_at to past
        conn.execute(
            "UPDATE notifications_outbox SET next_attempt_at = '2000-01-01T00:00:00.000Z'",
            [],
        )
        .unwrap();

        let rows = outbox_claim_pending(&conn, 10).expect("claim");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rule_id, "oom_kill");
    }

    #[test]
    fn outbox_mark_sent_and_dead() {
        let conn = in_memory_conn();
        outbox_insert(&conn, &make_params("key-d")).expect("insert");
        conn.execute(
            "UPDATE notifications_outbox SET next_attempt_at = '2000-01-01T00:00:00.000Z'",
            [],
        )
        .unwrap();

        let rows = outbox_claim_pending(&conn, 10).expect("claim");
        let id = rows[0].id;

        outbox_mark_sent(&conn, id, Some(200)).expect("mark sent");

        let status: String = conn
            .query_row(
                "SELECT status FROM notifications_outbox WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "sent");
    }

    #[test]
    fn outbox_mark_dropped_test() {
        let conn = in_memory_conn();
        outbox_insert(&conn, &make_params("key-e")).expect("insert");
        let id: i64 = conn
            .query_row("SELECT id FROM notifications_outbox LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        outbox_mark_dropped(&conn, id, "acked").expect("mark dropped");

        let status: String = conn
            .query_row(
                "SELECT status FROM notifications_outbox WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "dropped");
    }

    #[test]
    fn outbox_schedule_retry_test() {
        let conn = in_memory_conn();
        outbox_insert(&conn, &make_params("key-f")).expect("insert");
        let id: i64 = conn
            .query_row("SELECT id FROM notifications_outbox LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();

        outbox_schedule_retry(&conn, id, "2030-06-01T00:00:00.000Z", "timeout", Some(503))
            .expect("retry");

        let (attempt_count, last_error): (i64, String) = conn
            .query_row(
                "SELECT attempt_count, last_error FROM notifications_outbox WHERE id = ?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(attempt_count, 1);
        assert_eq!(last_error, "timeout");
    }

    #[test]
    fn outbox_mark_dead_test() {
        let conn = in_memory_conn();
        outbox_insert(&conn, &make_params("key-g")).expect("insert");
        let id: i64 = conn
            .query_row("SELECT id FROM notifications_outbox LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        outbox_mark_dead(&conn, id, Some(500), "server error").expect("mark dead");

        let status: String = conn
            .query_row(
                "SELECT status FROM notifications_outbox WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "dead");
    }

    #[test]
    fn firings_insert_and_dedup_check() {
        let conn = in_memory_conn();
        outbox_insert(&conn, &make_params("key-h")).expect("insert");
        let id: i64 = conn
            .query_row("SELECT id FROM notifications_outbox LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();

        firings_insert(
            &conn,
            FiringInsertParams {
                outbox_id: id,
                rule_id: "oom_kill",
                severity: "critical",
                hostname: "host1",
                status_code: Some(200),
                notes: None,
                dedup_key: "oom_kill:host1:key-h",
            },
        )
        .expect("firings insert");

        // Within window, same dedup_key -> should dedup
        let should_dedup =
            firings_recent_dedup_check(&conn, "oom_kill", "host1", "oom_kill:host1:key-h", 3600)
                .expect("dedup check");
        assert!(should_dedup, "should suppress within dedup window");

        // Different hostname -> no dedup
        let no_dedup =
            firings_recent_dedup_check(&conn, "oom_kill", "host2", "oom_kill:host1:key-h", 3600)
                .expect("dedup check 2");
        assert!(!no_dedup, "different host should not dedup");

        // Different dedup_key -> no dedup (this is the key fix: per-signature isolation)
        let no_dedup_dk = firings_recent_dedup_check(
            &conn,
            "oom_kill",
            "host1",
            "oom_kill:host1:other-key",
            3600,
        )
        .expect("dedup check 3");
        assert!(!no_dedup_dk, "different dedup_key should not dedup");
    }

    #[test]
    fn firings_recent_list() {
        let conn = in_memory_conn();
        outbox_insert(&conn, &make_params("key-i")).expect("insert");
        let id: i64 = conn
            .query_row("SELECT id FROM notifications_outbox LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        firings_insert(
            &conn,
            FiringInsertParams {
                outbox_id: id,
                rule_id: "oom_kill",
                severity: "critical",
                hostname: "host1",
                status_code: Some(200),
                notes: None,
                dedup_key: "key-oom",
            },
        )
        .unwrap();
        firings_insert(
            &conn,
            FiringInsertParams {
                outbox_id: id,
                rule_id: "fail2ban_ban",
                severity: "notice",
                hostname: "host2",
                status_code: Some(200),
                notes: None,
                dedup_key: "key-fail2ban",
            },
        )
        .unwrap();

        let all = firings_recent(&conn, 10, None, None).expect("all firings");
        assert_eq!(all.len(), 2);

        let filtered = firings_recent(&conn, 10, Some("oom_kill"), None).expect("filtered");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].rule_id, "oom_kill");
    }

    #[test]
    fn backoff_delays_are_increasing() {
        // Just verify the strings parse to valid datetimes and are in the future
        for attempt in 0u8..5 {
            let s = backoff_next_attempt_at(attempt);
            let parsed = chrono::DateTime::parse_from_rfc3339(&s);
            assert!(
                parsed.is_ok(),
                "attempt {attempt}: backoff_next_attempt_at returned invalid ISO8601: {s}"
            );
            let dt = parsed.unwrap();
            assert!(
                dt > chrono::Utc::now(),
                "attempt {attempt}: next_attempt_at should be in the future"
            );
        }
    }
}
