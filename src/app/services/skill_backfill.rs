//! `sessions skills backfill` — scans existing `logs` rows with `ai_tool IN
//! ('claude','codex')` and extracts/persists `ai_skill_events` for rows that
//! predate this phase's ingest-time wiring (`src/scanner.rs::flush_chunk`).
//!
//! Chunked scan-and-release: each chunk acquires a fresh pool connection,
//! processes up to `CHUNK_SIZE` rows, and drops the connection before
//! continuing — mirrors `purge_old_logs` in `src/db/maintenance.rs` so a
//! large historical backfill never starves the ingest writer of a pool
//! connection for more than one chunk's duration.
//!
//! **Eng review Fix 6**: unlike `purge_old_logs` (which DELETEs and
//! correctly holds `write_lock()`), `fetch_candidate_chunk` here is a pure
//! `SELECT` — WAL mode already gives readers a consistent snapshot without
//! the write lock, so it is NOT held around the fetch. Only
//! `insert_skill_events` (a real write) acquires the lock, and it does so
//! internally (see the DB layer).
//!
//! **Eng review Fix 7**: `limit` is hard-clamped to `[1, 1_000_000]` (an
//! operator/caller cannot drive an unbounded scan), and a process-wide
//! single-flight guard (`backfill_guard`) ensures only one backfill runs at
//! a time — a second concurrent call fails fast with `ServiceError::Busy`
//! instead of both holding a `run_db` semaphore permit for the whole
//! multi-chunk scan. The guard is service-scoped (not REST-scoped like
//! `api.rs`'s `SHARED_MAINTENANCE_PERMIT`) because this method is also
//! reachable from the CLI's local mode and MCP, neither of which goes
//! through `api.rs`.

use std::sync::{Arc, OnceLock};

use anyhow::Result;
use tokio::sync::Semaphore;

use crate::db::{DbPool, SkillEventInsert, insert_skill_events};
use crate::scanner::skill_events::{extract_claude_skill_events, extract_codex_skill_events};

use super::super::models::{SkillBackfillRequest, SkillBackfillResult};
use super::super::time::parse_optional_timestamp;
use super::{CortexService, ServiceError, ServiceResult};

const CHUNK_SIZE: i64 = 2_000;

/// Eng review Fix 7: hard upper bound on `SkillBackfillRequest.limit`. Chosen
/// generously above any realistic single-host `logs` table size (millions of
/// rows would already be well past `CORTEX_MAX_DB_SIZE_MB`'s default 1024 MB
/// guard in practice) while still being a real, enforced ceiling rather than
/// "whatever the caller asks for" — closes the unbounded-scan DoS surface
/// now that this is a `pub` service method reachable from CLI/MCP/REST.
const MAX_BACKFILL_LIMIT: u64 = 1_000_000;

/// Eng review Fix 7: process-wide single-flight gate for
/// `backfill_skill_events`, mirroring the `SHARED_MAINTENANCE_PERMIT`
/// pattern in `src/api.rs` (`OnceLock<Arc<Semaphore>>` + `try_acquire_owned`)
/// but scoped to the service layer so CLI-local and MCP callers are covered
/// too, not just REST. A held permit means a backfill is in flight; a second
/// concurrent call observes `NoPermits` and returns `ServiceError::Busy`
/// immediately rather than queuing behind the first scan.
fn backfill_guard() -> Arc<Semaphore> {
    static GUARD: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(GUARD.get_or_init(|| Arc::new(Semaphore::new(1))))
}

struct CandidateRow {
    id: i64,
    ai_tool: String,
    ai_project: Option<String>,
    ai_session_id: Option<String>,
    hostname: String,
    timestamp: String,
    message: String,
}

impl CortexService {
    pub async fn backfill_skill_events(
        &self,
        req: SkillBackfillRequest,
    ) -> ServiceResult<SkillBackfillResult> {
        let since = parse_optional_timestamp(req.since.as_deref(), "since")?;
        // Eng review Fix 7: hard clamp, not just a floor.
        let limit = req.limit.unwrap_or(10_000).clamp(1, MAX_BACKFILL_LIMIT);
        let dry_run = req.dry_run;

        // Eng review Fix 7: single-flight guard acquired BEFORE the run_db
        // call so a second concurrent caller never even queues for a DB
        // permit — it fails fast here instead.
        let _permit = backfill_guard()
            .try_acquire_owned()
            .map_err(|_| ServiceError::Busy("skill event backfill already running".into()))?;

        self.run_db("backfill_skill_events", move |pool| {
            run_backfill(pool, since.as_deref(), limit, dry_run)
        })
        .await
    }
}

fn run_backfill(
    pool: &DbPool,
    since: Option<&str>,
    limit: u64,
    dry_run: bool,
) -> Result<SkillBackfillResult> {
    let mut result = SkillBackfillResult {
        dry_run,
        ..Default::default()
    };
    let mut last_id = 0i64;
    let mut remaining = limit;

    loop {
        if remaining == 0 {
            result.truncated = true;
            break;
        }
        let chunk_limit = CHUNK_SIZE.min(remaining as i64).max(1);
        // Eng review Fix 6: no write_lock() here — this is a pure SELECT and
        // WAL mode already gives it a consistent snapshot. Holding the write
        // lock around a read needlessly serializes the backfill against the
        // live syslog ingest writer for zero correctness benefit.
        let conn = pool.get()?;
        let rows = fetch_candidate_chunk(&conn, since, last_id, chunk_limit)?;
        drop(conn); // release back to pool before per-row parse work + next chunk

        if rows.is_empty() {
            break;
        }
        last_id = rows.last().map(|r| r.id).unwrap_or(last_id);
        result.scanned += rows.len() as u64;
        remaining = remaining.saturating_sub(rows.len() as u64);

        let mut inserts = Vec::new();
        for row in &rows {
            // Eng review Fix 1: the backfill reads `row.message` straight
            // from the `logs` table (there is no pre-parsed Value to reuse
            // here, unlike the ingest hot path — this is a one-time
            // historical scan, not the per-request ingest loop), so a JSON
            // parse is unavoidable for Claude rows that DO have a skill
            // event. The substring short-circuit still applies: skip the
            // parse entirely for the common case where the row has no
            // attributionSkill field at all.
            let extracted = match row.ai_tool.as_str() {
                "claude" if row.message.contains("attributionSkill") => {
                    match serde_json::from_str::<serde_json::Value>(&row.message) {
                        Ok(value) => extract_claude_skill_events(&value),
                        Err(_) => {
                            result.parse_errors += 1;
                            continue;
                        }
                    }
                }
                "claude" => continue,
                "codex" => extract_codex_skill_events(&row.message),
                _ => continue,
            };
            for event in extracted {
                inserts.push(SkillEventInsert {
                    log_id: row.id,
                    ai_tool: row.ai_tool.clone(),
                    ai_project: row.ai_project.clone(),
                    ai_session_id: row.ai_session_id.clone(),
                    hostname: row.hostname.clone(),
                    timestamp: row.timestamp.clone(),
                    event,
                });
            }
        }

        if !dry_run && !inserts.is_empty() {
            let attempted = inserts.len() as u64;
            let inserted = insert_skill_events(pool, &inserts)? as u64;
            result.inserted += inserted;
            result.skipped_duplicates += attempted - inserted;
        }
        // Dry-run does not report a "would insert N" count — scanned /
        // parse_errors are the only meaningful dry-run signal per the CLI
        // contract (`--dry-run` reports scanned rows and parse errors
        // without touching the table). Callers that need a precise
        // "would insert N" count should drop --dry-run.

        if (rows.len() as i64) < chunk_limit {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    Ok(result)
}

fn fetch_candidate_chunk(
    conn: &rusqlite::Connection,
    since: Option<&str>,
    last_id: i64,
    chunk_limit: i64,
) -> Result<Vec<CandidateRow>> {
    let (sql, bindings): (&str, Vec<rusqlite::types::Value>) = match since {
        Some(since) => (
            "SELECT id, ai_tool, ai_project, ai_session_id, hostname, timestamp, message
             FROM logs
             WHERE ai_tool IN ('claude', 'codex')
               AND id > ?1
               AND timestamp >= ?2
             ORDER BY id ASC
             LIMIT ?3",
            vec![
                rusqlite::types::Value::Integer(last_id),
                rusqlite::types::Value::Text(since.to_string()),
                rusqlite::types::Value::Integer(chunk_limit),
            ],
        ),
        None => (
            "SELECT id, ai_tool, ai_project, ai_session_id, hostname, timestamp, message
             FROM logs
             WHERE ai_tool IN ('claude', 'codex')
               AND id > ?1
             ORDER BY id ASC
             LIMIT ?2",
            vec![
                rusqlite::types::Value::Integer(last_id),
                rusqlite::types::Value::Integer(chunk_limit),
            ],
        ),
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
            Ok(CandidateRow {
                id: row.get(0)?,
                ai_tool: row.get(1)?,
                ai_project: row.get(2)?,
                ai_session_id: row.get(3)?,
                hostname: row.get(4)?,
                timestamp: row.get(5)?,
                message: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
#[path = "skill_backfill_tests.rs"]
mod tests;
