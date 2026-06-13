use super::*;
use cortex::scanner::{IndexFileError, TranscriptRootStatus};

#[test]
fn checkpoint_json_output_is_accepted_for_empty_response() {
    print_checkpoints_response(&[], true).unwrap();
}

#[test]
fn parse_error_json_output_is_accepted_for_empty_response() {
    print_ai_parse_errors_response(&[], true).unwrap();
}

fn root_status(exists: bool, strict_ok: bool) -> TranscriptRootStatus {
    TranscriptRootStatus {
        path: "/tmp/transcripts".to_string(),
        exists,
        readable: exists,
        writable: exists,
        owner_uid: Some(1000),
        owner_gid: Some(1000),
        mode: Some(0o700),
        strict_ok,
    }
}

fn ai_doctor_report(
    claude_root: TranscriptRootStatus,
    codex_root: TranscriptRootStatus,
) -> AiDoctorReport {
    AiDoctorReport {
        db_path: "/tmp/cortex.db".to_string(),
        db_schema_version: 1,
        db_last_migration_at: None,
        known_schema_version: 1,
        schema_current: true,
        claude_root,
        codex_root,
        checkpoint_count: 0,
        checkpoint_error_count: 0,
        missing_checkpoint_count: 0,
        imported_record_count: 0,
        parse_error_count: 0,
        newest_indexed_path: None,
        newest_indexed_at: None,
    }
}

#[test]
fn ensure_index_success_accepts_clean_result_and_dropped_metadata_warning() {
    let clean = IndexResult::default();
    ensure_index_success(&clean).unwrap();

    let mut dropped_metadata = IndexResult::default();
    dropped_metadata.dropped_metadata_fields = 2;
    ensure_index_success(&dropped_metadata).unwrap();
}

#[test]
fn ensure_index_success_reports_storage_parse_and_file_failures() {
    let mut storage_blocked = IndexResult::default();
    storage_blocked.storage_blocked_chunks = 2;
    let err = ensure_index_success(&storage_blocked).unwrap_err();
    assert!(err.to_string().contains("blocked by storage guardrails"));

    let mut parse_errors = IndexResult::default();
    parse_errors.parse_errors = 3;
    let err = ensure_index_success(&parse_errors).unwrap_err();
    assert!(err.to_string().contains("failed to parse"));

    let mut file_errors = IndexResult::default();
    file_errors.file_errors.push(IndexFileError {
        path: "/tmp/bad.jsonl".to_string(),
        error: "permission denied".to_string(),
    });
    let err = ensure_index_success(&file_errors).unwrap_err();
    assert!(err.to_string().contains("failed to index"));
}

#[test]
fn ensure_ai_doctor_success_enforces_strict_permissions_only_for_existing_roots() {
    let strict_failure = ai_doctor_report(root_status(true, false), root_status(false, false));
    let err = ensure_ai_doctor_success(&strict_failure, true).unwrap_err();
    assert!(
        err.to_string()
            .contains("AI transcript root permission check failed")
    );

    ensure_ai_doctor_success(&strict_failure, false).unwrap();

    let missing_roots = ai_doctor_report(root_status(false, false), root_status(false, false));
    ensure_ai_doctor_success(&missing_roots, true).unwrap();
}
