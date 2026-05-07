#![allow(unused_imports)]

mod analytics;
mod ingest;
mod maintenance;
mod models;
mod pool;
mod queries;

pub use analytics::{
    anomalies, clock_skew, context_around, fetch_log_by_id, ingest_rate, ingest_rate_by_host,
    list_apps, list_source_ips, patterns, silent_hosts, summarize_range, timeline, AnomalyEntry,
    AppEntry, Bucket, ClockSkewEntry, ContextRef, IngestRateBuckets, IngestRatePerHost,
    LogEntryWithRaw, PatternEntry, RangeSummary, SilentHostEntry, SourceIpEntry,
    SourceIpHostBreakdown, TimelineGroupBy, TimelinePoint,
};
pub use ingest::insert_logs_batch;
pub use maintenance::{
    enforce_storage_budget, get_storage_metrics, purge_by_tag_window, purge_old_logs,
    DiskSpaceProbe,
};
pub use models::{
    DbStats, DockerCheckpoint, ErrorSummaryEntry, HostEntry, LogBatchEntry, LogEntry, SearchParams,
};
pub use models::{StorageBudgetState, StorageEnforcementOutcome, StorageMetrics, StorageRecovery};
pub use pool::{init_pool, DbPool};
pub use queries::{
    get_error_summary, get_stats, list_hosts, search_logs, severity_to_num, tail_logs,
    validate_fts_query, SEVERITY_LEVELS,
};
