//! Periodic log evaluator — scans recent logs and applies alert rules.
//!
//! Runs on a 5-minute cadence (configurable via NotificationsConfig).
//! Each cycle fetches logs from the last evaluator window and feeds them
//! to each enabled rule function.
//!
//! MUST NOT be imported from src/syslog/, src/ingest.rs, or src/syslog/writer.rs.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Semaphore;

use crate::config::NotificationsConfig;
use crate::db::DbPool;
use crate::notifications::rules::{
    evaluate_authelia_mfa_fail, evaluate_container_die_nonzero, evaluate_fail2ban_ban,
    evaluate_oom_kill, LogRow,
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
    let pool_r = Arc::clone(&pool);
    let all_params = tokio::task::spawn_blocking(
        move || -> Result<Vec<crate::db::notifications::OutboxInsertParams>> {
            let conn = pool_r.get()?;
            let rows = fetch_recent_logs(&conn, window_secs)?;
            drop(conn);

            let mut out = Vec::new();
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
            Ok(out)
        },
    )
    .await??;

    if all_params.is_empty() {
        return Ok(0);
    }

    // --- Phase 2: insert into outbox (permit held only during DB writes) ---
    let Ok(_permit) = Arc::clone(&permit_sem).acquire_owned().await else {
        tracing::error!("evaluator: maintenance semaphore closed, skipping inserts");
        return Ok(0);
    };

    let pool_w = Arc::clone(&pool);
    let count = tokio::task::spawn_blocking(move || -> Result<u64> {
        let _permit = _permit; // keep permit alive for the duration of the write block
        let conn = pool_w.get()?;
        let mut total = 0u64;
        for params in &all_params {
            match crate::db::notifications::outbox_insert(&conn, params) {
                Ok(()) => total += 1,
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
    .await??;

    Ok(count)
}

fn build_urls_json(cfg: &NotificationsConfig) -> String {
    serde_json::to_string(&cfg.apprise_urls).unwrap_or_else(|_| "[]".to_string())
}

/// Fetch log rows from the last `window_secs` seconds for rule evaluation.
fn fetch_recent_logs(
    conn: &rusqlite::Connection,
    window_secs: u64,
) -> rusqlite::Result<Vec<LogRow>> {
    let mut stmt = conn.prepare(
        "SELECT app_name, message, hostname, severity, metadata_json, timestamp
         FROM logs
         WHERE received_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', printf('-%d seconds', ?1))
         ORDER BY id DESC
         LIMIT 5000",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![window_secs as i64], |row| {
            Ok(LogRow {
                app_name: row.get(0)?,
                message: row.get(1)?,
                hostname: row.get(2)?,
                severity: row.get(3)?,
                metadata_json: row.get(4)?,
                timestamp: row.get(5)?,
            })
        })?
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
