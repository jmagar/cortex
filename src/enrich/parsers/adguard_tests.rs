use crate::enrich::{Parser, ParserInput, SourceKind};

fn input_from(fixture: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/parsers/adguard/{fixture}"))
        .unwrap()
        .trim()
        .to_string()
}

fn parse(
    message: &str,
    source_kind: SourceKind,
) -> Result<crate::enrich::ParserOutput, crate::enrich::ParserError> {
    super::AdguardParser.parse(ParserInput {
        app_name: Some("adguard-query"),
        container_name: None,
        message,
        raw: message,
        source_kind,
        severity: "info",
    })
}

#[test]
fn block_marks_dns_blocked_true() {
    let out = parse(&input_from("block.json"), SourceKind::DockerStream).unwrap();
    assert_eq!(out.dns_blocked, Some(true));
    assert_eq!(out.event_action.as_deref(), Some("dns_query"));
    assert_eq!(out.metadata["query"], serde_json::json!("doubleclick.net"));
    assert_eq!(out.metadata["qtype"], serde_json::json!("A"));
    assert_eq!(out.metadata["client"], serde_json::json!("192.168.10.55"));
    assert_eq!(
        out.metadata["reason"],
        serde_json::json!("FilteredBlackList")
    );
    assert_eq!(
        out.metadata["rule"],
        serde_json::json!("||doubleclick.net^")
    );
}

#[test]
fn allow_marks_dns_blocked_false() {
    let out = parse(&input_from("allow.json"), SourceKind::DockerStream).unwrap();
    assert_eq!(out.dns_blocked, Some(false));
}

#[test]
fn rewrite_marks_dns_blocked_null() {
    let out = parse(&input_from("rewrite.json"), SourceKind::DockerStream).unwrap();
    assert_eq!(out.dns_blocked, None);
    assert_eq!(out.metadata["reason"], serde_json::json!("Rewrite"));
}

#[test]
fn cached_hit() {
    let out = parse(&input_from("cached_hit.json"), SourceKind::DockerStream).unwrap();
    assert_eq!(out.metadata["cached"], serde_json::json!(true));
}

#[test]
fn legacy_camelcase_falls_back() {
    let out = parse(
        &input_from("legacy_camelcase.json"),
        SourceKind::DockerStream,
    )
    .unwrap();
    assert_eq!(out.metadata["query"], serde_json::json!("example.com"));
    assert_eq!(out.dns_blocked, Some(false));
}

#[test]
fn api_poller_path_yields_identical_output() {
    let from_docker = parse(&input_from("block.json"), SourceKind::DockerStream).unwrap();
    let from_api = parse(
        &input_from("api_poller_normalised.json"),
        SourceKind::AdguardApi,
    )
    .unwrap();
    assert_eq!(from_docker.dns_blocked, from_api.dns_blocked);
    assert_eq!(from_docker.metadata, from_api.metadata);
}

#[test]
fn truncated_invalid_returns_json_error() {
    let err = parse(
        &input_from("truncated_invalid.txt"),
        SourceKind::DockerStream,
    )
    .unwrap_err();
    assert!(matches!(err, crate::enrich::ParserError::Json(_)));
}
