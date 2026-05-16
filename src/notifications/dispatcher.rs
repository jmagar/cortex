//! Notification dispatcher — drains the outbox and delivers via Apprise.
//!
//! Runs on a 30-second cadence (configurable).
//! For each pending row:
//!   1. Check dedup against notification_firings
//!   2. Check ack against error_signatures (for error_sig rules)
//!   3. POST to Apprise (5s timeout)
//!   4. Update outbox status
//!   5. Insert into notification_firings
//!
//! Backoff: 1s → 5s → 30s → 5min → 30min cap.
//! Dead-letter after 8 attempts.
//! 207 = partial success, mark sent, do NOT retry.
//! 424 = delivery failed, treat as permanent error (dead-letter).
//!
//! Security: NEVER log Apprise URLs.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Semaphore;

use crate::config::NotificationsConfig;
use crate::db::notifications::{
    backoff_next_attempt_at, firings_insert, firings_recent_dedup_check, outbox_claim_pending,
    outbox_mark_dead, outbox_mark_dropped, outbox_mark_sent, outbox_schedule_retry,
    FiringInsertParams,
};
use crate::db::DbPool;
use crate::notifications::apprise::{AppriseClient, AppriseError, NotifyType};

const CLAIM_LIMIT: i64 = 50;

/// Run one dispatcher cycle.
pub(crate) async fn run_dispatch_cycle(
    pool: Arc<DbPool>,
    permit_sem: Arc<Semaphore>,
    apprise: &AppriseClient,
    cfg: &NotificationsConfig,
) -> Result<u64> {
    // Claim pending rows (read-only, no permit needed)
    let rows = {
        let pool_r = Arc::clone(&pool);
        tokio::task::spawn_blocking(move || {
            let conn = pool_r.get()?;
            outbox_claim_pending(&conn, CLAIM_LIMIT).map_err(anyhow::Error::from)
        })
        .await??
    };

    let mut dispatched = 0u64;

    for row in rows {
        // ----------------------------------------------------------------
        // Phase 1: acquire permit → run dedup + ack checks → drop permit.
        // The permit must NOT be held across the HTTP call (5s timeout).
        // ----------------------------------------------------------------
        let Ok(permit) = Arc::clone(&permit_sem).acquire_owned().await else {
            tracing::error!("dispatcher: maintenance semaphore closed, aborting");
            break;
        };

        // --- Dedup check (within permit) ---
        let is_dedup = {
            let pool_d = Arc::clone(&pool);
            let rule_id = row.rule_id.clone();
            let hostname = row.hostname.clone();
            let dedup_key = row.dedup_key.clone();
            let dedup_secs = cfg.dedup_window_secs;
            tokio::task::spawn_blocking(move || -> Result<bool> {
                let conn = pool_d.get()?;
                firings_recent_dedup_check(&conn, &rule_id, &hostname, &dedup_key, dedup_secs)
                    .map_err(anyhow::Error::from)
            })
            .await??
        };

        if is_dedup {
            let pool_dd = Arc::clone(&pool);
            let row_id = row.id;
            tokio::task::spawn_blocking(move || -> Result<()> {
                let conn = pool_dd.get()?;
                outbox_mark_dropped(&conn, row_id, "dedup_suppressed")?;
                Ok(())
            })
            .await??;
            drop(permit);
            tracing::debug!(
                rule_id = %row.rule_id,
                hostname = %row.hostname,
                "dispatcher: suppressed (dedup)"
            );
            continue;
        }

        // --- Check ack for error signature rules ---
        if row.rule_id == "unaddressed_error_signature" {
            // dedup_key = "error_sig:{hash}"
            if let Some(hash) = row.dedup_key.strip_prefix("error_sig:") {
                use rusqlite::OptionalExtension;
                let hash_owned = hash.to_string();
                let normalizer_version = crate::app::error_detection::NORMALIZER_VERSION;
                let pool_ack = Arc::clone(&pool);
                let is_acked = tokio::task::spawn_blocking(move || -> Result<bool> {
                    let conn = pool_ack.get()?;
                    // .optional() converts "no rows" into None (as opposed to
                    // Err(QueryReturnedNoRows)). The row's acknowledged_at column
                    // is itself nullable, so we get Option<Option<String>>:
                    // - None          → signature not found → not acked
                    // - Some(None)    → found but acknowledged_at IS NULL → not acked
                    // - Some(Some(_)) → found and acknowledged_at is set → acked
                    let acked: Option<Option<String>> = conn
                        .query_row(
                            "SELECT acknowledged_at FROM error_signatures
                             WHERE signature_hash = ?1 AND normalizer_version = ?2 LIMIT 1",
                            rusqlite::params![hash_owned, normalizer_version],
                            |row| row.get(0),
                        )
                        .optional()?;
                    Ok(matches!(acked, Some(Some(_))))
                })
                .await??;

                if is_acked {
                    let pool_dd = Arc::clone(&pool);
                    let row_id = row.id;
                    tokio::task::spawn_blocking(move || -> Result<()> {
                        let conn = pool_dd.get()?;
                        outbox_mark_dropped(&conn, row_id, "error_signature_acked")?;
                        Ok(())
                    })
                    .await??;
                    drop(permit);
                    tracing::debug!(
                        rule_id = %row.rule_id,
                        hostname = %row.hostname,
                        "dispatcher: suppressed (error signature acked)"
                    );
                    continue;
                }
            }
        }

        // Phase 1 complete — drop permit before the HTTP call.
        drop(permit);

        // ----------------------------------------------------------------
        // Phase 2: Deliver via Apprise — NO permit held during HTTP call.
        // ----------------------------------------------------------------

        // Parse URLs from JSON
        let urls: Vec<String> = serde_json::from_str(&row.apprise_urls_json).unwrap_or_default();
        // Override with config URLs if outbox has empty list (e.g. from error scanner)
        let effective_urls = if urls.is_empty() {
            cfg.apprise_urls.clone()
        } else {
            urls
        };

        if effective_urls.is_empty() {
            tracing::warn!(
                rule_id = %row.rule_id,
                "dispatcher: no apprise URLs configured, dropping notification"
            );
            // Phase 3: re-acquire permit for DB write-back.
            let Ok(permit3) = Arc::clone(&permit_sem).acquire_owned().await else {
                tracing::error!("dispatcher: maintenance semaphore closed, aborting");
                break;
            };
            let pool_dd = Arc::clone(&pool);
            let row_id = row.id;
            tokio::task::spawn_blocking(move || -> Result<()> {
                let conn = pool_dd.get()?;
                outbox_mark_dropped(&conn, row_id, "no_apprise_urls")?;
                Ok(())
            })
            .await??;
            drop(permit3);
            continue;
        }

        let notify_type = severity_to_notify_type(&row.severity);
        let delivery_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            apprise.notify(&effective_urls, &row.title, &row.body, notify_type),
        )
        .await;

        let row_id = row.id;
        let attempt_count = u8::try_from(row.attempt_count).unwrap_or(u8::MAX);
        let rule_id = row.rule_id.clone();
        let severity = row.severity.clone();
        let hostname = row.hostname.clone();
        let dedup_key = row.dedup_key.clone();

        // ----------------------------------------------------------------
        // Phase 3: re-acquire permit → write outbox status + firing → drop.
        // ----------------------------------------------------------------
        let Ok(permit3) = Arc::clone(&permit_sem).acquire_owned().await else {
            tracing::error!("dispatcher: maintenance semaphore closed, aborting");
            break;
        };

        match delivery_result {
            Ok(Ok(resp)) => {
                // Success (200/207)
                let pool_s = Arc::clone(&pool);
                let sc = resp.status_code as i64;
                let (rid, sev, host, dk) = (
                    rule_id.clone(),
                    severity.clone(),
                    hostname.clone(),
                    dedup_key.clone(),
                );
                tokio::task::spawn_blocking(move || -> Result<()> {
                    let mut conn = pool_s.get()?;
                    let tx = conn.transaction()?;
                    outbox_mark_sent(&tx, row_id, Some(sc))?;
                    firings_insert(
                        &tx,
                        FiringInsertParams {
                            outbox_id: row_id,
                            rule_id: &rid,
                            severity: &sev,
                            hostname: &host,
                            status_code: Some(sc),
                            notes: None,
                            dedup_key: &dk,
                        },
                    )?;
                    tx.commit()?;
                    Ok(())
                })
                .await??;
                drop(permit3);
                dispatched += 1;
                tracing::info!(
                    rule_id = %rule_id,
                    hostname = %hostname,
                    status_code = resp.status_code,
                    "dispatcher: notification sent"
                );
            }
            Ok(Err(AppriseError::Permanent { code, .. })) => {
                // 4xx — dead-letter immediately
                let pool_dl = Arc::clone(&pool);
                let error_msg = format!("permanent HTTP {code}");
                let (rid, sev, host, dk) = (
                    rule_id.clone(),
                    severity.clone(),
                    hostname.clone(),
                    dedup_key.clone(),
                );
                tokio::task::spawn_blocking(move || -> Result<()> {
                    let mut conn = pool_dl.get()?;
                    let tx = conn.transaction()?;
                    outbox_mark_dead(&tx, row_id, Some(code as i64), &error_msg)?;
                    firings_insert(
                        &tx,
                        FiringInsertParams {
                            outbox_id: row_id,
                            rule_id: &rid,
                            severity: &sev,
                            hostname: &host,
                            status_code: Some(code as i64),
                            notes: Some(&error_msg),
                            dedup_key: &dk,
                        },
                    )?;
                    tx.commit()?;
                    Ok(())
                })
                .await??;
                drop(permit3);
                tracing::warn!(
                    rule_id = %rule_id,
                    hostname = %hostname,
                    status_code = code,
                    "dispatcher: permanent failure, dead-lettered"
                );
            }
            transient_or_timeout => {
                // Extract a human-readable error string
                let error_msg = match &transient_or_timeout {
                    Ok(Err(e)) => format!("{e}"),
                    Err(_) => "timeout".to_string(),
                    Ok(Ok(_)) => unreachable!("handled above"),
                };

                if attempt_count + 1 >= cfg.max_retry_attempts {
                    // Exhausted retries — dead-letter
                    let dead_msg = format!("max retries: {error_msg}");
                    let pool_dl = Arc::clone(&pool);
                    let (rid, sev, host, dk) = (
                        rule_id.clone(),
                        severity.clone(),
                        hostname.clone(),
                        dedup_key.clone(),
                    );
                    tokio::task::spawn_blocking(move || -> Result<()> {
                        let mut conn = pool_dl.get()?;
                        let tx = conn.transaction()?;
                        outbox_mark_dead(&tx, row_id, None, &dead_msg)?;
                        firings_insert(
                            &tx,
                            FiringInsertParams {
                                outbox_id: row_id,
                                rule_id: &rid,
                                severity: &sev,
                                hostname: &host,
                                status_code: None,
                                notes: Some(&dead_msg),
                                dedup_key: &dk,
                            },
                        )?;
                        tx.commit()?;
                        Ok(())
                    })
                    .await??;
                    drop(permit3);
                    tracing::warn!(
                        rule_id = %rule_id,
                        hostname = %hostname,
                        attempts = attempt_count + 1,
                        "dispatcher: dead-lettered after max retries"
                    );
                } else {
                    // Transient — schedule retry with backoff (no firing inserted).
                    // Use attempt_count (pre-increment) so the first retry uses the
                    // 1s tier, not 5s.
                    let next_at = backoff_next_attempt_at(attempt_count);
                    let next_at_log = next_at.clone();
                    let pool_r = Arc::clone(&pool);
                    let err_clone = error_msg.clone();
                    tokio::task::spawn_blocking(move || -> Result<()> {
                        let conn = pool_r.get()?;
                        outbox_schedule_retry(&conn, row_id, &next_at, &err_clone, None)?;
                        Ok(())
                    })
                    .await??;
                    drop(permit3);
                    tracing::debug!(
                        rule_id = %rule_id,
                        hostname = %hostname,
                        attempt = attempt_count + 1,
                        next_at = %next_at_log,
                        "dispatcher: scheduled retry"
                    );
                }
            }
        }
    }

    Ok(dispatched)
}

fn severity_to_notify_type(severity: &str) -> NotifyType {
    match severity {
        "emerg" | "alert" | "crit" | "critical" => NotifyType::Failure,
        "err" | "error" | "warning" | "warn" => NotifyType::Warning,
        _ => NotifyType::Info,
    }
}

/// Spawn the dispatcher task.
pub(crate) fn spawn_dispatcher(
    pool: Arc<DbPool>,
    permit_sem: Arc<Semaphore>,
    cfg: NotificationsConfig,
) -> Option<tokio::task::JoinHandle<()>> {
    if !cfg.enabled {
        return None;
    }
    let apprise = AppriseClient::new(&cfg.apprise_url);
    let interval_secs = cfg.dispatcher_interval_secs;
    let handle = tokio::spawn(async move {
        let mut interval =
            crate::runtime::background_interval(tokio::time::Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            tracing::debug!("notification_dispatcher: cycle starting");
            match run_dispatch_cycle(Arc::clone(&pool), Arc::clone(&permit_sem), &apprise, &cfg)
                .await
            {
                Ok(n) => tracing::debug!(dispatched = n, "notification_dispatcher: cycle complete"),
                Err(e) => {
                    tracing::error!(error = %e, "notification_dispatcher: cycle failed")
                }
            }
        }
    });
    Some(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::notifications::outbox_insert;

    fn open_test_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
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
             );
             CREATE TABLE error_signatures (
                 signature_hash TEXT NOT NULL,
                 normalizer_version INTEGER NOT NULL,
                 template TEXT NOT NULL,
                 sample_message TEXT NOT NULL,
                 sample_hostname TEXT NOT NULL,
                 sample_app_name TEXT,
                 severity TEXT NOT NULL,
                 first_seen_at TEXT NOT NULL,
                 last_seen_at TEXT NOT NULL,
                 total_count INTEGER NOT NULL DEFAULT 0,
                 acknowledged_at TEXT,
                 acknowledged_by TEXT,
                 PRIMARY KEY (signature_hash, normalizer_version)
             );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn backoff_schedule_is_increasing() {
        let delays: Vec<_> = (0u8..5)
            .map(|i| {
                let s = backoff_next_attempt_at(i);
                chrono::DateTime::parse_from_rfc3339(&s).unwrap()
            })
            .collect();
        for window in delays.windows(2) {
            assert!(
                window[1] >= window[0],
                "backoff should be non-decreasing: {:?} >= {:?}",
                window[1],
                window[0]
            );
        }
    }

    #[test]
    fn severity_to_notify_type_mapping() {
        assert_eq!(severity_to_notify_type("crit"), NotifyType::Failure);
        assert_eq!(severity_to_notify_type("critical"), NotifyType::Failure);
        assert_eq!(severity_to_notify_type("warning"), NotifyType::Warning);
        assert_eq!(severity_to_notify_type("err"), NotifyType::Warning);
        assert_eq!(severity_to_notify_type("notice"), NotifyType::Info);
        assert_eq!(severity_to_notify_type("info"), NotifyType::Info);
    }

    #[test]
    fn outbox_row_dedup_suppressed() {
        let conn = open_test_db();
        // Insert a firing first (simulating a previous delivery)
        let params = crate::db::notifications::OutboxInsertParams {
            dedup_key: "oom_kill:host1:ts".to_string(),
            rule_id: "oom_kill".to_string(),
            severity: "critical".to_string(),
            hostname: "host1".to_string(),
            title: "OOM".to_string(),
            body: "body".to_string(),
            apprise_urls_json: "[]".to_string(),
            next_attempt_at: "2000-01-01T00:00:00.000Z".to_string(),
        };
        outbox_insert(&conn, &params).unwrap();
        let id: i64 = conn
            .query_row("SELECT id FROM notifications_outbox LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        // Insert a firing within the dedup window
        firings_insert(
            &conn,
            FiringInsertParams {
                outbox_id: id,
                rule_id: "oom_kill",
                severity: "critical",
                hostname: "host1",
                status_code: Some(200),
                notes: None,
                dedup_key: "oom_kill:host1:ts",
            },
        )
        .unwrap();

        let is_dedup =
            firings_recent_dedup_check(&conn, "oom_kill", "host1", "oom_kill:host1:ts", 3600)
                .unwrap();
        assert!(
            is_dedup,
            "should detect existing firing within dedup window"
        );
    }

    #[test]
    fn dead_letter_after_max_retries() {
        let conn = open_test_db();
        let params = crate::db::notifications::OutboxInsertParams {
            dedup_key: "test:dl".to_string(),
            rule_id: "oom_kill".to_string(),
            severity: "critical".to_string(),
            hostname: "host1".to_string(),
            title: "Title".to_string(),
            body: "Body".to_string(),
            apprise_urls_json: "[]".to_string(),
            next_attempt_at: "2000-01-01T00:00:00.000Z".to_string(),
        };
        outbox_insert(&conn, &params).unwrap();
        let id: i64 = conn
            .query_row("SELECT id FROM notifications_outbox LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();

        // Simulate 8 failed retries
        let max_retries: u8 = 8;
        for attempt in 0u8..max_retries {
            let next_at = backoff_next_attempt_at(attempt);
            outbox_schedule_retry(&conn, id, &next_at, "timeout", None).unwrap();
        }

        let attempt_count: i64 = conn
            .query_row(
                "SELECT attempt_count FROM notifications_outbox WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap();

        assert!(
            attempt_count >= max_retries as i64,
            "attempt_count={attempt_count} should be >= max_retries={max_retries}"
        );

        // Mark dead
        outbox_mark_dead(&conn, id, None, "exhausted").unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM notifications_outbox WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "dead");
    }
}
