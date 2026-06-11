use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;

const PROBE_TIMEOUT_SECS: u64 = 5;
const REMOTE_BIN: &str = ".local/bin/cortex";

// ── public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HostProbe {
    pub host: String,
    pub reachable: bool,
    pub cortex_version: Option<String>,
    /// `Some(true)` = active, `Some(false)` = installed but inactive, `None` = cortex absent
    pub agent_active: Option<bool>,
}

impl HostProbe {
    pub fn display_label(&self) -> String {
        let cortex = match &self.cortex_version {
            Some(v) => format!("cortex {v}"),
            None => "absent".to_string(),
        };
        let agent = match self.agent_active {
            Some(true) => "agent:active",
            Some(false) => "agent:inactive",
            None => "—",
        };
        let ok = if self.reachable { "✓" } else { "✗" };
        format!("{:<22} {ok}  {:<18}  {}", self.host, cortex, agent)
    }
}

#[derive(Debug, Clone, Default)]
pub struct AgentDeployConfig {
    pub target: Option<String>,
    pub token: Option<String>,
    pub docker: bool,
    pub journald: bool,
}

#[derive(Debug, Clone)]
pub struct DeployResult {
    pub host: String,
    pub ok: bool,
    pub detail: String,
    pub elapsed_ms: u128,
}

// ── discovery ────────────────────────────────────────────────────────────────

/// Parse `~/.ssh/config` and return all concrete host aliases (no wildcards).
pub fn ssh_config_hosts() -> Vec<String> {
    let path = home_dir()
        .map(|h| h.join(".ssh/config"))
        .unwrap_or_default();
    let body = std::fs::read_to_string(path).unwrap_or_default();
    parse_ssh_config_hosts(&body)
}

fn parse_ssh_config_hosts(body: &str) -> Vec<String> {
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
        for token in rest.split_whitespace() {
            if token.contains('*') || token.contains('?') {
                continue;
            }
            if token.eq_ignore_ascii_case("github.com") {
                continue;
            }
            if !crate::inventory::ssh::is_safe_ssh_host(token) {
                continue;
            }
            if !hosts.contains(&token.to_string()) {
                hosts.push(token.to_string());
            }
        }
    }
    hosts
}

/// Probe all hosts in parallel (SSH, `BatchMode=yes`, `ConnectTimeout` capped).
/// Hosts that don't respond within the deadline appear as unreachable.
pub fn probe_hosts(hosts: Vec<String>) -> Vec<HostProbe> {
    if hosts.is_empty() {
        return Vec::new();
    }
    let count = hosts.len();
    let (tx, rx) = mpsc::channel::<HostProbe>();
    for host in hosts {
        let tx = tx.clone();
        std::thread::spawn(move || {
            tx.send(probe_one(&host)).ok();
        });
    }
    drop(tx);

    let deadline = Instant::now() + Duration::from_secs(PROBE_TIMEOUT_SECS + 3);
    let mut results = Vec::with_capacity(count);
    for _ in 0..count {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(probe) => results.push(probe),
            Err(_) => break,
        }
    }
    results.sort_by(|a, b| a.host.cmp(&b.host));
    results
}

fn probe_one(host: &str) -> HostProbe {
    let script = "which cortex >/dev/null 2>&1 && cortex --version 2>/dev/null \
                  || echo 'cortex:absent'; \
                  systemctl --user is-active cortex-heartbeat-agent.service 2>/dev/null \
                  || echo 'inactive'";
    let out = Command::new("ssh")
        .args([
            "-o",
            &format!("ConnectTimeout={PROBE_TIMEOUT_SECS}"),
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "LogLevel=ERROR",
            host,
            script,
        ])
        .output();

    let Ok(out) = out else {
        return HostProbe {
            host: host.to_string(),
            reachable: false,
            cortex_version: None,
            agent_active: None,
        };
    };
    if !out.status.success() {
        return HostProbe {
            host: host.to_string(),
            reachable: false,
            cortex_version: None,
            agent_active: None,
        };
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut lines = stdout.lines();
    let first = lines.next().unwrap_or("cortex:absent").trim();
    let second = lines.next().unwrap_or("inactive").trim();

    let cortex_version = if first.contains("cortex:absent") {
        None
    } else {
        // `cortex --version` prints "cortex 1.17.0"
        first.split_whitespace().nth(1).map(str::to_string)
    };
    let agent_active = cortex_version.as_ref().map(|_| second == "active");

    HostProbe {
        host: host.to_string(),
        reachable: true,
        cortex_version,
        agent_active,
    }
}

// ── interactive selection ────────────────────────────────────────────────────

/// Show an `inquire::MultiSelect` over reachable hosts. Returns selected host
/// names. Unreachable hosts are excluded from the list but noted beforehand.
pub fn select_hosts_interactive(probes: &[HostProbe]) -> Result<Vec<String>> {
    let unreachable: Vec<&str> = probes
        .iter()
        .filter(|p| !p.reachable)
        .map(|p| p.host.as_str())
        .collect();
    if !unreachable.is_empty() {
        eprintln!("\n  unreachable (skipped): {}\n", unreachable.join(", "));
    }

    let reachable: Vec<&HostProbe> = probes.iter().filter(|p| p.reachable).collect();
    if reachable.is_empty() {
        anyhow::bail!("no reachable hosts found in ~/.ssh/config");
    }

    let labels: Vec<String> = reachable.iter().map(|p| p.display_label()).collect();

    let selected = inquire::MultiSelect::new(
        "Select hosts to deploy the cortex heartbeat agent:",
        labels.clone(),
    )
    .with_help_message("↑↓ move  space toggle  enter confirm  type to filter")
    .prompt()?;

    Ok(selected
        .into_iter()
        .filter_map(|label| {
            labels
                .iter()
                .position(|l| *l == label)
                .and_then(|i| reachable.get(i))
                .map(|p| p.host.clone())
        })
        .collect())
}

// ── deploy ───────────────────────────────────────────────────────────────────

/// Locate the best local cortex binary for deployment (prefer the installed
/// production binary over whatever is currently executing).
pub fn find_local_binary() -> Option<PathBuf> {
    which_cortex().or_else(|| std::env::current_exe().ok())
}

fn which_cortex() -> Option<PathBuf> {
    let out = Command::new("which").arg("cortex").output().ok()?;
    if out.status.success() {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    None
}

pub fn deploy_agent_to_host(
    host: &str,
    local_binary: &Path,
    config: &AgentDeployConfig,
) -> DeployResult {
    let started = Instant::now();
    match run_deploy(host, local_binary, config) {
        Ok(()) => DeployResult {
            host: host.to_string(),
            ok: true,
            detail: "installed and enabled".to_string(),
            elapsed_ms: started.elapsed().as_millis(),
        },
        Err(e) => DeployResult {
            host: host.to_string(),
            ok: false,
            detail: e.to_string(),
            elapsed_ms: started.elapsed().as_millis(),
        },
    }
}

fn run_deploy(host: &str, local_binary: &Path, config: &AgentDeployConfig) -> io::Result<()> {
    ssh_run(host, "mkdir -p ~/.local/bin")?;
    scp_file(local_binary, host, REMOTE_BIN)?;
    ssh_run(host, "chmod +x ~/.local/bin/cortex")?;

    let mut env_pairs: Vec<String> = Vec::new();
    if let Some(t) = &config.target {
        env_pairs.push(format!("CORTEX_HEARTBEAT_TARGET={}", shell_quote(t)));
    }
    if let Some(t) = &config.token {
        env_pairs.push(format!("CORTEX_TOKEN={}", shell_quote(t)));
    }
    if config.docker {
        env_pairs.push("CORTEX_AGENT_DOCKER=true".to_string());
    }
    if config.journald {
        env_pairs.push("CORTEX_AGENT_JOURNALD=true".to_string());
    }
    let prefix = if env_pairs.is_empty() {
        String::new()
    } else {
        format!("{} ", env_pairs.join(" "))
    };
    ssh_run(
        host,
        &format!("{prefix}~/.local/bin/cortex setup heartbeat-agent install"),
    )
}

fn ssh_run(host: &str, cmd: &str) -> io::Result<()> {
    let status = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "LogLevel=ERROR",
            host,
            cmd,
        ])
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "ssh {host}: '{cmd}' exited non-zero"
        )));
    }
    Ok(())
}

fn scp_file(local: &Path, host: &str, remote_path: &str) -> io::Result<()> {
    let dest = format!("{host}:{remote_path}");
    let status = Command::new("scp")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
        ])
        .arg(local)
        .arg(&dest)
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "scp {} → {dest} failed",
            local.display()
        )));
    }
    Ok(())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
#[path = "agent_deploy_tests.rs"]
mod tests;
