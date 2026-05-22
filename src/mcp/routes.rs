use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use serde_json::json;
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer};

use super::rmcp_server::allowed_origins;
use super::{build_auth_layer, streamable_http_config, streamable_http_service};
use super::{AppState, AuthPolicy};

const MCP_BODY_LIMIT_BYTES: u64 = 65_536;
const MCP_PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";
const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";

/// Build the MCP router
pub fn router(state: AppState) -> Router {
    // Authenticated RMCP Streamable HTTP endpoint.
    // /health is unauthenticated — Docker HEALTHCHECK, Compose, and SWAG reach it.
    // /health/full is auth-gated and returns OTLP counters + ingest observability.
    let rmcp_config = streamable_http_config(&state.config);
    let mcp_service =
        Router::new().nest_service("/mcp", streamable_http_service(state.clone(), rmcp_config));

    // Apply auth layer based on policy (see `build_auth_layer` for invariants).
    // `resource_url` is only used when a layer is actually mounted, so compute
    // it lazily inside the Mounted branch via the helper's Option parameter.
    let resource_url = match &state.auth_policy {
        AuthPolicy::Mounted { .. } => state
            .config
            .auth
            .public_url
            .as_deref()
            .map(|u| Arc::<str>::from(format!("{}/mcp", u.trim_end_matches('/')))),
        AuthPolicy::LoopbackDev => None,
    };
    let authenticated = if let Some(layer) = build_auth_layer(
        &state.auth_policy,
        state.config.api_token.as_deref().map(Arc::<str>::from),
        resource_url,
        state.config.static_token_is_admin,
    ) {
        mcp_service.layer(layer)
    } else {
        mcp_service
    };

    // Build the OAuth router (Router<()> — state already baked in) when
    // auth_state is Some (OAuth mode active). These routes ARE the auth flow
    // and must be unauthenticated. They are merged before applying AppState so
    // that axum's type-checker sees a consistent Router<AppState> merge target.
    //
    // Locked Decision: OAuth router only when auth_state: Some(_).
    // bearer-only (auth_state: None) and LoopbackDev have no OAuth routes.
    //
    // Locked Decision: use lab-auth's headless subset. /register and
    // /auth/login are NOT in bearer_only_router (confirmed by lab-auth's
    // BEARER_ONLY_ROUTER_FORBIDDEN_PATHS snapshot test).
    let oauth_router: Option<Router> = if let AuthPolicy::Mounted {
        auth_state: Some(ref state_arc),
    } = state.auth_policy
    {
        tracing::info!(
            "OAuth router mounted: /.well-known/oauth-authorization-server, \
                 /.well-known/oauth-protected-resource, /mcp/.well-known/*, \
                 /jwks, /authorize, /auth/google/callback, /token"
        );
        let auth_state = state_arc.as_ref().clone();
        let path_based_discovery = Router::new()
            .route(
                "/mcp/.well-known/oauth-authorization-server",
                get(lab_auth::metadata::authorization_server_metadata),
            )
            .route(
                "/mcp/.well-known/openid-configuration",
                get(lab_auth::metadata::authorization_server_metadata),
            )
            .route(
                "/mcp/.well-known/oauth-protected-resource",
                get(lab_auth::metadata::protected_resource_metadata),
            )
            .with_state(auth_state.clone());

        Some(lab_auth::routes::bearer_only_router(auth_state).merge(path_based_discovery))
    } else {
        None
    };

    // Build the combined router.
    //
    // authenticated: Router<()> — mcp_service embeds AppState in its service
    //   closure via nest_service; does NOT use the axum State extractor.
    //   After .layer(AuthLayer) it is still Router<()>.
    //
    // oauth_router: Router<()> — bearer_only_router bakes AuthState in via
    //   .with_state(auth_state). No axum State extractor used.
    //
    // /health: needs State<AppState>. It is added via .route() which constrains
    //   the router to Router<AppState>. The outer router is therefore Router<AppState>
    //   and .with_state(state) satisfies it at the end.
    //
    // OAuth router is a Router<()> (state already satisfied). To merge it into a
    // Router<AppState> we use .with_state(state.clone()) on the combined base first,
    // producing Router<()>, merge the oauth Router<()>, then re-add the health route
    // (which requires AppState) and call .with_state(state) at the end.
    let health_state = state.clone();
    let health_route = Router::new()
        .route("/health", get(health_minimal))
        .route("/health/full", get(health_full));

    let base_with_state: Router<()> = Router::new()
        .merge(authenticated)
        .merge(health_route)
        .with_state(health_state);

    let combined = match oauth_router {
        Some(oauth) => base_with_state.merge(oauth),
        None => base_with_state,
    };

    combined
        .fallback(|| async { (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))) })
        .layer(RequestBodyLimitLayer::new(MCP_BODY_LIMIT_BYTES as usize))
        .layer(cors_layer(&state.config))
}

fn cors_layer(config: &crate::config::McpConfig) -> CorsLayer {
    let origins: Vec<HeaderValue> = allowed_origins(config)
        .into_iter()
        .filter_map(|origin| match origin.parse::<HeaderValue>() {
            Ok(value) => Some(value),
            Err(error) => {
                tracing::warn!(origin = %origin, error = %error, "Ignoring invalid CORS origin");
                None
            }
        })
        .collect();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::POST, Method::GET])
        .allow_headers([
            header::ACCEPT,
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static(MCP_PROTOCOL_VERSION_HEADER),
            HeaderName::from_static(MCP_SESSION_ID_HEADER),
        ])
}

/// Minimal liveness probe — unauthenticated, safe for Docker HEALTHCHECK and
/// Compose health gates. Returns 200 when DB is reachable, 503 otherwise.
/// Does not expose counters or ingest metrics.
async fn health_minimal(State(state): State<AppState>) -> impl IntoResponse {
    match state.service.health_check().await {
        Ok(()) => Json(json!({"status": "ok"})).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "Health check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"status": "error"})),
            )
                .into_response()
        }
    }
}

/// Full health payload including OTLP counters and ingest observability.
/// Auth-gated when auth is configured: requires the static bearer token.
/// Both /health routes live outside the MCP auth layer, so this handler
/// enforces auth explicitly rather than relying on middleware.
async fn health_full(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let AuthPolicy::Mounted { .. } = &state.auth_policy {
        if let Some(expected) = state.config.api_token.as_deref() {
            let provided = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "));
            if provided != Some(expected) {
                return StatusCode::UNAUTHORIZED.into_response();
            }
        }
    }
    let started = Instant::now();
    let logs_received = state.otlp_counters.logs_received.load(Ordering::Relaxed);
    let decode_errors = state.otlp_counters.decode_errors.load(Ordering::Relaxed);
    let observability = state.observability.snapshot();
    match state.service.health_check().await {
        Ok(()) => {
            tracing::debug!(
                elapsed_ms = started.elapsed().as_millis(),
                "Health check passed"
            );
            Json(json!({
                "status": "ok",
                "otlp_logs_received": logs_received,
                "otlp_decode_errors": decode_errors,
                "ingest": observability,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                elapsed_ms = started.elapsed().as_millis(),
                "Health check failed"
            );
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "error",
                    "otlp_logs_received": logs_received,
                    "otlp_decode_errors": decode_errors,
                    "ingest": observability,
                })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
#[path = "routes_tests.rs"]
mod tests;
