use std::time::Duration;

use crate::db::{DockerCheckpoint, LogBatchEntry};

use super::{
    entry_is_at_or_before_checkpoint, event_task_policy, jittered_reconnect_delay_ms,
    next_reconnect_backoff_ms, should_reset_reconnect_backoff, DockerEventTaskPolicy, StreamEnd,
    MIN_STREAM_DURATION_FOR_BACKOFF_RESET,
};

fn docker_entry(timestamp: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: timestamp.into(),
        hostname: "edge-host-a".into(),
        facility: Some("local0".into()),
        severity: "info".into(),
        app_name: Some("nginx".into()),
        process_id: Some("abcdef123456".into()),
        message: "line".into(),
        raw: format!("{timestamp} line"),
        source_ip: "docker://edge-host-a/abcdef123456/stdout".into(),
        docker_checkpoint: Some(DockerCheckpoint {
            host_name: "edge-host-a".into(),
            container_id: "abcdef123456".into(),
            timestamp: timestamp.into(),
        }),
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

#[test]
fn checkpoint_filter_skips_only_entries_at_or_before_precise_checkpoint() {
    let checkpoint =
        chrono::DateTime::parse_from_rfc3339("2026-05-05T01:02:03.500000000Z").unwrap();

    assert!(entry_is_at_or_before_checkpoint(
        &docker_entry("2026-05-05T01:02:03.123456789Z"),
        &checkpoint
    ));
    assert!(entry_is_at_or_before_checkpoint(
        &docker_entry("2026-05-05T01:02:03.500000000Z"),
        &checkpoint
    ));
    assert!(!entry_is_at_or_before_checkpoint(
        &docker_entry("2026-05-05T01:02:03.500000001Z"),
        &checkpoint
    ));
}

#[test]
fn docker_event_policy_maps_lifecycle_actions_to_task_work() {
    assert_eq!(
        event_task_policy("start"),
        DockerEventTaskPolicy::EnsureLogTask
    );
    assert_eq!(
        event_task_policy("restart"),
        DockerEventTaskPolicy::EnsureLogTask
    );
    assert_eq!(
        event_task_policy("rename"),
        DockerEventTaskPolicy::ReplaceLogTask
    );
    assert_eq!(event_task_policy("die"), DockerEventTaskPolicy::StopLogTask);
    assert_eq!(
        event_task_policy("stop"),
        DockerEventTaskPolicy::StopLogTask
    );
    assert_eq!(
        event_task_policy("destroy"),
        DockerEventTaskPolicy::StopLogTask
    );
    assert_eq!(
        event_task_policy("exec_start"),
        DockerEventTaskPolicy::Ignore
    );
}

#[test]
fn reconnect_backoff_resets_only_after_durable_clean_streams() {
    assert!(!should_reset_reconnect_backoff(
        StreamEnd::Clean,
        MIN_STREAM_DURATION_FOR_BACKOFF_RESET - Duration::from_millis(1)
    ));
    assert!(should_reset_reconnect_backoff(
        StreamEnd::Clean,
        MIN_STREAM_DURATION_FOR_BACKOFF_RESET
    ));
    assert!(should_reset_reconnect_backoff(
        StreamEnd::ExpectedDisconnect,
        MIN_STREAM_DURATION_FOR_BACKOFF_RESET + Duration::from_secs(1)
    ));
    assert!(!should_reset_reconnect_backoff(
        StreamEnd::Failed,
        MIN_STREAM_DURATION_FOR_BACKOFF_RESET + Duration::from_secs(1)
    ));
}

#[test]
fn reconnect_backoff_doubles_to_cap_unless_reset() {
    assert_eq!(
        next_reconnect_backoff_ms(1_000, 1_000, 30_000, false),
        2_000
    );
    assert_eq!(
        next_reconnect_backoff_ms(20_000, 1_000, 30_000, false),
        30_000
    );
    assert_eq!(
        next_reconnect_backoff_ms(u64::MAX, 1_000, 30_000, false),
        30_000
    );
    assert_eq!(next_reconnect_backoff_ms(8_000, 1_000, 30_000, true), 1_000);
}

#[test]
fn reconnect_delay_jitter_is_deterministic_and_bounded() {
    let first = jittered_reconnect_delay_ms(10_000, "edge-host-a");
    let second = jittered_reconnect_delay_ms(10_000, "edge-host-a");
    let other = jittered_reconnect_delay_ms(10_000, "edge-host-b");

    assert_eq!(first, second);
    assert_ne!(first, other);
    assert!((8_000..=12_000).contains(&first));
    assert!((8_000..=12_000).contains(&other));
}
