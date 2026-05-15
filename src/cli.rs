use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::path::PathBuf;
use syslog_mcp::app::{
    AiCorrelateRequest, AiCorrelateResponse, CorrelateEventsRequest, CorrelateEventsResponse,
    CussSearchRequest, CussSearchResponse, DbBackupResult, DbCheckpointResult, DbIntegrityResult,
    DbMaintenanceStatus, DbStats, DbVacuumResult, GetErrorsRequest, GetErrorsResponse,
    ListAiProjectsRequest, ListAiProjectsResponse, ListAiToolsRequest, ListAiToolsResponse,
    ListHostsResponse, LogEntry, ProjectContextRequest, ProjectContextResponse, SearchLogsRequest,
    SearchLogsResponse, SearchSessionsRequest, SearchSessionsResponse, SyslogService,
    TailLogsRequest, UsageBlocksRequest, UsageBlocksResponse,
};
use syslog_mcp::compose::{
    CliDockerInspect, CommandOutput, ComposeCommandResult, ComposeDefaults, ComposeMutation,
    ComposeService, ComposeStatus, ComposeTarget, MutationOptions, ProcessRunner,
};
use syslog_mcp::scanner::{
    AiDoctorReport, CheckpointEntry, IndexResult, ParseErrorEntry, PruneCheckpointsResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CliCommand {
    Search(SearchArgs),
    Tail(TailArgs),
    Errors(TimeRangeArgs),
    Hosts(OutputArgs),
    Sessions(SessionsArgs),
    Ai(AiCommand),
    Correlate(CorrelateArgs),
    Stats(OutputArgs),
    Compose(ComposeCommand),
    Db(DbCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AiCommand {
    Search(AiSearchArgs),
    Cuss(AiCussArgs),
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
pub(crate) enum DbCommand {
    Status(OutputArgs),
    Integrity(OutputArgs),
    Checkpoint(DbCheckpointArgs),
    Vacuum(DbVacuumArgs),
    Backup(DbBackupArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OutputArgs {
    pub json: bool,
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
    pub json: bool,
}

impl Default for DbVacuumArgs {
    fn default() -> Self {
        Self {
            full: false,
            pages: 1000,
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
    pub from: Option<String>,
    pub to: Option<String>,
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
pub(crate) struct AiCussArgs {
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
            "ai" => parse_ai(rest),
            "correlate" => parse_correlate(rest),
            "stats" => parse_stats(rest),
            "compose" => parse_compose(rest),
            "db" => parse_db(rest),
            _ => bail!("unknown CLI command: {command}"),
        }
    }
}

pub(crate) async fn run(service: SyslogService, command: CliCommand) -> Result<()> {
    match command {
        CliCommand::Search(args) => {
            let json = args.json;
            let response = service
                .search_logs(SearchLogsRequest {
                    query: args.query,
                    hostname: args.hostname,
                    source_ip: args.source_ip,
                    severity: args.severity,
                    app_name: args.app_name,
                    facility: None,
                    process_id: None,
                    from: args.from,
                    to: args.to,
                    limit: args.limit,
                })
                .await?;
            print_search_response(&response, json)?;
        }
        CliCommand::Tail(args) => {
            let json = args.json;
            let response = service
                .tail_logs(TailLogsRequest {
                    hostname: args.hostname,
                    source_ip: args.source_ip,
                    app_name: args.app_name,
                    severity_min: None,
                    n: args.n,
                })
                .await?;
            print_search_response(&response, json)?;
        }
        CliCommand::Errors(args) => {
            let json = args.json;
            let response = service
                .get_errors(GetErrorsRequest {
                    from: args.from,
                    to: args.to,
                    group_by: None,
                })
                .await?;
            print_errors_response(&response, json)?;
        }
        CliCommand::Hosts(args) => {
            let response = service.list_hosts().await?;
            print_hosts_response(&response, args.json)?;
        }
        CliCommand::Sessions(args) => {
            let json = args.json;
            let response = service
                .list_sessions(syslog_mcp::app::ListSessionsRequest {
                    project: args.project,
                    tool: args.tool,
                    hostname: args.hostname,
                    from: args.from,
                    to: args.to,
                    limit: args.limit,
                })
                .await?;
            print_sessions_response(&response, json)?;
        }
        CliCommand::Ai(args) => match args {
            AiCommand::Search(args) => {
                let json = args.json;
                let response = service
                    .search_sessions(SearchSessionsRequest {
                        query: args.query,
                        project: args.project,
                        tool: args.tool,
                        from: args.from,
                        to: args.to,
                        limit: args.limit,
                    })
                    .await?;
                print_search_sessions_response(&response, json)?;
            }
            AiCommand::Cuss(args) => {
                let json = args.json;
                let response = service
                    .search_cusses(CussSearchRequest {
                        project: args.project,
                        tool: args.tool,
                        from: args.from,
                        to: args.to,
                        limit: args.limit,
                        before: args.before,
                        after: args.after,
                        terms: args.terms,
                    })
                    .await?;
                print_cuss_search_response(&response, json)?;
            }
            AiCommand::Correlate(args) => {
                let json = args.json;
                let response = service
                    .correlate_ai_logs(AiCorrelateRequest {
                        project: args.project,
                        tool: args.tool,
                        session_id: args.session_id,
                        ai_query: args.ai_query,
                        log_query: args.log_query,
                        hostname: args.hostname,
                        source_ip: args.source_ip,
                        app_name: args.app_name,
                        from: args.from,
                        to: args.to,
                        window_minutes: args.window_minutes,
                        severity_min: args.severity_min,
                        limit: args.limit,
                        events_per_anchor: args.events_per_anchor,
                    })
                    .await?;
                print_ai_correlate_response(&response, json)?;
            }
            AiCommand::Blocks(args) => {
                let json = args.json;
                let response = service
                    .usage_blocks(UsageBlocksRequest {
                        project: args.project,
                        tool: args.tool,
                        from: args.from,
                        to: args.to,
                    })
                    .await?;
                print_usage_blocks_response(&response, json)?;
            }
            AiCommand::Context(args) => {
                let json = args.json;
                let response = service
                    .project_context(ProjectContextRequest {
                        project: args.project,
                        tool: args.tool,
                        limit: args.limit,
                    })
                    .await?;
                print_project_context_response(&response, json)?;
            }
            AiCommand::Tools(args) => {
                let json = args.json;
                let response = service
                    .list_ai_tools(ListAiToolsRequest {
                        project: args.project,
                        from: args.from,
                        to: args.to,
                    })
                    .await?;
                print_ai_tools_response(&response, json)?;
            }
            AiCommand::Projects(args) => {
                let json = args.json;
                let response = service
                    .list_ai_projects(ListAiProjectsRequest {
                        tool: args.tool,
                        from: args.from,
                        to: args.to,
                    })
                    .await?;
                print_ai_projects_response(&response, json)?;
            }
            AiCommand::Index(args) => {
                let response = service
                    .index_ai_roots(args.path, args.force, args.since)
                    .await?;
                print_index_response(&response, args.json)?;
                ensure_index_success(&response)?;
            }
            AiCommand::Add(args) => {
                let response = service.add_ai_file(args.file, args.force).await?;
                print_index_response(&response, args.json)?;
                ensure_index_success(&response)?;
            }
            AiCommand::Watch(args) => {
                let options = syslog_mcp::ai_watch::WatchOptions {
                    path: args.path.map(std::path::PathBuf::from),
                    debounce: std::time::Duration::from_millis(args.debounce_ms),
                    settle: std::time::Duration::from_millis(args.settle_ms),
                    max_retries: args.max_retries,
                    initial_scan: !args.no_initial_scan,
                    json: args.json,
                };
                syslog_mcp::ai_watch::run(service, options).await?;
            }
            AiCommand::Checkpoints(args) => {
                let response = service
                    .list_ai_checkpoints(args.errors_only, args.missing_only, args.limit)
                    .await?;
                print_checkpoints_response(&response, args.json)?;
            }
            AiCommand::Errors(args) => {
                let response = service.list_ai_parse_errors(args.limit).await?;
                print_ai_parse_errors_response(&response, args.json)?;
            }
            AiCommand::PruneCheckpoints(args) => {
                let response = service
                    .prune_ai_checkpoints(args.missing_only, args.dry_run, args.limit)
                    .await?;
                print_prune_checkpoints_response(&response, args.json)?;
            }
            AiCommand::Doctor(args) => {
                let response = service.ai_doctor().await?;
                print_ai_doctor_response(&response, args.json)?;
                ensure_ai_doctor_success(&response, args.strict_permissions)?;
            }
            AiCommand::WatchStatus(args) => {
                let response = ai_watch_status()?;
                print_ai_watch_status_response(&response, args.json)?;
            }
            AiCommand::SmokeWatch(args) => {
                let response = ai_smoke_watch(&service).await?;
                print_ai_smoke_watch_response(&response, args.json)?;
                if !response.pruned_missing_checkpoint {
                    bail!("AI watch smoke checkpoint was not pruned within 30s");
                }
            }
        },
        CliCommand::Correlate(args) => {
            let json = args.json;
            let response = service
                .correlate_events(CorrelateEventsRequest {
                    reference_time: args.reference_time,
                    window_minutes: args.window_minutes,
                    severity_min: args.severity_min,
                    hostname: args.hostname,
                    source_ip: args.source_ip,
                    query: args.query,
                    limit: args.limit,
                })
                .await?;
            print_correlate_response(&response, json)?;
        }
        CliCommand::Stats(args) => {
            let response = service.get_stats().await?;
            print_stats_response(&response, args.json)?;
        }
        CliCommand::Db(args) => match args {
            DbCommand::Status(args) => {
                let response = service.db_status().await?;
                print_db_status_response(&response, args.json)?;
            }
            DbCommand::Integrity(args) => {
                let response = service.db_integrity().await?;
                print_db_integrity_response(&response, args.json)?;
                if !response.ok {
                    bail!("database integrity check failed");
                }
            }
            DbCommand::Checkpoint(args) => {
                let response = service.db_checkpoint(args.mode).await?;
                print_db_checkpoint_response(&response, args.json)?;
                if response.busy != 0 {
                    bail!("database WAL checkpoint was busy");
                }
            }
            DbCommand::Vacuum(args) => {
                let response = service.db_vacuum(args.full, args.pages).await?;
                print_db_vacuum_response(&response, args.json)?;
            }
            DbCommand::Backup(args) => {
                let response = service.db_backup(args.output.map(PathBuf::from)).await?;
                print_db_backup_response(&response, args.json)?;
            }
        },
        CliCommand::Compose(_) => {
            bail!("compose commands must run through run_compose");
        }
    }
    Ok(())
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
            print_compose_status_response(&status, args.json)?;
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
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
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
            _ if arg.starts_with('-') => bail!("unknown search option: {arg}"),
            _ => query.push(arg),
        }
    }
    parsed.query = (!query.is_empty()).then(|| query.join(" "));
    Ok(CliCommand::Search(parsed))
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
        "cuss" => parse_ai_cuss(rest),
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

fn parse_ai_cuss(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiCussArgs::default();
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
            _ if arg.starts_with('-') => bail!("unknown ai cuss option: {arg}"),
            _ => bail!("unexpected ai cuss argument: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Cuss(parsed)))
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
struct AiWatchStatusReport {
    service: String,
    active: Option<String>,
    enabled: Option<String>,
    main_pid: Option<u32>,
    exec_start: Option<String>,
    latest_journal: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AiSmokeWatchReport {
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

async fn ai_smoke_watch(service: &SyslogService) -> Result<AiSmokeWatchReport> {
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
            .search_sessions(SearchSessionsRequest {
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
        if pruned_missing_checkpoint || result.matched == 0 {
            pruned_missing_checkpoint = true;
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

fn ai_watch_status() -> Result<AiWatchStatusReport> {
    const SERVICE: &str = "syslog-ai-watch.service";
    let active = systemctl_user_output(&["is-active", SERVICE]).ok();
    let enabled = systemctl_user_output(&["is-enabled", SERVICE]).ok();
    let main_pid = systemctl_user_output(&["show", "-p", "MainPID", "--value", SERVICE])
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid > 0);
    let exec_start = systemctl_user_output(&["show", "-p", "ExecStart", "--value", SERVICE]).ok();
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
        "status" => Ok(CliCommand::Db(DbCommand::Status(parse_output_args(
            "db status",
            rest,
        )?))),
        "integrity" => Ok(CliCommand::Db(DbCommand::Integrity(parse_output_args(
            "db integrity",
            rest,
        )?))),
        "checkpoint" => parse_db_checkpoint(rest),
        "vacuum" => parse_db_vacuum(rest),
        "backup" => parse_db_backup(rest),
        _ => bail!("unknown db subcommand: {subcommand}"),
    }
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

fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_search_response(response: &SearchLogsResponse, json: bool) -> Result<()> {
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

fn print_errors_response(response: &GetErrorsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("HOST                 SEVERITY COUNT");
    for row in &response.summary {
        println!("{:<20} {:<8} {}", row.hostname, row.severity, row.count);
    }
    Ok(())
}

fn print_hosts_response(response: &ListHostsResponse, json: bool) -> Result<()> {
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

fn print_sessions_response(
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

fn print_search_sessions_response(response: &SearchSessionsResponse, json: bool) -> Result<()> {
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

fn print_cuss_search_response(response: &CussSearchResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} cuss match(es) from {} candidate row(s){}",
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
            "cuss scan capped at {} candidate rows; use --project, --tool, --from, or --to to narrow it",
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

fn print_ai_correlate_response(response: &AiCorrelateResponse, json: bool) -> Result<()> {
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

fn print_usage_blocks_response(response: &UsageBlocksResponse, json: bool) -> Result<()> {
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

fn print_project_context_response(response: &ProjectContextResponse, json: bool) -> Result<()> {
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

fn print_ai_tools_response(response: &ListAiToolsResponse, json: bool) -> Result<()> {
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

fn print_ai_projects_response(response: &ListAiProjectsResponse, json: bool) -> Result<()> {
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

fn print_checkpoints_response(response: &[CheckpointEntry], json: bool) -> Result<()> {
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

fn print_ai_parse_errors_response(response: &[ParseErrorEntry], json: bool) -> Result<()> {
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

fn print_prune_checkpoints_response(response: &PruneCheckpointsResult, json: bool) -> Result<()> {
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

fn print_ai_doctor_response(response: &AiDoctorReport, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("db_path: {}", response.db_path);
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

fn print_ai_watch_status_response(response: &AiWatchStatusReport, json: bool) -> Result<()> {
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
    if !response.latest_journal.is_empty() {
        println!("latest_journal:");
        for line in &response.latest_journal {
            println!("  {line}");
        }
    }
    Ok(())
}

fn print_ai_smoke_watch_response(response: &AiSmokeWatchReport, json: bool) -> Result<()> {
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

fn print_index_response(response: &IndexResult, json: bool) -> Result<()> {
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

fn ensure_index_success(response: &IndexResult) -> Result<()> {
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

fn ensure_ai_doctor_success(response: &AiDoctorReport, strict_permissions: bool) -> Result<()> {
    if strict_permissions && (!response.claude_root.strict_ok || !response.codex_root.strict_ok) {
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

fn print_correlate_response(response: &CorrelateEventsResponse, json: bool) -> Result<()> {
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

fn print_stats_response(stats: &DbStats, json: bool) -> Result<()> {
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

fn print_db_status_response(status: &DbMaintenanceStatus, json: bool) -> Result<()> {
    if json {
        return print_json(status);
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
    Ok(())
}

fn print_db_integrity_response(response: &DbIntegrityResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("ok: {}", response.ok);
    for message in &response.messages {
        println!("{message}");
    }
    Ok(())
}

fn print_db_checkpoint_response(response: &DbCheckpointResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("mode: {}", response.mode);
    println!("busy: {}", response.busy);
    println!("log_frames: {}", response.log_frames);
    println!("checkpointed_frames: {}", response.checkpointed_frames);
    Ok(())
}

fn print_db_vacuum_response(response: &DbVacuumResult, json: bool) -> Result<()> {
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

fn print_db_backup_response(response: &DbBackupResult, json: bool) -> Result<()> {
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

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
