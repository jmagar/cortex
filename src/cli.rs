use anyhow::{bail, Result};
use cortex::app::ServiceLogsRequest;
use cortex::compose::{
    CliDockerInspect, ComposeDefaults, ComposeMutation, ComposeService, ProcessRunner,
};

mod args;
mod args_config;
pub(crate) use args::{
    AgentCommandCommand, AgentCommandIngestSpoolArgs, AgentCommandWrapArgs, AiAbuseArgs, AiAddArgs,
    AiAskHistoryArgs, AiAssessArgs, AiBlocksArgs, AiCheckpointsArgs, AiCommand, AiContextArgs,
    AiCorrelateArgs, AiDoctorArgs, AiErrorsArgs, AiIncidentContextArgs, AiIncidentsArgs,
    AiIndexArgs, AiInvestigateArgs, AiListArgs, AiPruneCheckpointsArgs, AiSearchArgs,
    AiSimilarArgs, AiWatchArgs, CliCommand, ComposeArgs, ComposeCommand, ComposeLogsArgs,
    ComposeMutationArgs, CorrelateArgs, DbBackupArgs, DbCheckpointArgs, DbCommand, DbIntegrityArgs,
    DbIntegrityStatusArgs, DbStatusArgs, DbVacuumArgs, EntityArgs, FilterArgs, GraphAroundArgs,
    GraphCommand, HeartbeatAgentArgs, HeartbeatCommand, IncidentArgs, IngestRateArgs,
    NotifyRecentArgs, NotifyTestArgs, OutputArgs, PatternsArgs, PluginHookArgs, SearchArgs,
    ServiceCommand, ServiceLogsArgs, SessionsArgs, SetupArgs, SetupCommand, ShellAtuinIndexArgs,
    ShellCommand, ShellIndexArgs, SigAckArgs, SigListArgs, SigUnackArgs, SourceIpsArgs, TailArgs,
    TimeRangeArgs, TimelineArgs,
};
pub(crate) use args_config::{
    ConfigCommand, ConfigGetArgs, ConfigListArgs, ConfigSetArgs, ConfigTarget, ConfigUnsetArgs,
};

mod commands;

mod run;
pub(crate) use dispatch_command_log::run_agent_command_wrap;
#[allow(unused_imports)]
pub(crate) use run::ENV_USE_HTTP;
pub(crate) use run::{run, CliMode, GlobalFlags};

mod ai_watch;
pub(crate) mod color;
mod config_cmd;
mod config_toml;
mod coordination;
mod dispatch_command_log;
mod format;
mod heartbeat_agent;
pub(crate) mod help;
mod hyperlinks;
mod output_ai;
mod output_ai_more;
mod output_common;
mod output_graph;
mod output_logs;
mod output_ops;
mod panel;
mod parse;
mod parse_admin;
mod parse_ai;
mod parse_ai_more;
mod parse_command_log;
mod parse_common;
mod parse_config;
mod parse_logs;
mod setup;
mod sparkline;
mod table;

pub(crate) use config_cmd::run_config;
pub(crate) use heartbeat_agent::run_heartbeat_no_db;
pub(crate) use parse_common::{parse_i64_flag, parse_u32_flag, FlagCursor};
pub(crate) use setup::install_self;
pub(crate) use setup::run_setup;

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
            output_ops::print_compose_status_response(&status, args.json)
        }
        ComposeCommand::Doctor(args) => {
            let status = service.status(&args.target)?;
            let coordination = coordination::run_coordination_phases();
            output_ops::print_compose_doctor_response(&status, &coordination, args.json)?;
            output_ops::ensure_doctor_coordination_ok(&coordination)?;
            cortex::compose::ensure_doctor_ready(&status)
        }
        ComposeCommand::Up(args) => output_ops::print_compose_command_response(
            &service.run_mutation(ComposeMutation::Up, &args.target, &args.options)?,
            args.json,
        ),
        ComposeCommand::Down(args) => output_ops::print_compose_command_response(
            &service.run_mutation(ComposeMutation::Down, &args.target, &args.options)?,
            args.json,
        ),
        ComposeCommand::Restart(args) => output_ops::print_compose_command_response(
            &service.run_mutation(ComposeMutation::Restart, &args.target, &args.options)?,
            args.json,
        ),
        ComposeCommand::Pull(args) => output_ops::print_compose_command_response(
            &service.run_mutation(ComposeMutation::Pull, &args.target, &args.options)?,
            args.json,
        ),
        ComposeCommand::Logs(args) => {
            let output = service.logs(&args.target, args.tail)?;
            if args.json {
                output_common::print_json(&output)?;
            } else {
                print!("{}", output.stdout);
                eprint!("{}", output.stderr);
            }
            output_ops::ensure_command_success(&output)
        }
    }
}

/// DB-free entry point for `cortex service ...` — avoids opening the SQLite
/// pool so this command remains usable when the DB is corrupted/locked/full.
pub(crate) async fn run_service_no_db(command: CliCommand) -> Result<()> {
    let CliCommand::Service(command) = command else {
        bail!("internal: run_service_no_db called with non-service command");
    };
    match command {
        ServiceCommand::Logs(args) => {
            let json = args.json;
            let report = cortex::app::run_service_logs(
                ServiceLogsRequest {
                    service: args.service,
                    from: args.from,
                    to: args.to,
                    tail: args.tail,
                },
                &cortex::app::SystemOsAdapter,
            )
            .await?;
            output_ai::print_service_logs_response(&report, json)
        }
    }
}

#[cfg(test)]
use ai_watch::smoke_watch_target;
#[cfg(test)]
use coordination::{
    ai_watch_coordination_phase, canonicalize_with_warning, lookup_systemd_db_path,
    parse_systemctl_env_output, DoctorCache, SystemctlEnv,
};
#[cfg(test)]
use cortex::scanner::AiDoctorReport;
#[cfg(test)]
use output_ai::ensure_ai_doctor_success;
#[cfg(test)]
use output_common::truncate;
#[cfg(test)]
use output_ops::ensure_doctor_coordination_ok;
#[cfg(test)]
use setup::{SetupPhase, SetupStatus};

mod dispatch;
mod dispatch_ai;
mod dispatch_db;
mod dispatch_surface;
mod dispatch_surface_gap;
#[allow(dead_code)]
mod http_client;

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
