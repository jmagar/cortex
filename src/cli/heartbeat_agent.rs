use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use cortex::heartbeat_agent::{run_agent, HeartbeatAgentConfig};

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
        Ok(config)
    }
}

pub(crate) fn default_host_id_path() -> PathBuf {
    cortex::setup::syslog_home_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("heartbeat-host-id")
}
