use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

use crate::app::SyslogService;
use crate::config::{mcp_bind_is_loopback, AuthMode, Config};
use crate::db::{self, DbPool, StorageBudgetState};
use crate::ingest::IngestTx;
use crate::mcp::AuthPolicy;
use crate::observability::RuntimeObservability;
use crate::otlp::{self, OtlpCounters, OtlpState};
use crate::syslog::enrichment::EnrichmentConfig;
use crate::{docker_ingest, mcp, syslog};

pub struct RuntimeCore {
    pub config: Config,
    pool: Arc<DbPool>,
    storage_state: Arc<Mutex<Option<StorageBudgetState>>>,
    service: SyslogService,
    /// Semaphore for DB-heavy maintenance tasks: retention purge, storage
    /// guardrail enforcement, error scan, notification evaluator.
    maintenance_permit: Arc<Semaphore>,
    /// Separate semaphore for the notification dispatcher. The dispatcher
    /// makes outbound HTTP calls (5s timeout); keeping it separate prevents
    /// HTTP back-pressure from starving the DB maintenance tasks.
    dispatcher_permit: Arc<Semaphore>,
    ingest: IngestTx,
    otlp_counters: Arc<OtlpCounters>,
    auth_policy: AuthPolicy,
    observability: Arc<RuntimeObservability>,
}

pub struct MaintenanceHandles {
    purge: Option<JoinHandle<()>>,
    storage: Option<JoinHandle<()>>,
    docker_ingest: Vec<JoinHandle<()>>,
    error_scan: Option<JoinHandle<()>>,
    notification_dispatcher: Option<JoinHandle<()>>,
    notification_evaluator: Option<JoinHandle<()>>,
    notification_digest: Option<JoinHandle<()>>,
}

impl Drop for MaintenanceHandles {
    fn drop(&mut self) {
        if let Some(handle) = &self.purge {
            handle.abort();
        }
        if let Some(handle) = &self.storage {
            handle.abort();
        }
        for handle in &self.docker_ingest {
            handle.abort();
        }
        if let Some(handle) = &self.error_scan {
            handle.abort();
        }
        if let Some(handle) = &self.notification_dispatcher {
            handle.abort();
        }
        if let Some(handle) = &self.notification_evaluator {
            handle.abort();
        }
        if let Some(handle) = &self.notification_digest {
            handle.abort();
        }
    }
}

pub(crate) fn background_interval(period: tokio::time::Duration) -> tokio::time::Interval {
    tokio::time::interval_at(tokio::time::Instant::now() + period, period)
}

/// Tags whose retention is hard-capped at 7 days regardless of the global
/// `retention_days` setting. AdGuard query volume would otherwise dominate
/// the FTS5 index at homelab volumes (50k+ DNS queries/day).
const ADGUARD_RETENTION_TAGS: &[&str] = &["adguard-allowed", "adguard-query", "adguard-rewrite"];
const ADGUARD_RETENTION_DAYS: u32 = 7;

impl RuntimeCore {
    pub async fn load() -> Result<Self> {
        Self::for_server(Config::load()?).await
    }

    pub async fn load_query_only() -> Result<Self> {
        // Use load_for_stdio() to skip the non-loopback bind safety gate —
        // stdio mode never binds an HTTP port so the gate is irrelevant.
        Self::query_only(Config::load_for_stdio()?).await
    }

    pub async fn for_server(config: Config) -> Result<Self> {
        Self::from_config_inner(config, true, false).await
    }

    pub async fn query_only(config: Config) -> Result<Self> {
        // Stdio / query-only mode: build_auth_policy short-circuits to
        // LoopbackDev when is_stdio=true. Process isolation is the trust
        // boundary — no TCP port is bound, so AuthLayer and scope checks
        // don't apply.
        Self::from_config_inner(config, false, true).await
    }

    async fn from_config_inner(
        config: Config,
        enforce_initial_storage_budget: bool,
        is_stdio: bool,
    ) -> Result<Self> {
        reject_unsafe_otlp_oauth_only_exposure(&config, is_stdio)?;
        let pool = Arc::new(db::init_pool(&config.storage)?);
        let storage_state = Arc::new(Mutex::new(None));
        if enforce_initial_storage_budget
            && (config.storage.max_db_size_mb > 0 || config.storage.min_free_disk_mb > 0)
        {
            let initial_outcome = db::enforce_storage_budget(&pool, &config.storage)?;
            *storage_state.lock().expect("storage state mutex poisoned") =
                Some(StorageBudgetState {
                    metrics: initial_outcome.metrics.clone(),
                    write_blocked: initial_outcome.write_blocked,
                });
            tracing::info!(
                deleted_rows = initial_outcome.deleted_rows,
                logical_db_size_bytes = initial_outcome.metrics.logical_db_size_bytes,
                physical_db_size_bytes = initial_outcome.metrics.physical_db_size_bytes,
                free_disk_bytes = ?initial_outcome.metrics.free_disk_bytes,
                write_blocked = initial_outcome.write_blocked,
                "Initial storage budget check completed"
            );
        }
        let service = SyslogService::new(Arc::clone(&pool), config.storage.clone());
        let enrichment = EnrichmentConfig {
            authelia_source_ip: config.enrichment.authelia_source_ip.clone(),
            adguard_source_ip: config.enrichment.adguard_source_ip.clone(),
            scrub_prompts: config.enrichment.scrub_prompts,
            api_token: config.mcp.api_token.clone(),
        };
        let observability = Arc::new(RuntimeObservability::default());
        let ingest = crate::ingest::start_writer_from_syslog_config(
            &config.syslog,
            config.storage.clone(),
            Arc::clone(&pool),
            Arc::clone(&storage_state),
            enrichment,
            Arc::clone(&observability),
        );

        let auth_policy = build_auth_policy(&config, is_stdio).await?;

        Ok(Self {
            config,
            pool,
            storage_state,
            service,
            maintenance_permit: Arc::new(Semaphore::new(1)),
            dispatcher_permit: Arc::new(Semaphore::new(1)),
            ingest,
            otlp_counters: Arc::new(OtlpCounters::default()),
            auth_policy,
            observability,
        })
    }

    pub fn service(&self) -> SyslogService {
        self.service.clone()
    }

    /// Shared SQLite pool — exposed for callers that need to read startup-time
    /// metadata (e.g. `api::ApiState::new` caches the schema version).
    pub fn pool(&self) -> &Arc<DbPool> {
        &self.pool
    }

    /// Build the OTLP router with shared counters and the MCP API token (if any).
    pub fn otlp_router(&self) -> axum::Router {
        let state = OtlpState::new(
            self.ingest.clone(),
            self.config.mcp.api_token.clone(),
            Arc::clone(&self.otlp_counters),
        );
        otlp::router(state)
    }

    pub fn mcp_state(&self) -> mcp::AppState {
        mcp::AppState {
            service: self.service(),
            config: self.config.mcp.clone(),
            notifications_config: self.config.notifications.clone(),
            otlp_counters: Arc::clone(&self.otlp_counters),
            auth_policy: self.auth_policy.clone(),
            observability: Arc::clone(&self.observability),
        }
    }

    /// Borrow the resolved authentication policy. Useful for boot-time
    /// diagnostics and for tests.
    pub fn auth_policy(&self) -> &AuthPolicy {
        &self.auth_policy
    }

    pub async fn start_syslog(&self) -> Result<()> {
        syslog::start_listeners(self.config.syslog.clone(), self.ingest.clone()).await
    }

    pub fn spawn_maintenance_tasks(&self) -> MaintenanceHandles {
        let purge = self.spawn_retention_task();
        let storage = self.spawn_storage_task();
        let docker_ingest = docker_ingest::spawn_all(
            self.config.docker_ingest.clone(),
            Arc::clone(&self.pool),
            self.ingest.clone(),
        );
        let error_scan = self.spawn_error_scan_task();
        let notification_dispatcher = self.spawn_notification_dispatcher();
        let notification_evaluator = self.spawn_notification_evaluator();
        let notification_digest = self.spawn_notification_digest();
        MaintenanceHandles {
            purge,
            storage,
            docker_ingest,
            error_scan,
            notification_dispatcher,
            notification_evaluator,
            notification_digest,
        }
    }

    fn spawn_notification_dispatcher(&self) -> Option<JoinHandle<()>> {
        crate::notifications::dispatcher::spawn_dispatcher(
            Arc::clone(&self.pool),
            Arc::clone(&self.dispatcher_permit),
            self.config.notifications.clone(),
        )
    }

    fn spawn_notification_evaluator(&self) -> Option<JoinHandle<()>> {
        crate::notifications::evaluator::spawn_evaluator(
            Arc::clone(&self.pool),
            Arc::clone(&self.maintenance_permit),
            self.config.notifications.clone(),
        )
    }

    fn spawn_notification_digest(&self) -> Option<JoinHandle<()>> {
        crate::notifications::digest::spawn_digest(
            Arc::clone(&self.pool),
            Arc::clone(&self.maintenance_permit),
            self.config.notifications.clone(),
        )
    }

    fn spawn_error_scan_task(&self) -> Option<JoinHandle<()>> {
        let cfg = self.config.error_detection.clone();
        if !cfg.enabled {
            return None;
        }
        let pool = Arc::clone(&self.pool);
        let limiter = Arc::clone(&self.maintenance_permit);
        let interval_secs = cfg.scan_interval_secs.max(1);
        let handle = tokio::spawn(async move {
            let mut interval = background_interval(tokio::time::Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                tracing::debug!("error_scan: scan cycle starting");
                match crate::app::error_detection::run_error_scan(
                    Arc::clone(&pool),
                    Arc::clone(&limiter),
                    cfg.clone(),
                )
                .await
                {
                    Ok(n) => tracing::info!(rows_processed = n, "error_scan: cycle complete"),
                    Err(e) => tracing::error!(error = %e, "error_scan: cycle failed"),
                }
            }
        });
        Some(handle)
    }

    fn spawn_retention_task(&self) -> Option<JoinHandle<()>> {
        let retention_days = self.config.storage.retention_days;
        if retention_days == 0 {
            return None;
        }
        let purge_pool = Arc::clone(&self.pool);
        let limiter = Arc::clone(&self.maintenance_permit);
        let fts_merge_pages = self.config.enrichment.fts_merge_pages;
        let handle = tokio::spawn(async move {
            let mut interval = background_interval(tokio::time::Duration::from_secs(3600));
            loop {
                interval.tick().await;
                let started = Instant::now();
                let Ok(permit) = Arc::clone(&limiter).acquire_owned().await else {
                    tracing::error!("Maintenance limiter closed");
                    continue;
                };
                let pool = Arc::clone(&purge_pool);
                tracing::debug!(retention_days, "Retention purge tick started");
                // Tag-based purge runs FIRST so the global purge below scans
                // a smaller working set and FTS merge work consolidates.
                // Hardcoded 7-day windows for AdGuard tags. Other tags fall
                // through to the global retention_days policy.
                match tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    // Tag-window purges are independent maintenance ops. A
                    // transient SQLITE_BUSY on one tag must NOT abort the
                    // others or the global retention purge — that would stall
                    // all retention for an hour.
                    let mut tag_deleted: usize = 0;
                    for tag in ADGUARD_RETENTION_TAGS {
                        match db::purge_by_tag_window(
                            &pool,
                            tag,
                            ADGUARD_RETENTION_DAYS,
                            fts_merge_pages,
                        ) {
                            Ok(n) => tag_deleted += n,
                            Err(e) => tracing::error!(
                                tag,
                                error = %e,
                                "Tag-window purge failed; continuing"
                            ),
                        }
                    }
                    let global_deleted =
                        db::purge_old_logs(&pool, retention_days, fts_merge_pages)?;
                    Ok::<(usize, usize), anyhow::Error>((tag_deleted, global_deleted))
                })
                .await
                .map_err(|e| anyhow::anyhow!("spawn_blocking error: {e}"))
                .and_then(|r| r)
                {
                    Ok((tag_deleted, global_deleted)) => tracing::info!(
                        retention_days,
                        tag_deleted,
                        global_deleted,
                        total_deleted = tag_deleted + global_deleted,
                        elapsed_ms = started.elapsed().as_millis(),
                        "Retention purge tick completed"
                    ),
                    Err(e) => tracing::error!(
                        error = %e,
                        retention_days,
                        elapsed_ms = started.elapsed().as_millis(),
                        "Failed to purge old logs"
                    ),
                }
            }
        });
        tracing::info!(retention_days, "Log retention purge task started (hourly)");
        Some(handle)
    }

    fn spawn_storage_task(&self) -> Option<JoinHandle<()>> {
        if self.config.storage.max_db_size_mb == 0 && self.config.storage.min_free_disk_mb == 0 {
            return None;
        }
        let storage_pool = Arc::clone(&self.pool);
        let storage_config = self.config.storage.clone();
        let shared_storage_state = Arc::clone(&self.storage_state);
        let limiter = Arc::clone(&self.maintenance_permit);
        let handle = tokio::spawn(async move {
            let mut interval = background_interval(tokio::time::Duration::from_secs(
                storage_config.cleanup_interval_secs,
            ));
            loop {
                interval.tick().await;
                let started = Instant::now();
                let Ok(permit) = Arc::clone(&limiter).acquire_owned().await else {
                    tracing::error!("Maintenance limiter closed");
                    continue;
                };
                let pool = Arc::clone(&storage_pool);
                let storage = storage_config.clone();
                tracing::debug!(
                    cleanup_interval_secs = storage_config.cleanup_interval_secs,
                    "Storage budget enforcement tick started"
                );
                match tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    db::enforce_storage_budget(&pool, &storage)
                })
                .await
                .map_err(|e| anyhow::anyhow!("spawn_blocking error: {e}"))
                .and_then(|r| r)
                {
                    Ok(outcome) => {
                        let previous_blocked = {
                            let mut state = shared_storage_state
                                .lock()
                                .expect("storage state mutex poisoned");
                            let previous_blocked = state.as_ref().map(|s| s.write_blocked);
                            *state = Some(StorageBudgetState {
                                metrics: outcome.metrics.clone(),
                                write_blocked: outcome.write_blocked,
                            });
                            previous_blocked
                        };

                        if outcome.deleted_rows > 0
                            || outcome.write_blocked
                            || previous_blocked != Some(outcome.write_blocked)
                        {
                            tracing::info!(
                                deleted_rows = outcome.deleted_rows,
                                logical_db_size_bytes = outcome.metrics.logical_db_size_bytes,
                                physical_db_size_bytes = outcome.metrics.physical_db_size_bytes,
                                free_disk_bytes = ?outcome.metrics.free_disk_bytes,
                                write_blocked = outcome.write_blocked,
                                elapsed_ms = started.elapsed().as_millis(),
                                "Storage budget enforcement tick completed"
                            );
                        } else {
                            tracing::debug!(
                                deleted_rows = outcome.deleted_rows,
                                logical_db_size_bytes = outcome.metrics.logical_db_size_bytes,
                                physical_db_size_bytes = outcome.metrics.physical_db_size_bytes,
                                free_disk_bytes = ?outcome.metrics.free_disk_bytes,
                                write_blocked = outcome.write_blocked,
                                elapsed_ms = started.elapsed().as_millis(),
                                "Storage budget enforcement tick completed"
                            );
                        }
                    }
                    Err(e) => tracing::error!(
                        error = %e,
                        elapsed_ms = started.elapsed().as_millis(),
                        "Failed to enforce storage budget"
                    ),
                }
            }
        });
        tracing::info!(
            cleanup_interval_secs = self.config.storage.cleanup_interval_secs,
            "Storage budget enforcement task started"
        );
        Some(handle)
    }
}

/// Defense-in-depth duplicate of `validate_auth_config` for callers that use
/// `RuntimeCore::for_server(config)` without going through `Config::load()`.
fn reject_unsafe_otlp_oauth_only_exposure(config: &Config, is_stdio: bool) -> Result<()> {
    if is_stdio || config.mcp.no_auth || config.mcp.auth.mode != AuthMode::OAuth {
        return Ok(());
    }

    if !mcp_bind_is_loopback(config) && !mcp_static_token_active(config) {
        anyhow::bail!(
            "refusing to mount OTLP /v1/logs on non-loopback OAuth-only deployment: \
             OTLP only supports SYSLOG_MCP_TOKEN Bearer auth today; set SYSLOG_MCP_TOKEN, \
             bind to loopback, or set SYSLOG_MCP_NO_AUTH=true when an upstream gateway \
             protects all mounted routes"
        );
    }

    Ok(())
}

fn mcp_static_token_active(config: &Config) -> bool {
    config
        .mcp
        .api_token
        .as_deref()
        .is_some_and(|t| !t.trim().is_empty())
}

/// Decide which [`AuthPolicy`] to install on [`mcp::AppState`] given the
/// fully-loaded [`Config`].
///
/// Decision table (locked by the OAuth epic, post eng-review):
///
/// | `auth.mode` | `api_token` | bind         | result                                         |
/// |-------------|-------------|--------------|------------------------------------------------|
/// | `OAuth`     | any         | any          | `Mounted { auth_state: Some(_) }` (full OAuth) |
/// | `Bearer`    | set         | any          | `Mounted { auth_state: None }` (bearer-only)   |
/// | `Bearer`    | unset       | loopback     | `LoopbackDev` (dev mode; no auth enforced)     |
/// | `Bearer`    | unset       | non-loopback | rejected by `validate_auth_config` at startup  |
/// | any          | any         | any          | `LoopbackDev` when `mcp.no_auth` is true       |
///
/// Bearer-only (`static_token` set, no OAuth) produces `Mounted { auth_state: None }` so
/// that scope checks in tool dispatch (S5) know middleware is enforcing auth.
/// `lab_auth::AuthState::new` is only called for the OAuth row — it requires
/// mode == OAuth and initialises Google OIDC + SQLite session storage.
async fn build_auth_policy(config: &Config, is_stdio: bool) -> Result<AuthPolicy> {
    if config.mcp.no_auth {
        tracing::warn!(
            mcp_bind = %config.mcp.bind_addr(),
            "syslog-mcp auth policy: LoopbackDev (NO_AUTH=true — upstream gateway must enforce access)"
        );
        return Ok(AuthPolicy::LoopbackDev);
    }

    if is_stdio {
        if config.mcp.auth.mode == AuthMode::OAuth {
            tracing::warn!(
                "SYSLOG_MCP_AUTH_MODE=oauth is set but syslog-mcp is starting in stdio mode — \
                 OAuth config is ignored; LoopbackDev policy applies (process isolation is the \
                 trust boundary). If auth enforcement is required, use the HTTP server mode instead."
            );
        }
        tracing::info!(
            "syslog-mcp auth policy: LoopbackDev (stdio mode — process isolation is the trust boundary)"
        );
        return Ok(AuthPolicy::LoopbackDev);
    }

    let auth = &config.mcp.auth;
    let oauth_active = auth.mode == AuthMode::OAuth;
    let static_token_active = mcp_static_token_active(config);

    if !oauth_active {
        if static_token_active {
            // Bearer-only: middleware (AuthLayer) is mounted with just the
            // static token. Scope checks in S5 MUST run — use Mounted so the
            // tool dispatcher knows auth is enforced.
            tracing::info!(
                mcp_bind = %config.mcp.bind_addr(),
                "syslog-mcp auth policy: Mounted {{ auth_state: None }} (bearer-only; lab-auth OAuth not wired)"
            );
            return Ok(AuthPolicy::Mounted { auth_state: None });
        }

        // No auth at all — only legal on loopback (validated by validate_auth_config,
        // but double-checked here so LoopbackDev can never slip past on a non-loopback bind).
        // The early return above guarantees `is_stdio` is false here, so the bind
        // check always applies.
        if !mcp_bind_is_loopback(config) {
            anyhow::bail!(
                "internal invariant violated: no auth wired but bind `{}` is non-loopback",
                config.mcp.host
            );
        }
        tracing::info!(
            mcp_bind = %config.mcp.bind_addr(),
            "syslog-mcp auth policy: LoopbackDev (no auth wired; loopback bind)"
        );
        return Ok(AuthPolicy::LoopbackDev);
    }

    // Resolve auth file paths against the directory containing the syslog DB
    // so a single `/data` bind-mount captures everything.
    let storage_dir = config
        .storage
        .db_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let resolved_db_path = resolve_auth_path(storage_dir, &auth.sqlite_path);
    let resolved_key_path = resolve_auth_path(storage_dir, &auth.key_path);

    // Surface the refresh-token TTL override at info level — lab-auth's default
    // is 30 days; syslog-mcp deliberately ships a tighter (8h) ceiling.
    tracing::info!(
        refresh_token_ttl_secs = auth.refresh_token_ttl_secs,
        "syslog-mcp auth refresh TTL override (lab-auth default is 30d)"
    );

    // Build the env-var "fake source" that lab-auth's AuthConfigBuilder consumes.
    // Lab-auth never consults real `std::env::var` here — we hand it exactly
    // what we want it to see, derived from our typed `Config`.
    let mut vars: Vec<(String, String)> = Vec::with_capacity(16);
    push_var(
        &mut vars,
        "SYSLOG_MCP_AUTH_MODE",
        if oauth_active { "oauth" } else { "bearer" },
    );
    if let Some(url) = auth.public_url.as_deref() {
        push_var(&mut vars, "SYSLOG_MCP_PUBLIC_URL", url);
    }
    if let Some(id) = auth.google_client_id.as_deref() {
        push_var(&mut vars, "SYSLOG_MCP_GOOGLE_CLIENT_ID", id);
    }
    if let Some(secret) = auth.google_client_secret.as_deref() {
        push_var(&mut vars, "SYSLOG_MCP_GOOGLE_CLIENT_SECRET", secret);
    }
    if !auth.admin_email.is_empty() {
        push_var(&mut vars, "SYSLOG_MCP_AUTH_ADMIN_EMAIL", &auth.admin_email);
    }
    // NOTE: lab-auth's AuthConfig has no `allowed_emails` field — it only
    // enforces `admin_email`. syslog-mcp rejects non-empty allowed_emails in
    // OAuth mode until the full email allowlist is enforceable. Do NOT add a
    // no-op push_var for allowed_emails here; the entries would be silently
    // ignored by AuthConfigBuilder.build_from_sources.
    push_var(
        &mut vars,
        "SYSLOG_MCP_AUTH_SQLITE_PATH",
        &resolved_db_path.to_string_lossy(),
    );
    push_var(
        &mut vars,
        "SYSLOG_MCP_AUTH_KEY_PATH",
        &resolved_key_path.to_string_lossy(),
    );
    push_var(
        &mut vars,
        "SYSLOG_MCP_AUTH_ACCESS_TOKEN_TTL_SECS",
        &auth.access_token_ttl_secs.to_string(),
    );
    push_var(
        &mut vars,
        "SYSLOG_MCP_AUTH_REFRESH_TOKEN_TTL_SECS",
        &auth.refresh_token_ttl_secs.to_string(),
    );
    push_var(
        &mut vars,
        "SYSLOG_MCP_AUTH_CODE_TTL_SECS",
        &auth.auth_code_ttl_secs.to_string(),
    );
    push_var(
        &mut vars,
        "SYSLOG_MCP_AUTH_REGISTER_REQUESTS_PER_MINUTE",
        &auth.register_rpm.to_string(),
    );
    push_var(
        &mut vars,
        "SYSLOG_MCP_AUTH_AUTHORIZE_REQUESTS_PER_MINUTE",
        &auth.authorize_rpm.to_string(),
    );
    if !auth.allowed_client_redirect_uris.is_empty() {
        push_var(
            &mut vars,
            "SYSLOG_MCP_AUTH_ALLOWED_REDIRECT_URIS",
            &auth.allowed_client_redirect_uris.join(","),
        );
    }

    let auth_config = lab_auth::config::AuthConfigBuilder::new()
        .env_prefix("SYSLOG_MCP")
        .session_cookie_name("syslog_mcp_session")
        .scopes_supported(vec!["syslog:read".into(), "syslog:admin".into()])
        .default_scope("syslog:read")
        .resource_path("/mcp")
        .static_token_scopes(vec!["syslog:read".into(), "syslog:admin".into()])
        .disable_static_token_with_oauth(auth.disable_static_token_with_oauth)
        .enable_dynamic_registration(true)
        .build_from_sources(vars)
        .context("failed to build lab-auth AuthConfig from syslog-mcp config")?;

    let auth_state = lab_auth::state::AuthState::new(auth_config)
        .await
        .context("failed to initialize lab-auth AuthState")?;

    // lab-auth's SqliteStore::open creates the DB but only *checks* perms when
    // the file pre-existed. Enforce 0600 explicitly for the freshly-created
    // case. The JWT key path is already chmodded by lab-auth's jwt::SigningKeys.
    enforce_restrictive_permissions(&resolved_db_path).with_context(|| {
        format!(
            "failed to enforce 0600 permissions on auth db `{}`",
            resolved_db_path.display()
        )
    })?;
    enforce_restrictive_permissions(&resolved_key_path).with_context(|| {
        format!(
            "failed to enforce 0600 permissions on auth key `{}`",
            resolved_key_path.display()
        )
    })?;

    tracing::info!(
        oauth_active,
        static_token_active,
        auth_db = %resolved_db_path.display(),
        auth_key = %resolved_key_path.display(),
        "syslog-mcp auth policy: Mounted (lab-auth state initialized)"
    );

    Ok(AuthPolicy::Mounted {
        auth_state: Some(Arc::new(auth_state)),
    })
}

fn push_var(vars: &mut Vec<(String, String)>, key: &str, value: &str) {
    vars.push((key.to_string(), value.to_string()));
}

/// Resolve `path` against `base` if it is relative. Absolute paths are
/// returned untouched. Mirrors the `[mcp.auth].sqlite_path` and `key_path`
/// resolution rules documented on `AuthConfig`.
fn resolve_auth_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// Enforce `chmod 0600` on a file. Unix-only; on other platforms this is a
/// no-op (lab-auth makes the same trade-off — see `lab_auth::util`).
#[cfg(unix)]
fn enforce_restrictive_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if !path.exists() {
        // Nothing to lock down. lab-auth owns creation; this guards against
        // an unexpected order of operations.
        anyhow::bail!("expected file at {} after auth init", path.display());
    }
    let metadata = std::fs::metadata(path).with_context(|| format!("stat `{}`", path.display()))?;
    let current = metadata.permissions().mode() & 0o777;
    if current & 0o077 != 0 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 0600 `{}`", path.display()))?;
        tracing::warn!(
            path = %path.display(),
            previous_mode = format!("{:o}", current),
            "Tightened auth file permissions to 0600"
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn enforce_restrictive_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
