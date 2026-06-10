//! Periodic log evaluator — scans recent logs and applies alert rules.
//!
//! Runs on a 5-minute cadence (configurable via NotificationsConfig).
//! Each cycle fetches logs from the last evaluator window and feeds them
//! to each enabled rule function.
//!
//! MUST NOT be imported from src/syslog/, src/ingest.rs, or src/syslog/writer.rs.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use tokio::sync::Semaphore;

// Phase 1 scans up to 50,000 rows in one blocking call — allow 10s before warning.
const SLOW_EVAL_SCAN_MS: u128 = 10_000;
// Phase 2 inserts are bounded by matched rules (typically single-digit rows).
const SLOW_DB_MS: u128 = 500;

use crate::config::NotificationsConfig;
use crate::db::DbPool;
use crate::notifications::rules::{
    evaluate_authelia_mfa_fail, evaluate_container_die_nonzero, evaluate_fail2ban_ban,
    evaluate_ingest_silence, evaluate_oom_kill, LogRow,
};

/// Run one evaluation cycle.
///
/// Phase 1 (no permit): fetch recent logs and evaluate rules in memory.
/// Phase 2 (permit held): insert matched rows into the notifications outbox.
///
/// Separating the two phases prevents the evaluator's DB read (fetching up to
/// 5000 rows) from blocking the maintenance semaphore across the entire cycle.
pub(crate) async fn run_evaluation_cycle(
    pool: Arc<DbPool>,
    permit_sem: Arc<Semaphore>,
    cfg: NotificationsConfig,
) -> Result<u64> {
    let apprise_urls_json = build_urls_json(&cfg);
    let window_secs = cfg.evaluators.evaluator_interval_secs * 2; // look back 2x interval

    // --- Phase 1: fetch + evaluate (NO permit needed — read-only DB access) ---
    // Paginate in batches of 1,000 rows up to a 50,000 row total cap to avoid
    // truncating high-volume cycles at 5,000.
    const BATCH_SIZE: u64 = 1_000;
    const MAX_ROWS: u64 = 50_000;
    let pool_r = Arc::clone(&pool);
    let exec_start = Instant::now();
    let phase1_result = tokio::task::spawn_blocking(
        move || -> Result<Vec<crate::db::notifications::OutboxInsertParams>> {
            let conn = pool_r.get()?;

            let mut out = Vec::new();
            let mut offset: u64 = 0;
            loop {
                let rows = fetch_recent_logs(&conn, window_secs, BATCH_SIZE, offset)?;
                let is_last = rows.len() < BATCH_SIZE as usize;

                if cfg.evaluators.oom_kill {
                    out.extend(evaluate_oom_kill(&rows, &apprise_urls_json));
                }
                if cfg.evaluators.container_die_nonzero {
                    out.extend(evaluate_container_die_nonzero(&rows, &apprise_urls_json));
                }
                if cfg.evaluators.fail2ban_ban {
                    out.extend(evaluate_fail2ban_ban(&rows, &apprise_urls_json));
                }
                if cfg.evaluators.authelia_mfa_fail {
                    out.extend(evaluate_authelia_mfa_fail(&rows, &apprise_urls_json));
                }

                offset += BATCH_SIZE;
                if is_last || offset >= MAX_ROWS {
                    break;
                }
            }

            // Metric rule: ingest silence. Unlike the log-scan rules above it
            // needs the age of the newest row across the whole table, not the
            // recent window (a silent ingest pipeline has no recent rows at
            // all). MAX(received_at) is an O(1) reverse index probe.
            if cfg.evaluators.ingest_silence {
                let newest_row_age_secs = newest_row_age_secs(&conn)?;
                let hostname =
                    std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string());
                out.extend(evaluate_ingest_silence(
                    &hostname,
                    newest_row_age_secs,
                    cfg.evaluators.ingest_silence_threshold_secs,
                    &apprise_urls_json,
                ));
            }
            drop(conn);
            Ok(out)
        },
    )
    .await;
    let exec_ms = exec_start.elapsed().as_millis();
    let phase1_inner = phase1_result.map_err(|e| anyhow::anyhow!("db task join error: {e}"))?;
    if exec_ms > SLOW_EVAL_SCAN_MS {
        match &phase1_inner {
            Ok(_) => tracing::warn!(op = "notif.eval_phase1_scan", exec_ms, "db op ok"),
            Err(e) => {
                tracing::warn!(op = "notif.eval_phase1_scan", exec_ms, error = %e, "db op err")
            }
        }
    } else {
        match &phase1_inner {
            Ok(_) => tracing::debug!(op = "notif.eval_phase1_scan", exec_ms, "db op ok"),
            Err(e) => {
                tracing::debug!(op = "notif.eval_phase1_scan", exec_ms, error = %e, "db op err")
            }
        }
    }
    let all_params = phase1_inner?;

    if all_params.is_empty() {
        return Ok(0);
    }

    // --- Phase 2: insert into outbox (permit held only during DB writes) ---
    let Ok(_permit) = Arc::clone(&permit_sem).acquire_owned().await else {
        tracing::error!("evaluator: maintenance semaphore closed, skipping inserts");
        return Ok(0);
    };

    let pool_w = Arc::clone(&pool);
    let exec_start = Instant::now();
    let phase2_result = tokio::task::spawn_blocking(move || -> Result<u64> {
        let _permit = _permit; // keep permit alive for the duration of the write block
        let conn = pool_w.get()?;
        let mut total = 0u64;
        for params in &all_params {
            match crate::db::notifications::outbox_insert(&conn, params) {
                Ok(()) => {
                    // INSERT OR IGNORE: only count actual inserts, not silent no-ops.
                    if conn.changes() > 0 {
                        total += 1;
                    }
                }
                Err(e) => tracing::warn!(
                    rule_id = %params.rule_id,
                    hostname = %params.hostname,
                    error = %e,
                    "evaluator: outbox_insert failed (non-fatal)"
                ),
            }
        }
        Ok(total)
    })
    .await;
    let exec_ms = exec_start.elapsed().as_millis();
    let phase2_inner = phase2_result.map_err(|e| anyhow::anyhow!("db task join error: {e}"))?;
    if exec_ms > SLOW_DB_MS {
        match &phase2_inner {
            Ok(_) => tracing::warn!(op = "notif.eval_phase2_insert", exec_ms, "db op ok"),
            Err(e) => {
                tracing::warn!(op = "notif.eval_phase2_insert", exec_ms, error = %e, "db op err")
            }
        }
    } else {
        match &phase2_inner {
            Ok(_) => tracing::debug!(op = "notif.eval_phase2_insert", exec_ms, "db op ok"),
            Err(e) => {
                tracing::debug!(op = "notif.eval_phase2_insert", exec_ms, error = %e, "db op err")
            }
        }
    }
    let count = phase2_inner?;

    Ok(count)
}

fn build_urls_json(cfg: &NotificationsConfig) -> String {
    serde_json::to_string(&cfg.apprise_urls).unwrap_or_else(|_| "[]".to_string())
}

/// Age in seconds of the newest row in `logs`, or `None` when the table is
/// empty. Served by a reverse probe of `idx_logs_received_at` — O(1).
fn newest_row_age_secs(conn: &rusqlite::Connection) -> rusqlite::Result<Option<u64>> {
    let age: Option<i64> = conn.query_row(
        "SELECT CAST(strftime('%s','now') AS INTEGER) -
                CAST(strftime('%s', MAX(received_at)) AS INTEGER)
         FROM logs",
        [],
        |row| row.get(0),
    )?;
    // Clock skew can make the newest row appear to be from the future;
    // clamp to 0 rather than reporting a huge unsigned wraparound.
    Ok(age.map(|a| a.max(0) as u64))
}

/// Fetch log rows from the last `window_secs` seconds for rule evaluation.
///
/// `limit` and `offset` enable pagination — callers should iterate until a
/// batch smaller than `limit` is returned (or a total row cap is reached).
fn fetch_recent_logs(
    conn: &rusqlite::Connection,
    window_secs: u64,
    limit: u64,
    offset: u64,
) -> rusqlite::Result<Vec<LogRow>> {
    let mut stmt = conn.prepare(
        "SELECT app_name, message, hostname, severity, metadata_json, timestamp
         FROM logs
         WHERE received_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', printf('-%d seconds', ?1))
         ORDER BY id DESC
         LIMIT ?2 OFFSET ?3",
    )?;
    let rows = stmt
        .query_map(
            rusqlite::params![window_secs as i64, limit as i64, offset as i64],
            |row| {
                Ok(LogRow {
                    app_name: row.get(0)?,
                    message: row.get(1)?,
                    hostname: row.get(2)?,
                    severity: row.get(3)?,
                    metadata_json: row.get(4)?,
                    timestamp: row.get(5)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Spawn the evaluator task. Returns None if notifications are disabled.
pub(crate) fn spawn_evaluator(
    pool: Arc<DbPool>,
    permit_sem: Arc<Semaphore>,
    cfg: NotificationsConfig,
) -> Option<tokio::task::JoinHandle<()>> {
    if !cfg.enabled {
        return None;
    }
    let interval_secs = cfg.evaluators.evaluator_interval_secs;
    let handle = tokio::spawn(async move {
        let mut interval =
            crate::runtime::background_interval(tokio::time::Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            tracing::debug!("notification_evaluator: cycle starting");
            match run_evaluation_cycle(Arc::clone(&pool), Arc::clone(&permit_sem), cfg.clone())
                .await
            {
                Ok(n) => tracing::info!(queued = n, "notification_evaluator: cycle complete"),
                Err(e) => tracing::error!(
                    error = %e,
                    "notification_evaluator: cycle failed"
                ),
            }
        }
    });
    Some(handle)
}
