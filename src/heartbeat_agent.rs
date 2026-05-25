use std::collections::VecDeque;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use getrandom::fill as random_fill;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

pub const DEFAULT_INTERVAL_SECS: u64 = 30;
pub const DEFAULT_PROBE_DEADLINE_MS: u64 = 2_000;
pub const DEFAULT_COLLECTION_DEADLINE_MS: u64 = 5_000;
pub const DEFAULT_RETRY_BUFFER_LIMIT: usize = 32;
pub const DEFAULT_TARGET: &str = "http://127.0.0.1:3100";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatAgentConfig {
    pub target: Option<String>,
    pub token: Option<String>,
    pub interval: Duration,
    pub probe_deadline: Duration,
    pub collection_deadline: Duration,
    pub retry_buffer_limit: usize,
    pub once: bool,
    pub emit: bool,
    pub json: bool,
    pub host_id_path: PathBuf,
}

impl HeartbeatAgentConfig {
    pub fn from_env(host_id_path: PathBuf) -> Self {
        let target = std::env::var("SYSLOG_HEARTBEAT_TARGET")
            .ok()
            .or_else(|| std::env::var("SYSLOG_MCP_URL").ok())
            .or_else(|| Some(DEFAULT_TARGET.to_string()));
        let token = std::env::var("SYSLOG_HEARTBEAT_TOKEN")
            .ok()
            .or_else(|| std::env::var("SYSLOG_MCP_TOKEN").ok());
        Self {
            target,
            token,
            interval: Duration::from_secs(DEFAULT_INTERVAL_SECS),
            probe_deadline: Duration::from_millis(DEFAULT_PROBE_DEADLINE_MS),
            collection_deadline: Duration::from_millis(DEFAULT_COLLECTION_DEADLINE_MS),
            retry_buffer_limit: DEFAULT_RETRY_BUFFER_LIMIT,
            once: false,
            emit: false,
            json: false,
            host_id_path,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HeartbeatPayload {
    pub schema_version: u8,
    pub host: HeartbeatHost,
    pub sample: HeartbeatSample,
    pub agent: HeartbeatAgentInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<HeartbeatCpu>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<HeartbeatMemory>,
    pub disks: Vec<HeartbeatDisk>,
    pub networks: Vec<HeartbeatNetwork>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processes: Option<HeartbeatProcesses>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub containers: Option<HeartbeatContainers>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HeartbeatHost {
    pub host_id: String,
    pub hostname: String,
    pub os: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
    pub architecture: String,
    pub boot_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HeartbeatSample {
    pub sequence: i64,
    pub sampled_at: String,
    pub uptime_secs: i64,
    pub monotonic_ms: i64,
    pub collection_ms: i64,
    pub partial: bool,
    pub probe_errors: Vec<String>,
    pub skipped_probes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HeartbeatAgentInfo {
    pub version: String,
    pub mode: String,
    pub interval_secs: i64,
    pub push_latency_ms: Option<i64>,
    pub retry_backlog: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HeartbeatCpu {
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub usage_pct: Option<f64>,
    pub user_pct: Option<f64>,
    pub system_pct: Option<f64>,
    pub iowait_pct: Option<f64>,
    pub steal_pct: Option<f64>,
    pub core_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HeartbeatMemory {
    pub mem_total_bytes: i64,
    pub mem_available_bytes: i64,
    pub mem_used_bytes: Option<i64>,
    pub swap_total_bytes: i64,
    pub swap_used_bytes: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HeartbeatDisk {
    pub kind: String,
    pub name: String,
    pub fs_type: Option<String>,
    pub bytes_total: Option<i64>,
    pub bytes_free: Option<i64>,
    pub bytes_used: Option<i64>,
    pub read_bytes_per_sec: Option<f64>,
    pub write_bytes_per_sec: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HeartbeatNetwork {
    pub interface: String,
    pub rx_bytes_per_sec: Option<f64>,
    pub tx_bytes_per_sec: Option<f64>,
    pub rx_errors_per_sec: Option<f64>,
    pub tx_errors_per_sec: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HeartbeatProcesses {
    pub total: i64,
    pub running: Option<i64>,
    pub sleeping: Option<i64>,
    pub zombies: i64,
    pub top: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HeartbeatContainers {
    pub runtime: Option<String>,
    pub reachable: bool,
    pub running: i64,
    pub exited: i64,
    pub restarting: i64,
    pub unhealthy: i64,
    pub details: Vec<serde_json::Value>,
}

pub enum ProbeOutput {
    Cpu(HeartbeatCpu),
    Memory(HeartbeatMemory),
    Disk(HeartbeatDisk),
    Network(HeartbeatNetwork),
    Processes(HeartbeatProcesses),
    Containers(HeartbeatContainers),
}

pub trait HeartbeatProbe: Send + Sync {
    fn name(&self) -> &'static str;
    fn collect(&self) -> Pin<Box<dyn Future<Output = Result<ProbeOutput>> + Send + '_>>;
}

pub struct HeartbeatCollector {
    probes: Vec<Box<dyn HeartbeatProbe>>,
    started: Instant,
}

impl HeartbeatCollector {
    pub fn fake() -> Self {
        Self {
            probes: vec![
                Box::new(FakeProbe::cpu()),
                Box::new(FakeProbe::memory()),
                Box::new(FakeProbe::disk()),
                Box::new(FakeProbe::network()),
                Box::new(FakeProbe::processes()),
                Box::new(FakeProbe::containers()),
            ],
            started: Instant::now(),
        }
    }

    pub fn with_probes(probes: Vec<Box<dyn HeartbeatProbe>>) -> Self {
        Self {
            probes,
            started: Instant::now(),
        }
    }

    pub async fn collect(
        &self,
        host_id: String,
        sequence: i64,
        interval: Duration,
        retry_backlog: usize,
        probe_deadline: Duration,
        collection_deadline: Duration,
    ) -> HeartbeatPayload {
        let started = Instant::now();
        let mut cpu = None;
        let mut memory = None;
        let mut disks = Vec::new();
        let mut networks = Vec::new();
        let mut processes = None;
        let mut containers = None;
        let mut probe_errors = Vec::new();
        let mut skipped_probes = Vec::new();

        for probe in &self.probes {
            let elapsed = started.elapsed();
            if elapsed >= collection_deadline {
                skipped_probes.push(probe.name().to_string());
                continue;
            }
            let remaining = collection_deadline.saturating_sub(elapsed);
            let deadline = probe_deadline.min(remaining);
            match timeout(deadline, probe.collect()).await {
                Ok(Ok(output)) => match output {
                    ProbeOutput::Cpu(value) => cpu = Some(value),
                    ProbeOutput::Memory(value) => memory = Some(value),
                    ProbeOutput::Disk(value) if disks.len() < 16 => disks.push(value),
                    ProbeOutput::Network(value) if networks.len() < 16 => networks.push(value),
                    ProbeOutput::Processes(value) => processes = Some(value),
                    ProbeOutput::Containers(value) => containers = Some(value),
                    ProbeOutput::Disk(_) => skipped_probes.push("disk_limit".to_string()),
                    ProbeOutput::Network(_) => skipped_probes.push("network_limit".to_string()),
                },
                Ok(Err(error)) => probe_errors.push(bounded_probe_error(probe.name(), &error)),
                Err(_) => {
                    skipped_probes.push(probe.name().to_string());
                    probe_errors.push(format!("{} timed out", probe.name()));
                }
            }
        }

        let partial = !probe_errors.is_empty() || !skipped_probes.is_empty();
        HeartbeatPayload {
            schema_version: 1,
            host: HeartbeatHost {
                host_id,
                hostname: hostname(),
                os: std::env::consts::OS.to_string(),
                kernel: kernel_release(),
                architecture: std::env::consts::ARCH.to_string(),
                boot_id: boot_id(),
                timezone: std::env::var("TZ").ok(),
            },
            sample: HeartbeatSample {
                sequence,
                sampled_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                uptime_secs: self.started.elapsed().as_secs() as i64,
                monotonic_ms: self.started.elapsed().as_millis() as i64,
                collection_ms: started.elapsed().as_millis() as i64,
                partial,
                probe_errors,
                skipped_probes,
            },
            agent: HeartbeatAgentInfo {
                version: env!("CARGO_PKG_VERSION").to_string(),
                mode: "always_on".to_string(),
                interval_secs: interval.as_secs() as i64,
                push_latency_ms: None,
                retry_backlog: retry_backlog as i64,
            },
            cpu,
            memory,
            disks,
            networks,
            processes,
            containers,
        }
    }
}

#[derive(Clone)]
pub struct FakeProbe {
    name: &'static str,
    output: fn() -> ProbeOutput,
    delay: Duration,
    fail: bool,
}

impl FakeProbe {
    pub fn cpu() -> Self {
        Self::new("cpu", || {
            ProbeOutput::Cpu(HeartbeatCpu {
                load1: 0.10,
                load5: 0.12,
                load15: 0.08,
                usage_pct: Some(4.2),
                user_pct: Some(2.1),
                system_pct: Some(1.4),
                iowait_pct: Some(0.1),
                steal_pct: Some(0.0),
                core_count: 4,
            })
        })
    }

    pub fn memory() -> Self {
        Self::new("memory", || {
            ProbeOutput::Memory(HeartbeatMemory {
                mem_total_bytes: 8 * 1024 * 1024 * 1024,
                mem_available_bytes: 5 * 1024 * 1024 * 1024,
                mem_used_bytes: Some(3 * 1024 * 1024 * 1024),
                swap_total_bytes: 0,
                swap_used_bytes: 0,
            })
        })
    }

    pub fn disk() -> Self {
        Self::new("disk", || {
            ProbeOutput::Disk(HeartbeatDisk {
                kind: "mount".to_string(),
                name: "/".to_string(),
                fs_type: Some("stubfs".to_string()),
                bytes_total: Some(100 * 1024 * 1024 * 1024),
                bytes_free: Some(40 * 1024 * 1024 * 1024),
                bytes_used: Some(60 * 1024 * 1024 * 1024),
                read_bytes_per_sec: Some(0.0),
                write_bytes_per_sec: Some(0.0),
            })
        })
    }

    pub fn network() -> Self {
        Self::new("network", || {
            ProbeOutput::Network(HeartbeatNetwork {
                interface: "stub0".to_string(),
                rx_bytes_per_sec: Some(0.0),
                tx_bytes_per_sec: Some(0.0),
                rx_errors_per_sec: Some(0.0),
                tx_errors_per_sec: Some(0.0),
            })
        })
    }

    pub fn processes() -> Self {
        Self::new("processes", || {
            ProbeOutput::Processes(HeartbeatProcesses {
                total: 1,
                running: Some(1),
                sleeping: Some(0),
                zombies: 0,
                top: Vec::new(),
            })
        })
    }

    pub fn containers() -> Self {
        Self::new("containers", || {
            ProbeOutput::Containers(HeartbeatContainers {
                runtime: Some("docker".to_string()),
                reachable: false,
                running: 0,
                exited: 0,
                restarting: 0,
                unhealthy: 0,
                details: Vec::new(),
            })
        })
    }

    pub fn failing(name: &'static str) -> Self {
        Self {
            name,
            output: || {
                ProbeOutput::Processes(HeartbeatProcesses {
                    total: 0,
                    running: None,
                    sleeping: None,
                    zombies: 0,
                    top: Vec::new(),
                })
            },
            delay: Duration::ZERO,
            fail: true,
        }
    }

    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    fn new(name: &'static str, output: fn() -> ProbeOutput) -> Self {
        Self {
            name,
            output,
            delay: Duration::ZERO,
            fail: false,
        }
    }
}

impl HeartbeatProbe for FakeProbe {
    fn name(&self) -> &'static str {
        self.name
    }

    fn collect(&self) -> Pin<Box<dyn Future<Output = Result<ProbeOutput>> + Send + '_>> {
        Box::pin(async move {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            if self.fail {
                bail!("fake probe failure");
            }
            Ok((self.output)())
        })
    }
}

pub struct RetryBuffer {
    limit: usize,
    queue: VecDeque<HeartbeatPayload>,
}

impl RetryBuffer {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            queue: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn push(&mut self, payload: HeartbeatPayload) {
        if self.limit == 0 {
            return;
        }
        while self.queue.len() >= self.limit {
            self.queue.pop_front();
        }
        self.queue.push_back(payload);
    }

    fn pop_front(&mut self) -> Option<HeartbeatPayload> {
        self.queue.pop_front()
    }
}

pub fn backoff_duration(attempt: u32) -> Duration {
    let millis = 250u64.saturating_mul(1u64 << attempt.min(4));
    Duration::from_millis(millis.min(4_000))
}

pub async fn run_agent(config: HeartbeatAgentConfig) -> Result<()> {
    let host_id = load_or_create_host_id(&config.host_id_path)?;
    let collector = HeartbeatCollector::fake();
    let client = reqwest::Client::new();
    let mut retry = RetryBuffer::new(config.retry_buffer_limit);
    let mut sequence = 1i64;
    let mut attempt = 0u32;

    loop {
        let mut payload = collector
            .collect(
                host_id.clone(),
                sequence,
                config.interval,
                retry.len(),
                config.probe_deadline,
                config.collection_deadline,
            )
            .await;

        if config.emit {
            println!("{}", serde_json::to_string_pretty(&payload)?);
            return Ok(());
        }

        let target = config.target.as_deref().ok_or_else(|| {
            anyhow!("heartbeat agent requires --target or SYSLOG_HEARTBEAT_TARGET")
        })?;

        flush_retry_buffer(&client, &mut retry, target, config.token.as_deref()).await;
        payload.agent.retry_backlog = retry.len() as i64;
        match send_payload(&client, target, config.token.as_deref(), &mut payload).await {
            Ok(()) => attempt = 0,
            Err(error) => {
                tracing::warn!(error = %error, "heartbeat push failed; queued for retry");
                retry.push(payload);
                if config.once {
                    return Err(error);
                }
                tokio::time::sleep(backoff_duration(attempt)).await;
                attempt = attempt.saturating_add(1);
            }
        }

        if config.once {
            return Ok(());
        }

        sequence += 1;
        tokio::time::sleep(config.interval).await;
    }
}

async fn flush_retry_buffer(
    client: &reqwest::Client,
    retry: &mut RetryBuffer,
    target: &str,
    token: Option<&str>,
) {
    let initial = retry.len();
    for _ in 0..initial {
        let Some(mut payload) = retry.pop_front() else {
            return;
        };
        payload.agent.retry_backlog = retry.len() as i64;
        if let Err(error) = send_payload(client, target, token, &mut payload).await {
            tracing::warn!(error = %error, "heartbeat retry failed");
            retry.push(payload);
            return;
        }
    }
}

async fn send_payload(
    client: &reqwest::Client,
    target: &str,
    token: Option<&str>,
    payload: &mut HeartbeatPayload,
) -> Result<()> {
    let url = heartbeat_url(target)?;
    let started = Instant::now();
    let mut request = client.post(url).json(payload);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.context("heartbeat POST failed")?;
    payload.agent.push_latency_ms = Some(started.elapsed().as_millis() as i64);
    if response.status().as_u16() == 202 {
        return Ok(());
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    bail!("heartbeat POST returned {status}: {body}");
}

fn heartbeat_url(target: &str) -> Result<String> {
    let trimmed = target.trim_end_matches('/');
    if trimmed.ends_with("/v1/heartbeats") {
        return Ok(trimmed.to_string());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Ok(format!("{trimmed}/v1/heartbeats"));
    }
    bail!("heartbeat target must start with http:// or https://");
}

pub fn load_or_create_host_id(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(existing) => {
            let host_id = existing.trim();
            validate_host_id(host_id)?;
            Ok(host_id.to_string())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let host_id = generate_host_id()?;
            write_private(path, &format!("{host_id}\n"))?;
            Ok(host_id)
        }
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn validate_host_id(host_id: &str) -> Result<()> {
    if host_id.len() < 16
        || !host_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("invalid heartbeat host_id in config");
    }
    Ok(())
}

fn generate_host_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    random_fill(&mut bytes).context("generate heartbeat host id")?;
    Ok(format!("syslog_{}", hex_bytes(&bytes)))
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn write_private(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn bounded_probe_error(name: &str, error: &anyhow::Error) -> String {
    let mut text = format!("{name}: {error}");
    text.truncate(240);
    text
}

fn hostname() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string())
}

fn kernel_release() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn boot_id() -> String {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("process-{}", std::process::id()))
}

#[cfg(test)]
#[path = "heartbeat_agent_tests.rs"]
mod tests;
