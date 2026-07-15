//! Handler-level tests for the OTLP HTTP receiver (status-code contract for
//! the deferred `/v1/metrics` and `/v1/traces` routes, and counters). Pure
//! `AnyValue`/`build_entries` logic lives in `otlp::entries`'s sidecar tests;
//! the bearer-token gate lives in `otlp::auth`'s sidecar tests.

use super::*;

use std::sync::Arc;
use std::sync::atomic::Ordering;

fn state_with_token(token: Option<&str>) -> OtlpState {
    let (tx, _rx) = tokio::sync::mpsc::channel::<crate::db::LogBatchEntry>(10);
    let ingest = crate::ingest::IngestTx::from_sender_for_test(tx);
    // Use Mounted when a token is configured so is_authorized enforces it.
    let auth_policy = if token.is_some() {
        crate::mcp::AuthPolicy::Mounted { auth_state: None }
    } else {
        crate::mcp::AuthPolicy::LoopbackDev
    };
    OtlpState::new(
        ingest,
        token.map(String::from),
        Arc::new(OtlpCounters::default()),
        auth_policy,
    )
}

#[tokio::test]
async fn metrics_handler_returns_not_supported() {
    let response = metrics_handler(State(state_with_token(None)), HeaderMap::new()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn traces_handler_requires_bearer_when_token_configured() {
    let response = traces_handler(State(state_with_token(Some("secret"))), HeaderMap::new()).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn traces_handler_returns_not_supported_after_auth() {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        HeaderValue::from_static("Bearer secret"),
    );
    let response = traces_handler(State(state_with_token(Some("secret"))), headers).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn counters_default_to_zero() {
    let counters = OtlpCounters::default();
    assert_eq!(counters.logs_received.load(Ordering::Relaxed), 0);
    assert_eq!(counters.decode_errors.load(Ordering::Relaxed), 0);
}
