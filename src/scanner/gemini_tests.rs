use std::path::Path;

use super::*;

#[test]
fn parse_file_extracts_messages_from_gemini_chat_json() {
    let raw = r#"{
      "sessionId": "gemini-session",
      "projectHash": "abc123",
      "startTime": "2026-04-02T22:02:55.537Z",
      "messages": [
        {
          "id": "msg-1",
          "timestamp": "2026-04-02T22:03:23.324Z",
          "type": "user",
          "content": "hello gemini"
        },
        {
          "id": "msg-2",
          "timestamp": "2026-04-02T22:03:29.818Z",
          "type": "gemini",
          "content": "hello human",
          "thoughts": [{"description": "not indexed separately"}]
        }
      ]
    }"#;

    let parsed = parse_file(raw, Path::new("session-2026-04-02T22-02-da13.json")).unwrap();

    assert!(!parsed.missing_messages);
    assert_eq!(parsed.skipped_empty, 0);
    let records = parsed.records;
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].record_key, "id:msg-1");
    assert_eq!(records[0].message, "hello gemini");
    assert_eq!(records[0].session_id.as_deref(), Some("gemini-session"));
    assert_eq!(
        records[0].ai_project.as_deref(),
        Some("gemini://project/abc123")
    );
    assert_eq!(
        records[0].timestamp.as_deref(),
        Some("2026-04-02T22:03:23.324Z")
    );
    assert_eq!(records[1].record_key, "id:msg-2");
    assert_eq!(records[1].message, "hello human");
}

#[test]
fn parse_file_joins_array_content_and_handles_fallbacks() {
    let raw = r#"{
      "sessionId": "s",
      "messages": [
        {"id": "a", "content": [{"text": "a"}, {"content": "b"}, "c"]},
        {"id": "b", "message": "fallback text"},
        {"id": "c", "content": ""},
        {"id": "d", "type": "tool", "payload": {"unrecognized": true}}
      ]
    }"#;

    let parsed = parse_file(raw, Path::new("session-x.json")).unwrap();

    // Array content joined with spaces; scalar `message` used as a fallback.
    assert_eq!(parsed.records.len(), 2);
    assert_eq!(parsed.records[0].message, "a b c");
    assert_eq!(parsed.records[1].message, "fallback text");
    // Empty content and an unrecognized shape are both skipped — and counted,
    // not silently swallowed.
    assert_eq!(parsed.skipped_empty, 2);
    assert!(!parsed.missing_messages);
}

#[test]
fn parse_file_uses_index_hash_record_key_when_id_missing() {
    let raw = r#"{
      "sessionId": "s",
      "messages": [
        {"content": "first", "timestamp": "2026-04-02T22:03:23.324Z"},
        {"content": "second"}
      ]
    }"#;

    let parsed = parse_file(raw, Path::new("session-x.json")).unwrap();

    assert_eq!(parsed.records.len(), 2);
    assert!(
        parsed.records[0].record_key.starts_with("message:0:hash:"),
        "keyless messages must derive a stable index+hash record key: {}",
        parsed.records[0].record_key
    );
    assert!(parsed.records[1].record_key.starts_with("message:1:hash:"));
    // Distinct positions/content yield distinct keys (dedup stability).
    assert_ne!(parsed.records[0].record_key, parsed.records[1].record_key);
}

#[test]
fn parse_file_flags_missing_messages_array() {
    // A chat file with no `messages` key is a likely schema change, not an
    // empty session — it must be flagged so it is not checkpointed as clean.
    let missing = parse_file(r#"{"sessionId": "s"}"#, Path::new("session-x.json")).unwrap();
    assert!(missing.missing_messages);
    assert!(missing.records.is_empty());

    // An explicitly empty array is a legitimately-empty session, not drift.
    let empty = parse_file(
        r#"{"sessionId": "s", "messages": []}"#,
        Path::new("session-x.json"),
    )
    .unwrap();
    assert!(!empty.missing_messages);
    assert!(empty.records.is_empty());
    assert_eq!(empty.skipped_empty, 0);
}

#[test]
fn parse_file_falls_back_to_start_time_when_message_timestamp_missing() {
    let raw = r#"{
      "sessionId": "s",
      "startTime": "2026-04-02T22:02:55.537Z",
      "messages": [
        {"id": "m", "content": "no per-message timestamp"}
      ]
    }"#;

    let parsed = parse_file(raw, Path::new("session-x.json")).unwrap();
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(
        parsed.records[0].timestamp.as_deref(),
        Some("2026-04-02T22:02:55.537Z")
    );
}

#[test]
fn is_chat_file_matches_gemini_session_chat_path() {
    assert!(is_chat_file(Path::new(
        "/home/jmagar/.gemini/tmp/hash/chats/session-2026-04-02T22-02-da13.json"
    )));
    assert!(!is_chat_file(Path::new(
        "/home/jmagar/.gemini/tmp/hash/chats/notes.json"
    )));
    assert!(!is_chat_file(Path::new(
        "/home/jmagar/.gemini/tmp/hash/other/session-2026-04-02T22-02-da13.json"
    )));
}
