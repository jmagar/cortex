#![allow(unused_imports)]

mod analytics;
pub(crate) mod error_signatures;
pub mod graph;
pub mod graph_findings;
pub mod graph_inventory;
mod heartbeat;
mod ingest;
mod maintenance;
mod models;
pub(crate) mod notifications;
mod pool;
mod queries;

pub(crate) use analytics::PATTERN_SCAN_LIMIT_MAX;
pub use analytics::{
    anomalies, clock_skew, context_around, fetch_log_by_id, get_ai_project_context,
    get_ai_usage_blocks, ingest_rate, ingest_rate_by_host, list_apps, list_source_ips,
    silent_hosts, summarize_range, timeline, AnomalyEntry, AppEntry, Bucket, ClockSkewEntry,
    ContextRef, IngestRateBuckets, IngestRatePerHost, ListAppsParams, ListAppsResult,
    ListSourceIpsParams, ListSourceIpsResult, LogEntryWithRaw, PatternEntry, RangeSummary,
    SilentHostEntry, SourceIpEntry, SourceIpHostBreakdown, TimelineGroupBy, TimelinePoint,
};
pub(crate) use analytics::{cluster_pattern_rows, fetch_pattern_rows};
pub use graph::{
    is_known_entity_type, is_known_evidence_source_kind, is_known_reason_code,
    is_known_relationship_type, is_known_trust_level, ENTITY_TYPES, EVIDENCE_SOURCE_KINDS,
    PROJECTION_STATUSES, REASON_CODES, RELATIONSHIP_TYPES, TRUST_LEVELS,
};
pub use graph_findings::{
    list_mount_relationship_findings, list_public_route_findings, MountRelationshipFindingRow,
    PublicRouteFindingRow,
};
pub use heartbeat::{
    heartbeat_host_state, heartbeat_latest_all, heartbeat_metric_snapshot_batch,
    heartbeat_window_summaries, HeartbeatHostLookup, HeartbeatHostState, HeartbeatLatestEntry,
    HeartbeatMetricSnapshot, HeartbeatSampleState, HeartbeatStateFlags, HeartbeatWindowSummary,
};
pub use ingest::insert_logs_batch;
pub(crate) use ingest::insert_logs_batch_in_tx;
pub use maintenance::{
    db_full_vacuum, db_incremental_vacuum, db_integrity_check, db_wal_checkpoint,
    enforce_storage_budget, enforce_storage_budget_with_state, exceeds_trigger,
    finish_maintenance_job, get_maintenance_job, get_storage_metrics, insert_maintenance_job,
    physical_size_bytes, purge_by_tag_window, purge_old_heartbeats, purge_old_logs, DiskSpaceProbe,
    MaintenanceJob, SystemDiskSpaceProbe,
};
pub(crate) use maintenance::{db_pragma_i64, db_pragma_string, PragmaName};
pub use models::{
    AbuseIncident, AiAbuseMatch, AiAbuseParams, AiAbuseResult, AiCorrelateParams, AiIncidentParams,
    AiIncidentResult, AiInvestigateParams, AiInvestigateResult, AiProjectContext,
    AiProjectContextParams, AiProjectInventoryEntry, AiRelatedLogsForAnchor, AiRelatedLogsParams,
    AiRelatedWindow, AiSessionEntry, AiToolInventoryEntry, AiUsageBlock, AiUsageBlocksParams,
    AiUsageBlocksResult, AppLogCount, AskHistoryParams, AskHistoryResult, CorrelatedSession,
    DbStats, DockerCheckpoint, ErrorSummaryEntry, HostEntry, IncidentCluster,
    IncidentContextParams, IncidentContextResult, IncidentEvidence, ListAiProjectsParams,
    ListAiProjectsResult, ListAiSessionsParams, ListAiToolsParams, ListAiToolsResult,
    LogBatchEntry, LogEntry, SearchAiSessionsParams, SearchAiSessionsResult, SearchParams,
    SearchedAiSessionEntry, SeverityCount, SimilarIncidentsParams, SimilarIncidentsResult,
};
pub use models::{StorageBudgetState, StorageEnforcementOutcome, StorageMetrics, StorageRecovery};
pub use pool::{
    backfill_inventory_stats, init_pool, inventory_backfill_complete, read_schema_version_info,
    read_schema_version_info_conn, write_lock, DbPool, SchemaVersionInfo, KNOWN_SCHEMA_VERSION,
};
pub use queries::{
    ai_session_rollup_status, ask_history_sessions, get_error_summary, get_stats,
    get_stats_with_options, incident_context_summary, investigate_ai_incidents, list_ai_projects,
    list_ai_sessions, list_ai_sessions_live, list_ai_tools, list_hosts, prune_timeline_rollup,
    refresh_ai_session_rollup, refresh_ai_session_rollup_if_stale, refresh_timeline_rollup,
    search_ai_abuse, search_ai_anchors, search_ai_incidents, search_ai_related_logs,
    search_ai_sessions, search_logs, severity_to_num, similar_incidents_clusters, tail_logs,
    timeline_rollup_status, validate_fts_query, AiSessionRollupStatus, RollupRefresh,
    TimelineRollupStatus, SEVERITY_LEVELS,
};
