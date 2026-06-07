use anyhow::{anyhow, Result};
use futures_util::future::BoxFuture;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::inventory::process::{run_command, CommandOutput};

const SSH_IGNORE_UNKNOWN_OPTIONS: &str = "IgnoreUnknown=WarnWeakCrypto";
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 4;
const DEFAULT_SERVER_ALIVE_INTERVAL_SECS: u64 = 3;
const DEFAULT_SERVER_ALIVE_COUNT_MAX: u64 = 1;
pub const DEFAULT_MAX_CONCURRENT_SSH: usize = 8;
const DEFAULT_RETRY_ATTEMPTS: usize = 1;
const DEFAULT_RETRY_INITIAL_BACKOFF_MS: u64 = 250;

type SshRunner =
    Arc<dyn Fn(Vec<String>, Duration) -> BoxFuture<'static, Result<CommandOutput>> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshHostKeyPolicy {
    Strict,
    AcceptNew,
}

#[derive(Clone)]
pub struct SshOptions {
    pub config: Option<PathBuf>,
    pub known_hosts: Option<PathBuf>,
    pub host_key_policy: SshHostKeyPolicy,
    pub connect_timeout_secs: u64,
    pub server_alive_interval_secs: u64,
    pub server_alive_count_max: u64,
    pub max_concurrent: usize,
    pub retry_attempts: usize,
    pub retry_initial_backoff: Duration,
}

impl Default for SshOptions {
    fn default() -> Self {
        Self {
            config: None,
            known_hosts: None,
            host_key_policy: SshHostKeyPolicy::Strict,
            connect_timeout_secs: DEFAULT_CONNECT_TIMEOUT_SECS,
            server_alive_interval_secs: DEFAULT_SERVER_ALIVE_INTERVAL_SECS,
            server_alive_count_max: DEFAULT_SERVER_ALIVE_COUNT_MAX,
            max_concurrent: DEFAULT_MAX_CONCURRENT_SSH,
            retry_attempts: DEFAULT_RETRY_ATTEMPTS,
            retry_initial_backoff: Duration::from_millis(DEFAULT_RETRY_INITIAL_BACKOFF_MS),
        }
    }
}

impl SshOptions {
    pub fn for_config(config: Option<&Path>) -> Self {
        Self {
            config: config.map(Path::to_path_buf),
            ..Self::default()
        }
    }

    pub fn from_env(config: Option<&Path>) -> Self {
        let mut options = Self::for_config(config);
        options.known_hosts = env_path("CORTEX_INVENTORY_SSH_KNOWN_HOSTS");
        if env_bool("CORTEX_INVENTORY_SSH_TRUST_ON_FIRST_USE").unwrap_or(false) {
            options.host_key_policy = SshHostKeyPolicy::AcceptNew;
        }
        if let Some(max) = env_usize("CORTEX_INVENTORY_SSH_MAX_CONCURRENT").filter(|v| *v > 0) {
            options.max_concurrent = max;
        }
        if let Some(attempts) = env_usize("CORTEX_INVENTORY_SSH_RETRY_ATTEMPTS").filter(|v| *v > 0)
        {
            options.retry_attempts = attempts;
        }
        if let Some(backoff_ms) =
            env_u64("CORTEX_INVENTORY_SSH_RETRY_INITIAL_BACKOFF_MS").filter(|v| *v > 0)
        {
            options.retry_initial_backoff = Duration::from_millis(backoff_ms);
        }
        options
    }

    pub fn with_event_stream_defaults(mut self) -> Self {
        self.server_alive_interval_secs = 15;
        self.server_alive_count_max = 2;
        self
    }

    pub fn ssh_args(&self, host: &str, remote_command: &str) -> Result<Vec<String>> {
        if !is_safe_ssh_host(host) {
            anyhow::bail!("unsafe ssh host: {host}");
        }
        let mut args = Vec::new();
        args.push("-o".to_string());
        args.push(SSH_IGNORE_UNKNOWN_OPTIONS.to_string());
        if let Some(config) = &self.config {
            args.push("-F".to_string());
            args.push(config.display().to_string());
        }
        args.extend(["-o".to_string(), "BatchMode=yes".to_string()]);
        args.push("-o".to_string());
        args.push(match self.host_key_policy {
            SshHostKeyPolicy::Strict => "StrictHostKeyChecking=yes".to_string(),
            SshHostKeyPolicy::AcceptNew => "StrictHostKeyChecking=accept-new".to_string(),
        });
        if let Some(known_hosts) = &self.known_hosts {
            args.push("-o".to_string());
            args.push(format!("UserKnownHostsFile={}", known_hosts.display()));
        }
        args.extend([
            "-o".to_string(),
            format!("ConnectTimeout={}", self.connect_timeout_secs),
            "-o".to_string(),
            format!("ServerAliveInterval={}", self.server_alive_interval_secs),
            "-o".to_string(),
            format!("ServerAliveCountMax={}", self.server_alive_count_max),
            "--".to_string(),
            host.to_string(),
            remote_command.to_string(),
        ]);
        Ok(args)
    }
}

#[derive(Clone)]
pub struct SshContext {
    options: Arc<SshOptions>,
    limiter: Arc<Semaphore>,
    runner: SshRunner,
}

impl SshContext {
    pub fn new(options: SshOptions) -> Self {
        let runner: SshRunner = Arc::new(|args, timeout| {
            Box::pin(async move {
                let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
                run_command("ssh", &refs, timeout).await
            })
        });
        Self::with_runner(options, runner)
    }

    fn with_runner(options: SshOptions, runner: SshRunner) -> Self {
        let max_concurrent = options.max_concurrent.max(1);
        Self {
            options: Arc::new(options),
            limiter: Arc::new(Semaphore::new(max_concurrent)),
            runner,
        }
    }

    #[cfg(test)]
    pub fn with_runner_for_test<F>(options: SshOptions, runner: F) -> Self
    where
        F: Fn(Vec<String>, Duration) -> BoxFuture<'static, Result<CommandOutput>>
            + Send
            + Sync
            + 'static,
    {
        Self::with_runner(options, Arc::new(runner))
    }

    pub async fn run(
        &self,
        host: &str,
        remote_command: &str,
        timeout: Duration,
    ) -> Result<CommandOutput> {
        let args = self.options.ssh_args(host, remote_command)?;
        let attempts = self.options.retry_attempts.max(1);
        let mut last_error = None;
        for attempt in 0..attempts {
            let _permit = self
                .limiter
                .acquire()
                .await
                .map_err(|_| anyhow!("ssh concurrency limiter closed"))?;
            match (self.runner)(args.clone(), timeout).await {
                Ok(output) => return Ok(output),
                Err(error) => last_error = Some(error),
            }
            drop(_permit);
            if attempt + 1 < attempts {
                tokio::time::sleep(backoff_delay(self.options.retry_initial_backoff, attempt))
                    .await;
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("ssh command failed without an error")))
    }

    pub fn ssh_args(&self, host: &str, remote_command: &str) -> Result<Vec<String>> {
        self.options.ssh_args(host, remote_command)
    }

    pub async fn acquire_owned(&self) -> Result<OwnedSemaphorePermit> {
        Arc::clone(&self.limiter)
            .acquire_owned()
            .await
            .map_err(|_| anyhow!("ssh concurrency limiter closed"))
    }
}

fn backoff_delay(initial: Duration, attempt: usize) -> Duration {
    let multiplier = 1u32.checked_shl(attempt as u32).unwrap_or(u32::MAX);
    initial.saturating_mul(multiplier)
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .as_deref()
        .map(str::trim)
        .and_then(|value| value.parse().ok())
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .as_deref()
        .map(str::trim)
        .and_then(|value| value.parse().ok())
}

fn env_bool(name: &str) -> Option<bool> {
    std::env::var(name).ok().map(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

pub fn configured_hosts(ssh_config: Option<&Path>, configured_hosts: &[String]) -> Vec<String> {
    if !configured_hosts.is_empty() {
        return configured_hosts
            .iter()
            .filter(|h| is_safe_ssh_host(h))
            .cloned()
            .collect();
    }
    ssh_config
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|body| parse_ssh_hosts(&body))
        .unwrap_or_default()
}

/// Reject SSH host strings that could be interpreted as options or cause arg splitting.
pub fn is_safe_ssh_host(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with('-')
        && !host.chars().any(char::is_whitespace)
        && host.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '@' | '[' | ']')
        })
}

pub async fn run_ssh(
    ssh_config: Option<&Path>,
    host: &str,
    remote_command: &str,
    timeout: Duration,
) -> Result<CommandOutput> {
    SshContext::new(SshOptions::for_config(ssh_config))
        .run(host, remote_command, timeout)
        .await
}

pub async fn run_ssh_with_context(
    context: &SshContext,
    host: &str,
    remote_command: &str,
    timeout: Duration,
) -> Result<CommandOutput> {
    context.run(host, remote_command, timeout).await
}

#[cfg(test)]
fn ssh_args(ssh_config: Option<&Path>, host: &str, remote_command: &str) -> Result<Vec<String>> {
    SshOptions::for_config(ssh_config).ssh_args(host, remote_command)
}

pub fn ssh_config_buf(ssh_config: Option<&Path>) -> Option<PathBuf> {
    ssh_config.map(Path::to_path_buf)
}

fn parse_ssh_hosts(body: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        let Some(rest) = trimmed
            .strip_prefix("Host ")
            .or_else(|| trimmed.strip_prefix("host "))
        else {
            continue;
        };
        for host in rest.split_whitespace() {
            if host.contains('*')
                || host.contains('?')
                || host.eq_ignore_ascii_case("github.com")
                || !is_safe_ssh_host(host)
                || hosts.iter().any(|existing| existing == host)
            {
                continue;
            }
            hosts.push(host.to_string());
        }
    }
    hosts
}

#[cfg(test)]
#[path = "ssh_tests.rs"]
mod tests;
