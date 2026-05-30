use super::*;

#[test]
fn truncate_is_utf8_safe_and_preserves_short_strings() {
    assert_eq!(truncate("short", 10), "short");
    assert_eq!(truncate("éclair", 4), "écl…");
    assert_eq!(truncate("anything", 0), "");
}

#[test]
fn transcript_detection_accepts_source_ip_and_app_suffix() {
    let mut log = cortex::app::LogEntry {
        id: 1,
        timestamp: "2026-01-01T00:00:00Z".to_string(),
        received_at: "2026-01-01T00:00:00Z".to_string(),
        hostname: "host".to_string(),
        source_ip: "transcript://session".to_string(),
        facility: Some("user".to_string()),
        severity: "info".to_string(),
        app_name: None,
        process_id: None,
        message: "hello".to_string(),
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        ai_transcript_path: None,
        metadata_json: None,
    };
    assert!(is_transcript_log(&log));

    log.source_ip = "127.0.0.1".to_string();
    log.app_name = Some("codex-transcript".to_string());
    assert!(is_transcript_log(&log));
}
