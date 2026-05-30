use super::*;
use crate::app::CortexService;
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
            service: CortexService::new(Arc::clone(&pool), storage.clone()),
            config: McpConfig {
                host: "127.0.0.1".into(),
                port: 3100,
                server_name: "cortex".into(),
                no_auth: false,
                trusted_gateway_no_auth: false,
                api_token: token,
                allowed_hosts: Vec::new(),
                allowed_origins: Vec::new(),
                auth: Default::default(),
                static_token_is_admin: false,
            },
            notifications_config: crate::config::NotificationsConfig::default(),
            otlp_counters: Arc::new(crate::otlp::OtlpCounters::default()),
            auth_policy: crate::mcp::AuthPolicy::LoopbackDev,
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
async fn host_state_action_returns_bounded_heartbeat_state() {
    let h = TestHarness::new();
    let conn = h.pool.get().unwrap();
    for sequence in 1..=2 {
        conn.execute(
            "INSERT INTO host_heartbeats (
                 host_id, hostname, source_ip, sampled_at, received_at, boot_id,
                 uptime_secs, sequence, collection_ms, partial, agent_version,
                 os, architecture, metadata_json
             ) VALUES (
                 'host-a', 'tootie', '127.0.0.1:41000', ?1, ?1, 'boot-a',
                 60, ?2, 5, ?3, '0.1.0-test', 'linux', 'x86_64',
                 '{\"agent\":{\"interval_secs\":30}}'
             )",
            (
                format!("2026-05-25T00:0{sequence}:00Z"),
                sequence,
                (sequence == 2) as i64,
            ),
        )
        .unwrap();
    }
    drop(conn);

    let value = execute_tool(
        &h.state,
        "cortex",
        json!({"action": "host_state", "hostname": "tootie", "limit": 1}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(value["host_id"], "host-a");
    assert_eq!(value["samples"].as_array().unwrap().len(), 1);
    assert_eq!(value["flags"]["collector_partial"], true);
}

#[tokio::test]
async fn fleet_state_action_returns_fleet_snapshot() {
    let h = TestHarness::new();
    let value = execute_tool(&h.state, "cortex", json!({"action": "fleet_state"}), None)
        .await
        .unwrap();
    assert!(
        value.get("hosts").is_some(),
        "fleet_state response missing hosts: {value}"
    );
    assert!(
        value.get("summary").is_some(),
        "fleet_state response missing summary: {value}"
    );
}

#[tokio::test]
async fn host_state_action_reports_ambiguous_hostname() {
    let h = TestHarness::new();
    let conn = h.pool.get().unwrap();
    for host_id in ["host-a", "host-b"] {
        conn.execute(
            "INSERT INTO host_heartbeats (
                 host_id, hostname, source_ip, sampled_at, received_at, boot_id,
                 uptime_secs, sequence, collection_ms, partial, agent_version,
                 os, architecture
             ) VALUES (
                 ?1, 'shared', '127.0.0.1:41000', '2026-05-25T00:00:00Z',
                 '2026-05-25T00:00:01Z', 'boot-a', 60, 1, 5, 0,
                 '0.1.0-test', 'linux', 'x86_64'
             )",
            [host_id],
        )
        .unwrap();
    }
    drop(conn);

    let error = execute_tool(
        &h.state,
        "cortex",
        json!({"action": "host_state", "hostname": "shared"}),
        None,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("ambiguous_host"));
}

#[tokio::test]
async fn numeric_args_reject_out_of_range_values() {
    let h = TestHarness::new();
    let err = execute_tool(
        &h.state,
        "cortex",
        json!({"action": "tail", "n": u64::from(u32::MAX) + 1}),
        None,
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
        let err = execute_tool(&h.state, "cortex", args, None)
            .await
            .unwrap_err();
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
        }],
    )
    .unwrap();
    {
        let conn = h.pool.get().unwrap();
        conn.execute(
            "INSERT INTO host_heartbeats (
                 host_id, hostname, source_ip, sampled_at, received_at, boot_id,
                 uptime_secs, sequence, collection_ms, partial, agent_version,
                 os, architecture, metadata_json
             ) VALUES (
                 'schema-heartbeat', 'schema-heartbeat-host', '127.0.0.1:41000',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:01Z', 'boot-a',
                 60, 1, 5, 0, '0.1.0-test', 'linux', 'x86_64',
                 '{\"agent\":{\"interval_secs\":30}}'
             )",
            [],
        )
        .unwrap();
    }
    for action in &super::actions::action_names() {
        let args = match *action {
            "host_state" => json!({"action": action, "host_id": "schema-heartbeat"}),
            "correlate" => {
                json!({"action": action, "reference_time": "2026-01-01T00:00:00Z"})
            }
            "search_sessions" => json!({"action": action, "query": "schema"}),
            "ai_correlate" => json!({"action": action, "project": "/tmp/project"}),
            "project_context" => json!({"action": action, "project": "/tmp/project"}),
            "context" => {
                json!({"action": action, "hostname": "schema-test-host", "timestamp": "2026-01-01T00:00:00Z"})
            }
            "get" => json!({"action": action, "id": 1}),
            "compare" => {
                json!({"action": action, "a_from": "2026-01-01T00:00:00Z", "a_to": "2026-01-01T00:01:00Z", "b_from": "2026-01-01T00:01:00Z", "b_to": "2026-01-01T00:02:00Z"})
            }
            // ack_error / unack_error require signature_hash; provide a non-existent one
            // so they dispatch and return NotFound (not "required parameter" error).
            "ack_error" | "unack_error" => {
                json!({"action": action, "signature_hash": "0000000000000000000000000000000000000000000000000000000000000000"})
            }
            // RAG v1 actions require query or time range.
            "similar_incidents" => json!({"action": action, "query": "test"}),
            "ask_history" => json!({"action": action, "query": "test"}),
            "incident_context" => {
                json!({"action": action, "from": "2026-01-01T00:00:00Z", "to": "2026-01-01T01:00:00Z"})
            }
            _ => json!({"action": action}),
        };
        let result = execute_tool(&h.state, "cortex", args, None).await;
        if *action == "compose_doctor" {
            if let Err(error) = result {
                assert!(
                    error.to_string().contains("compose doctor failed"),
                    "compose_doctor failed before dispatching: {error}"
                );
            }
        } else if matches!(*action, "ack_error" | "unack_error") {
            // These require an existing signature. When given a non-existent hash,
            // they must return Err (ServiceError::NotFound propagates via ?).
            match result {
                Err(ref error) => {
                    assert!(
                        error.to_string().to_lowercase().contains("not found")
                            || error.to_string().contains("Signature"),
                        "action={action} returned unexpected error: {error}"
                    );
                }
                Ok(_) => {
                    panic!("action={action} with non-existent hash should return NotFound, got Ok")
                }
            }
        } else if *action == "notifications_test" {
            // notifications_test requires a live Apprise server; transient/delivery
            // failures are expected in test environments.
            if let Err(ref error) = result {
                assert!(
                    error.to_string().contains("Apprise")
                        || error.to_string().contains("delivery")
                        || error.to_string().contains("Rate limit")
                        || error.to_string().contains("no_apprise"),
                    "action={action} failed unexpectedly: {error}"
                );
            }
        } else {
            result
                .unwrap_or_else(|error| panic!("schema action {action} did not dispatch: {error}"));
        }
    }
}

#[tokio::test]
async fn public_action_references_cover_schema_registry() {
    let help = tool_cortex_help().await.unwrap();
    let help = help["help"].as_str().unwrap().to_ascii_lowercase();
    for action in &super::actions::action_names() {
        assert!(
            help.contains(&format!("## cortex {action}")),
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
        for action in &super::actions::action_names() {
            assert!(
                content.contains(&format!("cortex {action}"))
                    || content.contains(&format!("mcp_call {action}"))
                    || content.contains(&format!("\"action\":\"{action}\"")),
                "{path} missing action coverage for {action}"
            );
        }
    }

    for (path, content) in [
        ("docs/INVENTORY.md", include_str!("../../docs/INVENTORY.md")),
        (
            "docs/mcp/SCHEMA.md",
            include_str!("../../docs/mcp/SCHEMA.md"),
        ),
        ("docs/mcp/TOOLS.md", include_str!("../../docs/mcp/TOOLS.md")),
        ("docs/mcp/TESTS.md", include_str!("../../docs/mcp/TESTS.md")),
        (
            "plugins/cortex/skills/cortex/SKILL.md",
            include_str!("../../plugins/cortex/skills/cortex/SKILL.md"),
        ),
    ] {
        for action in &super::actions::action_names() {
            assert!(
                content.contains(&format!("`{action}`"))
                    || content.contains(&format!("cortex {action}")),
                "{path} missing action reference for {action}"
            );
        }
    }
}

#[tokio::test]
async fn syslog_tool_requires_known_action() {
    let h = TestHarness::new();
    let missing = execute_tool(&h.state, "cortex", json!({}), None)
        .await
        .unwrap_err();
    assert!(missing.to_string().contains("action is required"));

    let unknown = execute_tool(&h.state, "cortex", json!({"action": "reboot"}), None)
        .await
        .unwrap_err();
    assert!(unknown.to_string().contains("unknown cortex action"));
}

#[tokio::test]
async fn compose_action_rejects_target_override() {
    let h = TestHarness::new();
    for action in ["compose_status", "compose_doctor"] {
        for key in [
            "container",
            "container_name",
            "project_dir",
            "compose_file",
            "project_name",
            "service",
        ] {
            let mut args = json!({"action": action});
            args.as_object_mut()
                .unwrap()
                .insert(key.into(), json!("override-value"));
            let err = execute_tool(&h.state, "cortex", args, None)
                .await
                .unwrap_err();
            assert!(
                err.to_string().contains("target override"),
                "expected target override rejection for {action}.{key}, got: {err}"
            );
        }
    }
}

#[test]
fn parse_optional_timestamp_normalizes_offsets_to_utc() {
    let parsed = crate::app::parse_optional_timestamp(Some("2026-01-01T01:00:00+01:00"), "from")
        .unwrap()
        .unwrap();
    assert_eq!(parsed, "2026-01-01T00:00:00.000Z");
}

#[test]
fn parse_optional_timestamp_rejects_invalid_values() {
    let err = crate::app::parse_optional_timestamp(Some("not-a-date"), "from").unwrap_err();
    assert!(err.to_string().contains("Invalid from"));
}
