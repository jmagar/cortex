use super::*;
use std::collections::HashSet;

// ---- OTLP 404 ----

#[test]
fn otlp_404_produces_single_signature() {
    // Simulates Claude Code OTLP exporter logging repeated 404 errors.
    // Different request IDs, response sizes, sequence numbers — should all map
    // to exactly one signature hash (same format, same structure).
    let messages = [
        "OTLP export failed: POST /v1/metrics HTTP/1.1 404 0 req_id=a1b2c3d4",
        "OTLP export failed: POST /v1/metrics HTTP/1.1 404 0 req_id=e5f6a7b8",
        "OTLP export failed: POST /v1/metrics HTTP/1.1 404 0 req_id=11223344",
        "OTLP export failed: POST /v1/metrics HTTP/1.1 404 0 req_id=deadbeef",
        "OTLP export failed: POST /v1/metrics HTTP/1.1 404 128 req_id=cafebabe",
    ];
    let hashes: HashSet<String> = messages
        .iter()
        .map(|m| signature_hash(&normalize_template(m)))
        .collect();
    assert_eq!(
        hashes.len(),
        1,
        "Expected 1 unique signature for OTLP 404 messages, got {}: {:?}",
        hashes.len(),
        messages
            .iter()
            .map(|m| normalize_template(m))
            .collect::<Vec<_>>()
    );
}

#[test]
fn otlp_404_rfc3164_format_produces_single_signature() {
    // Same OTLP error but with RFC3164 timestamp prefix.
    let messages = [
        "Jan 15 08:30:00 cortex OTLP export failed: POST /v1/metrics HTTP/1.1 404 0",
        "Jan 15 08:30:10 cortex OTLP export failed: POST /v1/metrics HTTP/1.1 404 0",
        "Jan 15 09:00:00 cortex OTLP export failed: POST /v1/metrics HTTP/1.1 404 0",
        "Feb  1 12:00:00 cortex OTLP export failed: POST /v1/metrics HTTP/1.1 404 0",
    ];
    let hashes: HashSet<String> = messages
        .iter()
        .map(|m| signature_hash(&normalize_template(m)))
        .collect();
    assert_eq!(
        hashes.len(),
        1,
        "Expected 1 unique signature for RFC3164 OTLP 404 messages, got {}: {:?}",
        hashes.len(),
        messages
            .iter()
            .map(|m| normalize_template(m))
            .collect::<Vec<_>>()
    );
}

// ---- AdGuard JSON ----

#[test]
fn adguard_json_collapses_to_few_signatures() {
    // AdGuard emits JSON log lines with varying domain/upstream/timestamp.
    // The JSON pre-pass should collapse them to a small number of templates.
    let domains = [
        "example.com",
        "foo.bar",
        "cdn.cloudflare.com",
        "ads.google.com",
        "tracker.example.net",
        "api.stripe.com",
        "fonts.googleapis.com",
        "ssl.gstatic.com",
        "connect.facebook.net",
        "analytics.twitter.com",
    ];
    let upstreams = ["8.8.8.8:53", "1.1.1.1:53", "9.9.9.9:53"];

    let mut hashes = HashSet::new();
    for domain in &domains {
        for upstream in &upstreams {
            for i in 0..5u32 {
                let msg = format!(
                    r#"{{"QR":true,"Question":[{{"Name":"{domain}","Type":1}}],"Answer":[],"Upstream":"{upstream}","Elapsed":{i},"Result":{{"IsFiltered":false,"Reason":0}}}}"#
                );
                hashes.insert(signature_hash(&normalize_template(&msg)));
            }
        }
    }
    assert!(
        hashes.len() <= 10,
        "Expected ≤10 signatures for AdGuard JSON lines, got {}",
        hashes.len()
    );
}

// ---- sshd failed password ----

#[test]
fn sshd_failed_password_groups_by_user() {
    // Same user, different IPs/ports = same signature.
    // Different users = different signatures.
    let alice_msgs = [
        "Failed password for alice from 10.0.0.1 port 12345 ssh2",
        "Failed password for alice from 192.168.1.50 port 54321 ssh2",
        "Failed password for alice from 203.0.113.7 port 9999 ssh2",
    ];
    let bob_msgs = [
        "Failed password for bob from 10.0.0.1 port 12345 ssh2",
        "Failed password for bob from 10.0.0.2 port 22222 ssh2",
    ];
    let invalid_msgs = [
        "Failed password for invalid user charlie from 10.0.0.1 port 11111 ssh2",
        "Failed password for invalid user charlie from 10.0.0.2 port 22222 ssh2",
    ];

    let alice_hashes: HashSet<String> = alice_msgs
        .iter()
        .map(|m| signature_hash(&normalize_template(m)))
        .collect();
    assert_eq!(
        alice_hashes.len(),
        1,
        "alice messages should produce 1 signature"
    );

    let bob_hashes: HashSet<String> = bob_msgs
        .iter()
        .map(|m| signature_hash(&normalize_template(m)))
        .collect();
    assert_eq!(
        bob_hashes.len(),
        1,
        "bob messages should produce 1 signature"
    );

    let charlie_hashes: HashSet<String> = invalid_msgs
        .iter()
        .map(|m| signature_hash(&normalize_template(m)))
        .collect();
    assert_eq!(
        charlie_hashes.len(),
        1,
        "charlie invalid user messages should produce 1 signature"
    );

    // Different users must NOT share a signature
    let all_hashes: HashSet<String> = alice_msgs
        .iter()
        .chain(bob_msgs.iter())
        .chain(invalid_msgs.iter())
        .map(|m| signature_hash(&normalize_template(m)))
        .collect();
    assert_eq!(
        all_hashes.len(),
        3,
        "alice, bob, charlie should each have a distinct signature"
    );
}

// ---- existing: numbers, IPs, UUIDs ----

#[test]
fn template_normalises_numbers_ips_uuids() {
    let t = normalize_template(
        "connection refused from 10.0.0.5:42 (id b3a1c0de-1234-5678-9abc-def012345678)",
    );
    assert!(t.contains("<ip>:<n>"), "expected <ip>:<n> in: {t}");
    assert!(t.contains("<uuid>"), "expected <uuid> in: {t}");
}

#[test]
fn template_preserves_non_ascii() {
    let msg = "файл 1234 не найден";
    let t = normalize_template(msg);
    assert!(t.contains("файл"));
    assert!(t.contains("не найден"));
    assert!(t.contains("<n>"));
    assert!(t.is_char_boundary(t.len()));
}

#[test]
fn rfc3164_ts_stripped() {
    let t = normalize_template("Jan 15 08:30:00 sshd[1234]: message here");
    assert!(t.starts_with("<ts>"), "expected <ts> prefix, got: {t}");
    assert!(!t.contains("Jan"), "month should be replaced: {t}");
}

#[test]
fn json_span_replaced() {
    let t = normalize_template(r#"status: {"code":200,"body":"ok"}"#);
    assert!(t.contains("<json>"), "expected <json> in: {t}");
    assert!(
        !t.contains("200"),
        "raw number inside JSON should not be visible: {t}"
    );
}

#[test]
fn quoted_string_replaced() {
    let t = normalize_template(r#"error loading file "config-abc123.toml""#);
    assert!(t.contains("<str>"), "expected <str> in: {t}");
}

#[test]
fn linux_path_replaced() {
    let t = normalize_template("failed to open /var/log/syslog: No such file");
    assert!(t.contains("<path>"), "expected <path> in: {t}");
    assert!(
        !t.contains("/var/log/syslog"),
        "path should be replaced: {t}"
    );
}
