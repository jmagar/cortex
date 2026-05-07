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
    pub apps: Vec<AppEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEntry {
    pub app_name: String,
    pub log_count: i64,
    pub host_count: i64,
    pub first_seen: String,
    pub last_seen: String,
}

impl From<db::AppEntry> for AppEntry {
    fn from(value: db::AppEntry) -> Self {
        Self {
            app_name: value.app_name,
            log_count: value.log_count,
            host_count: value.host_count,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSourceIpsResponse {
    pub source_ips: Vec<SourceIpEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceIpHostBreakdown {
    pub hostname: String,
    pub log_count: i64,
}

impl From<db::SourceIpHostBreakdown> for SourceIpHostBreakdown {
    fn from(value: db::SourceIpHostBreakdown) -> Self {
        Self {
            hostname: value.hostname,
            log_count: value.log_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceIpEntry {
    pub source_ip: String,
    pub log_count: i64,
    pub host_count: i64,
    pub first_seen: String,
    pub last_seen: String,
    pub hostnames: Vec<SourceIpHostBreakdown>,
}

impl From<db::SourceIpEntry> for SourceIpEntry {
    fn from(value: db::SourceIpEntry) -> Self {
        Self {
            source_ip: value.source_ip,
            log_count: value.log_count,
            host_count: value.host_count,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
            hostnames: value.hostnames.into_iter().map(Into::into).collect(),
        }
    }
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
    pub points: Vec<TimelinePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelinePoint {
    pub bucket: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub count: i64,
}

impl From<db::TimelinePoint> for TimelinePoint {
    fn from(value: db::TimelinePoint) -> Self {
        Self {
            bucket: value.bucket,
            group: value.group,
            count: value.count,
        }
    }
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
    pub patterns: Vec<PatternEntry>,
    pub scanned: i64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternEntry {
    pub template: String,
    pub count: i64,
    pub host_count: i64,
    pub sample: String,
    pub first_seen: String,
    pub last_seen: String,
    pub hostnames: Vec<String>,
}

impl From<db::PatternEntry> for PatternEntry {
    fn from(value: db::PatternEntry) -> Self {
        Self {
            template: value.template,
            count: value.count,
            host_count: value.host_count,
            sample: value.sample,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
            hostnames: value.hostnames,
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetLogRequest {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetLogResponse {
    pub log: LogEntryWithRaw,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl From<db::LogEntryWithRaw> for LogEntryWithRaw {
    fn from(value: db::LogEntryWithRaw) -> Self {
        Self {
            id: value.id,
            timestamp: value.timestamp,
            hostname: value.hostname,
            facility: value.facility,
            severity: value.severity,
            app_name: value.app_name,
            process_id: value.process_id,
            message: value.message,
            raw: value.raw,
            received_at: value.received_at,
            source_ip: value.source_ip,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IngestRateRequest {
    pub by_host: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRateResponse {
    pub now: String,
    pub buckets: IngestRateBuckets,
    pub write_blocked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_host: Option<Vec<IngestRatePerHost>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRateBuckets {
    pub last_1m: i64,
    pub last_5m: i64,
    pub last_15m: i64,
    pub per_sec_1m: f64,
    pub per_sec_5m: f64,
    pub per_sec_15m: f64,
}

impl From<db::IngestRateBuckets> for IngestRateBuckets {
    fn from(value: db::IngestRateBuckets) -> Self {
        Self {
            last_1m: value.last_1m,
            last_5m: value.last_5m,
            last_15m: value.last_15m,
            per_sec_1m: value.per_sec_1m,
            per_sec_5m: value.per_sec_5m,
            per_sec_15m: value.per_sec_15m,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRatePerHost {
    pub hostname: String,
    pub last_1m: i64,
    pub last_5m: i64,
    pub last_15m: i64,
}

impl From<db::IngestRatePerHost> for IngestRatePerHost {
    fn from(value: db::IngestRatePerHost) -> Self {
        Self {
            hostname: value.hostname,
            last_1m: value.last_1m,
            last_5m: value.last_5m,
            last_15m: value.last_15m,
        }
    }
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
    pub hosts: Vec<SilentHostEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SilentHostEntry {
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
    pub log_count: i64,
    pub typical_interval_secs: Option<f64>,
    pub silent_for_secs: i64,
}

impl From<db::SilentHostEntry> for SilentHostEntry {
    fn from(value: db::SilentHostEntry) -> Self {
        Self {
            hostname: value.hostname,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
            log_count: value.log_count,
            typical_interval_secs: value.typical_interval_secs,
            silent_for_secs: value.silent_for_secs,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClockSkewRequest {
    pub since: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockSkewResponse {
    pub since: String,
    pub hosts: Vec<ClockSkewEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockSkewEntry {
    pub hostname: String,
    pub samples: i64,
    pub avg_skew_secs: f64,
    pub min_skew_secs: f64,
    pub max_skew_secs: f64,
}

impl From<db::ClockSkewEntry> for ClockSkewEntry {
    fn from(value: db::ClockSkewEntry) -> Self {
        Self {
            hostname: value.hostname,
            samples: value.samples,
            avg_skew_secs: value.avg_skew_secs,
            min_skew_secs: value.min_skew_secs,
            max_skew_secs: value.max_skew_secs,
        }
    }
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
    pub hosts: Vec<AnomalyEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyEntry {
    pub hostname: String,
    pub recent_count: i64,
    pub baseline_count: i64,
    pub recent_per_min: f64,
    pub baseline_per_min: f64,
    pub ratio: Option<f64>,
    pub z_score: Option<f64>,
    pub recent_errors: i64,
    pub baseline_errors: i64,
}

impl From<db::AnomalyEntry> for AnomalyEntry {
    fn from(value: db::AnomalyEntry) -> Self {
        Self {
            hostname: value.hostname,
            recent_count: value.recent_count,
            baseline_count: value.baseline_count,
            recent_per_min: value.recent_per_min,
            baseline_per_min: value.baseline_per_min,
            ratio: value.ratio,
            z_score: value.z_score,
            recent_errors: value.recent_errors,
            baseline_errors: value.baseline_errors,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareRequest {
    pub a_from: String,
    pub a_to: String,
    pub b_from: String,
    pub b_to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareResponse {
    pub a: RangeSummary,
    pub b: RangeSummary,
    pub delta_total_logs: i64,
    pub delta_total_errors: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeSummary {
    pub from: String,
    pub to: String,
    pub total_logs: i64,
    pub total_errors: i64,
    pub by_severity: Vec<(String, i64)>,
    pub top_hosts: Vec<(String, i64)>,
    pub top_apps: Vec<(String, i64)>,
}

impl From<db::RangeSummary> for RangeSummary {
    fn from(value: db::RangeSummary) -> Self {
        Self {
            from: value.from,
            to: value.to,
            total_logs: value.total_logs,
            total_errors: value.total_errors,
            by_severity: value.by_severity,
            top_hosts: value.top_hosts,
            top_apps: value.top_apps,
        }
    }
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
