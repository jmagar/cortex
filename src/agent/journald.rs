use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::syslog_sender::{SyslogSender, format_rfc5424, local0_pri};

/// Read from journald via `journalctl -f -o json` and forward entries as
/// RFC 5424 syslog to the given sender.  Unix-only.
#[cfg(unix)]
pub async fn run_journald_forwarder(hostname: &str, sender: Arc<SyslogSender>) -> Result<()> {
    let mut child = Command::new("journalctl")
        .args(["-f", "-o", "json", "--no-pager"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("spawn journalctl")?;

    let stdout = child.stdout.take().context("journalctl stdout")?;
    let mut lines = BufReader::new(stdout).lines();

    while let Some(line) = lines.next_line().await? {
        if let Some(syslog_line) = parse_entry(hostname, &line) {
            sender.try_send(syslog_line);
        }
    }

    child.wait().await.context("journalctl exited")?;
    Ok(())
}

#[cfg(not(unix))]
pub async fn run_journald_forwarder(_hostname: &str, _sender: Arc<SyslogSender>) -> Result<()> {
    anyhow::bail!("journald is not available on this platform")
}

fn parse_entry(hostname: &str, line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;

    let msg = v.get("MESSAGE")?.as_str()?.trim();
    if msg.is_empty() {
        return None;
    }

    let priority: u8 = v
        .get("PRIORITY")
        .and_then(|p| p.as_str())
        .and_then(|p| p.parse().ok())
        .unwrap_or(6); // default info

    let app_name = v
        .get("SYSLOG_IDENTIFIER")
        .or_else(|| v.get("_SYSTEMD_UNIT"))
        .and_then(|v| v.as_str())
        .unwrap_or("journald");

    let procid = v.get("_PID").and_then(|p| p.as_str()).unwrap_or("-");

    let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    Some(format_rfc5424(
        local0_pri(priority),
        &ts,
        hostname,
        app_name,
        procid,
        msg,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_entry_formats_journald_json_as_syslog_line() {
        let line = r#"{
            "MESSAGE": "disk almost full",
            "PRIORITY": "3",
            "SYSLOG_IDENTIFIER": "systemd",
            "_PID": "1234"
        }"#;

        let parsed = parse_entry("dookie", line).unwrap();

        assert!(parsed.starts_with("<131>1 "));
        assert!(parsed.contains(" dookie systemd 1234 - - disk almost full"));
    }

    #[test]
    fn parse_entry_uses_systemd_unit_and_default_priority_when_fields_are_missing() {
        let line = r#"{
            "MESSAGE": "started service",
            "_SYSTEMD_UNIT": "cortex.service"
        }"#;

        let parsed = parse_entry("dookie", line).unwrap();

        assert!(parsed.starts_with("<134>1 "));
        assert!(parsed.contains(" dookie cortex.service - - - started service"));
    }

    #[test]
    fn parse_entry_rejects_invalid_empty_or_messageless_entries() {
        assert_eq!(parse_entry("dookie", "not-json"), None);
        assert_eq!(parse_entry("dookie", r#"{"PRIORITY":"3"}"#), None);
        assert_eq!(parse_entry("dookie", r#"{"MESSAGE":"   "}"#), None);
    }
}
