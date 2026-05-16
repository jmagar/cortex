use anyhow::Result;
use axum::Router;
use rmcp::{transport::stdio, ServiceExt};
use serde::Serialize;
use syslog_mcp::{api, logging, mcp, runtime::RuntimeCore};
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
        Mode::Cli(command) => run_cli(*command).await,
        Mode::Setup(command) => run_setup(command).await,
        Mode::DoctorBinary(command) => run_binary_doctor(command).await,
        Mode::DoctorFull(command) => run_doctor_full(command).await,
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
    if let cli::CliCommand::Setup(command) = command {
        return cli::run_setup(command);
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

async fn run_doctor_full(command: DoctorFullCommand) -> Result<()> {
    use syslog_mcp::compose::{
        CliDockerInspect, ComposeDefaults, ComposeService, ComposeTarget, DiagnosticSeverity,
        ProcessRunner,
    };
    use syslog_mcp::setup::SetupStatus;

    // Tracks exit code; each section appends its error count.
    let mut total_errors: usize = 0;

    // ── helpers ─────────────────────────────────────────────────────────────
    fn status_tag(s: &SetupStatus) -> &'static str {
        match s {
            SetupStatus::Ok => "Ok     ",
            SetupStatus::Warn => "Warn   ",
            SetupStatus::Error => "Error  ",
            SetupStatus::Skipped => "Skip   ",
        }
    }

    fn diag_status(sev: &DiagnosticSeverity) -> SetupStatus {
        match sev {
            DiagnosticSeverity::Error | DiagnosticSeverity::Unsafe => SetupStatus::Error,
            DiagnosticSeverity::Warning => SetupStatus::Warn,
            DiagnosticSeverity::Info => SetupStatus::Ok,
        }
    }

    fn print_phase(status: &SetupStatus, name: &str, elapsed_ms: u128, detail: &str) {
        // Multi-line detail (e.g. script output from runtime_current_error) is
        // truncated to the first non-empty line to keep the table readable.
        let first = detail.lines().find(|l| !l.trim().is_empty()).unwrap_or(detail);
        println!(
            "  {}  {:<28} {:>4}ms  {}",
            status_tag(status),
            name,
            elapsed_ms,
            first
        );
    }

    if command.json {
        // ── JSON mode: aggregate all sub-reports ────────────────────────────
        let setup = syslog_mcp::setup::run_setup_doctor()
            .await
            .map(|r| serde_json::to_value(&r).unwrap_or_default())
            .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}));

        let compose_svc =
            ComposeService::new(CliDockerInspect, ProcessRunner, ComposeDefaults::default());
        let compose = compose_svc
            .status(&ComposeTarget::default())
            .map(|r| serde_json::to_value(&r).unwrap_or_default())
            .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}));

        let binary = BinaryDoctorReport::collect();

        let ai = match RuntimeCore::load_query_only().await {
            Ok(runtime) => runtime
                .service()
                .ai_doctor()
                .await
                .map(|r| serde_json::to_value(&r).unwrap_or_default())
                .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()})),
            Err(e) => serde_json::json!({"error": e.to_string()}),
        };

        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "setup":   setup,
                "compose": compose,
                "binary":  binary,
                "ai":      ai,
            }))?
        );

        // Determine exit code from each section's has_errors / diagnostics.
        let setup_err = setup.get("has_errors").and_then(|v| v.as_bool()).unwrap_or(false);
        let compose_err = compose
            .get("diagnostics")
            .and_then(|v| v.as_array())
            .map(|diags| {
                diags.iter().any(|d| {
                    matches!(
                        d.get("severity").and_then(|s| s.as_str()),
                        Some("error") | Some("unsafe")
                    )
                })
            })
            .unwrap_or(false);
        let binary_err = binary.runtime_current == Some(false);
        if setup_err || compose_err || binary_err {
            anyhow::bail!("doctor found issues");
        }
        return Ok(());
    }

    // ── Text mode ────────────────────────────────────────────────────────────

    // 1. Setup ----------------------------------------------------------------
    println!("Setup");
    match syslog_mcp::setup::run_setup_doctor().await {
        Ok(report) => {
            for phase in &report.phases {
                print_phase(&phase.status, phase.name, phase.elapsed_ms, &phase.detail);
                if matches!(phase.status, SetupStatus::Error) {
                    total_errors += 1;
                }
            }
        }
        Err(e) => {
            total_errors += 1;
            print_phase(&SetupStatus::Error, "setup_doctor", 0, &e.to_string());
        }
    }

    // 2. Compose --------------------------------------------------------------
    println!("\nCompose");
    let compose_svc =
        ComposeService::new(CliDockerInspect, ProcessRunner, ComposeDefaults::default());
    match compose_svc.status(&ComposeTarget::default()) {
        Ok(status) => {
            let running = status
                .status
                .as_deref()
                .is_some_and(|s| !s.to_ascii_lowercase().contains("exit") && s != "stopped");
            let health_detail = format!(
                "{} ({})",
                status.status.as_deref().unwrap_or("unknown"),
                status.health.as_deref().unwrap_or("no healthcheck")
            );
            print_phase(
                if running {
                    &SetupStatus::Ok
                } else {
                    &SetupStatus::Error
                },
                "status",
                0,
                &health_detail,
            );
            if !running {
                total_errors += 1;
            }
            // Show data volume mount (bind vs named-volume drift).
            let data_mount = status.data_mounts.iter().find(|m| m.target == "/data");
            match data_mount {
                Some(m) => {
                    let src = m
                        .source
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let detail = format!("{} {} → /data", m.kind, src);
                    let s = if m.kind == "bind" {
                        SetupStatus::Ok
                    } else {
                        total_errors += 1;
                        SetupStatus::Error
                    };
                    print_phase(&s, "data_volume", 0, &detail);
                }
                None => {
                    // Not an error when container is stopped; already reported above.
                    if running {
                        total_errors += 1;
                        print_phase(&SetupStatus::Error, "data_volume", 0, "no /data mount");
                    }
                }
            }
            for diag in &status.diagnostics {
                let s = diag_status(&diag.severity);
                if matches!(s, SetupStatus::Error) {
                    total_errors += 1;
                }
                print_phase(&s, &diag.code, 0, &diag.message);
            }
        }
        Err(e) => {
            total_errors += 1;
            print_phase(&SetupStatus::Error, "compose_status", 0, &e.to_string());
        }
    }

    // 3. Binary ---------------------------------------------------------------
    println!("\nBinary");
    let binary = BinaryDoctorReport::collect();
    let version_detail = format!(
        "container={} repo={}",
        binary.container_version.as_deref().unwrap_or("-"),
        binary.repo_version
    );
    let version_status = match binary.runtime_current {
        Some(true) => SetupStatus::Ok,
        Some(false) => {
            total_errors += 1;
            SetupStatus::Error
        }
        None => SetupStatus::Warn,
    };
    print_phase(&version_status, "runtime_current", 0, &version_detail);
    if let Some(err) = &binary.runtime_current_error {
        print_phase(&SetupStatus::Warn, "runtime_current_error", 0, err);
    }

    // 4. AI Transcripts -------------------------------------------------------
    println!("\nAI Transcripts");
    match RuntimeCore::load_query_only().await {
        Ok(runtime) => match runtime.service().ai_doctor().await {
            Ok(ai) => {
                for (name, root) in [("claude_root", &ai.claude_root), ("codex_root", &ai.codex_root)] {
                    let s = if root.exists && root.readable {
                        SetupStatus::Ok
                    } else {
                        SetupStatus::Warn
                    };
                    let detail = format!(
                        "{} (exists={} readable={} writable={})",
                        root.path, root.exists, root.readable, root.writable
                    );
                    print_phase(&s, name, 0, &detail);
                }
                let cp_status = if ai.checkpoint_error_count > 0 || ai.missing_checkpoint_count > 0 {
                    SetupStatus::Warn
                } else {
                    SetupStatus::Ok
                };
                print_phase(
                    &cp_status,
                    "checkpoints",
                    0,
                    &format!(
                        "{} indexed  {} errors  {} missing",
                        ai.checkpoint_count, ai.checkpoint_error_count, ai.missing_checkpoint_count
                    ),
                );
                let parse_status = if ai.parse_error_count > 0 {
                    SetupStatus::Warn
                } else {
                    SetupStatus::Ok
                };
                print_phase(
                    &parse_status,
                    "parse_errors",
                    0,
                    &format!("{} parse errors", ai.parse_error_count),
                );
            }
            Err(e) => {
                total_errors += 1;
                print_phase(&SetupStatus::Error, "ai_doctor", 0, &e.to_string());
            }
        },
        Err(e) => {
            total_errors += 1;
            print_phase(&SetupStatus::Error, "db_connect", 0, &e.to_string());
        }
    }

    // ── Summary ──────────────────────────────────────────────────────────────
    println!();
    if total_errors == 0 {
        println!("All checks passed");
    } else {
        anyhow::bail!("{total_errors} error(s) found");
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
    DoctorFull(DoctorFullCommand),
    Help,
    Version,
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
        match args.as_slice() {
            [] => Ok(Self::ServeMcp),
            [flag] if flag == "--help" || flag == "-h" || flag == "help" => Ok(Self::Help),
            [flag] if flag == "--version" || flag == "-V" || flag == "version" => Ok(Self::Version),
            [command] if command == "mcp" => Ok(Self::StdioMcp),
            [serve, service] if serve == "serve" && service == "mcp" => Ok(Self::ServeMcp),
            [command, rest @ ..]
                if command == "setup"
                    && rest.first().map(String::as_str) != Some("plugin-hook") =>
            {
                Ok(Self::Setup(parse_setup_command(rest)?))
            }
            [command, rest @ ..] if command == "doctor" => {
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
                        | "ai"
                        | "correlate"
                        | "stats"
                        | "db"
                        | "compose"
                        | "setup"
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
  syslog doctor [--json]          Run all health checks (setup, compose, binary, AI)
  syslog doctor binary [--json]
  syslog serve mcp    Start syslog UDP/TCP ingest plus HTTP MCP server
  syslog mcp          Start query-only MCP stdio transport
  syslog search [query] [--hostname HOST] [--source-ip SOURCE] [--severity LEVEL] [--app-name APP] [--from TIME] [--to TIME] [--limit N] [--json]
  syslog tail [-n N] [--hostname HOST] [--source-ip SOURCE] [--app-name APP] [--json]
  syslog errors [--from TIME] [--to TIME] [--json]
  syslog hosts [--json]
  syslog sessions [--project PATH] [--tool TOOL] [--hostname HOST] [--from TIME] [--to TIME] [--limit N] [--json]
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
  syslog db status|integrity [--json]
  syslog db checkpoint [--mode passive|full|restart|truncate] [--json]
  syslog db vacuum [--pages N|--full] [--json]
  syslog db backup [--output PATH] [--json]
  syslog compose doctor [--json]
  syslog compose status [--compose-file FILE] [--project-dir DIR] [--project-name NAME] [--json]
  syslog compose pull|up|restart [--dry-run] [--allow-cwd-target] [--json]
  syslog compose down --yes [--dry-run] [--allow-cwd-target] [--json]
  syslog compose logs [--tail N] [--json]
  syslog setup check|repair [--json]
  syslog setup plugin-hook [--no-repair] [--json]
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
