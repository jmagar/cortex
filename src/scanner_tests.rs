use super::*;
use crate::config::StorageConfig;
use crate::db::{
    init_pool, search_ai_sessions, search_logs, tail_logs, SearchAiSessionsParams, SearchParams,
};

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
fn index_roots_reports_file_errors_with_paths() {
    let (pool, dir) = test_pool();
    let target = dir.path().join("target.jsonl");
    let link = dir.path().join("bad.jsonl");
    std::fs::write(&target, "{\"sessionId\":\"sess-1\",\"content\":\"hi\"}\n").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let result = index_roots(&pool, Some(dir.path())).unwrap();
    assert_eq!(result.skipped_files, 1);
    assert_eq!(result.file_errors.len(), 1);
    assert!(result.file_errors[0].path.contains("bad.jsonl"));
    assert!(result.file_errors[0].error.contains("symlinks"));
}
