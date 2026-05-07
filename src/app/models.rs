use serde::{Deserialize, Serialize};

use crate::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: i64,
    pub timestamp: String,
    pub hostname: String,
    pub facility: Option<String>,
    pub severity: String,
    pub app_name: Option<String>,
    pub process_id: Option<String>,
    pub message: String,
    pub received_at: String,
    pub source_ip: String,
}

impl From<db::LogEntry> for LogEntry {
    fn from(value: db::LogEntry) -> Self {
        Self {
            id: value.id,
            timestamp: value.timestamp,
            hostname: value.hostname,
            facility: value.facility,
            severity: value.severity,
            app_name: value.app_name,
            process_id: value.process_id,
            message: value.message,
            received_at: value.received_at,
            source_ip: value.source_ip,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchLogsRequest {
    pub query: Option<String>,
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub severity: Option<String>,
    pub app_name: Option<String>,
    pub facility: Option<String>,
    pub process_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchLogsResponse {
    pub count: usize,
    pub logs: Vec<LogEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TailLogsRequest {
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub app_name: Option<String>,
    /// Minimum severity to return (e.g. `warning` returns warning + worse).
    pub severity_min: Option<String>,
    pub n: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorSummaryEntry {
    pub hostname: String,
    /// Optional secondary grouping key (e.g. app_name) when `group_by` is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    pub severity: String,
    pub count: i64,
}

impl From<db::ErrorSummaryEntry> for ErrorSummaryEntry {
    fn from(value: db::ErrorSummaryEntry) -> Self {
        Self {
            hostname: value.hostname,
            app_name: value.app_name,
            severity: value.severity,
            count: value.count,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetErrorsRequest {
    pub from: Option<String>,
    pub to: Option<String>,
    /// Secondary grouping key. Currently supports `app_name`.
    pub group_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetErrorsResponse {
    pub summary: Vec<ErrorSummaryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostEntry {
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
    pub log_count: i64,
}

impl From<db::HostEntry> for HostEntry {
    fn from(value: db::HostEntry) -> Self {
        Self {
            hostname: value.hostname,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
            log_count: value.log_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListHostsResponse {
    pub hosts: Vec<HostEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelateEventsRequest {
    pub reference_time: String,
    pub window_minutes: Option<u32>,
    pub severity_min: Option<String>,
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub query: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelatedHost {
    pub hostname: String,
    pub event_count: usize,
    pub events: Vec<LogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelateEventsResponse {
    pub reference_time: String,
    pub window_minutes: u32,
    pub window_from: String,
    pub window_to: String,
    pub severity_min: String,
    pub total_events: usize,
    pub truncated: bool,
    pub hosts_count: usize,
    pub hosts: Vec<CorrelatedHost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbStats {
    pub total_logs: i64,
    pub total_hosts: i64,
    pub oldest_log: Option<String>,
    pub newest_log: Option<String>,
    pub logical_db_size_mb: String,
    pub physical_db_size_mb: String,
    pub free_disk_mb: Option<String>,
    pub max_db_size_mb: u64,
    pub min_free_disk_mb: u64,
    pub write_blocked: bool,
    pub phantom_fts_rows: i64,
}

impl From<db::DbStats> for DbStats {
    fn from(value: db::DbStats) -> Self {
        Self {
            total_logs: value.total_logs,
            total_hosts: value.total_hosts,
            oldest_log: value.oldest_log,
            newest_log: value.newest_log,
            logical_db_size_mb: value.logical_db_size_mb,
            physical_db_size_mb: value.physical_db_size_mb,
            free_disk_mb: value.free_disk_mb,
            max_db_size_mb: value.max_db_size_mb,
            min_free_disk_mb: value.min_free_disk_mb,
            write_blocked: value.write_blocked,
            phantom_fts_rows: value.phantom_fts_rows,
        }
    }
}

// ---------------------------------------------------------------------------
// New analytics actions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListAppsRequest {
    pub hostname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAppsResponse {
    pub apps: Vec<db::AppEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSourceIpsResponse {
    pub source_ips: Vec<db::SourceIpEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimelineRequest {
    pub bucket: Option<String>,
    pub group_by: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub hostname: Option<String>,
    pub app_name: Option<String>,
    pub severity_min: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineResponse {
    pub bucket: String,
    pub group_by: Option<String>,
    pub points: Vec<db::TimelinePoint>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PatternsRequest {
    pub from: Option<String>,
    pub to: Option<String>,
    pub hostname: Option<String>,
    pub app_name: Option<String>,
    pub severity_min: Option<String>,
    pub scan_limit: Option<u32>,
    pub top_n: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternsResponse {
    pub patterns: Vec<db::PatternEntry>,
    pub scanned: i64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextRequest {
    pub log_id: Option<i64>,
    pub hostname: Option<String>,
    pub timestamp: Option<String>,
    pub before: Option<u32>,
    pub after: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextResponse {
    pub reference: LogEntry,
    pub before: Vec<LogEntry>,
    pub after: Vec<LogEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetLogRequest {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetLogResponse {
    pub log: db::LogEntryWithRaw,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IngestRateRequest {
    pub by_host: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRateResponse {
    pub now: String,
    pub buckets: db::IngestRateBuckets,
    pub write_blocked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_host: Option<Vec<db::IngestRatePerHost>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SilentHostsRequest {
    /// Hosts whose `last_seen` is older than `silent_minutes` ago (default 30).
    pub silent_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SilentHostsResponse {
    pub silent_minutes: u32,
    pub cutoff: String,
    pub now: String,
    pub hosts: Vec<db::SilentHostEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClockSkewRequest {
    pub since: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockSkewResponse {
    pub since: String,
    pub hosts: Vec<db::ClockSkewEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnomaliesRequest {
    /// Recent window in minutes (default 15, max 1440).
    pub recent_minutes: Option<u32>,
    /// Baseline window in minutes preceding the recent window (default 360, max 10080).
    pub baseline_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomaliesResponse {
    pub recent_from: String,
    pub recent_to: String,
    pub baseline_from: String,
    pub baseline_to: String,
    pub recent_minutes: u32,
    pub baseline_minutes: u32,
    pub hosts: Vec<db::AnomalyEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompareRequest {
    pub a_from: String,
    pub a_to: String,
    pub b_from: String,
    pub b_to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareResponse {
    pub a: db::RangeSummary,
    pub b: db::RangeSummary,
    pub delta_total_logs: i64,
    pub delta_total_errors: i64,
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
