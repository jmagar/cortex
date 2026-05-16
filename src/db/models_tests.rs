use super::*;

#[test]
fn log_batch_entry_keeps_claimed_hostname_separate_from_source_ip() {
    let entry = LogBatchEntry {
        timestamp: "2026-01-01T00:00:00Z".to_string(),
        hostname: "claimed-host".to_string(),
        facility: Some("local0".to_string()),
        severity: "info".to_string(),
        app_name: Some("app".to_string()),
        process_id: Some("123".to_string()),
        message: "message".to_string(),
        raw: "raw".to_string(),
        source_ip: "192.0.2.10:514".to_string(),
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
    };

    assert_eq!(entry.hostname, "claimed-host");
    assert_eq!(entry.source_ip, "192.0.2.10:514");
}

#[test]
fn log_batch_entry_has_enrichment_fields() {
    let entry = super::LogBatchEntry {
        timestamp: String::new(),
        hostname: String::new(),
        facility: None,
        severity: String::new(),
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
    };
    assert!(entry.http_status.is_none());
    assert!(entry.auth_outcome.is_none());
    assert!(entry.dns_blocked.is_none());
    assert!(entry.event_action.is_none());
    assert!(entry.parse_error.is_none());
}
