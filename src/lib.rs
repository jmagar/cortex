pub mod api;
pub mod app;
pub(crate) mod cli;
pub mod config;
pub mod mcp;
pub mod observability;
pub mod otlp;
pub mod runtime;
pub mod syslog;

pub(crate) mod db;
pub(crate) mod docker_ingest;
pub(crate) mod ingest;

use anyhow::Result;
use axum::Router;
use rmcp::{transport::stdio, ServiceExt};
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

pub async fn entry() -> Result<()> {
    let mode = Mode::parse(std::env::args().skip(1).collect())?;
    if mode == Mode::Help {
        print_usage();
        return Ok(());
    }
    if mode == Mode::Version {
        println!("Hive {}", env!("CARGO_PKG_VERSION"));
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

    info!("Hive v{}", env!("CARGO_PKG_VERSION"));

    match mode {
        Mode::ServeMcp => serve_mcp().await,
        Mode::StdioMcp => serve_stdio_mcp().await,
        Mode::Cli(command) => run_cli(command).await,
        Mode::Help => unreachable!("handled before logging initialization"),
        Mode::Version => unreachable!("handled before logging initialization"),
    }
}

async fn serve_stdio_mcp() -> Result<()> {
    let runtime = runtime::RuntimeCore::load_query_only().await?;
    let service = mcp::rmcp_server(runtime.mcp_state()).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

async fn run_cli(command: cli::CliCommand) -> Result<()> {
    let runtime = runtime::RuntimeCore::load_query_only().await?;
    cli::run(runtime.service(), command).await
}

async fn serve_mcp() -> Result<()> {
    let runtime = runtime::RuntimeCore::load().await?;
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
    info!("OTLP receiver mounted at /v1/logs (and /v1/metrics -> 200, /v1/traces -> 404)");
    if runtime.config.mcp.api_token.is_none() && !runtime.config.mcp.host.starts_with("127.") {
        tracing::warn!(
            bind = %runtime.config.mcp.bind_addr(),
            "OTLP /v1/logs is mounted WITHOUT authentication on a non-loopback bind. \
             Anyone reachable on this address can write log records. \
             Set HIVE_MCP_TOKEN or legacy SYSLOG_MCP_TOKEN to require Bearer auth."
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
    Cli(cli::CliCommand),
    Help,
    Version,
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod main_tests;

impl Mode {
    fn parse(args: Vec<String>) -> Result<Self> {
        match args.as_slice() {
            [] => Ok(Self::ServeMcp),
            [flag] if flag == "--help" || flag == "-h" || flag == "help" => Ok(Self::Help),
            [flag] if flag == "--version" || flag == "-V" || flag == "version" => Ok(Self::Version),
            [command] if command == "mcp" => Ok(Self::StdioMcp),
            [serve, service] if serve == "serve" && service == "mcp" => Ok(Self::ServeMcp),
            [command, rest @ ..]
                if matches!(
                    command.as_str(),
                    "search" | "tail" | "errors" | "hosts" | "correlate" | "stats"
                ) =>
            {
                let mut cli_args = Vec::with_capacity(rest.len() + 1);
                cli_args.push(command.clone());
                cli_args.extend(rest.iter().cloned());
                Ok(Self::Cli(cli::CliCommand::parse(cli_args)?))
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
            Self::Help => "info",
            Self::Version => "info",
        }
    }
}

fn print_usage() {
    eprintln!(
        "Usage:
  hive --version     Print version
  hive serve mcp     Start syslog UDP/TCP ingest plus HTTP MCP server
  hive mcp           Start query-only MCP stdio transport
  hive search [query] [--hostname HOST] [--source-ip SOURCE] [--severity LEVEL] [--app-name APP] [--from TIME] [--to TIME] [--limit N] [--json]
  hive tail [-n N] [--hostname HOST] [--source-ip SOURCE] [--app-name APP] [--json]
  hive errors [--from TIME] [--to TIME] [--json]
  hive hosts [--json]
  hive correlate --reference-time TIME [--window-minutes N] [--severity-min LEVEL] [--hostname HOST] [--source-ip SOURCE] [--query FTS] [--limit N] [--json]
  hive stats [--json]

Legacy alias:
  syslog            Runs the same commands for this transition release

Environment:
  HIVE_MCP_DB_PATH     SQLite database path used by both transports
  SYSLOG_MCP_DB_PATH   Legacy alias for HIVE_MCP_DB_PATH
  RUST_LOG             Log filter; stdio logs always go to stderr"
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

/// Test support: factory helpers for building [`mcp::AppState`] variants.
///
/// Gated by `cfg(any(test, feature = "test-support"))` so the helpers are
/// compiled in unit-test builds (crate-internal, via `#[cfg(test)]`) and for
/// integration tests that explicitly opt in with `--features test-support`.
/// They are `#[doc(hidden)]` so they don't pollute the public API surface.
///
/// All helpers take a `data_dir: &std::path::Path` so the caller controls the
/// lifetime of the temporary directory — no `tempfile` dep in this crate.
/// Integration tests pass `tempfile::TempDir::path()` or any other `Path`.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod testing {
    use std::{path::Path, sync::Arc};

    use crate::{
        app::SyslogService,
        config::{McpConfig, StorageConfig},
        db,
        mcp::{AppState, AuthPolicy},
        otlp::OtlpCounters,
    };

    /// Build an [`AppState`] with [`AuthPolicy::LoopbackDev`].
    /// `data_dir` must remain alive for the duration of the test.
    pub fn loopback_state(data_dir: &Path) -> AppState {
        state_with_policy(data_dir, AuthPolicy::LoopbackDev, None)
    }

    /// Build an [`AppState`] with [`AuthPolicy::Mounted`] + `auth_state: None`
    /// (static-bearer-only mode).
    pub fn bearer_state(data_dir: &Path, token: &str) -> AppState {
        state_with_policy(
            data_dir,
            AuthPolicy::Mounted { auth_state: None },
            Some(token.to_string()),
        )
    }

    /// Build an [`AppState`] with [`AuthPolicy::Mounted`] + a real
    /// [`lab_auth::state::AuthState`] (OAuth mode). Initialises the auth
    /// SQLite store and generates a fresh RSA signing key under `data_dir`.
    ///
    /// Google credentials are stubbed — no real Google requests are made.
    pub async fn oauth_state(data_dir: &Path) -> AppState {
        let auth_state = build_auth_state(data_dir).await;
        state_with_oauth(data_dir, Arc::new(auth_state), None)
    }

    /// Like [`oauth_state`] but also returns the [`lab_auth::state::AuthState`]
    /// so callers can issue tokens via `auth_state.signing_keys`.
    pub async fn oauth_state_with_auth_state(
        data_dir: &Path,
    ) -> (AppState, lab_auth::state::AuthState) {
        let auth_state = build_auth_state(data_dir).await;
        let state = state_with_oauth(data_dir, Arc::new(auth_state.clone()), None);
        (state, auth_state)
    }

    // ── private helpers ──────────────────────────────────────────────────────

    fn minimal_storage(data_dir: &Path) -> StorageConfig {
        StorageConfig {
            db_path: data_dir.join("syslog-test.db"),
            pool_size: 1,
            retention_days: 0,
            wal_mode: false,
            max_db_size_mb: 0,
            recovery_db_size_mb: 0,
            min_free_disk_mb: 0,
            recovery_free_disk_mb: 0,
            cleanup_interval_secs: 60,
            cleanup_chunk_size: 1,
        }
    }

    fn state_with_policy(data_dir: &Path, policy: AuthPolicy, token: Option<String>) -> AppState {
        let storage = minimal_storage(data_dir);
        let pool = Arc::new(db::init_pool(&storage).expect("test db pool should init"));
        AppState {
            service: SyslogService::new(pool, storage),
            config: base_config(None, token),
            otlp_counters: Arc::new(OtlpCounters::default()),
            auth_policy: policy,
            observability: Arc::new(crate::observability::RuntimeObservability::default()),
        }
    }

    fn state_with_oauth(
        data_dir: &Path,
        auth_state: Arc<lab_auth::state::AuthState>,
        token: Option<String>,
    ) -> AppState {
        let storage = minimal_storage(data_dir);
        let pool = Arc::new(db::init_pool(&storage).expect("test db pool should init"));
        AppState {
            service: SyslogService::new(pool, storage),
            config: base_config(Some("https://syslog.example.com"), token),
            otlp_counters: Arc::new(OtlpCounters::default()),
            auth_policy: AuthPolicy::Mounted {
                auth_state: Some(auth_state),
            },
            observability: Arc::new(crate::observability::RuntimeObservability::default()),
        }
    }

    fn base_config(public_url: Option<&str>, token: Option<String>) -> McpConfig {
        McpConfig {
            host: "127.0.0.1".into(),
            port: 3100,
            server_name: "syslog-mcp".into(),
            no_auth: false,
            api_token: token,
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
            auth: crate::config::AuthConfig {
                public_url: public_url.map(|u| u.to_string()),
                ..Default::default()
            },
        }
    }

    pub async fn build_auth_state(data_dir: &Path) -> lab_auth::state::AuthState {
        let vars: Vec<(String, String)> = vec![
            ("SYSLOG_MCP_AUTH_MODE".into(), "oauth".into()),
            (
                "SYSLOG_MCP_PUBLIC_URL".into(),
                "https://syslog.example.com".into(),
            ),
            (
                "SYSLOG_MCP_GOOGLE_CLIENT_ID".into(),
                "test-client-id".into(),
            ),
            (
                "SYSLOG_MCP_GOOGLE_CLIENT_SECRET".into(),
                "test-client-secret".into(),
            ),
            (
                "SYSLOG_MCP_AUTH_ADMIN_EMAIL".into(),
                "admin@example.com".into(),
            ),
            (
                "SYSLOG_MCP_AUTH_SQLITE_PATH".into(),
                data_dir
                    .join("auth.db")
                    .to_str()
                    .expect("auth.db path should be valid UTF-8")
                    .into(),
            ),
            (
                "SYSLOG_MCP_AUTH_KEY_PATH".into(),
                data_dir
                    .join("auth-jwt.pem")
                    .to_str()
                    .expect("auth-jwt.pem path should be valid UTF-8")
                    .into(),
            ),
        ];

        let auth_config = lab_auth::config::AuthConfigBuilder::new()
            .env_prefix("SYSLOG_MCP")
            .session_cookie_name("syslog_mcp_session")
            .scopes_supported(vec![
                "hive:read".into(),
                "hive:admin".into(),
                "syslog:read".into(),
                "syslog:admin".into(),
            ])
            .default_scope("hive:read")
            .resource_path("/mcp")
            .build_from_sources(vars)
            .expect("test auth config should build");

        lab_auth::state::AuthState::new(auth_config)
            .await
            .expect("test auth state should init")
    }
}
