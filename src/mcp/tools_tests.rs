use super::*;
use crate::app::SyslogService;
use crate::config::{McpConfig, StorageConfig};
use crate::db;
use crate::mcp::AppState;
use serde_json::json;
use std::sync::Arc;

fn test_state_with_token(token: Option<String>) -> (AppState, Arc<db::DbPool>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("mcp-test.db"));
    let pool = Arc::new(db::init_pool(&storage).unwrap());
    (
        AppState {
            service: SyslogService::new(Arc::clone(&pool), storage.clone()),
            config: McpConfig {
                host: "127.0.0.1".into(),
                port: 3100,
                server_name: "syslog-mcp".into(),
                api_token: token,
                allowed_hosts: Vec::new(),
                allowed_origins: Vec::new(),
            },
            otlp_counters: Arc::new(crate::otlp::OtlpCounters::default()),
            observability: Arc::new(crate::observability::RuntimeObservability::default()),
        },
        pool,
        dir,
    )
}

struct TestHarness {
    state: AppState,
    pool: Arc<db::DbPool>,
    _dir: tempfile::TempDir,
}

impl TestHarness {
    fn new() -> Self {
        let (state, pool, dir) = test_state_with_token(None);
        TestHarness {
            state,
            pool,
            _dir: dir,
        }
    }
}

#[tokio::test]
async fn tool_get_stats_returns_storage_guard_fields() {
    let h = TestHarness::new();
    let state = h.state;
    let value = tool_get_stats(&state, json!({})).await.unwrap();
    assert!(value.get("logical_db_size_mb").is_some());
    assert!(value.get("physical_db_size_mb").is_some());
    assert!(value.get("write_blocked").is_some());
    assert!(value.get("phantom_fts_rows").is_some());
    assert!(value.get("runtime_observability").is_some());
    assert!(value.get("otlp").is_some());
}

#[tokio::test]
async fn tool_get_status_returns_runtime_observability() {
    let h = TestHarness::new();
    let value = tool_get_status(&h.state, json!({})).await.unwrap();
    assert_eq!(value["status"], "ok");
    assert_eq!(value["db_ok"], true);
    assert!(value["runtime_observability"]["ingest_queue_depth"].is_number());
    assert!(value["otlp"]["logs_received"].is_number());
}

#[tokio::test]
async fn numeric_args_reject_out_of_range_values() {
    let h = TestHarness::new();
    let err = execute_tool(
        &h.state,
        "syslog",
        json!({"action": "tail", "n": u64::from(u32::MAX) + 1}),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("n must be <="));
}

#[tokio::test]
async fn numeric_args_reject_wrong_type_values() {
    let h = TestHarness::new();
    for args in [
        json!({"action": "tail", "n": "not-a-number"}),
        json!({"action": "search", "limit": "5"}),
        json!({"action": "correlate", "reference_time": "2026-01-01T00:00:00Z", "window_minutes": "5"}),
        json!({"action": "correlate", "reference_time": "2026-01-01T00:00:00Z", "limit": null}),
    ] {
        let err = execute_tool(&h.state, "syslog", args).await.unwrap_err();
        assert!(err.to_string().contains("must be an unsigned integer"));
    }
}

#[tokio::test]
async fn schema_actions_are_dispatchable() {
    let h = TestHarness::new();
    db::insert_logs_batch(
        &h.pool,
        &[db::LogBatchEntry {
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            hostname: "schema-test-host".to_string(),
            facility: Some("auth".to_string()),
            severity: "err".to_string(),
            app_name: Some("schema-test".to_string()),
            process_id: Some("42".to_string()),
            message: "schema dispatch test".to_string(),
            raw: "<11>schema dispatch test".to_string(),
            source_ip: "127.0.0.1:514".to_string(),
            docker_checkpoint: None,
        }],
    )
    .unwrap();
    for action in SYSLOG_ACTIONS {
        let args = match *action {
            "correlate" => {
                json!({"action": action, "reference_time": "2026-01-01T00:00:00Z"})
            }
            "context" => {
                json!({"action": action, "hostname": "schema-test-host", "timestamp": "2026-01-01T00:00:00Z"})
            }
            "get" => json!({"action": action, "id": 1}),
            "compare" => {
                json!({"action": action, "a_from": "2026-01-01T00:00:00Z", "a_to": "2026-01-01T00:01:00Z", "b_from": "2026-01-01T00:01:00Z", "b_to": "2026-01-01T00:02:00Z"})
            }
            _ => json!({"action": action}),
        };
        execute_tool(&h.state, "syslog", args)
            .await
            .unwrap_or_else(|error| panic!("schema action {action} did not dispatch: {error}"));
    }
}

#[tokio::test]
async fn public_action_references_cover_schema_registry() {
    let help = tool_syslog_help().await.unwrap();
    let help = help["help"].as_str().unwrap().to_ascii_lowercase();
    for action in SYSLOG_ACTIONS {
        assert!(
            help.contains(&format!("## syslog {action}")),
            "help text missing action section: {action}"
        );
    }

    for (path, content) in [
        (
            "scripts/smoke-test.sh",
            include_str!("../../scripts/smoke-test.sh"),
        ),
        (
            "tests/test_live.sh",
            include_str!("../../tests/test_live.sh"),
        ),
        (
            "tests/mcporter/test-tools.sh",
            include_str!("../../tests/mcporter/test-tools.sh"),
        ),
    ] {
        for action in SYSLOG_ACTIONS {
            assert!(
                content.contains(&format!("syslog {action}"))
                    || content.contains(&format!("mcp_call {action}"))
                    || content.contains(&format!("\"action\":\"{action}\"")),
                "{path} missing action coverage for {action}"
            );
        }
    }

    for (path, content) in [
        ("docs/mcp/TOOLS.md", include_str!("../../docs/mcp/TOOLS.md")),
        ("docs/mcp/TESTS.md", include_str!("../../docs/mcp/TESTS.md")),
        (
            "plugins/skills/syslog/SKILL.md",
            include_str!("../../plugins/skills/syslog/SKILL.md"),
        ),
    ] {
        for action in SYSLOG_ACTIONS {
            assert!(
                content.contains(&format!("`{action}`"))
                    || content.contains(&format!("syslog {action}")),
                "{path} missing action reference for {action}"
            );
        }
    }
}

#[tokio::test]
async fn syslog_tool_requires_known_action() {
    let h = TestHarness::new();
    let missing = execute_tool(&h.state, "syslog", json!({}))
        .await
        .unwrap_err();
    assert!(missing.to_string().contains("action is required"));

    let unknown = execute_tool(&h.state, "syslog", json!({"action": "reboot"}))
        .await
        .unwrap_err();
    assert!(unknown.to_string().contains("unknown syslog action"));
}

#[test]
fn parse_optional_timestamp_normalizes_offsets_to_utc() {
    let parsed = crate::app::parse_optional_timestamp(Some("2026-01-01T01:00:00+01:00"), "from")
        .unwrap()
        .unwrap();
    assert_eq!(parsed, "2026-01-01T00:00:00+00:00");
}

#[test]
fn parse_optional_timestamp_rejects_invalid_values() {
    let err = crate::app::parse_optional_timestamp(Some("not-a-date"), "from").unwrap_err();
    assert!(err.to_string().contains("Invalid from"));
}
