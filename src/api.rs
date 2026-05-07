use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use lab_auth::AuthLayer;
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};

use crate::app::{
    CorrelateEventsRequest, GetErrorsRequest, SearchLogsRequest, SyslogService, TailLogsRequest,
};
use crate::config::ApiConfig;
use crate::mcp::AuthPolicy;

#[derive(Clone)]
pub struct ApiState {
    pub service: SyslogService,
    pub config: ApiConfig,
    pub cors_port: u16,
    /// Authentication policy — mirrors mcp::AppState so /api/* can apply the
    /// same AuthLayer as /mcp.
    pub auth_policy: AuthPolicy,
}

pub fn router(state: ApiState) -> anyhow::Result<Router> {
    if !state.config.enabled {
        anyhow::bail!("non-MCP API is disabled");
    }
    if state.config.enabled && state.config.api_token.is_none() {
        anyhow::bail!("non-MCP API requires SYSLOG_API_TOKEN when enabled");
    }

    let routes = Router::new()
        .route("/api/search", get(search))
        .route("/api/tail", get(tail))
        .route("/api/errors", get(errors))
        .route("/api/hosts", get(hosts))
        .route("/api/correlate", get(correlate))
        .route("/api/stats", get(stats));

    // Apply auth layer based on policy. LoopbackDev skips auth (loopback bind
    // is the trust boundary). Mounted applies AuthLayer (bearer-only:
    // allow_session_cookie=false — no browser UI on /api/*).
    //
    // AuthLayer MUST NOT add any DB write path. JWT validation is stateless RS256
    // verify; static token is constant-time compare. If audit logging is ever
    // added, push to async background channel only.
    let routes = match &state.auth_policy {
        AuthPolicy::LoopbackDev => routes,
        AuthPolicy::Mounted { auth_state } => {
            let layer = AuthLayer::new()
                .with_static_token(state.config.api_token.as_deref().map(Arc::<str>::from))
                .with_auth_state(auth_state.clone())
                .with_resource_url(None)
                .with_allow_session_cookie(false);
            routes.layer(layer)
        }
    };

    let routes = routes.layer(cors_layer(state.cors_port)).with_state(state);
    Ok(routes)
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    query: Option<String>,
    hostname: Option<String>,
    source_ip: Option<String>,
    severity: Option<String>,
    app_name: Option<String>,
    from: Option<String>,
    to: Option<String>,
    limit: Option<u32>,
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
                from: query.from,
                to: query.to,
                limit: query.limit,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
struct TailQuery {
    hostname: Option<String>,
    source_ip: Option<String>,
    app_name: Option<String>,
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
                n: query.n,
            })
            .await,
    )
}

#[derive(Debug, Deserialize)]
struct ErrorQuery {
    from: Option<String>,
    to: Option<String>,
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
            })
            .await,
    )
}

async fn hosts(State(state): State<ApiState>) -> impl IntoResponse {
    respond(state.service.list_hosts().await)
}

#[derive(Debug, Deserialize)]
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

fn respond<T: serde::Serialize>(result: crate::app::ServiceResult<T>) -> axum::response::Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(crate::app::ServiceError::InvalidInput(msg)) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": msg}))).into_response()
        }
        Err(crate::app::ServiceError::Busy(msg)) => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": msg}))).into_response()
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

fn cors_layer(port: u16) -> CorsLayer {
    CorsLayer::new()
        .allow_origin([
            format!("http://localhost:{port}")
                .parse::<axum::http::HeaderValue>()
                .expect("valid localhost origin"),
            format!("http://127.0.0.1:{port}")
                .parse::<axum::http::HeaderValue>()
                .expect("valid 127.0.0.1 origin"),
        ])
        .allow_methods([axum::http::Method::GET])
        .allow_headers(Any)
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
