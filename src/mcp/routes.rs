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
use lab_auth::AuthLayer;
use serde_json::json;
use tower_http::{
    cors::{Any, CorsLayer},
    limit::RequestBodyLimitLayer,
};

use super::rmcp_server::allowed_origins;
use super::{streamable_http_config, streamable_http_service};
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
    let authenticated = match &state.auth_policy {
        AuthPolicy::LoopbackDev => mcp_service,
        AuthPolicy::Mounted { auth_state } => {
            let resource_url = state
                .config
                .auth
                .public_url
                .as_deref()
                .map(|u| Arc::<str>::from(format!("{}/mcp", u.trim_end_matches('/'))));
            let layer = AuthLayer::new()
                .with_static_token(state.config.api_token.as_deref().map(Arc::<str>::from))
                .with_auth_state(auth_state.clone())
                .with_resource_url(resource_url)
                .with_allow_session_cookie(false);
            mcp_service.layer(layer)
        }
    };

    let unauthenticated = Router::new().route("/health", get(health));

    Router::new()
        .merge(authenticated)
        .merge(unauthenticated)
        .fallback(|| async { (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))) })
        .layer(RequestBodyLimitLayer::new(MCP_BODY_LIMIT_BYTES as usize))
        .layer(cors_layer(&state.config))
        .with_state(state)
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
