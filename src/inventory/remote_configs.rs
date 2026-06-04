use anyhow::Result;
use std::path::Path;
use std::time::Duration;

use crate::inventory::collectors::CollectorOutput;
use crate::inventory::limits::MAX_RAW_ARTIFACT_BYTES;
use crate::inventory::process::{run_command, CommandOutput};
use crate::inventory::raw_configs::{collect_compose_body, collect_proxy_body};
use crate::inventory::storage::InventoryPaths;

pub async fn collect(
    ssh_config: Option<&Path>,
    configured_hosts: &[String],
    paths: &InventoryPaths,
    run_id: &str,
    timeout: Duration,
) -> CollectorOutput {
    let hosts = if configured_hosts.is_empty() {
        ssh_config
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|body| parse_ssh_hosts(&body))
            .unwrap_or_default()
    } else {
        configured_hosts.to_vec()
    };
    let mut out = CollectorOutput::new("raw_configs");
    for host in hosts {
        collect_host(&mut out, &host, ssh_config, paths, run_id, timeout).await;
    }
    out
}

async fn collect_host(
    out: &mut CollectorOutput,
    host: &str,
    ssh_config: Option<&Path>,
    paths: &InventoryPaths,
    run_id: &str,
    timeout: Duration,
) {
    for path in remote_paths(
        out,
        host,
        ssh_config,
        remote_compose_find_command(),
        timeout,
    )
    .await
    {
        match read_remote_file(host, &path, ssh_config, timeout).await {
            Ok(body) => match collect_compose_body(
                Some(host.to_string()),
                format!("{host}:{path}"),
                body,
                paths,
                run_id,
            ) {
                Ok((artifact, project)) => {
                    out.artifacts.push(artifact);
                    out.compose_projects.push(project);
                }
                Err(error) => out.warn("remote_compose", error.to_string()),
            },
            Err(error) => out.warn("remote_compose", format!("{host}:{path}: {error}")),
        }
    }
    for path in remote_paths(out, host, ssh_config, remote_proxy_find_command(), timeout).await {
        match read_remote_file(host, &path, ssh_config, timeout).await {
            Ok(body) => match collect_proxy_body(
                Some(host.to_string()),
                format!("{host}:{path}"),
                body,
                paths,
                run_id,
            ) {
                Ok((artifact, routes)) => {
                    out.artifacts.push(artifact);
                    out.reverse_proxies.extend(routes);
                }
                Err(error) => out.warn("remote_proxy", error.to_string()),
            },
            Err(error) => out.warn("remote_proxy", format!("{host}:{path}: {error}")),
        }
    }
}

async fn remote_paths(
    out: &mut CollectorOutput,
    host: &str,
    ssh_config: Option<&Path>,
    command: String,
    timeout: Duration,
) -> Vec<String> {
    match run_ssh(ssh_config, host, &command, timeout).await {
        Ok(output) if output.status == Some(0) => output
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect(),
        Ok(output) => {
            out.warn(
                "remote_config",
                format!("ssh config discovery failed on {host}: {}", output.stderr),
            );
            Vec::new()
        }
        Err(error) => {
            out.warn(
                "remote_config",
                format!("ssh config discovery failed on {host}: {error}"),
            );
            Vec::new()
        }
    }
}

async fn read_remote_file(
    host: &str,
    path: &str,
    ssh_config: Option<&Path>,
    timeout: Duration,
) -> Result<String> {
    let command = format!(
        "head -c {} -- {}",
        MAX_RAW_ARTIFACT_BYTES + 1,
        shell_quote(path)
    );
    let output = run_ssh(ssh_config, host, &command, timeout).await?;
    if output.status == Some(0) {
        Ok(output.stdout)
    } else {
        Err(anyhow::anyhow!("{}", output.stderr))
    }
}

async fn run_ssh(
    ssh_config: Option<&Path>,
    host: &str,
    remote_command: &str,
    timeout: Duration,
) -> Result<CommandOutput> {
    let mut args = Vec::new();
    if let Some(config) = ssh_config {
        args.push("-F".to_string());
        args.push(config.display().to_string());
    }
    args.extend([
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "ConnectTimeout=4".to_string(),
        "-o".to_string(),
        "ServerAliveInterval=3".to_string(),
        "-o".to_string(),
        "ServerAliveCountMax=1".to_string(),
        host.to_string(),
        remote_command.to_string(),
    ]);
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_command("ssh", &refs, timeout).await
}

fn remote_compose_find_command() -> String {
    "for d in \"$HOME/compose\" \"$HOME/.cortex/compose\" \"$HOME/.axon/compose\" \"$HOME/workspace\" /mnt/appdata /mnt/cache/appdata /mnt/user/appdata /opt /srv; do [ -d \"$d\" ] && find \"$d\" -maxdepth 4 -type f \\( -name docker-compose.yml -o -name docker-compose.yaml -o -name compose.yml -o -name compose.yaml \\) -print 2>/dev/null; done | sort -u | head -200".to_string()
}

fn remote_proxy_find_command() -> String {
    "for d in /mnt/appdata/swag/nginx/proxy-confs /mnt/cache/appdata/swag/nginx/proxy-confs /mnt/user/appdata/swag/nginx/proxy-confs \"$HOME/swag/nginx/proxy-confs\" \"$HOME/compose/swag/nginx/proxy-confs\"; do [ -d \"$d\" ] && find \"$d\" -maxdepth 1 -type f -name '*.conf' -print 2>/dev/null; done | sort -u | head -300".to_string()
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
                || hosts.iter().any(|existing| existing == host)
            {
                continue;
            }
            hosts.push(host.to_string());
        }
    }
    hosts
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
#[path = "remote_configs_tests.rs"]
mod tests;
