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
                dedup_key: format!("oom_kill:{}:{}", r.hostname, &r.timestamp),
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
        .filter(|r| {
            if let Some(ref meta) = r.metadata_json {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(meta) {
                    let action_match = v.get("action").and_then(|a| a.as_str()) == Some("die");
                    let exit_code = v.get("exit_code").and_then(|e| e.as_str()).unwrap_or("0");
                    return action_match && exit_code != "0";
                }
            }
            false
        })
        .map(|r| {
            let container = r
                .metadata_json
                .as_ref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| {
                    v.get("container_name")
                        .and_then(|c| c.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| "unknown".to_string());
            let exit_code = r
                .metadata_json
                .as_ref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| {
                    v.get("exit_code")
                        .and_then(|e| e.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| "?".to_string());

            let title = escape_for_notification(&format!(
                "[WARNING] Container {} died (exit {}) on {}",
                container, exit_code, r.hostname
            ));
            let body = escape_for_notification(&format!(
                "Container **{}** exited with code `{}` on **{}** at {}",
                container, exit_code, r.hostname, r.timestamp
            ));
            OutboxInsertParams {
                dedup_key: format!("container_die:{}:{}:{}", r.hostname, container, r.timestamp),
                rule_id: "container_die_nonzero".to_string(),
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
                dedup_key: format!("fail2ban_ban:{}:{}", r.hostname, r.timestamp),
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
            r.app_name.as_deref() == Some("authelia") && r.message.contains("second_factor")
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
                dedup_key: format!("authelia_mfa:{}:{}", r.hostname, r.timestamp),
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
            message: "second_factor authentication failed".to_string(),
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
}
