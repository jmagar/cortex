#![allow(unused_imports)]

mod analytics;
pub(crate) mod error_signatures;
mod ingest;
mod maintenance;
mod models;
pub(crate) mod notifications;
mod pool;
mod queries;

pub use analytics::{
    anomalies, clock_skew, context_around, fetch_log_by_id, get_ai_project_context,
    get_ai_usage_blocks, ingest_rate, ingest_rate_by_host, list_apps, list_source_ips, patterns,
    silent_hosts, summarize_range, timeline, AnomalyEntry, AppEntry, Bucket, ClockSkewEntry,
    ContextRef, IngestRateBuckets, IngestRatePerHost, ListAppsParams, ListAppsResult,
    ListSourceIpsParams, ListSourceIpsResult, LogEntryWithRaw, PatternEntry, RangeSummary,
    SilentHostEntry, SourceIpEntry, SourceIpHostBreakdown, TimelineGroupBy, TimelinePoint,
};
pub use ingest::insert_logs_batch;
pub(crate) use ingest::insert_logs_batch_in_tx;
pub use maintenance::{
    db_full_vacuum, db_incremental_vacuum, db_integrity_check, db_wal_checkpoint,
    enforce_storage_budget, get_storage_metrics, physical_size_bytes, purge_by_tag_window,
    purge_old_logs, DiskSpaceProbe,
};
pub(crate) use maintenance::{db_pragma_i64, db_pragma_string};
pub use models::{
    AbuseIncident, AiAbuseMatch, AiAbuseParams, AiAbuseResult, AiCorrelateParams, AiIncidentParams,
    AiIncidentResult, AiInvestigateParams, AiInvestigateResult, AiProjectContext,
    AiProjectContextParams, AiProjectInventoryEntry, AiRelatedLogsForAnchor, AiRelatedLogsParams,
    AiRelatedWindow, AiSessionEntry, AiToolInventoryEntry, AiUsageBlock, AiUsageBlocksParams,
    AiUsageBlocksResult, DbStats, DockerCheckpoint, ErrorSummaryEntry, HostEntry, IncidentEvidence,
    ListAiProjectsParams, ListAiProjectsResult, ListAiSessionsParams, ListAiToolsParams,
    ListAiToolsResult, LogBatchEntry, LogEntry, SearchAiSessionsParams, SearchAiSessionsResult,
    SearchParams, SearchedAiSessionEntry,
};
pub use models::{StorageBudgetState, StorageEnforcementOutcome, StorageMetrics, StorageRecovery};
pub use pool::{
    init_pool, read_schema_version_info, read_schema_version_info_conn, DbPool, SchemaVersionInfo,
    KNOWN_SCHEMA_VERSION,
};
pub use queries::{
    get_error_summary, get_stats, investigate_ai_incidents, list_ai_projects, list_ai_sessions,
    list_ai_tools, list_hosts, search_ai_abuse, search_ai_anchors, search_ai_incidents,
    search_ai_related_logs, search_ai_sessions, search_logs, severity_to_num, tail_logs,
    validate_fts_query, SEVERITY_LEVELS,
};
