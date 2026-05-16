// scanner_tests.rs
use super::{process_chunk, NORMALIZER_VERSION};
use crate::config::StorageConfig;
use crate::db::{self, DbPool};
use tempfile::TempDir;

fn test_pool() -> (DbPool, TempDir) {
    let dir = TempDir::new().unwrap();
    let storage = StorageConfig {
        db_path: dir.path().join("test.db"),
        pool_size: 1,
        wal_mode: false,
        ..Default::default()
    };
    let pool = db::init_pool(&storage).unwrap();
    (pool, dir)
}

/// Insert a log row with the current timestamp (so it falls in the 1-hour window).
fn insert_log(conn: &rusqlite::Connection, message: &str, severity: &str, hostname: &str) -> i64 {
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    conn.execute(
        "INSERT INTO logs (timestamp, hostname, severity, message, raw, received_at)
         VALUES (?4, ?1, ?2, ?3, ?3, ?4)",
        rusqlite::params![hostname, severity, message, now],
    )
    .unwrap();
    conn.last_insert_rowid()
}

#[test]
fn test_process_chunk_skips_below_cursor() {
    let (pool, _dir) = test_pool();
    {
        let conn = pool.get().unwrap();
        // Insert 3 log rows with severity that gets picked up by scanner
        insert_log(&conn, "error occurred alpha", "err", "host1");
        insert_log(&conn, "error occurred beta", "err", "host1");
        insert_log(&conn, "error occurred gamma", "err", "host1");
    }

    // Set cursor to 2: only row with id > 2 should be processed
    let result = process_chunk(&pool, 2, 200, 1).unwrap();

    // Only 1 row (id=3) should be returned
    assert_eq!(
        result.rows_in_chunk, 1,
        "only rows with id > cursor should be processed"
    );
    assert_eq!(result.new_cursor, 3);
}

#[test]
fn test_process_chunk_does_not_notify_below_threshold() {
    let (pool, _dir) = test_pool();
    {
        let conn = pool.get().unwrap();
        // Insert fewer rows than the frequency_threshold (threshold=5, insert 3)
        insert_log(&conn, "disk full on /var", "err", "host1");
        insert_log(&conn, "disk full on /var", "err", "host1");
        insert_log(&conn, "disk full on /var", "err", "host1");
    }

    // frequency_threshold=5; only 3 rows inserted
    let result = process_chunk(&pool, 0, 200, 5).unwrap();
    assert_eq!(result.rows_in_chunk, 3);

    // No outbox row should have been inserted
    let conn = pool.get().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM notifications_outbox", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap();
    assert_eq!(count, 0, "no outbox notification below frequency_threshold");
}

#[test]
fn test_process_chunk_notifies_above_threshold() {
    let (pool, _dir) = test_pool();
    {
        let conn = pool.get().unwrap();
        // Insert frequency_threshold+1 rows with the SAME message pattern
        // so they all land in the same signature group
        for _ in 0..6 {
            insert_log(
                &conn,
                "connection refused to 127.0.0.1:5432",
                "err",
                "host1",
            );
        }
    }

    // threshold=5; 6 rows inserted → should notify
    let result = process_chunk(&pool, 0, 200, 5).unwrap();
    assert_eq!(result.rows_in_chunk, 6);

    let conn = pool.get().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM notifications_outbox WHERE rule_id = 'unaddressed_error_signature'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(
        count, 1,
        "one outbox notification above frequency_threshold"
    );
}

#[test]
fn test_process_chunk_suppressed_when_acked() {
    let (pool, _dir) = test_pool();
    {
        let conn = pool.get().unwrap();
        // Insert enough rows to trigger notification
        for _ in 0..6 {
            insert_log(&conn, "oom: process killed repeatedly", "err", "host1");
        }
    }

    // First pass: process the chunk so the signature is written
    let result = process_chunk(&pool, 0, 200, 5).unwrap();
    assert_eq!(result.rows_in_chunk, 6);

    // Manually acknowledge the signature
    {
        let conn = pool.get().unwrap();
        // Find the signature hash that was written
        let hash: String = conn
            .query_row(
                "SELECT signature_hash FROM error_signatures LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap();

        // Clear previous outbox entries so we can check fresh state
        conn.execute("DELETE FROM notifications_outbox", [])
            .unwrap();

        // Set acknowledged_at on the signature
        conn.execute(
            "UPDATE error_signatures SET acknowledged_at = '2026-01-01T00:00:00.000Z',
             acknowledged_by = 'test'
             WHERE signature_hash = ?1 AND normalizer_version = ?2",
            rusqlite::params![hash, NORMALIZER_VERSION],
        )
        .unwrap();
    }

    // Insert more rows so there is something to scan past the previous cursor
    {
        let conn = pool.get().unwrap();
        for _ in 0..3 {
            insert_log(&conn, "oom: process killed repeatedly", "err", "host1");
        }
    }

    // Second pass: with cursor advanced past original rows
    let cursor = result.new_cursor;
    let result2 = process_chunk(&pool, cursor, 200, 5).unwrap();
    assert_eq!(result2.rows_in_chunk, 3);

    // Outbox should still be empty because the signature is acked
    let conn = pool.get().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM notifications_outbox WHERE rule_id = 'unaddressed_error_signature'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(count, 0, "no notification when signature is acknowledged");
}
