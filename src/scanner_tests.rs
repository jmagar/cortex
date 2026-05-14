use super::*;
use crate::config::StorageConfig;
use crate::db::{
    init_pool, search_ai_sessions, search_logs, tail_logs, SearchAiSessionsParams, SearchParams,
};
use serial_test::serial;

fn test_pool() -> (crate::db::DbPool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    (pool, dir)
}

#[test]
fn index_file_is_idempotent() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("session.jsonl");
    std::fs::write(&file, "{\"sessionId\":\"sess-1\",\"content\":\"hello\"}\n").unwrap();

    let first = index_file(&pool, &file, "explicit_file").unwrap();
    let second = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(first.ingested, 1);
    assert_eq!(second.ingested, 0);
    assert_eq!(second.skipped_dupes, 1);
}

#[test]
fn force_reindexes_file_without_duplicate_logs() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("force.jsonl");
    std::fs::write(&file, "{\"sessionId\":\"sess-1\",\"content\":\"first\"}\n").unwrap();
    assert_eq!(
        index_file(&pool, &file, "explicit_file").unwrap().ingested,
        1
    );

    let forced = index_file_with_options(
        &pool,
        &file,
        "explicit_file",
        IndexFileOptions { force: true },
        None,
    )
    .unwrap();

    assert_eq!(forced.ingested, 1);
    assert_eq!(forced.skipped_dupes, 0);
    let log_count: i64 = pool
        .get()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(log_count, 1);
}

#[test]
fn list_checkpoints_reports_errors_and_import_counts() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("broken.jsonl");
    std::fs::write(
        &file,
        "{\"sessionId\":\"sess-1\",\"content\":\"ok\"}\nnot-json\n",
    )
    .unwrap();
    let result = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(result.parse_errors, 1);

    let checkpoints = list_checkpoints(
        &pool,
        &CheckpointListOptions {
            errors_only: true,
            missing_only: false,
            limit: Some(10),
        },
    )
    .unwrap();

    assert_eq!(checkpoints.len(), 1);
    assert_eq!(checkpoints[0].imported_records, 1);
    assert!(checkpoints[0]
        .last_error
        .as_deref()
        .unwrap()
        .contains("failed to parse"));
}

#[test]
fn prune_checkpoints_dry_run_and_delete_only_missing_sources() {
    let (pool, dir) = test_pool();
    let missing_file = dir.path().join("missing.jsonl");
    std::fs::write(
        &missing_file,
        "{\"sessionId\":\"sess-missing\",\"content\":\"gone\"}\nnot-json\n",
    )
    .unwrap();
    let present_file = dir.path().join("present.jsonl");
    std::fs::write(
        &present_file,
        "{\"sessionId\":\"sess-present\",\"content\":\"still here\"}\n",
    )
    .unwrap();

    assert_eq!(
        index_file(&pool, &missing_file, "explicit_file")
            .unwrap()
            .ingested,
        1
    );
    assert_eq!(
        index_file(&pool, &present_file, "explicit_file")
            .unwrap()
            .ingested,
        1
    );
    std::fs::remove_file(&missing_file).unwrap();

    let missing = list_checkpoints(
        &pool,
        &CheckpointListOptions {
            errors_only: false,
            missing_only: true,
            limit: Some(10),
        },
    )
    .unwrap();
    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].canonical_path,
        missing_file.display().to_string()
    );

    let dry_run = prune_checkpoints(
        &pool,
        &PruneCheckpointsOptions {
            missing_only: true,
            dry_run: true,
            limit: Some(10),
        },
    )
    .unwrap();
    assert_eq!(dry_run.matched, 1);
    assert_eq!(dry_run.pruned, 0);
    assert!(dry_run.dry_run);

    let pruned = prune_checkpoints(
        &pool,
        &PruneCheckpointsOptions {
            missing_only: true,
            dry_run: false,
            limit: Some(10),
        },
    )
    .unwrap();
    assert_eq!(pruned.matched, 1);
    assert_eq!(pruned.pruned, 1);

    let remaining = list_checkpoints(
        &pool,
        &CheckpointListOptions {
            errors_only: false,
            missing_only: false,
            limit: Some(10),
        },
    )
    .unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining[0].canonical_path,
        present_file.display().to_string()
    );
}

#[test]
fn validate_path_rejects_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let link = dir.path().join("link");
    std::fs::write(&target, "hi").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let err = validate_path(&link).unwrap_err();
    assert!(err.to_string().contains("symlinks"));
}

#[test]
fn parse_errors_are_counted_without_panicking() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("broken.jsonl");
    std::fs::write(
        &file,
        "{\"sessionId\":\"sess-1\",\"content\":\"ok\"}\nnot-json\n{\"sessionId\":\"sess-1\",\"content\":\"still ok\"}\n",
    )
    .unwrap();
    let result = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(result.ingested, 2);
    assert_eq!(result.parse_errors, 1);
    assert_eq!(result.checkpoint_updates, 0);

    let last_error: Option<String> = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT last_error FROM transcript_sources WHERE canonical_path = ?1",
            [file.canonicalize().unwrap().to_string_lossy().to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(last_error
        .unwrap()
        .contains("1 transcript record(s) failed to parse"));
}

#[test]
fn parse_error_retry_imports_only_fixed_rows_and_clears_checkpoint_error() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("repair.jsonl");
    std::fs::write(
        &file,
        "{\"sessionId\":\"sess-1\",\"content\":\"first\"}\nnot-json\n{\"sessionId\":\"sess-1\",\"content\":\"third\"}\n",
    )
    .unwrap();

    let first = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(first.ingested, 2);
    assert_eq!(first.parse_errors, 1);

    std::fs::write(
        &file,
        "{\"sessionId\":\"sess-1\",\"content\":\"first\"}\n{\"sessionId\":\"sess-1\",\"content\":\"second\"}\n{\"sessionId\":\"sess-1\",\"content\":\"third\"}\n",
    )
    .unwrap();
    let second = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(second.ingested, 1);
    assert_eq!(second.skipped_dupes, 2);
    assert_eq!(second.parse_errors, 0);
    assert_eq!(second.checkpoint_updates, 1);

    let (last_error, last_indexed_at): (Option<String>, Option<String>) = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT last_error, last_indexed_at FROM transcript_sources WHERE canonical_path = ?1",
            [file.canonicalize().unwrap().to_string_lossy().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(last_error, None);
    assert!(last_indexed_at.is_some());
}

#[test]
fn oversized_records_are_counted_without_importing() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("oversized.jsonl");
    let oversized = "x".repeat(MAX_RECORD_SIZE_BYTES + 10);
    std::fs::write(
        &file,
        format!("{{\"sessionId\":\"sess-1\",\"content\":\"{oversized}\"}}\n"),
    )
    .unwrap();

    let result = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(result.ingested, 0);
    assert_eq!(result.parse_errors, 1);
}

#[test]
fn invalid_timestamp_does_not_record_import_identity() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("bad-time.jsonl");
    std::fs::write(
        &file,
        "{\"sessionId\":\"sess-1\",\"timestamp\":\"not-a-time\",\"content\":\"bad time\"}\n",
    )
    .unwrap();

    let err = index_file(&pool, &file, "explicit_file").unwrap_err();
    assert!(err.to_string().contains("invalid transcript timestamp"));

    let conn = pool.get().unwrap();
    let import_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transcript_import_records",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let log_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(import_count, 0);
    assert_eq!(log_count, 0);
}

#[test]
fn index_file_commits_multiple_chunks_but_marks_completion_once() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("multi.jsonl");
    let mut content = String::new();
    for i in 0..(MAX_INDEX_CHUNK_RECORDS + 1) {
        content.push_str(&format!(
            "{{\"sessionId\":\"sess-1\",\"content\":\"chunk {i}\"}}\n"
        ));
    }
    std::fs::write(&file, content).unwrap();

    let result = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(result.ingested, MAX_INDEX_CHUNK_RECORDS + 1);
    assert_eq!(result.checkpoint_updates, 1);
}

#[test]
fn storage_write_block_returns_error_without_importing() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("blocked.jsonl");
    std::fs::write(
        &file,
        "{\"sessionId\":\"sess-1\",\"content\":\"blocked\"}\n",
    )
    .unwrap();
    let storage = crate::config::StorageConfig {
        db_path: dir.path().join("test.db"),
        min_free_disk_mb: u64::MAX / 1_048_576,
        recovery_free_disk_mb: u64::MAX / 1_048_576,
        ..crate::config::StorageConfig::for_test(dir.path().join("test.db"))
    };

    let result = index_file_with_storage(&pool, &file, "explicit_file", Some(&storage)).unwrap();
    assert_eq!(result.storage_blocked_chunks, 1);
    assert_eq!(result.ingested, 0);
    let log_count: i64 = pool
        .get()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(log_count, 0);
}

#[test]
fn index_file_reimports_rewritten_line_with_different_content() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("session.jsonl");
    std::fs::write(&file, "{\"sessionId\":\"sess-1\",\"content\":\"first\"}\n").unwrap();
    assert_eq!(
        index_file(&pool, &file, "explicit_file").unwrap().ingested,
        1
    );

    std::fs::write(&file, "{\"sessionId\":\"sess-1\",\"content\":\"second\"}\n").unwrap();
    let second = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(second.ingested, 1);
    assert_eq!(second.skipped_dupes, 0);
}

#[test]
fn scanner_drops_oversized_metadata_fields() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("metadata.jsonl");
    let long_project = "p".repeat(MAX_AI_PROJECT_CHARS + 1);
    let long_session = "s".repeat(MAX_AI_SESSION_ID_CHARS + 1);
    std::fs::write(
        &file,
        format!(
            "{{\"sessionId\":\"{long_session}\",\"cwd\":\"{long_project}\",\"content\":\"metadata kept\"}}\n"
        ),
    )
    .unwrap();

    let result = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(result.dropped_metadata_fields, 2);
    let row = tail_logs(&pool, None, None, None, None, 1).unwrap();
    assert_eq!(row[0].ai_project, None);
    assert_eq!(row[0].ai_session_id, None);
}

#[test]
fn session_id_is_not_used_as_record_identity() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("shared-session.jsonl");
    std::fs::write(
        &file,
        concat!(
            "{\"session\":{\"id\":\"same-session\"},\"message\":{\"content\":\"first event\"}}\n",
            "{\"session\":{\"id\":\"same-session\"},\"message\":{\"content\":\"second event\"}}\n"
        ),
    )
    .unwrap();

    let result = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(result.ingested, 2);
    assert_eq!(result.skipped_dupes, 0);

    let rows = search_logs(
        &pool,
        &SearchParams {
            query: Some("event".into()),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn committed_chunk_can_be_retried_after_later_timestamp_failure() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("retry.jsonl");
    let mut content = String::new();
    for i in 0..MAX_INDEX_CHUNK_RECORDS {
        content.push_str(&format!(
            "{{\"sessionId\":\"sess-1\",\"content\":\"chunk {i}\"}}\n"
        ));
    }
    content.push_str("{\"sessionId\":\"sess-1\",\"timestamp\":\"bad\",\"content\":\"bad\"}\n");
    std::fs::write(&file, content).unwrap();

    let err = index_file(&pool, &file, "explicit_file").unwrap_err();
    assert!(err.to_string().contains("invalid transcript timestamp"));
    let first_count: i64 = pool
        .get()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(first_count, MAX_INDEX_CHUNK_RECORDS as i64);

    let mut fixed = String::new();
    for i in 0..MAX_INDEX_CHUNK_RECORDS {
        fixed.push_str(&format!(
            "{{\"sessionId\":\"sess-1\",\"content\":\"chunk {i}\"}}\n"
        ));
    }
    fixed.push_str("{\"sessionId\":\"sess-1\",\"content\":\"fixed tail\"}\n");
    std::fs::write(&file, fixed).unwrap();

    let second = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(second.ingested, 1);
    assert_eq!(second.skipped_dupes, MAX_INDEX_CHUNK_RECORDS);
}

#[test]
fn index_file_scrubs_secrets_before_fts_storage() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("session.jsonl");
    std::fs::write(
        &file,
        "{\"sessionId\":\"sess-1\",\"content\":\"token ghp_abcdefghijklmnopqrstuvwxyzABCDEFGHIJ\"}\n",
    )
    .unwrap();

    index_file(&pool, &file, "explicit_file").unwrap();
    let rows = tail_logs(&pool, None, None, None, None, 1).unwrap();
    assert_eq!(rows[0].message, "token [REDACTED]");

    let leaked = search_logs(
        &pool,
        &SearchParams {
            query: Some("ghp".into()),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(leaked.is_empty());
}

#[test]
fn index_file_parses_realistic_codex_transcript_shape() {
    let (pool, dir) = test_pool();
    let codex_root = dir.path().join(".codex/sessions/2026/05/11");
    std::fs::create_dir_all(&codex_root).unwrap();
    let file = codex_root.join("rollout-codex-1.jsonl");
    std::fs::write(
        &file,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"codex-1\",\"cwd\":\"/home/jmagar/workspace/syslog-mcp\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"id\":\"codex-1\",\"content\":[{\"type\":\"output_text\",\"text\":\"fixed parser bug\"}]},\"timestamp\":\"2026-05-11T00:00:00Z\"}\n"
        ),
    )
    .unwrap();

    let result = index_file(&pool, &file, "codex_session").unwrap();
    assert_eq!(result.ingested, 1);
    let search = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "parser".into(),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(search.sessions.len(), 1);
    assert_eq!(search.sessions[0].ai_tool, "codex");
    assert_eq!(
        search.sessions[0].ai_project,
        "/home/jmagar/workspace/syslog-mcp"
    );
    assert_eq!(search.sessions[0].ai_session_id, "codex-1");
}

#[test]
fn index_file_uses_claude_sessions_index_project_path() {
    let (pool, dir) = test_pool();
    let project_dir = dir.path().join(".claude/projects/-tmp-fallback");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("sessions-index.json"),
        "{\"originalPath\":\"/tmp/project-with-hyphen\"}",
    )
    .unwrap();
    let file = project_dir.join("session.jsonl");
    std::fs::write(
        &file,
        "{\"sessionId\":\"claude-1\",\"content\":\"hyphen project\"}\n",
    )
    .unwrap();

    index_file(&pool, &file, "claude_project").unwrap();
    let search = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "hyphen".into(),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(search.sessions[0].ai_project, "/tmp/project-with-hyphen");
}

#[test]
fn index_roots_ignores_claude_sessions_index_metadata() {
    let (pool, dir) = test_pool();
    let project_dir = dir.path().join(".claude/projects/-tmp-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("sessions-index.json"),
        "{\n  \"version\": 1,\n  \"entries\": []\n}\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("session.jsonl"),
        "{\"sessionId\":\"claude-1\",\"content\":\"real transcript line\"}\n",
    )
    .unwrap();

    let result = index_roots(&pool, Some(dir.path())).unwrap();
    assert_eq!(result.ingested, 1);
    assert_eq!(result.parse_errors, 0);
    assert_eq!(result.skipped_files, 0);
}

#[test]
#[serial]
fn index_roots_rejects_broad_home_root_and_repo_paths() {
    let (pool, _dir) = test_pool();
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
    let repo = std::env::current_dir().unwrap();

    for path in [std::path::Path::new("/"), home.as_path(), repo.as_path()] {
        let result = index_roots(&pool, Some(path)).unwrap();
        assert_eq!(
            result.ingested,
            0,
            "unsafe path ingested rows: {}",
            path.display()
        );
        assert_eq!(
            result.skipped_unsafe_paths,
            1,
            "unsafe path not counted: {}",
            path.display()
        );
        assert_eq!(
            result.discovered_files,
            0,
            "unsafe path was scanned: {}",
            path.display()
        );
    }
}

#[test]
fn index_roots_counts_unsupported_files_without_parsing_them() {
    let (pool, dir) = test_pool();
    let transcripts = dir.path().join("transcripts");
    std::fs::create_dir(&transcripts).unwrap();
    std::fs::write(transcripts.join("notes.txt"), "not a transcript").unwrap();
    std::fs::write(
        transcripts.join("session.jsonl"),
        "{\"sessionId\":\"safe\",\"content\":\"indexed\"}\n",
    )
    .unwrap();

    let result = index_roots(&pool, Some(&transcripts)).unwrap();

    assert_eq!(result.ingested, 1);
    assert_eq!(result.unsupported_files, 1);
    assert_eq!(result.skipped_unsafe_paths, 0);
}

#[test]
fn scanner_exposes_default_roots_and_supported_file_policy() {
    let roots = default_transcript_roots();
    assert!(roots.iter().any(|path| path.ends_with(".claude/projects")));
    assert!(roots.iter().any(|path| path.ends_with(".codex/sessions")));
    assert!(is_supported_transcript_file(std::path::Path::new(
        "session.jsonl"
    )));
    assert!(!is_supported_transcript_file(std::path::Path::new(
        "session.json"
    )));
}

#[test]
fn append_only_file_indexes_only_new_records_after_checkpoint() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("session.jsonl");
    std::fs::write(
        &file,
        "{\"uuid\":\"one\",\"type\":\"user\",\"timestamp\":\"2026-05-14T00:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"first\"}}\n",
    )
    .unwrap();

    let first = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(first.ingested, 1);

    let mut open = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap();
    use std::io::Write;
    writeln!(
        open,
        "{{\"uuid\":\"two\",\"type\":\"user\",\"timestamp\":\"2026-05-14T00:00:01Z\",\"message\":{{\"role\":\"user\",\"content\":\"second\"}}}}"
    )
    .unwrap();

    let second = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(second.ingested, 1);
    assert_eq!(second.skipped_dupes, 0);
}

#[test]
fn rewritten_file_falls_back_to_duplicate_safe_full_scan() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("session.jsonl");
    std::fs::write(
        &file,
        "{\"uuid\":\"one\",\"type\":\"user\",\"timestamp\":\"2026-05-14T00:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"first\"}}\n",
    )
    .unwrap();
    assert_eq!(
        index_file(&pool, &file, "explicit_file").unwrap().ingested,
        1
    );

    std::fs::write(
        &file,
        "{\"uuid\":\"one\",\"type\":\"user\",\"timestamp\":\"2026-05-14T00:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"first changed\"}}\n",
    )
    .unwrap();

    let second = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(second.ingested, 0);
    assert!(second.skipped_dupes >= 1);
}

#[test]
#[serial]
fn index_roots_default_scans_claude_and_codex_roots() {
    let (pool, dir) = test_pool();
    let _home = HomeOverride::set(dir.path());

    let claude_root = dir.path().join(".claude/projects/-tmp-default");
    std::fs::create_dir_all(&claude_root).unwrap();
    std::fs::write(
        claude_root.join("sessions-index.json"),
        "{\"originalPath\":\"/tmp/default-claude\"}",
    )
    .unwrap();
    std::fs::write(
        claude_root.join("session.jsonl"),
        "{\"sessionId\":\"claude-default\",\"content\":\"default claude root\"}\n",
    )
    .unwrap();

    let codex_root = dir.path().join(".codex/sessions/2026/05/13");
    std::fs::create_dir_all(&codex_root).unwrap();
    std::fs::write(
        codex_root.join("rollout-codex-default.jsonl"),
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"codex-default\",\"cwd\":\"/tmp/default-codex\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"content\":[{\"text\":\"default codex root\"}]},\"timestamp\":\"2026-05-13T00:00:00Z\"}\n"
        ),
    )
    .unwrap();

    let result = index_roots(&pool, None).unwrap();

    assert_eq!(result.discovered_files, 2);
    assert_eq!(result.ingested, 2);
    let claude = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "claude".into(),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(claude.sessions[0].ai_tool, "claude");
    assert_eq!(claude.sessions[0].ai_project, "/tmp/default-claude");

    let codex = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "codex".into(),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(codex.sessions[0].ai_tool, "codex");
    assert_eq!(codex.sessions[0].ai_project, "/tmp/default-codex");
    assert_eq!(codex.sessions[0].ai_session_id, "codex-default");
}

#[test]
fn index_file_uses_session_meta_for_codex_response_items() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("rollout-session.jsonl");
    std::fs::write(
        &file,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"codex-session\",\"cwd\":\"/work/project\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"id\":\"response-item-1\",\"content\":[{\"type\":\"output_text\",\"text\":\"session context preserved\"}]},\"timestamp\":\"2026-05-11T00:00:00Z\"}\n"
        ),
    )
    .unwrap();

    let result = index_file(&pool, &file, "codex_session").unwrap();
    assert_eq!(result.ingested, 1);
    let search = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "preserved".into(),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(search.sessions[0].ai_session_id, "codex-session");
    assert_eq!(search.sessions[0].ai_project, "/work/project");
}

#[test]
fn explicit_file_detects_codex_transcript_shape_outside_codex_root() {
    let (pool, dir) = test_pool();
    let file = dir.path().join("exported-codex.jsonl");
    std::fs::write(
        &file,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"explicit-codex\",\"cwd\":\"/tmp/exported-codex\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"content\":[{\"type\":\"output_text\",\"text\":\"explicit codex imported\"}]},\"timestamp\":\"2026-05-11T00:00:00Z\"}\n"
        ),
    )
    .unwrap();

    let result = index_file(&pool, &file, "explicit_file").unwrap();
    assert_eq!(result.ingested, 1);
    let search = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "imported".into(),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(search.sessions[0].ai_tool, "codex");
    assert_eq!(search.sessions[0].ai_session_id, "explicit-codex");
    assert_eq!(search.sessions[0].ai_project, "/tmp/exported-codex");
}

#[test]
#[serial]
fn default_index_does_not_skip_same_size_rewrite_with_new_mtime_nanos() {
    let (pool, dir) = test_pool();
    let _home = HomeOverride::set(dir.path());

    let codex_root = dir.path().join(".codex/sessions/2026/05/14");
    std::fs::create_dir_all(&codex_root).unwrap();
    let file = codex_root.join("rollout-rewrite.jsonl");
    std::fs::write(
        &file,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"rewrite-session\",\"cwd\":\"/tmp/rewrite\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"content\":[{\"text\":\"alpha\"}]},\"timestamp\":\"2026-05-14T00:00:00Z\"}\n"
        ),
    )
    .unwrap();
    set_mtime(&file, 1_800_000_000, 1);

    let first = index_roots(&pool, None).unwrap();
    assert_eq!(first.ingested, 1);

    std::fs::write(
        &file,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"rewrite-session\",\"cwd\":\"/tmp/rewrite\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"content\":[{\"text\":\"bravo\"}]},\"timestamp\":\"2026-05-14T00:00:00Z\"}\n"
        ),
    )
    .unwrap();
    set_mtime(&file, 1_800_000_000, 2);

    let second = index_roots(&pool, None).unwrap();

    assert_eq!(second.ingested, 1);
    let search = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "bravo".into(),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(search.sessions[0].ai_session_id, "rewrite-session");
}

#[test]
#[serial]
fn default_index_does_not_skip_same_size_rewrite_with_preserved_mtime() {
    let (pool, dir) = test_pool();
    let _home = HomeOverride::set(dir.path());

    let codex_root = dir.path().join(".codex/sessions/2026/05/14");
    std::fs::create_dir_all(&codex_root).unwrap();
    let file = codex_root.join("rollout-rewrite-preserved-mtime.jsonl");
    std::fs::write(
        &file,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"rewrite-preserved\",\"cwd\":\"/tmp/rewrite\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"content\":[{\"text\":\"alpha\"}]},\"timestamp\":\"2026-05-14T00:00:00Z\"}\n"
        ),
    )
    .unwrap();
    set_mtime(&file, 1_800_000_001, 1);

    let first = index_roots(&pool, None).unwrap();
    assert_eq!(first.ingested, 1);

    std::fs::write(
        &file,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"rewrite-preserved\",\"cwd\":\"/tmp/rewrite\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"content\":[{\"text\":\"bravo\"}]},\"timestamp\":\"2026-05-14T00:00:00Z\"}\n"
        ),
    )
    .unwrap();
    set_mtime(&file, 1_800_000_001, 1);

    let second = index_roots(&pool, None).unwrap();

    assert_eq!(second.ingested, 1);
    let search = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "bravo".into(),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(search.sessions[0].ai_session_id, "rewrite-preserved");
}

#[test]
fn index_roots_since_skips_older_file_mtimes() {
    let (pool, dir) = test_pool();
    let old_file = dir.path().join("old.jsonl");
    let new_file = dir.path().join("new.jsonl");
    std::fs::write(
        &old_file,
        "{\"sessionId\":\"old\",\"content\":\"oldtoken\"}\n",
    )
    .unwrap();
    std::fs::write(
        &new_file,
        "{\"sessionId\":\"new\",\"content\":\"newtoken\"}\n",
    )
    .unwrap();
    set_mtime(&old_file, 1_700_000_000, 0);
    set_mtime(&new_file, 1_800_000_000, 0);

    let result = index_roots_with_options(
        &pool,
        IndexOptions {
            root_override: Some(dir.path().to_path_buf()),
            since_mtime_nanos: Some(1_750_000_000_000_000_000),
            ..Default::default()
        },
        None,
    )
    .unwrap();

    assert_eq!(result.ingested, 1);
    assert_eq!(result.skipped_files, 1);
    assert_eq!(result.discovered_files, 1);
    let search = search_ai_sessions(
        &pool,
        &SearchAiSessionsParams {
            query: "newtoken".into(),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(search.sessions[0].ai_session_id, "new");
}

fn set_mtime(path: &std::path::Path, secs: u64, nanos: u32) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::new(secs, nanos))
        .unwrap();
}

struct HomeOverride(Option<std::ffi::OsString>);

impl HomeOverride {
    fn set(path: &std::path::Path) -> Self {
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", path);
        Self(previous)
    }
}

impl Drop for HomeOverride {
    fn drop(&mut self) {
        if let Some(home) = &self.0 {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }
    }
}

#[test]
fn index_roots_reports_file_errors_with_paths() {
    let (pool, dir) = test_pool();
    let target = dir.path().join("target.jsonl");
    let link = dir.path().join("bad.jsonl");
    std::fs::write(&target, "{\"sessionId\":\"sess-1\",\"content\":\"hi\"}\n").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let result = index_roots(&pool, Some(dir.path())).unwrap();
    assert_eq!(result.skipped_files, 1);
    assert_eq!(result.skipped_symlinks, 1);
    assert_eq!(result.file_errors.len(), 1);
    assert!(result.file_errors[0].path.contains("bad.jsonl"));
    assert!(result.file_errors[0].error.contains("symlinks"));
}

#[test]
fn index_roots_skips_unreadable_directories_and_continues() {
    use std::os::unix::fs::PermissionsExt;

    let (pool, dir) = test_pool();
    let good = dir.path().join("good.jsonl");
    let blocked = dir.path().join("blocked");
    std::fs::write(
        &good,
        "{\"sessionId\":\"sess-1\",\"content\":\"rust mention\"}\n",
    )
    .unwrap();
    std::fs::create_dir(&blocked).unwrap();
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let result = index_roots(&pool, Some(dir.path()));

    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700)).unwrap();

    let result = result.expect("unreadable directories should be skipped, not abort indexing");
    assert_eq!(result.ingested, 1);
    assert_eq!(result.skipped_files, 1);
    assert_eq!(result.file_errors.len(), 1);
    assert!(result.file_errors[0].error.contains("Permission denied"));
}
