//! Hardcoded alert rules for push notifications.
//!
//! Each function takes a slice of log rows and returns `OutboxInsertParams`
//! for rows that match the rule. The evaluator calls these functions periodically.

use crate::db::notifications::{backoff_next_attempt_at, OutboxInsertParams};
use crate::notifications::apprise::escape_for_notification;

/// A minimal log row used for rule evaluation.
#[derive(Debug, Clone)]
pub struct LogRow {
    pub app_name: Option<String>,
    pub message: String,
    pub hostname: String,
    #[allow(dead_code)]
    pub severity: String,
    pub metadata_json: Option<String>,
    pub timestamp: String,
}

/// Evaluate OOM kill events.
///
/// Matches: `app_name = 'kernel'` AND message contains `'Out of memory: Killed process'`.
pub fn evaluate_oom_kill(rows: &[LogRow], apprise_urls_json: &str) -> Vec<OutboxInsertParams> {
    rows.iter()
        .filter(|r| {
            r.app_name.as_deref() == Some("kernel")
                && r.message.contains("Out of memory: Killed process")
        })
        .map(|r| {
            let title = escape_for_notification(&format!("[CRITICAL] OOM Kill on {}", r.hostname));
            let body = escape_for_notification(&format!(
                "Kernel OOM killer fired on **{}** at {}\n\n```\n{}\n```",
                r.hostname, r.timestamp, r.message
            ));
            OutboxInsertParams {
                dedup_key: format!("oom_kill:{}", r.hostname),
                rule_id: "oom_kill".to_string(),
                severity: "critical".to_string(),
                hostname: r.hostname.clone(),
                title,
                body,
                apprise_urls_json: apprise_urls_json.to_string(),
                next_attempt_at: backoff_next_attempt_at(0),
            }
        })
        .collect()
}

/// Evaluate container die events with non-zero exit code.
///
/// Matches: `metadata_json` has `action=die` AND `exit_code != "0"`.
pub fn evaluate_container_die_nonzero(
    rows: &[LogRow],
    apprise_urls_json: &str,
) -> Vec<OutboxInsertParams> {
    rows.iter()
        .filter_map(|r| {
            let meta = r.metadata_json.as_deref()?;
            let v = serde_json::from_str::<serde_json::Value>(meta).ok()?;
            if v.get("action").and_then(|a| a.as_str()) != Some("die") {
                return None;
            }
            // Handle both string ("1") and numeric (1) exit_code values.
            let exit_code_val: Option<i64> = match v.get("exit_code") {
                Some(serde_json::Value::String(s)) => s.parse::<i64>().ok(),
                Some(serde_json::Value::Number(n)) => n.as_i64(),
                _ => None,
            };
            let is_nonzero = exit_code_val.map(|c| c != 0).unwrap_or(false);
            if !is_nonzero {
                return None;
            }
            let exit_code_str = exit_code_val
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let container = v
                .get("container_name")
                .and_then(|c| c.as_str())
                .unwrap_or("unknown");

            let title = escape_for_notification(&format!(
                "[WARNING] Container {} died (exit {}) on {}",
                container, exit_code_str, r.hostname
            ));
            let body = escape_for_notification(&format!(
                "Container **{}** exited with code `{}` on **{}** at {}",
                container, exit_code_str, r.hostname, r.timestamp
            ));
            Some(OutboxInsertParams {
                dedup_key: format!("container_die:{}:{}", r.hostname, container),
                rule_id: "container_die_nonzero".to_string(),
                severity: "warning".to_string(),
                hostname: r.hostname.clone(),
                title,
                body,
                apprise_urls_json: apprise_urls_json.to_string(),
                next_attempt_at: backoff_next_attempt_at(0),
            })
        })
        .collect()
}

/// Evaluate fail2ban ban events.
///
/// Matches: `app_name = 'fail2ban'` AND message contains `'Ban '`.
pub fn evaluate_fail2ban_ban(rows: &[LogRow], apprise_urls_json: &str) -> Vec<OutboxInsertParams> {
    rows.iter()
        .filter(|r| r.app_name.as_deref() == Some("fail2ban") && r.message.contains("Ban "))
        .map(|r| {
            let title =
                escape_for_notification(&format!("[NOTICE] fail2ban ban on {}", r.hostname));
            let body = escape_for_notification(&format!(
                "fail2ban banned an IP on **{}** at {}\n\n{}",
                r.hostname, r.timestamp, r.message
            ));
            OutboxInsertParams {
                dedup_key: format!("fail2ban_ban:{}", r.hostname),
                rule_id: "fail2ban_ban".to_string(),
                severity: "notice".to_string(),
                hostname: r.hostname.clone(),
                title,
                body,
                apprise_urls_json: apprise_urls_json.to_string(),
                next_attempt_at: backoff_next_attempt_at(0),
            }
        })
        .collect()
}

/// Evaluate Authelia MFA failure events.
///
/// Matches: `app_name = 'authelia'` AND message contains `'second_factor'`.
pub fn evaluate_authelia_mfa_fail(
    rows: &[LogRow],
    apprise_urls_json: &str,
) -> Vec<OutboxInsertParams> {
    rows.iter()
        .filter(|r| {
            r.app_name.as_deref() == Some("authelia")
                && r.message.contains("second_factor")
                && (r.message.to_lowercase().contains("failed")
                    || r.message.to_lowercase().contains("error")
                    || r.message.to_lowercase().contains("invalid"))
        })
        .map(|r| {
            let title = escape_for_notification(&format!(
                "[WARNING] Authelia MFA failure on {}",
                r.hostname
            ));
            let body = escape_for_notification(&format!(
                "Authelia second factor event on **{}** at {}\n\n{}",
                r.hostname, r.timestamp, r.message
            ));
            OutboxInsertParams {
                dedup_key: format!("authelia_mfa:{}", r.hostname),
                rule_id: "authelia_mfa_fail".to_string(),
                severity: "warning".to_string(),
                hostname: r.hostname.clone(),
                title,
                body,
                apprise_urls_json: apprise_urls_json.to_string(),
                next_attempt_at: backoff_next_attempt_at(0),
            }
        })
        .collect()
}

/// Evaluate disk fill pressure from storage metrics.
///
/// This is NOT a log-scan rule — it takes raw bytes, not `&[LogRow]`.
/// The storage enforcement task in `src/runtime.rs` calls this function
/// directly after each `enforce_storage_budget` cycle.
///
/// Fires when `free_bytes` is below the configured guardrail thresholds:
///   - `free_bytes < critical_bytes` → "critical"
///   - `free_bytes < warn_bytes`     → "warning"
///   - otherwise                     → `None`
///
/// `critical_bytes` = `min_free_disk_mb * 1024 * 1024` from StorageConfig.
/// `warn_bytes`     = `recovery_free_disk_mb * 1024 * 1024` from StorageConfig.
///
/// Pass `critical_bytes = 0` or `warn_bytes = 0` to disable that threshold.
pub fn evaluate_disk_fill(
    hostname: &str,
    free_bytes: u64,
    critical_bytes: u64,
    warn_bytes: u64,
    apprise_urls_json: &str,
) -> Option<OutboxInsertParams> {
    let (severity, label) = if critical_bytes > 0 && free_bytes < critical_bytes {
        ("critical", "CRITICAL")
    } else if warn_bytes > 0 && free_bytes < warn_bytes {
        ("warning", "WARNING")
    } else {
        return None;
    };
    let free_mib = free_bytes / (1024 * 1024);
    let title = escape_for_notification(&format!(
        "[{label}] Disk fill on {hostname}: {free_mib} MiB free"
    ));
    let body = escape_for_notification(&format!(
        "Host **{hostname}** has only {free_mib} MiB disk space remaining."
    ));
    Some(OutboxInsertParams {
        dedup_key: format!("disk_fill:{hostname}:{severity}"),
        rule_id: "disk_fill".to_string(),
        severity: severity.to_string(),
        hostname: hostname.to_string(),
        title,
        body,
        apprise_urls_json: apprise_urls_json.to_string(),
        next_attempt_at: backoff_next_attempt_at(0),
    })
}

/// Evaluate ingest queue pressure from runtime counters.
///
/// Fires when queue-full transitions or queue-full drops have increased since
/// the previous evaluation cycle.
pub fn evaluate_ingest_queue_pressure(
    hostname: &str,
    full_transitions_delta: u64,
    udp_drops_delta: u64,
    tcp_drops_delta: u64,
    queue_depth: usize,
    queue_capacity: usize,
    apprise_urls_json: &str,
) -> Option<OutboxInsertParams> {
    if full_transitions_delta == 0 && udp_drops_delta == 0 && tcp_drops_delta == 0 {
        return None;
    }

    let title = escape_for_notification(&format!(
        "[WARNING] syslog ingest queue pressure on {hostname}"
    ));
    let body = escape_for_notification(&format!(
        "cortex observed queue pressure on **{hostname}** since the last check:\n\n\
         - queue-full transitions: `{full_transitions_delta}`\n\
         - UDP drops from full queue: `{udp_drops_delta}`\n\
         - TCP drops from full queue: `{tcp_drops_delta}`\n\
         - current queue depth: `{queue_depth}/{queue_capacity}`"
    ));

    Some(OutboxInsertParams {
        dedup_key: format!("ingest_queue_pressure:{hostname}"),
        rule_id: "ingest_queue_pressure".to_string(),
        severity: "warning".to_string(),
        hostname: hostname.to_string(),
        title,
        body,
        apprise_urls_json: apprise_urls_json.to_string(),
        next_attempt_at: backoff_next_attempt_at(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kernel_row(message: &str, hostname: &str) -> LogRow {
        LogRow {
            app_name: Some("kernel".to_string()),
            message: message.to_string(),
            hostname: hostname.to_string(),
            severity: "crit".to_string(),
            metadata_json: None,
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    fn container_die_row(hostname: &str, exit_code: &str) -> LogRow {
        let meta = serde_json::json!({
            "action": "die",
            "container_name": "nginx",
            "exit_code": exit_code,
        })
        .to_string();
        LogRow {
            app_name: Some("dockerd".to_string()),
            message: format!("Container nginx died with exit code {exit_code}"),
            hostname: hostname.to_string(),
            severity: "warning".to_string(),
            metadata_json: Some(meta),
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    fn container_die_row_numeric(hostname: &str, exit_code: i64) -> LogRow {
        let meta = serde_json::json!({
            "action": "die",
            "container_name": "nginx",
            "exit_code": exit_code,
        })
        .to_string();
        LogRow {
            app_name: Some("dockerd".to_string()),
            message: format!("Container nginx died with exit code {exit_code}"),
            hostname: hostname.to_string(),
            severity: "warning".to_string(),
            metadata_json: Some(meta),
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    fn fail2ban_row(hostname: &str, msg: &str) -> LogRow {
        LogRow {
            app_name: Some("fail2ban".to_string()),
            message: msg.to_string(),
            hostname: hostname.to_string(),
            severity: "notice".to_string(),
            metadata_json: None,
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    fn authelia_row(hostname: &str) -> LogRow {
        LogRow {
            app_name: Some("authelia".to_string()),
            message: "second_factor authentication failed for user".to_string(),
            hostname: hostname.to_string(),
            severity: "warning".to_string(),
            metadata_json: None,
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    #[test]
    fn oom_kill_matches() {
        let rows = vec![
            kernel_row("Out of memory: Killed process 1234 (nginx)", "server1"),
            kernel_row("Some unrelated kernel message", "server1"),
        ];
        let results = evaluate_oom_kill(&rows, "[]");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].rule_id, "oom_kill");
        assert_eq!(results[0].severity, "critical");
        assert_eq!(results[0].hostname, "server1");
    }

    #[test]
    fn oom_kill_wrong_app_name() {
        let rows = vec![LogRow {
            app_name: Some("systemd".to_string()),
            message: "Out of memory: Killed process 1234 (nginx)".to_string(),
            hostname: "server1".to_string(),
            severity: "crit".to_string(),
            metadata_json: None,
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
        }];
        let results = evaluate_oom_kill(&rows, "[]");
        assert_eq!(results.len(), 0, "should not match non-kernel app_name");
    }

    #[test]
    fn container_die_nonzero_matches() {
        let rows = vec![
            container_die_row("host1", "1"),
            container_die_row("host1", "0"), // exit 0 should not match
        ];
        let results = evaluate_container_die_nonzero(&rows, "[]");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].rule_id, "container_die_nonzero");
    }

    #[test]
    fn container_die_exit_zero_ignored() {
        let rows = vec![container_die_row("host1", "0")];
        let results = evaluate_container_die_nonzero(&rows, "[]");
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn container_die_numeric_exit_code_matches() {
        // exit_code as a JSON number (not string) should still be detected
        let rows = vec![
            container_die_row_numeric("host1", 1),
            container_die_row_numeric("host1", 0), // exit 0 should not match
        ];
        let results = evaluate_container_die_nonzero(&rows, "[]");
        assert_eq!(results.len(), 1, "numeric non-zero exit_code should match");
        assert_eq!(results[0].rule_id, "container_die_nonzero");
    }

    #[test]
    fn fail2ban_ban_matches() {
        let rows = vec![
            fail2ban_row(
                "fw1",
                "2026-01-01 00:00:00,000 fail2ban.actions [1234]: NOTICE  [sshd] Ban 1.2.3.4",
            ),
            fail2ban_row(
                "fw1",
                "2026-01-01 00:00:01,000 fail2ban.actions [1234]: NOTICE  [sshd] Unban 1.2.3.4",
            ),
        ];
        let results = evaluate_fail2ban_ban(&rows, "[]");
        assert_eq!(results.len(), 1, "only 'Ban ' messages should match");
        assert_eq!(results[0].rule_id, "fail2ban_ban");
        assert_eq!(results[0].severity, "notice");
    }

    #[test]
    fn authelia_mfa_fail_matches() {
        let rows = vec![
            authelia_row("authhost"),
            LogRow {
                app_name: Some("authelia".to_string()),
                message: "successful login".to_string(),
                hostname: "authhost".to_string(),
                severity: "info".to_string(),
                metadata_json: None,
                timestamp: "2026-01-01T00:00:00.000Z".to_string(),
            },
        ];
        let results = evaluate_authelia_mfa_fail(&rows, "[]");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].rule_id, "authelia_mfa_fail");
    }

    #[test]
    fn authelia_mfa_successful_second_factor_does_not_match() {
        // A successful second_factor event should NOT trigger an alert.
        let rows = vec![LogRow {
            app_name: Some("authelia".to_string()),
            message: "second_factor authentication successful for user".to_string(),
            hostname: "authhost".to_string(),
            severity: "info".to_string(),
            metadata_json: None,
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
        }];
        let results = evaluate_authelia_mfa_fail(&rows, "[]");
        assert_eq!(
            results.len(),
            0,
            "successful second_factor should not match"
        );
    }

    #[test]
    fn escaped_titles_in_rules() {
        let rows = vec![kernel_row(
            "Out of memory: Killed process 1234 <nginx@host>",
            "server1",
        )];
        let results = evaluate_oom_kill(&rows, "[]");
        assert!(
            !results[0].title.contains('@'),
            "@ should be escaped in title"
        );
        assert!(
            !results[0].body.contains('<'),
            "< should be stripped from body"
        );
    }

    #[test]
    fn disk_fill_critical_fires() {
        // 300 MiB free, critical threshold = 512 MiB → critical
        let result = evaluate_disk_fill(
            "nas1",
            300 * 1024 * 1024,
            512 * 1024 * 1024,
            768 * 1024 * 1024,
            "[]",
        );
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.rule_id, "disk_fill");
        assert_eq!(p.severity, "critical");
        assert_eq!(p.hostname, "nas1");
        assert!(p.dedup_key.contains("nas1"));
        assert!(p.dedup_key.contains("critical"));
    }

    #[test]
    fn disk_fill_warning_fires() {
        // 600 MiB free: above critical (512), below warn (768) → warning
        let result = evaluate_disk_fill(
            "nas1",
            600 * 1024 * 1024,
            512 * 1024 * 1024,
            768 * 1024 * 1024,
            "[]",
        );
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.severity, "warning");
        assert!(p.dedup_key.contains("warning"));
    }

    #[test]
    fn disk_fill_ok_does_not_fire() {
        // 1 GiB free: above both thresholds → no alert
        let result = evaluate_disk_fill(
            "nas1",
            1024 * 1024 * 1024,
            512 * 1024 * 1024,
            768 * 1024 * 1024,
            "[]",
        );
        assert!(result.is_none());
    }

    #[test]
    fn disk_fill_zero_thresholds_do_not_fire() {
        // disabled thresholds: critical=0, warn=0 → no alert
        let result = evaluate_disk_fill("nas1", 0, 0, 0, "[]");
        assert!(result.is_none());
    }

    #[test]
    fn ingest_queue_pressure_fires_on_drops() {
        let result = evaluate_ingest_queue_pressure("dookie", 1, 2, 3, 99, 100, "[]");

        let params = result.expect("queue pressure should fire");
        assert_eq!(params.rule_id, "ingest_queue_pressure");
        assert_eq!(params.severity, "warning");
        assert!(params.body.contains("TCP drops"));
    }

    #[test]
    fn ingest_queue_pressure_ok_does_not_fire() {
        let result = evaluate_ingest_queue_pressure("dookie", 0, 0, 0, 0, 100, "[]");
        assert!(result.is_none());
    }
}
