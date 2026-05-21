use anyhow::Result;
use axum::Router;
use rmcp::{transport::stdio, ServiceExt};
use syslog_mcp::{api, doctor, logging, mcp, runtime::RuntimeCore};
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
        println!("syslog-mcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    logging::init(mode.default_log_filter());

    info!("syslog-mcp v{}", env!("CARGO_PKG_VERSION"));

    match mode {
        Mode::ServeMcp => serve_mcp().await,
        Mode::StdioMcp => serve_stdio_mcp().await,
        Mode::Cli(invocation) => run_cli(*invocation).await,
        Mode::Setup(command) => run_setup(command).await,
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
    // silently ignore the `SYSLOG_USE_HTTP` env trigger — `setup repair`
    // writes that into `~/.syslog-mcp/.env` as the post-cutover default, and
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

async fn run_setup(command: SetupCommand) -> Result<()> {
    let report = match command.kind {
        SetupCommandKind::Main(mode) => syslog_mcp::setup::run_setup(mode).await?,
        SetupCommandKind::AiIndexTimer(action) => {
            syslog_mcp::setup::run_ai_index_timer_setup(action).await?
        }
        SetupCommandKind::AiWatchService(action) => {
            syslog_mcp::setup::run_ai_watch_service_setup(action).await?
        }
        SetupCommandKind::DebugWrapper(action) => {
            syslog_mcp::setup::run_debug_wrapper_setup(action).await?
        }
        SetupCommandKind::DebugCompose(action) => {
            syslog_mcp::setup::run_debug_compose_setup(action).await?
        }
        SetupCommandKind::Doctor => syslog_mcp::setup::run_setup_doctor().await?,
    };
    if command.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("syslog setup mode: {}", report.mode);
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
        anyhow::bail!("syslog setup completed with failed phases");
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
        syslog_bind = %runtime.config.syslog.bind_addr(),
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
    let _maintenance = runtime.spawn_maintenance_tasks();

    let mut app: Router = mcp::router(runtime.mcp_state());
    // /api/* is always-on. The container fails to start without
    // SYSLOG_API_TOKEN — `api::router` enforces that explicitly with a
    // recovery hint pointing at `syslog setup repair`.
    {
        let api_state = api::ApiState::new(
            runtime.service(),
            runtime.config.api.clone(),
            runtime.config.mcp.port,
            syslog_mcp::config::mcp_bind_is_loopback(&runtime.config),
            runtime.config.mcp.allowed_origins.clone(),
            runtime.auth_policy().clone(),
            runtime.pool(),
            runtime.config.notifications.clone(),
        )?;
        app = app.merge(api::router(api_state)?);
        info!("Non-MCP API mounted under /api");
    }
    if syslog_mcp::config::api_token_plaintext_exposure(&runtime.config) {
        tracing::warn!(
            bind = %runtime.config.mcp.bind_addr(),
            public_url = ?runtime.config.mcp.auth.public_url,
            "SYSLOG_API_TOKEN will traverse the wire in plaintext: non-loopback bind with no \
             https:// public URL configured. Front the listener with a TLS-terminating reverse \
             proxy (e.g. SWAG) and set SYSLOG_MCP_PUBLIC_URL=https://..."
        );
    }
    app = app.merge(runtime.otlp_router());
    info!("OTLP receiver mounted at /v1/logs (and /v1/metrics, /v1/traces → 404)");
    if runtime.config.mcp.api_token.is_none() && !runtime.config.mcp.host.starts_with("127.") {
        tracing::warn!(
            bind = %runtime.config.mcp.bind_addr(),
            "OTLP /v1/logs is mounted WITHOUT authentication on a non-loopback bind. \
             Anyone reachable on this address can write log records. \
             Set SYSLOG_MCP_TOKEN to require Bearer auth."
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

    // HTTP connections are drained. Now signal the syslog listeners and batch
    // writer to drain and exit, then checkpoint the WAL so the next startup
    // doesn't replay a large WAL file.
    info!("HTTP server stopped; draining ingest pipeline");
    runtime.shutdown(std::time::Duration::from_secs(5)).await;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    ServeMcp,
    StdioMcp,
    Cli(Box<CliInvocation>),
    Setup(SetupCommand),
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
enum SetupCommandKind {
    Main(syslog_mcp::setup::SetupMode),
    AiIndexTimer(syslog_mcp::setup::AiIndexTimerAction),
    AiWatchService(syslog_mcp::setup::AiWatchServiceAction),
    DebugWrapper(syslog_mcp::setup::DebugWrapperAction),
    DebugCompose(syslog_mcp::setup::DebugComposeAction),
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
        // contract these work even with `SYSLOG_API_TOKEN` unset.
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
        // position: `syslog --http search foo` AND `syslog search --http foo`.
        // Non-CLI modes (serve/mcp/setup/doctor) reject them below — the
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
                        | "tail"
                        | "errors"
                        | "hosts"
                        | "sessions"
                        | "incident"
                        | "ai"
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
                     (search, tail, errors, hosts, sessions, ai, correlate, stats, incident, db); \
                     compose, service, and setup are local-only and reject HTTP flags; \
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
            Self::DoctorBinary(_) => "warn",
            Self::DoctorFull(_) => "warn",
            Self::Help => "info",
            Self::Version => "info",
        }
    }
}

fn parse_setup_command(args: &[String]) -> Result<SetupCommand> {
    let mut mode = syslog_mcp::setup::SetupMode::FirstRun;
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
                "install" => syslog_mcp::setup::AiIndexTimerAction::Install,
                "remove" => syslog_mcp::setup::AiIndexTimerAction::Remove,
                _ => syslog_mcp::setup::AiIndexTimerAction::Check,
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
                "install" => syslog_mcp::setup::AiWatchServiceAction::Install,
                "remove" => syslog_mcp::setup::AiWatchServiceAction::Remove,
                _ => syslog_mcp::setup::AiWatchServiceAction::Check,
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
                "install" => syslog_mcp::setup::DebugWrapperAction::Install,
                "remove" => syslog_mcp::setup::DebugWrapperAction::Remove,
                _ => syslog_mcp::setup::DebugWrapperAction::Check,
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
                "install" => syslog_mcp::setup::DebugComposeAction::Install,
                "remove" => syslog_mcp::setup::DebugComposeAction::Remove,
                _ => syslog_mcp::setup::DebugComposeAction::Check,
            }),
            json,
        });
    }
    for arg in args {
        match arg.as_str() {
            "check" => mode = syslog_mcp::setup::SetupMode::Check,
            "repair" => mode = syslog_mcp::setup::SetupMode::Repair,
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
    eprintln!(
        "Usage:
  syslog --version     Print version
  syslog setup [check|repair] [--json]
  syslog setup ai-index-timer install|remove|check [--json]
  syslog setup ai-watch-service install|remove|check [--json]
  syslog setup debug-wrapper install|remove|check [--json]
  syslog setup debug-compose install|remove|check [--json]
  syslog setup doctor [--json]
  syslog doctor [--json]          Run all health checks (setup, compose, binary, AI)
  syslog doctor binary [--json]
  syslog serve mcp    Start syslog UDP/TCP ingest plus HTTP MCP server
  syslog mcp          Start query-only MCP stdio transport
  syslog search [query] [--hostname HOST] [--source-ip SOURCE] [--severity LEVEL] [--app-name APP] [--facility FACILITY] [--exclude-facility FACILITY] [--from TIME] [--to TIME] [--received-from TIME] [--received-to TIME] [--limit N] [--json]
  syslog tail [-n N] [--hostname HOST] [--source-ip SOURCE] [--app-name APP] [--json]
  syslog errors [--from TIME] [--to TIME] [--json]
  syslog hosts [--json]
  syslog sessions [--project PATH] [--tool TOOL] [--hostname HOST] [--from TIME] [--to TIME] [--limit N] [--json]
  syslog incident --around TIME [--minutes N] [--service SERVICE] [--host HOST] [--limit N] [--json]
  syslog ai search QUERY [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--json]
  syslog ai abuse [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--before N] [--after N] [--term WORD] [--json]
  syslog ai correlate [--project PATH] [--tool TOOL] [--session-id ID] [--ai-query FTS] [--log-query FTS] [--hostname HOST] [--source-ip SOURCE] [--app-name APP] [--from TIME] [--to TIME] [--window-minutes N] [--severity-min LEVEL] [--limit N] [--events-per-anchor N] [--json]
  syslog ai blocks [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--json]
  syslog ai context --project PATH [--tool TOOL] [--limit N] [--json]
  syslog ai tools [--project PATH] [--from TIME] [--to TIME] [--json]
  syslog ai projects [--tool TOOL] [--from TIME] [--to TIME] [--json]
  syslog ai index [--path PATH] [--since TIME] [--force] [--json]
  syslog ai add --file FILE [--force] [--json]
  syslog ai watch [--path PATH] [--debounce-ms N] [--settle-ms N] [--max-retries N] [--no-initial-scan] [--json]
  syslog ai checkpoints [--errors] [--missing] [--limit N] [--json]
  syslog ai errors [--limit N] [--json]
  syslog ai prune-checkpoints --missing [--dry-run] [--limit N] [--json]
  syslog ai doctor [--strict-permissions] [--json]
  syslog ai watch-status [--json]
  syslog ai smoke-watch [--json]
  syslog db status [--check-coord] [--json]
  syslog db integrity [--quick] [--json]
  syslog db checkpoint [--mode passive|full|restart|truncate] [--json]
  syslog db vacuum [--pages N|--full] [--force] [--json]
  syslog db backup [--output PATH] [--json]
  syslog compose doctor [--json]
  syslog compose status [--compose-file FILE] [--project-dir DIR] [--project-name NAME] [--json]
  syslog compose pull|up|restart [--dry-run] [--allow-cwd-target] [--json]
  syslog compose down --yes [--dry-run] [--allow-cwd-target] [--json]
  syslog compose logs [--tail N] [--json]
  syslog service logs SERVICE [--from TIME] [--to TIME] [--tail N] [--json]
  syslog setup check|repair [--json]
  syslog setup plugin-hook [--no-repair] [--json]
  syslog config get KEY [--env|--toml] [--toml-path PATH] [--json]
  syslog config set KEY VALUE [--env|--toml] [--toml-path PATH] [--json]
  syslog config unset KEY [--env|--toml] [--toml-path PATH] [--json]
  syslog config list [--env|--toml] [--toml-path PATH] [--json]
  syslog correlate --reference-time TIME [--window-minutes N] [--severity-min LEVEL] [--hostname HOST] [--source-ip SOURCE] [--query FTS] [--limit N] [--json]
  syslog stats [--json]
  syslog source-ips [--limit N] [--offset N] [--json]
  syslog timeline [--bucket 1m|5m|1h|1d] [--group-by hostname|severity|app] [--hostname HOST] [--app-name APP] [--severity-min LEVEL] [--from TIME] [--to TIME] [--json]
  syslog patterns [--hostname HOST] [--app-name APP] [--severity-min LEVEL] [--from TIME] [--to TIME] [--scan-limit N] [--top-n N] [--json]
  syslog ingest-rate [--by-host] [--json]
  syslog sig list [--include-acknowledged] [--limit N] [--json]
  syslog sig ack HASH [--notes TEXT] [--json]
  syslog sig unack HASH [--reason TEXT] [--json]
  syslog notify recent [--rule-id ID] [--since TIME] [--limit N] [--json]
  syslog notify test [--body TEXT] [--json]

Global CLI flags (apply to query commands above; not valid for serve/mcp/setup/doctor):
  --http              Route this invocation through the container's REST API instead of opening the local SQLite DB.
                      Fails closed: if no token/server is discoverable, the CLI exits non-zero (never silently uses local).
  --server URL        Override the API base URL (implies --http). Default: SYSLOG_MCP_URL or http://127.0.0.1:3100
  --token TOKEN       Override the bearer token (implies --http). Default: SYSLOG_API_TOKEN

Environment:
  SYSLOG_MCP_DB_PATH  SQLite database path used by both transports
  SYSLOG_USE_HTTP     Set to 1 or true to default to HTTP mode without passing --http (fail-closed if discovery fails).
                      SYSLOG_API_TOKEN alone does NOT trigger HTTP — must explicitly opt in via --http or SYSLOG_USE_HTTP=1.
  SYSLOG_MCP_URL      Default API base URL for --http mode (overridden by --server)
  SYSLOG_API_TOKEN    Bearer token for --http mode (overridden by --token)
  RUST_LOG            Log filter; stdio logs always go to stderr"
    );
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
