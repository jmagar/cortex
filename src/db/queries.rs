use anyhow::Result;
use rusqlite::params;

use crate::config::StorageConfig;

use super::maintenance::{exceeds_trigger, get_storage_metrics};
use super::models::{
    AiCorrelateParams, AiCussMatch, AiCussParams, AiCussResult, AiProjectInventoryEntry,
    AiSessionEntry, AiToolInventoryEntry, DbStats, ErrorSummaryEntry, HostEntry,
    ListAiProjectsParams, ListAiProjectsResult, ListAiSessionsParams, ListAiToolsParams,
    ListAiToolsResult, LogEntry, SearchAiSessionsParams, SearchAiSessionsResult, SearchParams,
    SearchedAiSessionEntry,
};
use super::pool::DbPool;

/// Validate a user-supplied FTS5 query before execution.
///
/// Limits:
/// - Max 512 characters (prevents very long queries from taxing the FTS tokenizer)
/// - Max 16 whitespace-separated terms (prevents 28+ wildcard term DoS)
///
/// Returns a user-friendly error; the caller logs the details server-side.
pub fn validate_fts_query(query: &str) -> Result<()> {
    if query.len() > 512 {
        anyhow::bail!(
            "Search query too long ({} chars); maximum is 512 characters",
            query.len()
        );
    }
    let term_count = query.split_whitespace().count();
    if term_count > 16 {
        anyhow::bail!("Search query has too many terms ({term_count}); maximum is 16 terms");
    }
    Ok(())
}

/// Search logs with flexible filtering + FTS
pub fn search_logs(pool: &DbPool, params: &SearchParams) -> Result<Vec<LogEntry>> {
    let conn = pool.get()?;
    let limit = params.limit.unwrap_or(100).min(1000);

    // If we have a full-text query, use FTS5 join
    if let Some(ref query) = params.query {
        validate_fts_query(query)?;

        let mut sql = String::from(
            "SELECT l.id, l.timestamp, l.hostname, l.facility, l.severity,
                    l.app_name, l.process_id, l.message, l.received_at, l.source_ip,
                    l.ai_tool, l.ai_project, l.ai_session_id, l.ai_transcript_path, l.metadata_json
             FROM logs l
             JOIN logs_fts ON logs_fts.rowid = l.id
             WHERE logs_fts MATCH ?1",
        );
        let mut bindings: Vec<rusqlite::types::Value> =
            vec![rusqlite::types::Value::Text(query.clone())];
        let mut idx = 2;

        append_filters(&mut sql, &mut bindings, &mut idx, params);
        sql.push_str(&format!(" ORDER BY l.timestamp DESC LIMIT {limit}"));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(bindings.iter()), map_row)
            .map_err(|e| {
                tracing::error!(error = %e, query = %query, "FTS5 MATCH query failed");
                anyhow::anyhow!("Search query failed")
            })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| {
            tracing::error!(error = %e, query = %query, "FTS5 row mapping failed");
            anyhow::anyhow!("Search query failed")
        })
    } else {
        let mut sql = String::from(
            "SELECT l.id, l.timestamp, l.hostname, l.facility, l.severity,
                    l.app_name, l.process_id, l.message, l.received_at, l.source_ip,
                    l.ai_tool, l.ai_project, l.ai_session_id, l.ai_transcript_path, l.metadata_json
             FROM logs l WHERE 1=1",
        );
        let mut bindings: Vec<rusqlite::types::Value> = vec![];
        let mut idx = 1;

        append_filters(&mut sql, &mut bindings, &mut idx, params);
        sql.push_str(&format!(" ORDER BY l.timestamp DESC LIMIT {limit}"));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(bindings.iter()), map_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

/// Get the N most recent logs for a host/service
pub fn tail_logs(
    pool: &DbPool,
    hostname: Option<&str>,
    source_ip: Option<&str>,
    app_name: Option<&str>,
    severity_in: Option<&[String]>,
    n: u32,
) -> Result<Vec<LogEntry>> {
    let conn = pool.get()?;
    let n = n.min(500);

    let mut sql = String::from(
        "SELECT id, timestamp, hostname, facility, severity,
                app_name, process_id, message, received_at, source_ip,
                ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
         FROM logs WHERE 1=1",
    );
    let mut bindings: Vec<rusqlite::types::Value> = vec![];
    let mut idx = 1;

    if let Some(h) = hostname {
        sql.push_str(&format!(" AND hostname = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(h.to_string()));
        idx += 1;
    }
    if let Some(source_ip) = source_ip {
        sql.push_str(&format!(" AND source_ip = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(source_ip.to_string()));
        idx += 1;
    }
    if let Some(a) = app_name {
        sql.push_str(&format!(" AND app_name = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(a.to_string()));
        idx += 1;
    }
    if let Some(levels) = severity_in {
        if !levels.is_empty() {
            let placeholders: Vec<String> =
                (0..levels.len()).map(|i| format!("?{}", idx + i)).collect();
            sql.push_str(&format!(" AND severity IN ({})", placeholders.join(", ")));
            for lvl in levels {
                bindings.push(rusqlite::types::Value::Text(lvl.clone()));
                idx += 1;
            }
            debug_assert_eq!(bindings.len() + 1, idx);
        }
    }

    sql.push_str(&format!(" ORDER BY timestamp DESC LIMIT {n}"));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(bindings.iter()), map_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Get error/warning summary per host in a time window. When `group_by_app` is
/// true, results also include `app_name` as a secondary grouping key.
pub fn get_error_summary(
    pool: &DbPool,
    from: Option<&str>,
    to: Option<&str>,
    group_by_app: bool,
) -> Result<Vec<ErrorSummaryEntry>> {
    let conn = pool.get()?;

    let from = from.unwrap_or("1970-01-01T00:00:00Z");
    // Upper sentinel: any valid RFC 3339 timestamp will sort before this.
    let to = to.unwrap_or("9999-12-31T23:59:59Z");

    if group_by_app {
        let mut stmt = conn.prepare(
            "SELECT hostname, app_name, severity, COUNT(*) as count
             FROM logs
             WHERE severity IN ('emerg', 'alert', 'crit', 'err', 'warning')
               AND timestamp BETWEEN ?1 AND ?2
             GROUP BY hostname, app_name, severity
             ORDER BY hostname, app_name, count DESC",
        )?;
        let rows = stmt.query_map(params![from, to], |row| {
            Ok(ErrorSummaryEntry {
                hostname: row.get(0)?,
                app_name: row.get::<_, Option<String>>(1)?,
                severity: row.get(2)?,
                count: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    } else {
        let mut stmt = conn.prepare(
            "SELECT hostname, severity, COUNT(*) as count
             FROM logs
             WHERE severity IN ('emerg', 'alert', 'crit', 'err', 'warning')
               AND timestamp BETWEEN ?1 AND ?2
             GROUP BY hostname, severity
             ORDER BY hostname, count DESC",
        )?;
        let rows = stmt.query_map(params![from, to], |row| {
            Ok(ErrorSummaryEntry {
                hostname: row.get(0)?,
                app_name: None,
                severity: row.get(1)?,
                count: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

/// List all known hosts with stats
pub fn list_hosts(pool: &DbPool) -> Result<Vec<HostEntry>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT hostname, first_seen, last_seen, log_count FROM hosts ORDER BY last_seen DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(HostEntry {
            hostname: row.get(0)?,
            first_seen: row.get(1)?,
            last_seen: row.get(2)?,
            log_count: row.get(3)?,
        })
    })?;

    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn list_ai_sessions(
    pool: &DbPool,
    params: &ListAiSessionsParams,
) -> Result<Vec<AiSessionEntry>> {
    let conn = pool.get()?;
    let limit = params.limit.unwrap_or(100).min(1000);
    let mut sql = String::from(
        "SELECT ai_project, ai_tool, ai_session_id,
                MIN(ai_transcript_path) AS ai_transcript_path,
                hostname,
                MIN(timestamp) AS first_seen,
                MAX(timestamp) AS last_seen,
                COUNT(*) AS event_count
         FROM logs
         WHERE ai_project IS NOT NULL
           AND ai_project != ''
           AND ai_tool IS NOT NULL
           AND ai_tool != ''
           AND ai_session_id IS NOT NULL
           AND ai_session_id != ''",
    );
    let mut bindings: Vec<rusqlite::types::Value> = vec![];
    let mut idx = 1;

    if let Some(project) = &params.ai_project {
        sql.push_str(&format!(" AND ai_project = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(project.clone()));
        idx += 1;
    }
    if let Some(tool) = &params.ai_tool {
        sql.push_str(&format!(" AND ai_tool = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(tool.clone()));
        idx += 1;
    }
    if let Some(hostname) = &params.hostname {
        sql.push_str(&format!(" AND hostname = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(hostname.clone()));
        idx += 1;
    }
    if let Some(from) = &params.from {
        sql.push_str(&format!(" AND timestamp >= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(from.clone()));
        idx += 1;
    }
    if let Some(to) = &params.to {
        sql.push_str(&format!(" AND timestamp <= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(to.clone()));
    }

    sql.push_str(&format!(
        " GROUP BY ai_project, ai_tool, ai_session_id, hostname
          ORDER BY last_seen DESC
          LIMIT {limit}"
    ));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
        Ok(AiSessionEntry {
            ai_project: row.get(0)?,
            ai_tool: row.get(1)?,
            ai_session_id: row.get(2)?,
            ai_transcript_path: row.get(3)?,
            hostname: row.get(4)?,
            first_seen: row.get(5)?,
            last_seen: row.get(6)?,
            event_count: row.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn search_ai_sessions(
    pool: &DbPool,
    params: &SearchAiSessionsParams,
) -> Result<SearchAiSessionsResult> {
    validate_fts_query(&params.query)?;

    let conn = pool.get()?;
    let limit = params.limit.unwrap_or(20).clamp(1, 100) as usize;
    const CANDIDATE_CAP: usize = 5_000;

    let mut sql = String::from(
        "WITH candidates AS (
            SELECT l.ai_project,
                   l.ai_tool,
                   l.ai_session_id,
                   l.hostname,
                   l.timestamp,
                   l.message
            FROM logs_fts
            JOIN logs l ON l.id = logs_fts.rowid
            WHERE logs_fts MATCH ?1
              AND l.ai_project IS NOT NULL AND l.ai_project != ''
              AND l.ai_tool IS NOT NULL AND l.ai_tool != ''
              AND l.ai_session_id IS NOT NULL AND l.ai_session_id != ''",
    );
    let mut bindings = vec![rusqlite::types::Value::Text(params.query.clone())];
    let mut idx = 2usize;

    if let Some(project) = &params.ai_project {
        sql.push_str(&format!(" AND l.ai_project = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(project.clone()));
        idx += 1;
    }
    if let Some(tool) = &params.ai_tool {
        sql.push_str(&format!(" AND l.ai_tool = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(tool.clone()));
        idx += 1;
    }
    if let Some(from) = &params.from {
        sql.push_str(&format!(" AND l.timestamp >= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(from.clone()));
        idx += 1;
    }
    if let Some(to) = &params.to {
        sql.push_str(&format!(" AND l.timestamp <= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(to.clone()));
    }
    sql.push_str(&format!(
        " ORDER BY logs_fts.rowid DESC
           LIMIT {}
         ),
         grouped AS (
            SELECT ai_project,
                   ai_tool,
                   ai_session_id,
                   hostname,
                   MIN(timestamp) AS first_seen,
                   MAX(timestamp) AS last_seen,
                   COUNT(*) AS match_count,
                   (
                       SELECT c2.message
                       FROM candidates c2
                       WHERE c2.ai_project = c.ai_project
                         AND c2.ai_tool = c.ai_tool
                         AND c2.ai_session_id = c.ai_session_id
                         AND c2.hostname = c.hostname
                       ORDER BY c2.timestamp DESC
                       LIMIT 1
                   ) AS best_snippet
            FROM candidates c
            GROUP BY ai_project, ai_tool, ai_session_id, hostname
         )
         SELECT ai_project, ai_tool, ai_session_id, hostname,
                first_seen,
                last_seen,
                (
                    SELECT COUNT(*)
                    FROM logs total
                    WHERE total.ai_project = grouped.ai_project
                      AND total.ai_tool = grouped.ai_tool
                      AND total.ai_session_id = grouped.ai_session_id
                      AND total.hostname = grouped.hostname
                ) AS event_count,
                match_count,
                best_snippet,
                COUNT(*) OVER() AS total_candidates,
                (SELECT COUNT(*) FROM candidates) AS raw_candidate_count
         FROM grouped
         ORDER BY last_seen DESC
         LIMIT {limit}",
        CANDIDATE_CAP + 1
    ));

    let mut stmt = conn.prepare(&sql)?;
    let mut total_candidates = 0usize;
    let mut raw_candidate_count = 0usize;
    let rows = stmt.query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
        total_candidates = row.get::<_, i64>(9)? as usize;
        raw_candidate_count = row.get::<_, i64>(10)? as usize;
        Ok(SearchedAiSessionEntry {
            ai_project: row.get(0)?,
            ai_tool: row.get(1)?,
            ai_session_id: row.get(2)?,
            hostname: row.get(3)?,
            first_seen: row.get(4)?,
            last_seen: row.get(5)?,
            event_count: row.get(6)?,
            match_count: row.get(7)?,
            best_snippet: row.get(8)?,
        })
    })?;
    let sessions = rows.collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(SearchAiSessionsResult {
        total_candidates,
        candidate_rows: raw_candidate_count.min(CANDIDATE_CAP),
        candidate_cap: CANDIDATE_CAP,
        candidate_window_truncated: raw_candidate_count > CANDIDATE_CAP,
        truncated: total_candidates > sessions.len() || raw_candidate_count > CANDIDATE_CAP,
        sessions,
    })
}

pub fn search_ai_anchors(pool: &DbPool, params: &AiCorrelateParams) -> Result<Vec<LogEntry>> {
    let conn = pool.get()?;
    let limit = params.limit.unwrap_or(10).clamp(1, 50);
    let mut bindings: Vec<rusqlite::types::Value> = vec![];
    let mut idx = 1usize;
    let mut sql = if let Some(query) = &params.ai_query {
        validate_fts_query(query)?;
        bindings.push(rusqlite::types::Value::Text(query.clone()));
        idx += 1;
        String::from(
            "SELECT l.id, l.timestamp, l.hostname, l.facility, l.severity,
                    l.app_name, l.process_id, l.message, l.received_at, l.source_ip,
                    l.ai_tool, l.ai_project, l.ai_session_id, l.ai_transcript_path, l.metadata_json
             FROM logs_fts
             JOIN logs l ON l.id = logs_fts.rowid
             WHERE logs_fts MATCH ?1
               AND l.ai_project IS NOT NULL AND l.ai_project != ''
               AND l.ai_tool IS NOT NULL AND l.ai_tool != ''
               AND l.ai_session_id IS NOT NULL AND l.ai_session_id != ''",
        )
    } else {
        String::from(
            "SELECT l.id, l.timestamp, l.hostname, l.facility, l.severity,
                    l.app_name, l.process_id, l.message, l.received_at, l.source_ip,
                    l.ai_tool, l.ai_project, l.ai_session_id, l.ai_transcript_path, l.metadata_json
             FROM logs l
             WHERE l.ai_project IS NOT NULL AND l.ai_project != ''
               AND l.ai_tool IS NOT NULL AND l.ai_tool != ''
               AND l.ai_session_id IS NOT NULL AND l.ai_session_id != ''",
        )
    };

    if let Some(project) = &params.ai_project {
        sql.push_str(&format!(" AND l.ai_project = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(project.clone()));
        idx += 1;
    }
    if let Some(tool) = &params.ai_tool {
        sql.push_str(&format!(" AND l.ai_tool = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(tool.clone()));
        idx += 1;
    }
    if let Some(session_id) = &params.ai_session_id {
        sql.push_str(&format!(" AND l.ai_session_id = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(session_id.clone()));
        idx += 1;
    }
    if let Some(from) = &params.from {
        sql.push_str(&format!(" AND l.timestamp >= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(from.clone()));
        idx += 1;
    }
    if let Some(to) = &params.to {
        sql.push_str(&format!(" AND l.timestamp <= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(to.clone()));
    }

    sql.push_str(&format!(
        " ORDER BY l.timestamp DESC, l.id DESC LIMIT {}",
        limit + 1
    ));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(bindings.iter()), map_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

const DEFAULT_AI_CUSS_TERMS: &[&str] = &[
    "asshole", "bastard", "bitch", "biznitch", "bullshit", "crap", "damn", "dick", "fuck",
    "fucked", "fucker", "fucking", "hell", "piss", "shit", "shitty",
];

pub fn search_ai_cusses(pool: &DbPool, params: &AiCussParams) -> Result<AiCussResult> {
    let conn = pool.get()?;
    let limit = params.limit.unwrap_or(20).clamp(1, 100) as usize;
    let before = params.before.unwrap_or(2).min(20);
    let after = params.after.unwrap_or(2).min(20);
    let terms = normalized_cuss_terms(&params.terms);
    const CANDIDATE_CAP: usize = 10_000;

    let mut sql = String::from(
        "SELECT id, timestamp, hostname, facility, severity,
                app_name, process_id, message, received_at, source_ip,
                ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
         FROM logs
         WHERE ai_project IS NOT NULL AND ai_project != ''
           AND ai_tool IS NOT NULL AND ai_tool != ''
           AND ai_session_id IS NOT NULL AND ai_session_id != ''
           AND (",
    );
    let mut bindings = Vec::new();
    let mut idx = 1usize;
    for (term_idx, term) in terms.iter().enumerate() {
        if term_idx > 0 {
            sql.push_str(" OR ");
        }
        sql.push_str(&format!("lower(message) LIKE ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(format!("%{term}%")));
        idx += 1;
    }
    sql.push(')');

    if let Some(project) = &params.ai_project {
        sql.push_str(&format!(" AND ai_project = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(project.clone()));
        idx += 1;
    }
    if let Some(tool) = &params.ai_tool {
        sql.push_str(&format!(" AND ai_tool = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(tool.clone()));
        idx += 1;
    }
    if let Some(from) = &params.from {
        sql.push_str(&format!(" AND timestamp >= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(from.clone()));
        idx += 1;
    }
    if let Some(to) = &params.to {
        sql.push_str(&format!(" AND timestamp <= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(to.clone()));
    }
    sql.push_str(&format!(
        " ORDER BY timestamp DESC, id DESC LIMIT {}",
        CANDIDATE_CAP + 1
    ));

    let mut stmt = conn.prepare(&sql)?;
    let candidate_rows = stmt
        .query_map(rusqlite::params_from_iter(bindings.iter()), map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let candidate_window_truncated = candidate_rows.len() > CANDIDATE_CAP;
    let mut matches = Vec::new();
    for entry in candidate_rows.iter().take(CANDIDATE_CAP) {
        if let Some(term) = first_cuss_term(&entry.message, &terms) {
            let (before_rows, after_rows) = ai_session_context(&conn, entry, before, after)?;
            matches.push(AiCussMatch {
                term,
                entry: entry.clone(),
                before: before_rows,
                after: after_rows,
            });
            if matches.len() == limit {
                break;
            }
        }
    }

    Ok(AiCussResult {
        terms,
        candidate_rows: candidate_rows.len().min(CANDIDATE_CAP),
        candidate_cap: CANDIDATE_CAP,
        candidate_window_truncated,
        truncated: candidate_window_truncated || matches.len() == limit,
        matches,
    })
}

pub fn list_ai_tools(pool: &DbPool, params: &ListAiToolsParams) -> Result<ListAiToolsResult> {
    let conn = pool.get()?;
    const LIMIT: usize = 100;
    let mut sql = String::from(
        "SELECT ai_tool,
                COUNT(*) AS event_count,
                COUNT(DISTINCT ai_session_id) AS session_count,
                MIN(timestamp) AS first_seen,
                MAX(timestamp) AS last_seen
         FROM logs
         WHERE ai_tool IS NOT NULL
           AND ai_tool != ''",
    );
    let mut bindings: Vec<rusqlite::types::Value> = vec![];
    let mut idx = 1usize;

    if let Some(project) = &params.ai_project {
        sql.push_str(&format!(" AND ai_project = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(project.clone()));
        idx += 1;
    }
    if let Some(from) = &params.from {
        sql.push_str(&format!(" AND timestamp >= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(from.clone()));
        idx += 1;
    }
    if let Some(to) = &params.to {
        sql.push_str(&format!(" AND timestamp <= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(to.clone()));
    }
    let grouped_sql = format!("{sql} GROUP BY ai_tool");
    let total_tools = count_grouped_rows(&conn, &grouped_sql, &bindings)?;
    sql = grouped_sql;
    sql.push_str(&format!(
        " ORDER BY event_count DESC, ai_tool ASC LIMIT {}",
        LIMIT + 1
    ));

    let mut stmt = conn.prepare(&sql)?;
    let mut tools = stmt
        .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
            Ok(AiToolInventoryEntry {
                tool: row.get(0)?,
                event_count: row.get(1)?,
                session_count: row.get(2)?,
                first_seen: row.get(3)?,
                last_seen: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let truncated = truncate_to_limit(&mut tools, LIMIT);
    Ok(ListAiToolsResult {
        total_tools,
        truncated,
        tools,
    })
}

pub fn list_ai_projects(
    pool: &DbPool,
    params: &ListAiProjectsParams,
) -> Result<ListAiProjectsResult> {
    let conn = pool.get()?;
    const LIMIT: usize = 200;
    let mut sql = String::from(
        "SELECT ai_project,
                GROUP_CONCAT(DISTINCT ai_tool) AS tools,
                COUNT(*) AS event_count,
                COUNT(DISTINCT ai_session_id) AS session_count,
                MIN(timestamp) AS first_seen,
                MAX(timestamp) AS last_seen
         FROM logs
         WHERE ai_project IS NOT NULL
           AND ai_project != ''",
    );
    let mut bindings: Vec<rusqlite::types::Value> = vec![];
    let mut idx = 1usize;

    if let Some(tool) = &params.ai_tool {
        sql.push_str(&format!(" AND ai_tool = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(tool.clone()));
        idx += 1;
    }
    if let Some(from) = &params.from {
        sql.push_str(&format!(" AND timestamp >= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(from.clone()));
        idx += 1;
    }
    if let Some(to) = &params.to {
        sql.push_str(&format!(" AND timestamp <= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(to.clone()));
    }
    let grouped_sql = format!("{sql} GROUP BY ai_project");
    let total_projects = count_grouped_rows(&conn, &grouped_sql, &bindings)?;
    sql = grouped_sql;
    sql.push_str(&format!(
        " ORDER BY event_count DESC, ai_project ASC LIMIT {}",
        LIMIT + 1
    ));

    let mut stmt = conn.prepare(&sql)?;
    let mut projects = stmt
        .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
            let tools = row
                .get::<_, Option<String>>(1)?
                .unwrap_or_default()
                .split(',')
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect();
            Ok(AiProjectInventoryEntry {
                project: row.get(0)?,
                tools,
                event_count: row.get(2)?,
                session_count: row.get(3)?,
                first_seen: row.get(4)?,
                last_seen: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let truncated = truncate_to_limit(&mut projects, LIMIT);
    Ok(ListAiProjectsResult {
        total_projects,
        truncated,
        projects,
    })
}

fn count_grouped_rows(
    conn: &rusqlite::Connection,
    grouped_sql: &str,
    bindings: &[rusqlite::types::Value],
) -> Result<usize> {
    Ok(conn.query_row(
        &format!("SELECT COUNT(*) FROM ({grouped_sql})"),
        rusqlite::params_from_iter(bindings.iter()),
        |row| row.get::<_, i64>(0),
    )? as usize)
}

fn truncate_to_limit<T>(values: &mut Vec<T>, limit: usize) -> bool {
    let truncated = values.len() > limit;
    values.truncate(limit);
    truncated
}

fn normalized_cuss_terms(custom_terms: &[String]) -> Vec<String> {
    let source: Vec<String> = if custom_terms.is_empty() {
        DEFAULT_AI_CUSS_TERMS
            .iter()
            .map(|term| (*term).to_string())
            .collect()
    } else {
        custom_terms.to_vec()
    };

    let mut terms = source
        .into_iter()
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| {
            !term.is_empty()
                && term.len() <= 64
                && term
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        })
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    if terms.is_empty() {
        DEFAULT_AI_CUSS_TERMS
            .iter()
            .map(|term| (*term).to_string())
            .collect()
    } else {
        terms
    }
}

fn first_cuss_term(message: &str, terms: &[String]) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    terms
        .iter()
        .filter_map(|term| first_term_index(&lower, term).map(|idx| (idx, term)))
        .min_by_key(|(idx, _)| *idx)
        .map(|(_, term)| term.clone())
}

fn first_term_index(message: &str, term: &str) -> Option<usize> {
    let mut offset = 0usize;
    while let Some(relative) = message[offset..].find(term) {
        let start = offset + relative;
        let end = start + term.len();
        if is_cuss_boundary(message[..start].chars().next_back())
            && is_cuss_boundary(message[end..].chars().next())
        {
            return Some(start);
        }
        offset = end;
    }
    None
}

fn is_cuss_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
}

fn ai_session_context(
    conn: &rusqlite::Connection,
    entry: &LogEntry,
    before: u32,
    after: u32,
) -> Result<(Vec<LogEntry>, Vec<LogEntry>)> {
    let Some(tool) = entry.ai_tool.as_deref() else {
        return Ok((Vec::new(), Vec::new()));
    };
    let Some(project) = entry.ai_project.as_deref() else {
        return Ok((Vec::new(), Vec::new()));
    };
    let Some(session_id) = entry.ai_session_id.as_deref() else {
        return Ok((Vec::new(), Vec::new()));
    };

    let mut before_stmt = conn.prepare(
        "SELECT id, timestamp, hostname, facility, severity,
                app_name, process_id, message, received_at, source_ip,
                ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
         FROM logs
         WHERE hostname = ?1
           AND ai_tool = ?2
           AND ai_project = ?3
           AND ai_session_id = ?4
           AND (timestamp < ?5 OR (timestamp = ?5 AND id < ?6))
         ORDER BY timestamp DESC, id DESC
         LIMIT ?7",
    )?;
    let mut before_rows = before_stmt
        .query_map(
            params![
                &entry.hostname,
                tool,
                project,
                session_id,
                &entry.timestamp,
                entry.id,
                before
            ],
            map_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    before_rows.reverse();

    let mut after_stmt = conn.prepare(
        "SELECT id, timestamp, hostname, facility, severity,
                app_name, process_id, message, received_at, source_ip,
                ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
         FROM logs
         WHERE hostname = ?1
           AND ai_tool = ?2
           AND ai_project = ?3
           AND ai_session_id = ?4
           AND (timestamp > ?5 OR (timestamp = ?5 AND id > ?6))
         ORDER BY timestamp ASC, id ASC
         LIMIT ?7",
    )?;
    let after_rows = after_stmt
        .query_map(
            params![
                &entry.hostname,
                tool,
                project,
                session_id,
                &entry.timestamp,
                entry.id,
                after
            ],
            map_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok((before_rows, after_rows))
}

/// Get database stats
pub fn get_stats(pool: &DbPool, config: &StorageConfig) -> Result<DbStats> {
    let metrics = get_storage_metrics(pool, config)?;
    let write_blocked = exceeds_trigger(&metrics, config);
    let mut conn = pool.get()?;

    // Deferred read transaction ensures the log stats form a consistent snapshot
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)?;
    let total_logs: i64 = tx.query_row("SELECT COUNT(*) FROM logs", [], |r| r.get(0))?;
    let total_hosts: i64 = tx.query_row("SELECT COUNT(*) FROM hosts", [], |r| r.get(0))?;
    let fts_rows: i64 = tx
        .query_row("SELECT COUNT(*) FROM logs_fts", [], |r| r.get(0))
        .unwrap_or(0);
    let phantom_fts_rows = (fts_rows - total_logs).max(0);
    // MIN/MAX return a single nullable row; use get::<_, Option<_>> so NULL becomes
    // None while real query errors (e.g. missing table) still propagate via `?`.
    let oldest: Option<String> = tx.query_row("SELECT MIN(timestamp) FROM logs", [], |r| {
        r.get::<_, Option<String>>(0)
    })?;
    let newest: Option<String> = tx.query_row("SELECT MAX(timestamp) FROM logs", [], |r| {
        r.get::<_, Option<String>>(0)
    })?;
    tx.finish()?;

    Ok(DbStats {
        total_logs,
        total_hosts,
        oldest_log: oldest,
        newest_log: newest,
        logical_db_size_mb: format!("{:.2}", metrics.logical_db_size_bytes as f64 / 1_048_576.0),
        physical_db_size_mb: format!("{:.2}", metrics.physical_db_size_bytes as f64 / 1_048_576.0),
        free_disk_mb: metrics
            .free_disk_bytes
            .map(|bytes| format!("{:.2}", bytes as f64 / 1_048_576.0)),
        max_db_size_mb: config.max_db_size_mb,
        min_free_disk_mb: config.min_free_disk_mb,
        write_blocked,
        phantom_fts_rows,
    })
}

/// Syslog severity level names ordered by numeric value (0=emerg, 7=debug).
/// Used by both the MCP layer (for threshold filtering) and the syslog parser (for decoding).
pub const SEVERITY_LEVELS: &[&str] = &[
    "emerg", "alert", "crit", "err", "warning", "notice", "info", "debug",
];

/// Convert a severity name to its numeric syslog level (0=emerg, 7=debug).
/// Returns `None` for unrecognised names.
pub fn severity_to_num(s: &str) -> Option<u8> {
    SEVERITY_LEVELS
        .iter()
        .position(|&l| l == s)
        .map(|i| i as u8)
}

fn append_filters(
    sql: &mut String,
    bindings: &mut Vec<rusqlite::types::Value>,
    idx: &mut usize,
    params: &SearchParams,
) {
    if let Some(ref h) = params.hostname {
        sql.push_str(&format!(" AND l.hostname = ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(h.clone()));
        *idx += 1;
    }
    if let Some(ref source_ip) = params.source_ip {
        sql.push_str(&format!(" AND l.source_ip = ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(source_ip.clone()));
        *idx += 1;
    }
    if let Some(ref s) = params.severity {
        sql.push_str(&format!(" AND l.severity = ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(s.clone()));
        *idx += 1;
    }
    if let Some(ref levels) = params.severity_in {
        if !levels.is_empty() {
            let placeholders: Vec<String> = levels
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", *idx + i))
                .collect();
            sql.push_str(&format!(" AND l.severity IN ({})", placeholders.join(", ")));
            for level in levels {
                bindings.push(rusqlite::types::Value::Text(level.clone()));
                *idx += 1;
            }
        }
    }
    if let Some(ref a) = params.app_name {
        sql.push_str(&format!(" AND l.app_name = ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(a.clone()));
        *idx += 1;
    }
    if let Some(ref f) = params.facility {
        sql.push_str(&format!(" AND l.facility = ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(f.clone()));
        *idx += 1;
    }
    if let Some(ref pid) = params.process_id {
        sql.push_str(&format!(" AND l.process_id = ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(pid.clone()));
        *idx += 1;
    }
    if let Some(ref from) = params.from {
        sql.push_str(&format!(" AND l.timestamp >= ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(from.clone()));
        *idx += 1;
    }
    if let Some(ref to) = params.to {
        sql.push_str(&format!(" AND l.timestamp <= ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(to.clone()));
        *idx += 1;
    }
    if let Some(ref tool) = params.ai_tool {
        sql.push_str(&format!(" AND l.ai_tool = ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(tool.clone()));
        *idx += 1;
    }
    if let Some(ref project) = params.ai_project {
        sql.push_str(&format!(" AND l.ai_project = ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(project.clone()));
        *idx += 1;
    }
    if let Some(ref session_id) = params.ai_session_id {
        sql.push_str(&format!(" AND l.ai_session_id = ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(session_id.clone()));
        *idx += 1;
    }
    if params.exclude_ai {
        sql.push_str(
            " AND (l.ai_project IS NULL OR l.ai_project = '')
              AND (l.ai_tool IS NULL OR l.ai_tool = '')
              AND (l.ai_session_id IS NULL OR l.ai_session_id = '')
              AND (l.ai_transcript_path IS NULL OR l.ai_transcript_path = '')
              AND (
                l.app_name IS NULL
                OR l.app_name NOT IN (
                    'ai-transcript',
                    'claude-transcript',
                    'codex-transcript',
                    'gemini-transcript'
                )
              )",
        );
    }
}

pub(super) fn map_row(row: &rusqlite::Row) -> rusqlite::Result<LogEntry> {
    Ok(LogEntry {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        hostname: row.get(2)?,
        facility: row.get(3)?,
        severity: row.get(4)?,
        app_name: row.get(5)?,
        process_id: row.get(6)?,
        message: row.get(7)?,
        received_at: row.get(8)?,
        source_ip: row.get(9)?,
        ai_tool: row.get(10)?,
        ai_project: row.get(11)?,
        ai_session_id: row.get(12)?,
        ai_transcript_path: row.get(13)?,
        metadata_json: row.get(14)?,
    })
}

/// Map a row that includes the unparsed `raw` syslog frame (column index 8).
pub(super) fn map_row_with_raw(
    row: &rusqlite::Row,
) -> rusqlite::Result<super::analytics::LogEntryWithRaw> {
    Ok(super::analytics::LogEntryWithRaw {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        hostname: row.get(2)?,
        facility: row.get(3)?,
        severity: row.get(4)?,
        app_name: row.get(5)?,
        process_id: row.get(6)?,
        message: row.get(7)?,
        raw: row.get(8)?,
        received_at: row.get(9)?,
        source_ip: row.get(10)?,
        ai_tool: row.get(11)?,
        ai_project: row.get(12)?,
        ai_session_id: row.get(13)?,
        ai_transcript_path: row.get(14)?,
        metadata_json: row.get(15)?,
    })
}

#[cfg(test)]
#[path = "queries_tests.rs"]
mod tests;
