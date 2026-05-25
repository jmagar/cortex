use anyhow::Result;
use rusqlite::params;

use crate::config::StorageConfig;

use super::maintenance::{exceeds_trigger, get_storage_metrics};
use super::models::{
    AbuseIncident, AiAbuseMatch, AiAbuseParams, AiAbuseResult, AiCorrelateParams, AiIncidentParams,
    AiIncidentResult, AiInvestigateParams, AiInvestigateResult, AiProjectInventoryEntry,
    AiRelatedLogsForAnchor, AiRelatedLogsParams, AiSessionEntry, AiToolInventoryEntry, DbStats,
    ErrorSummaryEntry, HostEntry, IncidentEvidence, ListAiProjectsParams, ListAiProjectsResult,
    ListAiSessionsParams, ListAiToolsParams, ListAiToolsResult, LogEntry, SearchAiSessionsParams,
    SearchAiSessionsResult, SearchParams, SearchedAiSessionEntry,
};
use super::pool::DbPool;

const SEARCH_FTS_CANDIDATE_CAP: usize = 10_000;
const SIMILAR_INCIDENT_FTS_CANDIDATE_CAP: usize = 5_000;

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

fn search_logs_fts_sql(
    query: &str,
    params: &SearchParams,
    limit: u32,
) -> (String, Vec<rusqlite::types::Value>) {
    let mut sql = String::from(
        "WITH fts_candidates(id, ts) AS MATERIALIZED (
            SELECT l.id, l.timestamp
            FROM logs_fts
            JOIN logs l ON l.id = logs_fts.rowid
            WHERE logs_fts MATCH ?1",
    );
    let mut bindings: Vec<rusqlite::types::Value> =
        vec![rusqlite::types::Value::Text(query.to_string())];
    let mut idx = 2;

    append_filters(&mut sql, &mut bindings, &mut idx, params);
    sql.push_str(&format!(
        " ORDER BY logs_fts.rowid DESC
          LIMIT {SEARCH_FTS_CANDIDATE_CAP}
         )
         SELECT l.id, l.timestamp, l.hostname, l.facility, l.severity,
                l.app_name, l.process_id, l.message, l.received_at, l.source_ip,
                l.ai_tool, l.ai_project, l.ai_session_id, l.ai_transcript_path, l.metadata_json
         FROM fts_candidates c
         JOIN logs l ON l.id = c.id
         ORDER BY c.ts DESC, l.id DESC
         LIMIT {limit}"
    ));
    (sql, bindings)
}

#[derive(Debug, Default)]
struct SqlParams {
    bindings: Vec<rusqlite::types::Value>,
    next_idx: usize,
}

impl SqlParams {
    fn new(next_idx: usize) -> Self {
        Self {
            bindings: Vec::new(),
            next_idx,
        }
    }

    fn push_text(&mut self, value: String) -> usize {
        let idx = self.next_idx;
        self.bindings.push(rusqlite::types::Value::Text(value));
        self.next_idx += 1;
        idx
    }

    fn push_i64(&mut self, value: i64) -> usize {
        let idx = self.next_idx;
        self.bindings.push(rusqlite::types::Value::Integer(value));
        self.next_idx += 1;
        idx
    }
}

fn push_required_ai_filters(sql: &mut String, alias: &str) {
    sql.push_str(&format!(
        " AND {alias}.ai_project IS NOT NULL AND {alias}.ai_project != ''
          AND {alias}.ai_tool IS NOT NULL AND {alias}.ai_tool != ''
          AND {alias}.ai_session_id IS NOT NULL AND {alias}.ai_session_id != ''"
    ));
}

fn push_ai_scope_filters(
    sql: &mut String,
    params: &mut SqlParams,
    alias: &str,
    project: &Option<String>,
    tool: &Option<String>,
    from: &Option<String>,
    to: &Option<String>,
) {
    if let Some(project) = project {
        let idx = params.push_text(project.clone());
        sql.push_str(&format!(" AND {alias}.ai_project = ?{idx}"));
    }
    if let Some(tool) = tool {
        let idx = params.push_text(tool.clone());
        sql.push_str(&format!(" AND {alias}.ai_tool = ?{idx}"));
    }
    if let Some(from) = from {
        let idx = params.push_text(from.clone());
        sql.push_str(&format!(" AND {alias}.timestamp >= ?{idx}"));
    }
    if let Some(to) = to {
        let idx = params.push_text(to.clone());
        sql.push_str(&format!(" AND {alias}.timestamp <= ?{idx}"));
    }
}

/// Search logs with flexible filtering + FTS
pub fn search_logs(pool: &DbPool, params: &SearchParams) -> Result<Vec<LogEntry>> {
    let conn = pool.get()?;
    let limit = params.limit.unwrap_or(100).min(1000);

    // If we have a full-text query, use FTS5 join
    if let Some(ref query) = params.query {
        validate_fts_query(query)?;

        let (sql, bindings) = search_logs_fts_sql(query, params, limit);

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
    limit: Option<u32>,
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
        let mut rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        if let Some(limit) = limit {
            rows.truncate(limit as usize);
        }
        Ok(rows)
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
        let mut rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        if let Some(limit) = limit {
            rows.truncate(limit as usize);
        }
        Ok(rows)
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
    let (sql, bindings) = search_ai_sessions_sql(params, limit);

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

const CANDIDATE_CAP: usize = 5_000;

fn search_ai_sessions_sql(
    params: &SearchAiSessionsParams,
    limit: usize,
) -> (String, Vec<rusqlite::types::Value>) {
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
            WHERE logs_fts MATCH ?1",
    );
    push_required_ai_filters(&mut sql, "l");
    let mut query_params = SqlParams::new(2);
    query_params
        .bindings
        .push(rusqlite::types::Value::Text(params.query.clone()));
    push_ai_scope_filters(
        &mut sql,
        &mut query_params,
        "l",
        &params.ai_project,
        &params.ai_tool,
        &params.from,
        &params.to,
    );
    if let Some(hostname) = &params.hostname {
        let idx = query_params.push_text(hostname.clone());
        sql.push_str(&format!(" AND l.hostname = ?{idx}"));
    }
    if let Some(app_name) = &params.app_name {
        let idx = query_params.push_text(app_name.clone());
        sql.push_str(&format!(" AND l.app_name = ?{idx}"));
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
         ),
         event_counts AS (
            SELECT l.ai_project,
                   l.ai_tool,
                   l.ai_session_id,
                   l.hostname,
                   COUNT(*) AS event_count
            FROM logs l
            JOIN grouped g
              ON g.ai_project = l.ai_project
             AND g.ai_tool = l.ai_tool
             AND g.ai_session_id = l.ai_session_id
             AND g.hostname = l.hostname
            GROUP BY l.ai_project, l.ai_tool, l.ai_session_id, l.hostname
         )
         SELECT g.ai_project, g.ai_tool, g.ai_session_id, g.hostname,
                g.first_seen,
                g.last_seen,
                COALESCE(ec.event_count, 0) AS event_count,
                g.match_count,
                g.best_snippet,
                COUNT(*) OVER() AS total_candidates,
                (SELECT COUNT(*) FROM candidates) AS raw_candidate_count
         FROM grouped g
         LEFT JOIN event_counts ec
           ON ec.ai_project = g.ai_project
          AND ec.ai_tool = g.ai_tool
          AND ec.ai_session_id = g.ai_session_id
          AND ec.hostname = g.hostname
         ORDER BY g.last_seen DESC
         LIMIT {limit}",
        CANDIDATE_CAP + 1
    ));
    (sql, query_params.bindings)
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
             WHERE logs_fts MATCH ?1",
        )
    } else {
        String::from(
            "SELECT l.id, l.timestamp, l.hostname, l.facility, l.severity,
                    l.app_name, l.process_id, l.message, l.received_at, l.source_ip,
                    l.ai_tool, l.ai_project, l.ai_session_id, l.ai_transcript_path, l.metadata_json
             FROM logs l
             WHERE 1=1",
        )
    };

    push_required_ai_filters(&mut sql, "l");
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

pub fn search_ai_related_logs(
    pool: &DbPool,
    params: &AiRelatedLogsParams,
) -> Result<Vec<AiRelatedLogsForAnchor>> {
    if params.windows.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(query) = &params.query {
        validate_fts_query(query)?;
    }

    let conn = pool.get()?;
    let limit = params.limit_per_anchor.clamp(1, 200) as usize;
    let mut sql_params = SqlParams::new(1);
    let values = params
        .windows
        .iter()
        .map(|window| {
            let anchor_idx = sql_params.push_i64(window.anchor_index as i64);
            let from_idx = sql_params.push_text(window.window_from.clone());
            let to_idx = sql_params.push_text(window.window_to.clone());
            format!("(?{anchor_idx}, ?{from_idx}, ?{to_idx})")
        })
        .collect::<Vec<_>>()
        .join(", ");

    let mut sql = format!(
        "WITH windows(anchor_index, window_from, window_to) AS (VALUES {values}),
         ranked AS (
            SELECT w.anchor_index,
                   l.id, l.timestamp, l.hostname, l.facility, l.severity,
                   l.app_name, l.process_id, l.message, l.received_at, l.source_ip,
                   l.ai_tool, l.ai_project, l.ai_session_id, l.ai_transcript_path, l.metadata_json,
                   ROW_NUMBER() OVER (
                       PARTITION BY w.anchor_index
                       ORDER BY l.timestamp DESC, l.id DESC
                   ) AS related_rank
            FROM windows w"
    );
    if let Some(query) = &params.query {
        let query_idx = sql_params.push_text(query.clone());
        sql.push_str(
            "
            JOIN logs_fts ON logs_fts MATCH ?",
        );
        sql.push_str(&query_idx.to_string());
        sql.push_str(
            "
            JOIN logs l ON l.id = logs_fts.rowid
             AND l.timestamp >= w.window_from
             AND l.timestamp <= w.window_to",
        );
    } else {
        sql.push_str(
            "
            JOIN logs l
              ON l.timestamp >= w.window_from
             AND l.timestamp <= w.window_to",
        );
    }
    sql.push_str(" WHERE 1=1");

    let search_params = SearchParams {
        query: None,
        hostname: params.hostname.clone(),
        source_ip: params.source_ip.clone(),
        source_ip_prefix: None,
        severity: None,
        severity_in: Some(params.severity_in.clone()),
        app_name: params.app_name.clone(),
        facility: None,
        exclude_facility: None,
        process_id: None,
        from: None,
        to: None,
        received_from: None,
        received_to: None,
        limit: None,
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        event_action: None,
        exclude_ai: true,
    };
    append_filters(
        &mut sql,
        &mut sql_params.bindings,
        &mut sql_params.next_idx,
        &search_params,
    );
    sql.push_str(&format!(
        "
         )
         SELECT anchor_index, id, timestamp, hostname, facility, severity,
                app_name, process_id, message, received_at, source_ip,
                ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json,
                related_rank
         FROM ranked
         WHERE related_rank <= {}
         ORDER BY anchor_index ASC, related_rank ASC",
        limit + 1
    ));

    let mut grouped = params
        .windows
        .iter()
        .map(|window| AiRelatedLogsForAnchor {
            anchor_index: window.anchor_index,
            logs: Vec::new(),
            truncated: false,
        })
        .collect::<Vec<_>>();
    let mut anchor_pos: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::with_capacity(grouped.len());
    for (pos, group) in grouped.iter().enumerate() {
        if anchor_pos.insert(group.anchor_index, pos).is_some() {
            anyhow::bail!(
                "duplicate anchor_index {} in AiRelatedLogsParams windows",
                group.anchor_index
            );
        }
    }
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(sql_params.bindings.iter()))?;
    while let Some(row) = rows.next()? {
        let anchor_index = row.get::<_, i64>(0)? as usize;
        let related_rank = row.get::<_, i64>(16)? as usize;
        if let Some(&pos) = anchor_pos.get(&anchor_index) {
            let anchor = &mut grouped[pos];
            if related_rank > limit {
                anchor.truncated = true;
            } else {
                anchor.logs.push(map_row_offset(row, 1)?);
            }
        }
    }

    Ok(grouped)
}

const DEFAULT_AI_ABUSE_TERMS: &[&str] = &[
    "asshole", "bastard", "bitch", "biznitch", "bullshit", "crap", "damn", "dick", "fuck",
    "fucked", "fucker", "fucking", "hell", "piss", "shit", "shitty",
];

pub fn search_ai_abuse(pool: &DbPool, params: &AiAbuseParams) -> Result<AiAbuseResult> {
    let conn = pool.get()?;
    let limit = params.limit.unwrap_or(20).clamp(1, 100) as usize;
    let before = params.before.unwrap_or(2).min(20);
    let after = params.after.unwrap_or(2).min(20);
    let terms = normalized_abuse_terms(&params.terms);
    const CANDIDATE_CAP: usize = 10_000;

    let mut sql = String::from(
        "WITH candidates(id) AS MATERIALIZED (
            SELECT l.id
            FROM logs_fts
            JOIN logs l ON l.id = logs_fts.rowid
            WHERE logs_fts MATCH ?1
              AND l.ai_project IS NOT NULL AND l.ai_project != ''
              AND l.ai_tool IS NOT NULL AND l.ai_tool != ''
              AND l.ai_session_id IS NOT NULL AND l.ai_session_id != ''",
    );
    let mut bindings = vec![rusqlite::types::Value::Text(abuse_fts_query(&terms))];
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
        " ORDER BY logs_fts.rowid DESC LIMIT {}
         )
         SELECT l.id, l.timestamp, l.hostname, l.facility, l.severity,
                l.app_name, l.process_id, l.message, l.received_at, l.source_ip,
                l.ai_tool, l.ai_project, l.ai_session_id, l.ai_transcript_path, l.metadata_json
         FROM candidates c
         JOIN logs l ON l.id = c.id
         ORDER BY l.timestamp DESC, l.id DESC",
        CANDIDATE_CAP + 1
    ));

    let mut stmt = conn.prepare(&sql)?;
    let candidate_rows = stmt
        .query_map(rusqlite::params_from_iter(bindings.iter()), map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let candidate_window_truncated = candidate_rows.len() > CANDIDATE_CAP;
    let mut matches = Vec::new();
    let mut result_limit_truncated = false;
    for entry in candidate_rows.iter().take(CANDIDATE_CAP) {
        if let Some(term) = first_abuse_term(&entry.message, &terms) {
            if matches.len() == limit {
                result_limit_truncated = true;
                break;
            }
            let (before_rows, after_rows) = ai_session_context(&conn, entry, before, after)?;
            matches.push(AiAbuseMatch {
                term,
                entry: entry.clone(),
                before: before_rows,
                after: after_rows,
            });
        }
    }

    Ok(AiAbuseResult {
        terms,
        candidate_rows: candidate_rows.len().min(CANDIDATE_CAP),
        candidate_cap: CANDIDATE_CAP,
        candidate_window_truncated,
        truncated: candidate_window_truncated || result_limit_truncated,
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
    sql.push_str(&format!(
        " GROUP BY ai_tool ORDER BY event_count DESC, ai_tool ASC LIMIT {}",
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
        total_tools: tools.len(),
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
    sql.push_str(&format!(
        " GROUP BY ai_project ORDER BY event_count DESC, ai_project ASC LIMIT {}",
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
        total_projects: projects.len(),
        truncated,
        projects,
    })
}

fn truncate_to_limit<T>(values: &mut Vec<T>, limit: usize) -> bool {
    let truncated = values.len() > limit;
    values.truncate(limit);
    truncated
}

pub fn search_ai_incidents(pool: &DbPool, params: &AiIncidentParams) -> Result<AiIncidentResult> {
    use std::collections::HashMap;

    let conn = pool.get()?;
    let limit = params.limit.unwrap_or(20).clamp(1, 100) as usize;
    let window_secs = i64::from(params.window_minutes.unwrap_or(10).clamp(1, 120)) * 60;
    let terms = normalized_abuse_terms(&params.terms);
    const CANDIDATE_CAP: usize = 10_000;

    let (sql, bindings) = ai_incident_anchor_sql(params, &terms, CANDIDATE_CAP);

    // Fetch candidate abuse anchor rows (same FTS path as search_ai_abuse,
    // no per-hit context needed here).
    struct AnchorRow {
        id: i64,
        timestamp: String,
        hostname: String,
        tool: String,
        project: String,
        session_id: String,
        message: String,
    }

    let mut stmt = conn.prepare(&sql)?;
    let candidate_rows: Vec<AnchorRow> = stmt
        .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
            Ok(AnchorRow {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                hostname: row.get(2)?,
                tool: row.get(3)?,
                project: row.get(4)?,
                session_id: row.get(5)?,
                message: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let candidate_window_truncated = candidate_rows.len() > CANDIDATE_CAP;
    let raw_candidate_count = candidate_rows.len();

    // Group by (project, tool, session_id, hostname) + window-minute buckets.
    // Key: (project, tool, session_id, hostname, window_bucket)
    // window_bucket = unix_secs / window_secs * window_secs (floor to window boundary)
    type GroupKey = (String, String, String, String, i64);
    let mut groups: HashMap<GroupKey, Vec<&AnchorRow>> = HashMap::new();

    for row in candidate_rows.iter().take(CANDIDATE_CAP) {
        // Parse timestamp to unix seconds for bucketing.
        let bucket = chrono::DateTime::parse_from_rfc3339(&row.timestamp)
            .map(|dt| {
                let secs = dt.timestamp();
                (secs / window_secs) * window_secs
            })
            .unwrap_or(0);
        let key = (
            row.project.clone(),
            row.tool.clone(),
            row.session_id.clone(),
            row.hostname.clone(),
            bucket,
        );
        groups.entry(key).or_default().push(row);
    }

    // Build incidents from groups.
    let mut incidents: Vec<AbuseIncident> = groups
        .into_iter()
        .map(
            |((project, tool, session_id, hostname, _bucket), anchors)| {
                let abuse_count = anchors.len();
                let first_seen = anchors
                    .first()
                    .map(|r| r.timestamp.clone())
                    .unwrap_or_default();
                let last_seen = anchors
                    .last()
                    .map(|r| r.timestamp.clone())
                    .unwrap_or_default();

                // duration in seconds
                let duration_secs = {
                    let t0 = chrono::DateTime::parse_from_rfc3339(&first_seen)
                        .map(|dt| dt.timestamp())
                        .unwrap_or(0);
                    let t1 = chrono::DateTime::parse_from_rfc3339(&last_seen)
                        .map(|dt| dt.timestamp())
                        .unwrap_or(0);
                    (t1 - t0).max(0)
                };

                // Collect unique terms found in this group's messages.
                let mut found_terms: Vec<String> = terms
                    .iter()
                    .filter(|term| {
                        anchors.iter().any(|r| {
                            first_abuse_term(&r.message, std::slice::from_ref(term)).is_some()
                        })
                    })
                    .cloned()
                    .collect();
                found_terms.sort();
                found_terms.dedup();

                let mut anchor_ids: Vec<i64> = anchors.iter().map(|r| r.id).collect();
                anchor_ids.sort();

                // Score: abuse_count dominates; density and term variety boost.
                let density = if duration_secs > 0 {
                    abuse_count as f64 / (duration_secs as f64 / 60.0)
                } else {
                    abuse_count as f64
                };
                let term_variety = found_terms.len() as f64;
                let priority_score = abuse_count as f64 * 10.0 + density * 2.0 + term_variety;

                let priority_label = match priority_score as u64 {
                    0..=14 => "low",
                    15..=29 => "medium",
                    30..=49 => "high",
                    _ => "critical",
                }
                .to_string();

                // Stable incident ID using a deterministic hash of session identity + anchor IDs.
                let incident_id = {
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut h = DefaultHasher::new();
                    project.hash(&mut h);
                    tool.hash(&mut h);
                    session_id.hash(&mut h);
                    hostname.hash(&mut h);
                    for id in &anchor_ids {
                        id.hash(&mut h);
                    }
                    format!("inc-{:016x}", h.finish())
                };

                AbuseIncident {
                    incident_id,
                    project,
                    tool,
                    session_id,
                    hostname,
                    first_seen,
                    last_seen,
                    duration_secs,
                    abuse_count,
                    terms: found_terms,
                    anchor_ids,
                    priority_score,
                    priority_label,
                    window_minutes: (window_secs / 60) as u32,
                }
            },
        )
        .collect();

    // Sort by priority_score descending, then last_seen descending.
    incidents.sort_by(|a, b| {
        b.priority_score
            .partial_cmp(&a.priority_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.last_seen.cmp(&a.last_seen))
    });

    let total_incidents = incidents.len();
    let truncated = total_incidents > limit || candidate_window_truncated;
    incidents.truncate(limit);

    Ok(AiIncidentResult {
        incidents,
        total_incidents,
        candidate_rows: raw_candidate_count.min(CANDIDATE_CAP),
        candidate_cap: CANDIDATE_CAP,
        candidate_window_truncated,
        truncated,
    })
}

fn ai_incident_anchor_sql(
    params: &AiIncidentParams,
    terms: &[String],
    candidate_cap: usize,
) -> (String, Vec<rusqlite::types::Value>) {
    let mut sql = String::from(
        "WITH candidates(id) AS MATERIALIZED (
            SELECT l.id
            FROM logs_fts
            JOIN logs l ON l.id = logs_fts.rowid
            WHERE logs_fts MATCH ?1
              AND l.ai_project IS NOT NULL AND l.ai_project != ''
              AND l.ai_tool IS NOT NULL AND l.ai_tool != ''
              AND l.ai_session_id IS NOT NULL AND l.ai_session_id != ''",
    );
    let mut bindings = vec![rusqlite::types::Value::Text(abuse_fts_query(terms))];
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
    let _ = idx;
    sql.push_str(&format!(
        " ORDER BY logs_fts.rowid ASC LIMIT {}
         )
         SELECT l.id, l.timestamp, l.hostname,
                l.ai_tool, l.ai_project, l.ai_session_id, l.message
         FROM candidates c
         JOIN logs l ON l.id = c.id",
        candidate_cap + 1
    ));
    (sql, bindings)
}

pub fn investigate_ai_incidents(
    pool: &DbPool,
    params: &AiInvestigateParams,
) -> Result<AiInvestigateResult> {
    let limit = params.limit.unwrap_or(3).clamp(1, 10) as usize;
    let incident_lookup_limit = if params.incident_id.is_some() {
        100
    } else {
        limit as u32
    };
    let corr_mins = i64::from(params.correlation_window_minutes.unwrap_or(5).clamp(1, 120));

    // Reuse incident grouping to find the top incidents. Exact incident
    // assessment may target an ID outside the top investigation page, so it
    // searches up to the incident-list cap and then builds one evidence bundle.
    let incident_result = search_ai_incidents(
        pool,
        &AiIncidentParams {
            ai_project: params.ai_project.clone(),
            ai_tool: params.ai_tool.clone(),
            from: params.from.clone(),
            to: params.to.clone(),
            limit: Some(incident_lookup_limit),
            window_minutes: params.window_minutes,
            terms: params.terms.clone(),
        },
    )?;
    let total_incidents = incident_result.total_incidents;
    let truncated = incident_result.truncated;
    let incidents = if let Some(incident_id) = &params.incident_id {
        incident_result
            .incidents
            .into_iter()
            .filter(|incident| incident.incident_id == *incident_id)
            .collect()
    } else {
        incident_result.incidents
    };

    let conn = pool.get()?;
    let mut evidence = Vec::with_capacity(incidents.len());

    for incident in incidents {
        const TRANSCRIPT_CAP: usize = 20;
        const NEARBY_CAP: usize = 50;

        // Fetch anchor log entries.
        let anchors = if incident.anchor_ids.is_empty() {
            Vec::new()
        } else {
            let placeholders: Vec<String> = (1..=incident.anchor_ids.len())
                .map(|i| format!("?{i}"))
                .collect();
            let sql = format!(
                "SELECT id, timestamp, hostname, facility, severity, app_name,
                        process_id, message, received_at, source_ip,
                        ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
                 FROM logs WHERE id IN ({}) ORDER BY timestamp ASC",
                placeholders.join(",")
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(
                    rusqlite::params_from_iter(
                        incident
                            .anchor_ids
                            .iter()
                            .map(|id| rusqlite::types::Value::Integer(*id)),
                    ),
                    map_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };

        // Transcript context: entries in the same session before first anchor and after last anchor.
        let (transcript_before, transcript_before_truncated) = if let Some(first) = anchors.first()
        {
            let rows = {
                let mut stmt = conn.prepare(
                    "SELECT id, timestamp, hostname, facility, severity, app_name,
                                process_id, message, received_at, source_ip,
                                ai_tool, ai_project, ai_session_id, ai_transcript_path,
                                metadata_json
                         FROM logs
                         WHERE ai_session_id = ?1 AND ai_project = ?2 AND ai_tool = ?3
                           AND timestamp < ?4
                         ORDER BY timestamp DESC
                         LIMIT 21",
                )?;
                let rows = stmt
                    .query_map(
                        rusqlite::params![
                            &incident.session_id,
                            &incident.project,
                            &incident.tool,
                            &first.timestamp,
                        ],
                        map_row,
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };
            let truncated = rows.len() > TRANSCRIPT_CAP;
            let mut out = rows;
            out.truncate(TRANSCRIPT_CAP);
            out.reverse(); // chronological order
            (out, truncated)
        } else {
            (Vec::new(), false)
        };

        let (transcript_after, transcript_after_truncated) = if let Some(last) = anchors.last() {
            let rows = {
                let mut stmt = conn.prepare(
                    "SELECT id, timestamp, hostname, facility, severity, app_name,
                            process_id, message, received_at, source_ip,
                            ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
                     FROM logs
                     WHERE ai_session_id = ?1 AND ai_project = ?2 AND ai_tool = ?3
                       AND timestamp > ?4
                     ORDER BY timestamp ASC
                     LIMIT 21",
                )?;
                let rows = stmt
                    .query_map(
                        rusqlite::params![
                            &incident.session_id,
                            &incident.project,
                            &incident.tool,
                            &last.timestamp,
                        ],
                        map_row,
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };
            let truncated = rows.len() > TRANSCRIPT_CAP;
            let mut out = rows;
            out.truncate(TRANSCRIPT_CAP);
            (out, truncated)
        } else {
            (Vec::new(), false)
        };

        // Nearby non-AI logs in the correlation window.
        let (nearby_logs, nearby_logs_truncated) = {
            // Window: corr_mins before first_seen through corr_mins after last_seen.
            let win_from = chrono::DateTime::parse_from_rfc3339(&incident.first_seen)
                .map(|dt| {
                    use chrono::Duration;
                    (dt.with_timezone(&chrono::Utc) - Duration::minutes(corr_mins))
                        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                        .to_string()
                })
                .unwrap_or_else(|_| incident.first_seen.clone());
            let win_to = chrono::DateTime::parse_from_rfc3339(&incident.last_seen)
                .map(|dt| {
                    use chrono::Duration;
                    (dt.with_timezone(&chrono::Utc) + Duration::minutes(corr_mins))
                        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                        .to_string()
                })
                .unwrap_or_else(|_| incident.last_seen.clone());

            let mut stmt = conn.prepare(
                "SELECT id, timestamp, hostname, facility, severity, app_name,
                        process_id, message, received_at, source_ip,
                        ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
                 FROM logs
                 WHERE timestamp >= ?1 AND timestamp <= ?2
                   AND (ai_project IS NULL OR ai_project = '')
                 ORDER BY timestamp ASC
                 LIMIT 51",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![win_from, win_to], map_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let truncated = rows.len() > NEARBY_CAP;
            let mut out = rows;
            out.truncate(NEARBY_CAP);
            (out, truncated)
        };

        // Nearby errors: subset of nearby_logs with severity warning+.
        let error_sevs = ["emergency", "alert", "critical", "error", "warning"];
        let nearby_errors: Vec<LogEntry> = nearby_logs
            .iter()
            .filter(|e| error_sevs.contains(&e.severity.as_str()))
            .cloned()
            .collect();

        evidence.push(IncidentEvidence {
            incident,
            transcript_before,
            transcript_before_truncated,
            transcript_after,
            transcript_after_truncated,
            anchors,
            nearby_logs,
            nearby_logs_truncated,
            nearby_errors,
        });
    }

    Ok(AiInvestigateResult {
        evidence,
        total_incidents,
        truncated,
    })
}

fn normalized_abuse_terms(custom_terms: &[String]) -> Vec<String> {
    let source: Vec<String> = if custom_terms.is_empty() {
        DEFAULT_AI_ABUSE_TERMS
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
        DEFAULT_AI_ABUSE_TERMS
            .iter()
            .map(|term| (*term).to_string())
            .collect()
    } else {
        terms
    }
}

fn abuse_fts_query(terms: &[String]) -> String {
    terms
        .iter()
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn first_abuse_term(message: &str, terms: &[String]) -> Option<String> {
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
        if is_abuse_boundary(message[..start].chars().next_back())
            && is_abuse_boundary(message[end..].chars().next())
        {
            return Some(start);
        }
        offset = end;
    }
    None
}

fn is_abuse_boundary(ch: Option<char>) -> bool {
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
/// Accepts the canonical RFC 5424 keywords (case-insensitive) plus common
/// aliases: `error`/`fatal`/`panic` for `err`, `warn` for `warning`,
/// `critical` for `crit`, `emergency` for `emerg`.
/// Returns `None` for unrecognised names.
pub fn severity_to_num(s: &str) -> Option<u8> {
    let canonical = match s.to_ascii_lowercase().as_str() {
        "emergency" => "emerg",
        "critical" => "crit",
        "error" | "fatal" | "panic" => "err",
        "warn" => "warning",
        other => {
            return SEVERITY_LEVELS
                .iter()
                .position(|&l| l == other)
                .map(|i| i as u8)
        }
    };
    SEVERITY_LEVELS
        .iter()
        .position(|&l| l == canonical)
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
    if let Some(ref prefix) = params.source_ip_prefix {
        sql.push_str(&format!(" AND l.source_ip >= ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(prefix.clone()));
        *idx += 1;
        if let Some(upper) = prefix_upper_bound(prefix) {
            sql.push_str(&format!(" AND l.source_ip < ?{}", *idx));
            bindings.push(rusqlite::types::Value::Text(upper));
            *idx += 1;
        }
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
    if let Some(ref f) = params.exclude_facility {
        sql.push_str(&format!(
            " AND (l.facility IS NULL OR l.facility != ?{})",
            *idx
        ));
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
    if let Some(ref from) = params.received_from {
        sql.push_str(&format!(" AND l.received_at >= ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(from.clone()));
        *idx += 1;
    }
    if let Some(ref to) = params.received_to {
        sql.push_str(&format!(" AND l.received_at <= ?{}", *idx));
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
    if let Some(ref event_action) = params.event_action {
        sql.push_str(&format!(" AND l.event_action = ?{}", *idx));
        bindings.push(rusqlite::types::Value::Text(event_action.clone()));
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

fn prefix_upper_bound(prefix: &str) -> Option<String> {
    let mut bytes = prefix.as_bytes().to_vec();
    for idx in (0..bytes.len()).rev() {
        if bytes[idx] != u8::MAX {
            bytes[idx] += 1;
            bytes.truncate(idx + 1);
            return String::from_utf8(bytes).ok();
        }
    }
    None
}

pub(super) fn map_row(row: &rusqlite::Row) -> rusqlite::Result<LogEntry> {
    map_row_offset(row, 0)
}

fn map_row_offset(row: &rusqlite::Row, offset: usize) -> rusqlite::Result<LogEntry> {
    Ok(LogEntry {
        id: row.get(offset)?,
        timestamp: row.get(offset + 1)?,
        hostname: row.get(offset + 2)?,
        facility: row.get(offset + 3)?,
        severity: row.get(offset + 4)?,
        app_name: row.get(offset + 5)?,
        process_id: row.get(offset + 6)?,
        message: row.get(offset + 7)?,
        received_at: row.get(offset + 8)?,
        source_ip: row.get(offset + 9)?,
        ai_tool: row.get(offset + 10)?,
        ai_project: row.get(offset + 11)?,
        ai_session_id: row.get(offset + 12)?,
        ai_transcript_path: row.get(offset + 13)?,
        metadata_json: row.get(offset + 14)?,
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

// ---------------------------------------------------------------------------
// RAG v1: similar_incidents, ask_history, incident_context
// ---------------------------------------------------------------------------

use super::models::{
    AppLogCount, AskHistoryParams, AskHistoryResult, CorrelatedSession, IncidentCluster,
    IncidentContextParams, IncidentContextResult, SeverityCount, SimilarIncidentsParams,
    SimilarIncidentsResult,
};

/// Return incident clusters from FTS5 hits, grouped by hostname + app_name in
/// non-overlapping windows of `window_minutes` minutes (default 30).
///
/// Algorithm:
/// 1. FTS5 MATCH over non-AI log rows (optionally filtered by host/app/time).
/// 2. Group hits by (hostname, app_name, floor(unix_epoch / window_secs)).
/// 3. For each cluster: derive severity_peak (min numeric rank = highest sev),
///    collect up to 3 representative message snippets, and look up correlated
///    AI sessions whose transcript timestamps overlap the cluster window.
pub fn similar_incidents_clusters(
    pool: &DbPool,
    params: &SimilarIncidentsParams,
) -> Result<SimilarIncidentsResult> {
    validate_fts_query(&params.query)?;

    let conn = pool.get()?;
    let window_minutes = params.window_minutes.unwrap_or(30).clamp(5, 120);
    let limit = params.limit.unwrap_or(10).clamp(1, 50) as usize;
    let window_secs = i64::from(window_minutes) * 60;

    // Build the FTS5 + optional filter query.
    // Exclude AI transcript rows so clusters contain only system logs.
    let mut sql = String::from(
        "WITH hits AS (
            SELECT l.id, l.timestamp, l.hostname, l.app_name, l.severity, l.message
            FROM logs_fts
            JOIN logs l ON l.id = logs_fts.rowid
            WHERE logs_fts MATCH ?1
              AND (l.ai_project IS NULL OR l.ai_project = '')",
    );

    let mut query_params = SqlParams::new(2);
    query_params
        .bindings
        .push(rusqlite::types::Value::Text(params.query.clone()));

    if let Some(hostname) = &params.hostname {
        let idx = query_params.push_text(hostname.clone());
        sql.push_str(&format!(" AND l.hostname = ?{idx}"));
    }
    if let Some(app_name) = &params.app_name {
        let idx = query_params.push_text(app_name.clone());
        sql.push_str(&format!(" AND l.app_name = ?{idx}"));
    }
    if let Some(from) = &params.from {
        let idx = query_params.push_text(from.clone());
        sql.push_str(&format!(" AND l.timestamp >= ?{idx}"));
    }
    if let Some(to) = &params.to {
        let idx = query_params.push_text(to.clone());
        sql.push_str(&format!(" AND l.timestamp <= ?{idx}"));
    }
    // Apply severity_min filter: include only logs at or above the threshold.
    if let Some(severity_min) = &params.severity_min {
        let threshold = severity_to_num(severity_min).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid severity_min '{}': must be one of {}",
                severity_min,
                SEVERITY_LEVELS.join(", ")
            )
        })?;
        let levels_in: Vec<String> = SEVERITY_LEVELS[..=threshold as usize]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let placeholders: Vec<String> = levels_in
            .iter()
            .map(|s| {
                let idx = query_params.push_text(s.clone());
                format!("?{idx}")
            })
            .collect();
        sql.push_str(&format!(" AND l.severity IN ({})", placeholders.join(", ")));
    }

    sql.push_str(&format!(
        " ORDER BY logs_fts.rowid DESC LIMIT {SIMILAR_INCIDENT_FTS_CANDIDATE_CAP}
        ),
        bucketed AS (
            SELECT
                hostname,
                app_name,
                CAST(strftime('%s', timestamp) AS INTEGER) / {window_secs} AS bucket,
                MIN(timestamp) AS window_start,
                MAX(timestamp) AS window_end,
                COUNT(*) AS log_count,
                GROUP_CONCAT(severity, ',') AS severities,
                GROUP_CONCAT(SUBSTR(message, 1, 256), '|||') AS messages
            FROM hits
            GROUP BY hostname, app_name, bucket
        )
        SELECT hostname, app_name, window_start, window_end, log_count, severities, messages
        FROM bucketed
        ORDER BY log_count DESC, window_start DESC
        LIMIT {}",
        limit + 1
    ));

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        tracing::error!(error = %e, "similar_incidents_clusters prepare failed");
        anyhow::anyhow!("similar_incidents query failed")
    })?;
    let rows = stmt
        .query_map(
            rusqlite::params_from_iter(query_params.bindings.iter()),
            |row| {
                Ok((
                    row.get::<_, String>(0)?,         // hostname
                    row.get::<_, Option<String>>(1)?, // app_name
                    row.get::<_, String>(2)?,         // window_start
                    row.get::<_, String>(3)?,         // window_end
                    row.get::<_, i64>(4)?,            // log_count
                    row.get::<_, String>(5)?,         // severities (comma-joined)
                    row.get::<_, String>(6)?,         // messages (|||joined)
                ))
            },
        )
        .map_err(|e| {
            tracing::error!(error = %e, "similar_incidents_clusters query failed");
            anyhow::anyhow!("similar_incidents query failed")
        })?;

    // Collect raw cluster rows first; keep one extra to detect truncation.
    struct RawCluster {
        hostname: String,
        app_name: Option<String>,
        window_start: String,
        window_end: String,
        log_count: i64,
        severity_peak: String,
        representative_messages: Vec<String>,
    }
    let mut raw: Vec<RawCluster> = Vec::new();
    for row in rows {
        let (hostname, app_name, window_start, window_end, log_count, severities, messages) =
            row.map_err(|e| {
                tracing::error!(error = %e, "similar_incidents_clusters row mapping failed");
                anyhow::anyhow!("similar_incidents row mapping failed")
            })?;

        // Find peak severity (lowest numeric value = highest severity).
        let severity_peak = severities
            .split(',')
            .filter_map(|s| severity_to_num(s).map(|n| (n, s.to_string())))
            .min_by_key(|(n, _)| *n)
            .map(|(_, s)| s)
            .unwrap_or_else(|| "info".to_string());

        // Collect up to 3 representative messages.
        let representative_messages: Vec<String> = messages
            .split("|||")
            .take(3)
            .map(|m| m.to_string())
            .collect();

        raw.push(RawCluster {
            hostname,
            app_name,
            window_start,
            window_end,
            log_count,
            severity_peak,
            representative_messages,
        });
    }

    // Detect truncation (we queried limit+1 rows) and trim to the true limit.
    let truncated = raw.len() > limit;
    raw.truncate(limit);

    // Build one UNION ALL query across all cluster windows so each window gets
    // its own per-session match_count.  This is O(1) roundtrips while keeping
    // counts accurate (no global-span inflation when a session spans clusters).
    let per_cluster_sessions = find_correlated_sessions_per_cluster(
        &conn,
        &raw.iter()
            .map(|c| (c.window_start.as_str(), c.window_end.as_str()))
            .collect::<Vec<_>>(),
    )?;

    let clusters: Vec<IncidentCluster> = raw
        .into_iter()
        .map(|rc| {
            let key = (rc.window_start.clone(), rc.window_end.clone());
            let correlated_sessions = per_cluster_sessions.get(&key).cloned().unwrap_or_default();
            IncidentCluster {
                hostname: rc.hostname,
                app_name: rc.app_name,
                window_start: rc.window_start,
                window_end: rc.window_end,
                log_count: rc.log_count,
                severity_peak: rc.severity_peak,
                representative_messages: rc.representative_messages,
                correlated_sessions,
            }
        })
        .collect();

    let total_clusters = clusters.len();
    Ok(SimilarIncidentsResult {
        query: params.query.clone(),
        total_clusters,
        truncated,
        clusters,
    })
}

/// Per-cluster session lookup using a single UNION ALL query so each cluster
/// window gets its own accurate match_count rather than an inflated global count.
/// Returns a map keyed by (window_start, window_end) → top-5 sessions.
fn find_correlated_sessions_per_cluster(
    conn: &rusqlite::Connection,
    windows: &[(&str, &str)],
) -> Result<std::collections::HashMap<(String, String), Vec<CorrelatedSession>>> {
    use std::collections::HashMap;

    if windows.is_empty() {
        return Ok(HashMap::new());
    }

    // Build UNION ALL: one SELECT per cluster window, tagging each row with ws/we.
    // Parameters use stride-2 (?{p} = ws, ?{p+1} = we for window i).  SQLite ?N
    // numbered bindings reuse the same value within one arm's subquery without
    // requiring duplicate params in the binding list.
    let mut arms: Vec<String> = Vec::with_capacity(windows.len());
    for (i, _) in windows.iter().enumerate() {
        let p = 1 + i * 2;
        arms.push(format!(
            "SELECT ?{p} AS ws, ?{p1} AS we,
                    l.ai_project, l.ai_tool, l.ai_session_id,
                    COUNT(*) AS match_count,
                    (SELECT l2.message FROM logs l2
                     WHERE l2.ai_project = l.ai_project
                       AND l2.ai_tool = l.ai_tool
                       AND l2.ai_session_id = l.ai_session_id
                       AND l2.timestamp BETWEEN ?{p} AND ?{p1}
                     ORDER BY l2.timestamp DESC LIMIT 1) AS best_snippet
             FROM logs l
             WHERE l.ai_project IS NOT NULL AND l.ai_project != ''
               AND l.ai_tool IS NOT NULL AND l.ai_tool != ''
               AND l.ai_session_id IS NOT NULL AND l.ai_session_id != ''
               AND l.timestamp BETWEEN ?{p} AND ?{p1}
             GROUP BY l.ai_project, l.ai_tool, l.ai_session_id",
            p = p,
            p1 = p + 1,
        ));
    }
    let sql = arms.join("\nUNION ALL\n");

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| anyhow::anyhow!("find_correlated_sessions_per_cluster prepare: {e}"))?;

    // Two params per window (ws, we); ?N reuse within each arm handles the rest.
    let params: Vec<&dyn rusqlite::ToSql> = windows
        .iter()
        .flat_map(|(ws, we)| {
            let v: [&dyn rusqlite::ToSql; 2] = [ws, we];
            v
        })
        .collect();

    let rows = stmt
        .query_map(params.as_slice(), |row| {
            let ws: String = row.get(0)?;
            let we: String = row.get(1)?;
            let project: String = row.get(2)?;
            let tool: String = row.get(3)?;
            let session_id: String = row.get(4)?;
            let match_count: i64 = row.get(5)?;
            let best_snippet: Option<String> = row.get(6)?;
            Ok((ws, we, project, tool, session_id, match_count, best_snippet))
        })
        .map_err(|e| anyhow::anyhow!("find_correlated_sessions_per_cluster query: {e}"))?;

    // Collect all sessions per cluster before sorting — UNION ALL rows arrive
    // unordered, so the top-5 cap must come after sorting, not during insertion.
    let mut map: HashMap<(String, String), Vec<CorrelatedSession>> = HashMap::new();
    for row in rows {
        let (ws, we, project, tool, session_id, match_count, best_snippet) =
            row.map_err(|e| anyhow::anyhow!("find_correlated_sessions_per_cluster row: {e}"))?;
        map.entry((ws, we)).or_default().push(CorrelatedSession {
            project,
            tool,
            session_id,
            match_count,
            best_snippet,
        });
    }
    // Sort by match_count descending, then cap at 5 per cluster.
    for sessions in map.values_mut() {
        sessions.sort_by_key(|b| std::cmp::Reverse(b.match_count));
        sessions.truncate(5);
    }
    Ok(map)
}

/// FTS5 search over AI transcript entries, returns sessions grouped by
/// (project, tool, session_id), ranked by match count. Also returns system
/// log context from the top session's time window.
pub fn ask_history_sessions(pool: &DbPool, params: &AskHistoryParams) -> Result<AskHistoryResult> {
    validate_fts_query(&params.query)?;

    // Search AI transcript entries only using the existing grouping query.
    // Pass through hostname/app_name so the session search is properly scoped.
    let ai_params = SearchAiSessionsParams {
        query: params.query.clone(),
        ai_project: None,
        ai_tool: None,
        hostname: params.hostname.clone(),
        app_name: params.app_name.clone(),
        from: params.from.clone(),
        to: params.to.clone(),
        limit: Some(params.limit.unwrap_or(10).clamp(1, 50)),
    };
    let session_result = search_ai_sessions(pool, &ai_params)?;

    // Collect context logs from the top session's time window.
    let context_logs = if let Some(top) = session_result.sessions.first() {
        let ctx_from = top.first_seen.clone();
        let ctx_to = top.last_seen.clone();

        let conn = pool.get()?;
        let mut ctx_params = SqlParams::new(3);
        ctx_params
            .bindings
            .push(rusqlite::types::Value::Text(ctx_from.clone()));
        ctx_params
            .bindings
            .push(rusqlite::types::Value::Text(ctx_to.clone()));

        let mut ctx_sql = String::from(
            "SELECT id, timestamp, hostname, facility, severity,
                    app_name, process_id, message, received_at, source_ip,
                    ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
             FROM logs
             WHERE (ai_project IS NULL OR ai_project = '')
               AND timestamp BETWEEN ?1 AND ?2",
        );
        if let Some(hostname) = &params.hostname {
            let idx = ctx_params.push_text(hostname.clone());
            ctx_sql.push_str(&format!(" AND hostname = ?{idx}"));
        }
        if let Some(app_name) = &params.app_name {
            let idx = ctx_params.push_text(app_name.clone());
            ctx_sql.push_str(&format!(" AND app_name = ?{idx}"));
        }
        ctx_sql.push_str(" ORDER BY timestamp DESC LIMIT 20");

        let mut stmt = conn.prepare(&ctx_sql).map_err(|e| {
            tracing::error!(error = %e, "ask_history context_logs prepare failed");
            anyhow::anyhow!("ask_history context_logs query failed")
        })?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(ctx_params.bindings.iter()),
                map_row,
            )
            .map_err(|e| {
                tracing::error!(error = %e, "ask_history context_logs query failed");
                anyhow::anyhow!("ask_history context_logs query failed")
            })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        Vec::new()
    };

    Ok(AskHistoryResult {
        query: params.query.clone(),
        total_candidates: session_result.total_candidates,
        truncated: session_result.truncated,
        sessions: session_result.sessions,
        context_logs,
    })
}

/// Return aggregate log statistics + error logs + correlated AI sessions for a
/// given time window.
pub fn incident_context_summary(
    pool: &DbPool,
    params: &IncidentContextParams,
) -> Result<IncidentContextResult> {
    let conn = pool.get()?;
    let limit = params.limit.unwrap_or(50).clamp(1, 200) as usize;

    // Resolve severity threshold. Default to "warning" (numeric 4).
    let severity_threshold = params
        .severity_min
        .as_deref()
        .map(|s| {
            severity_to_num(s).ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid severity_min '{}': must be one of emerg, alert, crit, err, warning, notice, info, debug",
                    s
                )
            })
        })
        .transpose()?
        .unwrap_or_else(|| severity_to_num("warning").unwrap());

    // Build reusable aggregate params with host/app/AI-exclusion filters.
    // Params: ?1=from, ?2=to, then optional host/app starting at ?3.
    // All aggregate queries exclude AI transcript rows (ai_project IS NULL or '').
    let mut agg_params = SqlParams::new(3);
    agg_params
        .bindings
        .push(rusqlite::types::Value::Text(params.from.clone()));
    agg_params
        .bindings
        .push(rusqlite::types::Value::Text(params.to.clone()));
    let mut agg_host_clause = String::new();
    let mut agg_app_clause = String::new();
    if let Some(hostname) = &params.hostname {
        let idx = agg_params.push_text(hostname.clone());
        agg_host_clause = format!(" AND hostname = ?{idx}");
    }
    if let Some(app_name) = &params.app_name {
        let idx = agg_params.push_text(app_name.clone());
        agg_app_clause = format!(" AND app_name = ?{idx}");
    }
    let agg_base_filter = format!(
        "WHERE (ai_project IS NULL OR ai_project = '')
           AND timestamp BETWEEN ?1 AND ?2{agg_host_clause}{agg_app_clause}"
    );

    // Total log count in window (system logs only, scoped by host/app).
    let total_logs: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM logs INDEXED BY idx_logs_timestamp {agg_base_filter}"),
            rusqlite::params_from_iter(agg_params.bindings.iter()),
            |r| r.get(0),
        )
        .map_err(|e| anyhow::anyhow!("incident_context total_logs: {e}"))?;

    // Counts by severity (system logs only, scoped by host/app).
    let mut by_sev_stmt = conn
        .prepare(&format!(
            "SELECT severity, COUNT(*) FROM logs INDEXED BY idx_logs_timestamp
             {agg_base_filter}
             GROUP BY severity
             ORDER BY COUNT(*) DESC"
        ))
        .map_err(|e| anyhow::anyhow!("incident_context by_severity prepare: {e}"))?;
    let by_severity: Vec<SeverityCount> = by_sev_stmt
        .query_map(
            rusqlite::params_from_iter(agg_params.bindings.iter()),
            |row| {
                Ok(SeverityCount {
                    severity: row.get(0)?,
                    count: row.get(1)?,
                })
            },
        )
        .map_err(|e| anyhow::anyhow!("incident_context by_severity query: {e}"))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Counts by app_name (top 20, system logs only, scoped by host/app).
    let mut by_app_stmt = conn
        .prepare(&format!(
            "SELECT app_name, COUNT(*) FROM logs INDEXED BY idx_logs_timestamp
             {agg_base_filter}
             GROUP BY app_name
             ORDER BY COUNT(*) DESC
             LIMIT 20"
        ))
        .map_err(|e| anyhow::anyhow!("incident_context by_app prepare: {e}"))?;
    let by_app: Vec<AppLogCount> = by_app_stmt
        .query_map(
            rusqlite::params_from_iter(agg_params.bindings.iter()),
            |row| {
                Ok(AppLogCount {
                    app_name: row.get(0)?,
                    count: row.get(1)?,
                })
            },
        )
        .map_err(|e| anyhow::anyhow!("incident_context by_app query: {e}"))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Error logs: system logs at or above severity threshold in the window.
    let error_severities: Vec<String> = SEVERITY_LEVELS[..=severity_threshold as usize]
        .iter()
        .map(|s| s.to_string())
        .collect();

    // Build parameterized query for error logs.
    // Params: ?1=from, ?2=to, ?3..=?N=severities, then optional host/app.
    // SqlParams::new(3) sets next_idx=3 so push_text calls start at ?3, after
    // the two manually-pushed bindings for from (?1) and to (?2).
    let mut err_params = SqlParams::new(3);
    err_params
        .bindings
        .push(rusqlite::types::Value::Text(params.from.clone()));
    err_params
        .bindings
        .push(rusqlite::types::Value::Text(params.to.clone()));

    let sev_placeholders: Vec<String> = error_severities
        .iter()
        .map(|s| {
            let idx = err_params.push_text(s.clone());
            format!("?{idx}")
        })
        .collect();

    let mut err_sql = format!(
        "SELECT id, timestamp, hostname, facility, severity,
                app_name, process_id, message, received_at, source_ip,
                ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
         FROM logs
         INDEXED BY idx_logs_timestamp
         WHERE timestamp BETWEEN ?1 AND ?2
           AND severity IN ({})
           AND (ai_project IS NULL OR ai_project = '')",
        sev_placeholders.join(", ")
    );

    if let Some(hostname) = &params.hostname {
        let idx = err_params.push_text(hostname.clone());
        err_sql.push_str(&format!(" AND hostname = ?{idx}"));
    }
    if let Some(app_name) = &params.app_name {
        let idx = err_params.push_text(app_name.clone());
        err_sql.push_str(&format!(" AND app_name = ?{idx}"));
    }
    // Query limit+1 rows so we can detect true truncation.
    err_sql.push_str(&format!(" ORDER BY timestamp DESC LIMIT {}", limit + 1));

    let mut err_stmt = conn.prepare(&err_sql).map_err(|e| {
        tracing::error!(error = %e, "incident_context error_logs prepare failed");
        anyhow::anyhow!("incident_context error_logs query failed")
    })?;
    let error_rows = err_stmt
        .query_map(
            rusqlite::params_from_iter(err_params.bindings.iter()),
            map_row,
        )
        .map_err(|e| {
            tracing::error!(error = %e, "incident_context error_logs query failed");
            anyhow::anyhow!("incident_context error_logs query failed")
        })?;
    let mut error_logs: Vec<super::models::LogEntry> =
        error_rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let error_logs_truncated = error_logs.len() > limit;
    error_logs.truncate(limit);

    // AI sessions active in the window — query on the already-held conn to
    // avoid a second pool.get() call (which deadlocks on single-connection test pools).
    let ai_sessions = {
        let mut ai_sql = String::from(
            "SELECT ai_project, ai_tool, ai_session_id,
                    MIN(ai_transcript_path) AS ai_transcript_path,
                    hostname,
                    MIN(timestamp) AS first_seen,
                    MAX(timestamp) AS last_seen,
                    COUNT(*) AS event_count
             FROM logs
             WHERE ai_project IS NOT NULL AND ai_project != ''
               AND ai_tool IS NOT NULL AND ai_tool != ''
               AND ai_session_id IS NOT NULL AND ai_session_id != ''
               AND timestamp BETWEEN ?1 AND ?2",
        );
        let mut ai_bindings: Vec<rusqlite::types::Value> = vec![
            rusqlite::types::Value::Text(params.from.clone()),
            rusqlite::types::Value::Text(params.to.clone()),
        ];
        if let Some(hostname) = &params.hostname {
            ai_bindings.push(rusqlite::types::Value::Text(hostname.clone()));
            ai_sql.push_str(&format!(" AND hostname = ?{}", ai_bindings.len()));
        }
        ai_sql.push_str(
            " GROUP BY ai_project, ai_tool, ai_session_id, hostname
              ORDER BY last_seen DESC LIMIT 20",
        );
        let mut ai_stmt = conn
            .prepare(&ai_sql)
            .map_err(|e| anyhow::anyhow!("incident_context ai_sessions prepare: {e}"))?;
        let rows = ai_stmt
            .query_map(rusqlite::params_from_iter(ai_bindings.iter()), |row| {
                Ok(super::models::AiSessionEntry {
                    ai_project: row.get(0)?,
                    ai_tool: row.get(1)?,
                    ai_session_id: row.get(2)?,
                    ai_transcript_path: row.get(3)?,
                    hostname: row.get(4)?,
                    first_seen: row.get(5)?,
                    last_seen: row.get(6)?,
                    event_count: row.get(7)?,
                })
            })
            .map_err(|e| anyhow::anyhow!("incident_context ai_sessions query: {e}"))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    Ok(IncidentContextResult {
        window_from: params.from.clone(),
        window_to: params.to.clone(),
        total_logs,
        by_severity,
        by_app,
        error_logs,
        error_logs_truncated,
        ai_sessions,
    })
}

#[cfg(test)]
#[path = "queries_tests.rs"]
mod tests;
