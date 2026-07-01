//! Database operations for the shared LLM invocation audit table
//! (`llm_invocations`, migration 37). `LlmRunner`
//! (`src/app/llm_runner.rs`) is the only writer; the CLI/MCP/REST read
//! surfaces (`sessions llm-invocations`, MCP `llm_invocations` action,
//! `GET /api/sessions/llm-invocations`) are the readers.
//!
//! Call from inside `tokio::task::spawn_blocking`, never from async
//! context directly (same convention as `src/db/notifications.rs`).

use rusqlite::params;

/// Parameters for the initial (status='running' or a denial status)
/// insert. `id` is passed separately since callers generate it before
/// building the params (needed so denial paths can audit without a
/// completed spec).
pub struct LlmInvocationInsertParams {
    pub caller_surface: String,
    pub action: String,
    pub provider: String,
    pub model: Option<String>,
    pub program: Option<String>,
    pub incident_id: Option<String>,
    pub ai_tool: Option<String>,
    pub ai_project: Option<String>,
    pub ai_session_id: Option<String>,
    pub evidence_counts_json: Option<String>,
    pub prompt_bytes: Option<i64>,
    pub status: String,
    pub metadata_json: Option<String>,
}

pub fn insert_llm_invocation_running(
    conn: &rusqlite::Connection,
    id: &str,
    p: &LlmInvocationInsertParams,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO llm_invocations
             (id, started_at, caller_surface, action, provider, model, program,
              incident_id, ai_tool, ai_project, ai_session_id,
              evidence_counts_json, prompt_bytes, status, metadata_json)
         VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ','now'), ?2, ?3, ?4, ?5, ?6,
                 ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            id,
            p.caller_surface,
            p.action,
            p.provider,
            p.model,
            p.program,
            p.incident_id,
            p.ai_tool,
            p.ai_project,
            p.ai_session_id,
            p.evidence_counts_json,
            p.prompt_bytes,
            p.status,
            p.metadata_json,
        ],
    )?;
    Ok(())
}

pub fn finish_llm_invocation(
    conn: &rusqlite::Connection,
    id: &str,
    status: &str,
    error: Option<&str>,
    duration_ms: i64,
    output_bytes: Option<i64>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE llm_invocations
         SET finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'),
             duration_ms = ?2,
             status = ?3,
             error = ?4,
             output_bytes = COALESCE(?5, output_bytes)
         WHERE id = ?1",
        params![id, duration_ms, status, error, output_bytes],
    )?;
    Ok(())
}

/// A row from `llm_invocations`, as returned to CLI/MCP/REST readers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmInvocationRow {
    pub id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub caller_surface: String,
    pub action: String,
    pub provider: String,
    pub model: Option<String>,
    pub program: Option<String>,
    pub incident_id: Option<String>,
    pub ai_tool: Option<String>,
    pub ai_project: Option<String>,
    pub ai_session_id: Option<String>,
    pub evidence_counts_json: Option<String>,
    pub prompt_bytes: Option<i64>,
    pub output_bytes: Option<i64>,
    pub status: String,
    pub error: Option<String>,
    pub metadata_json: Option<String>,
}

/// Fetch recent invocations, optionally filtered by `action`/`status` and
/// bounded to those started at or after `since` (ISO8601). `limit` is
/// clamped to `[1, 500]`, matching `notifications::firings_recent`.
pub fn list_llm_invocations(
    conn: &rusqlite::Connection,
    limit: i64,
    since: Option<&str>,
    action: Option<&str>,
    status: Option<&str>,
) -> rusqlite::Result<Vec<LlmInvocationRow>> {
    let clamped_limit = limit.clamp(1, 500);
    let mut stmt = conn.prepare(
        "SELECT id, started_at, finished_at, duration_ms, caller_surface, action,
                provider, model, program, incident_id, ai_tool, ai_project,
                ai_session_id, evidence_counts_json, prompt_bytes, output_bytes,
                status, error, metadata_json
         FROM llm_invocations
         WHERE (?1 IS NULL OR started_at >= ?1)
           AND (?2 IS NULL OR action = ?2)
           AND (?3 IS NULL OR status = ?3)
         ORDER BY started_at DESC
         LIMIT ?4",
    )?;
    let rows = stmt
        .query_map(params![since, action, status, clamped_limit], |row| {
            Ok(LlmInvocationRow {
                id: row.get(0)?,
                started_at: row.get(1)?,
                finished_at: row.get(2)?,
                duration_ms: row.get(3)?,
                caller_surface: row.get(4)?,
                action: row.get(5)?,
                provider: row.get(6)?,
                model: row.get(7)?,
                program: row.get(8)?,
                incident_id: row.get(9)?,
                ai_tool: row.get(10)?,
                ai_project: row.get(11)?,
                ai_session_id: row.get(12)?,
                evidence_counts_json: row.get(13)?,
                prompt_bytes: row.get(14)?,
                output_bytes: row.get(15)?,
                status: row.get(16)?,
                error: row.get(17)?,
                metadata_json: row.get(18)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
#[path = "llm_invocations_tests.rs"]
mod tests;
