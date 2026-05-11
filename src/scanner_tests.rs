use super::*;
use crate::config::StorageConfig;
use crate::db::init_pool;

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
