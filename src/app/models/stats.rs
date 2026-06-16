use super::*;

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
    /// `None` when the (expensive) FTS diagnostic was skipped — the default
    /// `stats` path skips it. Serialized as `null` so clients can distinguish
    /// "not computed" from "zero phantom rows".
    pub phantom_fts_rows: Option<i64>,
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
#[serde(deny_unknown_fields)]
pub struct ListAppsRequest {
    pub host: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    /// Page size. Default 500, max 5000.
    pub limit: Option<u32>,
    /// Page offset. Default 0.
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAppsResponse {
    pub apps: Vec<AppEntry>,
    /// Total distinct app names matching the filter (across all pages).
    pub total: usize,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListSourceIpsRequest {
    /// Page size. Default 500, max 5000.
    pub limit: Option<u32>,
    /// Page offset. Default 0.
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSourceIpsResponse {
    pub source_ips: Vec<SourceIpEntry>,
    /// Total distinct source IPs in the database (across all pages).
    pub total: usize,
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
#[serde(deny_unknown_fields)]
pub struct TimelineRequest {
    pub bucket: Option<String>,
    pub group_by: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub host: Option<String>,
    pub app: Option<String>,
    pub severity_min: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineResponse {
    pub bucket: String,
    pub group_by: Option<String>,
    pub points: Vec<TimelinePoint>,
    /// For rollup-served buckets (hour/day/week/month), the last refresh time of
    /// the `timeline_hourly` rollup the points were read from — exposes bounded
    /// staleness. `None` for the live `minute` bucket (always current).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollup_as_of: Option<String>,
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
#[serde(deny_unknown_fields)]
pub struct PatternsRequest {
    pub since: Option<String>,
    pub until: Option<String>,
    pub host: Option<String>,
    pub app: Option<String>,
    pub severity_min: Option<String>,
    pub scan_limit: Option<u32>,
    #[serde(alias = "limit")]
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
