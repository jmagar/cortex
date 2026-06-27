use super::*;
use cortex::scanner::{AiDoctorReport, TranscriptRootStatus};

#[test]
fn smoke_watch_target_prefers_writable_claude_root() {
    let dir = tempfile::tempdir().unwrap();
    let doctor = doctor_with_roots(
        dir.path().display().to_string(),
        "/missing/codex".to_string(),
    );

    let target = smoke_watch_target(&doctor, "stamp", "session-1", "2026-01-01T00:00:00Z").unwrap();

    assert_eq!(target.tool, "claude");
    assert!(
        target
            .transcript_path
            .ends_with("syslog-smoke-watch-stamp.jsonl")
    );
    assert!(target.body.contains("session-1"));
}

fn doctor_with_roots(claude_path: String, codex_path: String) -> AiDoctorReport {
    AiDoctorReport {
        db_path: "cortex.db".to_string(),
        db_schema_version: 1,
        db_last_migration_at: None,
        known_schema_version: 1,
        schema_current: true,
        claude_root: root_status(claude_path, true),
        codex_root: root_status(codex_path, false),
        gemini_root: root_status("/missing/gemini".to_string(), false),
        checkpoint_count: 0,
        checkpoint_error_count: 0,
        missing_checkpoint_count: 0,
        imported_record_count: 0,
        parse_error_count: 0,
        newest_indexed_path: None,
        newest_indexed_at: None,
    }
}

fn root_status(path: String, usable: bool) -> TranscriptRootStatus {
    TranscriptRootStatus {
        path,
        exists: usable,
        readable: usable,
        writable: usable,
        owner_uid: None,
        owner_gid: None,
        mode: None,
        strict_ok: usable,
    }
}
