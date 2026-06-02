use anyhow::Result;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use scheduled_thread_pool::ScheduledThreadPool;
use std::sync::{Arc, OnceLock};

use crate::config::StorageConfig;

pub type DbPool = Pool<SqliteConnectionManager>;

/// Process-wide SQLite **write serialization** lock.
///
/// SQLite permits only one writer at a time, but cortex runs an r2d2 pool of several
/// connections with multiple concurrent writer subsystems (syslog/docker ingest,
/// heartbeat, notifications, AI index, retention maintenance). Without serialization
/// these race SQLite's single write lock, exceed `busy_timeout`, and surface as
/// `database is locked` — dropping log batches. Every write transaction acquires this
/// guard so writers queue in-process instead of colliding at the SQLite layer; reads
/// stay concurrent on the pool (WAL allows many readers). Reentrant so a write path that
/// nests guarded helpers on a single thread cannot deadlock.
pub fn write_lock() -> parking_lot::ReentrantMutexGuard<'static, ()> {
    static WRITE_LOCK: parking_lot::ReentrantMutex<()> = parking_lot::ReentrantMutex::new(());
    WRITE_LOCK.lock()
}

pub const KNOWN_SCHEMA_VERSION: i64 = 28;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SchemaVersionInfo {
    pub version: i64,
    pub last_migration_at: Option<String>,
    pub known_version: i64,
}

fn shared_scheduled_thread_pool() -> Arc<ScheduledThreadPool> {
    static POOL: OnceLock<Arc<ScheduledThreadPool>> = OnceLock::new();
    Arc::clone(POOL.get_or_init(|| Arc::new(ScheduledThreadPool::new(1))))
}

pub fn read_schema_version_info(pool: &DbPool) -> Result<SchemaVersionInfo> {
    let conn = pool.get()?;
    read_schema_version_info_conn(&conn)
}

/// Probe `schema_migrations` from an already-borrowed connection. Used by
/// callers that do not own a [`DbPool`] (e.g. the scanner's checkpoint store).
pub fn read_schema_version_info_conn(conn: &Connection) -> Result<SchemaVersionInfo> {
    let (version, last_migration_at): (Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT MAX(version), MAX(applied_at) FROM schema_migrations",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|err| anyhow::anyhow!("schema_migrations probe failed: {err}"))?;
    Ok(SchemaVersionInfo {
        version: version.unwrap_or(0),
        last_migration_at,
        known_version: KNOWN_SCHEMA_VERSION,
    })
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

    // Initialize schema. `mut` so migration 25's backfill can open an explicit
    // transaction (`Connection::transaction_with_behavior` needs `&mut`).
    let mut conn = pool.get()?;

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
            ai_transcript_path TEXT,
            metadata_json      TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON logs(timestamp);
        CREATE INDEX IF NOT EXISTS idx_logs_hostname  ON logs(hostname);
        CREATE INDEX IF NOT EXISTS idx_logs_severity  ON logs(severity);
        CREATE INDEX IF NOT EXISTS idx_logs_app_name  ON logs(app_name);
        CREATE INDEX IF NOT EXISTS idx_logs_host_time ON logs(hostname, timestamp);
        CREATE INDEX IF NOT EXISTS idx_logs_sev_time ON logs(severity, timestamp);
        CREATE INDEX IF NOT EXISTS idx_logs_app_name_timestamp ON logs(app_name, timestamp);
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
    // docker-socket-proxy log ingestion. This lets short cortex outages
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

    let migration_9_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 9",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_9_applied {
        let metadata_col_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('logs') WHERE name = 'metadata_json'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if !metadata_col_exists {
            conn.execute_batch("ALTER TABLE logs ADD COLUMN metadata_json TEXT")?;
        }
        conn.execute_batch("INSERT INTO schema_migrations (version) VALUES (9);")?;
        tracing::info!("Migration 9: added logs.metadata_json");
    }

    // Migration 10: error signature detection tables.
    let migration_10_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 10",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_10_applied {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS error_signatures (
                 signature_hash      TEXT NOT NULL,
                 normalizer_version  INTEGER NOT NULL,
                 template            TEXT NOT NULL,
                 sample_message      TEXT NOT NULL,
                 sample_hostname     TEXT NOT NULL,
                 sample_app_name     TEXT,
                 severity            TEXT NOT NULL,
                 first_seen_at       TEXT NOT NULL,
                 last_seen_at        TEXT NOT NULL,
                 total_count         INTEGER NOT NULL DEFAULT 0,
                 acknowledged_at     TEXT,
                 acknowledged_by     TEXT,
                 PRIMARY KEY (signature_hash, normalizer_version)
             );
             CREATE INDEX IF NOT EXISTS idx_error_sigs_last_seen
                 ON error_signatures(last_seen_at DESC);
             CREATE INDEX IF NOT EXISTS idx_error_sigs_ack
                 ON error_signatures(acknowledged_at)
                 WHERE acknowledged_at IS NULL;

             CREATE TABLE IF NOT EXISTS error_signature_windows (
                 signature_hash      TEXT NOT NULL,
                 normalizer_version  INTEGER NOT NULL,
                 window_start        TEXT NOT NULL,
                 window_end          TEXT NOT NULL,
                 count_in_window     INTEGER NOT NULL,
                 PRIMARY KEY (signature_hash, normalizer_version, window_start, window_end)
             );

             CREATE TABLE IF NOT EXISTS error_signature_ack_events (
                 id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                 signature_hash      TEXT NOT NULL,
                 normalizer_version  INTEGER NOT NULL,
                 event_type          TEXT NOT NULL CHECK (event_type IN ('ack','unack')),
                 actor               TEXT NOT NULL,
                 notes               TEXT CHECK (notes IS NULL OR length(notes) <= 4096),
                 created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             );
             CREATE INDEX IF NOT EXISTS idx_ack_events_sig
                 ON error_signature_ack_events(signature_hash, created_at DESC);

             CREATE TABLE IF NOT EXISTS error_scan_cursor (
                 id                      INTEGER PRIMARY KEY CHECK (id = 1),
                 last_scanned_log_id     INTEGER NOT NULL DEFAULT 0,
                 last_scan_completed_at  TEXT
             );
             INSERT OR IGNORE INTO error_scan_cursor (id, last_scanned_log_id) VALUES (1, 0);

             INSERT INTO schema_migrations (version) VALUES (10);",
        )?;
        tracing::info!("Migration 10: created error signature detection tables");
    }

    // Migration 11: notifications outbox and firings tables.
    let migration_11_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 11",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_11_applied {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS notifications_outbox (
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
             CREATE INDEX IF NOT EXISTS idx_outbox_pending
                 ON notifications_outbox(status, next_attempt_at)
                 WHERE status = 'pending';
             CREATE INDEX IF NOT EXISTS idx_outbox_dedup
                 ON notifications_outbox(dedup_key, enqueued_at DESC);

             CREATE TABLE IF NOT EXISTS notification_firings (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 outbox_id INTEGER NOT NULL,
                 rule_id TEXT NOT NULL,
                 severity TEXT NOT NULL,
                 hostname TEXT NOT NULL,
                 fired_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                 status_code INTEGER,
                 notes TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_firings_fired_at
                 ON notification_firings(fired_at DESC);
             CREATE INDEX IF NOT EXISTS idx_firings_rule
                 ON notification_firings(rule_id, fired_at DESC);

             INSERT INTO schema_migrations (version) VALUES (11);",
        )?;
        tracing::info!("Migration 11: created notifications outbox and firings tables");
    }

    // Migration 12: add dedup_key column to notification_firings and unique partial
    // index on notifications_outbox to fix TOCTOU on outbox_insert.
    let migration_12_applied: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 12",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !migration_12_applied {
        // Add dedup_key to notification_firings so dedup checks are scoped per
        // (rule_id, hostname, dedup_key) rather than just (rule_id, hostname).
        // Without this, all error_sig firings share rule_id='unaddressed_error_signature'
        // and the first firing suppresses all subsequent ones regardless of signature.
        let dedup_col_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('notification_firings') WHERE name = 'dedup_key'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if !dedup_col_exists {
            conn.execute_batch(
                "ALTER TABLE notification_firings ADD COLUMN dedup_key TEXT NOT NULL DEFAULT '';",
            )?;
        }
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_outbox_dedup_pending
                 ON notifications_outbox(dedup_key) WHERE status = 'pending';
             INSERT INTO schema_migrations (version) VALUES (12);",
        )?;
        tracing::info!(
            "Migration 12: added notification_firings.dedup_key, unique partial index on outbox"
        );
    }

    // Migration 13: enrichment-framework columns + partial indexes.
    // Spec: docs/superpowers/specs/2026-05-16-enrichment-framework-design.md §5
    // Contract: docs/contracts/db-additions.sql Epic B section
    if !migration_applied(&conn, 13)? {
        apply_migration_13(&conn)?;
        tracing::info!("Migration 13: added enrichment columns + partial indexes");
    }

    let already_applied_14: i64 = conn.query_row(
        "SELECT COUNT(*) FROM schema_migrations WHERE version = 14",
        [],
        |r| r.get(0),
    )?;
    if already_applied_14 == 0 {
        tracing::info!(
            "Migration 14: starting CREATE INDEX idx_logs_ai_session_host_time \
             — may take time on large AI transcript databases"
        );
        let started = std::time::Instant::now();
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_logs_ai_session_host_time
                 ON logs(ai_project, ai_tool, ai_session_id, hostname, timestamp)
                 WHERE ai_project IS NOT NULL
                   AND ai_tool IS NOT NULL
                   AND ai_session_id IS NOT NULL;
             INSERT INTO schema_migrations (version) VALUES (14);",
        )?;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            "Migration 14: AI session host/time index created"
        );
    }

    // Migration 15: first-class heartbeat telemetry storage.
    // Contract: docs/contracts/heartbeat-telemetry.md
    if !migration_applied(&conn, 15)? {
        apply_migration_15_heartbeat(&conn)?;
        tracing::info!("Migration 15: created heartbeat telemetry tables and indexes");
    }

    if !migration_applied(&conn, 16)? {
        tracing::info!(
            "Migration 16: starting CREATE INDEX idx_logs_app_name_timestamp \
             — may take time on large databases"
        );
        let started = std::time::Instant::now();
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_logs_app_name_timestamp
                 ON logs(app_name, timestamp);
             INSERT INTO schema_migrations (version) VALUES (16);",
        )?;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            "Migration 16: app_name/timestamp search index created"
        );
    }

    if !migration_applied(&conn, 17)? {
        apply_migration_17_inventory_stats(&conn)?;
        tracing::info!("Migration 17: created app/source inventory stats");
    }

    // Migration 18: add restarting column to heartbeat_containers.
    if !migration_applied(&conn, 18)? {
        apply_migration_18_heartbeat_restarting(&conn)?;
        tracing::info!("Migration 18: added restarting column to heartbeat_containers");
    }

    // Migration 19: add host_heartbeats_latest fleet cache table.
    if !migration_applied(&conn, 19)? {
        apply_migration_19_heartbeat_latest(&conn)?;
        tracing::info!("Migration 19: created host_heartbeats_latest fleet cache table");
    }

    // Migration 20: composite index on error_signature_windows(window_end, ...) for sig list queries.
    // The `sig list` action filters unaddressed signatures by recency, ordering on window_end DESC.
    // Without this index, every query does a full scan of error_signature_windows.
    if !migration_applied(&conn, 20)? {
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_error_sig_windows_end
                 ON error_signature_windows(window_end, signature_hash, normalizer_version);
             INSERT INTO schema_migrations (version) VALUES (20);",
        )?;
        tracing::info!("Migration 20: added index on error_signature_windows(window_end)");
    }

    // Migration 21: AI session rollup table (bead cortex-2vre).
    // `list_ai_sessions` aggregates GROUP BY (project, tool, session, hostname)
    // over the full AI-row partition then sorts by MAX(timestamp) — an
    // unavoidable temp-btree that grows with AI-row count (~4s at 10M rows).
    // The rollup is a periodically-refreshed materialization read in O(#sessions)
    // via idx, decoupling read latency from row count. It is REFRESH-based, not
    // trigger-based: trigger-maintained MIN/MAX is wrong on DELETE (deleting the
    // row holding the current MAX can't recover the new extreme without a rescan,
    // and that rescan reintroduces bulk-purge lock contention). Staleness is
    // exposed via ai_session_rollup_meta.refreshed_at.
    if !migration_applied(&conn, 21)? {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ai_session_rollup (
                 ai_project         TEXT NOT NULL,
                 ai_tool            TEXT NOT NULL,
                 ai_session_id      TEXT NOT NULL,
                 hostname           TEXT NOT NULL,
                 ai_transcript_path TEXT,
                 first_seen         TEXT NOT NULL,
                 last_seen          TEXT NOT NULL,
                 event_count        INTEGER NOT NULL,
                 PRIMARY KEY (ai_project, ai_tool, ai_session_id, hostname)
             );
             CREATE INDEX IF NOT EXISTS idx_ai_session_rollup_last_seen
                 ON ai_session_rollup(last_seen DESC);
             CREATE TABLE IF NOT EXISTS ai_session_rollup_meta (
                 id           INTEGER PRIMARY KEY CHECK (id = 1),
                 refreshed_at TEXT,
                 row_count    INTEGER NOT NULL DEFAULT 0
             );
             INSERT OR IGNORE INTO ai_session_rollup_meta (id, refreshed_at, row_count)
                 VALUES (1, NULL, 0);
             INSERT INTO schema_migrations (version) VALUES (21);",
        )?;
        tracing::info!("Migration 21: created AI session rollup table");
    }

    // Migration 22: source watermark for the AI session rollup (bead
    // cortex-g33v). The background refresh recomputed the full GROUP-BY
    // over `logs` every cadence even when no AI rows had changed. These two
    // columns record the source-side `(COUNT(*), MAX(id))` of AI rows captured
    // by the last refresh; the refresh task compares the live watermark against
    // them and skips the recompute entirely when nothing changed. Both default
    // to 0 so the first post-migration refresh always runs (live watermark > 0
    // whenever AI rows exist, and `refreshed_at` is still NULL regardless).
    if !migration_applied(&conn, 22)? {
        apply_migration_22(&conn)?;
        tracing::info!("Migration 22: added AI session rollup source watermark");
    }

    // Migration 23: covering indexes for the `errors` summary and `ai projects`
    // aggregation. Both previously read every matching row from the table to
    // fetch columns absent from the leading index (hostname for the error
    // GROUP BY; ai_tool / ai_session_id for the project rollup), making them
    // O(matching-rows) table-lookup scans (~10s and ~48s on a multi-million-row
    // DB). These covering indexes make both aggregations index-only — verified
    // via EXPLAIN QUERY PLAN flipping to `USING COVERING INDEX`.
    //
    // First-run cost: building these on a populated DB scans the table and
    // holds the write lock for the duration (seconds to minutes at multi-
    // million-row volumes); /health may gap and syslog packets may drop during
    // that window — the same one-time cost as the earlier index migrations.
    if !migration_applied(&conn, 23)? {
        tracing::info!(
            "Migration 23: building covering indexes (idx_logs_ai_project_cover, \
             idx_logs_sev_host_time) — may take minutes on large DBs, write lock held"
        );
        let started = std::time::Instant::now();
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_logs_ai_project_cover
                 ON logs(ai_project, ai_tool, ai_session_id, timestamp)
                 WHERE ai_project IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_logs_sev_host_time
                 ON logs(severity, hostname, timestamp);
             INSERT INTO schema_migrations (version) VALUES (23);",
        )?;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            "Migration 23: covering indexes for errors + ai projects created"
        );
    }

    // Migration 24: timestamp-positioned covering indexes for the AI
    // aggregations, plus baseline ANALYZE stats.
    //
    // Migration 23's idx_logs_ai_project_cover (ai_project, ai_tool,
    // ai_session_id, timestamp) made `ai projects` index-only, but with a
    // timestamp-range filter (e.g. `ai blocks`'s 30-day default) the planner
    // can't use its trailing `timestamp` as a seek and instead chose
    // idx_logs_timestamp — scanning all recent high-volume syslog and filtering
    // AI rows out one by one (~28s). Putting `timestamp` SECOND
    // (ai_project, timestamp, ai_tool, ai_session_id) gives both a seekable
    // range and full coverage, and supersedes the old index for every AI query
    // (verified: nothing picks idx_logs_ai_project_cover once this exists), so
    // it is dropped. idx_logs_ai_tool_cover does the same for `ai tools`
    // (GROUP BY ai_tool needs session_id + timestamp).
    //
    // CRITICAL: these indexes are only *chosen* when ANALYZE statistics exist —
    // without `sqlite_stat1`, the planner's no-stats heuristics still pick
    // idx_logs_timestamp (verified empirically). So this migration also runs an
    // initial ANALYZE (bounded by the connection's analysis_limit=400), and the
    // optimize maintenance task keeps stats fresh as the DB grows. Same first-
    // run write-lock cost as the other index migrations.
    if !migration_applied(&conn, 24)? {
        tracing::info!(
            "Migration 24: rebuilding AI covering indexes (timestamp-positioned) \
             + initial ANALYZE — may take minutes on large DBs, write lock held"
        );
        let started = std::time::Instant::now();
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_logs_ai_project_cover;
             CREATE INDEX IF NOT EXISTS idx_logs_ai_project_ts_cover
                 ON logs(ai_project, timestamp, ai_tool, ai_session_id)
                 WHERE ai_project IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_logs_ai_tool_cover
                 ON logs(ai_tool, ai_session_id, timestamp)
                 WHERE ai_tool IS NOT NULL;",
        )?;
        // Only seed stats when the table already has data. ANALYZE on an empty
        // `logs` (fresh install / tests) records "0 rows", which mis-guides the
        // planner once rows arrive; an empty DB instead gets its first stats
        // from the optimize maintenance task (or the next restart) once
        // populated. The existing populated DB analyzes immediately here.
        let has_rows: bool =
            conn.query_row("SELECT EXISTS(SELECT 1 FROM logs)", [], |r| r.get(0))?;
        if has_rows {
            conn.execute_batch("ANALYZE;")?;
        }
        conn.execute_batch("INSERT INTO schema_migrations (version) VALUES (24);")?;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            "Migration 24: AI covering indexes rebuilt + baseline ANALYZE done"
        );
    }

    // Migration 25: timeline_hourly rollup (bead syslog-mcp-kcvq).
    //
    // `timeline` (bucket=hour/day/week/month) and `stats.total_logs` previously
    // scanned the whole `logs` table (`strftime` GROUP BY ~3s; `COUNT(*)` ~7s on
    // a multi-million-row DB). This table materializes per-hour event counts at
    // grain (bucket_hour, hostname, app_name, severity) — ~9.3k rows over 2.65M
    // raw logs (~280x reduction), so timeline/stats reads become O(#buckets).
    //
    // INCREMENTAL, not full-recompute (contrast ai_session_rollup): a full
    // recompute is the 63s `strftime`-over-2.65M scan. The rollup holds ONLY
    // COUNT(*) — no MIN/MAX — so it is self-maintainable for ADDs: aggregate only
    // `logs WHERE id > source_max_id` and upsert-add into existing buckets. A
    // late-arriving high-id row with an old timestamp correctly lands in its old
    // bucket. The only incremental hazard is DELETEs (retention purges oldest
    // rows by received_at); the retention task prunes stale low buckets after
    // each purge (see spawn_retention_task), accepting a transient overcount only
    // in the single boundary hour.
    //
    // `app_name` is stored NOT NULL via COALESCE(app_name,'') — SQLite treats
    // NULLs as DISTINCT in UNIQUE/PK indexes, so a nullable column would make
    // ON CONFLICT never match for null-app grains and double-count every tick.
    //
    // First-run backfill is the one-time 63s scan, guarded by a has_rows check so
    // empty/test DBs skip it. It runs at server STARTUP before ingest begins, so
    // holding the write lock here is acceptable (same pattern as migration 24).
    if !migration_applied(&conn, 25)? {
        tracing::info!(
            "Migration 25: creating timeline_hourly rollup + backfill — backfill is \
             a one-time full scan (~60s on large DBs, write lock held)"
        );
        let started = std::time::Instant::now();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS timeline_hourly (
                 bucket      TEXT NOT NULL,
                 hostname    TEXT NOT NULL,
                 app_name    TEXT NOT NULL,
                 severity    TEXT NOT NULL,
                 event_count INTEGER NOT NULL,
                 PRIMARY KEY (bucket, hostname, app_name, severity)
             );
             CREATE TABLE IF NOT EXISTS timeline_hourly_meta (
                 id            INTEGER PRIMARY KEY CHECK (id = 1),
                 refreshed_at  TEXT,
                 source_max_id INTEGER NOT NULL DEFAULT 0
             );
             INSERT OR IGNORE INTO timeline_hourly_meta (id, refreshed_at, source_max_id)
                 VALUES (1, NULL, 0);",
        )?;
        // Backfill only when the table has data (fresh installs / tests skip the
        // scan and start from an empty rollup at watermark 0).
        let has_rows: bool =
            conn.query_row("SELECT EXISTS(SELECT 1 FROM logs)", [], |r| r.get(0))?;
        if has_rows {
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let max_id: i64 =
                tx.query_row("SELECT COALESCE(MAX(id), 0) FROM logs", [], |r| r.get(0))?;
            tx.execute(
                "INSERT INTO timeline_hourly (bucket, hostname, app_name, severity, event_count)
                 SELECT strftime('%Y-%m-%dT%H:00:00Z', timestamp) AS bucket,
                        hostname,
                        COALESCE(app_name, '') AS app_name,
                        severity,
                        COUNT(*) AS event_count
                 FROM logs
                 WHERE id <= ?1
                 GROUP BY bucket, hostname, app_name, severity
                 ON CONFLICT(bucket, hostname, app_name, severity)
                     DO UPDATE SET event_count = event_count + excluded.event_count",
                [max_id],
            )?;
            tx.execute(
                "UPDATE timeline_hourly_meta
                    SET refreshed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                        source_max_id = ?1
                  WHERE id = 1",
                [max_id],
            )?;
            tx.commit()?;
        }
        conn.execute_batch("INSERT INTO schema_migrations (version) VALUES (25);")?;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            "Migration 25: timeline_hourly rollup created + backfilled"
        );
    }

    // Migration 26: maintenance_jobs table (bead syslog-mcp-a4pd).
    //
    // `db integrity` on a 5GB DB is ~147s (PRAGMA quick_check reads every page —
    // unfixable). This table backs a server-side background job: the HTTP path
    // inserts a 'running' row, spawns the check on a blocking thread, and updates
    // the row to 'done'/'failed' + result_json; clients poll by id. quick_check
    // is read-only so it never blocks ingest writes.
    if !migration_applied(&conn, 26)? {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS maintenance_jobs (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 kind        TEXT NOT NULL,
                 status      TEXT NOT NULL,
                 started_at  TEXT NOT NULL,
                 finished_at TEXT,
                 result_json TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_maintenance_jobs_kind_status
                 ON maintenance_jobs(kind, status);
             INSERT INTO schema_migrations (version) VALUES (26);",
        )?;
        tracing::info!("Migration 26: created maintenance_jobs table");
    }

    // Migration 27: derived investigation graph projection (bead syslog-mcp-24vc.1).
    //
    // This is schema only: no ingest-path graph writes, no triggers, and no
    // service/API behavior. Raw logs, heartbeats, signatures, inventory, and AI
    // session rows remain authoritative; graph rows are rebuildable projection
    // data. Source references are intentionally soft references because this
    // process does not enable PRAGMA foreign_keys on pooled connections.
    if !migration_applied(&conn, 27)? {
        conn.execute_batch(
            "BEGIN IMMEDIATE;

             CREATE TABLE IF NOT EXISTS graph_entities (
                 id            INTEGER PRIMARY KEY AUTOINCREMENT,
                 entity_type   TEXT NOT NULL CHECK (entity_type IN (
                     'host', 'container', 'service', 'app', 'source_ip',
                     'ai_project', 'ai_session', 'error_signature'
                 )),
                 canonical_key TEXT NOT NULL,
                 display_label TEXT NOT NULL,
                 source_kind   TEXT NOT NULL DEFAULT '',
                 source_id     TEXT NOT NULL DEFAULT '',
                 trust_level   TEXT NOT NULL CHECK (trust_level IN (
                     'verified', 'claimed', 'inferred', 'correlated'
                 )),
                 first_seen_at TEXT,
                 last_seen_at  TEXT,
                 created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 UNIQUE(entity_type, canonical_key)
             );
             CREATE INDEX IF NOT EXISTS idx_graph_entities_type_key
                 ON graph_entities(entity_type, canonical_key);

             CREATE TABLE IF NOT EXISTS graph_entity_aliases (
                 id            INTEGER PRIMARY KEY AUTOINCREMENT,
                 entity_id     INTEGER NOT NULL,
                 alias_type    TEXT NOT NULL,
                 alias_key     TEXT NOT NULL,
                 alias_value   TEXT NOT NULL,
                 source_kind   TEXT NOT NULL DEFAULT '',
                 trust_level   TEXT NOT NULL CHECK (trust_level IN (
                     'verified', 'claimed', 'inferred', 'correlated'
                 )),
                 first_seen_at TEXT,
                 last_seen_at  TEXT,
                 created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 UNIQUE(entity_id, alias_type, alias_key, source_kind)
             );
             CREATE INDEX IF NOT EXISTS idx_graph_aliases_lookup
                 ON graph_entity_aliases(alias_type, alias_key);
             CREATE INDEX IF NOT EXISTS idx_graph_aliases_entity
                 ON graph_entity_aliases(entity_id);

             CREATE TABLE IF NOT EXISTS graph_relationships (
                 id                INTEGER PRIMARY KEY AUTOINCREMENT,
                 relationship_key  TEXT NOT NULL UNIQUE,
                 src_entity_id     INTEGER NOT NULL,
                 dst_entity_id     INTEGER NOT NULL,
                 relationship_type TEXT NOT NULL CHECK (relationship_type IN (
                     'observed_as', 'runs_on', 'emitted_by', 'worked_on',
                     'matches_signature'
                 )),
                 reason_code       TEXT NOT NULL CHECK (reason_code IN (
                     'syslog_claimed_hostname', 'log_app_name',
                     'docker_container_id', 'docker_service_label',
                     'ai_session_project', 'heartbeat_host_state',
                     'error_signature_match'
                 )),
                 trust_level       TEXT NOT NULL CHECK (trust_level IN (
                     'verified', 'claimed', 'inferred', 'correlated'
                 )),
                 confidence        REAL NOT NULL DEFAULT 0.0 CHECK (confidence >= 0.0 AND confidence <= 1.0),
                 evidence_count    INTEGER NOT NULL DEFAULT 0 CHECK (evidence_count >= 0),
                 first_seen_at     TEXT,
                 last_seen_at      TEXT,
                 created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 UNIQUE(src_entity_id, dst_entity_id, relationship_type, relationship_key)
             );
             CREATE INDEX IF NOT EXISTS idx_graph_relationships_src_type_seen
                 ON graph_relationships(src_entity_id, relationship_type, last_seen_at DESC);
             CREATE INDEX IF NOT EXISTS idx_graph_relationships_dst_type_seen
                 ON graph_relationships(dst_entity_id, relationship_type, last_seen_at DESC);

             CREATE TABLE IF NOT EXISTS graph_relationship_evidence (
                 id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                 relationship_id    INTEGER NOT NULL,
                 evidence_key       TEXT NOT NULL,
                 source_kind        TEXT NOT NULL CHECK (source_kind IN (
                     'log', 'heartbeat', 'ai_session_rollup', 'source_inventory',
                     'app_inventory', 'error_signature'
                 )),
                 source_id          TEXT NOT NULL DEFAULT '',
                 source_log_id      INTEGER,
                 source_heartbeat_id INTEGER,
                 source_signature_hash TEXT,
                 observed_at        TEXT NOT NULL,
                 reason_code        TEXT NOT NULL CHECK (reason_code IN (
                     'syslog_claimed_hostname', 'log_app_name',
                     'docker_container_id', 'docker_service_label',
                     'ai_session_project', 'heartbeat_host_state',
                     'error_signature_match'
                 )),
                 reason_text        TEXT,
                 confidence_delta   REAL NOT NULL DEFAULT 0.0 CHECK (confidence_delta >= -1.0 AND confidence_delta <= 1.0),
                 trust_level        TEXT NOT NULL CHECK (trust_level IN (
                     'verified', 'claimed', 'inferred', 'correlated'
                 )),
                 safe_excerpt       TEXT CHECK (safe_excerpt IS NULL OR length(safe_excerpt) <= 512),
                 metadata_path      TEXT,
                 evidence_count     INTEGER NOT NULL DEFAULT 1 CHECK (evidence_count >= 1),
                 created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 UNIQUE(relationship_id, evidence_key)
             );
             CREATE INDEX IF NOT EXISTS idx_graph_evidence_relationship_seen
                 ON graph_relationship_evidence(relationship_id, observed_at DESC);
             CREATE INDEX IF NOT EXISTS idx_graph_evidence_source_ref
                 ON graph_relationship_evidence(source_kind, source_id);
             CREATE INDEX IF NOT EXISTS idx_graph_evidence_log_id
                 ON graph_relationship_evidence(source_log_id)
                 WHERE source_log_id IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_graph_evidence_heartbeat_id
                 ON graph_relationship_evidence(source_heartbeat_id)
                 WHERE source_heartbeat_id IS NOT NULL;

             CREATE TABLE IF NOT EXISTS graph_projection_meta (
                 id                 INTEGER PRIMARY KEY CHECK (id = 1),
                 projection_status  TEXT NOT NULL CHECK (projection_status IN (
                     'never_built', 'building', 'ready', 'stale', 'failed'
                 )),
                 last_started_at    TEXT,
                 last_completed_at  TEXT,
                 source_watermark   TEXT NOT NULL DEFAULT '',
                 source_row_count   INTEGER NOT NULL DEFAULT 0 CHECK (source_row_count >= 0),
                 entity_count       INTEGER NOT NULL DEFAULT 0 CHECK (entity_count >= 0),
                 relationship_count INTEGER NOT NULL DEFAULT 0 CHECK (relationship_count >= 0),
                 evidence_count     INTEGER NOT NULL DEFAULT 0 CHECK (evidence_count >= 0),
                 is_degraded        INTEGER NOT NULL DEFAULT 0 CHECK (is_degraded IN (0, 1)),
                 last_error         TEXT CHECK (last_error IS NULL OR length(last_error) <= 2048),
                 updated_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             );
             INSERT OR IGNORE INTO graph_projection_meta
                 (id, projection_status, source_watermark)
                 VALUES (1, 'never_built', '');

             INSERT INTO schema_migrations (version) VALUES (27);

             COMMIT;",
        )?;
        tracing::info!("Migration 27: created graph projection schema");
    }

    // Migration 28: add graph rebuild runtime metrics.
    if !migration_applied(&conn, 28)? {
        let tx = conn.transaction()?;
        add_column_if_missing(
            &tx,
            "graph_projection_meta",
            "last_runtime_ms",
            "INTEGER NOT NULL DEFAULT 0 CHECK (last_runtime_ms >= 0)",
        )?;
        add_column_if_missing(
            &tx,
            "graph_projection_meta",
            "last_chunk_count",
            "INTEGER NOT NULL DEFAULT 0 CHECK (last_chunk_count >= 0)",
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO schema_migrations (version) VALUES (28)",
            [],
        )?;
        tx.commit()?;
        tracing::info!("Migration 28: added graph projection runtime metrics");
    }

    // A server crash/restart mid-check leaves an orphaned 'running' maintenance
    // job that would never resolve. Mark any such rows 'failed' on every startup
    // so polls get a terminal answer instead of hanging forever. Idempotent and
    // a no-op in the common case (no running jobs across a clean restart).
    conn.execute_batch(
        "UPDATE maintenance_jobs
            SET status = 'failed',
                finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                result_json = json_object('error', 'interrupted by server restart')
          WHERE status = 'running';",
    )?;

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_logs_ai_project_time
             ON logs(ai_project, timestamp)
             WHERE ai_project IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_logs_ai_session
             ON logs(ai_tool, ai_project, ai_session_id)
             WHERE ai_tool IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_logs_ai_session_host_time
             ON logs(ai_project, ai_tool, ai_session_id, hostname, timestamp)
             WHERE ai_project IS NOT NULL
               AND ai_tool IS NOT NULL
               AND ai_session_id IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_logs_ai_transcript_path
             ON logs(ai_transcript_path)
             WHERE ai_transcript_path IS NOT NULL;",
    )?;

    tracing::info!(path = %config.db_path.display(), "Database initialized");
    Ok(pool)
}

fn migration_applied(conn: &Connection, version: i64) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
        [version],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
        [table, column],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    column_type: &str,
) -> rusqlite::Result<()> {
    if !column_exists(conn, table, column)? {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {column_type};"
        ))?;
    }
    Ok(())
}

fn apply_migration_17_inventory_stats(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "BEGIN IMMEDIATE;

         CREATE TABLE IF NOT EXISTS app_inventory_stats (
             app_name   TEXT PRIMARY KEY,
             log_count  INTEGER NOT NULL DEFAULT 0,
             first_seen TEXT NOT NULL,
             last_seen  TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_app_inventory_last_seen
             ON app_inventory_stats(last_seen DESC, app_name ASC);

         CREATE TABLE IF NOT EXISTS app_host_inventory_stats (
             app_name   TEXT NOT NULL,
             hostname   TEXT NOT NULL,
             log_count  INTEGER NOT NULL DEFAULT 0,
             first_seen TEXT NOT NULL,
             last_seen  TEXT NOT NULL,
             PRIMARY KEY (app_name, hostname)
         );
         CREATE INDEX IF NOT EXISTS idx_app_host_inventory_count
             ON app_host_inventory_stats(app_name, log_count DESC, hostname ASC);

         CREATE TABLE IF NOT EXISTS source_ip_inventory_stats (
             source_ip  TEXT PRIMARY KEY,
             log_count  INTEGER NOT NULL DEFAULT 0,
             first_seen TEXT NOT NULL,
             last_seen  TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_source_ip_inventory_count
             ON source_ip_inventory_stats(log_count DESC, source_ip ASC);

         CREATE TABLE IF NOT EXISTS source_ip_host_inventory_stats (
             source_ip  TEXT NOT NULL,
             hostname   TEXT NOT NULL,
             log_count  INTEGER NOT NULL DEFAULT 0,
             first_seen TEXT NOT NULL,
             last_seen  TEXT NOT NULL,
             PRIMARY KEY (source_ip, hostname)
         );
         CREATE INDEX IF NOT EXISTS idx_source_ip_host_inventory_count
             ON source_ip_host_inventory_stats(source_ip, log_count DESC, hostname ASC);

         CREATE TABLE IF NOT EXISTS inventory_backfill_state (
             name         TEXT PRIMARY KEY,
             completed_at TEXT,
             last_error   TEXT,
             last_log_id  INTEGER NOT NULL DEFAULT 0,
             high_watermark_id INTEGER
         );
         INSERT OR IGNORE INTO inventory_backfill_state(name)
         VALUES ('app_source_inventory');

         DROP TRIGGER IF EXISTS logs_inventory_app_ai;
         DROP TRIGGER IF EXISTS logs_inventory_app_ad;
         DROP TRIGGER IF EXISTS logs_inventory_source_ip_ai;
         DROP TRIGGER IF EXISTS logs_inventory_source_ip_ad;

         CREATE TRIGGER logs_inventory_app_ai AFTER INSERT ON logs
         WHEN NEW.app_name IS NOT NULL AND NEW.app_name != ''
         BEGIN
             INSERT INTO app_inventory_stats(app_name, log_count, first_seen, last_seen)
             VALUES (NEW.app_name, 1, NEW.received_at, NEW.received_at)
             ON CONFLICT(app_name) DO UPDATE SET
                 log_count = log_count + 1,
                 first_seen = min(first_seen, excluded.first_seen),
                 last_seen = max(last_seen, excluded.last_seen);

             INSERT INTO app_host_inventory_stats(app_name, hostname, log_count, first_seen, last_seen)
             VALUES (NEW.app_name, NEW.hostname, 1, NEW.received_at, NEW.received_at)
             ON CONFLICT(app_name, hostname) DO UPDATE SET
                 log_count = log_count + 1,
                 first_seen = min(first_seen, excluded.first_seen),
                 last_seen = max(last_seen, excluded.last_seen);
         END;

         CREATE TRIGGER logs_inventory_app_ad AFTER DELETE ON logs
         WHEN OLD.app_name IS NOT NULL AND OLD.app_name != ''
         BEGIN
             UPDATE app_inventory_stats
             SET log_count = log_count - 1
             WHERE app_name = OLD.app_name;
             DELETE FROM app_inventory_stats
             WHERE app_name = OLD.app_name AND log_count <= 0;

             UPDATE app_host_inventory_stats
             SET log_count = log_count - 1
             WHERE app_name = OLD.app_name AND hostname = OLD.hostname;
             DELETE FROM app_host_inventory_stats
             WHERE app_name = OLD.app_name AND hostname = OLD.hostname AND log_count <= 0;
         END;

         CREATE TRIGGER logs_inventory_source_ip_ai AFTER INSERT ON logs
         WHEN NEW.source_ip != ''
         BEGIN
             INSERT INTO source_ip_inventory_stats(source_ip, log_count, first_seen, last_seen)
             VALUES (NEW.source_ip, 1, NEW.received_at, NEW.received_at)
             ON CONFLICT(source_ip) DO UPDATE SET
                 log_count = log_count + 1,
                 first_seen = min(first_seen, excluded.first_seen),
                 last_seen = max(last_seen, excluded.last_seen);

             INSERT INTO source_ip_host_inventory_stats(source_ip, hostname, log_count, first_seen, last_seen)
             VALUES (NEW.source_ip, NEW.hostname, 1, NEW.received_at, NEW.received_at)
             ON CONFLICT(source_ip, hostname) DO UPDATE SET
                 log_count = log_count + 1,
                 first_seen = min(first_seen, excluded.first_seen),
                 last_seen = max(last_seen, excluded.last_seen);
         END;

         CREATE TRIGGER logs_inventory_source_ip_ad AFTER DELETE ON logs
         WHEN OLD.source_ip != ''
         BEGIN
             UPDATE source_ip_inventory_stats
             SET log_count = log_count - 1
             WHERE source_ip = OLD.source_ip;
             DELETE FROM source_ip_inventory_stats
             WHERE source_ip = OLD.source_ip AND log_count <= 0;

             UPDATE source_ip_host_inventory_stats
             SET log_count = log_count - 1
             WHERE source_ip = OLD.source_ip AND hostname = OLD.hostname;
             DELETE FROM source_ip_host_inventory_stats
             WHERE source_ip = OLD.source_ip AND hostname = OLD.hostname AND log_count <= 0;
         END;

         INSERT OR IGNORE INTO schema_migrations (version) VALUES (17);
         COMMIT;",
    )?;
    tracing::info!("Migration 17: created app/source inventory stats tables and triggers");
    Ok(())
}

pub fn inventory_backfill_complete(pool: &DbPool) -> Result<bool> {
    let conn = pool.get()?;
    let complete = conn.query_row(
        "SELECT completed_at IS NOT NULL
         FROM inventory_backfill_state
         WHERE name = 'app_source_inventory'",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    Ok(complete)
}

fn ensure_inventory_backfill_state_columns(conn: &Connection) -> rusqlite::Result<()> {
    add_column_if_missing(
        conn,
        "inventory_backfill_state",
        "last_log_id",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        conn,
        "inventory_backfill_state",
        "high_watermark_id",
        "INTEGER",
    )?;
    Ok(())
}

pub fn backfill_inventory_stats(pool: &DbPool) -> Result<()> {
    const CHUNK_SIZE: i64 = 25_000;
    const BETWEEN_CHUNKS: std::time::Duration = std::time::Duration::from_millis(25);

    if inventory_backfill_complete(pool)? {
        return Ok(());
    }
    let conn = pool.get()?;
    ensure_inventory_backfill_state_columns(&conn)?;
    tracing::info!(
        "Inventory stats backfill starting — queries may fall back to logs until this completes"
    );
    let started = std::time::Instant::now();

    loop {
        let (last_log_id, high_watermark_id): (i64, Option<i64>) = conn.query_row(
            "SELECT last_log_id, high_watermark_id
             FROM inventory_backfill_state
             WHERE name = 'app_source_inventory'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let high_watermark_id = match high_watermark_id {
            Some(id) => id,
            None => {
                let high: i64 =
                    conn.query_row("SELECT COALESCE(MAX(id), 0) FROM logs", [], |row| {
                        row.get(0)
                    })?;
                conn.execute_batch(
                    "BEGIN IMMEDIATE;
                     DELETE FROM app_inventory_stats;
                     DELETE FROM app_host_inventory_stats;
                     DELETE FROM source_ip_inventory_stats;
                     DELETE FROM source_ip_host_inventory_stats;",
                )?;
                conn.execute(
                    "UPDATE inventory_backfill_state
                     SET last_log_id = 0,
                         high_watermark_id = ?1,
                         completed_at = NULL,
                         last_error = NULL
                     WHERE name = 'app_source_inventory'",
                    [high],
                )?;
                conn.execute_batch("COMMIT;")?;
                high
            }
        };

        if last_log_id >= high_watermark_id {
            conn.execute(
                "UPDATE inventory_backfill_state
                 SET completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     last_error = NULL
                 WHERE name = 'app_source_inventory'",
                [],
            )?;
            tracing::info!(
                elapsed_ms = started.elapsed().as_millis(),
                high_watermark_id,
                "Inventory stats backfill completed"
            );
            return Ok(());
        }

        let next_log_id = (last_log_id + CHUNK_SIZE).min(high_watermark_id);
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        let result = (|| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO app_inventory_stats(app_name, log_count, first_seen, last_seen)
                 SELECT app_name, COUNT(*), MIN(received_at), MAX(received_at)
                 FROM logs
                 WHERE id > ?1
                   AND id <= ?2
                   AND app_name IS NOT NULL
                   AND app_name != ''
                 GROUP BY app_name
                 ON CONFLICT(app_name) DO UPDATE SET
                     log_count = log_count + excluded.log_count,
                     first_seen = min(first_seen, excluded.first_seen),
                     last_seen = max(last_seen, excluded.last_seen)",
                (last_log_id, next_log_id),
            )?;
            conn.execute(
                "INSERT INTO app_host_inventory_stats(app_name, hostname, log_count, first_seen, last_seen)
                 SELECT app_name, hostname, COUNT(*), MIN(received_at), MAX(received_at)
                 FROM logs
                 WHERE id > ?1
                   AND id <= ?2
                   AND app_name IS NOT NULL
                   AND app_name != ''
                 GROUP BY app_name, hostname
                 ON CONFLICT(app_name, hostname) DO UPDATE SET
                     log_count = log_count + excluded.log_count,
                     first_seen = min(first_seen, excluded.first_seen),
                     last_seen = max(last_seen, excluded.last_seen)",
                (last_log_id, next_log_id),
            )?;
            conn.execute(
                "INSERT INTO source_ip_inventory_stats(source_ip, log_count, first_seen, last_seen)
                 SELECT source_ip, COUNT(*), MIN(received_at), MAX(received_at)
                 FROM logs
                 WHERE id > ?1
                   AND id <= ?2
                   AND source_ip != ''
                 GROUP BY source_ip
                 ON CONFLICT(source_ip) DO UPDATE SET
                     log_count = log_count + excluded.log_count,
                     first_seen = min(first_seen, excluded.first_seen),
                     last_seen = max(last_seen, excluded.last_seen)",
                (last_log_id, next_log_id),
            )?;
            conn.execute(
                "INSERT INTO source_ip_host_inventory_stats(source_ip, hostname, log_count, first_seen, last_seen)
                 SELECT source_ip, hostname, COUNT(*), MIN(received_at), MAX(received_at)
                 FROM logs
                 WHERE id > ?1
                   AND id <= ?2
                   AND source_ip != ''
                 GROUP BY source_ip, hostname
                 ON CONFLICT(source_ip, hostname) DO UPDATE SET
                     log_count = log_count + excluded.log_count,
                     first_seen = min(first_seen, excluded.first_seen),
                     last_seen = max(last_seen, excluded.last_seen)",
                (last_log_id, next_log_id),
            )?;
            conn.execute(
                "UPDATE inventory_backfill_state
                 SET last_log_id = ?1,
                     last_error = NULL
                 WHERE name = 'app_source_inventory'",
                [next_log_id],
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => conn.execute_batch("COMMIT;")?,
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK;");
                let _ = conn.execute(
                    "UPDATE inventory_backfill_state
                     SET last_error = ?1
                     WHERE name = 'app_source_inventory'",
                    [error.to_string()],
                );
                return Err(error.into());
            }
        }
        tracing::debug!(
            last_log_id = next_log_id,
            high_watermark_id,
            "Inventory stats backfill chunk completed"
        );
        std::thread::sleep(BETWEEN_CHUNKS);
    }
}

fn apply_migration_13(conn: &Connection) -> rusqlite::Result<()> {
    // Explicit transaction keeps index/version updates atomic, while each ALTER is
    // guarded so manually repaired or partially migrated DBs can converge instead
    // of failing on duplicate columns with no version row.
    conn.execute_batch("BEGIN IMMEDIATE;")?;
    let result = (|| {
        add_column_if_missing(conn, "logs", "http_status", "INTEGER")?;
        add_column_if_missing(conn, "logs", "auth_outcome", "TEXT")?;
        add_column_if_missing(conn, "logs", "dns_blocked", "INTEGER")?;
        add_column_if_missing(conn, "logs", "event_action", "TEXT")?;
        add_column_if_missing(conn, "logs", "parse_error", "TEXT")?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_logs_http_status_time
                 ON logs(http_status, timestamp) WHERE http_status IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_logs_auth_outcome_time
                 ON logs(auth_outcome, timestamp) WHERE auth_outcome IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_logs_dns_blocked_time
                 ON logs(dns_blocked, timestamp) WHERE dns_blocked IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_logs_event_action_time
                 ON logs(event_action, timestamp) WHERE event_action IS NOT NULL;
             INSERT OR IGNORE INTO schema_migrations (version) VALUES (13);",
        )
    })();

    match result {
        Ok(()) => conn.execute_batch("COMMIT;"),
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(error)
        }
    }
}

// Migration 22: source watermark for the AI session rollup (bead cortex-g33v).
// The background refresh recomputed the full GROUP-BY over `logs` every cadence
// even when no AI rows had changed. These two columns record the source-side
// `(COUNT(*), MAX(id))` of AI rows captured by the last refresh; the refresh
// task compares the live watermark against them and skips the recompute when
// nothing changed. Both default to 0 so the first post-migration refresh always
// runs (live watermark > 0 whenever AI rows exist, and `refreshed_at` is still
// NULL regardless).
//
// Wrapped in an explicit BEGIN IMMEDIATE / COMMIT-or-ROLLBACK transaction so a
// crash between the two ALTERs and the version marker rolls back BOTH columns
// and the marker atomically — the previous bare `execute_batch` auto-committed
// each statement, leaving a half-applied DB that bricked `init_pool` on restart
// with "duplicate column name". Each ALTER is guarded with `add_column_if_missing`
// so a partially-applied DB (columns present, marker absent) converges on retry.
fn apply_migration_22(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE;")?;
    let result = (|| {
        add_column_if_missing(
            conn,
            "ai_session_rollup_meta",
            "source_row_count",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(
            conn,
            "ai_session_rollup_meta",
            "source_max_id",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        conn.execute_batch("INSERT OR IGNORE INTO schema_migrations (version) VALUES (22);")
    })();

    match result {
        Ok(()) => conn.execute_batch("COMMIT;"),
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(error)
        }
    }
}

fn apply_migration_15_heartbeat(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE;")?;
    let result = conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS host_heartbeats (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            host_id         TEXT NOT NULL,
            hostname        TEXT NOT NULL,
            source_ip       TEXT NOT NULL DEFAULT '',
            sampled_at      TEXT NOT NULL,
            received_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            boot_id         TEXT NOT NULL,
            uptime_secs     INTEGER NOT NULL,
            sequence        INTEGER NOT NULL,
            collection_ms   INTEGER NOT NULL,
            push_latency_ms INTEGER,
            partial         INTEGER NOT NULL DEFAULT 0,
            agent_version   TEXT NOT NULL,
            os              TEXT NOT NULL,
            kernel          TEXT,
            architecture    TEXT NOT NULL,
            metadata_json   TEXT,
            UNIQUE(host_id, boot_id, sequence)
        );

        CREATE INDEX IF NOT EXISTS idx_host_heartbeats_host_sampled
            ON host_heartbeats(host_id, sampled_at);
        CREATE INDEX IF NOT EXISTS idx_host_heartbeats_received
            ON host_heartbeats(received_at);
        CREATE INDEX IF NOT EXISTS idx_host_heartbeats_hostname_sampled
            ON host_heartbeats(hostname, sampled_at);

        CREATE TABLE IF NOT EXISTS heartbeat_cpu (
            heartbeat_id      INTEGER NOT NULL,
            load1             REAL,
            load5             REAL,
            load15            REAL,
            usage_percent     REAL,
            steal_percent     REAL,
            io_wait_percent   REAL
        );
        CREATE INDEX IF NOT EXISTS idx_heartbeat_cpu_heartbeat_id
            ON heartbeat_cpu(heartbeat_id);

        CREATE TABLE IF NOT EXISTS heartbeat_memory (
            heartbeat_id      INTEGER NOT NULL,
            total_bytes       INTEGER,
            available_bytes   INTEGER,
            used_percent      REAL,
            swap_total_bytes  INTEGER,
            swap_used_bytes   INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_heartbeat_memory_heartbeat_id
            ON heartbeat_memory(heartbeat_id);

        CREATE TABLE IF NOT EXISTS heartbeat_disks (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            heartbeat_id        INTEGER NOT NULL,
            mountpoint          TEXT,
            filesystem          TEXT,
            total_bytes         INTEGER,
            available_bytes     INTEGER,
            used_percent        REAL,
            read_bytes_per_sec  REAL,
            write_bytes_per_sec REAL
        );
        CREATE INDEX IF NOT EXISTS idx_heartbeat_disks_heartbeat_id
            ON heartbeat_disks(heartbeat_id);

        CREATE TABLE IF NOT EXISTS heartbeat_network (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            heartbeat_id     INTEGER NOT NULL,
            interface        TEXT NOT NULL,
            rx_bytes_per_sec REAL,
            tx_bytes_per_sec REAL,
            rx_errors        INTEGER,
            tx_errors        INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_heartbeat_network_heartbeat_id
            ON heartbeat_network(heartbeat_id);

        CREATE TABLE IF NOT EXISTS heartbeat_processes (
            heartbeat_id    INTEGER NOT NULL,
            total           INTEGER,
            running         INTEGER,
            sleeping        INTEGER,
            zombie          INTEGER,
            top_cpu_json    TEXT,
            top_memory_json TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_heartbeat_processes_heartbeat_id
            ON heartbeat_processes(heartbeat_id);

        CREATE TABLE IF NOT EXISTS heartbeat_containers (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            heartbeat_id  INTEGER NOT NULL,
            runtime       TEXT,
            running       INTEGER,
            stopped       INTEGER,
            restarting    INTEGER,
            unhealthy     INTEGER,
            summary_json  TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_heartbeat_containers_heartbeat_id
            ON heartbeat_containers(heartbeat_id);

        INSERT OR IGNORE INTO schema_migrations (version) VALUES (15);
        ",
    );

    match result {
        Ok(()) => conn.execute_batch("COMMIT;"),
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(error)
        }
    }
}

fn apply_migration_18_heartbeat_restarting(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE;")?;
    let result = (|| {
        add_column_if_missing(conn, "heartbeat_containers", "restarting", "INTEGER")?;
        conn.execute_batch("INSERT OR IGNORE INTO schema_migrations (version) VALUES (18);")
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT;"),
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(error)
        }
    }
}

/// Migration 19: `host_heartbeats_latest` — one row per host_id, updated on
/// every new accepted heartbeat. This is the foundation for `fleet_state`
/// queries: instead of scanning `host_heartbeats` for the latest row per host
/// (O(heartbeats)), fleet queries scan this small table (O(hosts)).
///
/// Backfill on first apply: for each distinct `host_id`, find the row with the
/// highest `id` (proxy for latest, since `id` is AUTOINCREMENT) and seed the
/// cache. The GROUP BY scan happens once at migration time, not per query.
fn apply_migration_19_heartbeat_latest(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE;")?;
    let result = (|| {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS host_heartbeats_latest (
                 host_id       TEXT PRIMARY KEY,
                 heartbeat_id  INTEGER NOT NULL,
                 hostname      TEXT NOT NULL,
                 sampled_at    TEXT NOT NULL,
                 received_at   TEXT NOT NULL,
                 partial       INTEGER NOT NULL DEFAULT 0,
                 agent_version TEXT NOT NULL DEFAULT '',
                 os            TEXT NOT NULL DEFAULT '',
                 architecture  TEXT NOT NULL DEFAULT '',
                 metadata_json TEXT
             );
             INSERT OR IGNORE INTO host_heartbeats_latest
                 (host_id, heartbeat_id, hostname, sampled_at, received_at,
                  partial, agent_version, os, architecture, metadata_json)
             SELECT h.host_id, h.id, h.hostname, h.sampled_at, h.received_at,
                    h.partial, h.agent_version, h.os, h.architecture, h.metadata_json
             FROM host_heartbeats h
             INNER JOIN (
                 SELECT host_id, MAX(id) AS max_id
                 FROM host_heartbeats
                 GROUP BY host_id
             ) latest ON h.id = latest.max_id;",
        )?;
        conn.execute_batch("INSERT OR IGNORE INTO schema_migrations (version) VALUES (19);")
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT;"),
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(error)
        }
    }
}

fn configure_connection_pragmas(conn: &mut Connection, wal_mode: bool) -> rusqlite::Result<()> {
    if wal_mode {
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    }
    conn.execute_batch(
        "PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=5000;
         PRAGMA cache_size=-64000;
         PRAGMA analysis_limit=400;",
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;
