use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchLogsRequest {
    pub query: Option<String>,
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub severity: Option<String>,
    pub app_name: Option<String>,
    pub facility: Option<String>,
    pub exclude_facility: Option<String>,
    pub process_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub received_from: Option<String>,
    pub received_to: Option<String>,
    pub limit: Option<u32>,
    pub source_kind: Option<String>,
    pub tool: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub container: Option<String>,
    pub docker_host: Option<String>,
    pub stream: Option<String>,
    pub event_action: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterLogsRequest {
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub severity: Option<String>,
    pub app_name: Option<String>,
    pub facility: Option<String>,
    pub exclude_facility: Option<String>,
    pub process_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub received_from: Option<String>,
    pub received_to: Option<String>,
    pub limit: Option<u32>,
    pub source_kind: Option<String>,
    pub tool: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub container: Option<String>,
    pub docker_host: Option<String>,
    pub stream: Option<String>,
    pub event_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchLogsResponse {
    pub count: usize,
    pub logs: Vec<LogEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct GetErrorsRequest {
    pub from: Option<String>,
    pub to: Option<String>,
    /// Secondary grouping key. Currently supports `app_name`.
    pub group_by: Option<String>,
    /// Max summary rows to return. Defaults to all rows; clamped by service.
    pub limit: Option<u32>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomelabMapRequest {
    /// Maximum host nodes to return. Default 100, max 500.
    pub host_limit: Option<u32>,
    /// Maximum source IPs and apps attached to each host. Default 10, max 25.
    pub per_host_limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomelabMapResponse {
    pub schema: String,
    pub generated_at: String,
    pub summary: HomelabMapSummary,
    pub nodes: Vec<HomelabMapNode>,
    pub inventory_sources: Vec<HomelabMapInventorySource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomelabMapSummary {
    pub hosts: usize,
    pub returned_hosts: usize,
    pub source_ips: usize,
    pub apps: usize,
    pub heartbeat_hosts: usize,
    pub truncated_hosts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomelabMapNode {
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
    pub log_count: i64,
    pub source_ips: Vec<HomelabMapSourceIp>,
    pub apps: Vec<HomelabMapApp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat: Option<FleetStateHostRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomelabMapSourceIp {
    pub source_ip: String,
    pub log_count: i64,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomelabMapApp {
    pub app_name: String,
    pub log_count: i64,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomelabMapInventorySource {
    pub name: String,
    pub source: String,
    pub status: String,
    pub collects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
