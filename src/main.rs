use anyhow::Result;
use axum::Router;
use cortex::{api, doctor, logging, mcp, runtime::RuntimeCore};
use rmcp::{ServiceExt, transport::stdio};
use tracing::info;

mod cli;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        // Aurora rose-red `error:` prefix on stderr (gated by --color / TTY).
        cli::color::report_error(&format!("{e:#}"));
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    // Parse + strip `--color` / `--no-color` before anything prints, so the
    // help banner, errors, query output, doctor, setup, and tracing all honor
    // the same switch.
    let mut raw: Vec<String> = std::env::args().skip(1).collect();
    cli::color::install_color_from_args(&mut raw)?;

    // Explicit help (`cortex [--help|help]`, `cortex <cmd> --help`) prints the
    // Aurora grouped banner / per-command flags to stdout and exits 0.
    if cli::help::maybe_handle_help(&raw) {
        return Ok(());
    }

    let mode = Mode::parse(raw)?;
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

    if let cli::CliCommand::Completions(args) = command {
        return cli::run_completions(&args);
    }

    if let cli::CliCommand::Complete(args) = command {
        return cli::run_complete(&args);
    }

    if let cli::CliCommand::Inventory(command) = command {
        if let Some(trigger) = flags.http_flag_trigger() {
            anyhow::bail!(
                "{} has no effect on `inventory` (local-only command); remove --http / --server / --token",
                trigger
            );
        }
        return cli::run_inventory(command).await;
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
        DeployCommandKind::Agent {
            hosts,
            target,
            token,
            docker,
            journald,
            binary,
        } => {
            run_deploy_agent(hosts, target, token, docker, journald, binary)?;
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

fn run_deploy_agent(
    explicit_hosts: Vec<String>,
    target: Option<String>,
    token: Option<String>,
    docker: bool,
    journald: bool,
    binary: Option<String>,
) -> Result<()> {
    use cortex::agent_deploy::{
        AgentDeployConfig, deploy_agent_to_host, find_local_binary, probe_hosts,
        select_hosts_interactive, ssh_config_hosts,
    };

    // Resolve the binary to deploy.
    let local_binary = match binary.as_deref() {
        Some(p) => std::path::PathBuf::from(p),
        None => find_local_binary()
            .ok_or_else(|| anyhow::anyhow!("could not find cortex binary; use --binary"))?,
    };
    eprintln!("  binary  {}", local_binary.display());

    // Determine target hosts (explicit list or discover + interactive select).
    let selected = if !explicit_hosts.is_empty() {
        explicit_hosts
    } else {
        let all_hosts = ssh_config_hosts();
        if all_hosts.is_empty() {
            anyhow::bail!("no hosts found in ~/.ssh/config");
        }
        eprintln!(
            "  discovering {} host(s) from ~/.ssh/config …",
            all_hosts.len()
        );
        let probes = probe_hosts(all_hosts);
        select_hosts_interactive(&probes)?
    };

    if selected.is_empty() {
        eprintln!("  no hosts selected");
        return Ok(());
    }

    let config = AgentDeployConfig {
        target,
        token,
        docker,
        journald,
    };
    let mut failed = false;
    for host in &selected {
        eprint!("  deploying → {host} … ");
        let _ = std::io::Write::flush(&mut std::io::stderr());
        let result = deploy_agent_to_host(host, &local_binary, &config);
        if result.ok {
            eprintln!("✓  ({}ms)", result.elapsed_ms);
        } else {
            eprintln!("✗  {}", result.detail);
            failed = true;
        }
    }
    if failed {
        anyhow::bail!("one or more agent deployments failed");
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
                "{}\t{}\t{}ms\t{}",
                colorize_setup_status(&phase.status),
                phase.name,
                phase.elapsed_ms,
                phase.detail
            );
        }
    }
    if report.has_errors {
        anyhow::bail!("cortex setup completed with failed phases");
    }
    Ok(())
}

/// `{:?}` of a setup phase status, tinted with the Aurora palette (gated by the
/// unified `--color` policy / stdout TTY).
fn colorize_setup_status(status: &cortex::setup::SetupStatus) -> String {
    use cortex::setup::SetupStatus;
    let text = format!("{status:?}");
    match status {
        SetupStatus::Ok => cli::color::success(&text),
        SetupStatus::Warn => cli::color::warn(&text),
        SetupStatus::Error => cli::color::error(&text),
        SetupStatus::Skipped => cli::color::muted(&text),
    }
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

    let mut maintenance = runtime.spawn_maintenance_tasks();
    runtime.start_syslog(&mut maintenance).await?;

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
    Local {
        dry_run: bool,
    },
    Remote {
        host: String,
        dry_run: bool,
    },
    Agent {
        /// Explicit host list; if empty, run interactive discovery.
        hosts: Vec<String>,
        target: Option<String>,
        token: Option<String>,
        docker: bool,
        journald: bool,
        binary: Option<String>,
    },
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
                        | "entity"
                        | "graph"
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
                        | "inventory"
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
                        | "host-state"
                        | "fleet-state"
                        | "correlate-state"
                        | "file-tail"
                        | "__complete"
                        | "completions"
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
                     (search, tail, errors, hosts, sessions, incident, entity, graph, ai, shell, agent-command, heartbeat, correlate, stats, db, file-tail); \
                     compose, service, setup, inventory, and deploy are local-only and reject HTTP flags; \
                     got: {}",
                    args.join(" ")
                );
            }
            _ => match cli::CliCommand::parse(args.clone()) {
                Ok(_) => unreachable!("known CLI commands are handled above"),
                Err(err) => anyhow::bail!("{err}"),
            },
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
    if matches!(iter.clone().next().map(String::as_str), Some("install")) {
        let dest = cli::install_self()?;
        println!("installed -> {}", dest.display());
        std::process::exit(0);
    }
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
        anyhow::anyhow!("deploy requires a subcommand: preflight, local, remote, or agent")
    })?;
    let mut json = false;
    let mut dry_run = false;
    let mut host: Option<String> = None;
    // agent-specific
    let mut agent_hosts: Vec<String> = Vec::new();
    let mut agent_target: Option<String> = None;
    let mut agent_token: Option<String> = None;
    let mut agent_docker = false;
    let mut agent_journald = false;
    let mut agent_binary: Option<String> = None;
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "--json" => json = true,
            "--dry-run" if matches!(subcommand.as_str(), "local" | "remote") => dry_run = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--hosts" if subcommand == "agent" => {
                i += 1;
                let val = rest
                    .get(i)
                    .ok_or_else(|| anyhow::anyhow!("--hosts requires a value"))?;
                agent_hosts = val
                    .split(',')
                    .map(str::trim)
                    .filter(|host| !host.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "--target" if subcommand == "agent" => {
                i += 1;
                agent_target = Some(
                    rest.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--target requires a value"))?
                        .clone(),
                );
            }
            "--heartbeat-token" if subcommand == "agent" => {
                i += 1;
                agent_token = Some(
                    rest.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--heartbeat-token requires a value"))?
                        .clone(),
                );
            }
            "--binary" if subcommand == "agent" => {
                i += 1;
                agent_binary = Some(
                    rest.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--binary requires a value"))?
                        .clone(),
                );
            }
            "--docker" if subcommand == "agent" => agent_docker = true,
            "--journald" if subcommand == "agent" => agent_journald = true,
            other if subcommand == "remote" && !other.starts_with('-') => {
                if host.replace(other.to_string()).is_some() {
                    anyhow::bail!("deploy remote accepts exactly one host");
                }
            }
            other => anyhow::bail!("unknown deploy {subcommand} argument: {other}"),
        }
        i += 1;
    }
    let kind = match subcommand.as_str() {
        "preflight" => DeployCommandKind::Preflight,
        "local" => DeployCommandKind::Local { dry_run },
        "remote" => DeployCommandKind::Remote {
            host: host.ok_or_else(|| anyhow::anyhow!("deploy remote requires a host"))?,
            dry_run,
        },
        "agent" => DeployCommandKind::Agent {
            hosts: agent_hosts,
            target: agent_target,
            token: agent_token,
            docker: agent_docker,
            journald: agent_journald,
            binary: agent_binary,
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

/// Top-level usage to stderr — used by error/misuse fallbacks. Explicit help
/// (`--help`/`help`/`<cmd> --help`) is handled earlier by
/// `cli::help::maybe_handle_help` and prints to stdout instead.
fn print_usage() {
    cli::help::print_top_level_help_stderr();
}

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
