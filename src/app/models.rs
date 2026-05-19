use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbMaintenanceStatus {
    pub db_path: PathBuf,
    pub page_count: i64,
    pub freelist_count: i64,
    pub page_size: i64,
    pub logical_size_bytes: u64,
    pub physical_size_bytes: u64,
    pub wal_size_bytes: Option<u64>,
    pub shm_size_bytes: Option<u64>,
    pub auto_vacuum: i64,
    pub journal_mode: String,
    pub integrity_ok: Option<bool>,
    pub integrity_messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbCheckpointResult {
    pub mode: String,
    pub busy: i64,
    pub log_frames: i64,
    pub checkpointed_frames: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbVacuumResult {
    pub full: bool,
    pub incremental_pages: u32,
    pub before_physical_size_bytes: u64,
    pub after_physical_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbIntegrityResult {
    pub ok: bool,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbBackupResult {
    pub db_path: PathBuf,
    pub backup_path: PathBuf,
    pub size_bytes: u64,
}

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
    pub ai_tool: Option<String>,
    pub ai_project: Option<String>,
    pub ai_session_id: Option<String>,
    pub ai_transcript_path: Option<String>,
    pub metadata_json: Option<String>,
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
            ai_tool: value.ai_tool,
            ai_project: value.ai_project,
            ai_session_id: value.ai_session_id,
            ai_transcript_path: value.ai_transcript_path,
            metadata_json: value.metadata_json,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListSessionsRequest {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub hostname: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSessionsResponse {
    pub count: usize,
    pub sessions: Vec<AiSessionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSessionEntry {
    /// Stable response-local key for this host/tool/project/session tuple.
    pub session_key: String,
    pub project: String,
    pub tool: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
    pub event_count: i64,
}

impl From<db::AiSessionEntry> for AiSessionEntry {
    fn from(value: db::AiSessionEntry) -> Self {
        let session_key = ai_session_key(
            &value.hostname,
            &value.ai_tool,
            &value.ai_project,
            &value.ai_session_id,
        );
        Self {
            session_key,
            project: value.ai_project,
            tool: value.ai_tool,
            session_id: value.ai_session_id,
            transcript_path: value.ai_transcript_path,
            hostname: value.hostname,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
            event_count: value.event_count,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchSessionsRequest {
    pub query: String,
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchedSessionEntry {
    /// Stable response-local key for this host/tool/project/session tuple.
    pub session_key: String,
    pub project: String,
    pub tool: String,
    pub session_id: String,
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
    pub event_count: i64,
    pub match_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_snippet: Option<String>,
}

impl From<db::SearchedAiSessionEntry> for SearchedSessionEntry {
    fn from(value: db::SearchedAiSessionEntry) -> Self {
        let session_key = ai_session_key(
            &value.hostname,
            &value.ai_tool,
            &value.ai_project,
            &value.ai_session_id,
        );
        Self {
            session_key,
            project: value.ai_project,
            tool: value.ai_tool,
            session_id: value.ai_session_id,
            hostname: value.hostname,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
            event_count: value.event_count,
            match_count: value.match_count,
            best_snippet: value.best_snippet,
        }
    }
}

fn ai_session_key(hostname: &str, tool: &str, project: &str, session_id: &str) -> String {
    [hostname, tool, project, session_id]
        .into_iter()
        .map(|part| format!("{}:{part}", part.len()))
        .collect::<Vec<_>>()
        .join("|")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSessionsResponse {
    pub total_candidates: usize,
    pub candidate_rows: usize,
    pub candidate_cap: usize,
    pub candidate_window_truncated: bool,
    pub truncated: bool,
    pub sessions: Vec<SearchedSessionEntry>,
    /// Set by the REST handler when the caller-supplied `limit` exceeded the
    /// server-side hard cap and was clamped down. Omitted from MCP/non-REST
    /// responses, where no clamp is applied at this layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_clamped_to: Option<u32>,
}

impl From<db::SearchAiSessionsResult> for SearchSessionsResponse {
    fn from(value: db::SearchAiSessionsResult) -> Self {
        Self {
            total_candidates: value.total_candidates,
            candidate_rows: value.candidate_rows,
            candidate_cap: value.candidate_cap,
            candidate_window_truncated: value.candidate_window_truncated,
            truncated: value.truncated,
            sessions: value.sessions.into_iter().map(Into::into).collect(),
            limit_clamped_to: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbuseSearchRequest {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub before: Option<u32>,
    pub after: Option<u32>,
    #[serde(default)]
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbuseMatch {
    pub term: String,
    pub entry: LogEntry,
    pub before: Vec<LogEntry>,
    pub after: Vec<LogEntry>,
}

impl From<db::AiAbuseMatch> for AbuseMatch {
    fn from(value: db::AiAbuseMatch) -> Self {
        Self {
            term: value.term,
            entry: value.entry.into(),
            before: value.before.into_iter().map(Into::into).collect(),
            after: value.after.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbuseSearchResponse {
    pub terms: Vec<String>,
    pub candidate_rows: usize,
    pub candidate_cap: usize,
    pub candidate_window_truncated: bool,
    pub truncated: bool,
    pub matches: Vec<AbuseMatch>,
    /// Set by the REST handler when the caller-supplied `limit` exceeded the
    /// server-side hard cap and was clamped down. Omitted on MCP responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_clamped_to: Option<u32>,
}

impl From<db::AiAbuseResult> for AbuseSearchResponse {
    fn from(value: db::AiAbuseResult) -> Self {
        Self {
            terms: value.terms,
            candidate_rows: value.candidate_rows,
            candidate_cap: value.candidate_cap,
            candidate_window_truncated: value.candidate_window_truncated,
            truncated: value.truncated,
            matches: value.matches.into_iter().map(Into::into).collect(),
            limit_clamped_to: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiIncidentRequest {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub window_minutes: Option<u32>,
    #[serde(default)]
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiIncidentResponse {
    pub incidents: Vec<AbuseIncident>,
    pub total_incidents: usize,
    pub candidate_rows: usize,
    pub candidate_cap: usize,
    pub candidate_window_truncated: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbuseIncident {
    pub incident_id: String,
    pub project: String,
    pub tool: String,
    pub session_id: String,
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
    pub duration_secs: i64,
    pub abuse_count: usize,
    pub terms: Vec<String>,
    pub anchor_ids: Vec<i64>,
    pub priority_score: f64,
    pub priority_label: String,
    pub window_minutes: u32,
}

impl From<db::AbuseIncident> for AbuseIncident {
    fn from(v: db::AbuseIncident) -> Self {
        Self {
            incident_id: v.incident_id,
            project: v.project,
            tool: v.tool,
            session_id: v.session_id,
            hostname: v.hostname,
            first_seen: v.first_seen,
            last_seen: v.last_seen,
            duration_secs: v.duration_secs,
            abuse_count: v.abuse_count,
            terms: v.terms,
            anchor_ids: v.anchor_ids,
            priority_score: v.priority_score,
            priority_label: v.priority_label,
            window_minutes: v.window_minutes,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiInvestigateRequest {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub window_minutes: Option<u32>,
    pub correlation_window_minutes: Option<u32>,
    #[serde(default)]
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentEvidence {
    pub incident: AbuseIncident,
    pub transcript_before: Vec<LogEntry>,
    pub transcript_before_truncated: bool,
    pub transcript_after: Vec<LogEntry>,
    pub transcript_after_truncated: bool,
    pub anchors: Vec<LogEntry>,
    pub nearby_logs: Vec<LogEntry>,
    pub nearby_logs_truncated: bool,
    pub nearby_errors: Vec<LogEntry>,
}

impl From<db::IncidentEvidence> for IncidentEvidence {
    fn from(v: db::IncidentEvidence) -> Self {
        Self {
            incident: v.incident.into(),
            transcript_before: v.transcript_before.into_iter().map(Into::into).collect(),
            transcript_before_truncated: v.transcript_before_truncated,
            transcript_after: v.transcript_after.into_iter().map(Into::into).collect(),
            transcript_after_truncated: v.transcript_after_truncated,
            anchors: v.anchors.into_iter().map(Into::into).collect(),
            nearby_logs: v.nearby_logs.into_iter().map(Into::into).collect(),
            nearby_logs_truncated: v.nearby_logs_truncated,
            nearby_errors: v.nearby_errors.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiInvestigateResponse {
    pub evidence: Vec<IncidentEvidence>,
    pub total_incidents: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiCorrelateRequest {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub session_id: Option<String>,
    pub ai_query: Option<String>,
    pub log_query: Option<String>,
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub app_name: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub window_minutes: Option<u32>,
    pub severity_min: Option<String>,
    pub limit: Option<u32>,
    pub events_per_anchor: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCorrelationAnchor {
    pub entry: LogEntry,
    pub window_from: String,
    pub window_to: String,
    pub related: Vec<LogEntry>,
    pub related_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCorrelateResponse {
    pub window_minutes: u32,
    pub severity_min: String,
    pub total_anchors: usize,
    pub anchor_rows: usize,
    pub anchor_limit: usize,
    pub anchors_truncated: bool,
    pub related_limit_per_anchor: usize,
    pub total_related_events: usize,
    pub anchors: Vec<AiCorrelationAnchor>,
    /// Set by the REST handler when the caller-supplied `events_per_anchor`
    /// exceeded the server-side hard cap of 50 and was clamped down. Omitted
    /// on MCP responses, which use the service-layer clamp (200) only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events_per_anchor_clamped_to: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageBlocksRequest {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageBlock {
    pub bucket_start: String,
    pub bucket_end: String,
    pub project: String,
    pub tool: String,
    pub session_count: i64,
    pub event_count: i64,
}

impl From<db::AiUsageBlock> for UsageBlock {
    fn from(value: db::AiUsageBlock) -> Self {
        Self {
            bucket_start: value.bucket_start,
            bucket_end: value.bucket_end,
            project: value.project,
            tool: value.tool,
            session_count: value.session_count,
            event_count: value.event_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageBlocksResponse {
    pub total_blocks: usize,
    pub truncated: bool,
    pub blocks: Vec<UsageBlock>,
}

impl From<db::AiUsageBlocksResult> for UsageBlocksResponse {
    fn from(value: db::AiUsageBlocksResult) -> Self {
        Self {
            total_blocks: value.total_blocks,
            truncated: value.truncated,
            blocks: value.blocks.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectContextRequest {
    pub project: String,
    pub tool: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContextResponse {
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

impl From<db::AiProjectContext> for ProjectContextResponse {
    fn from(value: db::AiProjectContext) -> Self {
        Self {
            project: value.project,
            tools: value.tools,
            sessions: value.sessions,
            hostnames: value.hostnames,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
            event_count: value.event_count,
            recent_entries_truncated: value.recent_entries_truncated,
            recent_entries: value.recent_entries.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListAiToolsRequest {
    pub project: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolEntry {
    pub tool: String,
    pub event_count: i64,
    pub session_count: i64,
    pub first_seen: String,
    pub last_seen: String,
}

impl From<db::AiToolInventoryEntry> for AiToolEntry {
    fn from(value: db::AiToolInventoryEntry) -> Self {
        Self {
            tool: value.tool,
            event_count: value.event_count,
            session_count: value.session_count,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAiToolsResponse {
    pub total_tools: usize,
    pub truncated: bool,
    pub tools: Vec<AiToolEntry>,
}

impl From<db::ListAiToolsResult> for ListAiToolsResponse {
    fn from(value: db::ListAiToolsResult) -> Self {
        Self {
            total_tools: value.total_tools,
            truncated: value.truncated,
            tools: value.tools.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListAiProjectsRequest {
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProjectEntry {
    pub project: String,
    pub tools: Vec<String>,
    pub event_count: i64,
    pub session_count: i64,
    pub first_seen: String,
    pub last_seen: String,
}

impl From<db::AiProjectInventoryEntry> for AiProjectEntry {
    fn from(value: db::AiProjectInventoryEntry) -> Self {
        Self {
            project: value.project,
            tools: value.tools,
            event_count: value.event_count,
            session_count: value.session_count,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAiProjectsResponse {
    pub total_projects: usize,
    pub truncated: bool,
    pub projects: Vec<AiProjectEntry>,
}

impl From<db::ListAiProjectsResult> for ListAiProjectsResponse {
    fn from(value: db::ListAiProjectsResult) -> Self {
        Self {
            total_projects: value.total_projects,
            truncated: value.truncated,
            projects: value.projects.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    pub from: Option<String>,
    pub to: Option<String>,
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
    pub ai_tool: Option<String>,
    pub ai_project: Option<String>,
    pub ai_session_id: Option<String>,
    pub ai_transcript_path: Option<String>,
    pub metadata_json: Option<String>,
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
            ai_tool: value.ai_tool,
            ai_project: value.ai_project,
            ai_session_id: value.ai_session_id,
            ai_transcript_path: value.ai_transcript_path,
            metadata_json: value.metadata_json,
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

// ---------------------------------------------------------------------------
// Error Detection models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnaddressedErrorsRequest {
    /// Maximum number of signatures to return.
    pub limit: Option<u32>,
    /// Include already-acknowledged signatures in the result.
    pub include_acknowledged: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnaddressedErrorsResponse {
    pub signatures: Vec<ErrorSignatureEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorSignatureEntry {
    pub signature_hash: String,
    pub template: String,
    pub sample_message: String,
    pub severity: String,
    pub sample_hostname: String,
    pub sample_app_name: Option<String>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub total_count: i64,
    pub count_last_1h: i64,
    pub acknowledged_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckErrorRequest {
    pub signature_hash: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckErrorResponse {
    pub signature_hash: String,
    pub acknowledged_at: String,
    pub actor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnackErrorRequest {
    pub signature_hash: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnackErrorResponse {
    pub signature_hash: String,
    pub unacked_at: String,
    pub actor: String,
}

// ── AI checkpoint inventory + prune request structs (bead 0p8r.3) ────────────
//
// These are typed request shapes shared between the REST handlers in
// `src/api.rs` and the future HTTP client in bead 0p8r.5. The corresponding
// service methods (`list_ai_checkpoints`, `list_ai_parse_errors`,
// `prune_ai_checkpoints` in `src/app/service.rs:609,628,638`) keep their
// loose primitive signatures — handlers unpack the request into positional
// args before calling the service.
//
// `deny_unknown_fields` on all three: typo'd POST/JSON fields must surface
// as 400, not be silently dropped (eng-review #A1 echo).

/// Query parameters for `GET /api/ai/checkpoints`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiCheckpointsRequest {
    /// Restrict to checkpoints with persisted parse errors.
    #[serde(default)]
    pub errors_only: bool,
    /// Restrict to checkpoints whose source file is missing on disk.
    #[serde(default)]
    pub missing_only: bool,
    pub limit: Option<u32>,
}

/// Query parameters for `GET /api/ai/errors`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiParseErrorsRequest {
    pub limit: Option<u32>,
}

/// JSON body for `POST /api/ai/prune-checkpoints`.
///
/// `dry_run` is intentionally `bool` (not `Option<bool>`): the handler
/// pre-validates the JSON body contains the key before deserialization
/// (eng-review C3). Defaulting silently to `false` would let `POST {}`
/// mass-delete checkpoints — instead the handler returns 400.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiPruneCheckpointsRequest {
    /// REQUIRED — must be specified explicitly. See struct docs.
    pub dry_run: bool,
    #[serde(default)]
    pub missing_only: bool,
    pub limit: Option<u32>,
}

/// Query parameters for `GET /api/db/integrity`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbIntegrityRequest {
    /// Use the fast `PRAGMA quick_check` path. `false` (or absent) runs full
    /// `PRAGMA integrity_check`.
    #[serde(default)]
    pub quick: bool,
}

/// JSON body for `POST /api/db/checkpoint`.
///
/// `mode` is validated at the handler entry against
/// `{passive, full, restart, truncate}` (bead 0p8r.4 #A17) — SQLite would
/// also reject unknown modes, but explicit handler-side validation produces
/// a clearer 400 with the allowed list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbCheckpointRequest {
    pub mode: String,
}

/// JSON body for `POST /api/db/vacuum`.
///
/// `force` is intentionally `Option<bool>` (not `bool` with serde default):
/// the size pre-flight on `full == true` is bypassed ONLY when the body
/// explicitly carries `"force": true`. `None` and `Some(false)` both leave
/// the pre-flight in force, defending against accidental
/// `POST {"full":true}` on a multi-GB DB. See bead 0p8r.4 (eng-review C3).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbVacuumRequest {
    #[serde(default)]
    pub full: bool,
    #[serde(default)]
    pub incremental_pages: u32,
    /// Must be `Some(true)` to bypass the 2 GB size pre-flight on full
    /// VACUUM. See struct docs.
    pub force: Option<bool>,
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
