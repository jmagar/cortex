use super::*;
use crate::app::SyslogService;
use crate::config::{McpConfig, StorageConfig};
use crate::db;
use crate::mcp::{AppState, AuthPolicy};
use axum::body::to_bytes;
use axum::http::{header, Method, Request, StatusCode};
use std::sync::Arc;
use tower::util::ServiceExt;

/// Build an AppState with LoopbackDev policy (no auth applied).
fn test_state_no_auth() -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("mcp-test.db"));
    let pool = Arc::new(db::init_pool(&storage).unwrap());
    (
        AppState {
            service: SyslogService::new(pool, storage.clone()),
            config: McpConfig {
                host: "127.0.0.1".into(),
                port: 3100,
                server_name: "syslog-mcp".into(),
                api_token: None,
                allowed_hosts: Vec::new(),
                allowed_origins: Vec::new(),
                auth: Default::default(),
            },
            otlp_counters: Arc::new(crate::otlp::OtlpCounters::default()),
            auth_policy: AuthPolicy::LoopbackDev,
            observability: Arc::new(crate::observability::RuntimeObservability::default()),
        },
        dir,
    )
}

/// Build an AppState with Mounted { auth_state: None } policy (static-bearer auth via AuthLayer).
fn test_state_with_token(token: String) -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("mcp-test.db"));
    let pool = Arc::new(db::init_pool(&storage).unwrap());
    (
        AppState {
            service: SyslogService::new(pool, storage.clone()),
            config: McpConfig {
                host: "127.0.0.1".into(),
                port: 3100,
                server_name: "syslog-mcp".into(),
                api_token: Some(token),
                allowed_hosts: Vec::new(),
                allowed_origins: Vec::new(),
                auth: Default::default(),
            },
            otlp_counters: Arc::new(crate::otlp::OtlpCounters::default()),
            // Mounted { auth_state: None } = static-bearer only; AuthLayer IS applied.
            auth_policy: AuthPolicy::Mounted { auth_state: None },
            observability: Arc::new(crate::observability::RuntimeObservability::default()),
        },
        dir,
    )
}

struct TestHarness {
    state: AppState,
    _dir: tempfile::TempDir,
}

impl TestHarness {
    fn new() -> Self {
        let (state, dir) = test_state_no_auth();
        TestHarness { state, _dir: dir }
    }

    fn with_token(token: String) -> Self {
        let (state, dir) = test_state_with_token(token);
        TestHarness { state, _dir: dir }
    }
}

fn jsonrpc_request(id: u64, method: &str, params: Option<serde_json::Value>) -> serde_json::Value {
    let mut req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
    });
    if let Some(p) = params {
        req.as_object_mut().unwrap().insert("params".into(), p);
    }
    req
}

async fn mcp_post(
    app: Router,
    body: serde_json::Value,
    auth: Option<&str>,
) -> (axum::http::StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "localhost:3100")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream");
    if let Some(token) = auth {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    let request = builder
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, value)
}

#[tokio::test]
async fn integration_health_returns_200() {
    let h = TestHarness::new();
    let app = router(h.state);
    let request = Request::builder()
        .method("GET")
        .uri("/health")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["status"], "ok");
    assert!(value["ingest"]["ingest_queue_depth"].is_number());
}

#[tokio::test]
async fn integration_initialize() {
    let h = TestHarness::new();
    let body = jsonrpc_request(
        1,
        "initialize",
        Some(serde_json::json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "route-test", "version": "1.0"}
        })),
    );
    let (status, value) = mcp_post(router(h.state), body, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(value["result"]["protocolVersion"].is_string());
    assert!(value["result"]["serverInfo"]["name"].is_string());
}

#[tokio::test]
async fn integration_get_stats() {
    let h = TestHarness::new();
    let body = jsonrpc_request(
        3,
        "tools/call",
        Some(serde_json::json!({"name": "syslog", "arguments": {"action": "stats"}})),
    );
    let (status, value) = mcp_post(router(h.state), body, None).await;
    assert_eq!(status, StatusCode::OK);
    let content = value["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        content.contains("total_logs"),
        "expected total_logs in: {content}"
    );
}

#[tokio::test]
async fn integration_tail_logs_empty_db() {
    let h = TestHarness::new();
    let body = jsonrpc_request(
        4,
        "tools/call",
        Some(serde_json::json!({"name": "syslog", "arguments": {"action": "tail", "n": 10}})),
    );
    let (status, value) = mcp_post(router(h.state), body, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(value["error"].is_null(), "unexpected error: {value}");
}

#[tokio::test]
async fn integration_search_logs_empty_db() {
    let h = TestHarness::new();
    let body = jsonrpc_request(
        5,
        "tools/call",
        Some(
            serde_json::json!({"name": "syslog", "arguments": {"action": "search", "query": "error", "limit": 5}}),
        ),
    );
    let (status, value) = mcp_post(router(h.state), body, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(value["error"].is_null(), "unexpected error: {value}");
}

#[tokio::test]
async fn integration_auth_missing_token_returns_401() {
    let h = TestHarness::with_token("secret-token".into());
    let body = jsonrpc_request(7, "tools/list", None);
    let (status, _) = mcp_post(router(h.state), body, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn integration_auth_correct_token_succeeds() {
    let h = TestHarness::with_token("secret-token".into());
    let body = jsonrpc_request(8, "tools/list", None);
    let (status, _) = mcp_post(router(h.state), body, Some("secret-token")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn mcp_accepts_case_insensitive_bearer_scheme() {
    let h = TestHarness::with_token("secret-token".into());
    let app = router(h.state);
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "localhost:3100")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Authorization", "bearer secret-token")
        .body(axum::body::Body::from(
            serde_json::to_vec(&jsonrpc_request(10, "tools/list", None)).unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn mcp_cors_uses_configured_port() {
    let (mut state, _dir) = test_state_no_auth();
    state.config.port = 3201;
    let app = router(state);
    let request = Request::builder()
        .method("GET")
        .uri("/health")
        .header("Origin", "http://localhost:3201")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .unwrap(),
        "http://localhost:3201"
    );
}

#[tokio::test]
async fn mcp_cors_allows_configured_origins() {
    let (mut state, _dir) = test_state_no_auth();
    state.config.allowed_origins = vec!["https://syslog.example.com".into()];
    let app = router(state);
    let request = Request::builder()
        .method("GET")
        .uri("/health")
        .header("Origin", "https://syslog.example.com")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .unwrap(),
        "https://syslog.example.com"
    );
}

#[tokio::test]
async fn mcp_rejects_wrong_token() {
    let h = TestHarness::with_token("secret-token".into());
    let body = jsonrpc_request(9, "tools/list", None);
    let (status, _) = mcp_post(router(h.state), body, Some("wrong-token")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn legacy_sse_endpoint_is_removed() {
    let h = TestHarness::with_token("secret-token".into());
    let app = router(h.state);
    let request = Request::builder()
        .method("GET")
        .uri("/sse")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let request = Request::builder()
        .method("GET")
        .uri("/sse")
        .header("Authorization", "Bearer secret-token")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn oversized_mcp_request_is_rejected_by_body_limit() {
    let h = TestHarness::new();
    let app = router(h.state);
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "localhost:3100")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::CONTENT_LENGTH, "70000")
        .body(axum::body::Body::from("x".repeat(70_000)))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn mcp_rejects_missing_accept_header() {
    let h = TestHarness::new();
    let app = router(h.state);
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "localhost:3100")
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&jsonrpc_request(11, "tools/list", None)).unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
}

#[tokio::test]
async fn mcp_rejects_missing_content_type_header() {
    let h = TestHarness::new();
    let app = router(h.state);
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "localhost:3100")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .body(axum::body::Body::from(
            serde_json::to_vec(&jsonrpc_request(12, "tools/list", None)).unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn mcp_rejects_unsupported_protocol_version() {
    let h = TestHarness::new();
    let app = router(h.state);
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "localhost:3100")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "1900-01-01")
        .body(axum::body::Body::from(
            serde_json::to_vec(&jsonrpc_request(13, "tools/list", None)).unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn stateless_mcp_rejects_get_and_delete() {
    let h = TestHarness::new();
    let app = router(h.state);
    for method in [Method::GET, Method::DELETE] {
        let request = Request::builder()
            .method(method.clone())
            .uri("/mcp")
            .header(header::HOST, "localhost:3100")
            .header(header::ACCEPT, "text/event-stream")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}

// ── AuthPolicy coverage ──────────────────────────────────────────────────────

/// LoopbackDev: AuthLayer is NOT applied; requests reach /mcp without any token.
#[tokio::test]
async fn loopback_dev_policy_skips_auth_layer() {
    let h = TestHarness::new(); // LoopbackDev, no token
    let body = jsonrpc_request(20, "tools/list", None);
    let (status, _) = mcp_post(router(h.state), body, None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "LoopbackDev must not require bearer token"
    );
}

/// Mounted { auth_state: None }: valid static token → 200.
#[tokio::test]
async fn mounted_static_bearer_valid_token_succeeds() {
    let h = TestHarness::with_token("static-secret".into());
    let body = jsonrpc_request(21, "tools/list", None);
    let (status, _) = mcp_post(router(h.state), body, Some("static-secret")).await;
    assert_eq!(status, StatusCode::OK);
}

/// Mounted { auth_state: None }: wrong static token → 401 (no fall-through to permit).
#[tokio::test]
async fn mounted_static_bearer_wrong_token_returns_401_no_fallthrough() {
    let h = TestHarness::with_token("static-secret".into());
    let body = jsonrpc_request(22, "tools/list", None);
    let (status, _) = mcp_post(router(h.state), body, Some("wrong-token")).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "AuthLayer must not fall through on bad token"
    );
}

/// Mounted { auth_state: None }: no credentials at all → 401 (fail-closed; no permit fallthrough).
#[tokio::test]
async fn mounted_missing_credentials_returns_401_fail_closed() {
    let h = TestHarness::with_token("static-secret".into());
    let body = jsonrpc_request(23, "tools/list", None);
    let (status, _) = mcp_post(router(h.state), body, None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "missing credentials must be rejected, not permitted"
    );
}

/// Mounted: cookie header without Authorization is ignored — bearer-only mode.
#[tokio::test]
async fn mounted_cookie_without_bearer_is_rejected() {
    let h = TestHarness::with_token("static-secret".into());
    let app = router(h.state);
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "localhost:3100")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::COOKIE, "session=some-session-id")
        .body(axum::body::Body::from(
            serde_json::to_vec(&jsonrpc_request(24, "tools/list", None)).unwrap(),
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "session cookie must not bypass bearer-only AuthLayer"
    );
}

/// Bearer-only mode: valid static token + scope-gated action (stats) → 200.
///
/// Regression test for the bug where `build_auth_layer` built an `AuthLayer`
/// with `static_token_scopes: Vec::new()` in bearer-only mode because
/// `AuthLayer::with_auth_state(None)` does not populate scopes.
/// After the fix, `build_auth_layer` explicitly calls `.with_static_token_scopes`
/// so the `AuthContext` injected by the layer carries `["syslog:read", "syslog:admin"]`
/// and scope-gated actions succeed.
#[tokio::test]
async fn mounted_static_bearer_valid_token_can_call_scope_gated_action() {
    let h = TestHarness::with_token("static-secret".into());
    // `stats` requires syslog:read — it is scope-gated at the rmcp layer.
    let body = jsonrpc_request(
        25,
        "tools/call",
        Some(serde_json::json!({"name": "syslog", "arguments": {"action": "stats"}})),
    );
    let (status, response) = mcp_post(router(h.state), body, Some("static-secret")).await;
    assert_eq!(status, StatusCode::OK, "response: {response}");
    // Must be a successful result, not a JSON-RPC scope-denial error.
    assert!(
        response.get("error").is_none() || response["error"].is_null(),
        "static bearer token must pass scope check for stats; response: {response}"
    );
    assert!(
        response["result"].is_object(),
        "stats result expected; response: {response}"
    );
}

/// /health stays unauthenticated even when Mounted policy is active.
#[tokio::test]
async fn health_unauthenticated_under_mounted_policy() {
    let h = TestHarness::with_token("static-secret".into());
    let app = router(h.state);
    let request = Request::builder()
        .method("GET")
        .uri("/health")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ── OAuth router mount tests ─────────────────────────────────────────────────

/// Helper: build an AppState with `AuthPolicy::Mounted { auth_state: Some(...) }`.
///
/// Uses tempfiles for the SQLite store and JWT key so no real filesystem paths
/// are required. Calls `lab_auth::state::AuthState::new` with a minimal OAuth
/// config (mode=oauth, public_url, fake Google credentials).
async fn test_state_with_oauth() -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    // Auth files live in the same tempdir as the syslog DB.
    let auth_db = dir.path().join("auth.db");
    let auth_key = dir.path().join("auth.pem");

    // Key names match lab-auth's AuthConfigBuilder env_key() function:
    // env_key(prefix, suffix) → "{PREFIX}_{SUFFIX}" (uppercased).
    // e.g. env_key("SYSLOG_MCP", "AUTH_MODE") → "SYSLOG_MCP_AUTH_MODE"
    //      env_key("SYSLOG_MCP", "GOOGLE_CLIENT_ID") → "SYSLOG_MCP_GOOGLE_CLIENT_ID"
    let vars: Vec<(String, String)> = vec![
        ("SYSLOG_MCP_AUTH_MODE".into(), "oauth".into()),
        (
            "SYSLOG_MCP_PUBLIC_URL".into(),
            "https://syslog.example.com".into(),
        ),
        // Google credential keys do NOT have "AUTH_" prefix in lab-auth's schema.
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
            auth_db.to_str().unwrap().into(),
        ),
        (
            "SYSLOG_MCP_AUTH_KEY_PATH".into(),
            auth_key.to_str().unwrap().into(),
        ),
    ];

    let auth_config = lab_auth::config::AuthConfigBuilder::new()
        .env_prefix("SYSLOG_MCP")
        .session_cookie_name("syslog_mcp_session")
        .scopes_supported(vec!["syslog:read".into(), "syslog:admin".into()])
        .default_scope("syslog:read")
        .resource_path("/mcp")
        .build_from_sources(vars)
        .expect("test auth config should build");

    let auth_state = lab_auth::state::AuthState::new(auth_config)
        .await
        .expect("test auth state should init");

    let storage = StorageConfig::for_test(dir.path().join("mcp-test.db"));
    let pool = Arc::new(db::init_pool(&storage).unwrap());

    let state = AppState {
        service: SyslogService::new(pool, storage.clone()),
        config: McpConfig {
            host: "127.0.0.1".into(),
            port: 3100,
            server_name: "syslog-mcp".into(),
            api_token: None,
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
            auth: crate::config::AuthConfig {
                public_url: Some("https://syslog.example.com".into()),
                ..Default::default()
            },
        },
        otlp_counters: Arc::new(crate::otlp::OtlpCounters::default()),
        auth_policy: AuthPolicy::Mounted {
            auth_state: Some(Arc::new(auth_state)),
        },
        observability: Arc::new(crate::observability::RuntimeObservability::default()),
    };

    (state, dir)
}

/// OAuth router IS mounted when auth_state: Some(_).
/// GET /.well-known/oauth-authorization-server returns 200.
#[tokio::test]
async fn oauth_router_mounted_when_auth_state_is_some() {
    let (state, _dir) = test_state_with_oauth().await;
    let app = router(state);
    let request = Request::builder()
        .method("GET")
        .uri("/.well-known/oauth-authorization-server")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "OAuth well-known endpoint must be reachable when auth_state is Some"
    );
}

/// OAuth router NOT mounted when auth_state: None (bearer-only).
/// GET /.well-known/oauth-authorization-server returns 404.
#[tokio::test]
async fn oauth_router_not_mounted_when_bearer_only() {
    // Mounted { auth_state: None } = bearer-only; no OAuth router.
    let (state, _dir) = test_state_with_token("some-token".into());
    let app = router(state);
    let request = Request::builder()
        .method("GET")
        .uri("/.well-known/oauth-authorization-server")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "OAuth well-known endpoint must NOT be mounted in bearer-only mode"
    );
}

/// OAuth router NOT mounted when LoopbackDev.
#[tokio::test]
async fn oauth_router_not_mounted_when_loopback_dev() {
    let (state, _dir) = test_state_no_auth();
    let app = router(state);
    let request = Request::builder()
        .method("GET")
        .uri("/.well-known/oauth-authorization-server")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "OAuth well-known endpoint must NOT be mounted in LoopbackDev mode"
    );
}

/// POST /register is 404 in ALL modes — not in bearer_only_router.
/// Locked Decision: /register is excluded from the headless router subset.
#[tokio::test]
async fn register_returns_404_in_all_modes() {
    // Test all three modes.
    let (loopback_state, _dir1) = test_state_no_auth();
    let (bearer_state, _dir2) = test_state_with_token("tok".into());
    let (oauth_state, _dir3) = test_state_with_oauth().await;

    for (label, state) in [
        ("LoopbackDev", loopback_state),
        ("bearer-only", bearer_state),
        ("OAuth", oauth_state),
    ] {
        let app = router(state);
        let request = Request::builder()
            .method("POST")
            .uri("/register")
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(r#"{"redirect_uris":[]}"#))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "POST /register must not be mounted in {label} mode (Locked Decision)"
        );
    }
}

/// GET /auth/login — not reachable in LoopbackDev or bearer-only (no OAuth router
/// mounted), but IS mounted when OAuth is active because we use the full lab-auth
/// router() (not bearer_only_router) so that DCR /register is available for MCP
/// clients. /auth/login is a browser redirect to Google — harmless in a headless
/// context but present so callers get a redirect rather than a 404.
#[tokio::test]
async fn auth_login_not_mounted_without_oauth() {
    let (loopback_state, _dir1) = test_state_no_auth();
    let (bearer_state, _dir2) = test_state_with_token("tok".into());

    for (label, state) in [
        ("LoopbackDev", loopback_state),
        ("bearer-only", bearer_state),
    ] {
        let app = router(state);
        let request = Request::builder()
            .method("GET")
            .uri("/auth/login")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "GET /auth/login must not be mounted in {label} mode"
        );
    }
}

/// null-Origin is rejected (403 Forbidden) by rmcp's internal origin validator
/// on the /mcp endpoint.
///
/// rmcp's StreamableHttpService validates the Origin header before routing and
/// parses "null" as NormalizedOrigin::Null. Since "null" is never in our
/// allowed_origins list, the request is rejected with 403 Forbidden. This
/// makes the implicit rejection of spoofed sandboxed-iframe origins explicit
/// and verifiable.
///
/// Note: tower-http CorsLayer (applied to /health and other non-rmcp routes)
/// does NOT actively reject null-Origin — it simply omits the ACAO header and
/// relies on the browser to block the cross-origin read. The active 403 comes
/// from rmcp's DNS-rebinding guard on /mcp.
#[tokio::test]
async fn null_origin_rejected_by_rmcp_validator_on_mcp_endpoint() {
    let h = TestHarness::new(); // LoopbackDev — no auth so we reach the origin check
    let app = router(h.state);
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "localhost:3100")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Origin", "null")
        .body(axum::body::Body::from(
            serde_json::to_vec(&jsonrpc_request(99, "tools/list", None)).unwrap(),
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "Origin: null must be rejected by rmcp's origin validator (NormalizedOrigin::Null \
         is never in the allowed_origins list)"
    );
}

// NOTE: Tests for allowed_hosts() / allowed_origins() public_url extension
// live in rmcp_server_tests.rs (same module as the functions, via `use super::*`).
