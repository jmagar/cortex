use anyhow::{Result, anyhow, bail};

use super::parse_admin::{parse_compose, parse_db, parse_service, parse_setup, parse_stats};
use super::parse_ai::parse_ai;
use super::parse_command_log::{parse_agent_command, parse_shell};
use super::parse_logs::{
    parse_correlate, parse_errors, parse_filter, parse_hosts, parse_incident, parse_ingest_rate,
    parse_patterns, parse_search, parse_sessions, parse_source_ips, parse_tail, parse_timeline,
};
use super::{CliCommand, commands, parse_config, suggest};

const TOP_LEVEL_COMMANDS: &[&str] = &[
    "search",
    "filter",
    "tail",
    "errors",
    "hosts",
    "sessions",
    "incident",
    "ai",
    "shell",
    "agent-command",
    "heartbeat",
    "correlate",
    "stats",
    "compose",
    "service",
    "setup",
    "db",
    "config",
    "inventory",
    "source-ips",
    "timeline",
    "patterns",
    "ingest-rate",
    "sig",
    "notify",
    "silent-hosts",
    "clock-skew",
    "anomalies",
    "compare",
    "apps",
    "host-state",
    "fleet-state",
    "correlate-state",
    "file-tail",
];

pub(crate) fn parse_command(args: Vec<String>) -> Result<CliCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("CLI command is required"))?;
    match command.as_str() {
        "search" => parse_search(rest),
        "filter" => parse_filter(rest),
        "tail" => parse_tail(rest),
        "errors" => parse_errors(rest),
        "hosts" => parse_hosts(rest),
        "sessions" => parse_sessions(rest),
        "incident" => parse_incident(rest),
        "ai" => parse_ai(rest),
        "shell" => parse_shell(rest),
        "agent-command" => parse_agent_command(rest),
        "heartbeat" => parse_heartbeat(rest),
        "correlate" => parse_correlate(rest),
        "stats" => parse_stats(rest),
        "compose" => parse_compose(rest),
        "service" => parse_service(rest),
        "setup" => parse_setup(rest),
        "db" => parse_db(rest),
        "config" => parse_config::parse_config(rest),
        "inventory" => parse_inventory(rest),
        "source-ips" => parse_source_ips(rest),
        "timeline" => parse_timeline(rest),
        "patterns" => parse_patterns(rest),
        "ingest-rate" => parse_ingest_rate(rest),
        "entity" => commands::graph::parse_entity(rest),
        "graph" => commands::graph::parse_graph(rest),
        "sig" => commands::sig::parse_sig(rest),
        "notify" => commands::notify::parse_notify(rest),
        // Surface parity gap closure (2026-05-22)
        "silent-hosts" => commands::silent_hosts::parse_silent_hosts(rest),
        "clock-skew" => commands::clock_skew::parse_clock_skew(rest),
        "anomalies" => commands::anomalies::parse_anomalies(rest),
        "compare" => commands::compare::parse_compare(rest),
        "apps" => commands::apps::parse_apps(rest),
        // Heartbeat fleet state parity (cxih.4)
        "host-state" => commands::host_state::parse_host_state(rest),
        "fleet-state" => commands::fleet_state::parse_fleet_state(rest),
        "correlate-state" => commands::correlate_state::parse_correlate_state(rest),
        "file-tail" => commands::file_tails::parse_file_tail(rest),
        _ => bail!(
            "{}",
            suggest::unknown_command("CLI command", command, TOP_LEVEL_COMMANDS)
        ),
    }
}

fn parse_inventory(args: &[String]) -> Result<CliCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("inventory subcommand is required: refresh or status"))?;
    if matches!(command.as_str(), "--help" | "-h" | "help") {
        bail!("{}", inventory_usage());
    }
    let mut json = false;
    for arg in rest {
        match arg.as_str() {
            "--json" => json = true,
            "--help" | "-h" => bail!("{}", inventory_usage()),
            other => bail!(
                "{}",
                suggest::unknown_option("inventory", other, &["--json"])
            ),
        }
    }
    match command.as_str() {
        "refresh" => Ok(CliCommand::Inventory(super::InventoryCommand::Refresh(
            super::InventoryArgs { json },
        ))),
        "status" => Ok(CliCommand::Inventory(super::InventoryCommand::Status(
            super::InventoryArgs { json },
        ))),
        _ => bail!(
            "{}",
            suggest::unknown_command("inventory subcommand", command, &["refresh", "status"])
        ),
    }
}

fn inventory_usage() -> &'static str {
    "Usage: cortex inventory refresh [--json]\n       cortex inventory status [--json]"
}

fn parse_heartbeat(args: &[String]) -> Result<CliCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("heartbeat subcommand is required"))?;
    match command.as_str() {
        "agent" => parse_heartbeat_agent(rest),
        _ => bail!(
            "{}",
            suggest::unknown_command("heartbeat subcommand", command, &["agent"])
        ),
    }
}

fn parse_heartbeat_agent(args: &[String]) -> Result<CliCommand> {
    let mut out = super::HeartbeatAgentArgs {
        target: None,
        token: None,
        interval_secs: cortex::heartbeat_agent::DEFAULT_INTERVAL_SECS,
        probe_deadline_ms: cortex::heartbeat_agent::DEFAULT_PROBE_DEADLINE_MS,
        collection_deadline_ms: cortex::heartbeat_agent::DEFAULT_COLLECTION_DEADLINE_MS,
        retry_buffer: cortex::heartbeat_agent::DEFAULT_RETRY_BUFFER_LIMIT,
        once: false,
        emit: false,
        json: false,
        host_id_path: None,
        docker: false,
        docker_url: None,
        journald: false,
        syslog_target: None,
    };
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--target" => {
                i += 1;
                out.target = Some(required_value(args, i, "--target")?);
            }
            "--token" => {
                i += 1;
                out.token = Some(required_value(args, i, "--token")?);
            }
            "--interval-secs" => {
                i += 1;
                out.interval_secs = parse_u64_value(args, i, "--interval-secs")?;
            }
            "--probe-deadline-ms" => {
                i += 1;
                out.probe_deadline_ms = parse_u64_value(args, i, "--probe-deadline-ms")?;
            }
            "--collection-deadline-ms" => {
                i += 1;
                out.collection_deadline_ms = parse_u64_value(args, i, "--collection-deadline-ms")?;
            }
            "--retry-buffer" => {
                i += 1;
                out.retry_buffer = parse_usize_value(args, i, "--retry-buffer")?;
            }
            "--host-id-path" => {
                i += 1;
                out.host_id_path = Some(required_value(args, i, "--host-id-path")?);
            }
            "--docker-url" => {
                i += 1;
                out.docker_url = Some(required_value(args, i, "--docker-url")?);
            }
            "--syslog-target" => {
                i += 1;
                out.syslog_target = Some(required_value(args, i, "--syslog-target")?);
            }
            "--once" => out.once = true,
            "--emit" => out.emit = true,
            "--json" => out.json = true,
            "--docker" => out.docker = true,
            "--journald" => out.journald = true,
            other => bail!(
                "{}",
                suggest::unknown_option(
                    "heartbeat agent",
                    other,
                    &[
                        "--target",
                        "--token",
                        "--interval-secs",
                        "--probe-deadline-ms",
                        "--collection-deadline-ms",
                        "--retry-buffer",
                        "--host-id-path",
                        "--docker",
                        "--docker-url",
                        "--journald",
                        "--syslog-target",
                        "--once",
                        "--emit",
                        "--json",
                    ],
                )
            ),
        }
        i += 1;
    }
    if out.interval_secs == 0 {
        bail!("--interval-secs must be greater than zero");
    }
    if out.probe_deadline_ms == 0 {
        bail!("--probe-deadline-ms must be greater than zero");
    }
    if out.collection_deadline_ms == 0 {
        bail!("--collection-deadline-ms must be greater than zero");
    }
    Ok(CliCommand::Heartbeat(super::HeartbeatCommand::Agent(out)))
}

fn required_value(args: &[String], index: usize, flag: &str) -> Result<String> {
    let value = args
        .get(index)
        .ok_or_else(|| anyhow!("{flag} requires a value"))?;
    if value.starts_with('-') || value.trim().is_empty() {
        bail!("{flag} requires a value");
    }
    Ok(value.clone())
}

fn parse_u64_value(args: &[String], index: usize, flag: &str) -> Result<u64> {
    required_value(args, index, flag)?
        .parse()
        .map_err(|_| anyhow!("{flag} must be an integer"))
}

fn parse_usize_value(args: &[String], index: usize, flag: &str) -> Result<usize> {
    required_value(args, index, flag)?
        .parse()
        .map_err(|_| anyhow!("{flag} must be an integer"))
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
