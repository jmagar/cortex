use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeDelta, Utc};
use tokio::sync::Semaphore;

use super::correlate::{group_by_host, severity_at_or_above};
use super::models::{
    AiSessionEntry, AnomaliesRequest, AnomaliesResponse, ClockSkewRequest, ClockSkewResponse,
    CompareRequest, CompareResponse, ContextRequest, ContextResponse, CorrelateEventsRequest,
    CorrelateEventsResponse, DbBackupResult, DbCheckpointResult, DbIntegrityResult,
    DbMaintenanceStatus, DbStats, DbVacuumResult, GetErrorsRequest, GetErrorsResponse,
    GetLogRequest, GetLogResponse, IngestRateRequest, IngestRateResponse, ListAiProjectsRequest,
    ListAiProjectsResponse, ListAiToolsRequest, ListAiToolsResponse, ListAppsRequest,
    ListAppsResponse, ListHostsResponse, ListSessionsRequest, ListSessionsResponse,
    ListSourceIpsResponse, LogEntry, PatternsRequest, PatternsResponse, ProjectContextRequest,
    ProjectContextResponse, SearchLogsRequest, SearchLogsResponse, SearchSessionsRequest,
    SearchSessionsResponse, SilentHostsRequest, SilentHostsResponse, TailLogsRequest,
    TimelineRequest, TimelineResponse, UsageBlocksRequest, UsageBlocksResponse,
};
use super::time::{parse_optional_timestamp, parse_required_timestamp, rfc3339_z};
use super::{ServiceError, ServiceResult};
use crate::config::StorageConfig;
use crate::db::{self, Bucket, ContextRef, DbPool, SearchParams, TimelineGroupBy};
use crate::scanner;

const DB_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct SyslogService {
    pool: Arc<DbPool>,
    storage: StorageConfig,
    db_permits: Arc<Semaphore>,
    acquire_timeout: Duration,
}

impl SyslogService {
    pub(crate) fn new(pool: Arc<DbPool>, storage: StorageConfig) -> Self {
        let permits = storage.pool_size.max(1) as usize;
        Self {
            pool,
            storage,
            db_permits: Arc::new(Semaphore::new(permits)),
            acquire_timeout: DB_ACQUIRE_TIMEOUT,
        }
    }

    async fn run_db<F, T>(&self, f: F) -> ServiceResult<T>
    where
        F: FnOnce(&DbPool) -> anyhow::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let permit = tokio::time::timeout(
            self.acquire_timeout,
            Arc::clone(&self.db_permits).acquire_owned(),
        )
        .await
        .map_err(|_| ServiceError::Busy("database worker limit reached".into()))?
        .map_err(|_| ServiceError::Busy("database worker limit closed".into()))?;
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f(&pool)
        })
        .await
        .map_err(|e| ServiceError::Internal(anyhow::anyhow!("Task join error: {e}")))?
        .map_err(ServiceError::Internal)
    }

    pub async fn health_check(&self) -> ServiceResult<()> {
        self.run_db(|pool| {
            let conn = pool.get()?;
            conn.query_row("SELECT 1", [], |_| Ok(()))?;
            Ok(())
        })
        .await
    }

    pub async fn search_logs(&self, req: SearchLogsRequest) -> ServiceResult<SearchLogsResponse> {
        let severity = validate_optional_severity(req.severity)?;
        let params = SearchParams {
            query: req.query,
            hostname: req.hostname,
            source_ip: req.source_ip,
            severity,
            severity_in: None,
            app_name: req.app_name,
            facility: req.facility,
            process_id: req.process_id,
            from: parse_optional_timestamp(req.from.as_deref(), "from")?,
            to: parse_optional_timestamp(req.to.as_deref(), "to")?,
            limit: req.limit,
            ai_tool: None,
            ai_project: None,
            ai_session_id: None,
        };
        let logs = self
            .run_db(move |pool| db::search_logs(pool, &params))
            .await?;
        let logs: Vec<LogEntry> = logs.into_iter().map(Into::into).collect();
        Ok(SearchLogsResponse {
            count: logs.len(),
            logs,
        })
    }

    pub async fn tail_logs(&self, req: TailLogsRequest) -> ServiceResult<SearchLogsResponse> {
        let severity_in = match req.severity_min.as_deref() {
            Some(min) => Some(severity_at_or_above(min)?),
            None => None,
        };
        let logs = self
            .run_db(move |pool| {
                db::tail_logs(
                    pool,
                    req.hostname.as_deref(),
                    req.source_ip.as_deref(),
                    req.app_name.as_deref(),
                    severity_in.as_deref(),
                    req.n.unwrap_or(50),
                )
            })
            .await?;
        let logs: Vec<LogEntry> = logs.into_iter().map(Into::into).collect();
        Ok(SearchLogsResponse {
            count: logs.len(),
            logs,
        })
    }

    pub async fn get_errors(&self, req: GetErrorsRequest) -> ServiceResult<GetErrorsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let group_by_app = match req.group_by.as_deref() {
            None => false,
            Some("app_name") | Some("app") => true,
            Some(other) => {
                return Err(ServiceError::InvalidInput(format!(
                    "Invalid group_by '{other}'. Supported: app_name"
                )));
            }
        };
        let rows = self
            .run_db(move |pool| {
                db::get_error_summary(pool, from.as_deref(), to.as_deref(), group_by_app)
            })
            .await?;
        Ok(GetErrorsResponse {
            summary: rows.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn list_hosts(&self) -> ServiceResult<ListHostsResponse> {
        let rows = self.run_db(db::list_hosts).await?;
        Ok(ListHostsResponse {
            hosts: rows.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn list_sessions(
        &self,
        req: ListSessionsRequest,
    ) -> ServiceResult<ListSessionsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let params = db::ListAiSessionsParams {
            ai_project: req.project,
            ai_tool: req.tool,
            hostname: req.hostname,
            from,
            to,
            limit: req.limit,
        };
        let rows = self
            .run_db(move |pool| db::list_ai_sessions(pool, &params))
            .await?;
        let sessions: Vec<AiSessionEntry> = rows.into_iter().map(Into::into).collect();
        Ok(ListSessionsResponse {
            count: sessions.len(),
            sessions,
        })
    }

    pub async fn search_sessions(
        &self,
        req: SearchSessionsRequest,
    ) -> ServiceResult<SearchSessionsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let params = db::SearchAiSessionsParams {
            query: req.query,
            ai_project: req.project,
            ai_tool: req.tool,
            from,
            to,
            limit: req.limit,
        };
        let result = self
            .run_db(move |pool| db::search_ai_sessions(pool, &params))
            .await?;
        Ok(result.into())
    }

    pub async fn usage_blocks(
        &self,
        req: UsageBlocksRequest,
    ) -> ServiceResult<UsageBlocksResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let params = db::AiUsageBlocksParams {
            ai_project: req.project,
            ai_tool: req.tool,
            from,
            to,
        };
        let result = self
            .run_db(move |pool| db::get_ai_usage_blocks(pool, &params))
            .await?;
        Ok(result.into())
    }

    pub async fn project_context(
        &self,
        req: ProjectContextRequest,
    ) -> ServiceResult<ProjectContextResponse> {
        let params = db::AiProjectContextParams {
            project: req.project,
            ai_tool: req.tool,
            limit: req.limit,
        };
        let result = self
            .run_db(move |pool| db::get_ai_project_context(pool, &params))
            .await?;
        Ok(result.into())
    }

    pub async fn list_ai_tools(
        &self,
        req: ListAiToolsRequest,
    ) -> ServiceResult<ListAiToolsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let params = db::ListAiToolsParams {
            ai_project: req.project,
            from,
            to,
        };
        let result = self
            .run_db(move |pool| db::list_ai_tools(pool, &params))
            .await?;
        Ok(result.into())
    }

    pub async fn list_ai_projects(
        &self,
        req: ListAiProjectsRequest,
    ) -> ServiceResult<ListAiProjectsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let params = db::ListAiProjectsParams {
            ai_tool: req.tool,
            from,
            to,
        };
        let result = self
            .run_db(move |pool| db::list_ai_projects(pool, &params))
            .await?;
        Ok(result.into())
    }

    pub async fn correlate_events(
        &self,
        req: CorrelateEventsRequest,
    ) -> ServiceResult<CorrelateEventsResponse> {
        let window = req.window_minutes.unwrap_or(5).min(60);
        let severity_min = req.severity_min.unwrap_or_else(|| "warning".into());
        let severity_levels = severity_at_or_above(&severity_min)?;
        let ref_dt = parse_required_timestamp(&req.reference_time, "reference_time")?;
        let delta = TimeDelta::try_minutes(i64::from(window))
            .ok_or_else(|| ServiceError::InvalidInput("duration overflow".into()))?;
        let from = rfc3339_z(ref_dt - delta);
        let to = rfc3339_z(ref_dt + delta);
        let limit = req.limit.unwrap_or(500).min(999);
        let params = SearchParams {
            query: req.query,
            hostname: req.hostname,
            source_ip: req.source_ip,
            severity: None,
            severity_in: Some(severity_levels),
            app_name: None,
            facility: None,
            process_id: None,
            from: Some(from.clone()),
            to: Some(to.clone()),
            limit: Some(limit + 1),
            ai_tool: None,
            ai_project: None,
            ai_session_id: None,
        };
        let mut rows = self
            .run_db(move |pool| db::search_logs(pool, &params))
            .await?;
        let truncated = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let logs: Vec<LogEntry> = rows.into_iter().map(Into::into).collect();
        let hosts = group_by_host(logs);
        let total_events = hosts.iter().map(|h| h.event_count).sum();

        Ok(CorrelateEventsResponse {
            reference_time: req.reference_time,
            window_minutes: window,
            window_from: from,
            window_to: to,
            severity_min,
            total_events,
            truncated,
            hosts_count: hosts.len(),
            hosts,
        })
    }

    pub async fn get_stats(&self) -> ServiceResult<DbStats> {
        let storage = self.storage.clone();
        let stats = self
            .run_db(move |pool| db::get_stats(pool, &storage))
            .await?
            .into();
        Ok(stats)
    }

    pub async fn db_status(&self) -> ServiceResult<DbMaintenanceStatus> {
        let storage = self.storage.clone();
        self.run_db(move |pool| {
            let page_count = db::db_pragma_i64(pool, "page_count")?;
            let freelist_count = db::db_pragma_i64(pool, "freelist_count")?;
            let page_size = db::db_pragma_i64(pool, "page_size")?;
            let auto_vacuum = db::db_pragma_i64(pool, "auto_vacuum")?;
            let journal_mode = db::db_pragma_string(pool, "journal_mode")?;
            let logical_size_bytes =
                ((page_count - freelist_count).max(0) * page_size).max(0) as u64;
            let physical_size_bytes = db::physical_size_bytes(&storage.db_path)?;
            let wal_size_bytes = std::fs::metadata(wal_path(&storage.db_path))
                .ok()
                .map(|metadata| metadata.len());
            let shm_size_bytes = std::fs::metadata(shm_path(&storage.db_path))
                .ok()
                .map(|metadata| metadata.len());
            Ok(DbMaintenanceStatus {
                db_path: storage.db_path,
                page_count,
                freelist_count,
                page_size,
                logical_size_bytes,
                physical_size_bytes,
                wal_size_bytes,
                shm_size_bytes,
                auto_vacuum,
                journal_mode,
                integrity_ok: None,
                integrity_messages: Vec::new(),
            })
        })
        .await
    }

    pub async fn db_integrity(&self) -> ServiceResult<DbIntegrityResult> {
        self.run_db(move |pool| {
            let messages = db::db_integrity_check(pool)?;
            Ok(DbIntegrityResult {
                ok: messages.len() == 1 && messages.first().is_some_and(|value| value == "ok"),
                messages,
            })
        })
        .await
    }

    pub async fn db_checkpoint(&self, mode: String) -> ServiceResult<DbCheckpointResult> {
        self.run_db(move |pool| {
            let (busy, log_frames, checkpointed_frames) = db::db_wal_checkpoint(pool, &mode)?;
            Ok(DbCheckpointResult {
                mode,
                busy,
                log_frames,
                checkpointed_frames,
            })
        })
        .await
    }

    pub async fn db_vacuum(
        &self,
        full: bool,
        incremental_pages: u32,
    ) -> ServiceResult<DbVacuumResult> {
        let storage = self.storage.clone();
        self.run_db(move |pool| {
            let before_physical_size_bytes = db::physical_size_bytes(&storage.db_path)?;
            if full {
                db::db_full_vacuum(pool)?;
            } else {
                db::db_incremental_vacuum(pool, incremental_pages)?;
            }
            let after_physical_size_bytes = db::physical_size_bytes(&storage.db_path)?;
            Ok(DbVacuumResult {
                full,
                incremental_pages,
                before_physical_size_bytes,
                after_physical_size_bytes,
            })
        })
        .await
    }

    pub async fn db_backup(&self, output: Option<PathBuf>) -> ServiceResult<DbBackupResult> {
        let db_path = self.storage.db_path.clone();
        self.run_db(move |_pool| {
            let backup_path = backup_path_for(&db_path, output)?;
            if let Some(parent) = backup_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let escaped = backup_path.to_string_lossy().replace('\'', "''");
            let output = std::process::Command::new("sqlite3")
                .arg(&db_path)
                .arg(format!(".backup '{escaped}'"))
                .output()?;
            if !output.status.success() {
                anyhow::bail!(
                    "sqlite3 backup failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            let size_bytes = std::fs::metadata(&backup_path)?.len();
            Ok(DbBackupResult {
                db_path,
                backup_path,
                size_bytes,
            })
        })
        .await
    }

    pub async fn index_ai_roots(
        &self,
        path: Option<String>,
        force: bool,
        since: Option<String>,
    ) -> ServiceResult<scanner::IndexResult> {
        let storage = self.storage.clone();
        let since_mtime_nanos = since
            .as_deref()
            .map(|raw| parse_required_timestamp(raw, "since"))
            .transpose()?
            .map(|dt| {
                dt.timestamp_nanos_opt().ok_or_else(|| {
                    ServiceError::InvalidInput(
                        "since timestamp out of i64 nanoseconds range".to_string(),
                    )
                })
            })
            .transpose()?;
        self.run_db(move |pool| {
            scanner::index_roots_with_options(
                pool,
                scanner::IndexOptions {
                    root_override: path.map(std::path::PathBuf::from),
                    force,
                    since_mtime_nanos,
                },
                Some(&storage),
            )
        })
        .await
        .map_err(classify_scanner_error)
    }

    pub async fn add_ai_file(
        &self,
        file: String,
        force: bool,
    ) -> ServiceResult<scanner::IndexResult> {
        let storage = self.storage.clone();
        self.run_db(move |pool| {
            scanner::index_file_with_options(
                pool,
                std::path::Path::new(&file),
                "explicit_file",
                scanner::IndexFileOptions { force },
                Some(&storage),
            )
        })
        .await
        .map_err(classify_scanner_error)
    }

    pub async fn list_ai_checkpoints(
        &self,
        errors_only: bool,
        missing_only: bool,
        limit: Option<u32>,
    ) -> ServiceResult<Vec<scanner::CheckpointEntry>> {
        self.run_db(move |pool| {
            scanner::list_checkpoints(
                pool,
                &scanner::CheckpointListOptions {
                    errors_only,
                    missing_only,
                    limit,
                },
            )
        })
        .await
    }

    pub async fn list_ai_parse_errors(
        &self,
        limit: Option<u32>,
    ) -> ServiceResult<Vec<scanner::ParseErrorEntry>> {
        self.run_db(move |pool| {
            scanner::list_parse_errors(pool, &scanner::ParseErrorListOptions { limit })
        })
        .await
    }

    pub async fn prune_ai_checkpoints(
        &self,
        missing_only: bool,
        dry_run: bool,
        limit: Option<u32>,
    ) -> ServiceResult<scanner::PruneCheckpointsResult> {
        self.run_db(move |pool| {
            scanner::prune_checkpoints(
                pool,
                &scanner::PruneCheckpointsOptions {
                    missing_only,
                    dry_run,
                    limit,
                },
            )
        })
        .await
    }

    pub async fn ai_doctor(&self) -> ServiceResult<scanner::AiDoctorReport> {
        let db_path = self.storage.db_path.clone();
        self.run_db(move |pool| scanner::ai_doctor(pool, &db_path))
            .await
    }

    pub async fn list_apps(&self, req: ListAppsRequest) -> ServiceResult<ListAppsResponse> {
        let apps = self
            .run_db(move |pool| db::list_apps(pool, req.hostname.as_deref()))
            .await?;
        Ok(ListAppsResponse {
            apps: apps.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn list_source_ips(&self) -> ServiceResult<ListSourceIpsResponse> {
        let source_ips = self.run_db(db::list_source_ips).await?;
        Ok(ListSourceIpsResponse {
            source_ips: source_ips.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn timeline(&self, req: TimelineRequest) -> ServiceResult<TimelineResponse> {
        let bucket_str = req.bucket.unwrap_or_else(|| "hour".into());
        let bucket = Bucket::parse(&bucket_str).ok_or_else(|| {
            ServiceError::InvalidInput(format!(
                "Invalid bucket '{bucket_str}'. Expected: minute, hour, day"
            ))
        })?;
        let group_by = match req.group_by.as_deref() {
            None => TimelineGroupBy::None,
            Some(g) => TimelineGroupBy::parse(g).ok_or_else(|| {
                ServiceError::InvalidInput(format!(
                    "Invalid group_by '{g}'. Expected: hostname, severity, app_name"
                ))
            })?,
        };
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let severity_in = match req.severity_min.as_deref() {
            Some(min) => Some(severity_at_or_above(min)?),
            None => None,
        };
        let group_by_label = req.group_by.clone();
        let points = self
            .run_db(move |pool| {
                db::timeline(
                    pool,
                    bucket,
                    group_by,
                    from.as_deref(),
                    to.as_deref(),
                    req.hostname.as_deref(),
                    req.app_name.as_deref(),
                    severity_in.as_deref(),
                )
            })
            .await?;
        Ok(TimelineResponse {
            bucket: bucket_str,
            group_by: group_by_label,
            points: points.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn patterns(&self, req: PatternsRequest) -> ServiceResult<PatternsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let severity_in = match req.severity_min.as_deref() {
            Some(min) => Some(severity_at_or_above(min)?),
            None => None,
        };
        let scan_limit = req.scan_limit.unwrap_or(10_000);
        let top_n = req.top_n.unwrap_or(20).min(200);
        let (patterns, scanned, truncated) = self
            .run_db(move |pool| {
                db::patterns(
                    pool,
                    from.as_deref(),
                    to.as_deref(),
                    req.hostname.as_deref(),
                    req.app_name.as_deref(),
                    severity_in.as_deref(),
                    scan_limit,
                    top_n,
                )
            })
            .await?;
        Ok(PatternsResponse {
            patterns: patterns.into_iter().map(Into::into).collect(),
            scanned,
            truncated,
        })
    }

    pub async fn context(&self, req: ContextRequest) -> ServiceResult<ContextResponse> {
        let before = req.before.unwrap_or(10).min(500);
        let after = req.after.unwrap_or(10).min(500);

        // When the caller anchors via hostname+timestamp, parse and re-emit the
        // timestamp through the same path used elsewhere in the service so
        // `2026-01-01T00:00:00Z` and `2026-01-01T00:00:00+00:00` (and offset
        // forms like `+02:00`) compare correctly against stored RFC3339 values
        // — TEXT comparisons in SQLite would otherwise treat them as different.
        let synthetic_timestamp = if req.log_id.is_none() {
            match req.timestamp.as_deref() {
                Some(raw) => Some(rfc3339_z(parse_required_timestamp(raw, "timestamp")?)),
                None => None,
            }
        } else {
            None
        };

        let resolved = self
            .run_db(move |pool| -> anyhow::Result<_> {
                let (reference, hostname, timestamp, id): (LogEntry, String, String, Option<i64>) =
                    if let Some(id) = req.log_id {
                        let row = db::fetch_log_by_id(pool, id)?
                            .ok_or_else(|| anyhow::anyhow!("No log found for id {id}"))?;
                        let entry = LogEntry {
                            id: row.id,
                            timestamp: row.timestamp.clone(),
                            hostname: row.hostname.clone(),
                            facility: row.facility.clone(),
                            severity: row.severity.clone(),
                            app_name: row.app_name.clone(),
                            process_id: row.process_id.clone(),
                            message: row.message.clone(),
                            received_at: row.received_at.clone(),
                            source_ip: row.source_ip.clone(),
                            ai_tool: row.ai_tool.clone(),
                            ai_project: row.ai_project.clone(),
                            ai_session_id: row.ai_session_id.clone(),
                            ai_transcript_path: row.ai_transcript_path.clone(),
                        };
                        (entry, row.hostname, row.timestamp, Some(row.id))
                    } else {
                        let hostname = req.hostname.clone().ok_or_else(|| {
                            anyhow::anyhow!(
                                "Either `log_id` or both `hostname` + `timestamp` are required"
                            )
                        })?;
                        let timestamp = synthetic_timestamp.ok_or_else(|| {
                            anyhow::anyhow!(
                                "Either `log_id` or both `hostname` + `timestamp` are required"
                            )
                        })?;
                        let synthetic = LogEntry {
                            id: 0,
                            timestamp: timestamp.clone(),
                            hostname: hostname.clone(),
                            facility: None,
                            severity: String::new(),
                            app_name: None,
                            process_id: None,
                            message: "<reference timestamp>".into(),
                            received_at: timestamp.clone(),
                            source_ip: String::new(),
                            ai_tool: None,
                            ai_project: None,
                            ai_session_id: None,
                            ai_transcript_path: None,
                        };
                        (synthetic, hostname, timestamp, None)
                    };

                let (before_rows, after_rows) = db::context_around(
                    pool,
                    &ContextRef {
                        id,
                        hostname,
                        timestamp,
                    },
                    before,
                    after,
                )?;
                Ok((reference, before_rows, after_rows))
            })
            .await?;
        let (reference, before_rows, after_rows) = resolved;
        Ok(ContextResponse {
            reference,
            before: before_rows.into_iter().map(Into::into).collect(),
            after: after_rows.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn get_log(&self, req: GetLogRequest) -> ServiceResult<GetLogResponse> {
        let id = req.id;
        let row = self
            .run_db(move |pool| db::fetch_log_by_id(pool, id))
            .await?
            .ok_or_else(|| ServiceError::InvalidInput(format!("No log found for id {id}")))?;
        Ok(GetLogResponse { log: row.into() })
    }

    pub async fn ingest_rate(&self, req: IngestRateRequest) -> ServiceResult<IngestRateResponse> {
        let now_dt = Utc::now();
        let now = rfc3339_z(now_dt);
        let cut_1m = rfc3339_z(now_dt - chrono::Duration::seconds(60));
        let cut_5m = rfc3339_z(now_dt - chrono::Duration::seconds(300));
        let cut_15m = rfc3339_z(now_dt - chrono::Duration::seconds(900));
        let want_by_host = req.by_host.unwrap_or(false);

        let storage = self.storage.clone();
        let now_clone = now.clone();
        let cut_1m_q = cut_1m.clone();
        let cut_5m_q = cut_5m.clone();
        let cut_15m_q = cut_15m.clone();
        let result = self
            .run_db(move |pool| -> anyhow::Result<_> {
                let buckets = db::ingest_rate(pool, &now_clone, &cut_1m_q, &cut_5m_q, &cut_15m_q)?;
                let by_host = if want_by_host {
                    Some(db::ingest_rate_by_host(
                        pool, &now_clone, &cut_1m_q, &cut_5m_q, &cut_15m_q,
                    )?)
                } else {
                    None
                };
                let stats = db::get_stats(pool, &storage)?;
                Ok((buckets, by_host, stats.write_blocked))
            })
            .await?;
        let (buckets, by_host, write_blocked) = result;
        Ok(IngestRateResponse {
            now,
            buckets: buckets.into(),
            write_blocked,
            by_host: by_host.map(|rows| rows.into_iter().map(Into::into).collect()),
        })
    }

    pub async fn silent_hosts(
        &self,
        req: SilentHostsRequest,
    ) -> ServiceResult<SilentHostsResponse> {
        let silent_minutes = req.silent_minutes.unwrap_or(30).min(60 * 24 * 7);
        let now_dt = Utc::now();
        let now = rfc3339_z(now_dt);
        let cutoff_dt = now_dt - chrono::Duration::minutes(i64::from(silent_minutes));
        let cutoff = rfc3339_z(cutoff_dt);
        let now_unix = now_dt.timestamp();
        let cutoff_q = cutoff.clone();
        let hosts = self
            .run_db(move |pool| db::silent_hosts(pool, &cutoff_q, now_unix))
            .await?;
        Ok(SilentHostsResponse {
            silent_minutes,
            cutoff,
            now,
            hosts: hosts.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn clock_skew(&self, req: ClockSkewRequest) -> ServiceResult<ClockSkewResponse> {
        // Compared against `received_at`, which SQLite stores in `Z` form, so
        // emit the canonical Z-form regardless of input shape.
        let since_str = match req.since {
            Some(s) => rfc3339_z(parse_required_timestamp(&s, "since")?),
            None => rfc3339_z(Utc::now() - chrono::Duration::hours(24)),
        };
        let q = since_str.clone();
        let hosts = self.run_db(move |pool| db::clock_skew(pool, &q)).await?;
        Ok(ClockSkewResponse {
            since: since_str,
            hosts: hosts.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn anomalies(&self, req: AnomaliesRequest) -> ServiceResult<AnomaliesResponse> {
        let recent_minutes = req.recent_minutes.unwrap_or(15).clamp(1, 60 * 24);
        let baseline_minutes = req.baseline_minutes.unwrap_or(360).clamp(1, 60 * 24 * 7);
        let now_dt = chrono::Utc::now();
        let recent_to = rfc3339_z(now_dt);
        let recent_from_dt = now_dt - chrono::Duration::minutes(i64::from(recent_minutes));
        let recent_from = rfc3339_z(recent_from_dt);
        let baseline_to = recent_from.clone();
        let baseline_from =
            rfc3339_z(recent_from_dt - chrono::Duration::minutes(i64::from(baseline_minutes)));

        let rf = recent_from.clone();
        let rt = recent_to.clone();
        let bf = baseline_from.clone();
        let bt = baseline_to.clone();
        let hosts = self
            .run_db(move |pool| {
                db::anomalies(pool, &rf, &rt, &bf, &bt, recent_minutes, baseline_minutes)
            })
            .await?;
        Ok(AnomaliesResponse {
            recent_from,
            recent_to,
            baseline_from,
            baseline_to,
            recent_minutes,
            baseline_minutes,
            hosts: hosts.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn compare(&self, req: CompareRequest) -> ServiceResult<CompareResponse> {
        let a_from = rfc3339_z(parse_required_timestamp(&req.a_from, "a_from")?);
        let a_to = rfc3339_z(parse_required_timestamp(&req.a_to, "a_to")?);
        let b_from = rfc3339_z(parse_required_timestamp(&req.b_from, "b_from")?);
        let b_to = rfc3339_z(parse_required_timestamp(&req.b_to, "b_to")?);

        let a_from_q = a_from.clone();
        let a_to_q = a_to.clone();
        let b_from_q = b_from.clone();
        let b_to_q = b_to.clone();
        let result = self
            .run_db(move |pool| -> anyhow::Result<_> {
                let a = db::summarize_range(pool, &a_from_q, &a_to_q)?;
                let b = db::summarize_range(pool, &b_from_q, &b_to_q)?;
                Ok((a, b))
            })
            .await?;
        let (a, b) = result;
        let delta_total_logs = b.total_logs - a.total_logs;
        let delta_total_errors = b.total_errors - a.total_errors;
        Ok(CompareResponse {
            a: a.into(),
            b: b.into(),
            delta_total_logs,
            delta_total_errors,
        })
    }
}

fn classify_scanner_error(error: ServiceError) -> ServiceError {
    match error {
        ServiceError::Internal(err) if scanner_error_is_invalid_input(&err) => {
            ServiceError::InvalidInput(err.to_string())
        }
        other => other,
    }
}

fn scanner_error_is_invalid_input(error: &anyhow::Error) -> bool {
    scanner::is_invalid_input_error(error)
}

fn wal_path(db_path: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}-wal", db_path.display()))
}

fn shm_path(db_path: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}-shm", db_path.display()))
}

fn backup_path_for(db_path: &std::path::Path, output: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let timestamp = Utc::now().format("%Y-%m-%d-%H%M%S");
    match output {
        Some(path) if path.extension().is_some() => Ok(path),
        Some(dir) => Ok(dir.join(format!("syslog-{timestamp}.db"))),
        None => Ok(db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("backups")
            .join(format!("syslog-{timestamp}.db"))),
    }
}

fn validate_optional_severity(severity: Option<String>) -> ServiceResult<Option<String>> {
    let Some(severity) = severity else {
        return Ok(None);
    };
    if db::severity_to_num(&severity).is_some() {
        return Ok(Some(severity));
    }
    Err(ServiceError::InvalidInput(format!(
        "Invalid severity '{}'. Must be one of: {}",
        severity,
        db::SEVERITY_LEVELS.join(", ")
    )))
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
