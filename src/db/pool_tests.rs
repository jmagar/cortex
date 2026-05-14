use super::*;
use crate::config::StorageConfig;

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
fn init_db_adds_transcript_checkpoint_tables() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let config = crate::config::StorageConfig {
        db_path,
        ..Default::default()
    };

    let _pool = init_pool(&config).unwrap();
    let conn = rusqlite::Connection::open(&config.db_path).unwrap();
    for table in ["transcript_sources", "transcript_import_records"] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing table {table}");
    }
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
