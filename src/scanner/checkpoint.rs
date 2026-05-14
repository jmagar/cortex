use anyhow::Result;
use rusqlite::{params, OptionalExtension, Transaction};

use crate::db::DbPool;
use crate::scanner::{CheckpointEntry, CheckpointListOptions, FileMetadata};

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
            "INSERT INTO transcript_sources (canonical_path, source_kind)
             VALUES (?1, ?2)",
            params![canonical_path, source_kind],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn mark_error(&self, source_id: i64, error: &str) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE transcript_sources
             SET last_error = ?2
             WHERE id = ?1",
            params![source_id, error],
        )?;
        Ok(())
    }

    pub fn source_matches_metadata(
        &self,
        source_id: i64,
        file_size: u64,
        file_mtime: Option<i64>,
    ) -> Result<bool> {
        let conn = self.pool.get()?;
        let Some((stored_size, stored_mtime, last_error)) = conn
            .query_row(
                "SELECT file_size, file_mtime, last_error
                 FROM transcript_sources
                 WHERE id = ?1",
                [source_id],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?
        else {
            return Ok(false);
        };

        Ok(last_error.is_none()
            && stored_size == Some(file_size as i64)
            && stored_mtime == file_mtime)
    }

    pub fn reset_source(&self, source_id: i64, canonical_path: &str) -> Result<()> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM transcript_import_records WHERE source_id = ?1",
            [source_id],
        )?;
        tx.execute(
            "DELETE FROM logs WHERE ai_transcript_path = ?1",
            [canonical_path],
        )?;
        tx.execute(
            "UPDATE hosts
             SET log_count = (SELECT COUNT(*) FROM logs WHERE logs.hostname = hosts.hostname),
                 first_seen = COALESCE((SELECT MIN(received_at) FROM logs WHERE logs.hostname = hosts.hostname), first_seen),
                 last_seen = COALESCE((SELECT MAX(received_at) FROM logs WHERE logs.hostname = hosts.hostname), last_seen)",
            [],
        )?;
        tx.execute("DELETE FROM hosts WHERE log_count = 0", [])?;
        tx.execute(
            "UPDATE transcript_sources
             SET file_size = NULL,
                 file_mtime = NULL,
                 content_hash = NULL,
                 last_offset = 0,
                 last_indexed_at = NULL,
                 last_error = NULL
             WHERE id = ?1",
            [source_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_checkpoints(
        &self,
        options: &CheckpointListOptions,
    ) -> Result<Vec<CheckpointEntry>> {
        let conn = self.pool.get()?;
        let limit = options.limit.unwrap_or(50).min(500);
        let mut sql = String::from(
            "SELECT s.canonical_path,
                    s.source_kind,
                    s.file_size,
                    s.file_mtime,
                    s.content_hash,
                    s.last_offset,
                    s.last_indexed_at,
                    s.last_error,
                    COUNT(r.id) AS imported_records
             FROM transcript_sources s
             LEFT JOIN transcript_import_records r ON r.source_id = s.id",
        );
        if options.errors_only {
            sql.push_str(" WHERE s.last_error IS NOT NULL");
        }
        sql.push_str(
            " GROUP BY s.id
              ORDER BY
                CASE WHEN s.last_error IS NULL THEN 1 ELSE 0 END,
                COALESCE(s.last_indexed_at, '') DESC,
                s.canonical_path ASC
              LIMIT ?1",
        );

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([limit], |row| {
            Ok(CheckpointEntry {
                canonical_path: row.get(0)?,
                source_kind: row.get(1)?,
                file_size: row.get(2)?,
                file_mtime: row.get(3)?,
                content_hash: row.get(4)?,
                last_offset: row.get(5)?,
                last_indexed_at: row.get(6)?,
                last_error: row.get(7)?,
                imported_records: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

pub fn claim_imports_in_tx(
    tx: &Transaction<'_>,
    source_id: i64,
    record_keys: &[String],
) -> Result<Vec<bool>> {
    let mut claimed = Vec::with_capacity(record_keys.len());
    if record_keys.is_empty() {
        return Ok(claimed);
    }
    let mut stmt = tx.prepare_cached(
        "INSERT OR IGNORE INTO transcript_import_records (source_id, record_key)
         VALUES (?1, ?2)",
    )?;
    for record_key in record_keys {
        claimed.push(stmt.execute(params![source_id, record_key])? == 1);
    }
    Ok(claimed)
}

pub fn update_source_metadata_in_tx(
    tx: &Transaction<'_>,
    source_id: i64,
    file_metadata: &FileMetadata,
) -> Result<()> {
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

#[cfg(test)]
pub fn record_imports_in_tx(
    tx: &Transaction<'_>,
    source_id: i64,
    record_keys: &[String],
    file_metadata: &FileMetadata,
) -> Result<()> {
    let claimed = claim_imports_in_tx(tx, source_id, record_keys)?;
    if claimed.iter().any(|value| *value) || record_keys.is_empty() {
        update_source_metadata_in_tx(tx, source_id, file_metadata)?;
    } else {
        tx.execute(
            "UPDATE transcript_sources
             SET last_error = NULL
             WHERE id = ?1",
            params![source_id],
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
