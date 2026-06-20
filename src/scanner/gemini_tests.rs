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

    let records = parse_file(raw, Path::new("session-2026-04-02T22-02-da13.json")).unwrap();

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
