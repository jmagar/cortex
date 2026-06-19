use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;

const PROBE_TIMEOUT_SECS: u64 = 5;
const REMOTE_BIN_TMP: &str = ".local/bin/cortex.new";

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

/// Show a simple stdin prompt over reachable hosts. Returns selected host names.
/// Unreachable hosts are excluded from the list but noted beforehand.
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

    eprintln!("Select hosts to deploy the cortex heartbeat agent:");
    for (idx, probe) in reachable.iter().enumerate() {
        eprintln!("  {:>2}. {}", idx + 1, probe.display_label());
    }
    eprint!("Enter numbers separated by commas/spaces, or 'all': ");
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let selected_indexes = parse_host_selection(&input, reachable.len())?;

    Ok(selected_indexes
        .into_iter()
        .filter_map(|idx| reachable.get(idx))
        .map(|probe| probe.host.clone())
        .collect())
}

fn parse_host_selection(input: &str, reachable_count: usize) -> Result<Vec<usize>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("no hosts selected");
    }
    if trimmed.eq_ignore_ascii_case("all") {
        return Ok((0..reachable_count).collect());
    }

    let mut selected = BTreeSet::new();
    for token in trimmed.split(|c: char| c == ',' || c.is_ascii_whitespace()) {
        if token.is_empty() {
            continue;
        }
        let number: usize = token
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid host selection: {token}"))?;
        if number == 0 || number > reachable_count {
            anyhow::bail!("host selection {number} is out of range 1..={reachable_count}");
        }
        selected.insert(number - 1);
    }

    if selected.is_empty() {
        anyhow::bail!("no hosts selected");
    }
    Ok(selected.into_iter().collect())
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

fn is_unraid(host: &str) -> bool {
    let out = Command::new("ssh")
        .args([
            "-o",
            &format!("ConnectTimeout={PROBE_TIMEOUT_SECS}"),
            "-o",
            "BatchMode=yes",
            "-o",
            "LogLevel=ERROR",
            host,
            "test -f /etc/unraid-version && echo yes || echo no",
        ])
        .output();
    matches!(out, Ok(o) if String::from_utf8_lossy(&o.stdout).trim() == "yes")
}

fn run_deploy(host: &str, local_binary: &Path, config: &AgentDeployConfig) -> io::Result<()> {
    if is_unraid(host) {
        return run_deploy_unraid(host, local_binary, config);
    }

    ssh_run(host, "mkdir -p ~/.local/bin")?;
    // scp to a temp path then mv atomically — avoids ETXTBSY if the binary is
    // currently running as a service on the remote host.
    scp_file(local_binary, host, REMOTE_BIN_TMP)?;
    ssh_run(
        host,
        "chmod +x ~/.local/bin/cortex.new && mv -f ~/.local/bin/cortex.new ~/.local/bin/cortex",
    )?;

    let mut env_pairs: Vec<String> = Vec::new();
    if let Some(t) = &config.target {
        env_pairs.push(format!("CORTEX_HEARTBEAT_TARGET={}", shell_quote(t)));
    }
    if let Some(t) = &config.token {
        env_pairs.push(format!("CORTEX_HEARTBEAT_TOKEN={}", shell_quote(t)));
    }
    if config.docker {
        env_pairs.push("CORTEX_AGENT_DOCKER=true".to_string());
    }
    if config.journald {
        env_pairs.push("CORTEX_AGENT_JOURNALD=true".to_string());
    }
    if let Some(syslog_target) = deploy_syslog_target(config.target.as_deref()) {
        env_pairs.push(format!(
            "CORTEX_SYSLOG_TARGET={}",
            shell_quote(&syslog_target)
        ));
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

// Unraid: root fs is a RAM disk — nothing in /root survives reboot.
// Put everything in /mnt/user/appdata/cortex (array, persistent), then
// use `docker run --restart unless-stopped` so Docker itself persists the
// container definition across reboots without any file on disk.
const UNRAID_APPDATA: &str = "/mnt/user/appdata/cortex";
const UNRAID_ENV: &str = "/mnt/user/appdata/cortex/heartbeat-agent.env";
/// Published agent image. The agent runs the same baked binary as the server
/// (no bind-mounted host binary), pinned to this deploying binary's version so
/// server and agents stay in lockstep. Requires the matching tag to be present
/// on the registry before deploy.
const CORTEX_IMAGE_REPO: &str = "ghcr.io/jmagar/cortex";
const UNRAID_HOST_ID: &str = "/mnt/user/appdata/cortex/heartbeat-host-id";
const UNRAID_CONTAINER: &str = "cortex-heartbeat-agent";
const UNRAID_HOST_SYSLOG: &str = "/var/log/syslog";
const UNRAID_CONTAINER_SYSLOG: &str = "/host/var/log/syslog";

fn run_deploy_unraid(
    host: &str,
    _local_binary: &Path,
    config: &AgentDeployConfig,
) -> io::Result<()> {
    // Containerized agents run the published image with the binary baked in —
    // no host binary is staged or bind-mounted. Pin to this deploying binary's
    // version so server and agents stay in lockstep (the agent can still self-
    // update between republishes, but the image is the source of truth).
    let image = format!("{CORTEX_IMAGE_REPO}:{}", env!("CARGO_PKG_VERSION"));
    ssh_run(host, &format!("mkdir -p {UNRAID_APPDATA}"))?;
    ssh_run(host, &format!("docker pull {image}"))?;

    // Build env key=value pairs for both the persistent env file and the -e flags
    // passed directly to docker run. We use -e flags (not --env-file) to avoid
    // any shell-quoting or whitespace ambiguity that arises when writing values
    // via `echo` over SSH and then reading them back with Docker's env-file parser.
    let target = config
        .target
        .as_deref()
        .unwrap_or(crate::heartbeat_agent::DEFAULT_TARGET);
    let mut env_pairs: Vec<(String, String)> = vec![
        ("CORTEX_HEARTBEAT_TARGET".into(), target.to_string()),
        ("RUST_LOG".into(), "warn".into()),
        (
            "CORTEX_SYSLOG_TARGET".into(),
            deploy_syslog_target(Some(target)).unwrap_or_else(|| "127.0.0.1:1514".into()),
        ),
        (
            "CORTEX_AGENT_DOCKER".into(),
            if config.docker { "true" } else { "false" }.into(),
        ),
        (
            "CORTEX_AGENT_DOCKER_URL".into(),
            crate::heartbeat_agent::DEFAULT_DOCKER_URL.into(),
        ),
        // journald has no meaning inside a container — suppress it for Unraid.
        ("CORTEX_AGENT_JOURNALD".into(), "false".into()),
        (
            "CORTEX_AGENT_SYSLOG_FILE".into(),
            UNRAID_CONTAINER_SYSLOG.into(),
        ),
    ];
    if let Some(t) = &config.token {
        env_pairs.push(("CORTEX_HEARTBEAT_TOKEN".into(), t.clone()));
    }

    // Write a persistent env file for reference / manual restarts, then chmod 600.
    let mut write_cmd = format!("rm -f {UNRAID_ENV}");
    for (k, v) in &env_pairs {
        write_cmd.push_str(&format!(
            " && echo {}={} >> {UNRAID_ENV}",
            k,
            shell_quote(v)
        ));
    }
    write_cmd.push_str(&format!(" && chmod 600 {UNRAID_ENV}"));
    ssh_run(host, &write_cmd)?;

    // Build explicit -e KEY=VALUE flags for docker run so values are passed
    // verbatim without any env-file parsing.
    let e_flags: String = env_pairs
        .iter()
        .map(|(k, v)| format!("-e {}={} ", k, shell_quote(v)))
        .collect();

    // Remove any previous container then start fresh with docker run.
    // --restart unless-stopped is stored in Docker's state (not a file),
    // so it survives the Unraid RAM-disk wipe on reboot.
    //
    // The image bakes in the binary and ca-certificates, so the only mounts are
    // host *data* the agent reads/writes: the Docker socket, the host syslog
    // file, and the appdata dir (host-id + reference env file). --user 0:0
    // overrides the image's unprivileged server user because the agent must read
    // root-owned host files (docker.sock, /var/log/syslog). --no-healthcheck
    // disables the image's server health probe (the agent runs no HTTP server).
    ssh_run(
        host,
        &format!(
            "docker rm -f {UNRAID_CONTAINER} 2>/dev/null; \
             docker run -d \
               --name {UNRAID_CONTAINER} \
               --restart unless-stopped \
               --network host \
               --user 0:0 \
               --no-healthcheck \
               {e_flags}\
               -v {UNRAID_APPDATA}:{UNRAID_APPDATA} \
               -v /var/run/docker.sock:/var/run/docker.sock \
               -v {UNRAID_HOST_SYSLOG}:{UNRAID_CONTAINER_SYSLOG}:ro \
               {image} \
               cortex heartbeat agent \
                 --host-id-path {UNRAID_HOST_ID}"
        ),
    )
}

fn deploy_syslog_target(heartbeat_target: Option<&str>) -> Option<String> {
    std::env::var("CORTEX_SYSLOG_TARGET")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            heartbeat_target
                .and_then(crate::agent::AgentStreamsConfig::syslog_target_from_heartbeat)
        })
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
