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
use super::LogEntry;

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

pub fn list_apps(pool: &DbPool, hostname: Option<&str>) -> Result<Vec<AppEntry>> {
    let conn = pool.get()?;
    // first_seen / last_seen come from `received_at` (server clock) so they match
    // how the `hosts` table is updated and aren't skewed by a misconfigured device clock.
    let (sql, want_host) = match hostname {
        Some(_) => (
            "SELECT app_name, COUNT(*), COUNT(DISTINCT hostname),
                    MIN(received_at), MAX(received_at)
             FROM logs
             WHERE app_name IS NOT NULL AND app_name != '' AND hostname = ?1
             GROUP BY app_name
             ORDER BY MAX(received_at) DESC",
            true,
        ),
        None => (
            "SELECT app_name, COUNT(*), COUNT(DISTINCT hostname),
                    MIN(received_at), MAX(received_at)
             FROM logs
             WHERE app_name IS NOT NULL AND app_name != ''
             GROUP BY app_name
             ORDER BY MAX(received_at) DESC",
            false,
        ),
    };

    let mut stmt = conn.prepare(sql)?;
    let map = |row: &rusqlite::Row| -> rusqlite::Result<AppEntry> {
        Ok(AppEntry {
            app_name: row.get(0)?,
            log_count: row.get(1)?,
            host_count: row.get(2)?,
            first_seen: row.get(3)?,
            last_seen: row.get(4)?,
        })
    };
    let rows = if want_host {
        stmt.query_map(params![hostname.unwrap()], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map([], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
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

pub fn list_source_ips(pool: &DbPool) -> Result<Vec<SourceIpEntry>> {
    let conn = pool.get()?;
    // first_seen / last_seen come from `received_at` (server clock): for sender
    // identity / spoof-detection use cases, the network arrival time is the
    // verified value, while the message `timestamp` is whatever the sender claimed.
    let mut stmt = conn.prepare(
        "SELECT source_ip, hostname, COUNT(*), MIN(received_at), MAX(received_at)
         FROM logs
         WHERE source_ip != ''
         GROUP BY source_ip, hostname
         ORDER BY source_ip, COUNT(*) DESC",
    )?;
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
    Ok(out)
}

// -----------------------------------------------------------------------------
// timeline: bucketed counts
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum Bucket {
    Minute,
    Hour,
    Day,
}

impl Bucket {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "minute" | "min" | "m" => Some(Self::Minute),
            "hour" | "h" => Some(Self::Hour),
            "day" | "d" => Some(Self::Day),
            _ => None,
        }
    }

    fn strftime_format(self) -> &'static str {
        match self {
            Self::Minute => "%Y-%m-%dT%H:%M:00Z",
            Self::Hour => "%Y-%m-%dT%H:00:00Z",
            Self::Day => "%Y-%m-%dT00:00:00Z",
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

/// Normalise a message into a template by replacing variable runs with
/// placeholders. Designed to collapse near-duplicates without external regex
/// dependencies — character-class scan only.
///
/// Pattern detection is byte-level (all targets — digits, hex, IPv4, UUIDs —
/// are ASCII), but non-ASCII bytes are passed through as their proper
/// codepoint so internationalised log messages stay valid UTF-8.
pub(super) fn normalize_template(msg: &str) -> String {
    let bytes = msg.as_bytes();
    let mut out = String::with_capacity(msg.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if !b.is_ascii() {
            // Multi-byte UTF-8 sequence — copy the whole codepoint intact rather
            // than splitting it into mojibake. Indexing `msg[i..]` is safe because
            // the index lands on a char boundary (we only advance by char widths
            // or full ASCII pattern matches).
            let ch = msg[i..].chars().next().expect("char at boundary");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        // UUID: 8-4-4-4-12 hex with dashes
        if is_hex(b) && looks_like_uuid_at(bytes, i) {
            out.push_str("<uuid>");
            i += 36;
            continue;
        }
        // IPv4 / IPv4:port (digits + dots, optionally :digits)
        if b.is_ascii_digit() {
            if let Some(end) = ipv4_end(bytes, i) {
                out.push_str("<ip>");
                i = end;
                if i < bytes.len() && bytes[i] == b':' {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j > i + 1 {
                        out.push_str(":<n>");
                        i = j;
                    }
                }
                continue;
            }
        }
        // Long hex run (>= 8 chars) — typical for hashes
        if is_hex(b) {
            let mut j = i;
            while j < bytes.len() && is_hex(bytes[j]) {
                j += 1;
            }
            if j - i >= 8 {
                out.push_str("<hex>");
                i = j;
                continue;
            }
        }
        // Numeric run
        if b.is_ascii_digit() {
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            out.push_str("<n>");
            i = j;
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || (b'A'..=b'F').contains(&b)
}

fn looks_like_uuid_at(bytes: &[u8], i: usize) -> bool {
    if i + 36 > bytes.len() {
        return false;
    }
    let positions = [8, 13, 18, 23];
    for (k, b) in bytes[i..i + 36].iter().enumerate() {
        if positions.contains(&k) {
            if *b != b'-' {
                return false;
            }
        } else if !is_hex(*b) {
            return false;
        }
    }
    true
}

fn ipv4_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    let mut octets = 0;
    while octets < 4 {
        let octet_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let len = i - octet_start;
        if !(1..=3).contains(&len) {
            return None;
        }
        octets += 1;
        if octets < 4 {
            if i >= bytes.len() || bytes[i] != b'.' {
                return None;
            }
            i += 1;
        }
    }
    Some(i)
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
        let template = normalize_template(&msg);
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
                app_name, process_id, message, raw, received_at, source_ip
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
                        app_name, process_id, message, received_at, source_ip
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
                        app_name, process_id, message, received_at, source_ip
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
                        app_name, process_id, message, received_at, source_ip
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
                        app_name, process_id, message, received_at, source_ip
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

pub fn clock_skew(pool: &DbPool, since: &str) -> Result<Vec<ClockSkewEntry>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT hostname,
                COUNT(*),
                AVG((julianday(received_at) - julianday(timestamp)) * 86400),
                MIN((julianday(received_at) - julianday(timestamp)) * 86400),
                MAX((julianday(received_at) - julianday(timestamp)) * 86400)
         FROM logs
         WHERE received_at >= ?1
         GROUP BY hostname
         ORDER BY ABS(AVG((julianday(received_at) - julianday(timestamp)) * 86400)) DESC",
    )?;
    let rows = stmt.query_map(params![since], |r| {
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
         ORDER BY COUNT(*) DESC
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
         ORDER BY COUNT(*) DESC
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
