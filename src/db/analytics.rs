//! Higher-level analytics queries layered on top of the `logs` table.
//!
//! These power MCP actions beyond raw search/tail: distinct-value enumeration
//! (`apps`, `source_ips`), time-series aggregations (`timeline`,
//! `ingest_rate`), pattern clustering (`patterns`), drill-down helpers
//! (`context`, `get`), operational health (`silent_hosts`, `clock_skew`),
//! and comparison/anomaly detection.

use anyhow::Result;
use rusqlite::params;
use std::cmp::Reverse;
use std::collections::BTreeMap;

use super::pool::DbPool;
use super::queries::map_row_with_raw;
use super::{
    AiProjectContext, AiProjectContextParams, AiUsageBlock, AiUsageBlocksParams,
    AiUsageBlocksResult, LogEntry,
};

// -----------------------------------------------------------------------------
// apps: distinct app_names with stats
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppEntry {
    pub app_name: String,
    pub log_count: i64,
    pub host_count: i64,
    pub first_seen: String,
    pub last_seen: String,
}

pub struct ListAppsParams<'a> {
    pub hostname: Option<&'a str>,
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
    /// Page size. Default 500, max 5000.
    pub limit: usize,
    /// Page offset (number of distinct apps to skip). Default 0.
    pub offset: usize,
}

pub struct ListAppsResult {
    pub apps: Vec<AppEntry>,
    /// Total distinct app names matching the filter (across all pages).
    pub total: usize,
}

pub fn list_apps(pool: &DbPool, params: &ListAppsParams<'_>) -> Result<ListAppsResult> {
    let conn = pool.get()?;
    let limit = params.limit.clamp(1, 5_000);
    let offset = params.offset;

    if params.hostname.is_none()
        && params.from.is_none()
        && params.to.is_none()
        && inventory_backfill_complete_conn(&conn).unwrap_or(false)
    {
        let total = conn.query_row("SELECT COUNT(*) FROM app_inventory_stats", [], |row| {
            row.get::<_, i64>(0)
        })? as usize;
        let mut stmt = conn.prepare(&format!(
            "WITH page AS (
                SELECT app_name, log_count, first_seen, last_seen
                FROM app_inventory_stats
                ORDER BY last_seen DESC, app_name ASC
                LIMIT {limit} OFFSET {offset}
             )
             SELECT p.app_name, p.log_count, COUNT(h.hostname), p.first_seen, p.last_seen
             FROM page p
             LEFT JOIN app_host_inventory_stats h ON h.app_name = p.app_name
             GROUP BY p.app_name, p.log_count, p.first_seen, p.last_seen
             ORDER BY p.last_seen DESC, p.app_name ASC"
        ))?;
        let apps = stmt
            .query_map([], |row| {
                Ok(AppEntry {
                    app_name: row.get(0)?,
                    log_count: row.get(1)?,
                    host_count: row.get(2)?,
                    first_seen: row.get(3)?,
                    last_seen: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        return Ok(ListAppsResult { apps, total });
    }

    // Build the shared WHERE clause and bindings once; reuse for COUNT and data queries.
    // first_seen / last_seen come from `received_at` (server clock) so they match
    // how the `hosts` table is updated and aren't skewed by a misconfigured device clock.
    let mut where_clause = String::from("app_name IS NOT NULL AND app_name != ''");
    let mut bindings: Vec<rusqlite::types::Value> = vec![];
    let mut idx = 1usize;

    if let Some(h) = params.hostname {
        where_clause.push_str(&format!(" AND hostname = ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(h.to_owned()));
        idx += 1;
    }
    if let Some(f) = params.from {
        where_clause.push_str(&format!(" AND received_at >= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(f.to_owned()));
        idx += 1;
    }
    if let Some(t) = params.to {
        where_clause.push_str(&format!(" AND received_at <= ?{idx}"));
        bindings.push(rusqlite::types::Value::Text(t.to_owned()));
        idx += 1;
    }
    let _ = idx;

    let total = conn.query_row(
        &format!("SELECT COUNT(DISTINCT app_name) FROM logs WHERE {where_clause}"),
        rusqlite::params_from_iter(bindings.iter()),
        |row| row.get::<_, i64>(0),
    )? as usize;

    let data_sql = format!(
        "SELECT app_name, COUNT(*), COUNT(DISTINCT hostname),
                MIN(received_at), MAX(received_at)
         FROM logs
         WHERE {where_clause}
         GROUP BY app_name
         ORDER BY MAX(received_at) DESC, app_name ASC
         LIMIT {limit} OFFSET {offset}"
    );
    let mut stmt = conn.prepare(&data_sql)?;
    let apps = stmt
        .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
            Ok(AppEntry {
                app_name: row.get(0)?,
                log_count: row.get(1)?,
                host_count: row.get(2)?,
                first_seen: row.get(3)?,
                last_seen: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(ListAppsResult { apps, total })
}

// -----------------------------------------------------------------------------
// source_ips: distinct senders + hostnames seen per sender
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SourceIpHostBreakdown {
    pub hostname: String,
    pub log_count: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SourceIpEntry {
    pub source_ip: String,
    pub log_count: i64,
    pub host_count: i64,
    pub first_seen: String,
    pub last_seen: String,
    /// Top hostnames associated with this source_ip (capped at 10).
    pub hostnames: Vec<SourceIpHostBreakdown>,
}

pub struct ListSourceIpsResult {
    pub source_ips: Vec<SourceIpEntry>,
    /// Total distinct source IPs in the database (across all pages).
    pub total: usize,
}

pub struct ListSourceIpsParams {
    /// Page size. Default 500, max 5000.
    pub limit: usize,
    /// Page offset (number of distinct IPs to skip). Default 0.
    pub offset: usize,
}

pub fn list_source_ips(pool: &DbPool, params: &ListSourceIpsParams) -> Result<ListSourceIpsResult> {
    let limit = params.limit.clamp(1, 5_000);
    let offset = params.offset;

    let inventory_complete = {
        let conn = pool.get()?;
        inventory_backfill_complete_conn(&conn).unwrap_or(false)
    };

    if inventory_complete {
        let conn = pool.get()?;
        let total = conn.query_row(
            "SELECT COUNT(*) FROM source_ip_inventory_stats",
            [],
            |row| row.get::<_, i64>(0),
        )? as usize;

        let mut stmt = conn.prepare(&format!(
            "WITH page AS (
                SELECT source_ip, log_count, first_seen, last_seen
                FROM source_ip_inventory_stats
                ORDER BY log_count DESC, source_ip ASC
                LIMIT {limit} OFFSET {offset}
             )
             SELECT p.source_ip, p.log_count, p.first_seen, p.last_seen,
                    h.hostname, h.log_count, h.first_seen, h.last_seen
             FROM page p
             LEFT JOIN source_ip_host_inventory_stats h ON h.source_ip = p.source_ip
             ORDER BY p.log_count DESC, p.source_ip ASC, h.log_count DESC, h.hostname ASC"
        ))?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<i64>>(5)?.unwrap_or(0),
            ))
        })?;

        let mut by_ip: BTreeMap<String, SourceIpEntry> = BTreeMap::new();
        for row in rows {
            let (ip, total_count, first, last, host, host_count) = row?;
            let entry = by_ip.entry(ip.clone()).or_insert_with(|| SourceIpEntry {
                source_ip: ip,
                log_count: total_count,
                host_count: 0,
                first_seen: first,
                last_seen: last,
                hostnames: Vec::new(),
            });
            if let Some(host) = host {
                entry.host_count += 1;
                if entry.hostnames.len() < 10 {
                    entry.hostnames.push(SourceIpHostBreakdown {
                        hostname: host,
                        log_count: host_count,
                    });
                }
            }
        }

        let mut out: Vec<SourceIpEntry> = by_ip.into_values().collect();
        out.sort_by_key(|entry| Reverse(entry.log_count));
        return Ok(ListSourceIpsResult {
            source_ips: out,
            total,
        });
    }

    list_source_ips_from_logs(pool, params)
}

fn inventory_backfill_complete_conn(conn: &rusqlite::Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT completed_at IS NOT NULL
         FROM inventory_backfill_state
         WHERE name = 'app_source_inventory'",
        [],
        |row| row.get::<_, bool>(0),
    )
}

fn list_source_ips_from_logs(
    pool: &DbPool,
    params: &ListSourceIpsParams,
) -> Result<ListSourceIpsResult> {
    let limit = params.limit.clamp(1, 5_000);
    let offset = params.offset;
    let conn = pool.get()?;

    let total = conn.query_row(
        "SELECT COUNT(DISTINCT source_ip) FROM logs WHERE source_ip != ''",
        [],
        |row| row.get::<_, i64>(0),
    )? as usize;

    let mut stmt = conn.prepare(&format!(
        "WITH top_ips AS (
            SELECT source_ip
            FROM logs
            WHERE source_ip != ''
            GROUP BY source_ip
            ORDER BY COUNT(*) DESC, source_ip ASC
            LIMIT {limit} OFFSET {offset}
         )
         SELECT l.source_ip, l.hostname, COUNT(*), MIN(l.received_at), MAX(l.received_at)
         FROM logs l
         JOIN top_ips t ON t.source_ip = l.source_ip
         GROUP BY l.source_ip, l.hostname
         ORDER BY l.source_ip, COUNT(*) DESC"
    ))?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;

    let mut by_ip: BTreeMap<String, SourceIpEntry> = BTreeMap::new();
    for row in rows {
        let (ip, host, count, first, last) = row?;
        let entry = by_ip.entry(ip.clone()).or_insert_with(|| SourceIpEntry {
            source_ip: ip,
            log_count: 0,
            host_count: 0,
            first_seen: first.clone(),
            last_seen: last.clone(),
            hostnames: Vec::new(),
        });
        entry.log_count += count;
        entry.host_count += 1;
        if first < entry.first_seen {
            entry.first_seen = first;
        }
        if last > entry.last_seen {
            entry.last_seen = last;
        }
        if entry.hostnames.len() < 10 {
            entry.hostnames.push(SourceIpHostBreakdown {
                hostname: host,
                log_count: count,
            });
        }
    }

    let mut out: Vec<SourceIpEntry> = by_ip.into_values().collect();
    out.sort_by_key(|entry| Reverse(entry.log_count));
    Ok(ListSourceIpsResult {
        source_ips: out,
        total,
    })
}

pub fn get_ai_usage_blocks(
    pool: &DbPool,
    params: &AiUsageBlocksParams,
) -> Result<AiUsageBlocksResult> {
    let conn = pool.get()?;
    const LIMIT: usize = 1_000;
    const DEFAULT_LOOKBACK_DAYS: i64 = 30;
    const BUCKET_SECS: i64 = 18_000;
    let mut sql = format!(
        "SELECT datetime((CAST(strftime('%s', timestamp) AS INTEGER) / {BUCKET_SECS}) * {BUCKET_SECS}, 'unixepoch') AS bucket_start,
                datetime(((CAST(strftime('%s', timestamp) AS INTEGER) / {BUCKET_SECS}) * {BUCKET_SECS}) + {BUCKET_SECS}, 'unixepoch') AS bucket_end,
                ai_project,
                ai_tool,
                COUNT(DISTINCT ai_session_id) AS session_count,
                COUNT(*) AS event_count
         FROM logs
         WHERE ai_project IS NOT NULL AND ai_project != ''
           AND ai_tool IS NOT NULL AND ai_tool != ''
           AND ai_session_id IS NOT NULL AND ai_session_id != ''"
    );
    let mut bindings: Vec<rusqlite::types::Value> = Vec::new();
    let mut idx = 1usize;
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
    } else if params.from.is_none() {
        sql.push_str(&format!(
            " AND timestamp >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-{DEFAULT_LOOKBACK_DAYS} days')"
        ));
    }
    sql.push_str(&format!(
        " GROUP BY bucket_start, bucket_end, ai_project, ai_tool
          ORDER BY bucket_start ASC, ai_project ASC, ai_tool ASC
          LIMIT {}",
        LIMIT + 1
    ));

    let mut stmt = conn.prepare(&sql)?;
    let mut blocks = stmt
        .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
            Ok(AiUsageBlock {
                bucket_start: row.get(0)?,
                bucket_end: row.get(1)?,
                project: row.get(2)?,
                tool: row.get(3)?,
                session_count: row.get(4)?,
                event_count: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let truncated = truncate_to_limit(&mut blocks, LIMIT);
    Ok(AiUsageBlocksResult {
        total_blocks: blocks.len(),
        truncated,
        blocks,
    })
}

fn truncate_to_limit<T>(values: &mut Vec<T>, limit: usize) -> bool {
    let truncated = values.len() > limit;
    values.truncate(limit);
    truncated
}

pub fn get_ai_project_context(
    pool: &DbPool,
    params: &AiProjectContextParams,
) -> Result<AiProjectContext> {
    type ProjectAggregateRow = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
    );
    let conn = pool.get()?;
    let mut aggregate_sql = String::from(
        "SELECT GROUP_CONCAT(DISTINCT ai_tool),
                GROUP_CONCAT(DISTINCT ai_session_id),
                GROUP_CONCAT(DISTINCT hostname),
                MIN(timestamp),
                MAX(timestamp),
                COUNT(*)
         FROM logs
         WHERE ai_project = ?1",
    );
    let mut aggregate_bindings = vec![rusqlite::types::Value::Text(params.project.clone())];
    if let Some(tool) = &params.ai_tool {
        aggregate_sql.push_str(" AND ai_tool = ?2");
        aggregate_bindings.push(rusqlite::types::Value::Text(tool.clone()));
    }

    let (tools, sessions, hostnames, first_seen, last_seen, event_count): ProjectAggregateRow =
        conn.query_row(
            &aggregate_sql,
            rusqlite::params_from_iter(aggregate_bindings.iter()),
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;

    let limit = params.limit.unwrap_or(5).min(20);
    let mut recent_sql = String::from(
        "SELECT id, timestamp, hostname, facility, severity,
                app_name, process_id, message, received_at, source_ip,
                ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
         FROM logs
         WHERE ai_project = ?1",
    );
    let mut recent_bindings = vec![rusqlite::types::Value::Text(params.project.clone())];
    if let Some(tool) = &params.ai_tool {
        recent_sql.push_str(" AND ai_tool = ?2");
        recent_bindings.push(rusqlite::types::Value::Text(tool.clone()));
    }
    recent_sql.push_str(&format!(
        " ORDER BY timestamp DESC, id DESC LIMIT {}",
        limit + 1
    ));
    let mut stmt = conn.prepare(&recent_sql)?;
    let mut recent_entries = stmt
        .query_map(
            rusqlite::params_from_iter(recent_bindings.iter()),
            super::queries::map_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let recent_entries_truncated = recent_entries.len() > limit as usize;
    recent_entries.truncate(limit as usize);
    for entry in &mut recent_entries {
        entry.message = truncate_chars(&entry.message, 256);
    }

    Ok(AiProjectContext {
        project: params.project.clone(),
        tools: split_csv(tools),
        sessions: split_csv(sessions),
        hostnames: split_csv(hostnames),
        first_seen,
        last_seen,
        event_count,
        recent_entries_truncated,
        recent_entries,
    })
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>()
        + "…"
}

// -----------------------------------------------------------------------------
// timeline: bucketed counts
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Minute,
    Hour,
    Day,
    Week,
    Month,
}

impl Bucket {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "minute" | "min" | "m" => Some(Self::Minute),
            "hour" | "h" => Some(Self::Hour),
            "day" | "d" => Some(Self::Day),
            "week" | "w" => Some(Self::Week),
            "month" => Some(Self::Month),
            _ => None,
        }
    }

    /// SQLite `strftime` pattern for grouping timestamps into this bucket.
    ///
    /// `Week` uses `%W` (Monday-based week-of-year, 00–53). Days in early
    /// January that fall *before* the year's first Monday land in week `00`,
    /// producing labels like `2026-W00` (bead llto.2). Bucket tests deliberately
    /// avoid that range. We do NOT use ISO-8601 week-numbering (`%G-%V`, which
    /// would never emit week 00) because the SQLite version bundled here does
    /// not support the `%G`/`%V` specifiers.
    ///
    /// `pub(crate)` (bead llto.3): the `db` module is `pub(crate)`, so this is
    /// only reachable within the crate — widening past crate scope serves no
    /// caller.
    pub(crate) fn strftime_format(self) -> &'static str {
        match self {
            Self::Minute => "%Y-%m-%dT%H:%M:00Z",
            Self::Hour => "%Y-%m-%dT%H:00:00Z",
            Self::Day => "%Y-%m-%dT00:00:00Z",
            Self::Week => "%Y-W%W",
            Self::Month => "%Y-%m",
        }
    }

    /// Default lookback window (days) when no explicit `from`/`to` is provided.
    /// Wider buckets scan wider time ranges, so larger defaults are appropriate.
    pub fn default_lookback_days(self) -> i64 {
        match self {
            Self::Minute => 1,
            Self::Hour => 7,
            Self::Day => 30,
            Self::Week => 180,
            Self::Month => 730,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TimelineGroupBy {
    None,
    Hostname,
    Severity,
    AppName,
}

impl TimelineGroupBy {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "hostname" | "host" => Some(Self::Hostname),
            "severity" | "sev" => Some(Self::Severity),
            "app_name" | "app" => Some(Self::AppName),
            _ => None,
        }
    }

    fn column(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Hostname => Some("hostname"),
            Self::Severity => Some("severity"),
            Self::AppName => Some("app_name"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimelinePoint {
    pub bucket: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub count: i64,
}

#[allow(clippy::too_many_arguments)]
pub fn timeline(
    pool: &DbPool,
    bucket: Bucket,
    group_by: TimelineGroupBy,
    from: Option<&str>,
    to: Option<&str>,
    hostname: Option<&str>,
    app_name: Option<&str>,
    severity_in: Option<&[String]>,
) -> Result<Vec<TimelinePoint>> {
    let conn = pool.get()?;
    let mut sql = format!(
        "SELECT strftime('{fmt}', timestamp) AS bucket",
        fmt = bucket.strftime_format()
    );
    if let Some(col) = group_by.column() {
        sql.push_str(&format!(", COALESCE({col}, '<none>') AS grp"));
    }
    sql.push_str(", COUNT(*) FROM logs WHERE 1=1");

    let mut bindings: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1usize;

    if let Some(f) = from {
        sql.push_str(&format!(" AND timestamp >= ?{idx}"));
        bindings.push(Box::new(f.to_string()));
        idx += 1;
    }
    if let Some(t) = to {
        sql.push_str(&format!(" AND timestamp <= ?{idx}"));
        bindings.push(Box::new(t.to_string()));
        idx += 1;
    }
    if let Some(h) = hostname {
        sql.push_str(&format!(" AND hostname = ?{idx}"));
        bindings.push(Box::new(h.to_string()));
        idx += 1;
    }
    if let Some(a) = app_name {
        sql.push_str(&format!(" AND app_name = ?{idx}"));
        bindings.push(Box::new(a.to_string()));
        idx += 1;
    }
    if let Some(levels) = severity_in {
        if !levels.is_empty() {
            let placeholders: Vec<String> =
                (0..levels.len()).map(|i| format!("?{}", idx + i)).collect();
            sql.push_str(&format!(" AND severity IN ({})", placeholders.join(", ")));
            for lvl in levels {
                bindings.push(Box::new(lvl.clone()));
            }
        }
    }

    if group_by.column().is_some() {
        sql.push_str(" GROUP BY bucket, grp ORDER BY bucket ASC, grp ASC");
    } else {
        sql.push_str(" GROUP BY bucket ORDER BY bucket ASC");
    }

    let mut stmt = conn.prepare(&sql)?;
    let bind_refs: Vec<&dyn rusqlite::types::ToSql> = bindings.iter().map(|b| b.as_ref()).collect();
    let has_group = group_by.column().is_some();
    let rows = stmt.query_map(rusqlite::params_from_iter(bind_refs.iter().copied()), |r| {
        if has_group {
            Ok(TimelinePoint {
                bucket: r.get(0)?,
                group: Some(r.get::<_, String>(1)?),
                count: r.get(2)?,
            })
        } else {
            Ok(TimelinePoint {
                bucket: r.get(0)?,
                group: None,
                count: r.get(1)?,
            })
        }
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// -----------------------------------------------------------------------------
// patterns: cluster near-duplicate messages by template normalization
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PatternEntry {
    pub template: String,
    pub count: i64,
    pub host_count: i64,
    pub sample: String,
    pub first_seen: String,
    pub last_seen: String,
    /// Up to 5 hostnames where this template was seen.
    pub hostnames: Vec<String>,
}

fn split_csv(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn patterns(
    pool: &DbPool,
    from: Option<&str>,
    to: Option<&str>,
    hostname: Option<&str>,
    app_name: Option<&str>,
    severity_in: Option<&[String]>,
    scan_limit: u32,
    top_n: u32,
) -> Result<(Vec<PatternEntry>, i64, bool)> {
    let conn = pool.get()?;
    let scan_limit = scan_limit.clamp(1, 50_000);

    let mut sql = String::from("SELECT timestamp, hostname, message FROM logs WHERE 1=1");
    let mut bindings: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1usize;
    if let Some(f) = from {
        sql.push_str(&format!(" AND timestamp >= ?{idx}"));
        bindings.push(Box::new(f.to_string()));
        idx += 1;
    }
    if let Some(t) = to {
        sql.push_str(&format!(" AND timestamp <= ?{idx}"));
        bindings.push(Box::new(t.to_string()));
        idx += 1;
    }
    if let Some(h) = hostname {
        sql.push_str(&format!(" AND hostname = ?{idx}"));
        bindings.push(Box::new(h.to_string()));
        idx += 1;
    }
    if let Some(a) = app_name {
        sql.push_str(&format!(" AND app_name = ?{idx}"));
        bindings.push(Box::new(a.to_string()));
        idx += 1;
    }
    if let Some(levels) = severity_in {
        if !levels.is_empty() {
            let placeholders: Vec<String> =
                (0..levels.len()).map(|i| format!("?{}", idx + i)).collect();
            sql.push_str(&format!(" AND severity IN ({})", placeholders.join(", ")));
            for lvl in levels {
                bindings.push(Box::new(lvl.clone()));
            }
        }
    }
    // Cap rows scanned to bound CPU/memory — we ask for one extra to detect truncation.
    sql.push_str(&format!(
        " ORDER BY timestamp DESC LIMIT {}",
        scan_limit + 1
    ));

    let mut stmt = conn.prepare(&sql)?;
    let bind_refs: Vec<&dyn rusqlite::types::ToSql> = bindings.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(rusqlite::params_from_iter(bind_refs.iter().copied()), |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;

    struct Acc {
        count: i64,
        sample: String,
        first_seen: String,
        last_seen: String,
        hosts: BTreeMap<String, i64>,
    }
    let mut by_template: BTreeMap<String, Acc> = BTreeMap::new();
    let mut scanned = 0i64;
    let mut overflow = false;
    for row in rows {
        let (ts, host, msg) = row?;
        scanned += 1;
        if scanned > scan_limit as i64 {
            overflow = true;
            break;
        }
        let template = crate::app::error_detection::normalize::normalize_template(&msg);
        let entry = by_template.entry(template).or_insert_with(|| Acc {
            count: 0,
            sample: msg.clone(),
            first_seen: ts.clone(),
            last_seen: ts.clone(),
            hosts: BTreeMap::new(),
        });
        entry.count += 1;
        if ts < entry.first_seen {
            entry.first_seen = ts.clone();
        }
        if ts > entry.last_seen {
            entry.last_seen = ts;
        }
        *entry.hosts.entry(host).or_insert(0) += 1;
    }

    let total_scanned = scanned.min(scan_limit as i64);

    let mut out: Vec<PatternEntry> = by_template
        .into_iter()
        .map(|(template, acc)| {
            let mut hosts: Vec<(String, i64)> = acc.hosts.into_iter().collect();
            hosts.sort_by_key(|(_, count)| Reverse(*count));
            let host_count = hosts.len() as i64;
            let hostnames: Vec<String> = hosts.into_iter().take(5).map(|(h, _)| h).collect();
            PatternEntry {
                template,
                count: acc.count,
                host_count,
                sample: acc.sample,
                first_seen: acc.first_seen,
                last_seen: acc.last_seen,
                hostnames,
            }
        })
        .collect();
    out.sort_by_key(|entry| Reverse(entry.count));
    out.truncate(top_n as usize);
    Ok((out, total_scanned, overflow))
}

// -----------------------------------------------------------------------------
// context: surrounding logs for a single point of interest
// -----------------------------------------------------------------------------

pub struct ContextRef {
    /// `Some(id)` anchors with stable (timestamp, id) tiebreaking; `None` means
    /// the caller only has a timestamp (e.g. `context` invoked with
    /// `hostname` + `timestamp`), in which case the query splits cleanly on
    /// `< timestamp` / `> timestamp`.
    pub id: Option<i64>,
    pub hostname: String,
    pub timestamp: String,
}

pub fn fetch_log_by_id(pool: &DbPool, id: i64) -> Result<Option<LogEntryWithRaw>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, hostname, facility, severity,
                app_name, process_id, message, raw, received_at, source_ip,
                ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
         FROM logs WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(r) = rows.next()? {
        Ok(Some(map_row_with_raw(r)?))
    } else {
        Ok(None)
    }
}

pub fn context_around(
    pool: &DbPool,
    reference: &ContextRef,
    before: u32,
    after: u32,
) -> Result<(Vec<LogEntry>, Vec<LogEntry>)> {
    let conn = pool.get()?;
    let before = before.min(500);
    let after = after.min(500);

    let (mut before_rows, after_rows) = match reference.id {
        Some(id) => {
            // ID-anchored: stable (timestamp, id) tiebreaker — symmetrical because
            // we know exactly which row at `timestamp` is the reference.
            let mut before_stmt = conn.prepare(
                "SELECT id, timestamp, hostname, facility, severity,
                        app_name, process_id, message, received_at, source_ip,
                        ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
                 FROM logs
                 WHERE hostname = ?1
                   AND (timestamp < ?2 OR (timestamp = ?2 AND id < ?3))
                 ORDER BY timestamp DESC, id DESC
                 LIMIT ?4",
            )?;
            let before_rows = before_stmt
                .query_map(
                    params![reference.hostname, reference.timestamp, id, before],
                    super::queries::map_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let mut after_stmt = conn.prepare(
                "SELECT id, timestamp, hostname, facility, severity,
                        app_name, process_id, message, received_at, source_ip,
                        ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
                 FROM logs
                 WHERE hostname = ?1
                   AND (timestamp > ?2 OR (timestamp = ?2 AND id > ?3))
                 ORDER BY timestamp ASC, id ASC
                 LIMIT ?4",
            )?;
            let after_rows = after_stmt
                .query_map(
                    params![reference.hostname, reference.timestamp, id, after],
                    super::queries::map_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            (before_rows, after_rows)
        }
        None => {
            // Timestamp-anchored: no row identity, so split strictly on the
            // timestamp boundary. Rows that share the exact reference timestamp
            // are excluded from both sides rather than dumped onto one —
            // symmetry over completeness.
            let mut before_stmt = conn.prepare(
                "SELECT id, timestamp, hostname, facility, severity,
                        app_name, process_id, message, received_at, source_ip,
                        ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
                 FROM logs
                 WHERE hostname = ?1 AND timestamp < ?2
                 ORDER BY timestamp DESC, id DESC
                 LIMIT ?3",
            )?;
            let before_rows = before_stmt
                .query_map(
                    params![reference.hostname, reference.timestamp, before],
                    super::queries::map_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let mut after_stmt = conn.prepare(
                "SELECT id, timestamp, hostname, facility, severity,
                        app_name, process_id, message, received_at, source_ip,
                        ai_tool, ai_project, ai_session_id, ai_transcript_path, metadata_json
                 FROM logs
                 WHERE hostname = ?1 AND timestamp > ?2
                 ORDER BY timestamp ASC, id ASC
                 LIMIT ?3",
            )?;
            let after_rows = after_stmt
                .query_map(
                    params![reference.hostname, reference.timestamp, after],
                    super::queries::map_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            (before_rows, after_rows)
        }
    };

    // Reverse the "before" rows so the result reads chronologically.
    before_rows.reverse();

    Ok((before_rows, after_rows))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LogEntryWithRaw {
    pub id: i64,
    pub timestamp: String,
    pub hostname: String,
    pub facility: Option<String>,
    pub severity: String,
    pub app_name: Option<String>,
    pub process_id: Option<String>,
    pub message: String,
    pub raw: String,
    pub received_at: String,
    pub source_ip: String,
    pub ai_tool: Option<String>,
    pub ai_project: Option<String>,
    pub ai_session_id: Option<String>,
    pub ai_transcript_path: Option<String>,
    pub metadata_json: Option<String>,
}

// -----------------------------------------------------------------------------
// ingest_rate: throughput over the last 1m / 5m / 15m windows
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IngestRateBuckets {
    pub last_1m: i64,
    pub last_5m: i64,
    pub last_15m: i64,
    pub per_sec_1m: f64,
    pub per_sec_5m: f64,
    pub per_sec_15m: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IngestRatePerHost {
    pub hostname: String,
    pub last_1m: i64,
    pub last_5m: i64,
    pub last_15m: i64,
}

pub fn ingest_rate(
    pool: &DbPool,
    now: &str,
    cut_1m: &str,
    cut_5m: &str,
    cut_15m: &str,
) -> Result<IngestRateBuckets> {
    let conn = pool.get()?;
    let row: (i64, i64, i64) = conn.query_row(
        "SELECT
            SUM(CASE WHEN received_at >= ?1 THEN 1 ELSE 0 END),
            SUM(CASE WHEN received_at >= ?2 THEN 1 ELSE 0 END),
            SUM(CASE WHEN received_at >= ?3 THEN 1 ELSE 0 END)
         FROM logs
         WHERE received_at >= ?3 AND received_at <= ?4",
        params![cut_1m, cut_5m, cut_15m, now],
        |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                r.get::<_, Option<i64>>(2)?.unwrap_or(0),
            ))
        },
    )?;
    Ok(IngestRateBuckets {
        last_1m: row.0,
        last_5m: row.1,
        last_15m: row.2,
        per_sec_1m: row.0 as f64 / 60.0,
        per_sec_5m: row.1 as f64 / 300.0,
        per_sec_15m: row.2 as f64 / 900.0,
    })
}

pub fn ingest_rate_by_host(
    pool: &DbPool,
    now: &str,
    cut_1m: &str,
    cut_5m: &str,
    cut_15m: &str,
) -> Result<Vec<IngestRatePerHost>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT hostname,
                SUM(CASE WHEN received_at >= ?1 THEN 1 ELSE 0 END),
                SUM(CASE WHEN received_at >= ?2 THEN 1 ELSE 0 END),
                COUNT(*)
         FROM logs
         WHERE received_at >= ?3 AND received_at <= ?4
         GROUP BY hostname
         ORDER BY COUNT(*) DESC",
    )?;
    let rows = stmt.query_map(params![cut_1m, cut_5m, cut_15m, now], |r| {
        Ok(IngestRatePerHost {
            hostname: r.get(0)?,
            last_1m: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
            last_5m: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
            last_15m: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// -----------------------------------------------------------------------------
// silent_hosts: hosts whose last_seen is older than a threshold
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SilentHostEntry {
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
    pub log_count: i64,
    /// Approx seconds between log arrivals over the host's full history.
    pub typical_interval_secs: Option<f64>,
    /// Seconds since last log was received.
    pub silent_for_secs: i64,
}

pub fn silent_hosts(pool: &DbPool, cutoff: &str, now_unix: i64) -> Result<Vec<SilentHostEntry>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT hostname, first_seen, last_seen, log_count
         FROM hosts
         WHERE last_seen < ?1
         ORDER BY last_seen ASC",
    )?;
    let rows = stmt.query_map(params![cutoff], |r| {
        let hostname: String = r.get(0)?;
        let first_seen: String = r.get(1)?;
        let last_seen: String = r.get(2)?;
        let log_count: i64 = r.get(3)?;
        Ok((hostname, first_seen, last_seen, log_count))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (hostname, first_seen, last_seen, log_count) = row?;
        let typical_interval_secs = compute_interval(&first_seen, &last_seen, log_count);
        let silent_for_secs = chrono::DateTime::parse_from_rfc3339(&last_seen)
            .map(|dt| now_unix - dt.timestamp())
            .unwrap_or(0);
        out.push(SilentHostEntry {
            hostname,
            first_seen,
            last_seen,
            log_count,
            typical_interval_secs,
            silent_for_secs,
        });
    }
    Ok(out)
}

fn compute_interval(first: &str, last: &str, count: i64) -> Option<f64> {
    if count < 2 {
        return None;
    }
    let f = chrono::DateTime::parse_from_rfc3339(first).ok()?;
    let l = chrono::DateTime::parse_from_rfc3339(last).ok()?;
    let span = (l - f).num_seconds() as f64;
    if span <= 0.0 {
        return None;
    }
    Some(span / (count - 1) as f64)
}

// -----------------------------------------------------------------------------
// clock_skew: per-host distribution of received_at - timestamp
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClockSkewEntry {
    pub hostname: String,
    pub samples: i64,
    pub avg_skew_secs: f64,
    pub min_skew_secs: f64,
    pub max_skew_secs: f64,
}

const CLOCK_SKEW_SQL: &str = "
    SELECT hostname,
           COUNT(*),
           AVG((julianday(received_at) - julianday(timestamp)) * 86400),
           MIN((julianday(received_at) - julianday(timestamp)) * 86400),
           MAX((julianday(received_at) - julianday(timestamp)) * 86400)
      FROM logs INDEXED BY idx_logs_received_at
     WHERE received_at >= ?1
     GROUP BY hostname
     ORDER BY ABS(AVG((julianday(received_at) - julianday(timestamp)) * 86400)) DESC
     LIMIT ?2";

pub fn clock_skew(pool: &DbPool, since: &str, limit: Option<u32>) -> Result<Vec<ClockSkewEntry>> {
    let conn = pool.get()?;
    let limit = limit.map(i64::from).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare(CLOCK_SKEW_SQL)?;
    let rows = stmt.query_map(params![since, limit], |r| {
        Ok(ClockSkewEntry {
            hostname: r.get(0)?,
            samples: r.get(1)?,
            avg_skew_secs: r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
            min_skew_secs: r.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
            max_skew_secs: r.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// -----------------------------------------------------------------------------
// anomalies: per-host volume / error-rate vs baseline
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnomalyEntry {
    pub hostname: String,
    pub recent_count: i64,
    pub baseline_count: i64,
    pub recent_per_min: f64,
    pub baseline_per_min: f64,
    /// recent_per_min / baseline_per_min (1.0 means unchanged). `None` when
    /// baseline is zero (host is new — flagged separately).
    pub ratio: Option<f64>,
    /// Poisson-style z-score against baseline rate.
    pub z_score: Option<f64>,
    pub recent_errors: i64,
    pub baseline_errors: i64,
}

pub fn anomalies(
    pool: &DbPool,
    recent_from: &str,
    recent_to: &str,
    baseline_from: &str,
    baseline_to: &str,
    recent_minutes: u32,
    baseline_minutes: u32,
) -> Result<Vec<AnomalyEntry>> {
    let conn = pool.get()?;
    let error_levels = "('emerg','alert','crit','err','warning')";

    // FULL OUTER JOIN is only available in SQLite ≥ 3.39 — emulate via a
    // hosts-union CTE so we still pick up hosts that exist in baseline only.
    let sql = format!(
        "WITH recent AS (
             SELECT hostname,
                    COUNT(*) AS c,
                    SUM(CASE WHEN severity IN {err} THEN 1 ELSE 0 END) AS e
             FROM logs
             WHERE timestamp >= ?1 AND timestamp <= ?2
             GROUP BY hostname
         ),
         baseline AS (
             SELECT hostname,
                    COUNT(*) AS c,
                    SUM(CASE WHEN severity IN {err} THEN 1 ELSE 0 END) AS e
             FROM logs
             WHERE timestamp >= ?3 AND timestamp <= ?4
             GROUP BY hostname
         ),
         all_hosts AS (
             SELECT hostname FROM recent
             UNION
             SELECT hostname FROM baseline
         )
         SELECT a.hostname,
                COALESCE(r.c, 0), COALESCE(b.c, 0),
                COALESCE(r.e, 0), COALESCE(b.e, 0)
         FROM all_hosts a
         LEFT JOIN recent r ON r.hostname = a.hostname
         LEFT JOIN baseline b ON b.hostname = a.hostname
         ORDER BY a.hostname",
        err = error_levels
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params![recent_from, recent_to, baseline_from, baseline_to],
        |r| {
            let hostname: String = r.get(0)?;
            let recent_count: i64 = r.get(1)?;
            let baseline_count: i64 = r.get(2)?;
            let recent_errors: i64 = r.get(3)?;
            let baseline_errors: i64 = r.get(4)?;
            Ok((
                hostname,
                recent_count,
                baseline_count,
                recent_errors,
                baseline_errors,
            ))
        },
    )?;

    let recent_minutes = recent_minutes.max(1) as f64;
    let baseline_minutes = baseline_minutes.max(1) as f64;
    let mut out = Vec::new();
    for row in rows {
        let (hostname, recent_count, baseline_count, recent_errors, baseline_errors) = row?;
        let recent_per_min = recent_count as f64 / recent_minutes;
        let baseline_per_min = baseline_count as f64 / baseline_minutes;
        let ratio = if baseline_per_min > 0.0 {
            Some(recent_per_min / baseline_per_min)
        } else {
            None
        };
        let expected = baseline_per_min * recent_minutes;
        let z_score = if expected > 0.0 {
            Some((recent_count as f64 - expected) / expected.sqrt())
        } else {
            None
        };
        out.push(AnomalyEntry {
            hostname,
            recent_count,
            baseline_count,
            recent_per_min,
            baseline_per_min,
            ratio,
            z_score,
            recent_errors,
            baseline_errors,
        });
    }
    // Surface new-but-active hosts (`recent_count > 0` against a zero baseline)
    // at the top — they have no defined `z_score`, but they are exactly the
    // signal the docstring promises. Other unscored entries (e.g. recent zero
    // activity, dormant hosts) sink to the bottom.
    let sort_key = |e: &AnomalyEntry| -> f64 {
        if e.baseline_count == 0 && e.recent_count > 0 {
            f64::INFINITY
        } else {
            e.z_score.unwrap_or(f64::NEG_INFINITY)
        }
    };
    out.sort_by(|a, b| {
        sort_key(b)
            .partial_cmp(&sort_key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

// -----------------------------------------------------------------------------
// compare: side-by-side diff of two time ranges
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RangeSummary {
    pub from: String,
    pub to: String,
    pub total_logs: i64,
    pub total_errors: i64,
    pub by_severity: Vec<(String, i64)>,
    pub top_hosts: Vec<(String, i64)>,
    pub top_apps: Vec<(String, i64)>,
}

pub fn summarize_range(pool: &DbPool, from: &str, to: &str) -> Result<RangeSummary> {
    let conn = pool.get()?;
    let total_logs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM logs WHERE timestamp >= ?1 AND timestamp <= ?2",
        params![from, to],
        |r| r.get(0),
    )?;
    let total_errors: i64 = conn.query_row(
        "SELECT COUNT(*) FROM logs
         WHERE timestamp >= ?1 AND timestamp <= ?2
           AND severity IN ('emerg','alert','crit','err','warning')",
        params![from, to],
        |r| r.get(0),
    )?;
    let mut sev_stmt = conn.prepare(
        "SELECT severity, COUNT(*) FROM logs
         WHERE timestamp >= ?1 AND timestamp <= ?2
         GROUP BY severity
         ORDER BY COUNT(*) DESC",
    )?;
    let by_severity = sev_stmt
        .query_map(params![from, to], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut host_stmt = conn.prepare(
        "SELECT hostname, COUNT(*) FROM logs
         WHERE timestamp >= ?1 AND timestamp <= ?2
         GROUP BY hostname
         ORDER BY COUNT(*) DESC, source_ip ASC
         LIMIT 10",
    )?;
    let top_hosts = host_stmt
        .query_map(params![from, to], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut app_stmt = conn.prepare(
        "SELECT app_name, COUNT(*) FROM logs
         WHERE timestamp >= ?1 AND timestamp <= ?2
           AND app_name IS NOT NULL AND app_name != ''
         GROUP BY app_name
         ORDER BY COUNT(*) DESC, source_ip ASC
         LIMIT 10",
    )?;
    let top_apps = app_stmt
        .query_map(params![from, to], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(RangeSummary {
        from: from.to_string(),
        to: to.to_string(),
        total_logs,
        total_errors,
        by_severity,
        top_hosts,
        top_apps,
    })
}

#[cfg(test)]
#[path = "analytics_tests.rs"]
mod tests;
