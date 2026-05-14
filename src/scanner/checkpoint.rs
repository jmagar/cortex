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
