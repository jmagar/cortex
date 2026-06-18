//! Tests for the `topic_correlate` graph-anchored universal correlation action.

use super::*;
use crate::config::StorageConfig;
use crate::db::{LogBatchEntry, init_pool, insert_logs_batch};
use std::sync::Arc;

fn test_service() -> (CortexService, Arc<crate::db::DbPool>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("topic-correlate-test.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    (CortexService::new(Arc::clone(&pool), storage), pool, dir)
}

fn agent_command_log(ts: &str, host: &str, session: &str, cwd: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: host.to_string(),
        facility: Some("agent".to_string()),
        severity: "info".to_string(),
        app_name: Some("claude".to_string()),
        process_id: None,
        message: "cargo build --release --bin axon".to_string(),
        raw: "cargo build".to_string(),
        source_ip: format!("agent-command://{host}/claude/{session}"),
        docker_checkpoint: None,
        ai_tool: Some("claude".to_string()),
        ai_project: Some(cwd.to_string()),
        ai_session_id: Some(session.to_string()),
        ai_transcript_path: None,
        metadata_json: Some(format!(
            r#"{{"source_kind":"agent-command","agent_command":{{"cwd":"{cwd}"}}}}"#
        )),
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: Some("command".to_string()),
        parse_error: None,
    }
}

fn syslog(ts: &str, host: &str, app: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: ts.to_string(),
        hostname: host.to_string(),
        facility: None,
        severity: "warning".to_string(),
        app_name: Some(app.to_string()),
        process_id: None,
        message: format!("{app} event"),
        raw: format!("{app} event"),
        source_ip: "10.0.0.5:514".to_string(),
        docker_checkpoint: None,
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        ai_transcript_path: None,
        metadata_json: Some(r#"{"source_kind":"syslog-udp"}"#.to_string()),
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    }
}

async fn seeded_service() -> (CortexService, Arc<crate::db::DbPool>, tempfile::TempDir) {
    let (svc, pool, dir) = test_service();
    insert_logs_batch(
        &pool,
        &[
            agent_command_log(
                "2026-01-01T00:00:00Z",
                "dookie",
                "sess-7",
                "/home/jmagar/workspace/axon",
            ),
            syslog("2026-01-01T00:01:00Z", "dookie", "swag"),
            syslog("2026-01-01T00:02:00Z", "squirts", "authelia"),
        ],
    )
    .unwrap();
    crate::db::graph::refresh_graph_projection(&pool).unwrap();
    (svc, pool, dir)
}

#[tokio::test]
async fn topic_resolves_project_and_builds_timeline() {
    let _guard = crate::db::graph::GRAPH_TEST_LOCK.lock();
    let (svc, _pool, _dir) = seeded_service().await;

    let req = TopicCorrelateRequest {
        topic: "axon".to_string(),
        ..Default::default()
    };
    let resp = svc.topic_correlate(req).await.unwrap();

    assert_eq!(resp.topic, "axon");
    assert!(
        resp.resolved_entities
            .iter()
            .any(|e| e.entity_type == "ai_project" && e.key == "axon"),
        "topic must resolve to ai_project:axon: {:?}",
        resp.resolved_entities
    );
    // Graph expansion reaches the host the session ran on.
    assert!(
        resp.discovered_hosts.contains(&"dookie".to_string()),
        "expansion must discover dookie: {:?}",
        resp.discovered_hosts
    );
    // Timeline carries the agent-command row (Claude worked on axon).
    assert!(
        resp.timeline
            .iter()
            .any(|t| t.entity_path == "agent_command"),
        "timeline must include the agent-command lane"
    );
    // Unrelated host's logs are not pulled in.
    assert!(resp.timeline.iter().all(|t| t.hostname != "squirts"));
}

#[tokio::test]
async fn topic_with_no_match_is_empty() {
    let _guard = crate::db::graph::GRAPH_TEST_LOCK.lock();
    let (svc, _pool, _dir) = seeded_service().await;

    let req = TopicCorrelateRequest {
        topic: "nonexistent-topic-xyz".to_string(),
        ..Default::default()
    };
    let resp = svc.topic_correlate(req).await.unwrap();
    assert!(resp.resolved_entities.is_empty());
    assert!(resp.timeline.is_empty());
    assert!(!resp.truncated);
}

#[tokio::test]
async fn topic_multi_term_resolves_host_and_project() {
    let _guard = crate::db::graph::GRAPH_TEST_LOCK.lock();
    let (svc, _pool, _dir) = seeded_service().await;

    let req = TopicCorrelateRequest {
        topic: "dookie axon".to_string(),
        ..Default::default()
    };
    let resp = svc.topic_correlate(req).await.unwrap();
    let types: std::collections::HashSet<&str> = resp
        .resolved_entities
        .iter()
        .map(|e| e.entity_type.as_str())
        .collect();
    assert!(types.contains("host"), "dookie → host: {types:?}");
    assert!(types.contains("ai_project"), "axon → ai_project: {types:?}");
}

#[tokio::test]
async fn topic_source_kind_filter_restricts_timeline() {
    let _guard = crate::db::graph::GRAPH_TEST_LOCK.lock();
    let (svc, _pool, _dir) = seeded_service().await;

    let req = TopicCorrelateRequest {
        topic: "dookie".to_string(),
        source_kinds: Some(vec!["syslog-udp".to_string()]),
        ..Default::default()
    };
    let resp = svc.topic_correlate(req).await.unwrap();
    assert!(!resp.timeline.is_empty());
    assert!(
        resp.timeline
            .iter()
            .all(|t| t.entity_path != "agent_command"),
        "agent-command rows excluded by the syslog-udp filter"
    );
}
