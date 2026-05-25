use super::*;

use std::time::Duration;

#[tokio::test]
async fn fake_collector_emits_valid_v1_payload_defaults() {
    let collector = HeartbeatCollector::fake();
    let payload = collector
        .collect(
            "syslog_testhostid1234".to_string(),
            7,
            Duration::from_secs(DEFAULT_INTERVAL_SECS),
            0,
            Duration::from_millis(DEFAULT_PROBE_DEADLINE_MS),
            Duration::from_millis(DEFAULT_COLLECTION_DEADLINE_MS),
        )
        .await;

    assert_eq!(payload.schema_version, 1);
    assert_eq!(payload.host.host_id, "syslog_testhostid1234");
    assert_eq!(payload.sample.sequence, 7);
    assert_eq!(payload.agent.interval_secs, 30);
    assert!(!payload.sample.partial);
    assert!(payload.cpu.is_some());
    assert!(payload.memory.is_some());
    assert_eq!(payload.disks.len(), 1);
    assert_eq!(payload.networks.len(), 1);
    let json = serde_json::to_value(&payload).unwrap();
    assert!(json.get("networks").is_some());
    assert!(json.get("network").is_none());
}

#[tokio::test]
async fn failed_fake_probe_produces_partial_snapshot() {
    let collector = HeartbeatCollector::with_probes(vec![
        Box::new(FakeProbe::cpu()),
        Box::new(FakeProbe::failing("memory")),
        Box::new(FakeProbe::disk()),
    ]);

    let payload = collector
        .collect(
            "syslog_testhostid1234".to_string(),
            1,
            Duration::from_secs(30),
            0,
            Duration::from_millis(100),
            Duration::from_millis(500),
        )
        .await;

    assert!(payload.sample.partial);
    assert!(payload.cpu.is_some());
    assert_eq!(payload.disks.len(), 1);
    assert!(payload
        .sample
        .probe_errors
        .iter()
        .any(|error| error.contains("memory")));
}

#[tokio::test]
async fn probe_deadline_skips_slow_probe_but_keeps_completed_data() {
    let collector = HeartbeatCollector::with_probes(vec![
        Box::new(FakeProbe::cpu()),
        Box::new(FakeProbe::memory().delayed(Duration::from_millis(100))),
        Box::new(FakeProbe::disk()),
    ]);

    let payload = collector
        .collect(
            "syslog_testhostid1234".to_string(),
            1,
            Duration::from_secs(30),
            0,
            Duration::from_millis(10),
            Duration::from_millis(200),
        )
        .await;

    assert!(payload.sample.partial);
    assert!(payload.cpu.is_some());
    assert!(payload.memory.is_none());
    assert!(payload
        .sample
        .skipped_probes
        .iter()
        .any(|probe| probe == "memory"));
}

#[test]
fn retry_buffer_is_bounded_and_drops_oldest() {
    let mut buffer = RetryBuffer::new(2);
    buffer.push(test_payload(1));
    buffer.push(test_payload(2));
    buffer.push(test_payload(3));

    assert_eq!(buffer.len(), 2);
    assert_eq!(buffer.pop_front().unwrap().sample.sequence, 2);
    assert_eq!(buffer.pop_front().unwrap().sample.sequence, 3);
}

#[test]
fn backoff_is_bounded() {
    assert_eq!(backoff_duration(0), Duration::from_millis(250));
    assert_eq!(backoff_duration(4), Duration::from_millis(4_000));
    assert_eq!(backoff_duration(20), Duration::from_millis(4_000));
}

#[test]
fn generated_host_id_is_persisted_and_not_machine_id_shaped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("heartbeat-host-id");
    let first = load_or_create_host_id(&path).unwrap();
    let second = load_or_create_host_id(&path).unwrap();

    assert_eq!(first, second);
    assert!(first.starts_with("syslog_"));
    assert_ne!(first.len(), 32);
    assert_ne!(first.len(), 64);
}

fn test_payload(sequence: i64) -> HeartbeatPayload {
    HeartbeatPayload {
        schema_version: 1,
        host: HeartbeatHost {
            host_id: "syslog_testhostid1234".to_string(),
            hostname: "host".to_string(),
            os: "linux".to_string(),
            kernel: None,
            architecture: "x86_64".to_string(),
            boot_id: "boot".to_string(),
            timezone: None,
        },
        sample: HeartbeatSample {
            sequence,
            sampled_at: "2026-05-25T00:00:00.000Z".to_string(),
            uptime_secs: 1,
            monotonic_ms: 1,
            collection_ms: 1,
            partial: false,
            probe_errors: Vec::new(),
            skipped_probes: Vec::new(),
        },
        agent: HeartbeatAgentInfo {
            version: "0.0.0".to_string(),
            mode: "always_on".to_string(),
            interval_secs: 30,
            push_latency_ms: None,
            retry_backlog: 0,
        },
        cpu: None,
        memory: None,
        disks: Vec::new(),
        networks: Vec::new(),
        processes: None,
        containers: None,
    }
}
