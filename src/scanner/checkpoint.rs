use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::db::DbPool;

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

    pub fn has_record(&self, source_id: i64, record_key: &str) -> Result<bool> {
        let conn = self.pool.get()?;
        let exists = conn.query_row(
            "SELECT COUNT(*) FROM transcript_import_records
             WHERE source_id = ?1 AND record_key = ?2",
            params![source_id, record_key],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(exists > 0)
    }

    pub fn record_imports(&self, source_id: i64, record_keys: &[String]) -> Result<()> {
        if record_keys.is_empty() {
            return Ok(());
        }
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        {
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
             SET last_indexed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 last_error = NULL
             WHERE id = ?1",
            [source_id],
        )?;
        tx.commit()?;
        Ok(())
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
