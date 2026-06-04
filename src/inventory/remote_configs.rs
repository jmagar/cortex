use anyhow::Result;
use std::path::{Path, PathBuf};
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
    let ssh_config = ssh_config.map(Path::to_path_buf);
    let mut handles = Vec::new();
    for host in hosts {
        let paths = paths.clone();
        let run_id = run_id.to_string();
        let ssh_config = ssh_config.clone();
        handles.push(tokio::spawn(async move {
            collect_host(host, ssh_config, paths, run_id, timeout).await
        }));
    }

    let mut out = CollectorOutput::new("raw_configs");
    for handle in handles {
        match handle.await {
            Ok(host_output) => merge_output(&mut out, host_output),
            Err(error) => out.warn(
                "remote_config",
                format!("remote config task failed: {error}"),
            ),
        }
    }
    out
}

async fn collect_host(
    host: String,
    ssh_config: Option<PathBuf>,
    paths: InventoryPaths,
    run_id: String,
    timeout: Duration,
) -> CollectorOutput {
    let mut out = CollectorOutput::new("raw_configs");
    collect_compose(
        &mut out,
        &host,
        ssh_config.as_deref(),
        &paths,
        &run_id,
        timeout,
    )
    .await;
    collect_proxy(
        &mut out,
        &host,
        ssh_config.as_deref(),
        &paths,
        &run_id,
        timeout,
    )
    .await;
    out
}

async fn collect_compose(
    out: &mut CollectorOutput,
    host: &str,
    ssh_config: Option<&Path>,
    paths: &InventoryPaths,
    run_id: &str,
    timeout: Duration,
) {
    for (path, body) in
        remote_records(out, host, ssh_config, compose_batch_command(), timeout).await
    {
        match collect_compose_body(
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
        }
    }
}

async fn collect_proxy(
    out: &mut CollectorOutput,
    host: &str,
    ssh_config: Option<&Path>,
    paths: &InventoryPaths,
    run_id: &str,
    timeout: Duration,
) {
    for (path, body) in remote_records(out, host, ssh_config, proxy_batch_command(), timeout).await
    {
        match collect_proxy_body(
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
        }
    }
}

async fn remote_records(
    out: &mut CollectorOutput,
    host: &str,
    ssh_config: Option<&Path>,
    command: String,
    timeout: Duration,
) -> Vec<(String, String)> {
    match run_ssh(ssh_config, host, &command, timeout).await {
        Ok(output) if output.status == Some(0) => parse_records(&output.stdout),
        Ok(output) => {
            out.warn(
                "remote_config",
                format!("ssh config collection failed on {host}: {}", output.stderr),
            );
            Vec::new()
        }
        Err(error) => {
            out.warn(
                "remote_config",
                format!("ssh config collection failed on {host}: {error}"),
            );
            Vec::new()
        }
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

fn compose_batch_command() -> String {
    batch_command("for d in \"$HOME/compose\" \"$HOME/.cortex/compose\" \"$HOME/.axon/compose\" \"$HOME/workspace\" /mnt/appdata /mnt/cache/appdata /mnt/user/appdata /opt /srv; do [ -d \"$d\" ] && find \"$d\" -maxdepth 4 -type f \\( -name docker-compose.yml -o -name docker-compose.yaml -o -name compose.yml -o -name compose.yaml \\) -print 2>/dev/null; done | sort -u | head -200")
}

fn proxy_batch_command() -> String {
    batch_command("for d in /mnt/appdata/swag/nginx/proxy-confs /mnt/cache/appdata/swag/nginx/proxy-confs /mnt/user/appdata/swag/nginx/proxy-confs \"$HOME/swag/nginx/proxy-confs\" \"$HOME/compose/swag/nginx/proxy-confs\"; do [ -d \"$d\" ] && find \"$d\" -maxdepth 1 -type f -name '*.conf' -print 2>/dev/null; done | sort -u | head -300")
}

fn batch_command(find_command: &str) -> String {
    format!(
        "{find_command} | while IFS= read -r f; do [ -f \"$f\" ] || continue; printf '\\036%s\\n' \"$f\"; head -c {} -- \"$f\"; printf '\\n'; done",
        MAX_RAW_ARTIFACT_BYTES + 1
    )
}

fn parse_records(stdout: &str) -> Vec<(String, String)> {
    stdout
        .split('\u{1e}')
        .skip(1)
        .filter_map(|record| {
            let (path, body) = record.split_once('\n')?;
            Some((path.to_string(), body.trim_end_matches('\n').to_string()))
        })
        .collect()
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

fn merge_output(out: &mut CollectorOutput, remote: CollectorOutput) {
    out.compose_projects.extend(remote.compose_projects);
    out.reverse_proxies.extend(remote.reverse_proxies);
    out.artifacts.extend(remote.artifacts);
    out.errors.extend(remote.errors);
    out.warnings.extend(remote.warnings);
}

#[cfg(test)]
#[path = "remote_configs_tests.rs"]
mod tests;
