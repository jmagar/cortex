use anyhow::Result;
use axum::Router;
use cortex::{api, doctor, logging, mcp, runtime::RuntimeCore};
use rmcp::{transport::stdio, ServiceExt};
use tracing::info;

mod cli;

#[tokio::main]
async fn main() -> Result<()> {
    let mode = Mode::parse(std::env::args().skip(1).collect())?;
    if mode == Mode::Help {
        print_usage();
        return Ok(());
    }
    if mode == Mode::Version {
        println!("cortex {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    logging::init(mode.default_log_filter());

    info!("cortex v{}", env!("CARGO_PKG_VERSION"));

    match mode {
        Mode::ServeMcp => serve_mcp().await,
        Mode::StdioMcp => serve_stdio_mcp().await,
        Mode::Cli(invocation) => run_cli(*invocation).await,
        Mode::Setup(command) => run_setup(command).await,
        Mode::Deploy(command) => run_deploy(command).await,
        Mode::DoctorBinary(command) => doctor::run_binary_doctor(command.json).await,
        Mode::DoctorFull(command) => doctor::run_full_doctor(command.json).await,
        Mode::Help => unreachable!("handled before logging initialization"),
        Mode::Version => unreachable!("handled before logging initialization"),
    }
}

async fn serve_stdio_mcp() -> Result<()> {
    let runtime = RuntimeCore::load_query_only().await?;
    let service = mcp::rmcp_server(runtime.mcp_state()).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

async fn run_cli(invocation: CliInvocation) -> Result<()> {
    let CliInvocation { command, flags } = invocation;

    // `compose`, `setup`, and `service` stay on the local-only path: they
    // manage local host state (systemd units, Docker compose stacks, on-disk
    // config, user journal logs) that has no HTTP analogue. Reject explicit
    // HTTP-mode FLAGS up front, but
    // silently ignore the `CORTEX_USE_HTTP` env trigger — `setup repair`
    // writes that into `~/.cortex/.env` as the post-cutover default, and
    // bailing on it would break the very command operators run to repair.
    if matches!(
        command,
        cli::CliCommand::Compose(_) | cli::CliCommand::Setup(_) | cli::CliCommand::Service(_)
    ) {
        if let Some(trigger) = flags.http_flag_trigger() {
            let command_name = match command {
                cli::CliCommand::Compose(_) => "compose",
                cli::CliCommand::Setup(_) => "setup",
                cli::CliCommand::Service(_) => "service",
                _ => unreachable!("guarded by matches! above"),
            };
            anyhow::bail!(
                "{} has no effect on `{}` (local-only command); remove --http / --server / --token",
                trigger,
                command_name,
            );
        }
        return match command {
            cli::CliCommand::Compose(_) => cli::run_compose(command),
            // `service` is a pure-journal surface — don't open SQLite for it.
            // The watcher/DB might be the very thing the operator is debugging.
            cli::CliCommand::Service(_) => cli::run_service_no_db(command).await,
            cli::CliCommand::Setup(cmd) => cli::run_setup(cmd),
            _ => unreachable!("guarded by matches! above"),
        };
    }

    if let cli::CliCommand::Config(command) = command {
        return cli::run_config(command);
    }

    if let cli::CliCommand::AgentCommand(cli::AgentCommandCommand::Wrap(args)) = command {
        if let Some(trigger) = flags.http_flag_trigger() {
            anyhow::bail!(
                "{} has no effect on `agent-command wrap` (wrapper command); remove --http / --server / --token",
                trigger
            );
        }
        let code = cli::run_agent_command_wrap(args)?;
        std::process::exit(code);
    }

    if let cli::CliCommand::Heartbeat(mut command) = command {
        if flags.force_http {
            anyhow::bail!(
                "--http has no effect on `heartbeat agent`; use --target/--server and --token"
            );
        }
        match &mut command {
            cli::HeartbeatCommand::Agent(args) => {
                if args.target.is_none() {
                    args.target = flags.server;
                }
                if args.token.is_none() {
                    args.token = flags.token;
                }
            }
        }
        return cli::run_heartbeat_no_db(command).await;
    }

    if matches!(
        command,
        cli::CliCommand::Shell(_)
            | cli::CliCommand::AgentCommand(cli::AgentCommandCommand::IngestSpool(_))
    ) {
        if let Some(trigger) = flags.http_flag_trigger() {
            anyhow::bail!(
                "{} has no effect on local agent commands; remove --http / --server / --token",
                trigger
            );
        }
        let runtime = RuntimeCore::load_query_only().await?;
        return cli::run(cli::CliMode::Local(runtime.service()), command).await;
    }

    // Build CliMode ONCE per invocation, matching the per-invocation reqwest
    // Client rule from bead .5. For Local mode we lazily load the runtime so
    // HTTP-mode invocations don't pay the SQLite-open cost.
    let mode = match flags.http_trigger() {
        Some(trigger) => cli::CliMode::Http(flags.build_http_client(trigger)?),
        None => {
            let runtime = RuntimeCore::load_query_only().await?;
            cli::CliMode::Local(runtime.service())
        }
    };
    cli::run(mode, command).await
}

async fn run_deploy(command: DeployCommand) -> Result<()> {
    let (mode, label) = match command.kind {
        DeployCommandKind::Preflight => (cortex::setup::SetupMode::Check, "preflight"),
        DeployCommandKind::Local { dry_run: true } => {
            (cortex::setup::SetupMode::Check, "local dry-run")
        }
        DeployCommandKind::Local { dry_run: false } => (cortex::setup::SetupMode::Repair, "local"),
        DeployCommandKind::Remote { host, dry_run } => {
            let report = cortex::deploy::run_remote_deploy(&host, dry_run)?;
            if command.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("cortex deploy remote {host}");
                println!("mode: {}", report.mode);
                println!("host: {}", report.host);
                println!("home: {}", report.home);
                println!("env: {}", report.env_path);
                println!("compose: {}", report.compose_dir);
                println!("data: {}", report.data_dir);
                println!("health: {}", report.health_url);
                println!("mcp: {}", report.mcp_url);
                for phase in &report.phases {
                    println!(
                        "{:?}\t{}\t{}ms\t{}",
                        phase.status, phase.name, phase.elapsed_ms, phase.detail
                    );
                }
            }
            if report.has_errors {
                anyhow::bail!("cortex deploy remote {host} completed with failed phases");
            }
            return Ok(());
        }
    };
    let report = cortex::setup::run_setup(mode).await?;
    if command.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("cortex deploy {label}");
        println!("mode: {}", report.mode);
        println!("home: {}", report.home.display());
        println!("env: {}", report.env_path.display());
        println!("compose: {}", report.compose_dir.display());
        println!("data: {}", report.data_dir.display());
        println!("health: {}", report.health_url);
        println!("mcp: {}", report.mcp_url);
        for phase in &report.phases {
            println!(
                "{:?}\t{}\t{}ms\t{}",
                phase.status, phase.name, phase.elapsed_ms, phase.detail
            );
        }
    }
    if report.has_errors {
        anyhow::bail!("cortex deploy {label} completed with failed phases");
    }
    Ok(())
}

async fn run_setup(command: SetupCommand) -> Result<()> {
    let report = match command.kind {
        SetupCommandKind::Main(mode) => cortex::setup::run_setup(mode).await?,
        SetupCommandKind::AiIndexTimer(action) => {
            cortex::setup::run_ai_index_timer_setup(action).await?
        }
        SetupCommandKind::AiWatchService(action) => {
            cortex::setup::run_ai_watch_service_setup(action).await?
        }
        SetupCommandKind::AgentCommand(action) => {
            cortex::setup::run_agent_command_setup(action).await?
        }
        SetupCommandKind::HeartbeatAgent(action) => {
            cortex::setup::run_heartbeat_agent_setup(action).await?
        }
        SetupCommandKind::DebugWrapper(action) => {
            cortex::setup::run_debug_wrapper_setup(action).await?
        }
        SetupCommandKind::DebugCompose(action) => {
            cortex::setup::run_debug_compose_setup(action).await?
        }
        SetupCommandKind::Doctor => cortex::setup::run_setup_doctor().await?,
    };
    if command.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("cortex setup mode: {}", report.mode);
        println!("home: {}", report.home.display());
        println!("env: {}", report.env_path.display());
        println!("compose: {}", report.compose_dir.display());
        println!("data: {}", report.data_dir.display());
        println!("health: {}", report.health_url);
        println!("mcp: {}", report.mcp_url);
        for phase in &report.phases {
            println!(
                "{:?}\t{}\t{}ms\t{}",
                phase.status, phase.name, phase.elapsed_ms, phase.detail
            );
        }
    }
    if report.has_errors {
        anyhow::bail!("cortex setup completed with failed phases");
    }
    Ok(())
}

fn parse_doctor_full_command(args: &[String]) -> Result<DoctorFullCommand> {
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown doctor argument: {other}"),
        }
    }
    Ok(DoctorFullCommand { json })
}

async fn serve_mcp() -> Result<()> {
    let runtime = RuntimeCore::load().await?;
    info!(
        syslog_bind = %runtime.config.receiver.bind_addr(),
        mcp_bind = %runtime.config.mcp.bind_addr(),
        db_path = %runtime.config.storage.db_path.display(),
        retention_days = runtime.config.storage.retention_days,
        max_db_size_mb = runtime.config.storage.max_db_size_mb,
        recovery_db_size_mb = runtime.config.storage.recovery_db_size_mb,
        min_free_disk_mb = runtime.config.storage.min_free_disk_mb,
        recovery_free_disk_mb = runtime.config.storage.recovery_free_disk_mb,
        cleanup_interval_secs = runtime.config.storage.cleanup_interval_secs,
        pool_size = runtime.config.storage.pool_size,
        wal_mode = runtime.config.storage.wal_mode,
        mcp_auth_enabled = runtime.config.mcp.api_token.is_some(),
        docker_ingest_enabled = runtime.config.docker_ingest.enabled,
        docker_ingest_hosts = runtime.config.docker_ingest.hosts.len(),
        "Configuration loaded"
    );

    runtime.start_syslog().await?;
    let maintenance = runtime.spawn_maintenance_tasks();

    let mut app: Router = mcp::router(runtime.mcp_state());
    // /api/* is always-on. The container fails to start without
    // CORTEX_API_TOKEN — `api::router` enforces that explicitly with a
    // recovery hint pointing at `cortex setup repair`.
    {
        let api_state = api::ApiState::new(
            runtime.service(),
            runtime.config.api.clone(),
            runtime.config.mcp.port,
            cortex::config::mcp_bind_is_loopback(&runtime.config),
            runtime.config.mcp.allowed_origins.clone(),
            runtime.auth_policy().clone(),
            runtime.pool(),
            runtime.config.mcp.static_token_is_admin,
        )?;
        app = app.merge(api::router(api_state)?);
        info!("Non-MCP API mounted under /api");
    }
    if cortex::config::api_token_plaintext_exposure(&runtime.config) {
        tracing::warn!(
            bind = %runtime.config.mcp.bind_addr(),
            public_url = ?runtime.config.mcp.auth.public_url,
            "CORTEX_API_TOKEN will traverse the wire in plaintext: non-loopback bind with no \
             https:// public URL configured. Front the listener with a TLS-terminating reverse \
             proxy (e.g. SWAG) and set CORTEX_PUBLIC_URL=https://..."
        );
    }
    app = app.merge(runtime.otlp_router());
    info!("OTLP receiver mounted at /v1/logs (and /v1/metrics, /v1/traces → 404)");
    app = app.merge(runtime.heartbeat_router());
    info!("Heartbeat receiver mounted at /v1/heartbeats");
    if runtime.config.mcp.api_token.is_none() && !runtime.config.mcp.host.starts_with("127.") {
        tracing::warn!(
            bind = %runtime.config.mcp.bind_addr(),
            "OTLP /v1/logs and heartbeat /v1/heartbeats are mounted WITHOUT authentication on a \
             non-loopback bind. Anyone reachable on this address can write telemetry. \
             Set CORTEX_TOKEN to require Bearer auth."
        );
    }
    app = app.layer(tower_http::trace::TraceLayer::new_for_http());

    let mcp_bind = runtime.config.mcp.bind_addr();
    let listener = tokio::net::TcpListener::bind(&mcp_bind).await?;
    info!(bind = %mcp_bind, "MCP server listening");

    // OTLP handler needs ConnectInfo<SocketAddr> for source_ip provenance.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    // HTTP connections are drained. Now drain in order:
    // 1. Cooperatively cancel maintenance background tasks (purge, storage,
    //    error_scan); wait up to 10 s for them to exit cleanly.
    // 2. Drain the ingest pipeline (batch writer flush) then checkpoint WAL.
    info!("HTTP server stopped; shutting down maintenance tasks");
    maintenance
        .shutdown(std::time::Duration::from_secs(10))
        .await;
    info!("Maintenance tasks done; draining ingest pipeline");
    runtime.shutdown(std::time::Duration::from_secs(5)).await;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    ServeMcp,
    StdioMcp,
    Cli(Box<CliInvocation>),
    Setup(SetupCommand),
    Deploy(DeployCommand),
    DoctorBinary(DoctorBinaryCommand),
    DoctorFull(DoctorFullCommand),
    Help,
    Version,
}

/// Pairs a parsed [`cli::CliCommand`] with the [`cli::GlobalFlags`] that came
/// alongside it on the command line. Built by [`Mode::parse`] so `run_cli`
/// can construct the [`cli::CliMode`] (Local vs Http) exactly once per
/// invocation — matching the per-invocation `reqwest::Client` rule from
/// bead .5.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CliInvocation {
    command: cli::CliCommand,
    flags: cli::GlobalFlags,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorFullCommand {
    json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetupCommand {
    kind: SetupCommandKind,
    json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeployCommand {
    kind: DeployCommandKind,
    json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeployCommandKind {
    Preflight,
    Local { dry_run: bool },
    Remote { host: String, dry_run: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SetupCommandKind {
    Main(cortex::setup::SetupMode),
    AiIndexTimer(cortex::setup::AiIndexTimerAction),
    AiWatchService(cortex::setup::AiWatchServiceAction),
    AgentCommand(cortex::setup::AgentCommandAction),
    HeartbeatAgent(cortex::setup::HeartbeatAgentAction),
    DebugWrapper(cortex::setup::DebugWrapperAction),
    DebugCompose(cortex::setup::DebugComposeAction),
    Doctor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorBinaryCommand {
    json: bool,
}

impl Mode {
    fn parse(args: Vec<String>) -> Result<Self> {
        // `--help` / `--version` MUST bypass everything else: no global flag
        // extraction, no env reads, no service construction. Per bead .6
        // contract these work even with `CORTEX_API_TOKEN` unset.
        if let Some(first) = args.first() {
            if first == "--help" || first == "-h" || first == "help" {
                return Ok(Self::Help);
            }
            if first == "--version" || first == "-V" || first == "version" {
                return Ok(Self::Version);
            }
        }

        // Strip CLI-only global flags (`--http`, `--server`, `--token`) from
        // the arg list before subcommand dispatch so they work in any
        // position: `cortex --http search foo` AND `cortex search --http foo`.
        // Non-CLI modes (serve/mcp/setup/deploy/doctor) reject them below — the
        // flags imply HTTP transport, which only applies to the query CLI.
        let mut remaining = args.clone();
        let global = cli::GlobalFlags::extract(&mut remaining)?;

        match remaining.as_slice() {
            [] if global == cli::GlobalFlags::default() => Ok(Self::ServeMcp),
            [command] if command == "mcp" && global == cli::GlobalFlags::default() => {
                Ok(Self::StdioMcp)
            }
            [serve, service]
                if serve == "serve"
                    && service == "mcp"
                    && global == cli::GlobalFlags::default() =>
            {
                Ok(Self::ServeMcp)
            }
            [command, rest @ ..]
                if command == "setup"
                    && rest.first().map(String::as_str) != Some("plugin-hook")
                    && global == cli::GlobalFlags::default() =>
            {
                Ok(Self::Setup(parse_setup_command(rest)?))
            }
            [command, rest @ ..]
                if command == "deploy" && global == cli::GlobalFlags::default() =>
            {
                Ok(Self::Deploy(parse_deploy_command(rest)?))
            }
            [command, rest @ ..]
                if command == "doctor" && global == cli::GlobalFlags::default() =>
            {
                if rest.first().map(String::as_str) == Some("binary") {
                    Ok(Self::DoctorBinary(parse_doctor_command(rest)?))
                } else {
                    Ok(Self::DoctorFull(parse_doctor_full_command(rest)?))
                }
            }
            [command, rest @ ..]
                if matches!(
                    command.as_str(),
                    "search"
                        | "filter"
                        | "tail"
                        | "errors"
                        | "hosts"
                        | "sessions"
                        | "incident"
                        | "ai"
                        | "shell"
                        | "agent-command"
                        | "heartbeat"
                        | "correlate"
                        | "stats"
                        | "db"
                        | "compose"
                        | "service"
                        | "setup"
                        | "config"
                        | "source-ips"
                        | "timeline"
                        | "patterns"
                        | "ingest-rate"
                        | "sig"
                        | "notify"
                        | "silent-hosts"
                        | "clock-skew"
                        | "anomalies"
                        | "compare"
                        | "apps"
                ) =>
            {
                let mut cli_args = Vec::with_capacity(rest.len() + 1);
                cli_args.push(command.clone());
                cli_args.extend(rest.iter().cloned());
                let command = cli::CliCommand::parse(cli_args)?;
                Ok(Self::Cli(Box::new(CliInvocation {
                    command,
                    flags: global,
                })))
            }
            _ if global != cli::GlobalFlags::default() => {
                // Global HTTP flags only apply to CLI query commands. Surface
                // a precise error rather than letting them be silently
                // ignored for `serve mcp`, `setup`, etc.
                anyhow::bail!(
                    "--http / --server / --token only apply to CLI query commands \
                     (search, tail, errors, hosts, sessions, ai, shell, agent-command, heartbeat, correlate, stats, incident, db); \
                     compose, service, setup, and deploy are local-only and reject HTTP flags; \
                     got: {}",
                    args.join(" ")
                );
            }
            _ => {
                print_usage();
                anyhow::bail!("unknown command: {}", args.join(" "));
            }
        }
    }

    fn default_log_filter(&self) -> &'static str {
        match self {
            Self::ServeMcp => "info",
            Self::StdioMcp => "warn",
            Self::Cli(_) => "error",
            Self::Setup(_) => "warn",
            Self::Deploy(_) => "warn",
            Self::DoctorBinary(_) => "warn",
            Self::DoctorFull(_) => "warn",
            Self::Help => "info",
            Self::Version => "info",
        }
    }
}

fn parse_setup_command(args: &[String]) -> Result<SetupCommand> {
    let mut mode = cortex::setup::SetupMode::FirstRun;
    let mut json = false;
    let mut iter = args.iter();
    if matches!(iter.clone().next().map(String::as_str), Some("doctor")) {
        let _ = iter.next();
        for arg in iter {
            match arg.as_str() {
                "--json" => json = true,
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                other => anyhow::bail!("unknown setup doctor argument: {other}"),
            }
        }
        return Ok(SetupCommand {
            kind: SetupCommandKind::Doctor,
            json,
        });
    }
    if matches!(
        iter.clone().next().map(String::as_str),
        Some("ai-index-timer")
    ) {
        let _ = iter.next();
        let (action, json) = parse_setup_subcommand_args("ai-index-timer", iter)?;
        return Ok(SetupCommand {
            kind: SetupCommandKind::AiIndexTimer(match action {
                "install" => cortex::setup::AiIndexTimerAction::Install,
                "remove" => cortex::setup::AiIndexTimerAction::Remove,
                _ => cortex::setup::AiIndexTimerAction::Check,
            }),
            json,
        });
    }
    if matches!(
        iter.clone().next().map(String::as_str),
        Some("ai-watch-service")
    ) {
        let _ = iter.next();
        let (action, json) = parse_setup_subcommand_args("ai-watch-service", iter)?;
        return Ok(SetupCommand {
            kind: SetupCommandKind::AiWatchService(match action {
                "install" => cortex::setup::AiWatchServiceAction::Install,
                "remove" => cortex::setup::AiWatchServiceAction::Remove,
                _ => cortex::setup::AiWatchServiceAction::Check,
            }),
            json,
        });
    }
    if matches!(
        iter.clone().next().map(String::as_str),
        Some("agent-command")
    ) {
        let _ = iter.next();
        let (action, json) = parse_setup_subcommand_args("agent-command", iter)?;
        return Ok(SetupCommand {
            kind: SetupCommandKind::AgentCommand(match action {
                "install" => cortex::setup::AgentCommandAction::Install,
                "remove" => cortex::setup::AgentCommandAction::Remove,
                _ => cortex::setup::AgentCommandAction::Check,
            }),
            json,
        });
    }
    if matches!(
        iter.clone().next().map(String::as_str),
        Some("heartbeat-agent")
    ) {
        let _ = iter.next();
        let (action, json) = parse_setup_subcommand_args("heartbeat-agent", iter)?;
        return Ok(SetupCommand {
            kind: SetupCommandKind::HeartbeatAgent(match action {
                "install" => cortex::setup::HeartbeatAgentAction::Install,
                "remove" => cortex::setup::HeartbeatAgentAction::Remove,
                _ => cortex::setup::HeartbeatAgentAction::Check,
            }),
            json,
        });
    }
    if matches!(
        iter.clone().next().map(String::as_str),
        Some("debug-wrapper")
    ) {
        let _ = iter.next();
        let (action, json) = parse_setup_subcommand_args("debug-wrapper", iter)?;
        return Ok(SetupCommand {
            kind: SetupCommandKind::DebugWrapper(match action {
                "install" => cortex::setup::DebugWrapperAction::Install,
                "remove" => cortex::setup::DebugWrapperAction::Remove,
                _ => cortex::setup::DebugWrapperAction::Check,
            }),
            json,
        });
    }
    if matches!(
        iter.clone().next().map(String::as_str),
        Some("debug-compose")
    ) {
        let _ = iter.next();
        let (action, json) = parse_setup_subcommand_args("debug-compose", iter)?;
        return Ok(SetupCommand {
            kind: SetupCommandKind::DebugCompose(match action {
                "install" => cortex::setup::DebugComposeAction::Install,
                "remove" => cortex::setup::DebugComposeAction::Remove,
                _ => cortex::setup::DebugComposeAction::Check,
            }),
            json,
        });
    }
    for arg in args {
        match arg.as_str() {
            "check" => mode = cortex::setup::SetupMode::Check,
            "repair" => mode = cortex::setup::SetupMode::Repair,
            "--json" => json = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown setup argument: {other}"),
        }
    }
    Ok(SetupCommand {
        kind: SetupCommandKind::Main(mode),
        json,
    })
}

fn parse_deploy_command(args: &[String]) -> Result<DeployCommand> {
    let (subcommand, rest) = args.split_first().ok_or_else(|| {
        anyhow::anyhow!("deploy requires a subcommand: preflight, local, or remote")
    })?;
    let mut json = false;
    let mut dry_run = false;
    let mut host: Option<String> = None;
    for arg in rest {
        match arg.as_str() {
            "--json" => json = true,
            "--dry-run" if matches!(subcommand.as_str(), "local" | "remote") => dry_run = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other if subcommand == "remote" && !other.starts_with('-') => {
                if host.replace(other.to_string()).is_some() {
                    anyhow::bail!("deploy remote accepts exactly one host");
                }
            }
            other => anyhow::bail!("unknown deploy {subcommand} argument: {other}"),
        }
    }
    let kind = match subcommand.as_str() {
        "preflight" => DeployCommandKind::Preflight,
        "local" => DeployCommandKind::Local { dry_run },
        "remote" => DeployCommandKind::Remote {
            host: host.ok_or_else(|| anyhow::anyhow!("deploy remote requires a host"))?,
            dry_run,
        },
        other => anyhow::bail!("unknown deploy subcommand: {other}"),
    };
    Ok(DeployCommand { kind, json })
}

fn parse_setup_subcommand_args<'a>(
    name: &str,
    args: impl Iterator<Item = &'a String>,
) -> Result<(&'static str, bool)> {
    let mut action = "check";
    let mut action_seen = false;
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "install" | "remove" | "check" => {
                if action_seen {
                    anyhow::bail!("{name} action specified more than once");
                }
                action_seen = true;
                action = match arg.as_str() {
                    "install" => "install",
                    "remove" => "remove",
                    _ => "check",
                };
            }
            "--json" => json = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown {name} argument: {other}"),
        }
    }
    Ok((action, json))
}

fn parse_doctor_command(args: &[String]) -> Result<DoctorBinaryCommand> {
    let mut json = false;
    let mut saw_binary = false;
    for arg in args {
        match arg.as_str() {
            "binary" => saw_binary = true,
            "--json" => json = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown doctor argument: {other}"),
        }
    }
    if !saw_binary {
        anyhow::bail!("doctor requires `binary`");
    }
    Ok(DoctorBinaryCommand { json })
}

fn print_usage() {
    eprintln!("{USAGE}");
}

/// Top-level CLI usage banner printed by `cortex --help` (and on parse errors).
/// Kept in sync with the command surface in `src/cli/parse.rs`; the
/// `usage_banner_lists_*` tests guard against drift.
const USAGE: &str = "Usage:
  cortex --version     Print version
  cortex setup [check|repair] [--json]
  cortex setup ai-index-timer install|remove|check [--json]
  cortex setup ai-watch-service install|remove|check [--json]
  cortex setup agent-command install|remove|check [--json]
  cortex setup heartbeat-agent install|remove|check [--json]
  cortex setup debug-wrapper install|remove|check [--json]
  cortex setup debug-compose install|remove|check [--json]
  cortex setup doctor [--json]
  cortex deploy preflight [--json]
  cortex deploy local [--dry-run] [--json]
  cortex deploy remote HOST [--dry-run] [--json]
  cortex doctor [--json]          Run all health checks (setup, compose, binary, AI)
  cortex doctor binary [--json]
  cortex serve mcp    Start syslog UDP/TCP ingest plus HTTP MCP server
  cortex mcp          Start query-only MCP stdio transport
  cortex search [query] [--hostname HOST] [--source-ip SOURCE] [--severity LEVEL] [--app-name APP] [--facility FACILITY] [--exclude-facility FACILITY] [--from TIME] [--to TIME] [--received-from TIME] [--received-to TIME] [--limit N] [--json]
  cortex filter [--hostname HOST] [--source-ip SOURCE] [--source-kind KIND] [--tool TOOL] [--project PATH] [--session-id ID] [--container NAME] [--docker-host HOST] [--stream stdout|stderr] [--event-action ACTION] [--severity LEVEL] [--app-name APP] [--facility FACILITY] [--exclude-facility FACILITY] [--from TIME] [--to TIME] [--received-from TIME] [--received-to TIME] [--limit N] [--json]
  cortex tail [-n N] [--hostname HOST] [--source-ip SOURCE] [--app-name APP] [--json]
  cortex errors [--from TIME] [--to TIME] [--limit N] [--json]
  cortex hosts [--json]
  cortex sessions [--project PATH] [--tool TOOL] [--hostname HOST] [--from TIME] [--to TIME] [--limit N] [--json]
  cortex incident --around TIME [--minutes N] [--service SERVICE] [--host HOST] [--limit N] [--json]
  cortex ai search QUERY [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--json]
  cortex ai abuse [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--before N] [--after N] [--term WORD] [--json]
  cortex ai incidents [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--window-minutes N] [--term WORD] [--json]
  cortex ai investigate [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--window-minutes N] [--correlation-window-minutes N] [--term WORD] [--json]
  cortex ai assess INCIDENT_ID [--model MODEL] [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--window-minutes N] [--correlation-window-minutes N] [--term WORD] [--json]
  cortex ai correlate [--project PATH] [--tool TOOL] [--session-id ID] [--ai-query FTS] [--log-query FTS] [--hostname HOST] [--source-ip SOURCE] [--app-name APP] [--from TIME] [--to TIME] [--window-minutes N] [--severity-min LEVEL] [--limit N] [--events-per-anchor N] [--json]
  cortex ai blocks [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--json]
  cortex ai context --project PATH [--tool TOOL] [--limit N] [--json]
  cortex ai tools [--project PATH] [--from TIME] [--to TIME] [--json]
  cortex ai projects [--tool TOOL] [--from TIME] [--to TIME] [--json]
  cortex ai index [--path PATH] [--since TIME] [--force] [--json]
  cortex ai add --file FILE [--force] [--json]
  cortex ai watch [--path PATH] [--debounce-ms N] [--settle-ms N] [--max-retries N] [--no-initial-scan] [--json]
  cortex ai checkpoints [--errors] [--missing] [--limit N] [--json]
  cortex ai errors [--limit N] [--json]
  cortex ai prune-checkpoints --missing [--dry-run] [--limit N] [--json]
  cortex ai doctor [--strict-permissions] [--json]
  cortex ai watch-status [--json]
  cortex ai smoke-watch [--json]
  cortex shell index --path PATH [--shell zsh] [--json]
  cortex shell atuin-index --path PATH [--json]
  cortex agent-command ingest-spool --path PATH [--json]
  cortex agent-command wrap --spool PATH -- COMMAND...
  cortex heartbeat agent [--target URL] [--token TOKEN] [--interval-secs N] [--probe-deadline-ms N] [--collection-deadline-ms N] [--retry-buffer N] [--host-id-path PATH] [--once|--emit] [--json]
  cortex db status [--check-coord] [--json]
  cortex db integrity [--quick] [--json]
  cortex db checkpoint [--mode passive|full|restart|truncate] [--json]
  cortex db vacuum [--pages N|--full] [--force] [--json]
  cortex db backup [--output PATH] [--json]
  cortex compose doctor [--json]
  cortex compose status [--compose-file FILE] [--project-dir DIR] [--project-name NAME] [--json]
  cortex compose pull|up|restart [--dry-run] [--allow-cwd-target] [--json]
  cortex compose down --yes [--dry-run] [--allow-cwd-target] [--json]
  cortex compose logs [--tail N] [--json]
  syslog service logs SERVICE [--from TIME] [--to TIME] [--tail N] [--json]
  cortex setup check|repair [--json]
  cortex setup agent-command install|remove|check [--json]
  cortex setup plugin-hook [--no-repair] [--json]
  cortex deploy preflight [--json]
  cortex deploy local [--dry-run] [--json]
  cortex deploy remote HOST [--dry-run] [--json]
  cortex config get KEY [--env|--toml] [--toml-path PATH] [--json]
  cortex config set KEY VALUE [--env|--toml] [--toml-path PATH] [--json]
  cortex config unset KEY [--env|--toml] [--toml-path PATH] [--json]
  cortex config list [--env|--toml] [--toml-path PATH] [--json]
  syslog correlate --reference-time TIME [--window-minutes N] [--severity-min LEVEL] [--hostname HOST] [--source-ip SOURCE] [--query FTS] [--limit N] [--json]
  cortex stats [--json]
  cortex source-ips [--limit N] [--offset N] [--json]
  cortex timeline [--bucket minute|hour|day] [--group-by FIELD] [--hostname HOST] [--app-name APP] [--severity-min LEVEL] [--from TIME] [--to TIME] [--json]
  cortex patterns [--top-n N] [--scan-limit N] [--hostname HOST] [--app-name APP] [--severity-min LEVEL] [--from TIME] [--to TIME] [--json]
  cortex ingest-rate [--by-host] [--json]
  cortex sig list [--include-acknowledged] [--limit N] [--json]
  cortex sig ack HASH [--notes TEXT] [--json]
  cortex sig unack HASH [--reason TEXT] [--json]
  cortex notify recent [--rule-id ID] [--since TIME] [--limit N] [--json]
  cortex notify test [--body TEXT] [--json]   (requires --http)

Global CLI flags (apply to query commands above; not valid for serve/mcp/setup/doctor):
  --http              Route this invocation through the container's REST API instead of opening the local SQLite DB.
                      Fails closed: if no token/server is discoverable, the CLI exits non-zero (never silently uses local).
  --server URL        Override the API base URL (implies --http). Default: CORTEX_URL or http://127.0.0.1:3100
  --token TOKEN       Override the bearer token (implies --http). Default: CORTEX_API_TOKEN

Environment:
  CORTEX_DB_PATH  SQLite database path used by both transports
  CORTEX_USE_HTTP     Set to 1 or true to default to HTTP mode without passing --http (fail-closed if discovery fails).
                      CORTEX_API_TOKEN alone does NOT trigger HTTP — must explicitly opt in via --http or CORTEX_USE_HTTP=1.
  CORTEX_URL      Default API base URL for --http mode (overridden by --server)
  CORTEX_API_TOKEN    Bearer token for --http mode (overridden by --token)
  RUST_LOG            Log filter; stdio logs always go to stderr";

async fn shutdown_signal() {
    let ctrl_c = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(error = %e, "Failed to install CTRL+C handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("Shutdown signal received");
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
