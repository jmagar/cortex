use crate::enrich::{Parser, ParserInput, SourceKind};

fn input_from(fixture: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/parsers/fail2ban/{fixture}"))
        .unwrap()
        .trim()
        .to_string()
}

fn parse(message: &str) -> Result<crate::enrich::ParserOutput, crate::enrich::ParserError> {
    super::Fail2banParser.parse(ParserInput {
        app_name: Some("fail2ban"),
        container_name: None,
        message,
        raw: message,
        source_kind: SourceKind::SyslogTcp,
        severity: "notice",
    })
}

#[test]
fn ban() {
    let out = parse(&input_from("ban.txt")).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("ban"));
    assert_eq!(out.metadata["jail"], serde_json::json!("sshd"));
    assert_eq!(out.metadata["banned_ip"], serde_json::json!("203.0.113.7"));
    assert_eq!(out.severity, Some("warning"));
}

#[test]
fn unban() {
    let out = parse(&input_from("unban.txt")).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("unban"));
    assert_eq!(out.metadata["jail"], serde_json::json!("sshd"));
}

#[test]
fn found() {
    let out = parse(&input_from("found.txt")).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("found"));
    assert_eq!(out.metadata["banned_ip"], serde_json::json!("203.0.113.7"));
}

#[test]
fn restore_ban_different_jail() {
    let out = parse(&input_from("restore_ban.txt")).unwrap();
    assert_eq!(out.event_action.as_deref(), Some("restore_ban"));
    assert_eq!(out.metadata["jail"], serde_json::json!("authelia"));
    assert_eq!(out.metadata["banned_ip"], serde_json::json!("198.51.100.4"));
}

#[test]
fn multi_ip_ban_first_in_banned_ip() {
    let out = parse(&input_from("multi_ip_ban.txt")).unwrap();
    assert_eq!(out.metadata["banned_ip"], serde_json::json!("1.2.3.4"));
    let all = out.metadata["all_ips"].as_array().unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn error_line_no_match() {
    let err = parse(&input_from("error_line.txt")).unwrap_err();
    assert!(matches!(err, crate::enrich::ParserError::NoMatch(_)));
}
