use crate::enrich::{Parser, ParserInput, SourceKind};

fn input_from(fixture: &str) -> String {
    let path = format!("tests/fixtures/parsers/kernel/{fixture}");
    std::fs::read_to_string(&path)
        .expect(&path)
        .trim()
        .to_string()
}

fn parse(message: &str) -> Result<crate::enrich::ParserOutput, crate::enrich::ParserError> {
    let parser = super::KernelParser;
    let input = ParserInput {
        app_name: Some("kernel"),
        container_name: None,
        message,
        raw: message,
        source_kind: SourceKind::SyslogTcp,
        severity: "info",
    };
    parser.parse(input)
}

#[test]
fn oom_kill_extracts_fields() {
    let msg = input_from("oom_killed.txt");
    let out = parse(&msg).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("oom_kill"));
    assert_eq!(out.metadata["pid"], serde_json::json!(2475067_i64));
    assert_eq!(out.metadata["comm"], serde_json::json!("postgres"));
    assert_eq!(out.metadata["total_vm_kb"], serde_json::json!(2484556_i64));
    assert_eq!(out.metadata["anon_rss_kb"], serde_json::json!(143224_i64));
    assert_eq!(out.metadata["uid"], serde_json::json!(1011_i32));
    assert_eq!(out.metadata["oom_score_adj"], serde_json::json!(900_i32));
}

#[test]
fn link_up_extracts_speed() {
    let msg = input_from("link_up.txt");
    let out = parse(&msg).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("link_up"));
    assert_eq!(out.metadata["interface"], serde_json::json!("eth0"));
    assert_eq!(out.metadata["state"], serde_json::json!("up"));
    assert_eq!(out.metadata["speed_mbps"], serde_json::json!(1000_i32));
}

#[test]
fn link_down_no_speed() {
    let msg = input_from("link_down.txt");
    let out = parse(&msg).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("link_down"));
    assert_eq!(out.metadata["interface"], serde_json::json!("eth0"));
    assert!(out.metadata.get("speed_mbps").is_none());
}

#[test]
fn mac_collision_extracts_mac() {
    let msg = input_from("mac_collision.txt");
    let out = parse(&msg).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("mac_collision"));
    assert_eq!(out.metadata["interface"], serde_json::json!("br0"));
    assert_eq!(
        out.metadata["colliding_mac"],
        serde_json::json!("aa:bb:cc:dd:ee:ff")
    );
    assert_eq!(out.metadata["vlan"], serde_json::json!(0_i32));
}

#[test]
fn unknown_kernel_message_returns_no_match() {
    let msg = input_from("unknown_kern.txt");
    let err = parse(&msg).unwrap_err();
    assert!(matches!(err, crate::enrich::ParserError::NoMatch(_)));
}
