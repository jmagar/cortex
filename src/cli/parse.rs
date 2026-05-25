use anyhow::{anyhow, bail, Result};

use super::parse_admin::{parse_compose, parse_db, parse_service, parse_setup, parse_stats};
use super::parse_ai::parse_ai;
use super::parse_command_log::{parse_agent_command, parse_shell};
use super::parse_logs::{
    parse_correlate, parse_errors, parse_filter, parse_hosts, parse_incident, parse_ingest_rate,
    parse_patterns, parse_search, parse_sessions, parse_source_ips, parse_tail, parse_timeline,
};
use super::{commands, parse_config, CliCommand};

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
        "correlate" => parse_correlate(rest),
        "stats" => parse_stats(rest),
        "compose" => parse_compose(rest),
        "service" => parse_service(rest),
        "setup" => parse_setup(rest),
        "db" => parse_db(rest),
        "config" => parse_config::parse_config(rest),
        "source-ips" => parse_source_ips(rest),
        "timeline" => parse_timeline(rest),
        "patterns" => parse_patterns(rest),
        "ingest-rate" => parse_ingest_rate(rest),
        "sig" => commands::sig::parse_sig(rest),
        "notify" => commands::notify::parse_notify(rest),
        // Surface parity gap closure (2026-05-22)
        "silent-hosts" => commands::silent_hosts::parse_silent_hosts(rest),
        "clock-skew" => commands::clock_skew::parse_clock_skew(rest),
        "anomalies" => commands::anomalies::parse_anomalies(rest),
        "compare" => commands::compare::parse_compare(rest),
        "apps" => commands::apps::parse_apps(rest),
        _ => bail!("unknown CLI command: {command}"),
    }
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
