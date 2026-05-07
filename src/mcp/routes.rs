use std::sync::atomic::Ordering;
use std::time::Instant;

use axum::{
    extract::State,
    http::{HeaderValue, Method, StatusCode},
    middleware,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use serde_json::json;
use tower_http::{
    cors::{Any, CorsLayer},
    limit::RequestBodyLimitLayer,
};

use crate::auth::{bearer_token, token_matches};

use super::rmcp_server::allowed_origins;
use super::AppState;
use super::{streamable_http_config, streamable_http_service};

const MCP_BODY_LIMIT_BYTES: u64 = 65_536;

/// Build the MCP router
pub fn router(state: AppState) -> Router {
    // Authenticated RMCP Streamable HTTP endpoint. /health is mounted separately
    // so Docker HEALTHCHECK, docker-compose health probes, and SWAG can reach it.
    let rmcp_config = streamable_http_config(&state.config);
    let authenticated = Router::new()
        .nest_service("/mcp", streamable_http_service(state.clone(), rmcp_config))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

    let unauthenticated = Router::new().route("/health", get(health));

    Router::new()
        .merge(authenticated)
        .merge(unauthenticated)
        .fallback(|| async { (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))) })
        .layer(RequestBodyLimitLayer::new(MCP_BODY_LIMIT_BYTES as usize))
        .layer(cors_layer(&state.config))
        .with_state(state)
}

/// Bearer-token authentication middleware.
///
/// When `config.api_token` is `Some(token)`, every request must carry:
///   `Authorization: Bearer <token>`
/// Requests with a missing or incorrect token receive HTTP 401.
/// When `api_token` is `None` (the default), all requests pass through unchanged.
async fn require_auth(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    if let Some(ref expected) = state.config.api_token {
        let auth = req
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        let authorized = auth
            .and_then(bearer_token)
            .map(|token| token_matches(token, expected))
            .unwrap_or(false);
        if !authorized {
            tracing::warn!(
                method = %method,
                path = %path,
                has_auth_header = auth.is_some(),
                "Unauthorized MCP request rejected"
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {"code": -32001, "message": "unauthorized"}
                })),
            )
                .into_response();
        }
    }
    next.run(req).await
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
