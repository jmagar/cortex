use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::inventory::process::{run_command, CommandOutput};

const SSH_IGNORE_UNKNOWN_OPTIONS: &str = "IgnoreUnknown=WarnWeakCrypto";

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
fn is_safe_ssh_host(host: &str) -> bool {
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
    let args = ssh_args(ssh_config, host, remote_command);
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_command("ssh", &refs, timeout).await
}

fn ssh_args(ssh_config: Option<&Path>, host: &str, remote_command: &str) -> Vec<String> {
    let mut args = Vec::new();
    args.push("-o".to_string());
    args.push(SSH_IGNORE_UNKNOWN_OPTIONS.to_string());
    if let Some(config) = ssh_config {
        args.push("-F".to_string());
        args.push(config.display().to_string());
    }
    args.extend([
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        "ConnectTimeout=4".to_string(),
        "-o".to_string(),
        "ServerAliveInterval=3".to_string(),
        "-o".to_string(),
        "ServerAliveCountMax=1".to_string(),
        "--".to_string(),
        host.to_string(),
        remote_command.to_string(),
    ]);
    args
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
