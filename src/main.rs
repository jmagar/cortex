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
    use std::collections::HashSet;
    use syslog_mcp::compose::{
        CliDockerInspect, ComposeDefaults, ComposeService, ComposeTarget, DiagnosticSeverity,
        ProcessRunner,
    };
    use syslog_mcp::setup::SetupStatus;

    // (status, name, detail) — elapsed omitted from display for cleanliness.
    type Phase = (SetupStatus, String, String);

    fn status_label(s: &SetupStatus) -> &'static str {
        match s {
            SetupStatus::Ok => "Ok   ",
            SetupStatus::Warn => "Warn ",
            SetupStatus::Error => "Error",
            SetupStatus::Skipped => "Skip ",
        }
    }

    fn diag_status(sev: &DiagnosticSeverity) -> SetupStatus {
        match sev {
            DiagnosticSeverity::Error | DiagnosticSeverity::Unsafe => SetupStatus::Error,
            DiagnosticSeverity::Warning => SetupStatus::Warn,
            DiagnosticSeverity::Info => SetupStatus::Ok,
        }
    }

    /// Truncate multi-line text to the most meaningful single line.
    fn first_meaningful_line(text: &str) -> &str {
        text.lines().find(|l| !l.trim().is_empty()).unwrap_or(text)
    }

    /// Print a section: header with pass/warn/error counts, then only non-Ok phases.
    fn print_section(header: &str, phases: &[Phase]) -> usize {
        let errors = phases
            .iter()
            .filter(|(s, ..)| matches!(s, SetupStatus::Error))
            .count();
        let warnings = phases
            .iter()
            .filter(|(s, ..)| matches!(s, SetupStatus::Warn))
            .count();
        let passed = phases
            .iter()
            .filter(|(s, ..)| matches!(s, SetupStatus::Ok | SetupStatus::Skipped))
            .count();

        let counts = match (passed, errors, warnings) {
            (_, 0, 0) => format!("{passed} passed"),
            (0, e, 0) => format!("{e} error"),
            (0, 0, w) => format!("{w} warning"),
            (0, e, w) => format!("{e} error, {w} warning"),
            (_, e, 0) => format!("{passed} passed · {e} error"),
            (_, 0, w) => format!("{passed} passed · {w} warning"),
            (_, e, w) => format!("{passed} passed · {e} error, {w} warning"),
        };
        println!("{:<18} {}", header, counts);
        for (status, name, detail) in phases {
            if matches!(status, SetupStatus::Ok | SetupStatus::Skipped) {
                continue;
            }
            println!(
                "  {}  {:<26}  {}",
                status_label(status),
                name,
                first_meaningful_line(detail)
            );
        }
        errors
    }

    if command.json {
        // JSON: aggregate raw sub-reports; don't apply the text-mode fixups.
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

        let setup_errors = setup
            .get("blocking_errors")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        // Dev-mode checks don't count as real errors in the JSON exit code either.
        let setup_dev_errors = ["debug-wrapper-content", "debug-compose-content"]
            .iter()
            .filter(|name| {
                setup
                    .get("phases")
                    .and_then(|p| p.as_array())
                    .is_some_and(|phases| {
                        phases.iter().any(|ph| {
                            ph.get("name").and_then(|n| n.as_str()) == Some(name)
                                && matches!(
                                    ph.get("status").and_then(|s| s.as_str()),
                                    Some("error")
                                )
                        })
                    })
            })
            .count() as u64;
        let compose_errors = compose
            .get("diagnostics")
            .and_then(|v| v.as_array())
            .map(|d| {
                d.iter()
                    .filter(|diag| {
                        matches!(
                            diag.get("severity").and_then(|s| s.as_str()),
                            Some("error") | Some("unsafe")
                        )
                    })
                    .count() as u64
            })
            .unwrap_or(0);
        let binary_errors = if binary.runtime_current == Some(false) {
            1u64
        } else {
            0
        };
        let total = setup_errors.saturating_sub(setup_dev_errors) + compose_errors + binary_errors;
        if total > 0 {
            anyhow::bail!("doctor found {total} error(s)");
        }
        return Ok(());
    }

    // ── Text mode ─────────────────────────────────────────────────────────────
    let mut total_errors: usize = 0;

    // 1. Setup -----------------------------------------------------------------
    let mut setup_phases: Vec<Phase> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    match syslog_mcp::setup::run_setup_doctor().await {
        Ok(report) => {
            for phase in &report.phases {
                // Skip runtime-current — the Binary section covers it more clearly.
                if phase.name == "runtime-current" {
                    continue;
                }
                // Skip duplicates (setup doctor embeds ai-watch-service phases which
                // repeat some top-level phases like ai-transcript-root-permissions).
                if !seen.insert(phase.name.to_string()) {
                    continue;
                }
                // Dev-mode checks always fail when the production binary is
                // installed instead of the debug wrapper. Downgrade to Warn and
                // replace the cryptic file-content error with a clearer message.
                let (status, detail) = match phase.name {
                    "debug-wrapper-content" if matches!(phase.status, SetupStatus::Error) => (
                        SetupStatus::Warn,
                        "production binary installed (not the dev wrapper — expected in production)"
                            .to_string(),
                    ),
                    "debug-compose-content" if matches!(phase.status, SetupStatus::Error) => (
                        SetupStatus::Warn,
                        "override uses production config (not the debug build override — expected in production)"
                            .to_string(),
                    ),
                    _ => (phase.status.clone(), phase.detail.clone()),
                };
                setup_phases.push((status, phase.name.to_string(), detail));
            }
        }
        Err(e) => {
            setup_phases.push((SetupStatus::Error, "setup_doctor".into(), e.to_string()));
        }
    }
    total_errors += print_section("Setup", &setup_phases);

    // 2. Compose ---------------------------------------------------------------
    let mut compose_phases: Vec<Phase> = Vec::new();
    let compose_svc =
        ComposeService::new(CliDockerInspect, ProcessRunner, ComposeDefaults::default());
    match compose_svc.status(&ComposeTarget::default()) {
        Ok(status) => {
            let running = status
                .status
                .as_deref()
                .is_some_and(|s| !s.to_ascii_lowercase().contains("exit") && s != "stopped");
            compose_phases.push((
                if running {
                    SetupStatus::Ok
                } else {
                    SetupStatus::Error
                },
                "status".into(),
                format!(
                    "{} ({})",
                    status.status.as_deref().unwrap_or("unknown"),
                    status.health.as_deref().unwrap_or("no healthcheck")
                ),
            ));
            // data_volume bind-mount check.
            match status.data_mounts.iter().find(|m| m.target == "/data") {
                Some(m) => {
                    let src = m
                        .source
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    compose_phases.push((
                        if m.kind == "bind" {
                            SetupStatus::Ok
                        } else {
                            SetupStatus::Error
                        },
                        "data_volume".into(),
                        format!("{} {} → /data", m.kind, src),
                    ));
                }
                None if running => {
                    compose_phases.push((
                        SetupStatus::Error,
                        "data_volume".into(),
                        "no /data mount".into(),
                    ));
                }
                None => {}
            }
            for diag in &status.diagnostics {
                compose_phases.push((
                    diag_status(&diag.severity),
                    diag.code.clone(),
                    diag.message.clone(),
                ));
            }
        }
        Err(e) => {
            compose_phases.push((SetupStatus::Error, "compose_status".into(), e.to_string()));
        }
    }
    total_errors += print_section("Compose", &compose_phases);

    // 3. Binary ----------------------------------------------------------------
    let binary = BinaryDoctorReport::collect();
    let (bin_status, bin_detail) = match binary.runtime_current {
        Some(true) => (
            SetupStatus::Ok,
            format!(
                "container {} == repo {}",
                binary.container_version.as_deref().unwrap_or("-"),
                binary.repo_version
            ),
        ),
        Some(false) => (
            SetupStatus::Error,
            format!(
                "container {} != repo {} — run: syslog compose up",
                binary.container_version.as_deref().unwrap_or("-"),
                binary.repo_version
            ),
        ),
        None => (
            SetupStatus::Warn,
            binary
                .runtime_current_error
                .as_deref()
                .map(first_meaningful_line)
                .unwrap_or("could not determine container version")
                .to_string(),
        ),
    };
    total_errors += print_section(
        "Binary",
        &[(bin_status, "runtime_current".into(), bin_detail)],
    );

    // 4. AI Transcripts --------------------------------------------------------
    let mut ai_phases: Vec<Phase> = Vec::new();
    match RuntimeCore::load_query_only().await {
        Ok(runtime) => match runtime.service().ai_doctor().await {
            Ok(ai) => {
                for (name, root) in [
                    ("claude_root", &ai.claude_root),
                    ("codex_root", &ai.codex_root),
                ] {
                    let (s, detail) = if root.exists && root.readable {
                        (SetupStatus::Ok, root.path.clone())
                    } else if !root.exists {
                        (SetupStatus::Warn, format!("{} (missing)", root.path))
                    } else {
                        (SetupStatus::Warn, format!("{} (not readable)", root.path))
                    };
                    ai_phases.push((s, name.into(), detail));
                }
                ai_phases.push((
                    if ai.checkpoint_error_count > 0 || ai.missing_checkpoint_count > 0 {
                        SetupStatus::Warn
                    } else {
                        SetupStatus::Ok
                    },
                    "checkpoints".into(),
                    format!(
                        "{} indexed, {} errors, {} missing",
                        ai.checkpoint_count, ai.checkpoint_error_count, ai.missing_checkpoint_count
                    ),
                ));
                if ai.parse_error_count > 0 {
                    ai_phases.push((
                        SetupStatus::Warn,
                        "parse_errors".into(),
                        format!("{} parse errors", ai.parse_error_count),
                    ));
                }
            }
            Err(e) => {
                ai_phases.push((SetupStatus::Error, "ai_doctor".into(), e.to_string()));
            }
        },
        Err(e) => {
            ai_phases.push((SetupStatus::Error, "db_connect".into(), e.to_string()));
        }
    }
    total_errors += print_section("AI Transcripts", &ai_phases);

    // ── Summary ───────────────────────────────────────────────────────────────
    println!();
    if total_errors == 0 {
        println!("All checks passed.");
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
