use std::collections::HashSet;

use rusqlite::params;

use super::*;
use crate::config::StorageConfig;
use crate::db::init_pool;

fn test_pool() -> (DbPool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    (pool, dir)
}

#[test]
fn ensure_source_reuses_existing_source_id() {
    let (pool, _dir) = test_pool();
    let store = CheckpointStore::new(&pool);

    let first = store
        .ensure_source("/tmp/session.jsonl", "explicit_file")
        .unwrap();
    let second = store
        .ensure_source("/tmp/session.jsonl", "explicit_file")
        .unwrap();

    assert_eq!(first, second);
}

#[test]
fn record_keys_returns_imported_record_keys() {
    let (pool, _dir) = test_pool();
    let store = CheckpointStore::new(&pool);
    let source_id = store
        .ensure_source("/tmp/session.jsonl", "explicit_file")
        .unwrap();

    {
        let mut conn = pool.get().unwrap();
        let tx = conn.transaction().unwrap();
        let metadata = crate::scanner::FileMetadata {
            size: 42,
            mtime: Some(123),
            content_hash: "abc123".to_string(),
        };
        record_imports_in_tx(
            &tx,
            source_id,
            &["record-1".to_string(), "record-2".to_string()],
            &metadata,
        )
        .unwrap();
        tx.commit().unwrap();
    }

    let keys = store.record_keys(source_id).unwrap();

    assert_eq!(
        keys,
        HashSet::from(["record-1".to_string(), "record-2".to_string()])
    );
}

#[test]
fn mark_error_sets_last_error_and_successful_import_clears_it() {
    let (pool, _dir) = test_pool();
    let store = CheckpointStore::new(&pool);
    let source_id = store
        .ensure_source("/tmp/session.jsonl", "explicit_file")
        .unwrap();

    store.mark_error(source_id, "bad json").unwrap();

    {
        let conn = pool.get().unwrap();
        let last_error: String = conn
            .query_row(
                "SELECT last_error FROM transcript_sources WHERE id = ?1",
                [source_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_error, "bad json");
    }

    let mut conn = pool.get().unwrap();
    let tx = conn.transaction().unwrap();
    let metadata = crate::scanner::FileMetadata {
        size: 99,
        mtime: Some(456),
        content_hash: "content-hash".to_string(),
    };
    record_imports_in_tx(&tx, source_id, &["record-1".to_string()], &metadata).unwrap();
    tx.commit().unwrap();

    let row = conn
        .query_row(
            "SELECT file_size, file_mtime, content_hash, last_offset, last_error
             FROM transcript_sources WHERE id = ?1",
            [source_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(row, (99, 456, "content-hash".to_string(), 99, None));
}

#[test]
fn record_imports_ignores_duplicate_record_keys() {
    let (pool, _dir) = test_pool();
    let store = CheckpointStore::new(&pool);
    let source_id = store
        .ensure_source("/tmp/session.jsonl", "explicit_file")
        .unwrap();

    let mut conn = pool.get().unwrap();
    let tx = conn.transaction().unwrap();
    let metadata = crate::scanner::FileMetadata {
        size: 12,
        mtime: None,
        content_hash: "hash".to_string(),
    };
    record_imports_in_tx(
        &tx,
        source_id,
        &["same".to_string(), "same".to_string()],
        &metadata,
    )
    .unwrap();
    tx.commit().unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transcript_import_records WHERE source_id = ?1",
            params![source_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 1);
}
