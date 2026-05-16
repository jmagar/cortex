use crate::enrich::{Parser, ParserInput, SourceKind};

fn input_from(fixture: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/parsers/swag/{fixture}"))
        .unwrap()
        .trim()
        .to_string()
}

fn parse(message: &str) -> Result<crate::enrich::ParserOutput, crate::enrich::ParserError> {
    super::SwagParser.parse(ParserInput {
        app_name: Some("swag"),
        container_name: Some("swag"),
        message,
        raw: message,
        source_kind: SourceKind::DockerStream,
        severity: "info",
    })
}

#[test]
fn access_combined_401() {
    let out = parse(&input_from("access_combined.txt")).unwrap();
    assert_eq!(out.http_status, Some(401));
    assert_eq!(out.event_action.as_deref(), Some("http_request"));
    assert_eq!(out.metadata["method"], serde_json::json!("POST"));
    assert_eq!(out.metadata["path"], serde_json::json!("/login"));
    assert_eq!(out.metadata["client_ip"], serde_json::json!("192.0.2.55"));
    assert_eq!(out.metadata["bytes_sent"], serde_json::json!(87_i64));
}

#[test]
fn access_combined_upstream_extracts_latency_and_xff() {
    let out = parse(&input_from("access_combined_upstream.txt")).unwrap();
    assert_eq!(out.http_status, Some(200));
    assert_eq!(
        out.metadata["forwarded_for"],
        serde_json::json!("203.0.113.7")
    );
    assert_eq!(out.metadata["latency_ms"], serde_json::json!(41_i32));
}

#[test]
fn access_ipv6_client() {
    let out = parse(&input_from("access_ipv6.txt")).unwrap();
    assert_eq!(out.http_status, Some(200));
    assert_eq!(
        out.metadata["client_ip"],
        serde_json::json!("2001:db8::1")
    );
}

#[test]
fn access_escaped_quote_in_path() {
    let out = parse(&input_from("access_escaped_quote.txt")).unwrap();
    assert_eq!(out.http_status, Some(200));
    let path = out.metadata["path"].as_str().unwrap();
    assert!(path.contains("x="), "path should contain query: {path}");
}

#[test]
fn error_upstream_timeout() {
    let out = parse(&input_from("error_upstream_timeout.txt")).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("upstream_error"));
    assert_eq!(
        out.metadata["upstream"],
        serde_json::json!("http://10.0.0.5:3000/")
    );
    assert_eq!(out.metadata["error_class"], serde_json::json!("timeout"));
    assert_eq!(out.severity, Some("err"));
}

#[test]
fn error_no_upstream_returns_no_match() {
    let err = parse(&input_from("error_no_upstream.txt")).unwrap_err();
    assert!(matches!(err, crate::enrich::ParserError::NoMatch(_)));
}
