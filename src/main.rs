use anyhow::Result;
use axum::Router;
use rmcp::{transport::stdio, ServiceExt};
use serde::Serialize;
use syslog_mcp::{api, mcp, runtime::RuntimeCore};
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

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

    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(mode.default_log_filter())),
        )
        .with_writer(std::io::stderr)
        .with_target(true)
        .init();

    info!("syslog-mcp v{}", env!("CARGO_PKG_VERSION"));

    match mode {
        Mode::ServeMcp => serve_mcp().await,
        Mode::StdioMcp => serve_stdio_mcp().await,
        Mode::Cli(command) => run_cli(*command).await,
        Mode::Setup(command) => run_setup(command).await,
        Mode::DoctorBinary(command) => run_binary_doctor(command).await,
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

async fn run_cli(command: cli::CliCommand) -> Result<()> {
    if matches!(command, cli::CliCommand::Compose(_)) {
        return cli::run_compose(command);
    }
    let runtime = RuntimeCore::load_query_only().await?;
    cli::run(runtime.service(), command).await
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

async fn run_binary_doctor(command: DoctorBinaryCommand) -> Result<()> {
    let report = BinaryDoctorReport::collect();
    if command.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("current_exe: {}", report.current_exe);
        println!(
            "path_syslog: {}",
            report.path_syslog.as_deref().unwrap_or("-")
        );
        println!("repo_version: {}", report.repo_version);
        println!(
            "container_version: {}",
            report.container_version.as_deref().unwrap_or("-")
        );
        println!(
            "runtime_current: {}",
            report
                .runtime_current
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        );
        if let Some(error) = &report.runtime_current_error {
            println!("runtime_current_error: {}", error);
        }
    }
    if report.runtime_current == Some(false) {
        anyhow::bail!("running syslog container is not current");
    }
    Ok(())
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
        api_enabled = runtime.config.api.enabled,
        docker_ingest_enabled = runtime.config.docker_ingest.enabled,
        docker_ingest_hosts = runtime.config.docker_ingest.hosts.len(),
        "Configuration loaded"
    );

    runtime.start_syslog().await?;
    let _maintenance = runtime.spawn_maintenance_tasks();

    let mut app: Router = mcp::router(runtime.mcp_state());
    if runtime.config.api.enabled {
        app = app.merge(api::router(api::ApiState {
            service: runtime.service(),
            config: runtime.config.api.clone(),
            cors_port: runtime.config.mcp.port,
            auth_policy: runtime.auth_policy().clone(),
        })?);
        info!("Non-MCP API mounted under /api");
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

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    ServeMcp,
    StdioMcp,
    Cli(Box<cli::CliCommand>),
    Setup(SetupCommand),
    DoctorBinary(DoctorBinaryCommand),
    Help,
    Version,
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
        match args.as_slice() {
            [] => Ok(Self::ServeMcp),
            [flag] if flag == "--help" || flag == "-h" || flag == "help" => Ok(Self::Help),
            [flag] if flag == "--version" || flag == "-V" || flag == "version" => Ok(Self::Version),
            [command] if command == "mcp" => Ok(Self::StdioMcp),
            [serve, service] if serve == "serve" && service == "mcp" => Ok(Self::ServeMcp),
            [command, rest @ ..] if command == "setup" => {
                Ok(Self::Setup(parse_setup_command(rest)?))
            }
            [command, rest @ ..] if command == "doctor" => {
                Ok(Self::DoctorBinary(parse_doctor_command(rest)?))
            }
            [command, rest @ ..]
                if matches!(
                    command.as_str(),
                    "search"
                        | "tail"
                        | "errors"
                        | "hosts"
                        | "sessions"
                        | "ai"
                        | "correlate"
                        | "stats"
                        | "db"
                        | "compose"
                ) =>
            {
                let mut cli_args = Vec::with_capacity(rest.len() + 1);
                cli_args.push(command.clone());
                cli_args.extend(rest.iter().cloned());
                Ok(Self::Cli(Box::new(cli::CliCommand::parse(cli_args)?)))
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
            Self::Cli(_) => "warn",
            Self::Setup(_) => "warn",
            Self::DoctorBinary(_) => "warn",
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

#[derive(Debug, Serialize)]
struct BinaryDoctorReport {
    current_exe: String,
    path_syslog: Option<String>,
    repo_version: String,
    container_version: Option<String>,
    runtime_current: Option<bool>,
    runtime_current_error: Option<String>,
}

impl BinaryDoctorReport {
    fn collect() -> Self {
        let current_exe = std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| format!("unknown: {error}"));
        let path_syslog = command_stdout("sh", &["-c", "command -v syslog"]);
        let container_version =
            command_stdout("docker", &["exec", "syslog-mcp", "syslog", "--version"]);
        let (runtime_current, runtime_current_error) = runtime_current_status();
        Self {
            current_exe,
            path_syslog,
            repo_version: env!("CARGO_PKG_VERSION").to_string(),
            container_version,
            runtime_current,
            runtime_current_error,
        }
    }
}

fn runtime_current_status() -> (Option<bool>, Option<String>) {
    let Some(script) = runtime_current_script_path() else {
        return (
            None,
            Some("scripts/check-runtime-current.sh not found".into()),
        );
    };
    match std::process::Command::new("bash").arg(script).output() {
        Ok(output) if output.status.success() => (Some(true), None),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            (
                Some(false),
                Some(format!("{stdout}{stderr}").trim().to_string()),
            )
        }
        Err(error) => (None, Some(error.to_string())),
    }
}

fn runtime_current_script_path() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("SYSLOG_RUNTIME_CHECK_SCRIPT")
        .map(std::path::PathBuf::from)
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("scripts/check-runtime-current.sh"));
            candidates.push(exe_dir.join("../scripts/check-runtime-current.sh"));
            candidates.push(exe_dir.join("../../scripts/check-runtime-current.sh"));
            candidates.push(exe_dir.join("../../../scripts/check-runtime-current.sh"));
        }
    }
    candidates.push(std::path::PathBuf::from("scripts/check-runtime-current.sh"));

    candidates.into_iter().find(|path| path.exists())
}

fn command_stdout(command: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(command)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
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
  syslog doctor binary [--json]
  syslog serve mcp    Start syslog UDP/TCP ingest plus HTTP MCP server
  syslog mcp          Start query-only MCP stdio transport
  syslog search [query] [--hostname HOST] [--source-ip SOURCE] [--severity LEVEL] [--app-name APP] [--from TIME] [--to TIME] [--limit N] [--json]
  syslog tail [-n N] [--hostname HOST] [--source-ip SOURCE] [--app-name APP] [--json]
  syslog errors [--from TIME] [--to TIME] [--json]
  syslog hosts [--json]
  syslog sessions [--project PATH] [--tool TOOL] [--hostname HOST] [--from TIME] [--to TIME] [--limit N] [--json]
  syslog ai search QUERY [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--json]
  syslog ai cuss [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--before N] [--after N] [--term WORD] [--json]
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
  syslog db status|integrity [--json]
  syslog db checkpoint [--mode passive|full|restart|truncate] [--json]
  syslog db vacuum [--pages N|--full] [--json]
  syslog db backup [--output PATH] [--json]
  syslog compose doctor [--json]
  syslog compose status [--compose-file FILE] [--project-dir DIR] [--project-name NAME] [--json]
  syslog compose pull|up|restart [--dry-run] [--allow-cwd-target] [--json]
  syslog compose down --yes [--dry-run] [--allow-cwd-target] [--json]
  syslog compose logs [--tail N] [--json]
  syslog correlate --reference-time TIME [--window-minutes N] [--severity-min LEVEL] [--hostname HOST] [--source-ip SOURCE] [--query FTS] [--limit N] [--json]
  syslog stats [--json]

Environment:
  SYSLOG_MCP_DB_PATH  SQLite database path used by both transports
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
