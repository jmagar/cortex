use serde::{Deserialize, Serialize};

/// Named struct for a log entry used in batch insertion and the syslog parse pipeline.
///
/// Replaces the former 8-tuple type alias; named fields prevent silent data corruption
/// from positional swaps between structurally identical `String`/`Option<String>` fields.
///
/// For syslog input, `source_ip` records the actual network sender address (IP:port)
/// independent of the hostname claimed in the syslog message body. OTLP stores the
/// peer IP without the ephemeral port. Docker ingest uses configured
/// `docker://host/container/stream` and `docker-event://host/container/action`
/// source identifiers instead.
#[derive(Debug, Clone)]
pub struct LogBatchEntry {
    pub timestamp: String,
    pub hostname: String,
    pub facility: Option<String>,
    pub severity: String,
    pub app_name: Option<String>,
    pub process_id: Option<String>,
    pub message: String,
    pub raw: String,
    /// Source identifier. Syslog input uses the actual network sender address
    /// (IP:port); OTLP uses peer IP; Docker ingest uses
    /// docker://host/container/stream and docker-event://host/container/action.
    pub source_ip: String,
    pub docker_checkpoint: Option<DockerCheckpoint>,
    pub ai_tool: Option<String>,
    pub ai_project: Option<String>,
    pub ai_session_id: Option<String>,
    pub ai_transcript_path: Option<String>,
    pub metadata_json: Option<String>,
    /// HTTP status code (3 digits). Indexed column. Set by `swag` parser.
    pub http_status: Option<i32>,

    /// Authentication outcome ("success" | "failure" | "denied" | "challenge").
    /// Indexed column. Set by `authelia` parser.
    pub auth_outcome: Option<&'static str>,

    /// DNS block decision. `Some(true)` = filtered/blocked, `Some(false)` = explicit
    /// allow, `None` = N/A (rewrites and non-DNS rows). Indexed column.
    pub dns_blocked: Option<bool>,

    /// Normalised event verb (closed enum per parser). Indexed column.
    pub event_action: Option<String>,

    /// Per-row parser diagnostic: "{parser_name}: {ParserError::Display}",
    /// truncated to 512 bytes. No index — diagnostic only.
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DockerCheckpoint {
    pub host_name: String,
    pub container_id: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Default)]
pub struct ListAiSessionsParams {
    pub ai_project: Option<String>,
    pub ai_tool: Option<String>,
    pub hostname: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiSessionEntry {
    pub ai_project: String,
    pub ai_tool: String,
    pub ai_session_id: String,
    pub ai_transcript_path: Option<String>,
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
    pub event_count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct SearchAiSessionsParams {
    pub query: String,
    pub ai_project: Option<String>,
    pub ai_tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
}

/// Error/warning summary entry (one row per hostname+severity, plus optional app_name)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorSummaryEntry {
    pub hostname: String,
    /// Populated when the summary was requested with `group_by=app_name`.
    pub app_name: Option<String>,
    pub severity: String,
    pub count: i64,
}

/// Host registry entry with first/last seen and log count
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostEntry {
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
    pub log_count: i64,
}

/// Database statistics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbStats {
    pub total_logs: i64,
    pub total_hosts: i64,
    pub oldest_log: Option<String>,
    pub newest_log: Option<String>,
    /// Formatted as "X.XX" MB
    pub logical_db_size_mb: String,
    /// Formatted as "X.XX" MB
    pub physical_db_size_mb: String,
    /// Formatted as "X.XX" MB when available
    pub free_disk_mb: Option<String>,
    pub max_db_size_mb: u64,
    pub min_free_disk_mb: u64,
    pub write_blocked: bool,
    /// Phantom FTS rows: entries in logs_fts that no longer have a matching log row.
    /// Accumulate between merge cycles; non-zero value is normal and cleaned up by
    /// periodic fts_incremental_merge. High values indicate merge is falling behind.
    pub phantom_fts_rows: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchedAiSessionEntry {
    pub ai_project: String,
    pub ai_tool: String,
    pub ai_session_id: String,
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
    pub event_count: i64,
    pub match_count: i64,
    pub best_snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchAiSessionsResult {
    pub total_candidates: usize,
    pub candidate_rows: usize,
    pub candidate_cap: usize,
    pub candidate_window_truncated: bool,
    pub truncated: bool,
    pub sessions: Vec<SearchedAiSessionEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct AiAbuseParams {
    pub ai_project: Option<String>,
    pub ai_tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub before: Option<u32>,
    pub after: Option<u32>,
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAbuseMatch {
    pub term: String,
    pub entry: LogEntry,
    pub before: Vec<LogEntry>,
    pub after: Vec<LogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAbuseResult {
    pub terms: Vec<String>,
    pub candidate_rows: usize,
    pub candidate_cap: usize,
    pub candidate_window_truncated: bool,
    pub truncated: bool,
    pub matches: Vec<AiAbuseMatch>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiCorrelateParams {
    pub ai_project: Option<String>,
    pub ai_tool: Option<String>,
    pub ai_session_id: Option<String>,
    pub ai_query: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRelatedWindow {
    pub anchor_index: usize,
    pub window_from: String,
    pub window_to: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiRelatedLogsParams {
    pub windows: Vec<AiRelatedWindow>,
    pub query: Option<String>,
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub severity_in: Vec<String>,
    pub app_name: Option<String>,
    pub limit_per_anchor: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRelatedLogsForAnchor {
    pub anchor_index: usize,
    pub logs: Vec<LogEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiUsageBlocksParams {
    pub ai_project: Option<String>,
    pub ai_tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiUsageBlock {
    pub bucket_start: String,
    pub bucket_end: String,
    pub project: String,
    pub tool: String,
    pub session_count: i64,
    pub event_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiUsageBlocksResult {
    pub total_blocks: usize,
    pub truncated: bool,
    pub blocks: Vec<AiUsageBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiProjectContextParams {
    pub project: String,
    pub ai_tool: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProjectContext {
    pub project: String,
    pub tools: Vec<String>,
    pub sessions: Vec<String>,
    pub hostnames: Vec<String>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    pub event_count: i64,
    pub recent_entries_truncated: bool,
    pub recent_entries: Vec<LogEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListAiToolsParams {
    pub ai_project: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolInventoryEntry {
    pub tool: String,
    pub event_count: i64,
    pub session_count: i64,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAiToolsResult {
    pub total_tools: usize,
    pub truncated: bool,
    pub tools: Vec<AiToolInventoryEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListAiProjectsParams {
    pub ai_tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProjectInventoryEntry {
    pub project: String,
    pub tools: Vec<String>,
    pub event_count: i64,
    pub session_count: i64,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAiProjectsResult {
    pub total_projects: usize,
    pub truncated: bool,
    pub projects: Vec<AiProjectInventoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMetrics {
    pub logical_db_size_bytes: u64,
    pub physical_db_size_bytes: u64,
    pub free_disk_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageRecovery {
    pub logical_db_size_bytes: u64,
    pub free_disk_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageEnforcementOutcome {
    pub metrics: StorageMetrics,
    pub recovery: StorageRecovery,
    pub deleted_rows: usize,
    pub write_blocked: bool,
}

#[derive(Debug, Clone)]
pub struct StorageBudgetState {
    pub metrics: StorageMetrics,
    pub write_blocked: bool,
}

/// A parsed and stored log entry
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
    /// Source identifier. Syslog entries use verified network sender address
    /// (IP:port); OTLP entries use peer IP; Docker ingest entries use
    /// docker://host/container/stream or docker-event://host/container/action.
    /// Empty string for legacy rows inserted before this column was added.
    pub source_ip: String,
    pub ai_tool: Option<String>,
    pub ai_project: Option<String>,
    pub ai_session_id: Option<String>,
    pub ai_transcript_path: Option<String>,
    pub metadata_json: Option<String>,
}

/// Parameters for searching logs
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchParams {
    /// Full-text search query (FTS5 syntax)
    pub query: Option<String>,
    /// Filter by hostname
    pub hostname: Option<String>,
    /// Filter by source identifier. Syslog uses verified network sender address
    /// (IP:port); OTLP uses peer IP; Docker ingest uses
    /// docker://host/container/stream or docker-event://host/container/action.
    pub source_ip: Option<String>,
    /// Filter by severity (exact match: emerg, alert, crit, err, warning, notice, info, debug)
    pub severity: Option<String>,
    /// Filter by one of a set of severity levels (for threshold queries)
    pub severity_in: Option<Vec<String>>,
    /// Filter by app name
    pub app_name: Option<String>,
    /// Filter by syslog facility name (e.g. `kern`, `auth`, `daemon`)
    pub facility: Option<String>,
    /// Exclude a syslog facility while keeping rows with unknown facility.
    pub exclude_facility: Option<String>,
    /// Filter by process_id (exact match)
    pub process_id: Option<String>,
    /// Start of time range (ISO 8601)
    pub from: Option<String>,
    /// End of time range (ISO 8601)
    pub to: Option<String>,
    /// Start of receive-time range (ISO 8601)
    pub received_from: Option<String>,
    /// End of receive-time range (ISO 8601)
    pub received_to: Option<String>,
    /// Max results to return
    pub limit: Option<u32>,
    pub ai_tool: Option<String>,
    pub ai_project: Option<String>,
    pub ai_session_id: Option<String>,
    pub exclude_ai: bool,
}

// ---------------------------------------------------------------------------
// Abuse incident grouping
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiIncidentParams {
    pub ai_project: Option<String>,
    pub ai_tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// Max incidents to return. Default 20, clamp 1..=100.
    pub limit: Option<u32>,
    /// Grouping window in minutes. Default 10, clamp 1..=120.
    pub window_minutes: Option<u32>,
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbuseIncident {
    /// Stable synthetic ID: sha256 of "project|tool|session|host|first_anchor_id".
    pub incident_id: String,
    pub project: String,
    pub tool: String,
    pub session_id: String,
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
    pub duration_secs: i64,
    pub abuse_count: usize,
    /// Distinct normalized abuse terms found, sorted.
    pub terms: Vec<String>,
    /// Sorted anchor log IDs.
    pub anchor_ids: Vec<i64>,
    pub priority_score: f64,
    /// "low" | "medium" | "high" | "critical"
    pub priority_label: String,
    pub window_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiIncidentResult {
    pub incidents: Vec<AbuseIncident>,
    pub total_incidents: usize,
    pub candidate_rows: usize,
    pub candidate_cap: usize,
    pub candidate_window_truncated: bool,
    pub truncated: bool,
}

// ---------------------------------------------------------------------------
// AI investigate — evidence bundle layer (kmib.2)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiInvestigateParams {
    pub ai_project: Option<String>,
    pub ai_tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// Max incidents to investigate. Default 3, clamp 1..=10.
    pub limit: Option<u32>,
    /// Incident grouping window minutes. Default 10, clamp 1..=120.
    pub window_minutes: Option<u32>,
    /// Correlation window minutes around incident. Default 5, clamp 1..=120.
    pub correlation_window_minutes: Option<u32>,
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentEvidence {
    pub incident: AbuseIncident,
    /// Transcript entries before first anchor (same session), capped at 20.
    pub transcript_before: Vec<LogEntry>,
    pub transcript_before_truncated: bool,
    /// Transcript entries after last anchor (same session), capped at 20.
    pub transcript_after: Vec<LogEntry>,
    pub transcript_after_truncated: bool,
    /// The abuse anchor log entries.
    pub anchors: Vec<LogEntry>,
    /// Non-AI syslog/Docker logs in the correlation window, capped at 50.
    pub nearby_logs: Vec<LogEntry>,
    pub nearby_logs_truncated: bool,
    /// Subset of nearby_logs with severity warning or above.
    pub nearby_errors: Vec<LogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiInvestigateResult {
    pub evidence: Vec<IncidentEvidence>,
    pub total_incidents: usize,
    pub truncated: bool,
}

// ---------------------------------------------------------------------------
// RAG v1: similar_incidents, ask_history, incident_context
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SimilarIncidentsParams {
    pub query: String,
    pub hostname: Option<String>,
    pub app_name: Option<String>,
    /// Minimum severity (e.g. "warning"). None = all severities.
    pub severity_min: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// Cluster grouping window in minutes. Default 30, clamp 5..=120.
    pub window_minutes: Option<u32>,
    /// Max clusters to return. Default 10, clamp 1..=50.
    pub limit: Option<u32>,
}

/// A time-windowed cluster of log hits (one "incident").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentCluster {
    pub hostname: String,
    pub app_name: Option<String>,
    /// RFC 3339 timestamp of the first matching log in this cluster.
    pub window_start: String,
    /// RFC 3339 timestamp of the last matching log in this cluster.
    pub window_end: String,
    pub log_count: i64,
    /// Highest severity in this cluster (emerg > alert > ... > debug).
    pub severity_peak: String,
    /// Up to 3 representative message snippets (first 256 chars each).
    pub representative_messages: Vec<String>,
    /// AI sessions whose transcript entries overlap this cluster's time window.
    pub correlated_sessions: Vec<CorrelatedSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelatedSession {
    pub session_id: String,
    pub project: String,
    pub tool: String,
    pub match_count: i64,
    pub best_snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarIncidentsResult {
    pub query: String,
    pub total_clusters: usize,
    pub truncated: bool,
    pub clusters: Vec<IncidentCluster>,
}

#[derive(Debug, Clone, Default)]
pub struct AskHistoryParams {
    pub query: String,
    pub hostname: Option<String>,
    pub app_name: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// Max sessions to return. Default 10, clamp 1..=50.
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskHistoryResult {
    pub query: String,
    pub total_candidates: usize,
    pub truncated: bool,
    /// Sessions with transcript hits ranked by match count.
    pub sessions: Vec<SearchedAiSessionEntry>,
    /// System (non-AI) log entries from the same time windows as the top sessions.
    pub context_logs: Vec<LogEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct IncidentContextParams {
    pub from: String,
    pub to: String,
    pub hostname: Option<String>,
    pub app_name: Option<String>,
    pub query: Option<String>,
    pub severity_min: Option<String>,
    /// Max error log rows to return. Default 50, clamp 1..=200.
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeverityCount {
    pub severity: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppLogCount {
    pub app_name: Option<String>,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentContextResult {
    pub window_from: String,
    pub window_to: String,
    pub total_logs: i64,
    pub by_severity: Vec<SeverityCount>,
    pub by_app: Vec<AppLogCount>,
    /// Logs at or above severity_min (default: warning) within the window.
    pub error_logs: Vec<LogEntry>,
    pub error_logs_truncated: bool,
    /// AI sessions active in this window (have transcript entries between from..to).
    pub ai_sessions: Vec<AiSessionEntry>,
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
