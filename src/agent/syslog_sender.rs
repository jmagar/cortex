use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::sleep;

const CHANNEL_CAP: usize = 4_096;
const RECONNECT_MAX_MS: u64 = 30_000;

/// Async RFC 5424 syslog sender over TCP with automatic reconnect.
///
/// Messages are newline-terminated (non-transparent TCP framing, compatible
/// with the cortex receiver on port 1514).  If the TCP connection drops, the
/// writer loop reconnects with exponential back-off; messages that arrive
/// during reconnect are dropped (at-most-once, acceptable for log forwarding).
pub struct SyslogSender {
    tx: mpsc::Sender<String>,
}

impl SyslogSender {
    /// Spawn a background writer task and return the sender handle.
    pub fn new(target: String) -> Self {
        let (tx, rx) = mpsc::channel(CHANNEL_CAP);
        tokio::spawn(writer_loop(target, rx));
        Self { tx }
    }

    pub async fn send(&self, line: String) -> Result<()> {
        self.tx.send(line).await.context("syslog sender closed")
    }

    /// Non-blocking send; silently drops if the channel is full.
    pub fn try_send(&self, line: String) {
        let _ = self.tx.try_send(line);
    }
}

async fn writer_loop(target: String, mut rx: mpsc::Receiver<String>) {
    loop {
        match connect_and_drain(&target, &mut rx).await {
            Ok(()) => return, // channel closed — clean shutdown
            Err(e) => {
                tracing::warn!(target = %target, error = %e, "syslog TCP write failed; reconnecting");
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

async fn connect_and_drain(target: &str, rx: &mut mpsc::Receiver<String>) -> Result<()> {
    let mut stream = connect_with_backoff(target).await;
    while let Some(line) = rx.recv().await {
        let bytes = format!("{line}\n");
        stream.write_all(bytes.as_bytes()).await?;
    }
    Ok(())
}

async fn connect_with_backoff(target: &str) -> TcpStream {
    let mut attempt = 0u32;
    loop {
        match TcpStream::connect(target).await {
            Ok(s) => {
                tracing::debug!(target, "syslog TCP connected");
                return s;
            }
            Err(e) => {
                let delay = backoff_ms(attempt);
                tracing::warn!(target, error = %e, delay_ms = delay, "syslog connect failed; retrying");
                sleep(Duration::from_millis(delay)).await;
                attempt = attempt.saturating_add(1);
            }
        }
    }
}

fn backoff_ms(attempt: u32) -> u64 {
    500u64
        .saturating_mul(1u64 << attempt.min(6))
        .min(RECONNECT_MAX_MS)
}

// ── RFC 5424 formatting helpers ────────────────────────────────────────────

/// Format an RFC 5424 syslog line.
/// `pri` = facility * 8 + severity.  `procid` and `app_name` must be
/// printable ASCII with no spaces.
pub fn format_rfc5424(
    pri: u8,
    timestamp: &str,
    hostname: &str,
    app_name: &str,
    procid: &str,
    msg: &str,
) -> String {
    let msg = msg.replace('\n', " ");
    let app = sanitise_field(app_name, "cortex-agent");
    let proc = sanitise_field(procid, "-");
    format!("<{pri}>1 {timestamp} {hostname} {app} {proc} - - {msg}")
}

/// local0.info  (facility 16, severity 6)
pub const PRI_LOCAL0_INFO: u8 = 16 * 8 + 6; // 134
/// local0.warning (facility 16, severity 4)
pub const PRI_LOCAL0_WARN: u8 = 16 * 8 + 4; // 132
/// local0.err   (facility 16, severity 3)
pub const PRI_LOCAL0_ERR: u8 = 16 * 8 + 3; // 131

/// Map a journald/syslog numeric severity (0-7) to a local0 PRI.
pub fn local0_pri(severity: u8) -> u8 {
    16 * 8 + severity.min(7)
}

fn sanitise_field<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() || value.len() > 48 || !value.bytes().all(|b| b.is_ascii_graphic()) {
        fallback
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_ms_doubles_until_capped() {
        assert_eq!(backoff_ms(0), 500);
        assert_eq!(backoff_ms(1), 1_000);
        assert_eq!(backoff_ms(6), 30_000);
        assert_eq!(backoff_ms(42), 30_000);
    }

    #[test]
    fn local0_pri_clamps_unknown_severity_to_debug() {
        assert_eq!(local0_pri(0), 128);
        assert_eq!(local0_pri(6), PRI_LOCAL0_INFO);
        assert_eq!(local0_pri(99), 135);
    }

    #[test]
    fn format_rfc5424_replaces_newlines_and_keeps_valid_fields() {
        let line = format_rfc5424(
            PRI_LOCAL0_ERR,
            "2026-06-12T12:00:00.000Z",
            "dookie",
            "compose/service",
            "abcdef123456",
            "first\nsecond",
        );

        assert_eq!(
            line,
            "<131>1 2026-06-12T12:00:00.000Z dookie compose/service abcdef123456 - - first second"
        );
    }

    #[test]
    fn format_rfc5424_replaces_empty_spacey_or_long_structured_fields() {
        let long_app = "x".repeat(49);
        assert!(
            format_rfc5424(PRI_LOCAL0_INFO, "ts", "host", "", "pid", "msg")
                .contains(" cortex-agent pid ")
        );
        assert!(
            format_rfc5424(PRI_LOCAL0_INFO, "ts", "host", "bad app", "pid", "msg")
                .contains(" cortex-agent pid ")
        );
        assert!(
            format_rfc5424(PRI_LOCAL0_INFO, "ts", "host", &long_app, "pid", "msg")
                .contains(" cortex-agent pid ")
        );
        assert!(
            format_rfc5424(PRI_LOCAL0_INFO, "ts", "host", "app", "bad pid", "msg")
                .contains(" app - - - msg")
        );
    }
}
