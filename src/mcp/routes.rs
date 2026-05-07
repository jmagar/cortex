use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::State,
    http::{HeaderValue, Method, StatusCode},
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use serde_json::json;
use tower_http::{
    cors::{Any, CorsLayer},
    limit::RequestBodyLimitLayer,
};

use super::rmcp_server::allowed_origins;
use super::{build_auth_layer, streamable_http_config, streamable_http_service};
use super::{AppState, AuthPolicy};

const MCP_BODY_LIMIT_BYTES: u64 = 65_536;

/// Build the MCP router
pub fn router(state: AppState) -> Router {
    // Authenticated RMCP Streamable HTTP endpoint. /health is mounted separately
    // so Docker HEALTHCHECK, docker-compose health probes, and SWAG can reach it.
    let rmcp_config = streamable_http_config(&state.config);
    let mcp_service =
        Router::new().nest_service("/mcp", streamable_http_service(state.clone(), rmcp_config));

    // Apply auth layer based on policy. LoopbackDev skips auth entirely —
    // the loopback bind is the trust boundary. For Mounted variants, apply
    // AuthLayer (bearer-only: allow_session_cookie=false).
    //
    // AuthLayer MUST NOT add any DB write path. JWT validation is stateless RS256
    // verify; static token is constant-time compare. If audit logging is ever
    // added, push to async background channel only.
    let resource_url = state
        .config
        .auth
        .public_url
        .as_deref()
        .map(|u| Arc::<str>::from(format!("{}/mcp", u.trim_end_matches('/'))));
    let authenticated = if let Some(layer) = build_auth_layer(
        &state.auth_policy,
        state.config.api_token.as_deref().map(Arc::<str>::from),
        resource_url,
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
    // Locked Decision: /register and /auth/login are NOT in bearer_only_router
    // (confirmed by lab-auth's BEARER_ONLY_ROUTER_FORBIDDEN_PATHS snapshot test).
    let oauth_router: Option<Router> = if let AuthPolicy::Mounted {
        auth_state: Some(ref state_arc),
    } = state.auth_policy
    {
        tracing::info!(
            "OAuth router mounted: /.well-known/oauth-authorization-server, \
                 /.well-known/oauth-protected-resource, /jwks, /authorize, \
                 /auth/google/callback, /token"
        );
        Some(lab_auth::routes::bearer_only_router(
            state_arc.as_ref().clone(),
        ))
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
    let health_route = Router::new().route("/health", get(health));

    let base_with_state: Router<()> = Router::new()
        .merge(authenticated)
        .merge(health_route)
        .with_state(health_state);

    let mut combined: Router<()> = base_with_state;

    if let Some(oauth) = oauth_router {
        // Both are Router<()> — merge is straightforward.
        combined = combined.merge(oauth);
    }

    // Re-wrap as the final Router (Router<()> is the return type since all state
    // is already embedded). The outer caller uses Router<> (= Router<()>).
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
        .allow_headers(Any)
}

/// Health check — lightweight probe that verifies DB connectivity without
/// running COUNT(*) over the entire logs table. Also surfaces OTLP receiver
/// counters so operators can see ingest activity at a glance.
async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let started = Instant::now();
    let logs_received = state.otlp_counters.logs_received.load(Ordering::Relaxed);
    let decode_errors = state.otlp_counters.decode_errors.load(Ordering::Relaxed);
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
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "otlp_logs_received": logs_received,
                    "otlp_decode_errors": decode_errors,
                })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
#[path = "routes_tests.rs"]
mod tests;
