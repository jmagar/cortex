use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use syslog_mcp::app::{
    AbuseSearchResponse, AiCorrelateResponse, AiIncidentResponse, AiInvestigateResponse,
    AskHistoryResponse, CorrelateEventsResponse, DbBackupResult, DbCheckpointResult,
    DbIntegrityResult, DbMaintenanceStatus, DbStats, DbVacuumResult, GetErrorsResponse,
    IncidentContextResponse, IncidentResponse, ListAiProjectsResponse, ListAiToolsResponse,
    ListHostsResponse, LogEntry, ProjectContextResponse, SearchLogsResponse,
    SearchSessionsResponse, ServiceLogsRequest, ServiceLogsResponse, SimilarIncidentsResponse,
    SyslogService, UsageBlocksResponse,
};
use syslog_mcp::compose::{
    CliDockerInspect, CommandOutput, ComposeCommandResult, ComposeDefaults, ComposeMutation,
    ComposeService, ComposeStatus, ComposeTarget, ProcessRunner,
};
use syslog_mcp::scanner::{
    AiDoctorReport, AiIndexingHealth, CheckpointEntry, IndexResult, ParseErrorEntry,
    PruneCheckpointsResult,
};

mod args;
mod args_config;
pub(crate) use args::*;
pub(crate) use args_config::*;

mod commands;

mod run;
#[allow(unused_imports)]
pub(crate) use run::ENV_USE_HTTP;
pub(crate) use run::{run, CliMode, GlobalFlags};

mod ai_watch;
mod config_cmd;
mod config_toml;
mod coordination;
mod output_ai;
mod output_ai_more;
mod output_common;
mod output_logs;
mod output_ops;
mod parse;
mod parse_admin;
mod parse_ai;
mod parse_ai_more;
mod parse_common;
mod parse_config;
mod parse_logs;
mod setup;

pub(crate) use ai_watch::*;
pub(crate) use config_cmd::run_config;
pub(crate) use config_toml::*;
pub(crate) use coordination::*;
pub(crate) use output_ai::*;
pub(crate) use output_ai_more::*;
pub(crate) use output_common::*;
pub(crate) use output_logs::*;
pub(crate) use output_ops::*;
pub(crate) use parse_common::{parse_i64_flag, parse_u32_flag, FlagCursor};
pub(crate) use setup::*;

impl CliCommand {
    pub(crate) fn parse(args: Vec<String>) -> Result<Self> {
        parse::parse_command(args)
    }
}

pub(crate) fn run_compose(command: CliCommand) -> Result<()> {
    let CliCommand::Compose(command) = command else {
        bail!("run_compose called with non-compose command");
    };
    let service = ComposeService::new(CliDockerInspect, ProcessRunner, ComposeDefaults::default());
    match command {
        ComposeCommand::Status(args) => {
            let status = service.status(&args.target)?;
            print_compose_status_response(&status, args.json)
        }
        ComposeCommand::Doctor(args) => {
            let status = service.status(&args.target)?;
            let coordination = run_coordination_phases();
            print_compose_doctor_response(&status, &coordination, args.json)?;
            ensure_doctor_coordination_ok(&coordination)?;
            syslog_mcp::compose::ensure_doctor_ready(&status)
        }
        ComposeCommand::Up(args) => print_compose_command_response(
            &service.run_mutation(ComposeMutation::Up, &args.target, &args.options)?,
            args.json,
        ),
        ComposeCommand::Down(args) => print_compose_command_response(
            &service.run_mutation(ComposeMutation::Down, &args.target, &args.options)?,
            args.json,
        ),
        ComposeCommand::Restart(args) => print_compose_command_response(
            &service.run_mutation(ComposeMutation::Restart, &args.target, &args.options)?,
            args.json,
        ),
        ComposeCommand::Pull(args) => print_compose_command_response(
            &service.run_mutation(ComposeMutation::Pull, &args.target, &args.options)?,
            args.json,
        ),
        ComposeCommand::Logs(args) => {
            let output = service.logs(&args.target, args.tail)?;
            if args.json {
                print_json(&output)?;
            } else {
                print!("{}", output.stdout);
                eprint!("{}", output.stderr);
            }
            ensure_command_success(&output)
        }
    }
}

/// DB-free entry point for `syslog service ...` — avoids opening the SQLite
/// pool so this command remains usable when the DB is corrupted/locked/full.
pub(crate) async fn run_service_no_db(command: CliCommand) -> Result<()> {
    let CliCommand::Service(command) = command else {
        bail!("internal: run_service_no_db called with non-service command");
    };
    match command {
        ServiceCommand::Logs(args) => {
            let json = args.json;
            let report = syslog_mcp::app::run_service_logs(
                ServiceLogsRequest {
                    service: args.service,
                    from: args.from,
                    to: args.to,
                    tail: args.tail,
                },
                &syslog_mcp::app::SystemOsAdapter,
            )
            .await?;
            print_service_logs_response(&report, json)
        }
    }
}

mod dispatch;
mod dispatch_ai;
mod dispatch_db;
mod dispatch_surface;
#[allow(dead_code)]
mod http_client;

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
