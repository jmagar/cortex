use super::*;
use crate::config::StorageConfig;
use crate::db::{insert_logs_batch, LogBatchEntry};

fn test_storage_config(db_path: std::path::PathBuf) -> StorageConfig {
    StorageConfig::for_test(db_path)
}

#[test]
fn test_init_pool_enables_incremental_auto_vacuum() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_storage_config(dir.path().join("autovac.db"));
    let pool = init_pool(&config).unwrap();
    let conn = pool.get().unwrap();
    let mode: i64 = conn
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, 2);
}

#[test]
fn test_init_pool_migrates_existing_db_to_incremental_auto_vacuum() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("legacy.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "PRAGMA auto_vacuum=NONE;
         VACUUM;
         CREATE TABLE legacy_probe(id INTEGER PRIMARY KEY);",
    )
    .unwrap();
    drop(conn);

    let config = test_storage_config(db_path);
    let pool = init_pool(&config).unwrap();
    let conn = pool.get().unwrap();
    let mode: i64 = conn
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, 2);
}

#[test]
fn test_init_pool_applies_busy_timeout_to_each_pooled_connection() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_storage_config(dir.path().join("busy-timeout.db"));
    config.pool_size = 2;
    let pool = init_pool(&config).unwrap();

    let conn1 = pool.get().unwrap();
    let conn2 = pool.get().unwrap();

    let busy_timeout_1: i64 = conn1
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .unwrap();
    let busy_timeout_2: i64 = conn2
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .unwrap();

    assert_eq!(busy_timeout_1, 5000);
    assert_eq!(busy_timeout_2, 5000);
}

#[test]
fn init_db_creates_heartbeat_schema_migration_15() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("heartbeat.db");
    let config = test_storage_config(db_path);

    let pool = init_pool(&config).unwrap();
    let conn = pool.get().unwrap();

    let applied: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 15",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(applied, 1);

    for table in [
        "host_heartbeats",
        "heartbeat_cpu",
        "heartbeat_memory",
        "heartbeat_disks",
        "heartbeat_network",
        "heartbeat_processes",
        "heartbeat_containers",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing heartbeat table {table}");
    }

    for index in [
        "idx_host_heartbeats_host_sampled",
        "idx_host_heartbeats_received",
        "idx_host_heartbeats_hostname_sampled",
        "idx_heartbeat_cpu_heartbeat_id",
        "idx_heartbeat_memory_heartbeat_id",
        "idx_heartbeat_disks_heartbeat_id",
        "idx_heartbeat_network_heartbeat_id",
        "idx_heartbeat_processes_heartbeat_id",
        "idx_heartbeat_containers_heartbeat_id",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
                [index],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing heartbeat index {index}");
    }
}

#[test]
fn heartbeat_schema_enforces_idempotency_key() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("heartbeat-unique.db");
    let config = test_storage_config(db_path);

    let pool = init_pool(&config).unwrap();
    let conn = pool.get().unwrap();

    let insert = "INSERT INTO host_heartbeats (
        host_id, hostname, source_ip, sampled_at, received_at, boot_id,
        uptime_secs, sequence, collection_ms, partial, agent_version, os, architecture
    ) VALUES (
        'host-1', 'box-a', '127.0.0.1:3100', '2026-05-25T00:00:00Z',
        '2026-05-25T00:00:01Z', 'boot-a', 60, 1, 12, 0, '0.1.0', 'linux', 'x86_64'
    )";
    conn.execute(insert, []).unwrap();
    let duplicate = conn.execute(insert, []);
    assert!(
        duplicate.is_err(),
        "duplicate heartbeat key must be rejected"
    );
}

#[test]
fn init_db_adds_ai_session_metadata_columns() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let config = crate::config::StorageConfig {
        db_path,
        ..Default::default()
    };

    let _pool = init_pool(&config).unwrap();
    let conn = rusqlite::Connection::open(&config.db_path).unwrap();
    for column in [
        "ai_tool",
        "ai_project",
        "ai_session_id",
        "ai_transcript_path",
        "metadata_json",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('logs') WHERE name = ?1",
                [column],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing column {column}");
    }
}

#[test]
fn init_db_creates_partial_ai_metadata_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let config = crate::config::StorageConfig {
        db_path,
        ..Default::default()
    };

    let _pool = init_pool(&config).unwrap();
    let conn = rusqlite::Connection::open(&config.db_path).unwrap();
    let indexes: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT name, sql FROM sqlite_schema
                 WHERE type = 'index'
                   AND name IN (
                     'idx_logs_ai_project_time',
                     'idx_logs_ai_session',
                     'idx_logs_ai_transcript_path'
                   )
                 ORDER BY name",
            )
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };

    assert_eq!(indexes.len(), 3);
    for (_, sql) in indexes {
        assert!(sql.contains("WHERE"));
        assert!(sql.contains("IS NOT NULL"));
    }
}

#[test]
fn init_db_creates_inventory_stats_tables_and_triggers() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let config = crate::config::StorageConfig {
        db_path,
        ..Default::default()
    };

    let _pool = init_pool(&config).unwrap();
    let conn = rusqlite::Connection::open(&config.db_path).unwrap();
    for table in [
        "app_inventory_stats",
        "app_host_inventory_stats",
        "source_ip_inventory_stats",
        "source_ip_host_inventory_stats",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing table {table}");
    }
    for trigger in [
        "logs_inventory_app_ai",
        "logs_inventory_app_ad",
        "logs_inventory_source_ip_ai",
        "logs_inventory_source_ip_ad",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name = ?1",
                [trigger],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing trigger {trigger}");
    }
}

#[test]
fn inventory_backfill_processes_existing_logs_in_chunks() {
    let dir = tempfile::tempdir().unwrap();
    let config = crate::config::StorageConfig {
        db_path: dir.path().join("test.db"),
        ..Default::default()
    };
    let pool = init_pool(&config).unwrap();
    let mut entries = Vec::new();
    for i in 0..3 {
        entries.push(LogBatchEntry {
            timestamp: format!("2026-01-01T00:00:0{i}Z"),
            hostname: format!("host-{i}"),
            facility: None,
            severity: "info".to_string(),
            app_name: Some("nginx".to_string()),
            process_id: None,
            message: "hello".to_string(),
            raw: "hello".to_string(),
            source_ip: "10.0.0.1:514".to_string(),
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
        });
    }
    insert_logs_batch(&pool, &entries).unwrap();

    let conn = pool.get().unwrap();
    conn.execute("DELETE FROM app_inventory_stats", []).unwrap();
    conn.execute("DELETE FROM app_host_inventory_stats", [])
        .unwrap();
    conn.execute("DELETE FROM source_ip_inventory_stats", [])
        .unwrap();
    conn.execute("DELETE FROM source_ip_host_inventory_stats", [])
        .unwrap();
    drop(conn);

    backfill_inventory_stats(&pool).unwrap();

    let conn = pool.get().unwrap();
    let complete: bool = conn
        .query_row(
            "SELECT completed_at IS NOT NULL
             FROM inventory_backfill_state
             WHERE name = 'app_source_inventory'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(complete);
    let app_count: i64 = conn
        .query_row(
            "SELECT log_count FROM app_inventory_stats WHERE app_name = 'nginx'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(app_count, 3);
    let source_count: i64 = conn
        .query_row(
            "SELECT log_count FROM source_ip_inventory_stats WHERE source_ip = '10.0.0.1:514'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(source_count, 3);
}

#[test]
fn init_db_adds_transcript_checkpoint_tables() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let config = crate::config::StorageConfig {
        db_path,
        ..Default::default()
    };

    let _pool = init_pool(&config).unwrap();
    let conn = rusqlite::Connection::open(&config.db_path).unwrap();
    for table in [
        "transcript_sources",
        "transcript_import_records",
        "transcript_parse_errors",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing table {table}");
    }
    let preview_not_null: i64 = conn
        .query_row(
            "SELECT [notnull] FROM pragma_table_info('transcript_parse_errors') WHERE name = 'record_preview'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preview_not_null, 1);
}

#[test]
fn init_db_migrates_legacy_ai_schema_without_losing_logs() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("legacy-ai.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "
        CREATE TABLE logs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp   TEXT NOT NULL,
            hostname    TEXT NOT NULL,
            facility    TEXT,
            severity    TEXT NOT NULL,
            app_name    TEXT,
            process_id  TEXT,
            message     TEXT NOT NULL,
            raw         TEXT NOT NULL,
            received_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            source_ip   TEXT NOT NULL DEFAULT ''
        );
        CREATE VIRTUAL TABLE logs_fts USING fts5(
            message,
            content='logs',
            content_rowid='id',
            tokenize='porter unicode61'
        );
        CREATE TABLE hosts (
            hostname    TEXT PRIMARY KEY,
            first_seen  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            last_seen   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            log_count   INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE schema_migrations (
            version     INTEGER PRIMARY KEY,
            applied_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );
        INSERT INTO schema_migrations(version) VALUES (1), (2), (3);
        INSERT INTO logs(timestamp, hostname, facility, severity, app_name, process_id, message, raw, source_ip)
        VALUES ('2026-05-11T00:00:00Z', 'legacy-host', 'local0', 'info', 'legacy', NULL, 'legacy preserved', 'legacy preserved', '127.0.0.1:514');
        INSERT INTO logs_fts(rowid, message) VALUES (1, 'legacy preserved');
        INSERT INTO hosts(hostname, log_count) VALUES ('legacy-host', 1);
        ",
    )
    .unwrap();
    drop(conn);

    let pool = init_pool(&test_storage_config(db_path)).unwrap();
    let conn = pool.get().unwrap();
    for column in [
        "ai_tool",
        "ai_project",
        "ai_session_id",
        "ai_transcript_path",
        "metadata_json",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('logs') WHERE name = ?1",
                [column],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing migrated column {column}");
    }
    for version in [4, 5, 6] {
        let applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
                [version],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1, "missing migration {version}");
    }
    let preserved: String = conn
        .query_row(
            "SELECT message FROM logs WHERE hostname = 'legacy-host'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preserved, "legacy preserved");
}

#[test]
fn migration_13_adds_enrichment_columns() {
    let dir = tempfile::tempdir().unwrap();
    let config = crate::config::StorageConfig {
        db_path: dir.path().join("test.db"),
        wal_mode: true,
        pool_size: 1,
        ..Default::default()
    };
    let pool = init_pool(&config).expect("init_pool ok");
    let conn = pool.get().unwrap();

    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(logs)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();

    for expected in [
        "http_status",
        "auth_outcome",
        "dns_blocked",
        "event_action",
        "parse_error",
    ] {
        assert!(
            cols.contains(&expected.to_string()),
            "missing column {expected}"
        );
    }

    let indices: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='logs'")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect();

    for expected in [
        "idx_logs_http_status_time",
        "idx_logs_auth_outcome_time",
        "idx_logs_dns_blocked_time",
        "idx_logs_event_action_time",
    ] {
        assert!(
            indices.contains(&expected.to_string()),
            "missing index {expected}"
        );
    }

    let version_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 13",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version_count, 1, "migration 13 row not recorded");
}

#[test]
fn migration_13_tolerates_existing_columns_without_version_row() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("migration-13-drift.db");
    let config = crate::config::StorageConfig {
        db_path: db_path.clone(),
        wal_mode: true,
        pool_size: 1,
        ..Default::default()
    };
    let pool = init_pool(&config).expect("initial init_pool ok");
    drop(pool);

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute("DELETE FROM schema_migrations WHERE version = 13", [])
        .unwrap();
    conn.execute("DROP INDEX idx_logs_event_action_time", [])
        .unwrap();
    drop(conn);

    let pool = init_pool(&config).expect("re-init should repair migration drift");
    let conn = pool.get().unwrap();
    let version_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 13",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version_count, 1, "migration 13 row not restored");

    let index_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_logs_event_action_time'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(index_count, 1, "migration 13 index not restored");
}

#[test]
fn transcript_import_identity_enforces_uniqueness() {
    let dir = tempfile::tempdir().unwrap();
    let config = crate::config::StorageConfig {
        db_path: dir.path().join("test.db"),
        ..Default::default()
    };

    let _pool = init_pool(&config).unwrap();
    let conn = rusqlite::Connection::open(&config.db_path).unwrap();
    conn.execute(
        "INSERT INTO transcript_sources (canonical_path, source_kind) VALUES (?1, ?2)",
        rusqlite::params!["/tmp/session.jsonl", "explicit_file"],
    )
    .unwrap();
    let source_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO transcript_import_records (source_id, record_key) VALUES (?1, ?2)",
        rusqlite::params![source_id, "record-1"],
    )
    .unwrap();
    let err = conn
        .execute(
            "INSERT INTO transcript_import_records (source_id, record_key) VALUES (?1, ?2)",
            rusqlite::params![source_id, "record-1"],
        )
        .unwrap_err();
    assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
}

/// Reproduces the post-crash state of Migration 22 (bead syslog-mcp-tfr0): a
/// crash between the `ALTER TABLE ... ADD COLUMN` statements and the version
/// marker leaves the watermark columns present but version 22 absent from
/// `schema_migrations`. We reach that identical on-disk state cheaply by
/// migrating clean to head, then deleting only the version-22 marker row.
///
/// On the pre-fix (bare `execute_batch`) code this FAILS: re-running `init_pool`
/// re-issues the unguarded ALTERs and aborts with "duplicate column name". The
/// Style-C rewrite guards each ALTER with `add_column_if_missing` and stamps the
/// version with `INSERT OR IGNORE`, so `init_pool` converges (reentrant) and the
/// partial state becomes crash-impossible (a real mid-tx crash now rolls back
/// both columns and the marker atomically).
#[test]
fn migration_22_converges_from_partial_apply() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("partial_m22.db");
    let config = test_storage_config(db_path.clone());

    // 1. Migrate a clean DB to head (version 22, both columns present).
    let pool = init_pool(&config).unwrap();
    {
        let conn = pool.get().unwrap();
        // Sanity: migration 22 specifically is applied, with the columns present.
        // Assert on version 22 directly (not MAX(version)) so a future migration 23
        // cannot break this test even though migration 22 is correctly applied.
        let m22_applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 22",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(m22_applied, 1, "fixture must reach migration 22");
        for column in ["source_row_count", "source_max_id"] {
            assert!(
                column_exists(&conn, "ai_session_rollup_meta", column).unwrap(),
                "fixture must have column {column}"
            );
        }
        // 2. Recreate the post-crash state: columns present, marker absent.
        conn.execute("DELETE FROM schema_migrations WHERE version = 22", [])
            .unwrap();
    }
    drop(pool); // release the pooled connections / file handles

    // 3. Re-running init_pool must converge, not brick on "duplicate column name".
    let pool =
        init_pool(&config).expect("init_pool must be reentrant after a partial migration 22 apply");
    let conn = pool.get().unwrap();

    // Assert migration 22 specifically was re-stamped (not MAX(version)) so a
    // future migration 23 cannot mask a missing 22 marker / break this test.
    let m22_applied: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 22",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(m22_applied, 1, "version marker must be re-stamped to 22");

    for column in ["source_row_count", "source_max_id"] {
        assert!(
            column_exists(&conn, "ai_session_rollup_meta", column).unwrap(),
            "watermark column {column} must remain present after convergence"
        );
    }
}

/// Regression guard (bead syslog-mcp-tfr0): running `init_pool` twice against the
/// same file must both succeed. This passes on the pre-fix code too — it is NOT
/// the bug-prover (`migration_22_converges_from_partial_apply` is) — it just pins
/// the idempotent-on-clean-reopen behaviour so a future migration change can't
/// silently break it.
#[test]
fn init_pool_is_idempotent_when_run_twice() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("idempotent.db");
    let config = test_storage_config(db_path);

    let pool = init_pool(&config).expect("first init_pool must succeed");
    drop(pool);

    let pool = init_pool(&config).expect("second init_pool on same file must succeed");
    let conn = pool.get().unwrap();
    // Assert migration 22 specifically is applied (not MAX(version)) so a future
    // migration 23 cannot break this test even though 22 is correctly applied.
    let m22_applied: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 22",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(m22_applied, 1);
}
