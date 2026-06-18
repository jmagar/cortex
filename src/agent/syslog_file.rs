use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader, SeekFrom};
use tokio::time::sleep;

use super::syslog_sender::{PRI_LOCAL0_INFO, SyslogSender, format_rfc5424};

const EOF_SLEEP_MS: u64 = 500;

/// One file the agent tails and forwards to the cortex syslog receiver.
///
/// When `tag` is `Some`, lines are forwarded **raw** under that fixed
/// `app_name` (the message is the verbatim line) — this is how arbitrary app
/// log files (AdGuard's JSON `querylog.json`, SWAG `access.log`, fail2ban,
/// Plex) reach the right cortex parser without any host-side rsyslog drop-in.
/// When `tag` is `None`, each line is parsed as an RFC 3164 syslog record
/// (back-compat with `CORTEX_AGENT_SYSLOG_FILE` tailing `/var/log/syslog`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTailSource {
    pub path: PathBuf,
    pub tag: Option<String>,
}

/// Parse `CORTEX_AGENT_FILE_TAILS` — a comma-separated list of `PATH:TAG`
/// entries (e.g. `/data/querylog.json:adguard-query,/log/access.log:swag-access`).
/// The tag is the segment after the final colon, so absolute Linux paths (no
/// colon) round-trip cleanly. Entries without a usable `:TAG` suffix are
/// skipped.
pub fn parse_file_tails(spec: &str) -> Vec<FileTailSource> {
    spec.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let (path, tag) = entry.rsplit_once(':')?;
            let (path, tag) = (path.trim(), tag.trim());
            if path.is_empty() || tag.is_empty() {
                return None;
            }
            Some(FileTailSource {
                path: PathBuf::from(path),
                tag: Some(tag.to_string()),
            })
        })
        .collect()
}

struct ParsedSyslogLine<'a> {
    hostname: &'a str,
    app_name: &'a str,
    procid: &'a str,
    message: &'a str,
}

/// Tail a file from EOF and forward new lines to the cortex syslog receiver.
///
/// With `forced_app_name = Some(tag)` the line is forwarded verbatim as the
/// message under that `app_name` (raw mode, for arbitrary app log files). With
/// `None` the line is parsed as an RFC 3164 syslog record.
///
/// "Follow only": starting at EOF avoids replaying a large rotated backlog when
/// a heartbeat-agent container is redeployed.
pub async fn run_file_forwarder(
    path: &Path,
    fallback_hostname: &str,
    forced_app_name: Option<&str>,
    sender: Arc<SyslogSender>,
) -> Result<()> {
    let mut reader = open_at_end(path).await?;
    let mut position = reader.stream_position().await?;
    tracing::info!(
        path = %path.display(),
        tag = forced_app_name.unwrap_or("<syslog>"),
        "file forwarder following"
    );

    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            if file_was_truncated(path, position).await {
                tracing::info!(path = %path.display(), "file truncated; reopening");
                reader = open_at_end(path).await?;
                position = reader.stream_position().await?;
            }
            sleep(Duration::from_millis(EOF_SLEEP_MS)).await;
            continue;
        }

        position = position.saturating_add(read as u64);
        let raw = line.trim_end_matches(['\r', '\n']);
        if raw.is_empty() {
            continue;
        }

        let parsed = match forced_app_name {
            // Raw mode: verbatim line under the configured tag.
            Some(tag) => ParsedSyslogLine {
                hostname: fallback_hostname,
                app_name: tag,
                procid: "-",
                message: raw,
            },
            None => parse_syslog_line(raw, fallback_hostname),
        };
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let forwarded = format_rfc5424(
            PRI_LOCAL0_INFO,
            &timestamp,
            parsed.hostname,
            parsed.app_name,
            parsed.procid,
            parsed.message,
        );
        sender.try_send(forwarded);
    }
}

/// Tail a host syslog file (RFC 3164 lines). Back-compat wrapper over
/// [`run_file_forwarder`] with no forced tag.
pub async fn run_syslog_file_forwarder(
    path: &Path,
    fallback_hostname: &str,
    sender: Arc<SyslogSender>,
) -> Result<()> {
    run_file_forwarder(path, fallback_hostname, None, sender).await
}

async fn open_at_end(path: &Path) -> Result<BufReader<File>> {
    let mut file = File::open(path)
        .await
        .with_context(|| format!("open {}", path.display()))?;
    file.seek(SeekFrom::End(0)).await?;
    Ok(BufReader::new(file))
}

async fn file_was_truncated(path: &Path, position: u64) -> bool {
    tokio::fs::metadata(path)
        .await
        .map(|metadata| metadata.len() < position)
        .unwrap_or(false)
}

fn parse_syslog_line<'a>(line: &'a str, fallback_hostname: &'a str) -> ParsedSyslogLine<'a> {
    let Some(after_timestamp) = strip_rfc3164_timestamp(line) else {
        return ParsedSyslogLine {
            hostname: fallback_hostname,
            app_name: "syslog-file",
            procid: "-",
            message: line,
        };
    };
    let Some((hostname, rest)) = after_timestamp.split_once(' ') else {
        return ParsedSyslogLine {
            hostname: fallback_hostname,
            app_name: "syslog-file",
            procid: "-",
            message: line,
        };
    };
    let Some((tag, message)) = rest.split_once(':') else {
        return ParsedSyslogLine {
            hostname,
            app_name: "syslog-file",
            procid: "-",
            message: rest,
        };
    };

    let (app_name, procid) = parse_tag(tag);
    ParsedSyslogLine {
        hostname,
        app_name,
        procid,
        message: message.trim_start(),
    }
}

fn strip_rfc3164_timestamp(line: &str) -> Option<&str> {
    if line.len() < 16 {
        return None;
    }
    let bytes = line.as_bytes();
    if bytes.get(3) != Some(&b' ')
        || bytes.get(6) != Some(&b' ')
        || bytes.get(9) != Some(&b':')
        || bytes.get(12) != Some(&b':')
        || bytes.get(15) != Some(&b' ')
    {
        return None;
    }
    line.get(16..)
}

fn parse_tag(tag: &str) -> (&str, &str) {
    if let Some(open) = tag.rfind('[') {
        if tag.ends_with(']') && open > 0 {
            return (&tag[..open], &tag[open + 1..tag.len() - 1]);
        }
    }
    (tag, "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc3164_program_and_pid() {
        let parsed = parse_syslog_line(
            "Jun 11 09:24:12 tootie nginx[123]: request handled",
            "fallback",
        );
        assert_eq!(parsed.hostname, "tootie");
        assert_eq!(parsed.app_name, "nginx");
        assert_eq!(parsed.procid, "123");
        assert_eq!(parsed.message, "request handled");
    }

    #[test]
    fn parses_kernel_style_tag_without_pid() {
        let parsed = parse_syslog_line("Jun 11 09:24:12 shart kernel: disk online", "fallback");
        assert_eq!(parsed.hostname, "shart");
        assert_eq!(parsed.app_name, "kernel");
        assert_eq!(parsed.procid, "-");
        assert_eq!(parsed.message, "disk online");
    }

    #[test]
    fn falls_back_for_unstructured_lines() {
        let parsed = parse_syslog_line("not actually syslog", "tootie");
        assert_eq!(parsed.hostname, "tootie");
        assert_eq!(parsed.app_name, "syslog-file");
        assert_eq!(parsed.procid, "-");
        assert_eq!(parsed.message, "not actually syslog");
    }
}
