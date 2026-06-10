use super::*;

use std::sync::Mutex;
use std::time::{Duration, Instant};

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
    assert!(
        payload
            .sample
            .probe_errors
            .iter()
            .any(|error| error.contains("memory"))
    );
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
    assert!(
        payload
            .sample
            .skipped_probes
            .iter()
            .any(|probe| probe == "memory")
    );
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
fn linux_meminfo_parser_returns_bounded_memory_snapshot() {
    let memory = parse_meminfo(
        "MemTotal:        1000 kB\nMemAvailable:     400 kB\nSwapTotal:        200 kB\nSwapFree:          50 kB\n",
    )
    .unwrap();

    assert_eq!(memory.mem_total_bytes, 1_024_000);
    assert_eq!(memory.mem_available_bytes, 409_600);
    assert_eq!(memory.mem_used_bytes, Some(614_400));
    assert_eq!(memory.swap_total_bytes, 204_800);
    assert_eq!(memory.swap_used_bytes, 153_600);
}

#[test]
fn linux_proc_parsers_extract_cpu_network_disk_and_process_state() {
    assert_eq!(
        parse_loadavg("0.10 0.20 0.30 1/100 123").unwrap(),
        (0.10, 0.20, 0.30)
    );
    assert_eq!(
        parse_diskstats_device("   8       0 sda 1 0 8 0 2 0 16 0 0 0 0 0 0 0 0\n", "sda"),
        Some((4096, 8192))
    );
    assert_eq!(
        parse_proc_stat_state("123 (syslog agent) S 1 2 3"),
        Some('S')
    );

    let network = parse_network_interface(
        "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n  eth0: 1000 1 2 0 0 0 0 0 2000 1 3 0 0 0 0 0\n",
        "eth0",
    )
    .unwrap()
    .unwrap();
    assert_eq!(network.rx_bytes, 1000);
    assert_eq!(network.tx_bytes, 2000);
    assert_eq!(network.rx_errors, 2);
    assert_eq!(network.tx_errors, 3);
}

#[test]
fn linux_rate_helpers_return_none_on_first_sample_and_rates_afterwards() {
    let previous = Mutex::new(None);
    let now = Instant::now();
    assert_eq!(rate_pair(&previous, now, 100, 200).unwrap(), (None, None));
    let (read, write) = rate_pair(&previous, now + Duration::from_secs(2), 300, 260).unwrap();
    assert_eq!(read, Some(100.0));
    assert_eq!(write, Some(30.0));

    let previous_network = Mutex::new(None);
    let current = NetworkCounters {
        rx_bytes: 100,
        tx_bytes: 200,
        rx_errors: 1,
        tx_errors: 2,
    };
    assert_eq!(
        network_rates(&previous_network, now, current).unwrap(),
        NetworkRates {
            rx_bytes_per_sec: None,
            tx_bytes_per_sec: None,
            rx_errors_per_sec: None,
            tx_errors_per_sec: None,
        }
    );
    let rates = network_rates(
        &previous_network,
        now + Duration::from_secs(4),
        NetworkCounters {
            rx_bytes: 500,
            tx_bytes: 1000,
            rx_errors: 5,
            tx_errors: 10,
        },
    )
    .unwrap();
    assert_eq!(
        rates,
        NetworkRates {
            rx_bytes_per_sec: Some(100.0),
            tx_bytes_per_sec: Some(200.0),
            rx_errors_per_sec: Some(1.0),
            tx_errors_per_sec: Some(2.0),
        }
    );
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

#[test]
fn heartbeat_url_appends_path_to_bare_host() {
    assert_eq!(
        heartbeat_url("http://host:3100").unwrap(),
        "http://host:3100/v1/heartbeats"
    );
}

#[test]
fn heartbeat_url_strips_trailing_slash_before_appending() {
    assert_eq!(
        heartbeat_url("http://host:3100/").unwrap(),
        "http://host:3100/v1/heartbeats"
    );
}

#[test]
fn heartbeat_url_is_idempotent_when_path_already_present() {
    assert_eq!(
        heartbeat_url("http://host:3100/v1/heartbeats").unwrap(),
        "http://host:3100/v1/heartbeats"
    );
}

#[test]
fn heartbeat_url_rejects_non_http_scheme() {
    assert!(heartbeat_url("ftp://host:3100").is_err());
}

#[test]
fn retry_buffer_zero_limit_discards_all_pushes() {
    let mut buffer = RetryBuffer::new(0);
    buffer.push(test_payload(1));
    buffer.push(test_payload(2));
    assert_eq!(buffer.len(), 0);
    assert!(buffer.is_empty());
}

#[test]
fn parse_meminfo_errors_on_missing_mem_total() {
    let result = parse_meminfo("MemAvailable: 400 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("MemTotal"));
}

#[test]
fn parse_meminfo_errors_on_missing_mem_available() {
    let result = parse_meminfo("MemTotal: 1000 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("MemAvailable"));
}

#[test]
fn parse_docker_states_maps_all_known_states() {
    let containers = parse_docker_states(
        "running\nexited\ndead\ncreated\nremoving\npaused\nrestarting\nunknown_future_state\n",
    );
    assert!(containers.reachable);
    assert_eq!(containers.running, 1);
    assert_eq!(containers.exited, 5); // exited + dead + created + removing + paused
    assert_eq!(containers.restarting, 1);
    // unknown_future_state should be ignored without panicking
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
