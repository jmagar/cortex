//! Daily digest builder.
//!
//! Queries the last 24h of logs to produce a per-host summary, then inserts
//! a single outbox row with rule_id='daily_digest'.
//!
//! Scheduling: checks every 60s whether the configured digest hour/minute has
//! been reached and hasn't fired today. No cron crate needed.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Semaphore;

use crate::config::NotificationsConfig;
use crate::db::notifications::{backoff_next_attempt_at, outbox_insert, OutboxInsertParams};
use crate::db::DbPool;
use crate::notifications::apprise::escape_for_notification;

/// Summary for one host in the digest.
#[derive(Debug, Clone)]
pub struct HostDigestEntry {
    pub hostname: String,
    pub total_logs: i64,
    pub error_count: i64,
    pub warning_count: i64,
    pub top_app: Option<String>,
}

/// Build a markdown digest body from per-host entries.
pub fn build_digest_body(entries: &[HostDigestEntry], window_hours: u32) -> String {
    if entries.is_empty() {
        return format!("No log activity in the last {window_hours}h.");
    }

    let mut lines = vec![format!("## Daily Digest — last {window_hours}h\n")];
    lines.push("| Host | Total | Errors | Warnings | Top App |".to_string());
    lines.push("|------|-------|--------|----------|---------|".to_string());

    for e in entries {
        let app = e.top_app.as_deref().unwrap_or("—");
        lines.push(format!(
            "| {} | {} | {} | {} | {} |",
            escape_for_notification(&e.hostname),
            e.total_logs,
            e.error_count,
            e.warning_count,
            escape_for_notification(app),
        ));
    }

    let total_errors: i64 = entries.iter().map(|e| e.error_count).sum();
    let total_warnings: i64 = entries.iter().map(|e| e.warning_count).sum();
    lines.push(String::new());
    lines.push(format!(
        "**{} hosts** — {} errors, {} warnings",
        entries.len(),
        total_errors,
        total_warnings
    ));

    lines.join("\n")
}

/// Fetch per-host stats for the last `window_hours` hours.
///
/// Uses a single query with a window function to find the top app per host,
/// avoiding the previous N+1 query pattern (one per-host subquery per host).
/// Requires SQLite 3.25+ (window function support).
pub fn fetch_host_stats(
    conn: &rusqlite::Connection,
    window_hours: u32,
) -> rusqlite::Result<Vec<HostDigestEntry>> {
    let window_secs = window_hours as i64 * 3600;

    // Fetch per-host totals + top_app in two queries (no N+1).
    // Query 1: per-host aggregate stats.
    let mut stmt = conn.prepare(
        "SELECT
             hostname,
             COUNT(*) AS total,
             SUM(CASE WHEN severity IN ('err','crit','alert','emerg') THEN 1 ELSE 0 END) AS errors,
             SUM(CASE WHEN severity = 'warning' THEN 1 ELSE 0 END) AS warnings
         FROM logs
         WHERE received_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', printf('-%d seconds', ?1))
         GROUP BY hostname
         ORDER BY total DESC
         LIMIT 50",
    )?;
    let mut entries: Vec<HostDigestEntry> = stmt
        .query_map(rusqlite::params![window_secs], |row| {
            Ok(HostDigestEntry {
                hostname: row.get(0)?,
                total_logs: row.get(1)?,
                error_count: row.get(2)?,
                warning_count: row.get(3)?,
                top_app: None, // populated below
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Query 2: top app per host using a window function (SQLite 3.25+).
    // Returns one row per hostname with the highest-count app_name.
    let mut top_app_stmt = conn.prepare(
        "SELECT hostname, app_name
         FROM (
             SELECT hostname, app_name,
                    ROW_NUMBER() OVER (PARTITION BY hostname ORDER BY COUNT(*) DESC) AS rn
             FROM logs
             WHERE received_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', printf('-%d seconds', ?1))
               AND app_name IS NOT NULL
             GROUP BY hostname, app_name
         )
         WHERE rn = 1",
    )?;
    // Collect into a HashMap for O(1) lookup when merging.
    let top_apps: std::collections::HashMap<String, String> = top_app_stmt
        .query_map(rusqlite::params![window_secs], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()?;

    for entry in &mut entries {
        entry.top_app = top_apps.get(&entry.hostname).cloned();
    }
    Ok(entries)
}

/// Build and enqueue a daily digest notification.
pub(crate) async fn run_digest(
    pool: Arc<DbPool>,
    permit_sem: Arc<Semaphore>,
    cfg: &NotificationsConfig,
) -> Result<()> {
    let Ok(_permit) = Arc::clone(&permit_sem).acquire_owned().await else {
        tracing::error!("digest: maintenance semaphore closed, skipping");
        return Ok(());
    };

    let apprise_urls_json =
        serde_json::to_string(&cfg.apprise_urls).unwrap_or_else(|_| "[]".to_string());
    let pool_d = Arc::clone(&pool);

    tokio::task::spawn_blocking(move || -> Result<()> {
        let conn = pool_d.get()?;
        let entries = fetch_host_stats(&conn, 24).map_err(anyhow::Error::from)?;
        let body = build_digest_body(&entries, 24);
        let title = format!(
            "Daily Digest — {} hosts, {} total logs",
            entries.len(),
            entries.iter().map(|e| e.total_logs).sum::<i64>()
        );

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let params = OutboxInsertParams {
            dedup_key: format!("daily_digest:{today}"),
            rule_id: "daily_digest".to_string(),
            severity: "info".to_string(),
            hostname: "all".to_string(),
            title,
            body,
            apprise_urls_json,
            next_attempt_at: backoff_next_attempt_at(0),
        };
        outbox_insert(&conn, &params).map_err(anyhow::Error::from)?;
        tracing::info!(date = %today, "digest: daily digest queued");
        Ok(())
    })
    .await??;

    Ok(())
}

/// Parse hour and minute from a cron string (fields 0 and 1).
/// Returns `(hour, minute)` or defaults `(8, 0)` on parse failure.
///
/// Only the first two fields of a 5-field cron expression are used.
fn parse_cron_hour_minute(cron: &str) -> (u32, u32) {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() < 2 {
        tracing::warn!(input = %cron, "Failed to parse digest_cron_local as cron expression; defaulting to 08:00");
        return (8, 0);
    }
    let minute_res = parts[0].parse::<u32>();
    let hour_res = parts[1].parse::<u32>();
    if minute_res.is_err() || hour_res.is_err() {
        tracing::warn!(input = %cron, "Failed to parse digest_cron_local as cron expression; defaulting to 08:00");
    }
    let minute = minute_res.unwrap_or(0).min(59);
    let hour = hour_res.unwrap_or(8).min(23);
    (hour, minute)
}

/// Spawn the digest task.
///
/// Wakes every 60s and fires when the local clock matches the cron
/// hour:minute, at most once per calendar day.
///
/// TODO: Quiet hours not implemented. Add a quiet_hours table when
/// chrono-tz is available.
pub(crate) fn spawn_digest(
    pool: Arc<DbPool>,
    permit_sem: Arc<Semaphore>,
    cfg: NotificationsConfig,
) -> Option<tokio::task::JoinHandle<()>> {
    if !cfg.enabled {
        return None;
    }
    let (target_hour, target_minute) = parse_cron_hour_minute(&cfg.digest_cron_local);
    let handle = tokio::spawn(async move {
        let mut last_fired_date: Option<chrono::NaiveDate> = None;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            let now = chrono::Local::now();
            let today = now.date_naive();
            let already_fired = last_fired_date == Some(today);
            if !already_fired && now.hour() == target_hour && now.minute() == target_minute {
                match run_digest(Arc::clone(&pool), Arc::clone(&permit_sem), &cfg).await {
                    Ok(()) => last_fired_date = Some(today),
                    Err(e) => tracing::error!(error = %e, "digest: failed to build/queue digest"),
                }
            }
        }
    });
    Some(handle)
}

use chrono::Timelike;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entries() -> Vec<HostDigestEntry> {
        vec![
            HostDigestEntry {
                hostname: "server1".to_string(),
                total_logs: 1000,
                error_count: 5,
                warning_count: 20,
                top_app: Some("nginx".to_string()),
            },
            HostDigestEntry {
                hostname: "server2".to_string(),
                total_logs: 500,
                error_count: 0,
                warning_count: 3,
                top_app: None,
            },
        ]
    }

    #[test]
    fn build_digest_body_golden() {
        let entries = make_entries();
        let body = build_digest_body(&entries, 24);

        assert!(
            body.contains("Daily Digest — last 24h"),
            "should have header"
        );
        assert!(body.contains("server1"), "should contain host1");
        assert!(body.contains("server2"), "should contain host2");
        assert!(body.contains("nginx"), "should contain top app");
        assert!(body.contains("1000"), "should contain log count");
        assert!(body.contains("2 hosts"), "should have host count summary");
        assert!(body.contains("5 errors"), "should have error count");
    }

    #[test]
    fn build_digest_body_empty() {
        let body = build_digest_body(&[], 24);
        assert!(
            body.contains("No log activity"),
            "empty input should say so"
        );
    }

    #[test]
    fn build_digest_body_escapes_hostnames() {
        let entries = vec![HostDigestEntry {
            hostname: "admin@server".to_string(),
            total_logs: 10,
            error_count: 0,
            warning_count: 0,
            top_app: None,
        }];
        let body = build_digest_body(&entries, 24);
        assert!(!body.contains('@'), "@ should be escaped in hostname");
        assert!(body.contains('＠'), "should contain fullwidth @");
    }

    #[test]
    fn parse_cron_hour_minute_standard() {
        assert_eq!(parse_cron_hour_minute("0 8 * * *"), (8, 0));
        assert_eq!(parse_cron_hour_minute("30 7 * * *"), (7, 30));
    }

    #[test]
    fn parse_cron_hour_minute_defaults_on_bad_input() {
        assert_eq!(parse_cron_hour_minute(""), (8, 0));
        assert_eq!(parse_cron_hour_minute("bad input"), (8, 0));
    }
}
