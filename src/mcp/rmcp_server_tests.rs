use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    extract::Request as AxumRequest,
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    Router,
};
use lab_auth::AuthContext;
use serde_json::{json, Value};
use tower::util::ServiceExt;

use crate::{
    app::SyslogService,
    config::{McpConfig, StorageConfig},
    db::{self, DbPool, LogBatchEntry},
    mcp::{streamable_http_config, streamable_http_service, AppState, AuthPolicy},
};

use super::{actions, allowed_hosts, allowed_origins, is_validation_error, required_scope_for};

fn test_state() -> (AppState, Arc<DbPool>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("rmcp-server-test.db"));
    let pool = Arc::new(db::init_pool(&storage).unwrap());
    let state = AppState {
        service: SyslogService::new(Arc::clone(&pool), storage.clone()),
        config: McpConfig {
            host: "127.0.0.1".into(),
            port: 3100,
            server_name: "syslog-mcp".into(),
            no_auth: false,
            api_token: None,
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
            auth: Default::default(),
            static_token_is_admin: false,
        },
        notifications_config: crate::config::NotificationsConfig::default(),
        otlp_counters: Arc::new(crate::otlp::OtlpCounters::default()),
        auth_policy: crate::mcp::AuthPolicy::LoopbackDev,
        observability: Arc::new(crate::observability::RuntimeObservability::default()),
    };
    (state, pool, dir)
}

/// Build a Mounted-policy AppState (no OAuth; static-bearer only path).
fn mounted_state() -> (AppState, Arc<DbPool>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("rmcp-mounted-test.db"));
    let pool = Arc::new(db::init_pool(&storage).unwrap());
    let state = AppState {
        service: SyslogService::new(Arc::clone(&pool), storage.clone()),
        config: McpConfig {
            host: "127.0.0.1".into(),
            port: 3100,
            server_name: "syslog-mcp".into(),
            no_auth: false,
            api_token: None,
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
            auth: Default::default(),
            static_token_is_admin: false,
        },
        notifications_config: crate::config::NotificationsConfig::default(),
        otlp_counters: Arc::new(crate::otlp::OtlpCounters::default()),
        auth_policy: AuthPolicy::Mounted { auth_state: None },
        observability: Arc::new(crate::observability::RuntimeObservability::default()),
    };
    (state, pool, dir)
}

/// Build a test router with an axum middleware that injects `auth_ctx` into
/// request extensions before the request reaches the rmcp service.
fn rmcp_router_with_auth(state: AppState, auth_ctx: AuthContext) -> Router {
    let config = streamable_http_config(&state.config);
    let service = streamable_http_service(state, config);
    Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn(
            move |mut req: AxumRequest, next: Next| {
                let ctx = auth_ctx.clone();
                async move {
                    req.extensions_mut().insert(ctx);
                    next.run(req).await
                }
            },
        ))
}

/// Build a test router WITHOUT any auth middleware (simulates broken
/// middleware ordering — AuthContext never inserted).
fn rmcp_router_no_auth_middleware(state: AppState) -> Router {
    let config = streamable_http_config(&state.config);
    Router::new().nest_service("/mcp", streamable_http_service(state, config))
}

fn auth_ctx(subject: &str, scopes: Vec<&str>, email: Option<&str>) -> AuthContext {
    AuthContext {
        sub: subject.to_string(),
        actor_key: None,
        scopes: scopes.into_iter().map(String::from).collect(),
        issuer: "local".to_string(),
        via_session: false,
        csrf_token: None,
        email: email.map(String::from),
    }
}

fn auth_ctx_with_scopes(scopes: Vec<&str>) -> AuthContext {
    auth_ctx("test-user@example.com", scopes, None)
}

fn seed_error_signature(pool: &DbPool, hash: &str) {
    let conn = pool.get().unwrap();
    crate::db::error_signatures::upsert_signature(
        &conn,
        crate::db::error_signatures::UpsertSignatureParams {
            hash,
            normalizer_version: crate::app::error_detection::NORMALIZER_VERSION,
            template: "mounted auth coverage",
            sample_message: "mounted auth coverage",
            sample_hostname: "auth-test-host",
            sample_app_name: Some("schema-test"),
            severity: "err",
            first_seen_at: "2026-01-01T00:00:00.000Z",
            last_seen_at: "2026-01-01T00:00:00.000Z",
            delta: 1,
        },
    )
    .unwrap();
}

fn entry(ts: &str, host: &str, severity: &str, msg: &str, source_ip: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: host.to_string(),
        facility: None,
        severity: severity.to_string(),
        app_name: None,
        process_id: None,
        message: msg.to_string(),
        raw: msg.to_string(),
        source_ip: source_ip.to_string(),
        docker_checkpoint: None,
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        ai_transcript_path: None,
        metadata_json: None,
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    }
}

fn seed_auth_action_log(pool: &DbPool) {
    db::insert_logs_batch(
        pool,
        &[entry(
            "2026-01-01T00:00:00Z",
            "auth-test-host",
            "err",
            "mounted auth coverage",
            "127.0.0.1:514",
        )],
    )
    .unwrap();
}

fn minimal_args_for_action(action: &str) -> Value {
    match action {
        "correlate" => json!({"action": action, "reference_time": "2026-01-01T00:00:00Z"}),
        "search_sessions" => json!({"action": action, "query": "mounted"}),
        "ai_correlate" => json!({"action": action, "project": "/tmp/project"}),
        "project_context" => json!({"action": action, "project": "/tmp/project"}),
        "context" => json!({"action": action, "log_id": 1}),
        "get" => json!({"action": action, "id": 1}),
        "compare" => json!({
            "action": action,
            "a_from": "2026-01-01T00:00:00Z",
            "a_to": "2026-01-01T00:01:00Z",
            "b_from": "2026-01-01T00:01:00Z",
            "b_to": "2026-01-01T00:02:00Z",
        }),
        _ => json!({"action": action}),
    }
}

fn rmcp_router(state: AppState) -> Router {
    let config = streamable_http_config(&state.config);
    Router::new().nest_service("/mcp", streamable_http_service(state, config))
}

fn jsonrpc_request(id: u64, method: &str, params: Option<Value>) -> Value {
    let mut req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
    });
    if let Some(params) = params {
        req.as_object_mut()
            .unwrap()
            .insert("params".to_string(), params);
    }
    req
}

async fn post_rmcp(router: Router, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "localhost:3100")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn content_json(response: &Value) -> Value {
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    serde_json::from_str(text).unwrap()
}

fn structured_json(response: &Value) -> &Value {
    &response["result"]["structuredContent"]
}

#[test]
fn allowed_hosts_include_bracketed_ipv6_authority_variants() {
    let mut config = McpConfig {
        host: "::1".into(),
        port: 3100,
        server_name: "syslog-mcp".into(),
        no_auth: false,
        api_token: None,
        allowed_hosts: vec!["[fd00::1]".into(), "syslog.example.com:443".into()],
        allowed_origins: Vec::new(),
        auth: Default::default(),
        static_token_is_admin: false,
    };

    let hosts = allowed_hosts(&config);
    assert!(hosts.contains(&"::1".to_string()));
    assert!(hosts.contains(&"[::1]".to_string()));
    assert!(hosts.contains(&"[::1]:3100".to_string()));
    assert!(!hosts.contains(&"::1:3100".to_string()));

    config.host = "0.0.0.0".into();
    let hosts = allowed_hosts(&config);
    assert!(hosts.contains(&"[fd00::1]:3100".to_string()));
    assert!(hosts.contains(&"syslog.example.com:443".to_string()));
    assert!(!hosts.contains(&"[syslog.example.com:443]".to_string()));
}

#[test]
fn busy_errors_are_not_validation_errors() {
    let error = anyhow::Error::new(crate::app::ServiceError::Busy(
        "database worker limit reached".into(),
    ));

    assert!(!is_validation_error(&error));
}

#[tokio::test]
async fn rmcp_tools_list_exposes_one_action_tool() {
    let (state, _pool, _dir) = test_state();
    let (status, response) = post_rmcp(
        rmcp_router(state),
        jsonrpc_request(1, "tools/list", Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let tools = response["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["syslog"]);
    assert_eq!(tools[0]["inputSchema"]["required"], json!(["action"]));
    assert_eq!(
        tools[0]["_meta"]["ui"]["resourceUri"],
        super::QUERY_WIDGET_RESOURCE_URI
    );
    assert_eq!(
        tools[0]["_meta"]["ui"]["visibility"],
        json!(["model", "app"])
    );
}

#[tokio::test]
async fn rmcp_get_stats_works_against_temp_db() {
    let (state, _pool, _dir) = test_state();
    let (status, response) = post_rmcp(
        rmcp_router(state),
        jsonrpc_request(
            2,
            "tools/call",
            Some(json!({"name": "syslog", "arguments": {"action": "stats"}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let stats = content_json(&response);
    assert_eq!(stats["total_logs"], 0);
    assert!(stats.get("logical_db_size_mb").is_some());
}

#[tokio::test]
async fn rmcp_search_logs_works_against_seeded_data() {
    let (state, pool, _dir) = test_state();
    db::insert_logs_batch(
        &pool,
        &[entry(
            "2026-01-01T00:00:00Z",
            "host-a",
            "err",
            "disk full",
            "10.0.0.1:514",
        )],
    )
    .unwrap();

    let (status, response) = post_rmcp(
        rmcp_router(state),
        jsonrpc_request(
            3,
            "tools/call",
            Some(json!({"name": "syslog", "arguments": {"action": "search", "query": "disk", "limit": 5}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let result = content_json(&response);
    assert_eq!(result["count"], 1);
    assert_eq!(result["logs"][0]["hostname"], "host-a");
    let structured = structured_json(&response);
    assert_eq!(structured["count"], 1);
    assert_eq!(structured["logs"][0]["message"], "disk full");
    assert!(
        response["result"]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("\"message\": \"disk full\"")),
        "text content should remain readable JSON; response: {response}"
    );
}

#[tokio::test]
async fn rmcp_correlate_events_rejects_bad_reference_time_as_invalid_params() {
    let (state, _pool, _dir) = test_state();
    let (status, response) = post_rmcp(
        rmcp_router(state),
        jsonrpc_request(
            4,
            "tools/call",
            Some(json!({"name": "syslog", "arguments": {"action": "correlate", "reference_time": "bad"}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["error"]["code"], -32602);
}

#[tokio::test]
async fn rmcp_correlate_events_rejects_bad_severity_as_invalid_params() {
    let (state, _pool, _dir) = test_state();
    let (status, response) = post_rmcp(
        rmcp_router(state),
        jsonrpc_request(
            5,
            "tools/call",
            Some(json!({
                "name": "syslog",
                "arguments": {
                    "action": "correlate",
                    "reference_time": "2026-01-01T00:00:00Z",
                    "severity_min": "loud"
                }
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["error"]["code"], -32602);
}

#[tokio::test]
async fn rmcp_search_rejects_bad_severity_as_invalid_params() {
    let (state, _pool, _dir) = test_state();
    let (status, response) = post_rmcp(
        rmcp_router(state),
        jsonrpc_request(
            6,
            "tools/call",
            Some(json!({
                "name": "syslog",
                "arguments": {
                    "action": "search",
                    "severity": "bogus"
                }
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["error"]["code"], -32602);
}

#[tokio::test]
async fn rmcp_numeric_args_reject_wrong_type_values() {
    for (id, arguments) in [
        (7, json!({"action": "tail", "n": "10"})),
        (8, json!({"action": "search", "limit": "5"})),
        (
            9,
            json!({
                "action": "correlate",
                "reference_time": "2026-01-01T00:00:00Z",
                "window_minutes": "5"
            }),
        ),
        (
            10,
            json!({
                "action": "correlate",
                "reference_time": "2026-01-01T00:00:00Z",
                "limit": null
            }),
        ),
    ] {
        let (state, _pool, _dir) = test_state();
        let (status, response) = post_rmcp(
            rmcp_router(state),
            jsonrpc_request(
                id,
                "tools/call",
                Some(json!({"name": "syslog", "arguments": arguments})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["error"]["code"], -32602);
    }
}

#[tokio::test]
async fn rmcp_correlate_events_preserves_truncation_and_host_grouping() {
    let (state, pool, _dir) = test_state();
    db::insert_logs_batch(
        &pool,
        &[
            entry(
                "2026-01-01T00:00:00Z",
                "host-a",
                "err",
                "disk full",
                "10.0.0.1:514",
            ),
            entry(
                "2026-01-01T00:01:00Z",
                "host-b",
                "warning",
                "service slow",
                "10.0.0.2:514",
            ),
        ],
    )
    .unwrap();

    let (status, response) = post_rmcp(
        rmcp_router(state),
        jsonrpc_request(
            11,
            "tools/call",
            Some(json!({
                "name": "syslog",
                "arguments": {
                    "action": "correlate",
                    "reference_time": "2026-01-01T00:00:00Z",
                    "window_minutes": 5,
                    "severity_min": "warning",
                    "limit": 1
                }
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let result = content_json(&response);
    assert_eq!(result["total_events"], 1);
    assert_eq!(result["hosts_count"], 1);
    assert_eq!(result["truncated"], true);
}

// ── PUBLIC_URL host/origin allowlist extension ───────────────────────────────

/// `SYSLOG_MCP_PUBLIC_URL` bare host is added to `allowed_hosts`.
#[test]
fn public_url_host_added_to_allowed_hosts() {
    let config = McpConfig {
        host: "0.0.0.0".into(),
        port: 3100,
        server_name: "syslog-mcp".into(),
        no_auth: false,
        api_token: None,
        allowed_hosts: Vec::new(),
        allowed_origins: Vec::new(),
        auth: crate::config::AuthConfig {
            public_url: Some("https://syslog.example.com".into()),
            ..Default::default()
        },
        static_token_is_admin: false,
    };

    let hosts = allowed_hosts(&config);
    assert!(
        hosts.contains(&"syslog.example.com".to_string()),
        "public_url bare host must be in allowed_hosts; got: {hosts:?}"
    );
}

/// `SYSLOG_MCP_PUBLIC_URL` standard-port https origin is added to `allowed_origins`
/// without the port (browsers omit default ports from the Origin header).
#[test]
fn public_url_origin_added_to_allowed_origins() {
    let config = McpConfig {
        host: "0.0.0.0".into(),
        port: 3100,
        server_name: "syslog-mcp".into(),
        no_auth: false,
        api_token: None,
        allowed_hosts: Vec::new(),
        allowed_origins: Vec::new(),
        auth: crate::config::AuthConfig {
            public_url: Some("https://syslog.example.com".into()),
            ..Default::default()
        },
        static_token_is_admin: false,
    };

    let origins = allowed_origins(&config);
    // https on port 443 (default) — browser omits port from Origin header.
    assert!(
        origins.contains(&"https://syslog.example.com".to_string()),
        "public_url origin must be in allowed_origins; got: {origins:?}"
    );
}

/// Non-standard port: host and origin variants both include the explicit port.
#[test]
fn public_url_non_standard_port_included_in_host_and_origin() {
    let config = McpConfig {
        host: "0.0.0.0".into(),
        port: 3100,
        server_name: "syslog-mcp".into(),
        no_auth: false,
        api_token: None,
        allowed_hosts: Vec::new(),
        allowed_origins: Vec::new(),
        auth: crate::config::AuthConfig {
            public_url: Some("https://syslog.example.com:8443".into()),
            ..Default::default()
        },
        static_token_is_admin: false,
    };

    let hosts = allowed_hosts(&config);
    // Non-standard port: both bare host and host:port must be present.
    // Browsers include the port in the Host header for non-standard ports.
    assert!(
        hosts.contains(&"syslog.example.com".to_string()),
        "expected bare host in allowed_hosts for non-standard port; got: {hosts:?}"
    );
    assert!(
        hosts.contains(&"syslog.example.com:8443".to_string()),
        "expected host:port in allowed_hosts for non-standard port; got: {hosts:?}"
    );

    let origins = allowed_origins(&config);
    // Non-standard port is included in the Origin header by browsers.
    assert!(
        origins.contains(&"https://syslog.example.com:8443".to_string()),
        "expected https://syslog.example.com:8443 in allowed_origins; got: {origins:?}"
    );
}

/// Standard port (https:443): bare host AND host:443 must be in allowed_hosts.
/// Browsers omit the default port from the Host header, so bare host is required.
/// host:443 is also added so rmcp's port-aware comparison passes.
#[test]
fn public_url_standard_https_port_host_variants() {
    let config = McpConfig {
        host: "0.0.0.0".into(),
        port: 3100,
        server_name: "syslog-mcp".into(),
        no_auth: false,
        api_token: None,
        allowed_hosts: Vec::new(),
        allowed_origins: Vec::new(),
        auth: crate::config::AuthConfig {
            public_url: Some("https://syslog.example.com".into()),
            ..Default::default()
        },
        static_token_is_admin: false,
    };

    let hosts = allowed_hosts(&config);
    // Bare host: what browsers send when using the default port.
    assert!(
        hosts.contains(&"syslog.example.com".to_string()),
        "expected bare host in allowed_hosts for standard port; got: {hosts:?}"
    );
    // host:443: for rmcp's port-aware comparison when the URL port is explicit.
    assert!(
        hosts.contains(&"syslog.example.com:443".to_string()),
        "expected host:443 in allowed_hosts for standard-https URL; got: {hosts:?}"
    );
}

// ── Scope-based authorization tests ──────────────────────────────────────────
//
// These tests verify the fail-closed scope check added in syslog-mcp-brt0.8.
// Pattern: middleware injects AuthContext into request extensions; rmcp
// propagates it into RequestContext.extensions via http::request::Parts.

/// `AuthPolicy::LoopbackDev` bypasses all auth checks — any action succeeds
/// regardless of whether AuthContext is present.
#[tokio::test]
async fn loopback_dev_policy_permits_all_actions_without_auth_context() {
    let (state, _pool, _dir) = test_state();
    // No auth middleware — AuthContext is NOT in extensions.
    let router = rmcp_router_no_auth_middleware(state);

    // tools/list should succeed without AuthContext under LoopbackDev.
    let (status, response) = post_rmcp(
        router.clone(),
        jsonrpc_request(10, "tools/list", Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        response["result"]["tools"].is_array(),
        "response: {response}"
    );

    let (status, response) = post_rmcp(
        router.clone(),
        jsonrpc_request(12, "resources/list", Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        response["result"]["resources"].is_array(),
        "resources/list should succeed under LoopbackDev; response: {response}"
    );

    // tools/call should succeed without AuthContext under LoopbackDev.
    let (status, response) = post_rmcp(
        router,
        jsonrpc_request(
            11,
            "tools/call",
            Some(json!({"name": "syslog", "arguments": {"action": "stats"}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(response["result"].is_object(), "response: {response}");
}

/// `AuthPolicy::Mounted` + valid AuthContext with `syslog:read` → read
/// actions permitted, but admin actions (ack_error, unack_error, notifications_test)
/// are denied.
#[tokio::test]
async fn mounted_policy_with_read_scope_permits_read_actions() {
    let (state, pool, _dir) = mounted_state();
    seed_auth_action_log(&pool);
    let auth = auth_ctx_with_scopes(vec!["syslog:read"]);
    let router = rmcp_router_with_auth(state, auth);

    for action in actions::ACTION_SPECS
        .iter()
        .map(|s| s.name)
        .filter(|action| {
            *action != "help" && actions::required_scope_for(action) != Some("syslog:admin")
        })
    {
        let (status, response) = post_rmcp(
            router.clone(),
            jsonrpc_request(
                20,
                "tools/call",
                Some(json!({"name": "syslog", "arguments": minimal_args_for_action(action)})),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "action={action} should succeed; response: {response}"
        );
        // Must not be a forbidden error (-32600).
        assert_ne!(
            response["error"]["code"], -32600,
            "action={action} got forbidden; response: {response}"
        );
    }

    // Admin actions must be denied for syslog:read-only callers.
    for action in actions::ACTION_SPECS
        .iter()
        .map(|s| s.name)
        .filter(|a| actions::required_scope_for(a) == Some("syslog:admin"))
    {
        let (status, response) = post_rmcp(
            router.clone(),
            jsonrpc_request(
                21,
                "tools/call",
                Some(json!({"name": "syslog", "arguments": minimal_args_for_action(action)})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            response["error"]["code"], -32600,
            "admin action={action} should be denied with read-only scope; response: {response}"
        );
        let msg = response["error"]["message"].as_str().unwrap_or("");
        assert!(
            msg.contains("requires scope: syslog:admin"),
            "denial message should reference admin scope; got: {msg}"
        );
    }
}

#[test]
fn public_read_actions_require_syslog_read_scope() {
    for action in actions::ACTION_SPECS
        .iter()
        .map(|s| s.name)
        .filter(|action| {
            *action != "help" && actions::required_scope_for(action) != Some("syslog:admin")
        })
    {
        assert_eq!(
            required_scope_for(action),
            Some("syslog:read"),
            "action={action} must require syslog:read"
        );
    }
    // Admin actions require syslog:admin, not syslog:read
    for action in actions::ACTION_SPECS
        .iter()
        .map(|s| s.name)
        .filter(|a| actions::required_scope_for(a) == Some("syslog:admin"))
    {
        assert_eq!(
            required_scope_for(action),
            Some("syslog:admin"),
            "admin action={action} must require syslog:admin"
        );
    }
    assert_eq!(required_scope_for("help"), None);
    assert_eq!(
        required_scope_for("not_a_real_action"),
        Some("syslog:__deny__")
    );
}

#[test]
fn sessions_action_requires_read_scope() {
    assert_eq!(required_scope_for("sessions"), Some("syslog:read"));
}

#[test]
fn compose_actions_require_read_scope() {
    assert_eq!(required_scope_for("compose_status"), Some("syslog:read"));
    assert_eq!(required_scope_for("compose_doctor"), Some("syslog:read"));
}

/// `AuthPolicy::Mounted` + AuthContext with `syslog:admin` (superset) → read
/// actions permitted because `syslog:admin` implies `syslog:read`.
#[tokio::test]
async fn mounted_policy_with_admin_scope_permits_read_actions() {
    let (state, _pool, _dir) = mounted_state();
    // syslog:admin is a superset of syslog:read — check_scope treats it as
    // satisfying any syslog:read requirement (admin ⊃ read superset semantics).
    let auth = auth_ctx_with_scopes(vec!["syslog:admin"]);
    let router = rmcp_router_with_auth(state, auth);

    let (status, response) = post_rmcp(
        router,
        jsonrpc_request(
            30,
            "tools/call",
            Some(json!({"name": "syslog", "arguments": {"action": "stats"}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // syslog:admin implies syslog:read — must be permitted.
    assert_ne!(
        response["error"]["code"], -32600,
        "syslog:admin should satisfy syslog:read requirement; response: {response}"
    );
    assert!(
        response["result"].is_object(),
        "stats should return result; response: {response}"
    );
}

/// `AuthPolicy::Mounted` + AuthContext with BOTH scopes → all actions permitted.
#[tokio::test]
async fn mounted_policy_with_both_scopes_permits_all_actions() {
    let (state, _pool, _dir) = mounted_state();
    let auth = auth_ctx_with_scopes(vec!["syslog:read", "syslog:admin"]);
    let router = rmcp_router_with_auth(state, auth);

    let (status, response) = post_rmcp(
        router,
        jsonrpc_request(
            40,
            "tools/call",
            Some(json!({"name": "syslog", "arguments": {"action": "stats"}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(response["result"].is_object(), "response: {response}");
}

#[tokio::test]
async fn mounted_admin_actions_record_per_request_subject_actor() {
    let (state, pool, _dir) = mounted_state();
    let signature_hash = "1111111111111111111111111111111111111111111111111111111111111111";
    seed_error_signature(&pool, signature_hash);

    let alice_router = rmcp_router_with_auth(
        state.clone(),
        auth_ctx("alice-subject", vec!["syslog:admin"], None),
    );
    let (status, response) = post_rmcp(
        alice_router,
        jsonrpc_request(
            41,
            "tools/call",
            Some(json!({
                "name": "syslog",
                "arguments": {
                    "action": "ack_error",
                    "signature_hash": signature_hash,
                    "notes": "alice ack"
                }
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ack = content_json(&response);
    assert_eq!(ack["actor"], "alice-subject", "response: {response}");

    let bob_router = rmcp_router_with_auth(
        state,
        auth_ctx("bob-subject", vec!["syslog:admin"], Some("bob@example.com")),
    );
    let (status, response) = post_rmcp(
        bob_router,
        jsonrpc_request(
            42,
            "tools/call",
            Some(json!({
                "name": "syslog",
                "arguments": {
                    "action": "unack_error",
                    "signature_hash": signature_hash,
                    "reason": "bob unack"
                }
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let unack = content_json(&response);
    assert_eq!(unack["actor"], "bob@example.com", "response: {response}");

    let conn = pool.get().unwrap();
    let events = conn
        .prepare(
            "SELECT event_type, actor FROM error_signature_ack_events
             WHERE signature_hash = ?1
             ORDER BY id",
        )
        .unwrap()
        .query_map([signature_hash], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(
        events,
        vec![
            ("ack".to_string(), "alice-subject".to_string()),
            ("unack".to_string(), "bob@example.com".to_string()),
        ]
    );
}

/// `AuthPolicy::Mounted` + AuthContext with EMPTY scopes + any action → denied.
#[tokio::test]
async fn mounted_policy_with_empty_scopes_denies_read_actions() {
    let (state, _pool, _dir) = mounted_state();
    let auth = auth_ctx_with_scopes(vec![]);
    let router = rmcp_router_with_auth(state, auth);

    for action in actions::ACTION_SPECS
        .iter()
        .map(|s| s.name)
        .filter(|action| *action != "help")
    {
        let (status, response) = post_rmcp(
            router.clone(),
            jsonrpc_request(
                50,
                "tools/call",
                Some(json!({"name": "syslog", "arguments": minimal_args_for_action(action)})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            response["error"]["code"], -32600,
            "action={action} with empty scopes should be denied; response: {response}"
        );
        let msg = response["error"]["message"].as_str().unwrap_or("");
        // Read actions require syslog:read; admin actions require syslog:admin.
        let expected_scope = if actions::required_scope_for(action) == Some("syslog:admin") {
            "syslog:admin"
        } else {
            "syslog:read"
        };
        assert!(
            msg.contains(&format!("requires scope: {expected_scope}")),
            "error message should name the required scope '{expected_scope}' for action={action}; got: {msg}"
        );
    }
}

/// `AuthPolicy::Mounted` + AuthContext with empty scopes + `help` action →
/// permitted (help requires AuthContext but no scope).
#[tokio::test]
async fn mounted_policy_with_empty_scopes_permits_help_action() {
    let (state, _pool, _dir) = mounted_state();
    let auth = auth_ctx_with_scopes(vec![]);
    let router = rmcp_router_with_auth(state, auth);

    let (status, response) = post_rmcp(
        router,
        jsonrpc_request(
            60,
            "tools/call",
            Some(json!({"name": "syslog", "arguments": {"action": "help"}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // help should succeed (no scope gate) and return tool content.
    assert!(
        response["result"]["content"].is_array(),
        "help should return content; response: {response}"
    );
}

/// Fail-closed: `AuthPolicy::Mounted` + **missing** AuthContext (simulating
/// broken middleware ordering) → ALL actions denied, including `help` and
/// `tools/list`.
#[tokio::test]
async fn mounted_policy_missing_auth_context_denies_all_including_help_and_tools_list() {
    let (state, _pool, _dir) = mounted_state();
    // No auth middleware — AuthContext absent from extensions.
    let router = rmcp_router_no_auth_middleware(state);

    // tools/list must be denied when AuthContext absent under Mounted policy.
    let (status, response) = post_rmcp(
        router.clone(),
        jsonrpc_request(70, "tools/list", Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response["error"]["code"], -32600,
        "tools/list with missing AuthContext should be forbidden; response: {response}"
    );

    let (status, response) = post_rmcp(
        router.clone(),
        jsonrpc_request(73, "resources/list", Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response["error"]["code"], -32600,
        "resources/list with missing AuthContext should be forbidden; response: {response}"
    );

    let (status, response) = post_rmcp(
        router.clone(),
        jsonrpc_request(
            74,
            "resources/read",
            Some(json!({"uri": super::SCHEMA_RESOURCE_URI})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response["error"]["code"], -32600,
        "resources/read with missing AuthContext should be forbidden; response: {response}"
    );

    // help must also be denied.
    let (status, response) = post_rmcp(
        router.clone(),
        jsonrpc_request(
            71,
            "tools/call",
            Some(json!({"name": "syslog", "arguments": {"action": "help"}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response["error"]["code"], -32600,
        "help with missing AuthContext should be forbidden; response: {response}"
    );

    // A read action must also be denied.
    let (status, response) = post_rmcp(
        router,
        jsonrpc_request(
            72,
            "tools/call",
            Some(json!({"name": "syslog", "arguments": {"action": "stats"}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response["error"]["code"], -32600,
        "stats with missing AuthContext should be forbidden; response: {response}"
    );
}

/// `AuthPolicy::Mounted` + valid AuthContext with `syslog:read` + `tools/list`
/// → capability discovery succeeds (AuthContext present, no scope required).
#[tokio::test]
async fn mounted_policy_with_auth_context_permits_tools_list() {
    let (state, _pool, _dir) = mounted_state();
    let auth = auth_ctx_with_scopes(vec![]);
    let router = rmcp_router_with_auth(state, auth);

    let (status, response) =
        post_rmcp(router, jsonrpc_request(80, "tools/list", Some(json!({})))).await;
    assert_eq!(status, StatusCode::OK);
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(
        tools[0]["name"], "syslog",
        "tools/list should return syslog tool; response: {response}"
    );
}

#[tokio::test]
async fn mounted_policy_with_auth_context_permits_schema_resources() {
    let (state, _pool, _dir) = mounted_state();
    let auth = auth_ctx_with_scopes(vec![]);
    let router = rmcp_router_with_auth(state, auth);

    let (status, response) = post_rmcp(
        router.clone(),
        jsonrpc_request(81, "resources/list", Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resources = response["result"]["resources"].as_array().unwrap();
    let uris: Vec<&str> = resources
        .iter()
        .filter_map(|resource| resource["uri"].as_str())
        .collect();
    assert!(
        uris.contains(&super::SCHEMA_RESOURCE_URI),
        "resources/list should expose schema resource; response: {response}"
    );
    assert!(
        uris.contains(&super::QUERY_WIDGET_RESOURCE_URI),
        "resources/list should expose query widget resource; response: {response}"
    );

    let (status, response) = post_rmcp(
        router,
        jsonrpc_request(
            82,
            "resources/read",
            Some(json!({"uri": super::SCHEMA_RESOURCE_URI})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        response["result"]["contents"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("\"name\": \"syslog\"")),
        "resources/read should return schema JSON; response: {response}"
    );
}

#[tokio::test]
async fn mounted_policy_with_auth_context_permits_query_widget_resource() {
    let (state, _pool, _dir) = mounted_state();
    let auth = auth_ctx_with_scopes(vec![]);
    let router = rmcp_router_with_auth(state, auth);

    let (status, response) = post_rmcp(
        router,
        jsonrpc_request(
            83,
            "resources/read",
            Some(json!({"uri": super::QUERY_WIDGET_RESOURCE_URI})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response["result"]["contents"][0]["uri"],
        super::QUERY_WIDGET_RESOURCE_URI
    );
    assert_eq!(
        response["result"]["contents"][0]["mimeType"],
        super::MCP_APP_HTML_MIME_TYPE
    );
    assert!(
        response["result"]["contents"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("data-syslog-query-widget")),
        "resources/read should return query widget HTML; response: {response}"
    );
}

/// Scope check fires BEFORE execute_tool — a read denied by scope must not
/// trigger any DB query. Verified by asserting the error comes back without
/// any `content` field (DB results would appear in content).
#[tokio::test]
async fn scope_check_fires_before_db_execution() {
    let (state, _pool, _dir) = mounted_state();
    let auth = auth_ctx_with_scopes(vec![]); // no scopes → denied before DB
    let router = rmcp_router_with_auth(state, auth);

    let (status, response) = post_rmcp(
        router,
        jsonrpc_request(
            90,
            "tools/call",
            Some(json!({"name": "syslog", "arguments": {"action": "search", "query": "error"}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Must be a JSON-RPC error (scope denied), not a successful result.
    assert_eq!(
        response["error"]["code"], -32600,
        "scope check must fire before DB; response: {response}"
    );
    assert!(
        response.get("result").is_none() || response["result"].is_null(),
        "no result should be present when scope check fails; response: {response}"
    );
}

/// Unknown action → denied by `syslog:__deny__` sentinel, not passed through.
///
/// The catch-all arm of `required_scope_for` returns `Some("syslog:__deny__")`
/// — a scope that is never granted — so unknown actions are rejected at the
/// auth layer rather than falling through to `execute_tool`.
/// This prevents future actions added to dispatch but not to the scope map
/// from being silently accessible with only `syslog:read`.
#[tokio::test]
async fn unknown_action_is_denied_by_sentinel_scope() {
    let (state, _pool, _dir) = mounted_state();
    // syslog:read + syslog:admin — both real scopes, but neither matches __deny__
    let auth = auth_ctx_with_scopes(vec!["syslog:read", "syslog:admin"]);
    let router = rmcp_router_with_auth(state, auth);

    let (status, response) = post_rmcp(
        router,
        jsonrpc_request(
            100,
            "tools/call",
            Some(json!({"name": "syslog", "arguments": {"action": "not_a_real_action"}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Must be a JSON-RPC error — the sentinel scope is never granted.
    assert_eq!(
        response["error"]["code"], -32600,
        "unknown action must be denied by sentinel scope; response: {response}"
    );
    let msg = response["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("requires scope"),
        "denial message should reference scope requirement; got: {msg}"
    );
}
