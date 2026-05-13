use std::path::Path;

use super::*;

#[test]
fn parse_line_extracts_top_level_content_and_session_id() {
    let line = r#"{"sessionId":"claude-1","timestamp":"2026-05-11T00:00:00Z","content":"hello"}"#;

    let parsed = parse_line(line, Path::new("/tmp/session.jsonl"), 0)
        .unwrap()
        .expect("content should produce a transcript record");

    assert_eq!(parsed.message, "hello");
    assert_eq!(parsed.session_id.as_deref(), Some("claude-1"));
    assert_eq!(parsed.timestamp.as_deref(), Some("2026-05-11T00:00:00Z"));
    assert!(parsed.record_key.starts_with("hash:"));
    assert!(parsed.ai_project.is_none());
}

#[test]
fn parse_line_extracts_nested_message_content() {
    let line = r#"{"session":{"id":"nested-1"},"message":{"content":"nested text"}}"#;

    let parsed = parse_line(line, Path::new("/tmp/session.jsonl"), 0)
        .unwrap()
        .expect("nested message content should produce a transcript record");

    assert_eq!(parsed.message, "nested text");
    assert_eq!(parsed.session_id.as_deref(), Some("nested-1"));
}

#[test]
fn parse_line_joins_string_content_arrays() {
    let line = r#"{"session_id":"claude-array","content":["first","second",{"ignored":true}]}"#;

    let parsed = parse_line(line, Path::new("/tmp/session.jsonl"), 0)
        .unwrap()
        .expect("string array content should produce a transcript record");

    assert_eq!(parsed.message, "first second");
    assert_eq!(parsed.session_id.as_deref(), Some("claude-array"));
}

#[test]
fn parse_line_falls_back_to_path_as_session_id() {
    let path = Path::new("/tmp/no-session.jsonl");
    let line = r#"{"content":"hello without session"}"#;

    let parsed = parse_line(line, path, 0)
        .unwrap()
        .expect("content should produce a transcript record");

    assert_eq!(parsed.session_id.as_deref(), Some("/tmp/no-session.jsonl"));
}

#[test]
fn parse_line_ignores_records_without_message_content() {
    let line = r#"{"sessionId":"claude-1","timestamp":"2026-05-11T00:00:00Z"}"#;

    let parsed = parse_line(line, Path::new("/tmp/session.jsonl"), 0).unwrap();

    assert!(parsed.is_none());
}
