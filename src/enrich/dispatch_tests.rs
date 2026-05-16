use crate::db::LogBatchEntry;
use crate::enrich::EnrichmentPipeline;

fn fixture_entry() -> LogBatchEntry {
    LogBatchEntry {
        timestamp: "2026-05-16T10:00:00Z".into(),
        hostname: "h".into(),
        facility: None,
        severity: "info".into(),
        app_name: Some("kernel".into()),
        process_id: None,
        message: "hello".into(),
        raw: "hello".into(),
        source_ip: "udp://127.0.0.1:5678".into(),
        docker_checkpoint: None,
        ai_tool: None, ai_project: None, ai_session_id: None, ai_transcript_path: None,
        metadata_json: None,
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    }
}

#[test]
fn empty_pipeline_leaves_entry_unchanged() {
    let pipeline = EnrichmentPipeline::new();
    let mut entry = fixture_entry();
    pipeline.dispatch(&mut entry);
    assert!(entry.http_status.is_none());
    assert!(entry.event_action.is_none());
    assert!(entry.parse_error.is_none());
    assert!(entry.metadata_json.is_none());
}
