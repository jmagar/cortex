use std::path::Path;

use super::*;

#[test]
fn parse_line_extracts_payload_content_items_and_project_from_arguments() {
    let line = r#"{"type":"response_item","payload":{"id":"item-1","content":[{"type":"output_text","text":"fixed parser"},{"content":"added test"}],"arguments":"{\"workdir\":\"/home/jmagar/workspace/syslog-mcp\"}","timestamp":"2026-05-11T00:00:00Z"}}"#;

    let parsed = parse_line(line, Path::new("/tmp/rollout-test.jsonl"), 0)
        .unwrap()
        .expect("content should produce a transcript record");

    assert_eq!(parsed.message, "fixed parser added test");
    assert_eq!(parsed.session_id.as_deref(), Some("item-1"));
    assert_eq!(parsed.timestamp.as_deref(), Some("2026-05-11T00:00:00Z"));
    assert_eq!(
        parsed.ai_project.as_deref(),
        Some("/home/jmagar/workspace/syslog-mcp")
    );
    assert_eq!(parsed.record_key, "id:item-1");
}

#[test]
fn parse_line_uses_file_stem_when_session_id_is_missing() {
    let line = r#"{"timestamp":"2026-05-11T00:00:00Z","payload":{"text":"standalone text"}}"#;

    let parsed = parse_line(line, Path::new("/tmp/rollout-codex-123.jsonl"), 0)
        .unwrap()
        .expect("payload text should produce a transcript record");

    assert_eq!(parsed.message, "standalone text");
    assert_eq!(parsed.session_id.as_deref(), Some("rollout-codex-123"));
    assert!(parsed.record_key.starts_with("hash:"));
}

#[test]
fn parse_line_ignores_records_without_message_content() {
    let line = r#"{"type":"session_meta","payload":{"id":"codex-1","cwd":"/tmp/project"}}"#;

    let parsed = parse_line(line, Path::new("/tmp/rollout.jsonl"), 0).unwrap();

    assert!(parsed.is_none());
}

#[test]
fn project_from_line_reads_turn_context_cwd() {
    let line = r#"{"turn_context":{"cwd":"/tmp/from-turn-context"},"content":"hello"}"#;

    assert_eq!(
        project_from_line(line).as_deref(),
        Some("/tmp/from-turn-context")
    );
}
