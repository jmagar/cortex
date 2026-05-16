use crate::enrich::{AuthOutcome, Parser, ParserInput, SourceKind};

fn input_from(fixture: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/parsers/authelia/{fixture}"))
        .unwrap()
        .trim()
        .to_string()
}

fn parse(message: &str) -> Result<crate::enrich::ParserOutput, crate::enrich::ParserError> {
    super::AutheliaParser.parse(ParserInput {
        app_name: Some("authelia"),
        container_name: Some("authelia"),
        message,
        raw: message,
        source_kind: SourceKind::DockerStream,
        severity: "info",
    })
}

#[test]
fn ffa_success() {
    let out = parse(&input_from("1fa_success.json")).unwrap();
    assert_eq!(out.auth_outcome, Some(AuthOutcome::Success));
    assert_eq!(out.metadata["username"], serde_json::json!("alice"));
    assert_eq!(out.metadata["mfa_method"], serde_json::json!("1fa"));
    assert_eq!(out.metadata["src_ip"], serde_json::json!("100.0.0.1"));
}

#[test]
fn ffa_failure() {
    let out = parse(&input_from("1fa_failure.json")).unwrap();
    assert_eq!(out.auth_outcome, Some(AuthOutcome::Failure));
    assert_eq!(out.metadata["username"], serde_json::json!("bob"));
    assert_eq!(out.severity, Some("err"));
}

#[test]
fn totp_success() {
    let out = parse(&input_from("totp_success.json")).unwrap();
    assert_eq!(out.auth_outcome, Some(AuthOutcome::Success));
    assert_eq!(out.metadata["mfa_method"], serde_json::json!("totp"));
}

#[test]
fn totp_failure_warning_severity() {
    let out = parse(&input_from("totp_failure.json")).unwrap();
    assert_eq!(out.auth_outcome, Some(AuthOutcome::Failure));
    assert_eq!(out.severity, Some("warning"));
}

#[test]
fn health_probe_no_auth_outcome() {
    let out = parse(&input_from("health_probe.json")).unwrap();
    assert_eq!(out.auth_outcome, None);
    assert_eq!(out.metadata["path"], serde_json::json!("/api/health"));
}

#[test]
fn text_mode_legacy_returns_structural_error() {
    let out = parse(&input_from("text_mode_legacy.txt"));
    assert!(matches!(
        out,
        Err(crate::enrich::ParserError::Structural(_))
    ));
}
