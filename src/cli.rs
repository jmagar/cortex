use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use syslog_mcp::app::{
    AbuseSearchResponse, AiCorrelateResponse, CorrelateEventsResponse, DbBackupResult,
    DbCheckpointResult, DbIntegrityResult, DbMaintenanceStatus, DbStats, DbVacuumResult,
    GetErrorsResponse, IncidentResponse, ListAiProjectsResponse, ListAiToolsResponse,
    ListHostsResponse, LogEntry, ProjectContextResponse, SearchLogsResponse,
    SearchSessionsResponse, ServiceLogsRequest, ServiceLogsResponse, SyslogService,
    UsageBlocksResponse,
};
use syslog_mcp::compose::{
    CliDockerInspect, CommandOutput, ComposeCommandResult, ComposeDefaults, ComposeMutation,
    ComposeService, ComposeStatus, ComposeTarget, MutationOptions, ProcessRunner,
};
use syslog_mcp::scanner::{
    AiDoctorReport, AiIndexingHealth, CheckpointEntry, IndexResult, ParseErrorEntry,
    PruneCheckpointsResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CliCommand {
    Search(SearchArgs),
    Tail(TailArgs),
    Errors(TimeRangeArgs),
    Hosts(OutputArgs),
    Sessions(SessionsArgs),
    Incident(IncidentArgs),
    Ai(AiCommand),
    Correlate(CorrelateArgs),
    Stats(OutputArgs),
    Compose(ComposeCommand),
    Service(ServiceCommand),
    Setup(SetupCommand),
    Db(DbCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AiCommand {
    Search(AiSearchArgs),
    Abuse(AiAbuseArgs),
    Correlate(AiCorrelateArgs),
    Blocks(AiBlocksArgs),
    Context(AiContextArgs),
    Tools(AiListArgs),
    Projects(AiListArgs),
    Index(AiIndexArgs),
    Add(AiAddArgs),
    Watch(AiWatchArgs),
    Checkpoints(AiCheckpointsArgs),
    Errors(AiErrorsArgs),
    PruneCheckpoints(AiPruneCheckpointsArgs),
    Doctor(AiDoctorArgs),
    WatchStatus(OutputArgs),
    SmokeWatch(OutputArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposeCommand {
    Status(ComposeArgs),
    Doctor(ComposeArgs),
    Up(ComposeMutationArgs),
    Down(ComposeMutationArgs),
    Restart(ComposeMutationArgs),
    Pull(ComposeMutationArgs),
    Logs(ComposeLogsArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServiceCommand {
    Logs(ServiceLogsArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetupCommand {
    Check(SetupArgs),
    Repair(SetupArgs),
    PluginHook(PluginHookArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SetupArgs {
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PluginHookArgs {
    pub json: bool,
    pub no_repair: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DbCommand {
    Status(DbStatusArgs),
    Integrity(DbIntegrityArgs),
    Checkpoint(DbCheckpointArgs),
    Vacuum(DbVacuumArgs),
    Backup(DbBackupArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DbIntegrityArgs {
    pub quick: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OutputArgs {
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DbStatusArgs {
    pub json: bool,
    /// Opt-in: run the host/container coordination diagnostic phases
    /// (`data-mount`, `ai-watch-coord`). These shell out to `docker inspect`
    /// and `systemctl --user show` and add roughly 100-200ms per invocation,
    /// so the default `db status` path is left untouched.
    pub check_coord: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DbCheckpointArgs {
    pub mode: String,
    pub json: bool,
}

impl Default for DbCheckpointArgs {
    fn default() -> Self {
        Self {
            mode: "passive".into(),
            json: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DbVacuumArgs {
    pub full: bool,
    pub pages: u32,
    /// CLI bool maps to server `Option<bool>` as
    /// `present → Some(true)`, `absent → None`. The size pre-flight on
    /// `--full` is bypassed only when the server receives `Some(true)`.
    /// See [`crate::app::models::DbVacuumRequest`] (bead 0p8r.4 #C3).
    pub force: bool,
    pub json: bool,
}

impl Default for DbVacuumArgs {
    fn default() -> Self {
        Self {
            full: false,
            pages: 1000,
            force: false,
            json: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DbBackupArgs {
    pub output: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiDoctorArgs {
    pub json: bool,
    pub strict_permissions: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ComposeArgs {
    pub target: ComposeTarget,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ComposeMutationArgs {
    pub target: ComposeTarget,
    pub options: MutationOptions,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ComposeLogsArgs {
    pub target: ComposeTarget,
    pub tail: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceLogsArgs {
    pub service: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub tail: Option<u32>,
    pub json: bool,
}

impl Default for ServiceLogsArgs {
    fn default() -> Self {
        Self {
            service: String::new(),
            from: None,
            to: None,
            tail: Some(200),
            json: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IncidentArgs {
    pub around: String,
    pub minutes: Option<u32>,
    pub service: Option<String>,
    pub hostname: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub hostname: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SearchArgs {
    pub query: Option<String>,
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub severity: Option<String>,
    pub app_name: Option<String>,
    pub facility: Option<String>,
    pub exclude_facility: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub received_from: Option<String>,
    pub received_to: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TailArgs {
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub app_name: Option<String>,
    pub n: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TimeRangeArgs {
    pub from: Option<String>,
    pub to: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CorrelateArgs {
    pub reference_time: String,
    pub window_minutes: Option<u32>,
    pub severity_min: Option<String>,
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub query: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiSearchArgs {
    pub query: String,
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiAbuseArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub before: Option<u32>,
    pub after: Option<u32>,
    pub terms: Vec<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiCorrelateArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub session_id: Option<String>,
    pub ai_query: Option<String>,
    pub log_query: Option<String>,
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub app_name: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub window_minutes: Option<u32>,
    pub severity_min: Option<String>,
    pub limit: Option<u32>,
    pub events_per_anchor: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiBlocksArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiContextArgs {
    pub project: String,
    pub tool: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiListArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiIndexArgs {
    pub path: Option<String>,
    pub force: bool,
    pub since: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiAddArgs {
    pub file: String,
    pub force: bool,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AiWatchArgs {
    pub path: Option<String>,
    pub debounce_ms: u64,
    pub settle_ms: u64,
    pub max_retries: u8,
    pub no_initial_scan: bool,
    pub json: bool,
}

impl Default for AiWatchArgs {
    fn default() -> Self {
        Self {
            path: None,
            debounce_ms: 750,
            settle_ms: 500,
            max_retries: 5,
            no_initial_scan: false,
            json: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiCheckpointsArgs {
    pub errors_only: bool,
    pub missing_only: bool,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiErrorsArgs {
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiPruneCheckpointsArgs {
    pub missing_only: bool,
    pub dry_run: bool,
    pub limit: Option<u32>,
    pub json: bool,
}

impl CliCommand {
    pub(crate) fn parse(args: Vec<String>) -> Result<Self> {
        let (command, rest) = args
            .split_first()
            .ok_or_else(|| anyhow!("CLI command is required"))?;
        match command.as_str() {
            "search" => parse_search(rest),
            "tail" => parse_tail(rest),
            "errors" => parse_errors(rest),
            "hosts" => parse_hosts(rest),
            "sessions" => parse_sessions(rest),
            "incident" => parse_incident(rest),
            "ai" => parse_ai(rest),
            "correlate" => parse_correlate(rest),
            "stats" => parse_stats(rest),
            "compose" => parse_compose(rest),
            "service" => parse_service(rest),
            "setup" => parse_setup(rest),
            "db" => parse_db(rest),
            _ => bail!("unknown CLI command: {command}"),
        }
    }
}

pub(crate) fn run_setup(command: SetupCommand) -> Result<()> {
    match command {
        SetupCommand::Check(args) => {
            let report = setup_report(SetupMode::Check)?;
            print_setup_report(&report, args.json)?;
            ensure_setup_success(&report)
        }
        SetupCommand::Repair(args) => {
            let report = setup_report(SetupMode::Repair)?;
            print_setup_report(&report, args.json)?;
            ensure_setup_success(&report)
        }
        SetupCommand::PluginHook(args) => run_plugin_hook(args),
    }
}

/// Top-level dispatch entry point. Built once per CLI invocation by `run_cli`
/// in `main.rs`. The [`CliMode`] decides whether we hit a local SQLite-backed
/// [`SyslogService`] or a remote container via [`HttpClient`].
///
/// HTTP dispatch is implemented incrementally by bead .7+ — for now, the
/// `Http` arm returns a clear placeholder error per command. The mode wiring
/// is in place so .7 can light up commands one by one without touching this
/// signature.
pub(crate) async fn run(mode: CliMode, command: CliCommand) -> Result<()> {
    // Query commands (search/tail/errors/hosts/correlate/stats/sessions) are
    // mode-agnostic: dispatch::run_X branches on `&CliMode` internally and
    // wraps the HTTP path in `http_or_cancel` for SIGINT handling. Everything
    // else (ai/db/compose/setup) still flows through the Local-only path.
    match command {
        CliCommand::Search(args) => dispatch::run_search(&mode, args).await,
        CliCommand::Tail(args) => dispatch::run_tail(&mode, args).await,
        CliCommand::Errors(args) => dispatch::run_errors(&mode, args).await,
        CliCommand::Hosts(args) => dispatch::run_hosts(&mode, args).await,
        CliCommand::Incident(args) => dispatch::run_incident(&mode, args).await,
        CliCommand::Correlate(args) => dispatch::run_correlate(&mode, args).await,
        CliCommand::Stats(args) => dispatch::run_stats(&mode, args).await,
        CliCommand::Sessions(args) => dispatch::run_sessions(&mode, args).await,
        // AI commands (bead 0p8r.8). 10 are HTTP-capable; 6 are LOCAL-only
        // and bail in HTTP mode with a per-command inline message.
        CliCommand::Ai(ai) => match ai {
            AiCommand::Search(args) => dispatch::run_ai_search(&mode, args).await,
            AiCommand::Abuse(args) => dispatch::run_ai_abuse(&mode, args).await,
            AiCommand::Correlate(args) => dispatch::run_ai_correlate(&mode, args).await,
            AiCommand::Blocks(args) => dispatch::run_ai_blocks(&mode, args).await,
            AiCommand::Context(args) => dispatch::run_ai_context(&mode, args).await,
            AiCommand::Tools(args) => dispatch::run_ai_tools(&mode, args).await,
            AiCommand::Projects(args) => dispatch::run_ai_projects(&mode, args).await,
            AiCommand::Checkpoints(args) => dispatch::run_ai_checkpoints(&mode, args).await,
            AiCommand::Errors(args) => dispatch::run_ai_errors(&mode, args).await,
            AiCommand::PruneCheckpoints(args) => {
                dispatch::run_ai_prune_checkpoints(&mode, args).await
            }
            AiCommand::Index(args) => dispatch::run_ai_index(&mode, args).await,
            AiCommand::Add(args) => dispatch::run_ai_add(&mode, args).await,
            AiCommand::Doctor(args) => dispatch::run_ai_doctor(&mode, args).await,
            AiCommand::SmokeWatch(args) => dispatch::run_ai_smoke_watch(&mode, args).await,
            AiCommand::WatchStatus(args) => dispatch::run_ai_watch_status(&mode, args).await,
            AiCommand::Watch(args) => dispatch::run_ai_watch(&mode, args).await,
        },
        // DB commands (bead 0p8r.9). 4 are HTTP-capable; backup stays LOCAL
        // and bails in HTTP mode with an inline message.
        CliCommand::Db(db) => match db {
            DbCommand::Status(args) => dispatch::run_db_status(&mode, args).await,
            DbCommand::Integrity(args) => dispatch::run_db_integrity(&mode, args).await,
            DbCommand::Checkpoint(args) => dispatch::run_db_checkpoint(&mode, args).await,
            DbCommand::Vacuum(args) => dispatch::run_db_vacuum(&mode, args).await,
            DbCommand::Backup(args) => dispatch::run_db_backup(&mode, args).await,
        },
        // Compose/Setup are local-only and main::run_cli reroutes them BEFORE
        // calling run(). If we reach here, the front door was bypassed —
        // bail with a clear internal-error message rather than a placeholder.
        CliCommand::Compose(_) | CliCommand::Service(_) | CliCommand::Setup(_) => {
            bail!(
                "internal: compose/service/setup must be dispatched by main::run_cli before reaching cli::run()"
            )
        }
    }
}

/// CLI transport mode resolved from global flags + env. Built once per
/// invocation; passed by value into [`run`].
///
/// `Local` keeps the full sqlx + rusqlite + FTS5 stack linked into the host
/// binary — acknowledged limitation, tracked for the v0.30 successor (bead
/// .12 doc note + epic acceptance criteria).
pub(crate) enum CliMode {
    Local(SyslogService),
    Http(http_client::HttpClient),
}

impl std::fmt::Debug for CliMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(_) => f.write_str("CliMode::Local(SyslogService)"),
            Self::Http(_) => f.write_str("CliMode::Http(HttpClient)"),
        }
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
pub async fn run_service_no_db(command: CliCommand) -> Result<()> {
    let CliCommand::Service(command) = command else {
        bail!("internal: run_service_no_db called with non-service command");
    };
    match command {
        ServiceCommand::Logs(args) => {
            let json = args.json;
            let report = syslog_mcp::app::run_service_logs(ServiceLogsRequest {
                service: args.service,
                from: args.from,
                to: args.to,
                tail: args.tail,
            })
            .await?;
            print_service_logs_response(&report, json)
        }
    }
}

fn parse_search(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SearchArgs::default();
    let mut query = Vec::new();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--source-ip" => parsed.source_ip = Some(flags.value("--source-ip")?),
            "--severity" => parsed.severity = Some(flags.value("--severity")?),
            "--app-name" => parsed.app_name = Some(flags.value("--app-name")?),
            "--facility" => parsed.facility = Some(flags.value("--facility")?),
            "--exclude-facility" => {
                parsed.exclude_facility = Some(flags.value("--exclude-facility")?)
            }
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            "--received-from" => parsed.received_from = Some(flags.value("--received-from")?),
            "--received-to" => parsed.received_to = Some(flags.value("--received-to")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "-h" | "--help" => bail!("use `syslog --help` for usage"),
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--source-ip=") => {
                parsed.source_ip = Some(value_after_equals(arg, "--source-ip")?)
            }
            _ if arg.starts_with("--severity=") => {
                parsed.severity = Some(value_after_equals(arg, "--severity")?)
            }
            _ if arg.starts_with("--app-name=") => {
                parsed.app_name = Some(value_after_equals(arg, "--app-name")?)
            }
            _ if arg.starts_with("--facility=") => {
                parsed.facility = Some(value_after_equals(arg, "--facility")?)
            }
            _ if arg.starts_with("--exclude-facility=") => {
                parsed.exclude_facility = Some(value_after_equals(arg, "--exclude-facility")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ if arg.starts_with("--received-from=") => {
                parsed.received_from = Some(value_after_equals(arg, "--received-from")?)
            }
            _ if arg.starts_with("--received-to=") => {
                parsed.received_to = Some(value_after_equals(arg, "--received-to")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown search option: {arg}"),
            _ => query.push(arg),
        }
    }
    parsed.query = (!query.is_empty()).then(|| query.join(" "));
    Ok(CliCommand::Search(parsed))
}

fn parse_service(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("service requires a subcommand"))?;
    match subcommand.as_str() {
        "logs" => parse_service_logs(rest),
        _ => bail!("unknown service subcommand: {subcommand}"),
    }
}

fn parse_service_logs(args: &[String]) -> Result<CliCommand> {
    let (service, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("service logs requires a service name"))?;
    if service.starts_with('-') {
        bail!("service logs requires a service name");
    }
    let mut parsed = ServiceLogsArgs {
        service: service.clone(),
        ..Default::default()
    };
    let mut flags = FlagCursor::new(rest);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            "--tail" | "-n" => parsed.tail = Some(parse_u32_flag(&arg, flags.value(&arg)?)?),
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ if arg.starts_with("--tail=") => {
                parsed.tail = Some(parse_u32_flag(
                    "--tail",
                    value_after_equals(arg, "--tail")?,
                )?)
            }
            _ if arg.starts_with("-n=") => {
                parsed.tail = Some(parse_u32_flag("-n", value_after_equals(arg, "-n")?)?)
            }
            _ if arg.starts_with('-') => bail!("unknown service logs option: {arg}"),
            _ => bail!("unexpected service logs argument: {arg}"),
        }
    }
    Ok(CliCommand::Service(ServiceCommand::Logs(parsed)))
}

fn parse_incident(args: &[String]) -> Result<CliCommand> {
    let mut parsed = IncidentArgs {
        minutes: Some(5),
        limit: Some(500),
        ..Default::default()
    };
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--around" => parsed.around = flags.value("--around")?,
            "--minutes" => {
                parsed.minutes = Some(parse_u32_flag("--minutes", flags.value("--minutes")?)?)
            }
            "--service" => parsed.service = Some(flags.value("--service")?),
            "--hostname" | "--host" => parsed.hostname = Some(flags.value(&arg)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--around=") => {
                parsed.around = value_after_equals(arg, "--around")?
            }
            _ if arg.starts_with("--minutes=") => {
                parsed.minutes = Some(parse_u32_flag(
                    "--minutes",
                    value_after_equals(arg, "--minutes")?,
                )?)
            }
            _ if arg.starts_with("--service=") => {
                parsed.service = Some(value_after_equals(arg, "--service")?)
            }
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--host=") => {
                parsed.hostname = Some(value_after_equals(arg, "--host")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown incident option: {arg}"),
            _ => bail!("unexpected incident argument: {arg}"),
        }
    }
    if parsed.around.is_empty() {
        bail!("incident requires --around <RFC3339>");
    }
    Ok(CliCommand::Incident(parsed))
}

fn parse_tail(args: &[String]) -> Result<CliCommand> {
    let mut parsed = TailArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--source-ip" => parsed.source_ip = Some(flags.value("--source-ip")?),
            "--app-name" => parsed.app_name = Some(flags.value("--app-name")?),
            "--n" | "-n" => parsed.n = Some(parse_u32_flag(&arg, flags.value(&arg)?)?),
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--source-ip=") => {
                parsed.source_ip = Some(value_after_equals(arg, "--source-ip")?)
            }
            _ if arg.starts_with("--app-name=") => {
                parsed.app_name = Some(value_after_equals(arg, "--app-name")?)
            }
            _ if arg.starts_with("--n=") => {
                parsed.n = Some(parse_u32_flag("--n", value_after_equals(arg, "--n")?)?)
            }
            _ if arg.starts_with('-') => bail!("unknown tail option: {arg}"),
            _ => parsed.n = Some(parse_u32_flag("n", arg)?),
        }
    }
    Ok(CliCommand::Tail(parsed))
}

fn parse_errors(args: &[String]) -> Result<CliCommand> {
    let mut parsed = TimeRangeArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ => bail!("unknown errors option: {arg}"),
        }
    }
    Ok(CliCommand::Errors(parsed))
}

fn parse_hosts(args: &[String]) -> Result<CliCommand> {
    Ok(CliCommand::Hosts(parse_output_args("hosts", args)?))
}

fn parse_sessions(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown sessions option: {arg}"),
            _ => bail!("unexpected sessions argument: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(parsed))
}

fn parse_ai(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("ai requires a subcommand"))?;
    match subcommand.as_str() {
        "search" => parse_ai_search(rest),
        "abuse" => parse_ai_abuse(rest),
        "correlate" => parse_ai_correlate(rest),
        "blocks" => parse_ai_blocks(rest),
        "context" => parse_ai_context(rest),
        "tools" => parse_ai_tools(rest),
        "projects" => parse_ai_projects(rest),
        "index" => parse_ai_index(rest),
        "add" => parse_ai_add(rest),
        "watch" => parse_ai_watch(rest),
        "checkpoints" => parse_ai_checkpoints(rest),
        "errors" => parse_ai_errors(rest),
        "prune-checkpoints" => parse_ai_prune_checkpoints(rest),
        "doctor" => parse_ai_doctor(rest),
        "watch-status" => Ok(CliCommand::Ai(AiCommand::WatchStatus(parse_output_args(
            "ai watch-status",
            rest,
        )?))),
        "smoke-watch" => Ok(CliCommand::Ai(AiCommand::SmokeWatch(parse_output_args(
            "ai smoke-watch",
            rest,
        )?))),
        _ => bail!("unknown ai subcommand: {subcommand}"),
    }
}

fn parse_ai_search(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiSearchArgs::default();
    let mut query = Vec::new();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown ai search option: {arg}"),
            _ => query.push(arg),
        }
    }
    parsed.query = query.join(" ");
    if parsed.query.is_empty() {
        bail!("ai search requires a query");
    }
    Ok(CliCommand::Ai(AiCommand::Search(parsed)))
}

fn parse_ai_abuse(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiAbuseArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--before" => {
                parsed.before = Some(parse_u32_flag("--before", flags.value("--before")?)?)
            }
            "--after" => parsed.after = Some(parse_u32_flag("--after", flags.value("--after")?)?),
            "--term" => parsed.terms.push(flags.value("--term")?),
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with("--before=") => {
                parsed.before = Some(parse_u32_flag(
                    "--before",
                    value_after_equals(arg, "--before")?,
                )?)
            }
            _ if arg.starts_with("--after=") => {
                parsed.after = Some(parse_u32_flag(
                    "--after",
                    value_after_equals(arg, "--after")?,
                )?)
            }
            _ if arg.starts_with("--term=") => {
                parsed.terms.push(value_after_equals(arg, "--term")?)
            }
            _ if arg.starts_with('-') => bail!("unknown ai abuse option: {arg}"),
            _ => bail!("unexpected ai abuse argument: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Abuse(parsed)))
}

fn parse_ai_correlate(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiCorrelateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--session-id" => parsed.session_id = Some(flags.value("--session-id")?),
            "--ai-query" => parsed.ai_query = Some(flags.value("--ai-query")?),
            "--log-query" => parsed.log_query = Some(flags.value("--log-query")?),
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--source-ip" => parsed.source_ip = Some(flags.value("--source-ip")?),
            "--app-name" => parsed.app_name = Some(flags.value("--app-name")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            "--window-minutes" => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--severity-min" => parsed.severity_min = Some(flags.value("--severity-min")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--events-per-anchor" => {
                parsed.events_per_anchor = Some(parse_u32_flag(
                    "--events-per-anchor",
                    flags.value("--events-per-anchor")?,
                )?)
            }
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--session-id=") => {
                parsed.session_id = Some(value_after_equals(arg, "--session-id")?)
            }
            _ if arg.starts_with("--ai-query=") => {
                parsed.ai_query = Some(value_after_equals(arg, "--ai-query")?)
            }
            _ if arg.starts_with("--log-query=") => {
                parsed.log_query = Some(value_after_equals(arg, "--log-query")?)
            }
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--source-ip=") => {
                parsed.source_ip = Some(value_after_equals(arg, "--source-ip")?)
            }
            _ if arg.starts_with("--app-name=") => {
                parsed.app_name = Some(value_after_equals(arg, "--app-name")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ if arg.starts_with("--window-minutes=") => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    value_after_equals(arg, "--window-minutes")?,
                )?)
            }
            _ if arg.starts_with("--severity-min=") => {
                parsed.severity_min = Some(value_after_equals(arg, "--severity-min")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with("--events-per-anchor=") => {
                parsed.events_per_anchor = Some(parse_u32_flag(
                    "--events-per-anchor",
                    value_after_equals(arg, "--events-per-anchor")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown ai correlate option: {arg}"),
            _ => bail!("unexpected ai correlate argument: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Correlate(parsed)))
}

fn parse_ai_blocks(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiBlocksArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ => bail!("unknown ai blocks option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Blocks(parsed)))
}

fn parse_ai_context(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiContextArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = flags.value("--project")?,
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--project=") => {
                parsed.project = value_after_equals(arg, "--project")?
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown ai context option: {arg}"),
            _ if parsed.project.is_empty() => parsed.project = arg,
            _ => bail!("unexpected ai context argument: {arg}"),
        }
    }
    if parsed.project.is_empty() {
        bail!("ai context requires --project <PATH>");
    }
    Ok(CliCommand::Ai(AiCommand::Context(parsed)))
}

fn parse_ai_tools(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiListArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ => bail!("unknown ai tools option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Tools(parsed)))
}

fn parse_ai_projects(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiListArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ => bail!("unknown ai projects option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Projects(parsed)))
}

fn parse_ai_index(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiIndexArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--path" => parsed.path = Some(flags.value("--path")?),
            "--force" => parsed.force = true,
            "--since" => parsed.since = Some(flags.value("--since")?),
            _ if arg.starts_with("--path=") => {
                parsed.path = Some(value_after_equals(arg, "--path")?)
            }
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(value_after_equals(arg, "--since")?)
            }
            _ => bail!("unknown ai index option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Index(parsed)))
}

fn parse_ai_add(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiAddArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--file" => parsed.file = flags.value("--file")?,
            "--force" => parsed.force = true,
            _ if arg.starts_with("--file=") => parsed.file = value_after_equals(arg, "--file")?,
            _ => bail!("unknown ai add option: {arg}"),
        }
    }
    if parsed.file.is_empty() {
        bail!("ai add requires --file <PATH>");
    }
    Ok(CliCommand::Ai(AiCommand::Add(parsed)))
}

fn parse_positive_u64_flag(flag: &str, value: String) -> Result<u64> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| anyhow!("{flag} expects a positive integer"))?;
    if parsed == 0 {
        bail!("{flag} expects a positive integer");
    }
    Ok(parsed)
}

fn parse_ai_watch(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiWatchArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--path" => parsed.path = Some(flags.value("--path")?),
            "--debounce-ms" => {
                parsed.debounce_ms =
                    parse_positive_u64_flag("--debounce-ms", flags.value("--debounce-ms")?)?;
            }
            "--settle-ms" => {
                parsed.settle_ms =
                    parse_positive_u64_flag("--settle-ms", flags.value("--settle-ms")?)?;
            }
            "--max-retries" => {
                parsed.max_retries =
                    parse_u32_flag("--max-retries", flags.value("--max-retries")?)?
                        .try_into()
                        .map_err(|_| anyhow!("--max-retries is too large"))?;
            }
            "--no-initial-scan" => parsed.no_initial_scan = true,
            _ if arg.starts_with("--path=") => {
                parsed.path = Some(value_after_equals(arg, "--path")?)
            }
            _ if arg.starts_with("--debounce-ms=") => {
                parsed.debounce_ms = parse_positive_u64_flag(
                    "--debounce-ms",
                    value_after_equals(arg, "--debounce-ms")?,
                )?;
            }
            _ if arg.starts_with("--settle-ms=") => {
                parsed.settle_ms = parse_positive_u64_flag(
                    "--settle-ms",
                    value_after_equals(arg, "--settle-ms")?,
                )?;
            }
            _ if arg.starts_with("--max-retries=") => {
                parsed.max_retries =
                    parse_u32_flag("--max-retries", value_after_equals(arg, "--max-retries")?)?
                        .try_into()
                        .map_err(|_| anyhow!("--max-retries is too large"))?;
            }
            _ => bail!("unknown ai watch option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Watch(parsed)))
}

fn parse_ai_checkpoints(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiCheckpointsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--errors" => parsed.errors_only = true,
            "--missing" => parsed.missing_only = true,
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ => bail!("unknown ai checkpoints option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Checkpoints(parsed)))
}

fn parse_ai_errors(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiErrorsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ => bail!("unknown ai errors option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Errors(parsed)))
}

fn parse_ai_prune_checkpoints(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiPruneCheckpointsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--missing" => parsed.missing_only = true,
            "--dry-run" => parsed.dry_run = true,
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ => bail!("unknown ai prune-checkpoints option: {arg}"),
        }
    }
    if !parsed.missing_only {
        bail!("ai prune-checkpoints requires --missing");
    }
    Ok(CliCommand::Ai(AiCommand::PruneCheckpoints(parsed)))
}

fn parse_ai_doctor(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiDoctorArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--strict-permissions" => parsed.strict_permissions = true,
            _ => bail!("unknown ai doctor option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Doctor(parsed)))
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AiWatchStatusReport {
    service: String,
    active: Option<String>,
    enabled: Option<String>,
    main_pid: Option<u32>,
    exec_start: Option<String>,
    exec_main_start_timestamp: Option<String>,
    process_start_time: Option<String>,
    db_path: String,
    health: AiIndexingHealth,
    latest_journal: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AiSmokeWatchReport {
    session_id: String,
    transcript_path: PathBuf,
    ingested: bool,
    pruned_missing_checkpoint: bool,
    missing_checkpoint_count: i64,
}

struct AiSmokeWatchTarget {
    tool: &'static str,
    project: String,
    transcript_path: PathBuf,
    body: String,
}

pub(super) async fn ai_smoke_watch(service: &SyslogService) -> Result<AiSmokeWatchReport> {
    let doctor = service.ai_doctor().await?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let session_id = format!("syslogsmokewatch{stamp}{}", std::process::id());
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let target = smoke_watch_target(&doctor, &stamp, &session_id, &now)?;
    if let Some(parent) = target.transcript_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target.transcript_path, &target.body)?;
    let canonical_transcript_path = target.transcript_path.canonicalize()?;

    let mut ingested = false;
    for _ in 0..30 {
        let response = service
            .search_sessions(syslog_mcp::app::SearchSessionsRequest {
                query: session_id.clone(),
                project: Some(target.project.clone()),
                tool: Some(target.tool.into()),
                from: None,
                to: None,
                limit: Some(5),
            })
            .await?;
        if response
            .sessions
            .iter()
            .any(|session| session.session_id == session_id)
        {
            ingested = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    if !ingested {
        let _ = std::fs::remove_file(&target.transcript_path);
        bail!("AI watch smoke file was not ingested within 30s");
    }

    std::fs::remove_file(&target.transcript_path)?;
    let canonical_transcript_path = canonical_transcript_path.to_string_lossy().to_string();
    let mut missing_checkpoint_count = i64::MAX;
    let mut pruned_missing_checkpoint = false;
    for _ in 0..30 {
        let result = service.prune_ai_checkpoints(true, false, Some(500)).await?;
        if result
            .paths
            .iter()
            .any(|path| path == &canonical_transcript_path)
        {
            pruned_missing_checkpoint = true;
        }
        let current_doctor = service.ai_doctor().await?;
        missing_checkpoint_count = current_doctor.missing_checkpoint_count;
        if pruned_missing_checkpoint {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    Ok(AiSmokeWatchReport {
        session_id,
        transcript_path: target.transcript_path,
        ingested,
        pruned_missing_checkpoint,
        missing_checkpoint_count,
    })
}

fn smoke_watch_target(
    doctor: &AiDoctorReport,
    stamp: &str,
    session_id: &str,
    now: &str,
) -> Result<AiSmokeWatchTarget> {
    let project = std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| "/tmp/syslog-smoke-watch".to_string());
    if doctor.claude_root.exists && doctor.claude_root.readable && doctor.claude_root.writable {
        let root = PathBuf::from(&doctor.claude_root.path);
        let transcript_path = root.join(format!("syslog-smoke-watch-{stamp}.jsonl"));
        let body = serde_json::json!({
            "sessionId": session_id,
            "timestamp": now,
            "cwd": project.clone(),
            "content": format!("{session_id} live watcher smoke probe"),
        })
        .to_string()
            + "\n";
        return Ok(AiSmokeWatchTarget {
            tool: "claude",
            project,
            transcript_path,
            body,
        });
    }
    if doctor.codex_root.exists && doctor.codex_root.readable && doctor.codex_root.writable {
        let root = PathBuf::from(&doctor.codex_root.path);
        let transcript_path = root.join(format!("syslog-smoke-watch-{stamp}.jsonl"));
        let body = serde_json::json!({
            "type": "session_meta",
            "payload": {
                "id": session_id,
                "cwd": project.clone(),
            },
        })
        .to_string()
            + "\n"
            + &serde_json::json!({
                "type": "response_item",
                "timestamp": now,
                "payload": {
                    "id": session_id,
                    "content": [{
                        "type": "output_text",
                        "text": format!("{session_id} live watcher smoke probe"),
                    }],
                },
            })
            .to_string()
            + "\n";
        return Ok(AiSmokeWatchTarget {
            tool: "codex",
            project,
            transcript_path,
            body,
        });
    }
    bail!("no writable AI transcript root is available for smoke-watch");
}

pub(super) async fn ai_watch_status(service: &SyslogService) -> Result<AiWatchStatusReport> {
    const SERVICE: &str = "syslog-ai-watch.service";
    let active = systemctl_user_output(&["is-active", SERVICE]).ok();
    let enabled = systemctl_user_output(&["is-enabled", SERVICE]).ok();
    let main_pid = systemctl_user_output(&["show", "-p", "MainPID", "--value", SERVICE])
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid > 0);
    let exec_start = systemctl_user_output(&["show", "-p", "ExecStart", "--value", SERVICE]).ok();
    let exec_main_start_timestamp =
        systemctl_user_output(&["show", "-p", "ExecMainStartTimestamp", "--value", SERVICE]).ok();
    let process_start_time = syslog_mcp::doctor::ai_watcher_process_start_time();
    let doctor = service.ai_doctor().await?;
    let health = service
        .ai_indexing_health(process_start_time.clone())
        .await?;
    let latest_journal = command_output(
        "journalctl",
        &[
            "--user",
            "-u",
            SERVICE,
            "-n",
            "10",
            "--no-pager",
            "--output",
            "short-iso",
        ],
    )
    .map(|raw| raw.lines().map(str::to_string).collect())
    .unwrap_or_default();
    Ok(AiWatchStatusReport {
        service: SERVICE.to_string(),
        active,
        enabled,
        main_pid,
        exec_start,
        exec_main_start_timestamp,
        process_start_time,
        db_path: doctor.db_path,
        health,
        latest_journal,
    })
}

fn systemctl_user_output(args: &[&str]) -> Result<String> {
    let mut command = std::process::Command::new("systemctl");
    command.arg("--user").args(args);
    let output = command.output()?;
    let output =
        if output.status.success() || std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
            output
        } else if systemctl_needs_user_bus_fallback(&output) {
            if let Some((runtime_dir, bus_address)) = inferred_user_bus_env() {
                std::process::Command::new("systemctl")
                    .env("XDG_RUNTIME_DIR", runtime_dir)
                    .env("DBUS_SESSION_BUS_ADDRESS", bus_address)
                    .arg("--user")
                    .args(args)
                    .output()?
            } else {
                output
            }
        } else {
            output
        };
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() || !stdout.is_empty() {
        return Ok(stdout);
    }
    if !output.status.success() {
        bail!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(stdout)
}

fn systemctl_needs_user_bus_fallback(output: &std::process::Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr.contains("DBUS_SESSION_BUS_ADDRESS") || stderr.contains("user scope bus")
}

fn inferred_user_bus_env() -> Option<(PathBuf, String)> {
    let runtime_dir = PathBuf::from(format!("/run/user/{}", current_uid()));
    let bus = runtime_dir.join("bus");
    bus.exists()
        .then(|| (runtime_dir, format!("unix:path={}", bus.display())))
}

fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn command_output(program: &str, args: &[&str]) -> Result<String> {
    let mut command = std::process::Command::new(program);
    command.args(args);
    if program == "journalctl" && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        if let Some((runtime_dir, bus_address)) = inferred_user_bus_env() {
            command
                .env("XDG_RUNTIME_DIR", runtime_dir)
                .env("DBUS_SESSION_BUS_ADDRESS", bus_address);
        }
    }
    let output = command.output()?;
    if !output.status.success() {
        bail!(
            "{} {} failed: {}",
            program,
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_stats(args: &[String]) -> Result<CliCommand> {
    Ok(CliCommand::Stats(parse_output_args("stats", args)?))
}

fn parse_db(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("db requires a subcommand"))?;
    match subcommand.as_str() {
        "status" => parse_db_status(rest),
        "integrity" => parse_db_integrity(rest),
        "checkpoint" => parse_db_checkpoint(rest),
        "vacuum" => parse_db_vacuum(rest),
        "backup" => parse_db_backup(rest),
        _ => bail!("unknown db subcommand: {subcommand}"),
    }
}

fn parse_db_status(args: &[String]) -> Result<CliCommand> {
    let mut parsed = DbStatusArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--check-coord" => parsed.check_coord = true,
            _ => bail!("unknown db status option: {arg}"),
        }
    }
    Ok(CliCommand::Db(DbCommand::Status(parsed)))
}

fn parse_db_integrity(args: &[String]) -> Result<CliCommand> {
    let mut parsed = DbIntegrityArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--quick" => parsed.quick = true,
            _ => bail!("unknown db integrity option: {arg}"),
        }
    }
    Ok(CliCommand::Db(DbCommand::Integrity(parsed)))
}

fn parse_db_checkpoint(args: &[String]) -> Result<CliCommand> {
    let mut parsed = DbCheckpointArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--mode" => parsed.mode = flags.value("--mode")?,
            _ if arg.starts_with("--mode=") => parsed.mode = value_after_equals(arg, "--mode")?,
            _ => bail!("unknown db checkpoint option: {arg}"),
        }
    }
    match parsed.mode.as_str() {
        "passive" | "full" | "restart" | "truncate" => {}
        _ => bail!("--mode must be one of passive, full, restart, truncate"),
    }
    Ok(CliCommand::Db(DbCommand::Checkpoint(parsed)))
}

fn parse_db_vacuum(args: &[String]) -> Result<CliCommand> {
    let mut parsed = DbVacuumArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--full" => parsed.full = true,
            "--force" => parsed.force = true,
            "--pages" => parsed.pages = parse_u32_flag("--pages", flags.value("--pages")?)?,
            _ if arg.starts_with("--pages=") => {
                parsed.pages = parse_u32_flag("--pages", value_after_equals(arg, "--pages")?)?
            }
            _ => bail!("unknown db vacuum option: {arg}"),
        }
    }
    if parsed.pages == 0 {
        bail!("--pages must be greater than zero");
    }
    Ok(CliCommand::Db(DbCommand::Vacuum(parsed)))
}

fn parse_db_backup(args: &[String]) -> Result<CliCommand> {
    let mut parsed = DbBackupArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--output" => parsed.output = Some(flags.value("--output")?),
            _ if arg.starts_with("--output=") => {
                parsed.output = Some(value_after_equals(arg, "--output")?)
            }
            _ => bail!("unknown db backup option: {arg}"),
        }
    }
    Ok(CliCommand::Db(DbCommand::Backup(parsed)))
}

fn parse_compose(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("compose requires a subcommand"))?;
    match subcommand.as_str() {
        "status" => Ok(CliCommand::Compose(ComposeCommand::Status(
            parse_compose_args(rest)?,
        ))),
        "doctor" => Ok(CliCommand::Compose(ComposeCommand::Doctor(
            parse_compose_args(rest)?,
        ))),
        "up" => Ok(CliCommand::Compose(ComposeCommand::Up(
            parse_compose_mutation(rest, false)?,
        ))),
        "down" => Ok(CliCommand::Compose(ComposeCommand::Down(
            parse_compose_mutation(rest, true)?,
        ))),
        "restart" => Ok(CliCommand::Compose(ComposeCommand::Restart(
            parse_compose_mutation(rest, false)?,
        ))),
        "pull" => Ok(CliCommand::Compose(ComposeCommand::Pull(
            parse_compose_mutation(rest, false)?,
        ))),
        "logs" => Ok(CliCommand::Compose(ComposeCommand::Logs(
            parse_compose_logs(rest)?,
        ))),
        "config" => bail!("syslog compose config is deferred from the first pass"),
        "upgrade" => bail!(
            "syslog compose upgrade is deferred; run `syslog compose pull` then `syslog compose up`"
        ),
        other => bail!("unknown compose subcommand: {other}"),
    }
}

fn parse_setup(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("setup requires a subcommand"))?;
    match subcommand.as_str() {
        "check" => Ok(CliCommand::Setup(SetupCommand::Check(parse_setup_args(
            rest,
        )?))),
        "repair" => Ok(CliCommand::Setup(SetupCommand::Repair(parse_setup_args(
            rest,
        )?))),
        "plugin-hook" | "hook" => Ok(CliCommand::Setup(SetupCommand::PluginHook(
            parse_plugin_hook_args(rest)?,
        ))),
        other => bail!("unknown setup subcommand: {other}"),
    }
}

fn parse_setup_args(args: &[String]) -> Result<SetupArgs> {
    let mut parsed = SetupArgs::default();
    for arg in args {
        match arg.as_str() {
            "--json" => parsed.json = true,
            _ => bail!("unknown setup option: {arg}"),
        }
    }
    Ok(parsed)
}

fn parse_plugin_hook_args(args: &[String]) -> Result<PluginHookArgs> {
    let mut parsed = PluginHookArgs::default();
    for arg in args {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--no-repair" => parsed.no_repair = true,
            _ => bail!("unknown setup plugin-hook option: {arg}"),
        }
    }
    Ok(parsed)
}

fn parse_compose_args(args: &[String]) -> Result<ComposeArgs> {
    let mut parsed = ComposeArgs::default();
    parse_compose_common(args, &mut parsed.target, &mut parsed.json)?;
    reject_unknown_compose_args("compose", args, &[])?;
    Ok(parsed)
}

fn parse_compose_mutation(args: &[String], destructive: bool) -> Result<ComposeMutationArgs> {
    let mut parsed = ComposeMutationArgs::default();
    parse_compose_common(args, &mut parsed.target, &mut parsed.json)?;
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--dry-run" => parsed.options.dry_run = true,
            "--allow-cwd-target" => parsed.options.allow_cwd_target = true,
            "--yes" => parsed.options.yes = true,
            _ if is_compose_common_arg(&arg) => {
                consume_compose_common_value(&mut flags, &arg)?;
            }
            _ if arg.starts_with("--") => bail!("unknown compose option: {arg}"),
            _ => bail!("unexpected compose argument: {arg}"),
        }
    }
    parsed.options.non_interactive = destructive;
    Ok(parsed)
}

fn parse_compose_logs(args: &[String]) -> Result<ComposeLogsArgs> {
    let mut parsed = ComposeLogsArgs::default();
    parse_compose_common(args, &mut parsed.target, &mut parsed.json)?;
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--tail" => parsed.tail = Some(parse_u32_flag("--tail", flags.value("--tail")?)?),
            _ if arg.starts_with("--tail=") => {
                parsed.tail = Some(parse_u32_flag(
                    "--tail",
                    value_after_equals(arg, "--tail")?,
                )?)
            }
            "--follow" => bail!("syslog compose logs --follow is deferred"),
            _ if is_compose_common_arg(&arg) => {
                consume_compose_common_value(&mut flags, &arg)?;
            }
            _ if arg.starts_with("--") => bail!("unknown compose logs option: {arg}"),
            _ => bail!("unexpected compose logs argument: {arg}"),
        }
    }
    Ok(parsed)
}

fn parse_compose_common(
    args: &[String],
    target: &mut ComposeTarget,
    json: &mut bool,
) -> Result<()> {
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => *json = true,
            "--compose-file" => target.compose_file = Some(flags.value("--compose-file")?.into()),
            "--project-dir" => target.project_dir = Some(flags.value("--project-dir")?.into()),
            "--project-name" => target.project_name = Some(flags.value("--project-name")?),
            "--service" => target.service = Some(flags.value("--service")?),
            "--container" => target.container_name = Some(flags.value("--container")?),
            _ if arg.starts_with("--compose-file=") => {
                target.compose_file = Some(value_after_equals(arg, "--compose-file")?.into())
            }
            _ if arg.starts_with("--project-dir=") => {
                target.project_dir = Some(value_after_equals(arg, "--project-dir")?.into())
            }
            _ if arg.starts_with("--project-name=") => {
                target.project_name = Some(value_after_equals(arg, "--project-name")?)
            }
            _ if arg.starts_with("--service=") => {
                target.service = Some(value_after_equals(arg, "--service")?)
            }
            _ if arg.starts_with("--container=") => {
                target.container_name = Some(value_after_equals(arg, "--container")?)
            }
            _ => {}
        }
    }
    Ok(())
}

fn reject_unknown_compose_args(command: &str, args: &[String], extra: &[&str]) -> Result<()> {
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if extra.contains(&arg.as_str()) {
            continue;
        }
        if is_compose_common_arg(&arg) {
            consume_compose_common_value(&mut flags, &arg)?;
            continue;
        }
        if arg.starts_with("--") {
            bail!("unknown {command} option: {arg}");
        }
        bail!("unexpected {command} argument: {arg}");
    }
    Ok(())
}

fn is_compose_common_arg(arg: &str) -> bool {
    matches!(
        arg,
        "--json"
            | "--compose-file"
            | "--project-dir"
            | "--project-name"
            | "--service"
            | "--container"
    ) || arg.starts_with("--compose-file=")
        || arg.starts_with("--project-dir=")
        || arg.starts_with("--project-name=")
        || arg.starts_with("--service=")
        || arg.starts_with("--container=")
}

fn needs_value(arg: &str) -> bool {
    matches!(
        arg,
        "--compose-file" | "--project-dir" | "--project-name" | "--service" | "--container"
    )
}

fn consume_compose_common_value(flags: &mut FlagCursor<'_>, arg: &str) -> Result<()> {
    if !arg.contains('=') && needs_value(arg) {
        let _ = flags.value(arg)?;
    }
    Ok(())
}

fn parse_correlate(args: &[String]) -> Result<CliCommand> {
    let mut parsed = CorrelateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--reference-time" => parsed.reference_time = flags.value("--reference-time")?,
            "--window-minutes" => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--severity-min" => parsed.severity_min = Some(flags.value("--severity-min")?),
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--source-ip" => parsed.source_ip = Some(flags.value("--source-ip")?),
            "--query" => parsed.query = Some(flags.value("--query")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--reference-time=") => {
                parsed.reference_time = value_after_equals(arg, "--reference-time")?
            }
            _ if arg.starts_with("--window-minutes=") => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    value_after_equals(arg, "--window-minutes")?,
                )?)
            }
            _ if arg.starts_with("--severity-min=") => {
                parsed.severity_min = Some(value_after_equals(arg, "--severity-min")?)
            }
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--source-ip=") => {
                parsed.source_ip = Some(value_after_equals(arg, "--source-ip")?)
            }
            _ if arg.starts_with("--query=") => {
                parsed.query = Some(value_after_equals(arg, "--query")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown correlate option: {arg}"),
            _ if parsed.reference_time.is_empty() => parsed.reference_time = arg,
            _ => bail!("unexpected correlate argument: {arg}"),
        }
    }
    if parsed.reference_time.is_empty() {
        bail!("correlate requires --reference-time <RFC3339>");
    }
    Ok(CliCommand::Correlate(parsed))
}

fn parse_output_args(command: &str, args: &[String]) -> Result<OutputArgs> {
    let mut parsed = OutputArgs::default();
    for arg in args {
        match arg.as_str() {
            "--json" => parsed.json = true,
            _ => bail!("unknown {command} option: {arg}"),
        }
    }
    Ok(parsed)
}

struct FlagCursor<'a> {
    args: &'a [String],
    index: usize,
}

impl<'a> FlagCursor<'a> {
    fn new(args: &'a [String]) -> Self {
        Self { args, index: 0 }
    }

    fn next(&mut self) -> Option<String> {
        let value = self.args.get(self.index)?.clone();
        self.index += 1;
        Some(value)
    }

    fn value(&mut self, flag: &str) -> Result<String> {
        let value = self
            .args
            .get(self.index)
            .ok_or_else(|| anyhow!("{flag} requires a value"))?
            .clone();
        if value.starts_with('-') {
            bail!("{flag} requires a value");
        }
        self.index += 1;
        Ok(value)
    }
}

fn value_after_equals(arg: String, flag: &str) -> Result<String> {
    let prefix = format!("{flag}=");
    let value = arg
        .strip_prefix(&prefix)
        .ok_or_else(|| anyhow!("expected {flag}=<value>"))?;
    if value.is_empty() {
        bail!("{flag} requires a value");
    }
    Ok(value.to_string())
}

fn parse_u32_flag(flag: &str, value: String) -> Result<u32> {
    value
        .parse::<u32>()
        .map_err(|_| anyhow!("{flag} must be an unsigned integer"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SetupMode {
    Check,
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SetupStatus {
    Ok,
    Warn,
    Error,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct SetupPhase {
    pub(super) name: &'static str,
    pub(super) status: SetupStatus,
    pub(super) detail: String,
}

#[derive(Debug, Clone, Serialize)]
struct SetupReport {
    mode: SetupMode,
    data_dir: PathBuf,
    env_path: PathBuf,
    phases: Vec<SetupPhase>,
    has_errors: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PluginHookExitPolicy {
    Success,
    AdvisoryFailure,
    BlockingFailure,
}

#[derive(Debug, Clone, Serialize)]
struct PluginHookReport {
    exit_policy: PluginHookExitPolicy,
    ran_repair: bool,
    no_repair: bool,
    blocking_failures: Vec<String>,
    advisory_failures: Vec<String>,
    check: SetupReport,
    repair: Option<SetupReport>,
}

fn run_plugin_hook(args: PluginHookArgs) -> Result<()> {
    let check = setup_report(SetupMode::Check)?;
    let repair = if check.has_errors && !args.no_repair {
        Some(setup_report(SetupMode::Repair)?)
    } else {
        None
    };
    let active = repair.as_ref().unwrap_or(&check);
    let blocking_failures = setup_blocking_failures(active);
    let advisory_failures = setup_advisory_failures(active);
    let exit_policy = if !blocking_failures.is_empty() {
        PluginHookExitPolicy::BlockingFailure
    } else if !advisory_failures.is_empty() {
        PluginHookExitPolicy::AdvisoryFailure
    } else {
        PluginHookExitPolicy::Success
    };
    let report = PluginHookReport {
        exit_policy,
        ran_repair: repair.is_some(),
        no_repair: args.no_repair,
        blocking_failures,
        advisory_failures,
        check,
        repair,
    };
    print_plugin_hook_report(&report, args.json)?;
    if matches!(report.exit_policy, PluginHookExitPolicy::BlockingFailure) {
        bail!(
            "syslog setup plugin-hook completed with blocking failed phases: {}",
            report.blocking_failures.join(", ")
        );
    }
    Ok(())
}

fn setup_report(mode: SetupMode) -> Result<SetupReport> {
    let data_dir = setup_data_dir();
    let env_path = data_dir.join(".env");

    if matches!(mode, SetupMode::Repair) {
        std::fs::create_dir_all(&data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&data_dir)?.permissions();
            perms.set_mode(0o700);
            std::fs::set_permissions(&data_dir, perms)?;
        }
    }

    let mut phases = Vec::new();
    phases.push(if data_dir.is_dir() {
        SetupPhase {
            name: "data-dir",
            status: SetupStatus::Ok,
            detail: format!("found {}", data_dir.display()),
        }
    } else {
        SetupPhase {
            name: "data-dir",
            status: SetupStatus::Error,
            detail: format!("missing {}; run syslog setup repair", data_dir.display()),
        }
    });
    phases.push(if env_path.exists() {
        SetupPhase {
            name: "env",
            status: SetupStatus::Ok,
            detail: format!("found {}", env_path.display()),
        }
    } else {
        SetupPhase {
            name: "env",
            status: SetupStatus::Warn,
            detail: format!(
                "missing {}; plugin env may be supplied by process",
                env_path.display()
            ),
        }
    });
    phases.push(
        if std::env::var("SYSLOG_MCP_TOKEN").is_ok()
            || std::env::var("SYSLOG_MCP_API_TOKEN").is_ok()
            || std::env::var("NO_AUTH").ok().as_deref() == Some("true")
        {
            SetupPhase {
                name: "auth",
                status: SetupStatus::Ok,
                detail: "token/no_auth configuration present".to_string(),
            }
        } else {
            SetupPhase {
                name: "auth",
                status: SetupStatus::Warn,
                detail: "no SYSLOG_MCP_TOKEN/SYSLOG_MCP_API_TOKEN in process env".to_string(),
            }
        },
    );
    phases.push(mcp_port_phase());
    // data_mount_phase intentionally NOT included here (bead syslog-mcp-0p8r.11).
    // Post-cutover (SYSLOG_USE_HTTP=true is the default), the CLI no longer
    // opens SQLite directly, so the SessionStart cost of docker inspect is no
    // longer paying for itself. Drift detection is preserved via:
    //   - `syslog compose doctor`           (always runs coord phases)
    //   - `syslog db status --check-coord`  (opt-in)
    // See bead syslog-mcp-0p8r.13 for the coord-phase wiring.

    let has_errors = phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error));
    Ok(SetupReport {
        mode,
        data_dir,
        env_path,
        phases,
        has_errors,
    })
}

/// Minimal `.env` parser: reads KEY=VALUE lines, ignores comments and quotes.
/// Returns the unquoted value if `key` is present.
fn read_env_value(path: &std::path::Path, key: &str) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() == key {
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(v);
            let v = v
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .unwrap_or(v);
            return Some(v.to_string());
        }
    }
    None
}

fn setup_data_dir() -> PathBuf {
    std::env::var_os("SYSLOG_DATA_DIR")
        .or_else(|| std::env::var_os("CLAUDE_PLUGIN_DATA"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".syslog-mcp")))
        .unwrap_or_else(|| PathBuf::from(".syslog-mcp"))
}

fn mcp_port_phase() -> SetupPhase {
    let port = setup_port("SYSLOG_MCP_PORT", 3100);
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => SetupPhase {
            name: "mcp-port",
            status: SetupStatus::Ok,
            detail: format!("port {port} available"),
        },
        Err(error) => SetupPhase {
            name: "mcp-port",
            status: SetupStatus::Warn,
            detail: format!("port {port} is already in use: {error}"),
        },
    }
}

fn setup_port(env_name: &str, default: u16) -> u16 {
    std::env::var(env_name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn setup_blocking_failures(report: &SetupReport) -> Vec<String> {
    report
        .phases
        .iter()
        .filter(|phase| matches!(phase.status, SetupStatus::Error))
        .map(|phase| phase.name.to_string())
        .collect()
}

fn setup_advisory_failures(report: &SetupReport) -> Vec<String> {
    report
        .phases
        .iter()
        .filter(|phase| matches!(phase.status, SetupStatus::Warn))
        .map(|phase| phase.name.to_string())
        .collect()
}

fn ensure_setup_success(report: &SetupReport) -> Result<()> {
    if report.has_errors {
        bail!("syslog setup completed with failed phases");
    }
    Ok(())
}

fn print_setup_report(report: &SetupReport, json: bool) -> Result<()> {
    if json {
        return print_json(report);
    }
    println!("Syslog setup mode: {:?}", report.mode);
    println!("Data dir: {}", report.data_dir.display());
    println!("Env: {}", report.env_path.display());
    for phase in &report.phases {
        println!("{:?}\t{}\t{}", phase.status, phase.name, phase.detail);
    }
    Ok(())
}

fn print_plugin_hook_report(report: &PluginHookReport, json: bool) -> Result<()> {
    if json {
        return print_json(report);
    }
    print_setup_report(&report.check, false)?;
    if let Some(repair) = &report.repair {
        print_setup_report(repair, false)?;
    }
    println!("Plugin hook policy: {:?}", report.exit_policy);
    println!("Plugin hook ran repair: {}", report.ran_repair);
    Ok(())
}

fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub(super) fn print_search_response(response: &SearchLogsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("{} log(s)", response.count);
    for log in &response.logs {
        print_log(log);
    }
    Ok(())
}

fn print_log(log: &LogEntry) {
    if is_transcript_log(log) {
        print_ai_log(log);
        return;
    }
    let app = log.app_name.as_deref().unwrap_or("-");
    println!(
        "{} {:<7} {:<20} {:<16} {}",
        log.timestamp, log.severity, log.hostname, app, log.message
    );
}

fn print_ai_log(log: &LogEntry) {
    let tool = log
        .ai_tool
        .as_deref()
        .or_else(|| {
            log.app_name
                .as_deref()
                .and_then(|app| app.strip_suffix("-transcript"))
        })
        .unwrap_or("ai");
    let project = log.ai_project.as_deref().unwrap_or("(unknown project)");
    let session = log.ai_session_id.as_deref().unwrap_or("(unknown session)");
    println!(
        "{} {:<7} {:<8} {:<36} session={}",
        log.timestamp,
        log.severity,
        truncate(tool, 8),
        truncate(project, 35),
        truncate(session, 24)
    );
    println!("    {}", indent_multiline(&log.message));
}

fn is_transcript_log(log: &LogEntry) -> bool {
    log.source_ip.starts_with("transcript://")
        || log
            .app_name
            .as_deref()
            .is_some_and(|app| app.ends_with("-transcript"))
}

fn indent_multiline(value: &str) -> String {
    value.replace('\n', "\n    ")
}

pub(super) fn print_errors_response(response: &GetErrorsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("HOST                 SEVERITY COUNT");
    for row in &response.summary {
        println!("{:<20} {:<8} {}", row.hostname, row.severity, row.count);
    }
    Ok(())
}

pub(super) fn print_hosts_response(response: &ListHostsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("HOST                 COUNT LAST SEEN");
    for host in &response.hosts {
        println!(
            "{:<20} {:<5} {}",
            host.hostname, host.log_count, host.last_seen
        );
    }
    Ok(())
}

pub(super) fn print_sessions_response(
    response: &syslog_mcp::app::ListSessionsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("{} session(s)", response.count);
    println!(
        "{:<40} {:<10} {:<36} {:<15} COUNT",
        "PROJECT", "TOOL", "SESSION ID", "HOST"
    );
    for s in &response.sessions {
        println!(
            "{:<40} {:<10} {:<36} {:<15} {}",
            truncate(&s.project, 39),
            s.tool,
            s.session_id,
            s.hostname,
            s.event_count
        );
    }
    Ok(())
}

pub(super) fn print_search_sessions_response(
    response: &SearchSessionsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} grouped session(s) from {} newest matching row(s){}",
        response.sessions.len(),
        response.candidate_rows,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    if response.candidate_window_truncated {
        println!(
            "search window capped at {} matching rows; use --project, --tool, --from, or --to to narrow exact grouping",
            response.candidate_cap
        );
    }
    println!(
        "{:<10} {:<30} {:<20} {:<6} MATCH",
        "TOOL", "PROJECT", "SESSION ID", "EVENTS"
    );
    for session in &response.sessions {
        println!(
            "{:<10} {:<30} {:<20} {:<6} {}",
            session.tool,
            truncate(&session.project, 29),
            truncate(&session.session_id, 19),
            session.event_count,
            session.match_count
        );
    }
    Ok(())
}

pub(super) fn print_abuse_search_response(
    response: &AbuseSearchResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} abuse match(es) from {} candidate row(s){}",
        response.matches.len(),
        response.candidate_rows,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    if response.candidate_window_truncated {
        println!(
            "abuse scan capped at {} candidate rows; use --project, --tool, --from, or --to to narrow it",
            response.candidate_cap
        );
    }
    println!("terms: {}", response.terms.join(", "));
    for item in &response.matches {
        println!();
        println!(
            "match term={} id={} {}",
            item.term, item.entry.id, item.entry.timestamp
        );
        for before in &item.before {
            println!("  before:");
            print_log(before);
        }
        println!("  hit:");
        print_log(&item.entry);
        for after in &item.after {
            println!("  after:");
            print_log(after);
        }
    }
    Ok(())
}

pub(super) fn print_ai_correlate_response(
    response: &AiCorrelateResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} AI anchor(s), {} related non-AI event(s), +/-{}m, severity >= {}{}",
        response.total_anchors,
        response.total_related_events,
        response.window_minutes,
        response.severity_min,
        if response.anchors_truncated {
            " (anchors truncated)"
        } else {
            ""
        }
    );
    for anchor in &response.anchors {
        println!();
        println!(
            "AI anchor id={} {} window={}..{}{}",
            anchor.entry.id,
            anchor.entry.timestamp,
            anchor.window_from,
            anchor.window_to,
            if anchor.related_truncated {
                " (related truncated)"
            } else {
                ""
            }
        );
        print_log(&anchor.entry);
        for log in &anchor.related {
            print_log(log);
        }
    }
    Ok(())
}

pub(super) fn print_usage_blocks_response(
    response: &UsageBlocksResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} usage block(s) shown of {}{}",
        response.blocks.len(),
        response.total_blocks,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for block in &response.blocks {
        println!(
            "{} {} {} {} events={} sessions={}",
            block.bucket_start,
            block.bucket_end,
            block.tool,
            truncate(&block.project, 30),
            block.event_count,
            block.session_count
        );
    }
    Ok(())
}

pub(super) fn print_project_context_response(
    response: &ProjectContextResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("project: {}", response.project);
    println!("event_count: {}", response.event_count);
    println!("tools: {}", response.tools.join(", "));
    println!("sessions: {}", response.sessions.len());
    println!("hosts: {}", response.hostnames.join(", "));
    println!(
        "recent_entries: {}{}",
        response.recent_entries.len(),
        if response.recent_entries_truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for entry in &response.recent_entries {
        print_log(entry);
    }
    Ok(())
}

pub(super) fn print_ai_tools_response(response: &ListAiToolsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} tool(s) shown of {}{}",
        response.tools.len(),
        response.total_tools,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    println!("TOOL       EVENTS SESSIONS LAST SEEN");
    for tool in &response.tools {
        println!(
            "{:<10} {:<6} {:<8} {}",
            tool.tool, tool.event_count, tool.session_count, tool.last_seen
        );
    }
    Ok(())
}

pub(super) fn print_ai_projects_response(
    response: &ListAiProjectsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} project(s) shown of {}{}",
        response.projects.len(),
        response.total_projects,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    println!("PROJECT                          EVENTS SESSIONS TOOLS");
    for project in &response.projects {
        println!(
            "{:<32} {:<6} {:<8} {}",
            truncate(&project.project, 32),
            project.event_count,
            project.session_count,
            project.tools.join(",")
        );
    }
    Ok(())
}

pub(super) fn print_checkpoints_response(response: &[CheckpointEntry], json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("{} checkpoint(s)", response.len());
    println!(
        "{:<12} {:<8} {:<6} {:<7} {:<7} PATH",
        "KIND", "RECORDS", "PARSE", "MISSING", "ERROR"
    );
    for checkpoint in response {
        println!(
            "{:<12} {:<8} {:<6} {:<7} {:<7} {}",
            checkpoint.source_kind,
            checkpoint.imported_records,
            checkpoint.parse_errors,
            if checkpoint.missing { "yes" } else { "-" },
            if checkpoint.last_error.is_some() {
                "yes"
            } else {
                "-"
            },
            truncate(&checkpoint.canonical_path, 80),
        );
        if let Some(error) = &checkpoint.last_error {
            println!("    error: {}", truncate(error, 160));
        }
    }
    Ok(())
}

pub(super) fn print_ai_parse_errors_response(
    response: &[ParseErrorEntry],
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("{} parse error(s)", response.len());
    println!(
        "{:<24} {:<8} {:<8} {:<40} ERROR",
        "SEEN", "KIND", "LINE", "PATH"
    );
    for error in response {
        println!(
            "{:<24} {:<8} {:<8} {:<40} {}",
            truncate(&error.seen_at, 23),
            truncate(&error.source_kind, 8),
            error.line_no,
            truncate(&error.canonical_path, 39),
            truncate(&error.error, 100),
        );
        if let Some(preview) = &error.record_preview {
            println!("    preview: {}", truncate(preview, 160));
        }
    }
    Ok(())
}

pub(super) fn print_prune_checkpoints_response(
    response: &PruneCheckpointsResult,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "matched={} pruned={} dry_run={}",
        response.matched, response.pruned, response.dry_run
    );
    for path in &response.paths {
        println!("  {}", path);
    }
    Ok(())
}

pub(super) fn print_ai_doctor_response(response: &AiDoctorReport, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("db_path: {}", response.db_path);
    println!(
        "db_schema_version: {}/{} ({})",
        response.db_schema_version,
        response.known_schema_version,
        if response.schema_current {
            "current"
        } else {
            "behind"
        }
    );
    println!(
        "db_last_migration_at: {}",
        response.db_last_migration_at.as_deref().unwrap_or("-")
    );
    println!(
        "claude_root: {} ({}, readable={}, writable={}, owner={:?}:{:?}, mode={:?}, strict_ok={})",
        response.claude_root.path,
        if response.claude_root.exists {
            "exists"
        } else {
            "missing"
        },
        response.claude_root.readable,
        response.claude_root.writable,
        response.claude_root.owner_uid,
        response.claude_root.owner_gid,
        response.claude_root.mode.map(|mode| format!("{mode:o}")),
        response.claude_root.strict_ok
    );
    println!(
        "codex_root: {} ({}, readable={}, writable={}, owner={:?}:{:?}, mode={:?}, strict_ok={})",
        response.codex_root.path,
        if response.codex_root.exists {
            "exists"
        } else {
            "missing"
        },
        response.codex_root.readable,
        response.codex_root.writable,
        response.codex_root.owner_uid,
        response.codex_root.owner_gid,
        response.codex_root.mode.map(|mode| format!("{mode:o}")),
        response.codex_root.strict_ok
    );
    println!("checkpoint_count: {}", response.checkpoint_count);
    println!(
        "checkpoint_error_count: {}",
        response.checkpoint_error_count
    );
    println!(
        "missing_checkpoint_count: {}",
        response.missing_checkpoint_count
    );
    println!("imported_record_count: {}", response.imported_record_count);
    println!("parse_error_count: {}", response.parse_error_count);
    println!(
        "newest_indexed: {} {}",
        response.newest_indexed_at.as_deref().unwrap_or("-"),
        response.newest_indexed_path.as_deref().unwrap_or("-")
    );
    Ok(())
}

pub(super) fn print_ai_watch_status_response(
    response: &AiWatchStatusReport,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("service: {}", response.service);
    println!("active: {}", response.active.as_deref().unwrap_or("-"));
    println!("enabled: {}", response.enabled.as_deref().unwrap_or("-"));
    println!(
        "main_pid: {}",
        response
            .main_pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "exec_start: {}",
        response.exec_start.as_deref().unwrap_or("-")
    );
    println!(
        "process_start_time: {}",
        response.process_start_time.as_deref().unwrap_or("-")
    );
    println!("db_path: {}", response.db_path);
    println!(
        "db_schema_version: {}/{}",
        response.health.db_schema_version, response.health.known_schema_version
    );
    println!(
        "schema_drift_detected: {}",
        response.health.schema_drift_detected
    );
    println!(
        "last_successful_ingest_at: {}",
        response
            .health
            .last_successful_ingest_at
            .as_deref()
            .unwrap_or("-")
    );
    println!(
        "recent_failure_count: {}",
        response.health.recent_failure_count
    );
    if !response.health.stale_indicators.is_empty() {
        println!(
            "stale_indicators: {}",
            response.health.stale_indicators.join(", ")
        );
    }
    if !response.latest_journal.is_empty() {
        println!("latest_journal:");
        for line in &response.latest_journal {
            println!("  {line}");
        }
    }
    Ok(())
}

fn print_service_logs_response(report: &ServiceLogsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(report);
    }
    if report.dropped_lines > 0 {
        eprintln!(
            "warning: {} malformed journal line(s) dropped",
            report.dropped_lines
        );
    }
    if report.entries.is_empty() {
        println!("{}: 0 journal entries", report.service);
        return Ok(());
    }
    for entry in &report.entries {
        let timestamp = entry.timestamp.as_deref().unwrap_or("-");
        let ident = entry
            .syslog_identifier
            .as_deref()
            .or(entry.unit.as_deref())
            .unwrap_or("-");
        let message = entry.message.as_deref().unwrap_or("");
        println!("{timestamp} {ident}: {message}");
    }
    Ok(())
}

pub(super) fn print_incident_response(response: &IncidentResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "incident around {} +/- {}m: {} event(s){}",
        response.around,
        response.window_minutes,
        response.event_count,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for warning in &response.warnings {
        println!("warn: {warning}");
    }
    for event in &response.events {
        let host = event.host.as_deref().unwrap_or("-");
        let severity = event.severity.as_deref().unwrap_or("-");
        let app = event.app.as_deref().unwrap_or("-");
        println!(
            "{} {} {} {} {}: {}",
            event.timestamp, event.source, host, severity, app, event.message
        );
    }
    Ok(())
}

pub(super) fn print_ai_smoke_watch_response(
    response: &AiSmokeWatchReport,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("session_id: {}", response.session_id);
    println!("transcript_path: {}", response.transcript_path.display());
    println!("ingested: {}", response.ingested);
    println!(
        "pruned_missing_checkpoint: {}",
        response.pruned_missing_checkpoint
    );
    println!(
        "missing_checkpoint_count: {}",
        response.missing_checkpoint_count
    );
    Ok(())
}

pub(super) fn print_index_response(response: &IndexResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "files={} ingested={} duplicates={} parse_errors={} skipped={} unsupported={} symlinks={} unsafe_paths={} storage_blocked_chunks={} dropped_metadata_fields={} checkpoint_updates={} file_errors={}",
        response.discovered_files,
        response.ingested,
        response.skipped_dupes,
        response.parse_errors,
        response.skipped_files,
        response.unsupported_files,
        response.skipped_symlinks,
        response.skipped_unsafe_paths,
        response.storage_blocked_chunks,
        response.dropped_metadata_fields,
        response.checkpoint_updates,
        response.file_errors.len()
    );
    for error in &response.file_errors {
        eprintln!("index error: {}: {}", error.path, error.error);
    }
    Ok(())
}

pub(super) fn ensure_index_success(response: &IndexResult) -> Result<()> {
    if response.file_errors.is_empty()
        && response.storage_blocked_chunks == 0
        && response.parse_errors == 0
    {
        if response.dropped_metadata_fields > 0 {
            eprintln!(
                "warning: {} transcript metadata field(s) were dropped",
                response.dropped_metadata_fields
            );
        }
        Ok(())
    } else if response.storage_blocked_chunks > 0 {
        bail!(
            "{} transcript chunk(s) blocked by storage guardrails",
            response.storage_blocked_chunks
        )
    } else if response.parse_errors > 0 {
        bail!(
            "{} transcript record(s) failed to parse",
            response.parse_errors
        )
    } else {
        bail!(
            "{} transcript file(s) failed to index",
            response.file_errors.len()
        )
    }
}

pub(super) fn ensure_ai_doctor_success(
    response: &AiDoctorReport,
    strict_permissions: bool,
) -> Result<()> {
    if strict_permissions
        && ((response.claude_root.exists && !response.claude_root.strict_ok)
            || (response.codex_root.exists && !response.codex_root.strict_ok))
    {
        bail!("AI transcript root permission check failed");
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.chars().count() > max {
        let prefix: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{prefix}…")
    } else {
        s.to_string()
    }
}

pub(super) fn print_correlate_response(
    response: &CorrelateEventsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} event(s) across {} host(s), window {} to {}, severity >= {}{}",
        response.total_events,
        response.hosts_count,
        response.window_from,
        response.window_to,
        response.severity_min,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for host in &response.hosts {
        println!();
        println!("{} ({} event(s))", host.hostname, host.event_count);
        for log in &host.events {
            print_log(log);
        }
    }
    Ok(())
}

pub(super) fn print_stats_response(stats: &DbStats, json: bool) -> Result<()> {
    if json {
        return print_json(stats);
    }
    println!("total_logs: {}", stats.total_logs);
    println!("total_hosts: {}", stats.total_hosts);
    println!("oldest_log: {}", stats.oldest_log.as_deref().unwrap_or("-"));
    println!("newest_log: {}", stats.newest_log.as_deref().unwrap_or("-"));
    println!("logical_db_size_mb: {}", stats.logical_db_size_mb);
    println!("physical_db_size_mb: {}", stats.physical_db_size_mb);
    println!(
        "free_disk_mb: {}",
        stats.free_disk_mb.as_deref().unwrap_or("-")
    );
    println!("max_db_size_mb: {}", stats.max_db_size_mb);
    println!("min_free_disk_mb: {}", stats.min_free_disk_mb);
    println!("write_blocked: {}", stats.write_blocked);
    println!("phantom_fts_rows: {}", stats.phantom_fts_rows);
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct DbStatusReport<'a> {
    #[serde(flatten)]
    status: &'a DbMaintenanceStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    coordination: Option<&'a [SetupPhase]>,
}

pub(super) fn print_db_status_response(
    status: &DbMaintenanceStatus,
    coordination: Option<&[SetupPhase]>,
    json: bool,
) -> Result<()> {
    if json {
        let report = DbStatusReport {
            status,
            coordination,
        };
        return print_json(&report);
    }
    println!("db_path: {}", status.db_path.display());
    println!("page_count: {}", status.page_count);
    println!("freelist_count: {}", status.freelist_count);
    println!("page_size: {}", status.page_size);
    println!("logical_size_bytes: {}", status.logical_size_bytes);
    println!("physical_size_bytes: {}", status.physical_size_bytes);
    println!(
        "wal_size_bytes: {}",
        status
            .wal_size_bytes
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "shm_size_bytes: {}",
        status
            .shm_size_bytes
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!("auto_vacuum: {}", status.auto_vacuum);
    println!("journal_mode: {}", status.journal_mode);
    println!(
        "integrity_ok: {}",
        status
            .integrity_ok
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not checked".to_string())
    );
    if let Some(phases) = coordination {
        println!();
        println!("coordination:");
        for phase in phases {
            println!("  {:?} {} — {}", phase.status, phase.name, phase.detail);
        }
    }
    Ok(())
}

pub(super) fn print_db_integrity_response(response: &DbIntegrityResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("ok: {}", response.ok);
    for message in &response.messages {
        println!("{message}");
    }
    Ok(())
}

pub(super) fn print_db_checkpoint_response(
    response: &DbCheckpointResult,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("mode: {}", response.mode);
    println!("busy: {}", response.busy);
    println!("log_frames: {}", response.log_frames);
    println!("checkpointed_frames: {}", response.checkpointed_frames);
    Ok(())
}

pub(super) fn print_db_vacuum_response(response: &DbVacuumResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("full: {}", response.full);
    println!("incremental_pages: {}", response.incremental_pages);
    println!(
        "before_physical_size_bytes: {}",
        response.before_physical_size_bytes
    );
    println!(
        "after_physical_size_bytes: {}",
        response.after_physical_size_bytes
    );
    Ok(())
}

pub(super) fn print_db_backup_response(response: &DbBackupResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("db_path: {}", response.db_path.display());
    println!("backup_path: {}", response.backup_path.display());
    println!("size_bytes: {}", response.size_bytes);
    Ok(())
}

fn print_compose_status_response(status: &ComposeStatus, json: bool) -> Result<()> {
    if json {
        return print_json(status);
    }
    println!("Container: {}", status.container_name);
    if let Some(value) = &status.status {
        println!("Status: {value}");
    }
    if let Some(value) = &status.health {
        println!("Docker health: {value}");
    }
    if let Some(value) = &status.image {
        println!("Image: {value}");
    }
    if let Some(value) = &status.compose_project {
        println!("Compose project: {value}");
    }
    if let Some(value) = &status.compose_working_dir {
        println!("Compose working dir: {}", value.display());
    }
    for diag in &status.diagnostics {
        println!("{:?}: {} - {}", diag.severity, diag.code, diag.message);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct ComposeDoctorReport<'a> {
    #[serde(flatten)]
    status: &'a ComposeStatus,
    coordination: &'a [SetupPhase],
}

fn print_compose_doctor_response(
    status: &ComposeStatus,
    coordination: &[SetupPhase],
    json: bool,
) -> Result<()> {
    if json {
        let report = ComposeDoctorReport {
            status,
            coordination,
        };
        return print_json(&report);
    }
    print_compose_status_response(status, false)?;
    println!();
    println!("coordination:");
    for phase in coordination {
        println!("  {:?} {} — {}", phase.status, phase.name, phase.detail);
    }
    Ok(())
}

fn ensure_doctor_coordination_ok(phases: &[SetupPhase]) -> Result<()> {
    let failures: Vec<String> = phases
        .iter()
        .filter(|p| matches!(p.status, SetupStatus::Error))
        .map(|p| format!("{} — {}", p.name, p.detail))
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    bail!(
        "compose doctor coordination check failed: {}",
        failures.join("; ")
    );
}

fn print_compose_command_response(result: &ComposeCommandResult, json: bool) -> Result<()> {
    match result {
        ComposeCommandResult::Executed(output) => {
            if json {
                print_json(output)?;
            } else {
                print!("{}", output.stdout);
                eprint!("{}", output.stderr);
            }
            ensure_command_success(output)
        }
        ComposeCommandResult::DryRun(dry_run) => {
            if json {
                print_json(dry_run)?;
            } else {
                println!("Dry run passed: {}", dry_run.command.join(" "));
            }
            Ok(())
        }
    }
}

fn ensure_command_success(output: &CommandOutput) -> Result<()> {
    if output.exit_status == Some(0) && !output.timed_out {
        return Ok(());
    }
    bail!(
        "compose command failed: status={:?} timed_out={} stderr={}",
        output.exit_status,
        output.timed_out,
        output.stderr
    )
}

// ---------------------------------------------------------------------------
// Drift guard: host/container coordination diagnostics
// ---------------------------------------------------------------------------
//
// The CLI and the running container must agree on:
//   1. The host-side directory bind-mounted at `/data` (data-mount).
//   2. The `SYSLOG_MCP_DB_PATH` the host-side `syslog-ai-watch.service` writes
//      to via systemd `Environment` / `EnvironmentFiles`.
//
// Both checks shell out to `docker inspect` and `systemctl --user show`; in
// `compose doctor` we hit them once and share the results via `DoctorCache`
// so the multi-phase output doesn't re-fork them. This phase adds roughly
// 100-200ms per invocation, which is why `db status --check-coord` is opt-in.

/// Result of `docker inspect syslog-mcp` for the `/data` mount.
#[derive(Debug, Clone)]
struct ContainerMountInfo {
    mount_type: Option<String>,
    mount_source: Option<String>,
    running: bool,
}

#[derive(Debug, Clone)]
struct SystemctlEnv {
    /// Inline `Environment=` KEY=VALUE pairs (from `-p Environment`).
    inline: Vec<(String, String)>,
    /// Paths from `EnvironmentFiles=`.
    files: Vec<PathBuf>,
    /// True when `systemctl show` succeeded but the unit was not found.
    unit_missing: bool,
}

#[derive(Debug, Default)]
struct DoctorCache {
    container_inspect: Option<Result<ContainerMountInfo, String>>,
    systemctl_env: Option<Result<SystemctlEnv, String>>,
}

impl DoctorCache {
    fn container_inspect(&mut self, container: &str) -> Result<ContainerMountInfo, String> {
        if let Some(cached) = &self.container_inspect {
            return cached.clone();
        }
        let result = docker_inspect_data_mount(container);
        self.container_inspect = Some(result.clone());
        result
    }

    fn systemctl_env(&mut self, unit: &str) -> Result<SystemctlEnv, String> {
        if let Some(cached) = &self.systemctl_env {
            return cached.clone();
        }
        let result = systemctl_show_env(unit);
        self.systemctl_env = Some(result.clone());
        result
    }
}

/// Run both coordination phases (data-mount + ai-watch-coord) with a shared
/// cache so the underlying `docker inspect` only fires once.
pub(super) fn run_coordination_phases() -> Vec<SetupPhase> {
    let data_dir = setup_data_dir();
    let env_path = data_dir.join(".env");
    let mut cache = DoctorCache::default();
    vec![
        data_mount_phase_cached(data_dir.as_path(), env_path.as_path(), &mut cache),
        ai_watch_coordination_phase(env_path.as_path(), &mut cache),
    ]
}

fn docker_inspect_data_mount(container: &str) -> Result<ContainerMountInfo, String> {
    let output = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{range .Mounts}}{{if eq .Destination \"/data\"}}{{.Type}}|{{.Source}}{{end}}{{end}}|{{.State.Running}}",
            container,
        ])
        .output()
        .map_err(|error| format!("docker not available: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "container '{container}' not present (docker inspect failed: {})",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parts: Vec<&str> = stdout.split('|').collect();
    let running = parts.last().is_some_and(|s| *s == "true");
    if parts.len() < 3 || parts[0].is_empty() {
        return Ok(ContainerMountInfo {
            mount_type: None,
            mount_source: None,
            running,
        });
    }
    Ok(ContainerMountInfo {
        mount_type: Some(parts[0].to_string()),
        mount_source: Some(parts[1].to_string()),
        running,
    })
}

fn systemctl_show_env(unit: &str) -> Result<SystemctlEnv, String> {
    // Reuse the shared --user wrapper so we pick up the
    // DBUS_SESSION_BUS_ADDRESS / XDG_RUNTIME_DIR fallback for headless
    // hosts where the user bus isn't auto-discovered.
    let stdout = systemctl_user_output(&[
        "show",
        unit,
        "-p",
        "Environment",
        "-p",
        "EnvironmentFiles",
        "-p",
        "LoadState",
        "--no-pager",
    ])
    .map_err(|error| error.to_string())?;
    Ok(parse_systemctl_env_output(&stdout))
}

/// Parse `systemctl --user show -p Environment -p EnvironmentFiles -p LoadState`
/// output into a `SystemctlEnv`. Lines look like:
///
/// ```text
/// Environment=KEY1=val1 KEY2=val2
/// EnvironmentFiles=/etc/foo (ignore_errors=no)
/// LoadState=loaded
/// ```
///
/// Uses `split_once('=')` everywhere — values may legitimately contain `=`.
fn parse_systemctl_env_output(stdout: &str) -> SystemctlEnv {
    let mut inline = Vec::new();
    let mut files = Vec::new();
    let mut unit_missing = false;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "Environment" => {
                inline.extend(parse_systemctl_env_inline(value));
            }
            "EnvironmentFiles" => {
                for path in parse_systemctl_env_files(value) {
                    files.push(path);
                }
            }
            "LoadState" if value.trim() == "not-found" => {
                unit_missing = true;
            }
            _ => {}
        }
    }
    SystemctlEnv {
        inline,
        files,
        unit_missing,
    }
}

/// Parse the inline value of `Environment=...`. Each space-separated token is
/// a `KEY=VALUE` pair; `VALUE` may contain `=`, so we use `split_once('=')`.
fn parse_systemctl_env_inline(value: &str) -> Vec<(String, String)> {
    value
        .split_whitespace()
        .filter_map(|entry| {
            let (k, v) = entry.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// Parse the inline value of `EnvironmentFiles=...`. systemd renders this as
/// a space-separated list of `<path> (ignore_errors=<bool>)` pairs. We take
/// just the path token.
fn parse_systemctl_env_files(value: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for token in value.split_whitespace() {
        if token.starts_with('(') {
            continue;
        }
        if token.is_empty() {
            continue;
        }
        paths.push(PathBuf::from(token));
    }
    paths
}

/// Look up `SYSLOG_MCP_DB_PATH` from the systemctl-rendered env. Inline
/// `Environment=` values take precedence; otherwise we walk each
/// `EnvironmentFiles` entry. Missing files are skipped (not fatal).
fn lookup_systemd_db_path(env: &SystemctlEnv) -> Option<String> {
    if let Some((_, value)) = env.inline.iter().find(|(k, _)| k == "SYSLOG_MCP_DB_PATH") {
        return Some(value.clone());
    }
    for path in &env.files {
        if let Some(value) = read_env_value(path, "SYSLOG_MCP_DB_PATH") {
            return Some(value);
        }
    }
    None
}

// data_mount_phase (uncached wrapper) removed by bead syslog-mcp-0p8r.11.
// Sole caller was setup_report (SessionStart hook), which no longer needs it
// post-cutover. All remaining callers (compose doctor, db status --check-coord)
// use data_mount_phase_cached so the docker inspect result can be shared with
// ai_watch_coordination_phase within a single invocation.

/// Cached variant of `data_mount_phase`. See module-level note for why we
/// share `docker inspect` across phases.
fn data_mount_phase_cached(
    data_dir: &std::path::Path,
    env_path: &std::path::Path,
    cache: &mut DoctorCache,
) -> SetupPhase {
    let name = "data-mount";
    let container =
        std::env::var("SYSLOG_MCP_CONTAINER_NAME").unwrap_or_else(|_| "syslog-mcp".to_string());

    let expected_dir = std::env::var("SYSLOG_MCP_DATA_VOLUME")
        .ok()
        .or_else(|| read_env_value(env_path, "SYSLOG_MCP_DATA_VOLUME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.to_path_buf());

    let info = match cache.container_inspect(&container) {
        Ok(info) => info,
        Err(detail) => {
            // Distinguish "container absent" (Skipped per doctor spec —
            // ai-watch absent style) from "docker enumeration failed"
            // (Warn — could not enumerate inputs). docker inspect on a
            // missing container reports "No such object" / "no such
            // container"; anything else is a probe failure.
            let lower = detail.to_ascii_lowercase();
            let status = if lower.contains("no such object") || lower.contains("no such container")
            {
                SetupStatus::Skipped
            } else {
                SetupStatus::Warn
            };
            return SetupPhase {
                name,
                status,
                detail,
            };
        }
    };
    if !info.running {
        return SetupPhase {
            name,
            status: SetupStatus::Skipped,
            detail: format!("container '{container}' not running"),
        };
    }
    let Some(mount_source) = info.mount_source else {
        return SetupPhase {
            name,
            status: SetupStatus::Error,
            detail: format!("container '{container}' has no /data mount — run `syslog compose up`"),
        };
    };
    let mount_type = info.mount_type.unwrap_or_default();
    if mount_type != "bind" {
        return SetupPhase {
            name,
            status: SetupStatus::Error,
            detail: format!(
                "container /data is a {} (expected bind to {}). \
                 CLI and container are writing different DBs. \
                 Repair: `syslog compose up` (recreates with --env-file)",
                mount_type,
                expected_dir.display()
            ),
        };
    }
    let expected = match canonicalize_with_warning(&expected_dir) {
        Ok(path) => path,
        Err(detail) => {
            return SetupPhase {
                name,
                status: SetupStatus::Warn,
                detail,
            };
        }
    };
    let actual_source = PathBuf::from(&mount_source);
    let actual = match canonicalize_with_warning(&actual_source) {
        Ok(path) => path,
        Err(detail) => {
            return SetupPhase {
                name,
                status: SetupStatus::Warn,
                detail,
            };
        }
    };
    if actual != expected {
        return SetupPhase {
            name,
            status: SetupStatus::Error,
            detail: format!(
                "container /data bind source ({}) does not match SYSLOG_MCP_DATA_VOLUME ({}). \
                 CLI and container are writing different DBs. Repair: `syslog compose up`",
                mount_source,
                expected_dir.display()
            ),
        };
    }
    SetupPhase {
        name,
        status: SetupStatus::Ok,
        detail: format!(
            "bind {} -> /data matches SYSLOG_MCP_DATA_VOLUME",
            mount_source
        ),
    }
}

/// Verify the host systemd `syslog-ai-watch.service`'s effective
/// `SYSLOG_MCP_DB_PATH` resolves to the same canonical host path as the
/// container's `/data` bind source. A mismatch means the host ai-watch
/// service is writing checkpoints to a DB the container will never read.
///
/// Status semantics (per epic decisions):
/// - `Skipped` — ai-watch unit not installed or not loadable. Only valid
///   skipped reason.
/// - `Ok` — canonical paths match.
/// - `Warning` — could not enumerate the inputs (docker/systemctl failed,
///   canonicalize ENOENT/EACCES). The drift bug was a silent literal-string
///   compare fallback; we never do that — always warn with the OS error.
/// - `Error` — both sides resolved and the canonical paths differ.
fn ai_watch_coordination_phase(env_path: &std::path::Path, cache: &mut DoctorCache) -> SetupPhase {
    let name = "ai-watch-coord";
    let unit = std::env::var("SYSLOG_AI_WATCH_UNIT")
        .unwrap_or_else(|_| "syslog-ai-watch.service".to_string());
    let container =
        std::env::var("SYSLOG_MCP_CONTAINER_NAME").unwrap_or_else(|_| "syslog-mcp".to_string());

    let env = match cache.systemctl_env(&unit) {
        Ok(env) => env,
        Err(detail) => {
            // systemctl enumeration failed (binary missing, bus error,
            // permission denied, etc.); per the doctor spec this is
            // `warn` — `skipped` is reserved for "ai-watch absent".
            return SetupPhase {
                name,
                status: SetupStatus::Warn,
                detail,
            };
        }
    };
    if env.unit_missing {
        return SetupPhase {
            name,
            status: SetupStatus::Skipped,
            detail: format!("systemd unit {unit} is not installed"),
        };
    }
    let Some(ai_db_path) = lookup_systemd_db_path(&env) else {
        return SetupPhase {
            name,
            status: SetupStatus::Warn,
            detail: format!(
                "could not find SYSLOG_MCP_DB_PATH in {unit} (Environment/EnvironmentFiles)"
            ),
        };
    };
    let info = match cache.container_inspect(&container) {
        Ok(info) => info,
        Err(detail) => {
            return SetupPhase {
                name,
                status: SetupStatus::Warn,
                detail: format!("could not inspect container: {detail}"),
            };
        }
    };
    if !info.running {
        return SetupPhase {
            name,
            status: SetupStatus::Warn,
            detail: format!("container '{container}' not running"),
        };
    }
    let Some(mount_source) = info.mount_source else {
        return SetupPhase {
            name,
            status: SetupStatus::Warn,
            detail: format!("container '{container}' has no /data mount"),
        };
    };

    // ai-watch points at the SQLite *file*; container exposes the parent dir.
    let ai_path = PathBuf::from(&ai_db_path);
    let ai_dir = ai_path.parent().map(PathBuf::from).unwrap_or(ai_path);
    let canonical_ai = match canonicalize_with_warning(&ai_dir) {
        Ok(path) => path,
        Err(detail) => {
            // NEVER silently compare literal strings on canonicalize failure
            // — that was the original drift bug.
            let _ = env_path; // env_path reserved for future plugin .env cross-checks.
            return SetupPhase {
                name,
                status: SetupStatus::Warn,
                detail,
            };
        }
    };
    let mount_pathbuf = PathBuf::from(&mount_source);
    let canonical_container = match canonicalize_with_warning(&mount_pathbuf) {
        Ok(path) => path,
        Err(detail) => {
            return SetupPhase {
                name,
                status: SetupStatus::Warn,
                detail,
            };
        }
    };
    if canonical_ai == canonical_container {
        return SetupPhase {
            name,
            status: SetupStatus::Ok,
            detail: format!(
                "ai-watch SYSLOG_MCP_DB_PATH ({}) and container /data bind ({}) resolve to {}",
                ai_db_path,
                mount_source,
                canonical_ai.display()
            ),
        };
    }
    SetupPhase {
        name,
        status: SetupStatus::Error,
        detail: format!(
            "ai-watch SYSLOG_MCP_DB_PATH canonicalizes to {} but container /data bind canonicalizes to {} — \
             host service and container are writing different DBs",
            canonical_ai.display(),
            canonical_container.display()
        ),
    }
}

/// Canonicalize a path, returning a structured warning string on ENOENT /
/// EACCES instead of falling back to the literal path. The literal-fallback
/// pattern is the drift bug we're guarding against.
fn canonicalize_with_warning(path: &std::path::Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path)
        .map_err(|err| format!("could not canonicalize {}: {err}", path.display()))
}

// REST HTTP client used by --http subcommand mode (bead 0p8r.5). The module
// lives under `src/cli/http_client.rs`; Rust 2018+ auto-resolves `mod foo;`
// in `src/cli.rs` to either `src/cli/foo.rs` or `src/cli/foo/mod.rs`.
#[allow(dead_code)]
pub(crate) mod http_client;

// Per-arm dispatch for query commands (bead 0p8r.7). Holds the `run_X` free
// functions, the `http_or_cancel` SIGINT helper, and `Cli*Args::into_request`
// conversions shared between the Local and HTTP arms.
pub(crate) mod dispatch;

// ─── Global flag plumbing (bead 0p8r.6) ─────────────────────────────────────

/// Env var that opts a process into HTTP transport without passing `--http`.
/// Accepts `1` or `true` (case-insensitive). Any other value is treated as
/// unset to avoid surprising "I typoed `falze`" silent flips.
pub(crate) const ENV_USE_HTTP: &str = "SYSLOG_USE_HTTP";

/// Global CLI flags that apply to every subcommand. Stripped from the raw
/// arg list by [`GlobalFlags::extract`] **before** subcommand parsing so the
/// per-command parsers (which we did NOT touch in this bead) keep matching
/// only the flags they already know about.
///
/// `--http` is a bare bool. `--server` and `--token` accept either a separate
/// arg (`--server URL`) or `=`-glued form (`--server=URL`). Passing `--server`
/// or `--token` implies HTTP mode even without an explicit `--http`.
///
/// `SYSLOG_API_TOKEN` alone does **not** flip the default to HTTP — that
/// would silently change behaviour for users who already exported the token
/// from earlier deploys (locked decision, eng-review #C6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GlobalFlags {
    pub force_http: bool,
    pub server: Option<String>,
    pub token: Option<String>,
}

impl GlobalFlags {
    /// Strip global flags out of `args` in-place and return them.
    ///
    /// Unknown args are left in place untouched so the existing per-subcommand
    /// parsers see exactly what they used to. We deliberately allow both
    /// `syslog --http search foo` and `syslog search --http foo` — the
    /// stripper walks the whole vec, not just a prefix.
    ///
    /// `--server` / `--token` without a following value error out; an empty
    /// value (e.g. `--token=`) is also an error so a stray trailing `=` does
    /// not silently produce HTTP mode with a blank token.
    pub(crate) fn extract(args: &mut Vec<String>) -> Result<Self> {
        let mut out = GlobalFlags::default();
        let mut i = 0;
        while i < args.len() {
            // Two flag families: bare "--http", and value-bearing
            // "--server"/"--token" which accept "--flag VALUE" or "--flag=VALUE".
            let arg = args[i].as_str();
            if arg == "--http" {
                out.force_http = true;
                args.remove(i);
                continue;
            }
            if let Some(value) = strip_eq_prefix(arg, "--server") {
                if value.is_empty() {
                    bail!("--server requires a value");
                }
                out.server = Some(value.to_string());
                args.remove(i);
                continue;
            }
            if arg == "--server" {
                if i + 1 >= args.len() {
                    bail!("--server requires a value");
                }
                let value = args.remove(i + 1);
                if value.trim().is_empty() {
                    bail!("--server requires a non-empty value");
                }
                out.server = Some(value);
                args.remove(i);
                continue;
            }
            if let Some(value) = strip_eq_prefix(arg, "--token") {
                if value.is_empty() {
                    bail!("--token requires a value");
                }
                out.token = Some(value.to_string());
                args.remove(i);
                continue;
            }
            if arg == "--token" {
                if i + 1 >= args.len() {
                    bail!("--token requires a value");
                }
                let value = args.remove(i + 1);
                if value.trim().is_empty() {
                    bail!("--token requires a non-empty value");
                }
                out.token = Some(value);
                args.remove(i);
                continue;
            }
            i += 1;
        }
        Ok(out)
    }

    /// Returns `Some(trigger_label)` if HTTP mode was requested via any of:
    /// `--http`, `--server`, `--token`, or `SYSLOG_USE_HTTP=1|true`. Returns
    /// `None` for the default Local mode. The label is the literal flag the
    /// user passed, used verbatim in error messages.
    ///
    /// Note: `SYSLOG_API_TOKEN` being set does NOT trigger HTTP — only the
    /// explicit opt-ins above do (locked decision).
    pub(crate) fn http_trigger(&self) -> Option<&'static str> {
        if let Some(flag) = self.http_flag_trigger() {
            return Some(flag);
        }
        if env_opts_into_http() {
            return Some("SYSLOG_USE_HTTP=1");
        }
        None
    }

    /// Like [`http_trigger`] but only considers explicit command-line FLAGS,
    /// ignoring the `SYSLOG_USE_HTTP` env var. Used by local-only commands
    /// (`compose`, `setup`) that must not bail just because operators have
    /// `SYSLOG_USE_HTTP=true` written into `~/.syslog-mcp/.env`.
    pub(crate) fn http_flag_trigger(&self) -> Option<&'static str> {
        if self.force_http {
            return Some("--http");
        }
        if self.server.is_some() {
            return Some("--server");
        }
        if self.token.is_some() {
            return Some("--token");
        }
        None
    }

    /// Build an [`HttpClient`] from these flags. On discovery failure, wraps
    /// the underlying error with a prefix naming the trigger so the operator
    /// knows exactly which knob put them into HTTP mode — this is the
    /// fail-closed contract from eng-review #C6.
    pub(crate) fn build_http_client(
        &self,
        trigger: &'static str,
    ) -> Result<http_client::HttpClient> {
        http_client::HttpClient::discover(self.server.clone(), self.token.clone())
            .map_err(|err| anyhow!("HTTP mode requested via {trigger} but discovery failed: {err}"))
    }
}

/// Returns `true` when `SYSLOG_USE_HTTP` is set to `1` or `true`
/// (case-insensitive). Any other value — including empty string, `0`, `false`,
/// or typos — is treated as unset.
fn env_opts_into_http() -> bool {
    match std::env::var(ENV_USE_HTTP) {
        Ok(v) => {
            let v = v.trim();
            v.eq_ignore_ascii_case("1") || v.eq_ignore_ascii_case("true")
        }
        Err(_) => false,
    }
}

/// If `arg` matches `flag=...` return the suffix; otherwise `None`.
fn strip_eq_prefix<'a>(arg: &'a str, flag: &str) -> Option<&'a str> {
    arg.strip_prefix(flag)
        .and_then(|rest| rest.strip_prefix('='))
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
