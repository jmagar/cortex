use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};

use axum::{
    extract::{ConnectInfo, Query, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Semaphore;
use tower_http::cors::CorsLayer;

use crate::app::{
    AbuseSearchRequest, AckErrorRequest, AiCheckpointsRequest, AiCorrelateRequest,
    AiIncidentRequest, AiInvestigateRequest, AiParseErrorsRequest, AiPruneCheckpointsRequest,
    AnomaliesRequest, AskHistoryRequest, ClockSkewRequest, CompareRequest, CorrelateEventsRequest,
    DbCheckpointRequest, DbIntegrityRequest, DbVacuumRequest, FilterLogsRequest, GetErrorsRequest,
    GetLogRequest, IncidentContextRequest, IngestRateRequest, ListAiProjectsRequest,
    ListAiToolsRequest, ListAppsRequest, ListSessionsRequest, ListSourceIpsRequest,
    PatternsRequest, ProjectContextRequest, SearchLogsRequest, SearchSessionsRequest, ServiceError,
    SilentHostsRequest, SimilarIncidentsRequest, SyslogService, TailLogsRequest, TimelineRequest,
    UnackErrorRequest, UnaddressedErrorsRequest, UsageBlocksRequest,
};
use crate::config::ApiConfig;
use crate::db::DbPool;
use crate::mcp::{build_auth_layer, AuthPolicy};

/// Crate version cached at compile time (CARGO_PKG_VERSION).
const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Optional git SHA injected at build time via the `GIT_SHA` env var. When
/// absent we emit `None` so the `/api/version` JSON response omits the field
/// rather than rendering `null`.
const GIT_SHA: Option<&str> = option_env!("GIT_SHA");

/// Server-side hard cap for `events_per_anchor` on `/api/ai/correlate`. When
/// the caller-supplied value exceeds this, the response carries
/// `events_per_anchor_clamped_to: 50`. The service layer applies its own
/// (larger) clamp; this one defends the REST surface against accidental
/// `events_per_anchor=10000` requests blowing up the JSON payload.
const REST_CORRELATE_EVENTS_PER_ANCHOR_CAP: u32 = 50;

/// Server-side hard cap for `limit` on `/api/ai/search` + `/api/ai/abuse`.
/// When the caller-supplied value exceeds this, the response carries
/// `limit_clamped_to: 500` and `truncated: true`.
const REST_AI_LIMIT_CAP: u32 = 500;

/// Size threshold for the `POST /api/db/vacuum` full-vacuum pre-flight.
/// When the cached physical size exceeds this AND the request does NOT carry
/// `"force": true`, the handler returns 409 instead of starting a multi-minute
/// VACUUM that would block ingest. See `db_vacuum` for the dual-permit
/// design (eng-review C2/C3).
pub const FULL_VACUUM_SIZE_GUARD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Process-wide single-flight gate for the maintenance routes
/// (`POST /api/db/vacuum`, `POST /api/db/checkpoint`,
/// `POST /api/ai/prune-checkpoints`). Held via `ApiState::maintenance_permit`,
/// which clones the `Arc<Semaphore>` populated here at first call.
///
/// **Dual-permit pattern (eng-review C2)**: this gate is SEPARATE from
/// `SyslogService::db_permits` (the read-worker pool). Handlers
/// `try_acquire_owned` this permit BEFORE calling the service; on `NoPermits`
/// they return 409 with `{"error": "db maintenance already in progress"}`.
/// Holding the gate outside the read pool means VACUUM can't starve
/// concurrent reads (`GET /api/hosts`, etc.). The permit is held for the
/// whole handler call including response IO — see `ApiState::maintenance_permit`
/// for the intentional "whole-op gate" rationale (bead 0p8r.19).
///
/// **Process-wide invariant (bead 0p8r.18)**: a single `OnceLock` semaphore
/// is shared across every `ApiState` constructed in this process. The
/// invariant that vacuum/checkpoint cannot run concurrently relies on
/// production wiring one ApiState per process (the standard `main::run_server`
/// path satisfies this). Multiple ApiStates in one process would all see the
/// same gate — safe. Tests opt out of the global via
/// `ApiState::with_isolated_maintenance_permit`; see its doc for details.
static SHARED_MAINTENANCE_PERMIT: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn shared_maintenance_permit() -> Arc<Semaphore> {
    Arc::clone(SHARED_MAINTENANCE_PERMIT.get_or_init(|| Arc::new(Semaphore::new(1))))
}

/// Static snapshot of the server identity returned by `GET /api/version`.
/// Built once at `ApiState` construction; `/api/version` is a hot read path
/// for CLI health checks and must not touch SQLite per request (eng-review #A3).
#[derive(Clone, Debug, Serialize)]
pub struct VersionInfo {
    pub version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    pub schema_version: i64,
}

/// Shared mutable state for the /api/* router.
///
/// **One-pool-per-process invariant (bead 0p8r.18)**: `ApiState::new` clones
/// `maintenance_permit` from the process-wide [`SHARED_MAINTENANCE_PERMIT`]
/// `OnceLock`, so every router/listener in the process serializes against
/// the same single-flight gate. Constructing more than one `ApiState` in
/// production is supported but they all share the same maintenance gate by
/// design — vacuum cannot run twice concurrently per process.
///
/// **Maintenance-permit lifetime (bead 0p8r.19)**: `db_vacuum`,
/// `db_checkpoint`, and `prune_ai_checkpoints` hold the permit across the
/// awaited service call AND the JSON response serialization. This is the
/// intentional "whole-op gate" — on loopback the response IO is microseconds;
/// on a remote bind (SWAG) it's tens of ms. We accept this to keep the
/// 409 contract simple: while the route reports work, the gate is held.
#[derive(Clone)]
pub struct ApiState {
    pub service: SyslogService,
    pub config: ApiConfig,
    pub cors_port: u16,
    /// `true` when the MCP HTTP listener binds to a loopback address (e.g.
    /// `127.0.0.1` / `::1`). The CORS layer only emits the `localhost:{port}`
    /// and `127.0.0.1:{port}` allowlist entries when this is set; on external
    /// binds (homelab IP, Tailscale, etc.) those defaults are skipped because
    /// they'd let a malicious page on the operator's *workstation* speak to
    /// the remote API (bead 0p8r.21). `SYSLOG_MCP_ALLOWED_ORIGINS` is
    /// authoritative on external binds.
    pub loopback_bind: bool,
    /// Origins to allow via CORS (in addition to the default `cors_port`
    /// loopback variants when `loopback_bind` is true). Sourced from
    /// `SYSLOG_MCP_ALLOWED_ORIGINS` — single env shared with the /mcp
    /// surface. Mirrors `src/mcp/routes.rs:cors_layer`.
    pub allowed_origins: Vec<String>,
    /// Authentication policy. The `/api/*` router forces bearer enforcement
    /// regardless of this variant (see `router()`), so callers may pass any
    /// policy — the field is still carried so future per-route OAuth scope
    /// checks can read the shared `auth_state`.
    pub auth_policy: AuthPolicy,
    /// Cached server identity for `GET /api/version`.
    pub version_info: Arc<VersionInfo>,
    /// Test-overridable threshold for the `POST /api/db/vacuum` full-vacuum
    /// pre-flight (bytes). Defaults to [`FULL_VACUUM_SIZE_GUARD_BYTES`] in
    /// production via `ApiState::new`. Tests use
    /// `ApiState::with_full_vacuum_size_guard_bytes` to set a small value so
    /// they can drive the guard without seeding a multi-GB DB.
    pub full_vacuum_size_guard_bytes: u64,
    /// Single-flight gate for `POST /api/db/vacuum` and
    /// `POST /api/db/checkpoint`. In production this is a clone of the
    /// process-wide `SHARED_MAINTENANCE_PERMIT` so every router/listener in
    /// the process serializes against the same gate. See
    /// `SHARED_MAINTENANCE_PERMIT` docs for the dual-permit rationale
    /// (eng-review C2) and the test-isolation rationale.
    pub maintenance_permit: Arc<Semaphore>,
    /// When `true`, the static bearer token (`SYSLOG_MCP_TOKEN`) is granted
    /// `syslog:admin` scope in addition to `syslog:read`. Mirrors
    /// [`crate::config::McpConfig::static_token_is_admin`]. Default: `false`.
    pub static_token_is_admin: bool,
}

impl ApiState {
    /// Build an `ApiState`, querying the SQLite schema version once at
    /// startup. Caching avoids per-request DB hits on `/api/version`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        service: SyslogService,
        config: ApiConfig,
        cors_port: u16,
        loopback_bind: bool,
        allowed_origins: Vec<String>,
        auth_policy: AuthPolicy,
        pool: &DbPool,
        static_token_is_admin: bool,
    ) -> anyhow::Result<Self> {
        let schema_version = read_schema_version(pool)?;
        let version_info = Arc::new(VersionInfo {
            version: CRATE_VERSION,
            git_sha: GIT_SHA.map(str::to_string),
            schema_version,
        });
        Ok(Self {
            service,
            config,
            cors_port,
            loopback_bind,
            allowed_origins,
            auth_policy,
            version_info,
            full_vacuum_size_guard_bytes: FULL_VACUUM_SIZE_GUARD_BYTES,
            maintenance_permit: shared_maintenance_permit(),
            static_token_is_admin,
        })
    }

    /// Test-only constructor that replaces `maintenance_permit` with a fresh
    /// per-state `Arc<Semaphore>` so parallel tests don't contend on the
    /// process-wide `SHARED_MAINTENANCE_PERMIT`. Production code MUST use
    /// `ApiState::new` so vacuum/checkpoint serialize across the whole
    /// process.
    #[cfg(test)]
    pub fn with_isolated_maintenance_permit(mut self) -> Self {
        self.maintenance_permit = Arc::new(Semaphore::new(1));
        self
    }

    /// Test-only knob: lowers the full-vacuum pre-flight threshold so tests
    /// can drive the 409 path without seeding a multi-GB DB. Production code
    /// MUST NOT call this — the constant guards against multi-minute VACUUMs
    /// that block ingest.
    #[cfg(test)]
    pub fn with_full_vacuum_size_guard_bytes(mut self, bytes: u64) -> Self {
        self.full_vacuum_size_guard_bytes = bytes;
        self
    }
}

fn read_schema_version(pool: &DbPool) -> anyhow::Result<i64> {
    Ok(crate::db::read_schema_version_info(pool)?.version)
}

pub fn router(state: ApiState) -> anyhow::Result<Router> {
    if state.config.api_token.is_none() {
        anyhow::bail!(
            "SYSLOG_API_TOKEN required for the REST API — run 'syslog setup repair' to generate one"
        );
    }

    let routes = Router::new()
        // --- syslog queries ---
        .route("/api/search", get(search))
        .route("/api/filter", get(filter))
        .route("/api/tail", get(tail))
        .route("/api/errors", get(errors))
        .route("/api/hosts", get(hosts))
        .route("/api/correlate", get(correlate))
        .route("/api/stats", get(stats))
        .route("/api/version", get(version))
        // --- surface parity routes ---
        .route("/api/source-ips", get(source_ips))
        .route("/api/timeline", get(timeline))
        .route("/api/patterns", get(patterns))
        .route("/api/ingest-rate", get(ingest_rate))
        .route("/api/get", get(get_log))
        .route("/api/errors/unaddressed", get(unaddressed_errors))
        .route("/api/errors/ack", post(ack_error))
        .route("/api/errors/unack", post(unack_error))
        .route("/api/notifications/recent", get(notifications_recent))
        .route("/api/notifications/test", post(notifications_test))
        // --- surface parity gap closure (12 new routes) ---
        .route("/api/silent-hosts", get(silent_hosts))
        .route("/api/clock-skew", get(clock_skew))
        .route("/api/anomalies", get(anomalies))
        .route("/api/compare", get(compare))
        .route("/api/apps", get(apps))
        .route("/api/similar-incidents", get(similar_incidents))
        .route("/api/incident-context", get(incident_context))
        .route("/api/ai/ask-history", get(ai_ask_history))
        .route("/api/ai/incidents", get(ai_incidents))
        .route("/api/ai/investigate", get(ai_investigate))
        .route("/api/compose/status", get(compose_status))
        .route("/api/compose/doctor", get(compose_doctor))
        // --- ai session queries ---
        .route("/api/sessions", get(sessions))
        .route("/api/ai/search", get(ai_search))
        .route("/api/ai/abuse", get(ai_abuse))
        .route("/api/ai/correlate", get(ai_correlate))
        .route("/api/ai/blocks", get(ai_blocks))
        .route("/api/ai/context", get(ai_context))
        .route("/api/ai/tools", get(ai_tools))
        .route("/api/ai/projects", get(ai_projects))
        // --- ai diagnostic + admin (bead 0p8r.3) ---
        .route("/api/ai/checkpoints", get(ai_checkpoints))
        .route("/api/ai/errors", get(ai_parse_errors))
        .route("/api/ai/prune-checkpoints", post(ai_prune_checkpoints))
        // --- db ops (bead 0p8r.4) ---
        .route("/api/db/status", get(db_status))
        .route("/api/db/integrity", get(db_integrity))
        .route("/api/db/checkpoint", post(db_checkpoint))
        .route("/api/db/vacuum", post(db_vacuum));

    // Force `AuthPolicy::Mounted` on /api/* regardless of the listener bind.
    // Loopback callers (CLI on the same host) MUST still present a bearer
    // token — the single-token model documented for /api/* and /mcp depends
    // on this invariant (eng-review C1).
    let forced_policy = match &state.auth_policy {
        AuthPolicy::LoopbackDev => AuthPolicy::Mounted { auth_state: None },
        AuthPolicy::Mounted { auth_state } => AuthPolicy::Mounted {
            auth_state: auth_state.clone(),
        },
    };
    let routes = if let Some(layer) = build_auth_layer(
        &forced_policy,
        state.config.api_token.as_deref().map(Arc::<str>::from),
        None,
        state.static_token_is_admin,
    ) {
        routes.layer(layer)
    } else {
        // `forced_policy` is always `Mounted`, so `build_auth_layer` returns
        // `Some(_)`. Reach here only if `build_auth_layer` ever changes its
        // contract — fail loud rather than mount routes without auth.
        anyhow::bail!("internal: auth layer construction returned None for /api/* (forced Mounted)")
    };

    let cors = cors_layer(state.cors_port, state.loopback_bind, &state.allowed_origins);
    let routes = routes.layer(cors).with_state(state);
    Ok(routes)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchQuery {
    query: Option<String>,
    hostname: Option<String>,
    source_ip: Option<String>,
    severity: Option<String>,
    app_name: Option<String>,
    facility: Option<String>,
    exclude_facility: Option<String>,
    process_id: Option<String>,
    from: Option<String>,
    to: Option<String>,
    received_from: Option<String>,
    received_to: Option<String>,
    limit: Option<u32>,
    source_kind: Option<String>,
    tool: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
    container: Option<String>,
    docker_host: Option<String>,
    stream: Option<String>,
    event_action: Option<String>,
}

async fn search(
    State(state): State<ApiState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .search_logs(SearchLogsRequest {
                query: query.query,
                hostname: query.hostname,
                source_ip: query.source_ip,
                severity: query.severity,
                app_name: query.app_name,
                facility: query.facility,
                exclude_facility: query.exclude_facility,
                process_id: query.process_id,
                from: query.from,
                to: query.to,
                received_from: query.received_from,
                received_to: query.received_to,
                limit: query.limit,
                source_kind: query.source_kind,
                tool: query.tool,
                project: query.project,
                session_id: query.session_id,
                container: query.container,
                docker_host: query.docker_host,
                stream: query.stream,
                event_action: query.event_action,
            })
            .await,
    )
}

async fn filter(
    State(state): State<ApiState>,
    Query(query): Query<FilterLogsRequest>,
) -> impl IntoResponse {
    respond(state.service.filter_logs(query).await)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TailQuery {
    hostname: Option<String>,
    source_ip: Option<String>,
    app_name: Option<String>,
    severity_min: Option<String>,
    n: Option<u32>,
}

async fn tail(State(state): State<ApiState>, Query(query): Query<TailQuery>) -> impl IntoResponse {
    respond(
        state
            .service
            .tail_logs(TailLogsRequest {
                hostname: query.hostname,
                source_ip: query.source_ip,
                app_name: query.app_name,
                severity_min: query.severity_min,
                n: query.n,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorQuery {
    from: Option<String>,
    to: Option<String>,
    group_by: Option<String>,
    limit: Option<u32>,
}

async fn errors(
    State(state): State<ApiState>,
    Query(query): Query<ErrorQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .get_errors(GetErrorsRequest {
                from: query.from,
                to: query.to,
                group_by: query.group_by,
                limit: query.limit,
            })
            .await,
    )
}

async fn hosts(State(state): State<ApiState>) -> impl IntoResponse {
    respond(state.service.list_hosts().await)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorrelateQuery {
    reference_time: String,
    window_minutes: Option<u32>,
    severity_min: Option<String>,
    hostname: Option<String>,
    source_ip: Option<String>,
    query: Option<String>,
    limit: Option<u32>,
}

async fn correlate(
    State(state): State<ApiState>,
    Query(query): Query<CorrelateQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .correlate_events(CorrelateEventsRequest {
                reference_time: query.reference_time,
                window_minutes: query.window_minutes,
                severity_min: query.severity_min,
                hostname: query.hostname,
                source_ip: query.source_ip,
                query: query.query,
                limit: query.limit,
            })
            .await,
    )
}

async fn stats(State(state): State<ApiState>) -> impl IntoResponse {
    respond(state.service.get_stats().await)
}

/// `GET /api/version` — returns the cached server identity. SQLite is NOT
/// queried per request; `schema_version` is captured once at startup.
async fn version(State(state): State<ApiState>) -> impl IntoResponse {
    Json((*state.version_info).clone()).into_response()
}

// ─── Surface parity routes ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SourceIpsQuery {
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn source_ips(
    State(state): State<ApiState>,
    Query(query): Query<SourceIpsQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .list_source_ips(ListSourceIpsRequest {
                limit: query.limit,
                offset: query.offset,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
struct TimelineQuery {
    bucket: Option<String>,
    group_by: Option<String>,
    from: Option<String>,
    to: Option<String>,
    hostname: Option<String>,
    app_name: Option<String>,
    severity_min: Option<String>,
}

async fn timeline(
    State(state): State<ApiState>,
    Query(query): Query<TimelineQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .timeline(TimelineRequest {
                bucket: query.bucket,
                group_by: query.group_by,
                from: query.from,
                to: query.to,
                hostname: query.hostname,
                app_name: query.app_name,
                severity_min: query.severity_min,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
struct PatternsQuery {
    from: Option<String>,
    to: Option<String>,
    hostname: Option<String>,
    app_name: Option<String>,
    severity_min: Option<String>,
    scan_limit: Option<u32>,
    top_n: Option<u32>,
}

async fn patterns(
    State(state): State<ApiState>,
    Query(query): Query<PatternsQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .patterns(PatternsRequest {
                from: query.from,
                to: query.to,
                hostname: query.hostname,
                app_name: query.app_name,
                severity_min: query.severity_min,
                scan_limit: query.scan_limit,
                top_n: query.top_n,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
struct IngestRateQuery {
    by_host: Option<bool>,
}

async fn ingest_rate(
    State(state): State<ApiState>,
    Query(query): Query<IngestRateQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .ingest_rate(IngestRateRequest {
                by_host: query.by_host,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
struct GetLogQuery {
    id: i64,
}

async fn get_log(
    State(state): State<ApiState>,
    Query(query): Query<GetLogQuery>,
) -> impl IntoResponse {
    respond(state.service.get_log(GetLogRequest { id: query.id }).await)
}

#[derive(Debug, Deserialize)]
struct UnaddressedErrorsQuery {
    limit: Option<u32>,
    include_acknowledged: Option<bool>,
}

async fn unaddressed_errors(
    State(state): State<ApiState>,
    Query(query): Query<UnaddressedErrorsQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .unaddressed_errors(UnaddressedErrorsRequest {
                limit: query.limit,
                include_acknowledged: query.include_acknowledged,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
struct AckErrorBody {
    signature_hash: String,
    notes: Option<String>,
}

async fn ack_error(
    State(state): State<ApiState>,
    Json(body): Json<AckErrorBody>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .ack_error(
                AckErrorRequest {
                    signature_hash: body.signature_hash,
                    notes: body.notes,
                },
                "api",
            )
            .await,
    )
}

#[derive(Debug, Deserialize)]
struct UnackErrorBody {
    signature_hash: String,
    reason: Option<String>,
}

async fn unack_error(
    State(state): State<ApiState>,
    Json(body): Json<UnackErrorBody>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .unack_error(
                UnackErrorRequest {
                    signature_hash: body.signature_hash,
                    reason: body.reason,
                },
                "api",
            )
            .await,
    )
}

#[derive(Debug, Deserialize)]
struct NotificationsRecentQuery {
    limit: Option<i64>,
    rule_id: Option<String>,
    since: Option<String>,
}

async fn notifications_recent(
    State(state): State<ApiState>,
    Query(query): Query<NotificationsRecentQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .notifications_recent(
                query.limit.unwrap_or(50).clamp(1, 500),
                query.rule_id,
                query.since,
            )
            .await,
    )
}

async fn notifications_test() -> impl IntoResponse {
    (
        axum::http::StatusCode::NOT_IMPLEMENTED,
        "notifications_test requires server-side apprise config; use MCP notify test instead",
    )
}

// ─── Surface parity gap closure (12 new handlers) ───────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SilentHostsQuery {
    silent_minutes: Option<u32>,
}

async fn silent_hosts(
    State(state): State<ApiState>,
    Query(query): Query<SilentHostsQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .silent_hosts(SilentHostsRequest {
                silent_minutes: query.silent_minutes,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClockSkewQuery {
    since: Option<String>,
    limit: Option<u32>,
}

async fn clock_skew(
    State(state): State<ApiState>,
    Query(query): Query<ClockSkewQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .clock_skew(ClockSkewRequest {
                since: query.since,
                limit: query.limit,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnomaliesQuery {
    recent_minutes: Option<u32>,
    baseline_minutes: Option<u32>,
}

async fn anomalies(
    State(state): State<ApiState>,
    Query(query): Query<AnomaliesQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .anomalies(AnomaliesRequest {
                recent_minutes: query.recent_minutes,
                baseline_minutes: query.baseline_minutes,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompareQuery {
    a_from: String,
    a_to: String,
    b_from: String,
    b_to: String,
}

async fn compare(
    State(state): State<ApiState>,
    Query(query): Query<CompareQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .compare(CompareRequest {
                a_from: query.a_from,
                a_to: query.a_to,
                b_from: query.b_from,
                b_to: query.b_to,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppsQuery {
    hostname: Option<String>,
    from: Option<String>,
    to: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn apps(State(state): State<ApiState>, Query(query): Query<AppsQuery>) -> impl IntoResponse {
    respond(
        state
            .service
            .list_apps(ListAppsRequest {
                hostname: query.hostname,
                from: query.from,
                to: query.to,
                limit: query.limit,
                offset: query.offset,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SimilarIncidentsQuery {
    query: String,
    hostname: Option<String>,
    app_name: Option<String>,
    severity_min: Option<String>,
    from: Option<String>,
    to: Option<String>,
    window_minutes: Option<u32>,
    limit: Option<u32>,
}

async fn similar_incidents(
    State(state): State<ApiState>,
    Query(q): Query<SimilarIncidentsQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .similar_incidents(SimilarIncidentsRequest {
                query: q.query,
                hostname: q.hostname,
                app_name: q.app_name,
                severity_min: q.severity_min,
                from: q.from,
                to: q.to,
                window_minutes: q.window_minutes,
                limit: q.limit,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IncidentContextQuery {
    from: String,
    to: String,
    hostname: Option<String>,
    app_name: Option<String>,
    query: Option<String>,
    severity_min: Option<String>,
    limit: Option<u32>,
}

async fn incident_context(
    State(state): State<ApiState>,
    Query(q): Query<IncidentContextQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .incident_context(IncidentContextRequest {
                from: q.from,
                to: q.to,
                hostname: q.hostname,
                app_name: q.app_name,
                query: q.query,
                severity_min: q.severity_min,
                limit: q.limit,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AskHistoryQuery {
    query: String,
    hostname: Option<String>,
    app_name: Option<String>,
    from: Option<String>,
    to: Option<String>,
    limit: Option<u32>,
}

async fn ai_ask_history(
    State(state): State<ApiState>,
    Query(q): Query<AskHistoryQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .ask_history(AskHistoryRequest {
                query: q.query,
                hostname: q.hostname,
                app_name: q.app_name,
                from: q.from,
                to: q.to,
                limit: q.limit,
            })
            .await,
    )
}

/// AI incidents — uses `QsQuery` because `terms: Vec<String>` cannot be
/// deserialized from a URL query string via `axum::extract::Query`
/// (which uses `serde_urlencoded`). Mirrors `ai_abuse` above.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AiIncidentsQuery {
    project: Option<String>,
    tool: Option<String>,
    from: Option<String>,
    to: Option<String>,
    limit: Option<u32>,
    window_minutes: Option<u32>,
    #[serde(default)]
    terms: Vec<String>,
}

async fn ai_incidents(
    State(state): State<ApiState>,
    serde_qs::axum::QsQuery(q): serde_qs::axum::QsQuery<AiIncidentsQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .list_ai_incidents(AiIncidentRequest {
                project: q.project,
                tool: q.tool,
                from: q.from,
                to: q.to,
                limit: q.limit,
                window_minutes: q.window_minutes,
                terms: q.terms,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AiInvestigateQuery {
    project: Option<String>,
    tool: Option<String>,
    from: Option<String>,
    to: Option<String>,
    limit: Option<u32>,
    window_minutes: Option<u32>,
    correlation_window_minutes: Option<u32>,
    #[serde(default)]
    terms: Vec<String>,
}

async fn ai_investigate(
    State(state): State<ApiState>,
    serde_qs::axum::QsQuery(q): serde_qs::axum::QsQuery<AiInvestigateQuery>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .investigate_ai_incidents(AiInvestigateRequest {
                incident_id: None,
                project: q.project,
                tool: q.tool,
                from: q.from,
                to: q.to,
                limit: q.limit,
                window_minutes: q.window_minutes,
                correlation_window_minutes: q.correlation_window_minutes,
                terms: q.terms,
            })
            .await,
    )
}

/// Run `crate::compose::ComposeService::status()` on a blocking task, gated
/// by a process-wide semaphore so multiple concurrent REST callers cannot
/// spawn unbounded `docker inspect` subprocesses. Mirrors the helper in
/// `src/mcp/tools.rs:412-433` (`compose_status`).
async fn compose_status_inner() -> anyhow::Result<crate::compose::ComposeStatus> {
    static COMPOSE_REST_DIAGNOSTICS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::OnceLock::new();
    let permit = COMPOSE_REST_DIAGNOSTICS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| anyhow::anyhow!("compose diagnostics limiter closed: {e}"))?;
    let service = crate::compose::ComposeService::new(
        crate::compose::CliDockerInspect,
        crate::compose::ProcessRunner,
        crate::compose::ComposeDefaults::default(),
    );
    let status = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        service.status(&crate::compose::ComposeTarget::default())
    })
    .await
    .map_err(|e| anyhow::anyhow!("compose status task failed: {e}"))??;
    Ok(status)
}

async fn compose_status() -> impl IntoResponse {
    match compose_status_inner().await {
        Ok(status) => respond::<_>(Ok(crate::compose::mcp_projection(&status))),
        Err(e) => respond::<crate::compose::ComposeMcpStatus>(Err(ServiceError::Internal(
            anyhow::anyhow!("compose status: {e}"),
        ))),
    }
}

async fn compose_doctor() -> impl IntoResponse {
    let status = match compose_status_inner().await {
        Ok(s) => s,
        Err(e) => {
            return respond::<crate::compose::ComposeMcpStatus>(Err(ServiceError::Internal(
                anyhow::anyhow!("compose doctor status: {e}"),
            )));
        }
    };
    if let Err(e) = crate::compose::ensure_doctor_ready(&status) {
        return compose_doctor_unready_response(&status, e);
    }
    respond::<_>(Ok(crate::compose::mcp_projection(&status)))
}

fn compose_doctor_unready_response(
    status: &crate::compose::ComposeStatus,
    error: anyhow::Error,
) -> axum::response::Response {
    tracing::warn!(error = %error, "Compose doctor readiness check failed");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(crate::compose::mcp_projection(status)),
    )
        .into_response()
}

// ─── AI session queries ─────────────────────────────────────────────────────

async fn sessions(
    State(state): State<ApiState>,
    Query(req): Query<ListSessionsRequest>,
) -> impl IntoResponse {
    respond(state.service.list_sessions(req).await)
}

/// Returns `Some(cap)` if `value` exceeds `cap`, otherwise `None`. Used by
/// the three AI handlers below to detect-and-report a server-side clamp on
/// caller-supplied limits in a single line (bead 0p8r.30).
fn clamp_to(value: Option<u32>, cap: u32) -> Option<u32> {
    value.filter(|&supplied| supplied > cap).map(|_| cap)
}

async fn ai_search(
    State(state): State<ApiState>,
    Query(mut req): Query<SearchSessionsRequest>,
) -> impl IntoResponse {
    let clamped = clamp_to(req.limit, REST_AI_LIMIT_CAP);
    if let Some(cap) = clamped {
        req.limit = Some(cap);
    }
    let mut response = match state.service.search_sessions(req).await {
        Ok(v) => v,
        Err(err) => return respond::<()>(Err(err)),
    };
    if let Some(cap) = clamped {
        response.limit_clamped_to = Some(cap);
        response.truncated = true;
    }
    Json(response).into_response()
}

/// `/api/ai/abuse` deserializes directly into [`AbuseSearchRequest`] via
/// `serde_qs::axum::QsQuery`, which handles `Vec<String>` from repeated
/// `?terms=a&terms=b` (and `?terms[]=a&terms[]=b`) query params — something
/// the default `serde_urlencoded` backing of `axum::extract::Query` cannot do
/// (bead 0p8r.15: closes the wire-shape duplication seam).
async fn ai_abuse(
    State(state): State<ApiState>,
    serde_qs::axum::QsQuery(mut req): serde_qs::axum::QsQuery<AbuseSearchRequest>,
) -> impl IntoResponse {
    let clamped = clamp_to(req.limit, REST_AI_LIMIT_CAP);
    if let Some(cap) = clamped {
        req.limit = Some(cap);
    }
    let mut response = match state.service.search_abuse(req).await {
        Ok(v) => v,
        Err(err) => return respond::<()>(Err(err)),
    };
    if let Some(cap) = clamped {
        response.limit_clamped_to = Some(cap);
        response.truncated = true;
    }
    Json(response).into_response()
}

async fn ai_correlate(
    State(state): State<ApiState>,
    Query(mut req): Query<AiCorrelateRequest>,
) -> impl IntoResponse {
    // Clamp `events_per_anchor` to REST_CORRELATE_EVENTS_PER_ANCHOR_CAP.
    // Mark the response when the caller asked for more so clients know
    // their value was reduced.
    let clamped = clamp_to(req.events_per_anchor, REST_CORRELATE_EVENTS_PER_ANCHOR_CAP);
    if let Some(cap) = clamped {
        req.events_per_anchor = Some(cap);
    }
    let mut response = match state.service.correlate_ai_logs(req).await {
        Ok(v) => v,
        Err(err) => return respond::<()>(Err(err)),
    };
    if let Some(cap) = clamped {
        response.events_per_anchor_clamped_to = Some(cap);
    }
    Json(response).into_response()
}

async fn ai_blocks(
    State(state): State<ApiState>,
    Query(req): Query<UsageBlocksRequest>,
) -> impl IntoResponse {
    respond(state.service.usage_blocks(req).await)
}

async fn ai_context(
    State(state): State<ApiState>,
    Query(req): Query<ProjectContextRequest>,
) -> impl IntoResponse {
    // `project` is required by the service, but axum/serde happily accepts
    // empty strings. Eng-review #A7: reject empty up-front with a 400 so
    // callers don't get an empty-result 200 instead of a clear error.
    if req.project.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "project query parameter is required and must be non-empty"})),
        )
            .into_response();
    }
    respond(state.service.project_context(req).await)
}

async fn ai_tools(
    State(state): State<ApiState>,
    Query(req): Query<ListAiToolsRequest>,
) -> impl IntoResponse {
    respond(state.service.list_ai_tools(req).await)
}

async fn ai_projects(
    State(state): State<ApiState>,
    Query(req): Query<ListAiProjectsRequest>,
) -> impl IntoResponse {
    respond(state.service.list_ai_projects(req).await)
}

// ─── AI diagnostic + admin (bead 0p8r.3) ─────────────────────────────────────
//
// `list_ai_checkpoints`, `list_ai_parse_errors`, `prune_ai_checkpoints` keep
// their loose primitive signatures on `SyslogService` (eng-review #S3 — the
// service refactor was cut). Handlers build the typed Request struct from
// query/body, then unpack into positional args.

/// `GET /api/ai/checkpoints` — inventory of AI transcript checkpoints (read).
async fn ai_checkpoints(
    State(state): State<ApiState>,
    Query(req): Query<AiCheckpointsRequest>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .list_ai_checkpoints(req.errors_only, req.missing_only, req.limit)
            .await,
    )
}

/// `GET /api/ai/errors` — recent transcript parse errors (read).
async fn ai_parse_errors(
    State(state): State<ApiState>,
    Query(req): Query<AiParseErrorsRequest>,
) -> impl IntoResponse {
    respond(state.service.list_ai_parse_errors(req.limit).await)
}

/// `POST /api/ai/prune-checkpoints` — admin/destructive: delete checkpoints
/// from the AI transcript inventory.
///
/// Validation flow (eng-review C3 — defense against `POST {}` mass-delete):
/// 1. Deserialize the body as `serde_json::Value` first.
/// 2. If the `dry_run` key is absent → 400 `"dry_run is required and must be
///    specified explicitly"`. Do NOT default to `false`.
/// 3. Then deserialize the value into `AiPruneCheckpointsRequest`
///    (`deny_unknown_fields` catches typos).
///
/// Audit log (eng-review #A13 / security #35): fires `tracing::warn!` BEFORE
/// the service call so a crash mid-prune still leaves an audit row.
///
/// `caller_ip` is sourced from `ConnectInfo<SocketAddr>`. Production wires it
/// via `into_make_service_with_connect_info` (see `src/main.rs:565`); tests
/// drive the router through a `MockConnectInfo` layer because
/// `tower::ServiceExt::oneshot` does not populate `ConnectInfo` on its own.
async fn ai_prune_checkpoints(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    // Step 1+2: parse as Value, require `dry_run` key explicitly.
    let value: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("invalid JSON body: {err}")})),
            )
                .into_response();
        }
    };
    let obj = match value.as_object() {
        Some(obj) => obj,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "request body must be a JSON object"})),
            )
                .into_response();
        }
    };
    if !obj.contains_key("dry_run") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "dry_run is required and must be specified explicitly"
            })),
        )
            .into_response();
    }

    // Step 3: typed deserialize — `deny_unknown_fields` rejects typos.
    let req: AiPruneCheckpointsRequest = match serde_json::from_value(value) {
        Ok(req) => req,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("invalid request body: {err}")})),
            )
                .into_response();
        }
    };

    // Audit BEFORE the service call so a process crash mid-prune still
    // leaves a trace of who asked for what.
    tracing::warn!(
        caller_ip = %peer,
        action = "prune_ai_checkpoints",
        dry_run = req.dry_run,
        missing_only = req.missing_only,
        limit = ?req.limit,
        "admin: prune_ai_checkpoints invoked"
    );

    // Single-flight gate — prune competes with vacuum/checkpoint for the
    // SQLite writer lock, so it joins the same MAINTENANCE_PERMIT cohort to
    // give callers a uniform 409 contract during concurrent maintenance
    // (bead 0p8r.16). Without the gate, concurrent prune+vacuum surfaces as
    // SQLITE_BUSY/timeout to clients instead of a clean 409.
    let _permit = match Arc::clone(&state.maintenance_permit).try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({"error": "db maintenance already in progress"})),
            )
                .into_response();
        }
    };

    respond(
        state
            .service
            .prune_ai_checkpoints(req.missing_only, req.dry_run, req.limit)
            .await,
    )
}

fn respond<T: serde::Serialize>(result: crate::app::ServiceResult<T>) -> axum::response::Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(crate::app::ServiceError::InvalidInput(msg)) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": msg}))).into_response()
        }
        Err(crate::app::ServiceError::Busy(msg)) => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": msg}))).into_response()
        }
        Err(crate::app::ServiceError::NotFound(msg)) => {
            (StatusCode::NOT_FOUND, Json(json!({"error": msg}))).into_response()
        }
        Err(crate::app::ServiceError::DatabaseTimeout) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "database_timeout"})),
        )
            .into_response(),
        Err(crate::app::ServiceError::ConstraintViolation { message }) => {
            tracing::warn!(error = %message, "Constraint violation in API request");
            (
                StatusCode::CONFLICT,
                Json(json!({"error": "constraint_violation", "detail": message})),
            )
                .into_response()
        }
        Err(crate::app::ServiceError::RowNotFound) => {
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(crate::app::ServiceError::Internal(err)) => {
            tracing::error!(error = %err, "API request failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal_error"})),
            )
                .into_response()
        }
    }
}

fn cors_layer(port: u16, loopback_bind: bool, allowed_origins: &[String]) -> CorsLayer {
    // Default loopback origins are only useful when the API actually listens
    // on a loopback address — otherwise they grant CORS access from the
    // operator's *workstation* (where `localhost:port` points at unrelated
    // services) to a remote API (bead 0p8r.21). On external binds,
    // `SYSLOG_MCP_ALLOWED_ORIGINS` is the only authority.
    let mut origins: Vec<HeaderValue> = if loopback_bind {
        vec![
            format!("http://localhost:{port}")
                .parse::<HeaderValue>()
                .expect("valid localhost origin"),
            format!("http://127.0.0.1:{port}")
                .parse::<HeaderValue>()
                .expect("valid 127.0.0.1 origin"),
            // IPv6 loopback — when the listener binds [::1] or :: the
            // browser sends an Origin like http://[::1]:port and would
            // otherwise be blocked by CORS.
            format!("http://[::1]:{port}")
                .parse::<HeaderValue>()
                .expect("valid ::1 origin"),
        ]
    } else {
        Vec::new()
    };
    for origin in allowed_origins {
        match origin.parse::<HeaderValue>() {
            Ok(value) => origins.push(value),
            Err(error) => {
                tracing::warn!(
                    origin = %origin,
                    error = %error,
                    "Ignoring invalid CORS origin from SYSLOG_MCP_ALLOWED_ORIGINS"
                );
            }
        }
    }
    // GET for reads, POST for mutating endpoints (added with bead 0p8r.3 —
    // first POST route is /api/ai/prune-checkpoints), OPTIONS so browser
    // preflights for the POST endpoint succeed.
    //
    // `allow_headers` is an explicit allowlist (bead 0p8r.14): bearer auth
    // still defends every request, but pinning the preflight surface to the
    // headers the API actually reads keeps a compromised allowed-origin page
    // from echoing arbitrary headers (cookies from other origins, custom auth
    // tokens) through the browser into POST /api/ai/prune-checkpoints,
    // /api/db/vacuum, /api/db/checkpoint.
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::header::ACCEPT,
        ])
}

// ─── DB ops (bead 0p8r.4) ────────────────────────────────────────────────────
//
// Maintenance routes use the dual-permit pattern described on
// `MAINTENANCE_PERMIT` above: vacuum/checkpoint hold MAINTENANCE_PERMIT for the
// duration of the awaited service call, while reads continue to acquire from
// `SyslogService::db_permits` independently. `db_status` and `db_integrity` are
// read-side and bypass MAINTENANCE_PERMIT entirely.

/// `GET /api/db/status` — cached PRAGMA snapshot (read).
async fn db_status(State(state): State<ApiState>) -> impl IntoResponse {
    respond(state.service.db_status().await)
}

/// `GET /api/db/integrity` — full or `?quick=true` integrity check (read).
async fn db_integrity(
    State(state): State<ApiState>,
    Query(req): Query<DbIntegrityRequest>,
) -> impl IntoResponse {
    respond(state.service.db_integrity(req.quick).await)
}

/// Allowed values for `DbCheckpointRequest::mode` (validated at handler entry
/// per bead 0p8r.4 #A17). SQLite would also reject unknown modes, but explicit
/// validation gives a clearer 400 with the allowed list.
const CHECKPOINT_ALLOWED_MODES: &[&str] = &["passive", "full", "restart", "truncate"];

/// `POST /api/db/checkpoint` — admin: `PRAGMA wal_checkpoint(<mode>)`.
///
/// Uses MAINTENANCE_PERMIT (dual-permit pattern — see `MAINTENANCE_PERMIT`
/// docs). On contention returns 409 immediately rather than queuing.
async fn db_checkpoint(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let req: DbCheckpointRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("invalid request body: {err}")})),
            )
                .into_response();
        }
    };

    // Audit BEFORE mode validation (bead 0p8r.22) so rejected 400s are also
    // recorded; otherwise an attacker can probe `mode=evil` indefinitely
    // without leaving a trace. Audit BEFORE the service call so a process
    // crash mid-checkpoint also leaves a row of who asked for what.
    let mode_lower = req.mode.to_ascii_lowercase();
    tracing::warn!(
        caller_ip = %peer,
        action = "db_checkpoint",
        mode = %mode_lower,
        "admin: db_checkpoint invoked"
    );

    // Validate mode (bead 0p8r.4 #A17). SQLite would also reject unknown
    // modes, but an explicit allowlist gives a clearer 400.
    if !CHECKPOINT_ALLOWED_MODES.contains(&mode_lower.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!(
                    "mode must be one of: {}",
                    CHECKPOINT_ALLOWED_MODES.join(", ")
                )
            })),
        )
            .into_response();
    }

    // Single-flight gate — separate from the read-worker pool (eng-review C2).
    // See `maintenance_permit` field docs on ApiState.
    let _permit = match Arc::clone(&state.maintenance_permit).try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({"error": "db maintenance already in progress"})),
            )
                .into_response();
        }
    };

    respond(state.service.db_checkpoint(mode_lower).await)
}

/// `POST /api/db/vacuum` — admin: full or incremental VACUUM.
///
/// Flow:
/// 1. Deserialize the body. `force` is `Option<bool>` so the size pre-flight
///    only relaxes when the body explicitly carries `"force": true`.
/// 2. Audit log (`tracing::warn!`) BEFORE any other work.
/// 3. Acquire MAINTENANCE_PERMIT (single-flight, dual-permit pattern —
///    see `MAINTENANCE_PERMIT` docs). On contention return 409.
/// 4. Size pre-flight when `full == true && force != Some(true)`: read
///    a FRESH `page_count * page_size` via the service (bead 0p8r.17 —
///    cached snapshots cannot defend a gate after weeks of ingest growth)
///    and 409 if `> full_vacuum_size_guard_bytes`.
/// 5. Call the service.
async fn db_vacuum(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let req: DbVacuumRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("invalid request body: {err}")})),
            )
                .into_response();
        }
    };

    // Audit BEFORE service call so a process crash mid-vacuum leaves a trace.
    tracing::warn!(
        caller_ip = %peer,
        action = "db_vacuum",
        full = req.full,
        force = ?req.force,
        incremental_pages = req.incremental_pages,
        "admin: db_vacuum invoked"
    );

    // Single-flight gate FIRST so two concurrent callers can't both pass the
    // size pre-flight and then both queue inside run_db. Acquired from
    // `state.maintenance_permit` (NOT the read-worker pool — eng-review C2).
    let _permit = match Arc::clone(&state.maintenance_permit).try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({"error": "db maintenance already in progress"})),
            )
                .into_response();
        }
    };

    // Size pre-flight (bead 0p8r.4 / eng-review C3, bead 0p8r.17). Only
    // applies to full VACUUM, and only when force is NOT explicitly true.
    // The size is read FRESH from `page_count * page_size` on every call so
    // a long-running container (months of ingest growth) cannot defeat the
    // guard with a stale startup snapshot.
    if req.full && req.force != Some(true) {
        let size = match state.service.db_logical_size_bytes().await {
            Ok(bytes) => bytes,
            Err(err) => return respond::<()>(Err(err)),
        };
        if size > state.full_vacuum_size_guard_bytes {
            let gb = size as f64 / (1024.0 * 1024.0 * 1024.0);
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": format!(
                        "DB size {gb:.2} GB; full VACUUM would block ingest. Pass {{\"force\":true}} or use incremental"
                    )
                })),
            )
                .into_response();
        }
    }

    respond(
        state
            .service
            .db_vacuum(req.full, req.incremental_pages)
            .await,
    )
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
