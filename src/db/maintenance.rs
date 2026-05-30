use std::path::Path;

use anyhow::Result;
use chrono::Utc;
use rusqlite::params;

use crate::config::StorageConfig;

use super::models::{StorageEnforcementOutcome, StorageMetrics, StorageRecovery};
use super::pool::DbPool;

pub trait DiskSpaceProbe {
    fn free_bytes(&self, path: &Path) -> Result<u64>;
}

struct SystemDiskSpaceProbe;

impl DiskSpaceProbe for SystemDiskSpaceProbe {
    fn free_bytes(&self, path: &Path) -> Result<u64> {
        let stats = rustix::fs::statvfs(path)?;
        Ok(stats.f_bavail.saturating_mul(stats.f_bsize))
    }
}

pub fn get_storage_metrics(pool: &DbPool, config: &StorageConfig) -> Result<StorageMetrics> {
    get_storage_metrics_with_probe(pool, config, &SystemDiskSpaceProbe)
}

pub fn physical_size_bytes(path: &Path) -> Result<u64> {
    physical_db_size_bytes(path)
}

pub fn db_wal_checkpoint(pool: &DbPool, mode: &str) -> Result<(i64, i64, i64)> {
    let mode = match mode {
        "passive" => "PASSIVE",
        "full" => "FULL",
        "restart" => "RESTART",
        "truncate" => "TRUNCATE",
        other => anyhow::bail!("unsupported WAL checkpoint mode: {other}"),
    };
    let sql = format!("PRAGMA wal_checkpoint({mode})");
    let conn = pool.get()?;
    let result = conn.query_row(&sql, [], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    Ok(result)
}

pub fn db_incremental_vacuum(pool: &DbPool, pages: u32) -> Result<()> {
    let conn = pool.get()?;
    conn.execute_batch(&format!("PRAGMA incremental_vacuum({pages});"))?;
    Ok(())
}

pub fn db_full_vacuum(pool: &DbPool) -> Result<()> {
    let conn = pool.get()?;
    conn.execute_batch("VACUUM;")?;
    Ok(())
}

/// Run `PRAGMA integrity_check` (full) or `PRAGMA quick_check` (skips
/// cross-row consistency, ~10x faster on multi-GB databases).
pub fn db_integrity_check(pool: &DbPool, quick: bool) -> Result<Vec<String>> {
    let conn = pool.get()?;
    let pragma = if quick {
        "quick_check"
    } else {
        "integrity_check"
    };
    let mut stmt = conn.prepare(&format!("PRAGMA {pragma}"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let messages = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(messages)
}

/// Type-safe PRAGMA identifier. The `pub(crate)` field prevents external crates
/// from constructing arbitrary values — only crate-internal code can build one,
/// and only from `'static` str literals, enforcing the SQL interpolation contract.
pub(crate) struct PragmaName(pub(crate) &'static str);

/// Reads a trusted, hardcoded integer PRAGMA.
pub(crate) fn db_pragma_i64(pool: &DbPool, pragma: PragmaName) -> Result<i64> {
    let conn = pool.get()?;
    Ok(conn.query_row(&format!("PRAGMA {}", pragma.0), [], |row| row.get(0))?)
}

/// Reads a trusted, hardcoded string PRAGMA.
pub(crate) fn db_pragma_string(pool: &DbPool, pragma: PragmaName) -> Result<String> {
    let conn = pool.get()?;
    Ok(conn.query_row(&format!("PRAGMA {}", pragma.0), [], |row| row.get(0))?)
}

pub fn get_storage_metrics_with_probe(
    pool: &DbPool,
    config: &StorageConfig,
    probe: &impl DiskSpaceProbe,
) -> Result<StorageMetrics> {
    let conn = pool.get()?;
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let freelist_count: i64 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    drop(conn);

    let logical_db_size_bytes = ((page_count - freelist_count).max(0) * page_size).max(0) as u64;
    let physical_db_size_bytes = physical_db_size_bytes(&config.db_path)?;
    let free_disk_bytes = probe
        .free_bytes(config.db_path.parent().unwrap_or_else(|| Path::new(".")))
        .ok();
    tracing::debug!(
        logical_db_size_bytes,
        physical_db_size_bytes,
        free_disk_bytes = ?free_disk_bytes,
        db_path = %config.db_path.display(),
        "Collected storage metrics"
    );

    Ok(StorageMetrics {
        logical_db_size_bytes,
        physical_db_size_bytes,
        free_disk_bytes,
    })
}

pub fn enforce_storage_budget(
    pool: &DbPool,
    config: &StorageConfig,
) -> Result<StorageEnforcementOutcome> {
    enforce_storage_budget_with_probe(pool, config, &SystemDiskSpaceProbe)
}

pub fn enforce_storage_budget_with_probe(
    pool: &DbPool,
    config: &StorageConfig,
    probe: &impl DiskSpaceProbe,
) -> Result<StorageEnforcementOutcome> {
    let recovery = recovery_targets(config);
    let mut deleted_rows = 0usize;
    let mut deleted_log_rows = 0usize;
    let mut all_hosts: std::collections::HashSet<String> = Default::default();

    let mut metrics = get_storage_metrics_with_probe(pool, config, probe)?;
    tracing::debug!(
        logical_db_size_bytes = metrics.logical_db_size_bytes,
        physical_db_size_bytes = metrics.physical_db_size_bytes,
        free_disk_bytes = ?metrics.free_disk_bytes,
        max_db_size_mb = config.max_db_size_mb,
        recovery_db_size_mb = config.recovery_db_size_mb,
        min_free_disk_mb = config.min_free_disk_mb,
        recovery_free_disk_mb = config.recovery_free_disk_mb,
        "Storage budget enforcement check started"
    );
    if !storage_limits_enabled(config) {
        tracing::debug!("Storage limits disabled — skipping enforcement");
        return Ok(StorageEnforcementOutcome {
            metrics,
            recovery,
            deleted_rows,
            write_blocked: false,
        });
    }

    // Only enter the cleanup loop if we have actually exceeded a trigger.
    if exceeds_trigger(&metrics, config) {
        while !within_recovery(&metrics, &recovery, config) {
            tracing::warn!(
                logical_db_size_bytes = metrics.logical_db_size_bytes,
                physical_db_size_bytes = metrics.physical_db_size_bytes,
                free_disk_bytes = ?metrics.free_disk_bytes,
                deleted_rows,
                "Storage budget exceeded trigger — deleting oldest telemetry chunk"
            );

            let deleted_orphan_children = delete_orphan_heartbeat_children(pool)?;
            if deleted_orphan_children > 0 {
                deleted_rows += deleted_orphan_children;
                tracing::info!(
                    deleted_rows = deleted_orphan_children,
                    total_deleted_rows = deleted_rows,
                    "Deleted orphan heartbeat child rows for storage recovery"
                );
                metrics = get_storage_metrics_with_probe(pool, config, probe)?;
                continue;
            }

            let deleted = match oldest_telemetry_source(pool)? {
                Some(TelemetrySource::Heartbeats) => {
                    let deleted = delete_oldest_heartbeats_chunk(pool, config.cleanup_chunk_size)?;
                    DeletedTelemetryChunk {
                        deleted_rows: deleted,
                        log_hostnames: Vec::new(),
                        source: TelemetrySource::Heartbeats,
                    }
                }
                Some(TelemetrySource::Logs) => {
                    let deleted = delete_oldest_logs_chunk(pool, config.cleanup_chunk_size)?;
                    DeletedTelemetryChunk {
                        deleted_rows: deleted.deleted_rows,
                        log_hostnames: deleted.hostnames,
                        source: TelemetrySource::Logs,
                    }
                }
                None => DeletedTelemetryChunk {
                    deleted_rows: 0,
                    log_hostnames: Vec::new(),
                    source: TelemetrySource::Logs,
                },
            };
            if deleted.deleted_rows == 0 {
                metrics = get_storage_metrics_with_probe(pool, config, probe)?;
                let write_blocked = exceeds_trigger(&metrics, config);
                tracing::warn!(
                    logical_db_size_bytes = metrics.logical_db_size_bytes,
                    free_disk_bytes = ?metrics.free_disk_bytes,
                    deleted_rows,
                    write_blocked,
                    "Storage budget enforcement could not delete more rows"
                );
                return Ok(StorageEnforcementOutcome {
                    metrics,
                    recovery,
                    deleted_rows,
                    write_blocked,
                });
            }

            deleted_rows += deleted.deleted_rows;
            if deleted.source == TelemetrySource::Logs {
                deleted_log_rows += deleted.deleted_rows;
            }
            tracing::info!(
                deleted_rows = deleted.deleted_rows,
                total_deleted_rows = deleted_rows,
                source = ?deleted.source,
                affected_hosts = deleted.log_hostnames.len(),
                "Deleted oldest telemetry chunk for storage recovery"
            );
            all_hosts.extend(deleted.log_hostnames);
            metrics = get_storage_metrics_with_probe(pool, config, probe)?;
        }
    }

    if deleted_rows > 0 {
        // Reconcile hosts once after all chunks — avoids N×3 SQL round-trips
        // (one per chunk × 3 queries per hostname) competing with the batch writer.
        let host_list: Vec<String> = all_hosts.into_iter().collect();
        reconcile_hosts(pool, &host_list)?;

        // Incremental FTS merge — clean up phantom rows left by bulk deletes
        // (DELETE trigger is intentionally absent).
        // drop the connection before checkpoint_wal_and_incremental_vacuum to
        // avoid pool exhaustion when pool_size = 1.
        // Hardcoded M=0 here — storage enforcement is rare, force unconditional
        // merge. Tunable M only matters for the regular retention path.
        if deleted_log_rows > 0 {
            fts_incremental_merge(pool, deleted_log_rows, 0);
        }

        checkpoint_wal_and_incremental_vacuum(pool)?;
    }

    tracing::debug!(
        deleted_rows,
        logical_db_size_bytes = metrics.logical_db_size_bytes,
        physical_db_size_bytes = metrics.physical_db_size_bytes,
        free_disk_bytes = ?metrics.free_disk_bytes,
        "Storage budget enforcement completed"
    );

    Ok(StorageEnforcementOutcome {
        metrics,
        recovery,
        deleted_rows,
        write_blocked: false,
    })
}

/// Run an incremental FTS5 merge to clean up phantom rows left by bulk DELETEs.
///
/// A single `merge=500,250` call processes at most ~500 FTS index pages, which
/// covers <1% of phantoms after a 500k-row delete. This function scales the
/// number of merge iterations proportionally to `deleted_rows` (one iteration
/// per 5 000 rows, capped at 20) and falls back to a forced `rebuild` after
/// 3 consecutive failures — a last-resort recovery for a corrupt or severely
/// fragmented FTS index.
///
/// Best-effort: errors are logged but never propagated.
fn fts_incremental_merge(pool: &DbPool, deleted_rows: usize, merge_pages: u32) {
    // Budget one merge=500,M call per 5 000 deleted rows (rough heuristic),
    // with a floor of 1 and a ceiling of 20 to bound wall-clock time.
    let iterations = deleted_rows.div_ceil(5000).clamp(1, 20);
    let mut consecutive_failures: u32 = 0;
    // M=0 forces unconditional merge regardless of segment count, which is
    // the right choice after bulk DELETEs (level-0 segments may be too few
    // to satisfy the default M=250 threshold). Operators can raise M via
    // CORTEX_FTS_MERGE_PAGES if M=0 holds the write lock too long on a
    // very large index — config rollback rather than binary rollback.
    let merge_stmt = format!("INSERT INTO logs_fts(logs_fts) VALUES('merge=500,{merge_pages}');");

    for i in 0..iterations {
        match pool.get() {
            Ok(conn) => {
                match conn.execute_batch(&merge_stmt) {
                    Ok(()) => {
                        consecutive_failures = 0;
                        tracing::trace!(
                            iteration = i + 1,
                            total_iterations = iterations,
                            "FTS incremental merge iteration"
                        );
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        tracing::warn!(
                            error = %e,
                            iteration = i + 1,
                            consecutive_failures,
                            "FTS incremental merge failed; attempting optimize as fallback"
                        );

                        // Best-effort fallback to optimize if merge is rejected by SQLite
                        if let Ok(optimize_conn) = pool.get() {
                            let _ = optimize_conn.execute_batch(
                                "INSERT INTO logs_fts(logs_fts) VALUES('optimize');",
                            );
                        }

                        if consecutive_failures >= 3 {
                            // Escalate to full rebuild — last-resort recovery for a
                            // corrupt or severely fragmented FTS index.
                            match pool.get() {
                                Ok(rebuild_conn) => {
                                    if let Err(e) = rebuild_conn.execute_batch(
                                        "INSERT INTO logs_fts(logs_fts) VALUES('rebuild');",
                                    ) {
                                        tracing::error!(error = %e, "FTS forced rebuild failed");
                                    } else {
                                        tracing::error!(
                                            "FTS incremental merge failed 3 times; forced rebuild completed"
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        error = %e,
                                        "FTS forced rebuild: failed to get connection"
                                    );
                                }
                            }
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "FTS incremental merge: failed to get connection");
                consecutive_failures += 1;
                if consecutive_failures >= 3 {
                    tracing::error!(
                        "FTS incremental merge: 3 consecutive connection failures, giving up"
                    );
                    return;
                }
            }
        }
    }
}

/// Purge logs older than N days.
///
/// Uses chunked DELETEs (10 000 rows per iteration) so the WAL write lock is
/// released between chunks, letting the batch writer proceed without timing out
/// or overflowing its 1 000-entry cap.  After all chunks complete, an
/// incremental FTS5 merge is issued instead of a full rebuild — `merge=500,M`
/// processes at most a bounded number of index pages per call and holds the
/// write lock for milliseconds rather than seconds.
///
/// **High-severity exemption:** rows with `severity IN ('err','crit','alert','emerg')`
/// are excluded from time-based purge — they are never aged out by retention.
/// They CAN still be deleted by `enforce_storage_budget` under disk pressure
/// (oldest-first, no severity filter). Permanent err+ retention is therefore
/// only guaranteed if the DB never breaches `max_db_size_mb` or
/// `min_free_disk_mb`. See CLAUDE.md "Retention" for the policy interaction.
pub fn purge_old_logs(pool: &DbPool, retention_days: u32, fts_merge_pages: u32) -> Result<usize> {
    if retention_days == 0 {
        return Ok(0);
    }

    let cutoff = Utc::now()
        .checked_sub_signed(chrono::TimeDelta::days(retention_days as i64))
        .ok_or_else(|| {
            anyhow::anyhow!("date arithmetic overflow for retention_days={retention_days}")
        })?
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    // Chunked DELETE: each iteration acquires a fresh connection from the pool
    // and releases it (along with its write lock) before sleeping, giving the
    // batch writer a window to acquire a connection between chunks.
    // Use received_at (server clock) instead of timestamp (device clock) so that
    // a device with a misconfigured clock cannot cause its logs to be purged
    // immediately (future timestamp) or retained forever (past timestamp).
    let mut total_deleted: usize = 0;
    loop {
        let conn = pool.get()?;
        let chunk = conn.execute(
            "DELETE FROM logs WHERE id IN (
                 SELECT id FROM logs
                 WHERE received_at < ?1
                   AND severity NOT IN ('err', 'crit', 'alert', 'emerg')
                 LIMIT 10000
             )",
            params![cutoff],
        )?;
        total_deleted += chunk;
        drop(conn); // release back to pool before sleeping
        if chunk == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // Incremental FTS merge — much shorter write-lock duration than full rebuild.
    if total_deleted > 0 {
        fts_incremental_merge(pool, total_deleted, fts_merge_pages);
    }

    // Passive WAL checkpoint: attempt to move WAL pages into the main DB file
    // without blocking writers. Prevents unbounded WAL growth between restarts.
    {
        let conn = pool.get()?;
        if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);") {
            tracing::warn!(error = %e, "WAL checkpoint skipped (non-fatal)");
        }
    }

    tracing::info!(deleted = total_deleted, cutoff = %cutoff, "Purged old logs");
    Ok(total_deleted)
}

/// Purge heartbeat samples older than N days.
///
/// Heartbeat tables do not rely on global SQLite foreign-key enforcement, so
/// child metric rows are deleted explicitly before their parent heartbeat rows.
/// Each chunk is its own short transaction to avoid starving log ingest.
pub fn purge_old_heartbeats(
    pool: &DbPool,
    retention_days: u32,
    chunk_size: usize,
) -> Result<usize> {
    if retention_days == 0 {
        return Ok(0);
    }

    let cutoff = Utc::now()
        .checked_sub_signed(chrono::TimeDelta::days(retention_days as i64))
        .ok_or_else(|| {
            anyhow::anyhow!("date arithmetic overflow for retention_days={retention_days}")
        })?
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let mut total_deleted = 0usize;
    let orphan_children = delete_orphan_heartbeat_children(pool)?;
    if orphan_children > 0 {
        tracing::warn!(
            deleted_rows = orphan_children,
            "Purged orphan heartbeat child rows"
        );
    }
    loop {
        let deleted = delete_heartbeat_chunk_before(pool, &cutoff, chunk_size)?;
        total_deleted += deleted;
        if deleted == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    tracing::info!(
        deleted = total_deleted,
        cutoff = %cutoff,
        "Purged old heartbeats"
    );
    Ok(total_deleted)
}

/// Delete rows for a single `app_name` older than `max_days`.
///
/// Same chunked-DELETE pattern as [`purge_old_logs`] (10 000 rows per
/// iteration with the WAL lock released between chunks). Rows with
/// `severity IN ('err','crit','alert','emerg')` are excluded — high-severity
/// log entries are protected from time-based purge regardless of source.
///
/// Designed for short-retention tags (e.g. `adguard-allowed` at 7 days)
/// running on the new composite index `(app_name, received_at)` introduced
/// by Migration 3. **MUST run before [`purge_old_logs`]** in the maintenance
/// task to avoid SQLite write-lock contention from concurrent chunked
/// DELETEs over the same table.
///
/// Uses [`fts_incremental_merge`] after the loop because FTS5 DELETE triggers
/// were intentionally dropped in Migration 1 — phantoms otherwise accumulate.
pub fn purge_by_tag_window(
    pool: &DbPool,
    app_name: &str,
    max_days: u32,
    fts_merge_pages: u32,
) -> Result<usize> {
    if max_days == 0 {
        return Ok(0);
    }

    let cutoff = Utc::now()
        .checked_sub_signed(chrono::TimeDelta::days(max_days as i64))
        .ok_or_else(|| anyhow::anyhow!("date arithmetic overflow for max_days={max_days}"))?
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let mut total_deleted: usize = 0;
    loop {
        let conn = pool.get()?;
        let chunk = conn.execute(
            "DELETE FROM logs WHERE id IN (
                 SELECT id FROM logs
                 WHERE app_name = ?1
                   AND received_at < ?2
                   AND severity NOT IN ('err', 'crit', 'alert', 'emerg')
                 LIMIT 10000
             )",
            params![app_name, cutoff],
        )?;
        total_deleted += chunk;
        drop(conn);
        if chunk == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    if total_deleted > 0 {
        fts_incremental_merge(pool, total_deleted, fts_merge_pages);
    }

    tracing::info!(
        app_name,
        max_days,
        deleted = total_deleted,
        cutoff = %cutoff,
        "Purged tag window"
    );
    Ok(total_deleted)
}

#[derive(Debug)]
struct DeletedChunk {
    deleted_rows: usize,
    hostnames: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelemetrySource {
    Logs,
    Heartbeats,
}

#[derive(Debug)]
struct DeletedTelemetryChunk {
    deleted_rows: usize,
    log_hostnames: Vec<String>,
    source: TelemetrySource,
}

fn oldest_telemetry_source(pool: &DbPool) -> Result<Option<TelemetrySource>> {
    let conn = pool.get()?;
    let oldest_log: Option<String> =
        conn.query_row("SELECT MIN(received_at) FROM logs", [], |row| row.get(0))?;
    let oldest_heartbeat: Option<String> =
        conn.query_row("SELECT MIN(received_at) FROM host_heartbeats", [], |row| {
            row.get(0)
        })?;

    Ok(match (oldest_log, oldest_heartbeat) {
        (Some(log), Some(heartbeat)) if heartbeat <= log => Some(TelemetrySource::Heartbeats),
        (Some(_), Some(_)) | (Some(_), None) => Some(TelemetrySource::Logs),
        (None, Some(_)) => Some(TelemetrySource::Heartbeats),
        (None, None) => None,
    })
}

fn delete_oldest_logs_chunk(pool: &DbPool, chunk_size: usize) -> Result<DeletedChunk> {
    let conn = pool.get()?;

    // Collect distinct hostnames from the chunk we're about to delete.
    // Use a subquery instead of a dynamic IN-list to avoid SQLite expression
    // depth limit (default 1000) at large chunk sizes.
    let hostnames: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT hostname FROM logs \
             WHERE id IN (SELECT id FROM logs ORDER BY received_at ASC, id ASC LIMIT ?1)",
        )?;
        let result = stmt
            .query_map([chunk_size as i64], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        result
    };

    // Pre-flight: count high-severity rows in the chunk so we can warn the
    // operator that disk-pressure cleanup is overriding the time-based
    // retention exemption for err+ logs.
    let high_severity_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM logs \
         WHERE id IN (SELECT id FROM logs ORDER BY received_at ASC, id ASC LIMIT ?1) \
           AND severity IN ('err', 'crit', 'alert', 'emerg')",
        [chunk_size as i64],
        |row| row.get(0),
    )?;
    if high_severity_count > 0 {
        tracing::warn!(
            high_severity_count,
            chunk_size,
            "Storage enforcement deleting high-severity rows — \
             disk pressure overrides time-based retention exemption"
        );
    }

    // Delete the oldest chunk using a subquery — O(1) SQL string size regardless
    // of chunk_size, no expression depth issues.
    let deleted_rows = conn.execute(
        "DELETE FROM logs \
         WHERE id IN (SELECT id FROM logs ORDER BY received_at ASC, id ASC LIMIT ?1)",
        [chunk_size as i64],
    )?;

    tracing::debug!(
        deleted_rows,
        affected_hosts = hostnames.len(),
        chunk_size,
        "Deleted oldest logs chunk"
    );

    Ok(DeletedChunk {
        deleted_rows,
        hostnames,
    })
}

fn delete_oldest_heartbeats_chunk(pool: &DbPool, chunk_size: usize) -> Result<usize> {
    delete_heartbeat_chunk_where(pool, "", &[], chunk_size)
}

fn delete_heartbeat_chunk_before(pool: &DbPool, cutoff: &str, chunk_size: usize) -> Result<usize> {
    delete_heartbeat_chunk_where(pool, "WHERE received_at < ?1", &[cutoff], chunk_size)
}

fn delete_heartbeat_chunk_where(
    pool: &DbPool,
    where_clause: &str,
    params: &[&str],
    chunk_size: usize,
) -> Result<usize> {
    let mut conn = pool.get()?;
    let tx = conn.transaction()?;
    tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS temp_heartbeat_delete_ids (
             id INTEGER PRIMARY KEY
         );
         DELETE FROM temp_heartbeat_delete_ids;",
    )?;

    let insert_sql = format!(
        "INSERT INTO temp_heartbeat_delete_ids (id)
         SELECT id FROM host_heartbeats
         {where_clause}
         ORDER BY received_at ASC, id ASC
         LIMIT ?{}",
        params.len() + 1
    );
    let mut values: Vec<&dyn rusqlite::ToSql> = params
        .iter()
        .map(|value| value as &dyn rusqlite::ToSql)
        .collect();
    let chunk_limit = chunk_size as i64;
    values.push(&chunk_limit);
    tx.execute(&insert_sql, rusqlite::params_from_iter(values))?;

    let selected: usize = tx.query_row(
        "SELECT COUNT(*) FROM temp_heartbeat_delete_ids",
        [],
        |row| row.get::<_, i64>(0),
    )? as usize;
    if selected == 0 {
        tx.execute_batch("DELETE FROM temp_heartbeat_delete_ids;")?;
        tx.commit()?;
        return Ok(0);
    }

    for table in HEARTBEAT_CHILD_TABLES {
        tx.execute(
            &format!(
                "DELETE FROM {table}
                 WHERE heartbeat_id IN (SELECT id FROM temp_heartbeat_delete_ids)"
            ),
            [],
        )?;
    }
    let deleted = tx.execute(
        "DELETE FROM host_heartbeats
         WHERE id IN (SELECT id FROM temp_heartbeat_delete_ids)",
        [],
    )?;
    tx.execute_batch("DELETE FROM temp_heartbeat_delete_ids;")?;
    tx.commit()?;

    tracing::debug!(
        deleted_rows = deleted,
        child_tables = HEARTBEAT_CHILD_TABLES.len(),
        chunk_size,
        "Deleted heartbeat chunk"
    );
    Ok(deleted)
}

fn delete_orphan_heartbeat_children(pool: &DbPool) -> Result<usize> {
    let conn = pool.get()?;
    let mut total_deleted = 0usize;
    for table in HEARTBEAT_CHILD_TABLES {
        let deleted = conn.execute(
            &format!(
                "DELETE FROM {table}
                 WHERE NOT EXISTS (
                     SELECT 1 FROM host_heartbeats
                     WHERE host_heartbeats.id = {table}.heartbeat_id
                 )"
            ),
            [],
        )?;
        total_deleted += deleted;
    }
    Ok(total_deleted)
}

const HEARTBEAT_CHILD_TABLES: &[&str] = &[
    "heartbeat_cpu",
    "heartbeat_memory",
    "heartbeat_disks",
    "heartbeat_network",
    "heartbeat_processes",
    "heartbeat_containers",
];

fn reconcile_hosts(pool: &DbPool, hostnames: &[String]) -> Result<()> {
    if hostnames.is_empty() {
        return Ok(());
    }

    let mut conn = pool.get()?;
    let tx = conn.transaction()?;
    for hostname in hostnames {
        // One query: count + timestamp bounds in a single pass over the index.
        // MIN/MAX return NULL when count=0, so timestamps are Option<String>.
        let (count, first_seen, last_seen): (i64, Option<String>, Option<String>) = tx.query_row(
            "SELECT COUNT(*), MIN(received_at), MAX(received_at)
                 FROM logs WHERE hostname = ?1",
            [hostname],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

        match (count, first_seen, last_seen) {
            (0, _, _) | (_, None, _) | (_, _, None) => {
                tx.execute("DELETE FROM hosts WHERE hostname = ?1", [hostname])?;
            }
            (count, Some(first_seen), Some(last_seen)) => {
                tx.execute(
                    "UPDATE hosts
                     SET first_seen = ?2, last_seen = ?3, log_count = ?4
                     WHERE hostname = ?1",
                    params![hostname, first_seen, last_seen, count],
                )?;
            }
        }
    }
    tx.commit()?;
    tracing::debug!(
        host_count = hostnames.len(),
        "Reconciled host aggregates after log deletion"
    );
    Ok(())
}

fn checkpoint_wal_and_incremental_vacuum(pool: &DbPool) -> Result<()> {
    let conn = pool.get()?;
    if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);") {
        tracing::warn!(error = %e, "WAL checkpoint skipped (non-fatal)");
    } else {
        tracing::debug!("WAL checkpoint completed");
    }
    if let Err(e) = conn.execute_batch("PRAGMA incremental_vacuum(1000);") {
        tracing::warn!(error = %e, "incremental vacuum skipped (non-fatal)");
    } else {
        tracing::debug!("Incremental vacuum completed");
    }
    Ok(())
}

fn storage_limits_enabled(config: &StorageConfig) -> bool {
    config.max_db_size_mb > 0 || config.min_free_disk_mb > 0
}

fn recovery_targets(config: &StorageConfig) -> StorageRecovery {
    StorageRecovery {
        logical_db_size_bytes: mb_to_bytes(config.recovery_db_size_mb),
        free_disk_bytes: (config.min_free_disk_mb > 0)
            .then(|| mb_to_bytes(config.recovery_free_disk_mb)),
    }
}

pub fn exceeds_trigger(metrics: &StorageMetrics, config: &StorageConfig) -> bool {
    (config.max_db_size_mb > 0
        && metrics.logical_db_size_bytes > mb_to_bytes(config.max_db_size_mb))
        || (config.min_free_disk_mb > 0
            && metrics.free_disk_bytes.unwrap_or(0) < mb_to_bytes(config.min_free_disk_mb))
}

fn within_recovery(
    metrics: &StorageMetrics,
    recovery: &StorageRecovery,
    config: &StorageConfig,
) -> bool {
    let db_ok = config.max_db_size_mb == 0
        || metrics.logical_db_size_bytes <= recovery.logical_db_size_bytes;
    let disk_ok = config.min_free_disk_mb == 0
        || metrics.free_disk_bytes.unwrap_or(0) >= recovery.free_disk_bytes.unwrap_or(0);
    db_ok && disk_ok
}

fn mb_to_bytes(mb: u64) -> u64 {
    mb.saturating_mul(1_048_576)
}

fn physical_db_size_bytes(db_path: &Path) -> Result<u64> {
    let mut total = file_size_if_exists(db_path)?;
    total += file_size_if_exists(&db_path.with_extension(format!(
        "{}-wal",
        db_path.extension().and_then(|ext| ext.to_str()).unwrap_or_default()
    )))?;
    total += file_size_if_exists(&db_path.with_extension(format!(
        "{}-shm",
        db_path.extension().and_then(|ext| ext.to_str()).unwrap_or_default()
    )))?;
    Ok(total)
}

fn file_size_if_exists(path: &Path) -> Result<u64> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
#[path = "maintenance_tests.rs"]
mod tests;
