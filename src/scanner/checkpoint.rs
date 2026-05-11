use std::collections::HashSet;

use anyhow::Result;
use rusqlite::{params, OptionalExtension, Transaction};

use crate::db::DbPool;
use crate::scanner::FileMetadata;

pub struct CheckpointStore<'a> {
    pool: &'a DbPool,
}

impl<'a> CheckpointStore<'a> {
    pub fn new(pool: &'a DbPool) -> Self {
        Self { pool }
    }

    pub fn ensure_source(&self, canonical_path: &str, source_kind: &str) -> Result<i64> {
        let conn = self.pool.get()?;
        if let Some(id) = conn
            .query_row(
                "SELECT id FROM transcript_sources WHERE canonical_path = ?1",
                [canonical_path],
                |row| row.get(0),
            )
            .optional()?
        {
            return Ok(id);
        }
        conn.execute(
            "INSERT INTO transcript_sources (canonical_path, source_kind, last_indexed_at)
             VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![canonical_path, source_kind],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn record_keys(&self, source_id: i64) -> Result<HashSet<String>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare_cached(
            "SELECT record_key FROM transcript_import_records WHERE source_id = ?1",
        )?;
        let rows = stmt.query_map([source_id], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
    }

    pub fn mark_error(&self, source_id: i64, error: &str) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE transcript_sources
             SET last_error = ?2,
                 last_indexed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            params![source_id, error],
        )?;
        Ok(())
    }
}

pub fn record_imports_in_tx(
    tx: &Transaction<'_>,
    source_id: i64,
    record_keys: &[String],
    file_metadata: &FileMetadata,
) -> Result<()> {
    if !record_keys.is_empty() {
        let mut stmt = tx.prepare_cached(
            "INSERT OR IGNORE INTO transcript_import_records (source_id, record_key)
             VALUES (?1, ?2)",
        )?;
        for record_key in record_keys {
            stmt.execute(params![source_id, record_key])?;
        }
    }
    tx.execute(
        "UPDATE transcript_sources
         SET file_size = ?2,
             file_mtime = ?3,
             content_hash = ?4,
             last_offset = ?5,
             last_indexed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             last_error = NULL
         WHERE id = ?1",
        params![
            source_id,
            file_metadata.size as i64,
            file_metadata.mtime,
            file_metadata.content_hash,
            file_metadata.size as i64,
        ],
    )?;
    Ok(())
}
