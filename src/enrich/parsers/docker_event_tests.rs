use crate::enrich::{Parser, ParserInput, SourceKind};

fn input_from(fixture: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/parsers/docker_event/{fixture}"))
        .unwrap()
        .trim()
        .to_string()
}

fn parse(message: &str) -> Result<crate::enrich::ParserOutput, crate::enrich::ParserError> {
    super::DockerEventParser.parse(ParserInput {
        app_name: Some("dockerd"),
        container_name: None,
        message,
        raw: message,
        source_kind: SourceKind::DockerEvent,
        severity: "info",
    })
}

#[test]
fn die_extracts_exit_code_and_severity() {
    let out = parse(&input_from("die.txt")).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("die"));
    assert_eq!(out.metadata["container_name"], serde_json::json!("postgres"));
    assert_eq!(out.metadata["image"], serde_json::json!("postgres:16"));
    assert_eq!(out.metadata["exit_code"], serde_json::json!(137_i32));
    assert_eq!(out.severity, Some("warning"));
}

#[test]
fn oom_promotes_severity_to_crit() {
    let out = parse(&input_from("oom.txt")).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("oom"));
    assert_eq!(out.severity, Some("crit"));
}

#[test]
fn start_is_info_severity() {
    let out = parse(&input_from("start.txt")).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("start"));
    assert_eq!(out.severity, None);
}

#[test]
fn health_unhealthy_normalised() {
    let out = parse(&input_from("health_unhealthy.txt")).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("health_status_unhealthy"));
    assert_eq!(out.severity, Some("warning"));
}

#[test]
fn rename_captures_old_name() {
    let out = parse(&input_from("rename.txt")).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("rename"));
    assert_eq!(out.metadata["old_name"], serde_json::json!("nginx-proxy"));
}
