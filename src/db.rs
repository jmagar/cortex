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
    AnomalyEntry, AppEntry, Bucket, ClockSkewEntry, ContextRef, IngestRateBuckets,
    IngestRatePerHost, ListAppsParams, ListAppsResult, ListSourceIpsParams, ListSourceIpsResult,
    LogEntryWithRaw, PatternEntry, RangeSummary, SilentHostEntry, SourceIpEntry,
    SourceIpHostBreakdown, TimelineGroupBy, TimelinePoint, anomalies, clock_skew, context_around,
    fetch_log_by_id, get_ai_project_context, get_ai_usage_blocks, ingest_rate, ingest_rate_by_host,
    list_apps, list_source_ips, silent_hosts, summarize_range, timeline,
};
pub(crate) use analytics::{cluster_pattern_rows, fetch_pattern_rows};
pub use graph::{
    ENTITY_TYPES, EVIDENCE_SOURCE_KINDS, GRAPH_WALK_MAX_DEPTH, GraphWalkEntity,
    PROJECTION_STATUSES, REASON_CODES, RELATIONSHIP_TYPES, TRUST_LEVELS, graph_walk_n_hops,
    is_known_entity_type, is_known_evidence_source_kind, is_known_reason_code,
    is_known_relationship_type, is_known_trust_level,
};
pub use graph_findings::{
    MountRelationshipFindingRow, PublicRouteFindingRow, list_mount_relationship_findings,
    list_public_route_findings,
};
pub use heartbeat::{
    HeartbeatHostLookup, HeartbeatHostState, HeartbeatLatestEntry, HeartbeatMetricSnapshot,
    HeartbeatSampleState, HeartbeatStateFlags, HeartbeatWindowSummary, heartbeat_host_state,
    heartbeat_latest_all, heartbeat_metric_snapshot_batch, heartbeat_window_summaries,
};
pub use ingest::insert_logs_batch;
pub(crate) use ingest::insert_logs_batch_in_tx;
pub use maintenance::{
    DiskSpaceProbe, MaintenanceJob, SystemDiskSpaceProbe, db_full_vacuum, db_incremental_vacuum,
    db_integrity_check, db_wal_checkpoint, enforce_storage_budget,
    enforce_storage_budget_with_state, exceeds_trigger, finish_maintenance_job,
    get_maintenance_job, get_storage_metrics, insert_maintenance_job, physical_size_bytes,
    purge_by_tag_window, purge_old_heartbeats, purge_old_logs,
};
pub(crate) use maintenance::{PragmaName, db_pragma_i64, db_pragma_string};
pub use models::{
    AbuseIncident, AiAbuseMatch, AiAbuseParams, AiAbuseResult, AiCorrelateParams, AiIncidentParams,
    AiIncidentResult, AiInvestigateParams, AiInvestigateResult, AiProjectContext,
    AiProjectContextParams, AiProjectInventoryEntry, AiRelatedLogsForAnchor, AiRelatedLogsParams,
    AiRelatedWindow, AiSessionEntry, AiToolInventoryEntry, AiUsageBlock, AiUsageBlocksParams,
    AiUsageBlocksResult, AppLogCount, AskHistoryParams, AskHistoryResult, CorrelatedSession,
    DbStats, DockerCheckpoint, ErrorSummaryEntry, HostEntry, IncidentCluster,
    IncidentContextParams, IncidentContextResult, IncidentEvidence, ListAiProjectsParams,
    ListAiProjectsResult, ListAiSessionsParams, ListAiToolsParams, ListAiToolsResult,
    LogBatchEntry, LogEntry, ResolvedTopicEntity, SearchAiSessionsParams, SearchAiSessionsResult,
    SearchParams, SearchedAiSessionEntry, SessionGraphInputs, SeverityCount,
    SimilarIncidentsParams, SimilarIncidentsResult, TopicGraphInputs,
};
pub use models::{StorageBudgetState, StorageEnforcementOutcome, StorageMetrics, StorageRecovery};
pub use pool::{
    DbPool, KNOWN_SCHEMA_VERSION, SchemaVersionInfo, backfill_inventory_stats, init_pool,
    inventory_backfill_complete, read_schema_version_info, read_schema_version_info_conn,
    write_lock,
};
pub use queries::{
    AiSessionRollupStatus, RollupRefresh, SEVERITY_LEVELS, TimelineRollupStatus,
    ai_session_rollup_status, ask_history_sessions, correlate_session_graph, get_error_summary,
    get_stats, get_stats_with_options, incident_context_summary, investigate_ai_incidents,
    list_ai_projects, list_ai_sessions, list_ai_sessions_live, list_ai_tools, list_hosts,
    prune_timeline_rollup, refresh_ai_session_rollup, refresh_ai_session_rollup_if_stale,
    refresh_timeline_rollup, search_ai_abuse, search_ai_anchors, search_ai_incidents,
    search_ai_related_logs, search_ai_sessions, search_logs,
    search_logs_from_graph_related_entities, severity_to_num, similar_incidents_clusters,
    tail_logs, timeline_rollup_status, topic_correlate_inputs, validate_fts_query,
};
