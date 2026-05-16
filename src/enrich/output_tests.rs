use crate::db::LogBatchEntry;
use crate::enrich::{AuthOutcome, ParserOutput, SourceKind};

fn blank_entry() -> LogBatchEntry {
    LogBatchEntry {
        timestamp: String::new(),
        hostname: String::new(),
        facility: None,
        severity: "info".into(),
        app_name: None,
        process_id: None,
        message: String::new(),
        raw: String::new(),
        source_ip: String::new(),
        docker_checkpoint: None,
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
fn merges_indexed_columns() {
    let mut entry = blank_entry();
    let out = ParserOutput {
        http_status: Some(404),
        auth_outcome: Some(AuthOutcome::Failure),
        dns_blocked: Some(true),
        event_action: Some("http_request".into()),
        severity: Some("err"),
        metadata: Default::default(),
    };
    super::merge_output(&mut entry, "swag", out);
    assert_eq!(entry.http_status, Some(404));
    assert_eq!(entry.auth_outcome, Some("failure"));
    assert_eq!(entry.dns_blocked, Some(true));
    assert_eq!(entry.event_action.as_deref(), Some("http_request"));
    assert_eq!(entry.severity, "err");
}

#[test]
fn merges_metadata_under_namespace() {
    let mut entry = blank_entry();
    let mut meta = serde_json::Map::new();
    meta.insert("method".into(), serde_json::json!("GET"));
    meta.insert("path".into(), serde_json::json!("/api"));
    let out = ParserOutput {
        metadata: meta,
        ..Default::default()
    };
    super::merge_output(&mut entry, "swag", out);

    let parsed: serde_json::Value =
        serde_json::from_str(entry.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(parsed["swag"]["method"], serde_json::json!("GET"));
    assert_eq!(parsed["swag"]["path"], serde_json::json!("/api"));
    assert_eq!(parsed["parser"]["name"], serde_json::json!("swag"));
}

#[test]
fn preserves_existing_metadata_namespaces() {
    let mut entry = blank_entry();
    entry.metadata_json = Some(r#"{"docker":{"container_name":"swag"}}"#.into());
    let mut meta = serde_json::Map::new();
    meta.insert("method".into(), serde_json::json!("GET"));
    let out = ParserOutput {
        metadata: meta,
        ..Default::default()
    };
    super::merge_output(&mut entry, "swag", out);

    let parsed: serde_json::Value =
        serde_json::from_str(entry.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(
        parsed["docker"]["container_name"],
        serde_json::json!("swag")
    );
    assert_eq!(parsed["swag"]["method"], serde_json::json!("GET"));
}

#[test]
fn record_error_writes_parse_error_truncated() {
    let mut entry = blank_entry();
    let long = "x".repeat(1000);
    super::record_error(&mut entry, "swag", &format!("structural: {long}"));
    let pe = entry.parse_error.unwrap();
    assert!(pe.starts_with("swag: structural: "));
    assert!(pe.len() <= 512);
}

#[test]
fn stamps_source_kind_in_metadata() {
    let mut entry = blank_entry();
    super::stamp_source_kind(&mut entry, SourceKind::DockerStream);
    let parsed: serde_json::Value =
        serde_json::from_str(entry.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(parsed["source_kind"], serde_json::json!("docker-stream"));
}

#[test]
fn stamps_source_kind_idempotent() {
    let mut entry = blank_entry();
    entry.metadata_json = Some(r#"{"source_kind":"docker-event"}"#.into());
    super::stamp_source_kind(&mut entry, SourceKind::SyslogUdp);
    let parsed: serde_json::Value =
        serde_json::from_str(entry.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(parsed["source_kind"], serde_json::json!("docker-event"));
}
