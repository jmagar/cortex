use anyhow::Result;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use scheduled_thread_pool::ScheduledThreadPool;
use std::sync::{Arc, OnceLock};

use crate::config::StorageConfig;

pub type DbPool = Pool<SqliteConnectionManager>;

fn shared_scheduled_thread_pool() -> Arc<ScheduledThreadPool> {
    static POOL: OnceLock<Arc<ScheduledThreadPool>> = OnceLock::new();
    Arc::clone(POOL.get_or_init(|| Arc::new(ScheduledThreadPool::new(1))))
}

/// Initialize the database pool and schema
pub fn init_pool(config: &StorageConfig) -> Result<DbPool> {
    // Ensure parent directory exists
    if let Some(parent) = config.db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let wal_mode = config.wal_mode;
    let manager = SqliteConnectionManager::file(&config.db_path)
        .with_init(move |conn| configure_connection_pragmas(conn, wal_mode));
    let pool = Pool::builder()
        .max_size(config.pool_size)
        .thread_pool(shared_scheduled_thread_pool())
        .build(manager)?;

    // Initialize schema
    let conn = pool.get()?;

    let auto_vacuum_mode: i64 = conn.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?;
    if auto_vacuum_mode != 2 {
        conn.execute_batch("PRAGMA auto_vacuum=INCREMENTAL;")?;
        let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        if page_count > 0 {
            conn.execute_batch("VACUUM;")?;
        }
    }

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS logs (
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
            source_ip   TEXT NOT NULL DEFAULT '',
            ai_tool            TEXT,
            ai_project         TEXT,
            ai_session_id      TEXT,
            ai_transcript_path TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON logs(timestamp);
        CREATE INDEX IF NOT EXISTS idx_logs_hostname  ON logs(hostname);
        CREATE INDEX IF NOT EXISTS idx_logs_severity  ON logs(severity);
        CREATE INDEX IF NOT EXISTS idx_logs_app_name  ON logs(app_name);
        CREATE INDEX IF NOT EXISTS idx_logs_host_time ON logs(hostname, timestamp);
        CREATE INDEX IF NOT EXISTS idx_logs_sev_time ON logs(severity, timestamp);
        CREATE INDEX IF NOT EXISTS idx_logs_received_at ON logs(received_at);
        CREATE INDEX IF NOT EXISTS idx_logs_hostname_received_at ON logs(hostname, received_at);
        CREATE INDEX IF NOT EXISTS idx_logs_source_ip_timestamp ON logs(source_ip, timestamp);
        DROP INDEX IF EXISTS idx_logs_source_ip;

        -- FTS5 virtual table for full-text search on messages
        CREATE VIRTUAL TABLE IF NOT EXISTS logs_fts USING fts5(
            message,
            content='logs',
            content_rowid='id',
            tokenize='porter unicode61'
        );

        -- Trigger to keep FTS in sync on INSERT only.
        -- DELETE and UPDATE triggers are intentionally absent: bulk DELETEs during
        -- retention purge and storage-budget enforcement fire the trigger for every
        -- deleted row inside a single implicit transaction, holding the SQLite write
        -- lock long enough to starve the batch writer. FTS5 content tables tolerate
        -- phantom rows — stale entries are skipped at query time and cleaned up by
        -- periodic incremental merge (merge=500,250).
        CREATE TRIGGER IF NOT EXISTS logs_ai AFTER INSERT ON logs BEGIN
            INSERT INTO logs_fts(rowid, message) VALUES (new.id, new.message);
        END;

        -- Hostname registry for quick lookups
        CREATE TABLE IF NOT EXISTS hosts (
            hostname    TEXT PRIMARY KEY,
            first_seen  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            last_seen   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            log_count   INTEGER NOT NULL DEFAULT 0
        );

        -- Migration version table: each row records a completed schema migration.
        -- Guards migrations so they run exactly once per database, not on every startup.
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version     INTEGER PRIMARY KEY,
            applied_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );
        ",
    )?;

    // Migration: add source_ip column to existing databases that predate this column.
    // ALTER TABLE ADD COLUMN is a no-op if the column already exists in SQLite ≥ 3.37,
    // but older SQLite returns an error on duplicate columns, so we check first.
    let col_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('logs') WHERE name = 'source_ip'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !col_exists {
        conn.execute_batch("ALTER TABLE logs ADD COLUMN source_ip TEXT NOT NULL DEFAULT ''")?;
        tracing::info!("Migration: added source_ip column to logs table");
    }

    // Migration 1: drop FTS5 DELETE/UPDATE triggers from existing databases.
    // These triggers caused write-lock contention during bulk deletes (retention
    // purge, storage enforcement). See schema comment above for rationale.
    // Guarded by schema_migrations so it runs exactly once per database.
    let migration_1_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_1_applied {
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS logs_ad;
             DROP TRIGGER IF EXISTS logs_au;
             INSERT INTO schema_migrations (version) VALUES (1);",
        )?;
        tracing::info!("Migration 1: dropped FTS5 DELETE/UPDATE triggers");
    }

    // Migration 2: store per Docker host/container checkpoints for optional
    // docker-socket-proxy log ingestion. This lets short syslog-mcp outages
    // replay from Docker's local log store with /containers/{id}/logs?since=.
    let migration_2_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 2",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_2_applied {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS docker_ingest_checkpoints (
                 host_name      TEXT NOT NULL,
                 container_id   TEXT NOT NULL,
                 last_timestamp TEXT NOT NULL,
                 updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 PRIMARY KEY (host_name, container_id)
             );
             INSERT INTO schema_migrations (version) VALUES (2);",
        )?;
        tracing::info!("Migration 2: created docker_ingest_checkpoints table");
    }

    // Migration 3: composite index on (app_name, received_at).
    //
    // The new `purge_by_tag_window` function deletes rows by `app_name` within
    // a `received_at` window (e.g. all `adguard-allowed` older than 7 days).
    // Without this composite index, each chunked DELETE scans the entire
    // app_name partition before applying the time filter — pathological at
    // AdGuard volumes.
    //
    // First-run cost: on a multi-million-row database the CREATE INDEX may
    // take several minutes and holds the write lock for that duration. The
    // /health endpoint will not respond and syslog UDP packets may be dropped
    // at the kernel buffer during that window. Operators upgrading on a
    // populated DB should plan for a brief health-check gap.
    let migration_3_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 3",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_3_applied {
        tracing::info!(
            "Migration 3: starting CREATE INDEX idx_logs_app_name_received_at \
             — may take several minutes on large databases, write lock held"
        );
        let started = std::time::Instant::now();
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_logs_app_name_received_at \
                 ON logs(app_name, received_at);
             INSERT INTO schema_migrations (version) VALUES (3);",
        )?;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            "Migration 3: composite index (app_name, received_at) created"
        );
    }

    // Migration 4: add AI transcript metadata columns and indexes.
    let migration_4_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 4",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_4_applied {
        for (column, sql_type) in [
            ("ai_tool", "TEXT"),
            ("ai_project", "TEXT"),
            ("ai_session_id", "TEXT"),
            ("ai_transcript_path", "TEXT"),
        ] {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('logs') WHERE name = ?1",
                    [column],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0)
                > 0;
            if !exists {
                conn.execute_batch(&format!("ALTER TABLE logs ADD COLUMN {column} {sql_type}"))?;
            }
        }
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_logs_ai_project_time
                 ON logs(ai_project, timestamp)
                 WHERE ai_project IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_logs_ai_session
                 ON logs(ai_tool, ai_project, ai_session_id)
                 WHERE ai_tool IS NOT NULL;
             INSERT INTO schema_migrations (version) VALUES (4);",
        )?;
        tracing::info!("Migration 4: added AI transcript metadata columns and indexes");
    }

    let migration_5_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 5",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_5_applied {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS transcript_sources (
                 id              INTEGER PRIMARY KEY AUTOINCREMENT,
                 canonical_path  TEXT NOT NULL UNIQUE,
                 source_kind     TEXT NOT NULL,
                 file_size       INTEGER,
                 file_mtime      INTEGER,
                 content_hash    TEXT,
                 last_offset     INTEGER NOT NULL DEFAULT 0,
                 last_indexed_at TEXT,
                 last_error      TEXT
             );
             INSERT INTO schema_migrations (version) VALUES (5);",
        )?;
        tracing::info!("Migration 5: created transcript_sources table");
    }

    let migration_6_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 6",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_6_applied {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS transcript_import_records (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 source_id   INTEGER NOT NULL REFERENCES transcript_sources(id),
                 record_key  TEXT NOT NULL,
                 imported_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 UNIQUE(source_id, record_key)
             );
             CREATE INDEX IF NOT EXISTS idx_transcript_import_records_source_id
                 ON transcript_import_records(source_id);
             INSERT INTO schema_migrations (version) VALUES (6);",
        )?;
        tracing::info!("Migration 6: created transcript_import_records table");
    }

    let migration_7_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 7",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_7_applied {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS transcript_parse_errors (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 source_id      INTEGER NOT NULL REFERENCES transcript_sources(id),
                 line_no        INTEGER NOT NULL,
                 error          TEXT NOT NULL,
                 record_preview TEXT NOT NULL,
                 seen_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 UNIQUE(source_id, line_no, error, record_preview)
             );
             CREATE INDEX IF NOT EXISTS idx_transcript_parse_errors_source_seen
                 ON transcript_parse_errors(source_id, seen_at DESC);
             CREATE INDEX IF NOT EXISTS idx_transcript_parse_errors_seen
                 ON transcript_parse_errors(seen_at DESC);
             INSERT INTO schema_migrations (version) VALUES (7);",
        )?;
        tracing::info!("Migration 7: created transcript_parse_errors table");
    }

    let migration_8_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 8",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_8_applied {
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_logs_ai_project_time;
             DROP INDEX IF EXISTS idx_logs_ai_session;
             CREATE INDEX IF NOT EXISTS idx_logs_ai_project_time
                 ON logs(ai_project, timestamp)
                 WHERE ai_project IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_logs_ai_session
                 ON logs(ai_tool, ai_project, ai_session_id)
                 WHERE ai_tool IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_logs_ai_transcript_path
                 ON logs(ai_transcript_path)
                 WHERE ai_transcript_path IS NOT NULL;
             INSERT INTO schema_migrations (version) VALUES (8);",
        )?;
        tracing::info!("Migration 8: rebuilt AI metadata indexes as partial indexes");
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_logs_ai_project_time
             ON logs(ai_project, timestamp)
             WHERE ai_project IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_logs_ai_session
             ON logs(ai_tool, ai_project, ai_session_id)
             WHERE ai_tool IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_logs_ai_transcript_path
             ON logs(ai_transcript_path)
             WHERE ai_transcript_path IS NOT NULL;",
    )?;

    tracing::info!(path = %config.db_path.display(), "Database initialized");
    Ok(pool)
}

fn configure_connection_pragmas(conn: &mut Connection, wal_mode: bool) -> rusqlite::Result<()> {
    if wal_mode {
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    }
    conn.execute_batch(
        "PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=5000;
         PRAGMA cache_size=-64000;",
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;
