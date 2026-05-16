#![allow(unused_imports)]

mod analytics;
pub(crate) mod error_signatures;
mod ingest;
mod maintenance;
mod models;
mod pool;
mod queries;

pub use analytics::{
    anomalies, clock_skew, context_around, fetch_log_by_id, get_ai_project_context,
    get_ai_usage_blocks, ingest_rate, ingest_rate_by_host, list_apps, list_source_ips, patterns,
    silent_hosts, summarize_range, timeline, AnomalyEntry, AppEntry, Bucket, ClockSkewEntry,
    ContextRef, IngestRateBuckets, IngestRatePerHost, LogEntryWithRaw, PatternEntry, RangeSummary,
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
    AiAbuseMatch, AiAbuseParams, AiAbuseResult, AiCorrelateParams, AiProjectContext,
    AiProjectContextParams, AiProjectInventoryEntry, AiSessionEntry, AiToolInventoryEntry,
    AiUsageBlock, AiUsageBlocksParams, AiUsageBlocksResult, DbStats, DockerCheckpoint,
    ErrorSummaryEntry, HostEntry, ListAiProjectsParams, ListAiProjectsResult, ListAiSessionsParams,
    ListAiToolsParams, ListAiToolsResult, LogBatchEntry, LogEntry, SearchAiSessionsParams,
    SearchAiSessionsResult, SearchParams, SearchedAiSessionEntry,
};
pub use models::{StorageBudgetState, StorageEnforcementOutcome, StorageMetrics, StorageRecovery};
pub use pool::{init_pool, DbPool};
pub use queries::{
    get_error_summary, get_stats, list_ai_projects, list_ai_sessions, list_ai_tools, list_hosts,
    search_ai_abuse, search_ai_anchors, search_ai_sessions, search_logs, severity_to_num,
    tail_logs, validate_fts_query, SEVERITY_LEVELS,
};
