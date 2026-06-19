pub mod docker;
pub mod journald;
pub mod self_update;
pub mod syslog_file;
pub mod syslog_sender;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::task::JoinSet;
use tokio::time::sleep;

use syslog_file::FileTailSource;
use syslog_sender::SyslogSender;

const RESTART_DELAY_SECS: u64 = 5;

#[derive(Debug, Clone)]
pub struct AgentStreamsConfig {
    pub docker: bool,
    /// Docker endpoint.  On Linux defaults to `unix:///var/run/docker.sock`;
    /// HTTP endpoints like `http://localhost:2375` are also accepted.
    pub docker_url: String,
    pub journald: bool,
    /// Optional host syslog file to tail and forward.  Used by containerized
    /// agents that cannot read the host journal directly.
    pub syslog_file: Option<PathBuf>,
    /// Arbitrary app log files to tail, each forwarded raw under a fixed tag
    /// (`CORTEX_AGENT_FILE_TAILS`).  Replaces host-side rsyslog imfile drop-ins
    /// for file-only sources (AdGuard query log, SWAG access, fail2ban, Plex).
    pub file_tails: Vec<FileTailSource>,
    /// TCP syslog target in `host:port` form.  Derived from the heartbeat
    /// target when not set explicitly.
    pub syslog_target: String,
    pub hostname: String,
}

impl AgentStreamsConfig {
    /// Derive the syslog target from an HTTP heartbeat target URL.
    ///
    /// `http://dookie:3100` → `dookie:1514`
    pub fn syslog_target_from_heartbeat(heartbeat_target: &str) -> Option<String> {
        let s = heartbeat_target.trim_end_matches('/');
        let host_part = s
            .strip_prefix("http://")
            .or_else(|| s.strip_prefix("https://"))?;
        let host = host_part.split(':').next()?.split('/').next()?;
        if host.is_empty() {
            return None;
        }
        Some(format!("{host}:1514"))
    }
}

/// Spawn Docker and/or journald log-forwarding tasks.  Each task restarts
/// automatically on failure.  Returns when all tasks exit (i.e. only on
/// shutdown / panic).
pub async fn run_agent_streams(config: AgentStreamsConfig) -> Result<()> {
    if !config.docker
        && !config.journald
        && config.syslog_file.is_none()
        && config.file_tails.is_empty()
    {
        return Ok(());
    }

    let sender = Arc::new(SyslogSender::new(config.syslog_target.clone()));
    let mut tasks: JoinSet<()> = JoinSet::new();

    if config.docker {
        let docker_url = config.docker_url.clone();
        let hostname = config.hostname.clone();
        let sender = Arc::clone(&sender);
        tasks.spawn(async move {
            loop {
                match docker::run_docker_forwarder(&docker_url, &hostname, Arc::clone(&sender))
                    .await
                {
                    Ok(()) => return,
                    Err(e) => {
                        tracing::warn!(error = %e, "docker forwarder exited; restarting");
                        sleep(Duration::from_secs(RESTART_DELAY_SECS)).await;
                    }
                }
            }
        });
    }

    if config.journald {
        let hostname = config.hostname.clone();
        let sender = Arc::clone(&sender);
        tasks.spawn(async move {
            loop {
                match journald::run_journald_forwarder(&hostname, Arc::clone(&sender)).await {
                    Ok(()) => return,
                    Err(e) => {
                        tracing::warn!(error = %e, "journald forwarder exited; restarting");
                        sleep(Duration::from_secs(RESTART_DELAY_SECS)).await;
                    }
                }
            }
        });
    }

    if let Some(path) = config.syslog_file {
        let hostname = config.hostname.clone();
        let sender = Arc::clone(&sender);
        tasks.spawn(async move {
            loop {
                match syslog_file::run_syslog_file_forwarder(&path, &hostname, Arc::clone(&sender))
                    .await
                {
                    Ok(()) => return,
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "syslog file forwarder exited; restarting"
                        );
                        sleep(Duration::from_secs(RESTART_DELAY_SECS)).await;
                    }
                }
            }
        });
    }

    // One tailing task per configured app log file, each forwarded raw under
    // its fixed tag.
    for source in config.file_tails {
        let hostname = config.hostname.clone();
        let sender = Arc::clone(&sender);
        tasks.spawn(async move {
            loop {
                match syslog_file::run_file_forwarder(
                    &source.path,
                    &hostname,
                    source.tag.as_deref(),
                    Arc::clone(&sender),
                )
                .await
                {
                    Ok(()) => return,
                    Err(e) => {
                        tracing::warn!(
                            path = %source.path.display(),
                            tag = source.tag.as_deref().unwrap_or("<syslog>"),
                            error = %e,
                            "file forwarder exited; restarting"
                        );
                        sleep(Duration::from_secs(RESTART_DELAY_SECS)).await;
                    }
                }
            }
        });
    }

    while tasks.join_next().await.is_some() {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syslog_target_from_heartbeat_extracts_host_and_uses_syslog_port() {
        assert_eq!(
            AgentStreamsConfig::syslog_target_from_heartbeat("http://dookie:3100"),
            Some("dookie:1514".to_string())
        );
        assert_eq!(
            AgentStreamsConfig::syslog_target_from_heartbeat(
                "https://cortex.example.test:3100/mcp/"
            ),
            Some("cortex.example.test:1514".to_string())
        );
    }

    #[test]
    fn syslog_target_from_heartbeat_rejects_non_http_or_missing_host() {
        assert_eq!(
            AgentStreamsConfig::syslog_target_from_heartbeat("dookie:3100"),
            None
        );
        assert_eq!(
            AgentStreamsConfig::syslog_target_from_heartbeat("https:///mcp"),
            None
        );
    }

    #[tokio::test]
    async fn run_agent_streams_returns_immediately_when_all_sources_disabled() {
        let config = AgentStreamsConfig {
            docker: false,
            docker_url: "unix:///var/run/docker.sock".to_string(),
            journald: false,
            syslog_file: None,
            file_tails: Vec::new(),
            syslog_target: "127.0.0.1:1514".to_string(),
            hostname: "test-host".to_string(),
        };

        run_agent_streams(config).await.unwrap();
    }

    #[test]
    fn parse_file_tails_extracts_path_and_tag_pairs() {
        use syslog_file::parse_file_tails;
        let parsed = parse_file_tails(
            "/mnt/appdata/adguard/var/data/querylog.json:adguard-query, /log/access.log:swag-access",
        );
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0].path,
            PathBuf::from("/mnt/appdata/adguard/var/data/querylog.json")
        );
        assert_eq!(parsed[0].tag.as_deref(), Some("adguard-query"));
        assert_eq!(parsed[1].path, PathBuf::from("/log/access.log"));
        assert_eq!(parsed[1].tag.as_deref(), Some("swag-access"));

        // Entries without a usable :TAG are skipped; empties ignored.
        assert!(parse_file_tails("/no/tag/here").is_empty());
        assert!(parse_file_tails(" , ").is_empty());
    }
}
