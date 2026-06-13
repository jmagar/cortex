use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use cortex::heartbeat_agent::{HeartbeatAgentConfig, run_agent};

use super::{HeartbeatAgentArgs, HeartbeatCommand};

pub(crate) async fn run_heartbeat_no_db(command: HeartbeatCommand) -> Result<()> {
    match command {
        HeartbeatCommand::Agent(args) => run_agent(args.into_config()?).await,
    }
}

impl HeartbeatAgentArgs {
    fn into_config(self) -> Result<HeartbeatAgentConfig> {
        let host_id_path = self
            .host_id_path
            .map(PathBuf::from)
            .unwrap_or_else(default_host_id_path);
        let mut config = HeartbeatAgentConfig::from_env(host_id_path);
        if let Some(target) = self.target {
            config.target = Some(target);
        }
        if let Some(token) = self.token {
            config.token = Some(token);
        }
        config.interval = Duration::from_secs(self.interval_secs);
        config.probe_deadline = Duration::from_millis(self.probe_deadline_ms);
        config.collection_deadline = Duration::from_millis(self.collection_deadline_ms);
        config.retry_buffer_limit = self.retry_buffer;
        config.once = self.once;
        config.emit = self.emit;
        config.json = self.json;
        if self.docker {
            config.docker = true;
        }
        if let Some(url) = self.docker_url {
            config.docker_url = url;
        }
        if self.journald {
            config.journald = true;
        }
        if let Some(target) = self.syslog_target {
            config.syslog_target = Some(target);
        }
        Ok(config)
    }
}

pub(crate) fn default_host_id_path() -> PathBuf {
    cortex::setup::cortex_home_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("heartbeat-host-id")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> HeartbeatAgentArgs {
        HeartbeatAgentArgs {
            target: None,
            token: None,
            interval_secs: 30,
            probe_deadline_ms: 2_000,
            collection_deadline_ms: 5_000,
            retry_buffer: 32,
            once: false,
            emit: false,
            json: false,
            host_id_path: Some("/tmp/cortex-host-id".to_string()),
            docker: false,
            docker_url: None,
            journald: false,
            syslog_target: None,
        }
    }

    #[test]
    fn into_config_maps_explicit_cli_flags() {
        let config = HeartbeatAgentArgs {
            target: Some("http://cortex.example".to_string()),
            token: Some("secret".to_string()),
            interval_secs: 7,
            probe_deadline_ms: 800,
            collection_deadline_ms: 1_500,
            retry_buffer: 3,
            once: true,
            emit: true,
            json: true,
            host_id_path: Some("/tmp/host-id".to_string()),
            docker: true,
            docker_url: Some("unix:///tmp/docker.sock".to_string()),
            journald: true,
            syslog_target: Some("127.0.0.1:1514".to_string()),
        }
        .into_config()
        .unwrap();

        assert_eq!(config.target.as_deref(), Some("http://cortex.example"));
        assert_eq!(config.token.as_deref(), Some("secret"));
        assert_eq!(config.interval, Duration::from_secs(7));
        assert_eq!(config.probe_deadline, Duration::from_millis(800));
        assert_eq!(config.collection_deadline, Duration::from_millis(1_500));
        assert_eq!(config.retry_buffer_limit, 3);
        assert!(config.once);
        assert!(config.emit);
        assert!(config.json);
        assert_eq!(config.host_id_path, PathBuf::from("/tmp/host-id"));
        assert!(config.docker);
        assert_eq!(config.docker_url, "unix:///tmp/docker.sock");
        assert!(config.journald);
        assert_eq!(config.syslog_target.as_deref(), Some("127.0.0.1:1514"));
    }

    #[test]
    fn into_config_preserves_env_defaults_when_optional_flags_are_absent() {
        let config = base_args().into_config().unwrap();

        assert_eq!(config.host_id_path, PathBuf::from("/tmp/cortex-host-id"));
        assert_eq!(config.interval, Duration::from_secs(30));
        assert_eq!(config.probe_deadline, Duration::from_millis(2_000));
        assert_eq!(config.collection_deadline, Duration::from_millis(5_000));
        assert_eq!(config.retry_buffer_limit, 32);
        assert!(!config.once);
        assert!(!config.emit);
        assert!(!config.json);
        assert!(!config.docker);
        assert!(!config.journald);
        assert_eq!(config.syslog_target, None);
    }
}
