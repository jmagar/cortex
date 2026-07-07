use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::*;
use crate::config::StorageConfig;
use crate::mcp::AuthPolicy;

fn test_app(token: Option<&str>) -> (Router, tempfile::TempDir) {
    test_app_with(
        token,
        AuthPolicy::Mounted { auth_state: None },
        SocketAddr::from(([10, 0, 0, 7], 41000)),
    )
}

fn test_app_with(
    token: Option<&str>,
    auth_policy: AuthPolicy,
    peer: SocketAddr,
) -> (Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("agent-command-ingest-test.db"));
    let pool = Arc::new(crate::db::init_pool(&storage).unwrap());
    let state = AgentCommandIngestState::new(pool, token.map(str::to_string), auth_policy);
    let app = router(state).layer(MockConnectInfo(peer));
    (app, dir)
}

#[tokio::test]
async fn rejects_missing_bearer_token() {
    let (app, _dir) = test_app(Some("secret"));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/agent-commands")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("[]"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn accepts_batch_with_valid_bearer_token() {
    let (app, _dir) = test_app(Some("secret"));
    let body = serde_json::to_string(&[serde_json::json!({
        "started_at": "2026-07-06T00:00:00Z",
        "finished_at": "2026-07-06T00:00:01Z",
        "duration_ms": 1000,
        "exit_status": 0,
        "command": "echo hi",
        "cwd": null,
        "agent": "claude-code",
        "command_surface": null,
        "hostname": "testhost",
        "user": null,
        "pid": 1234,
        "session_id": null,
        "schema_version": 1,
        "content_scrubbed": true
    })])
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/agent-commands")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer secret")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: crate::command_log::CommandLogImportResult =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result.imported, 1);
}

#[tokio::test]
async fn rejects_malformed_json_body() {
    let (app, _dir) = test_app(Some("secret"));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/agent-commands")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer secret")
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn loopback_dev_policy_accepts_loopback_peer_without_bearer_token() {
    // Test-review addition: no existing test exercised `AuthPolicy::LoopbackDev`
    // at all — every test used `Mounted`. This proves the loopback bypass
    // actually gates on the real `ConnectInfo` peer address, not merely on
    // "no policy configured."
    let (app, _dir) = test_app_with(
        None,
        AuthPolicy::LoopbackDev,
        SocketAddr::from(([127, 0, 0, 1], 41000)),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/agent-commands")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("[]"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn loopback_dev_policy_rejects_non_loopback_peer() {
    let (app, _dir) = test_app_with(
        None,
        AuthPolicy::LoopbackDev,
        SocketAddr::from(([10, 0, 0, 7], 41000)),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/agent-commands")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("[]"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mounted_policy_with_no_configured_token_rejects_every_request() {
    // Test-review addition: a `Mounted` policy with `api_token: None` (e.g.
    // before an operator sets CORTEX_TOKEN) must fail closed, not silently
    // accept requests just because there's no header to check against.
    let (app, _dir) = test_app(None);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/agent-commands")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer anything")
                .body(Body::from("[]"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_batch_over_max_records() {
    // engineering-review addition: a batch exceeding MAX_RECORDS_PER_BATCH
    // must be rejected outright, not accepted and processed — the 1 MiB body
    // cap alone bounds bytes, not record count.
    let (app, _dir) = test_app(Some("secret"));
    let one_record = serde_json::json!({
        "started_at": "2026-07-06T00:00:00Z",
        "finished_at": "2026-07-06T00:00:01Z",
        "duration_ms": 1,
        "exit_status": 0,
        "command": "x",
        "cwd": null,
        "agent": "claude-code",
        "command_surface": null,
        "hostname": "testhost",
        "user": null,
        "pid": 1,
        "session_id": null,
        "schema_version": 1,
        "content_scrubbed": true
    });
    let too_many: Vec<serde_json::Value> =
        std::iter::repeat_n(one_record, MAX_RECORDS_PER_BATCH + 1).collect();
    let body = serde_json::to_string(&too_many).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/agent-commands")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer secret")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
