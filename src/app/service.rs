use std::sync::Arc;
use std::time::Duration;

use chrono::TimeDelta;
use tokio::sync::Semaphore;

use super::correlate::{group_by_host, severity_at_or_above};
use super::models::{
    AnomaliesRequest, AnomaliesResponse, ClockSkewRequest, ClockSkewResponse, CompareRequest,
    CompareResponse, ContextRequest, ContextResponse, CorrelateEventsRequest,
    CorrelateEventsResponse, DbStats, GetErrorsRequest, GetErrorsResponse, GetLogRequest,
    GetLogResponse, IngestRateRequest, IngestRateResponse, ListAppsRequest, ListAppsResponse,
    ListHostsResponse, ListSourceIpsResponse, LogEntry, PatternsRequest, PatternsResponse,
    SearchLogsRequest, SearchLogsResponse, SilentHostsRequest, SilentHostsResponse,
    TailLogsRequest, TimelineRequest, TimelineResponse,
};
use super::time::{parse_optional_timestamp, parse_required_timestamp};
use super::{ServiceError, ServiceResult};
use crate::config::StorageConfig;
use crate::db::{self, Bucket, ContextRef, DbPool, SearchParams, TimelineGroupBy};

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
                )))
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
        let from = (ref_dt - delta).to_rfc3339();
        let to = (ref_dt + delta).to_rfc3339();
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

    pub async fn list_apps(&self, req: ListAppsRequest) -> ServiceResult<ListAppsResponse> {
        let apps = self
            .run_db(move |pool| db::list_apps(pool, req.hostname.as_deref()))
            .await?;
        Ok(ListAppsResponse { apps })
    }

    pub async fn list_source_ips(&self) -> ServiceResult<ListSourceIpsResponse> {
        let source_ips = self.run_db(db::list_source_ips).await?;
        Ok(ListSourceIpsResponse { source_ips })
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
            points,
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
            patterns,
            scanned,
            truncated,
        })
    }

    pub async fn context(&self, req: ContextRequest) -> ServiceResult<ContextResponse> {
        let before = req.before.unwrap_or(10).min(500);
        let after = req.after.unwrap_or(10).min(500);
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
                        };
                        (entry, row.hostname, row.timestamp, Some(row.id))
                    } else {
                        let hostname = req.hostname.clone().ok_or_else(|| {
                            anyhow::anyhow!(
                                "Either `log_id` or both `hostname` + `timestamp` are required"
                            )
                        })?;
                        let timestamp = req.timestamp.clone().ok_or_else(|| {
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
        Ok(GetLogResponse { log: row })
    }

    pub async fn ingest_rate(&self, req: IngestRateRequest) -> ServiceResult<IngestRateResponse> {
        let now_dt = chrono::Utc::now();
        let now = now_dt.to_rfc3339();
        let cut_1m = (now_dt - chrono::Duration::seconds(60)).to_rfc3339();
        let cut_5m = (now_dt - chrono::Duration::seconds(300)).to_rfc3339();
        let cut_15m = (now_dt - chrono::Duration::seconds(900)).to_rfc3339();
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
            buckets,
            write_blocked,
            by_host,
        })
    }

    pub async fn silent_hosts(
        &self,
        req: SilentHostsRequest,
    ) -> ServiceResult<SilentHostsResponse> {
        let silent_minutes = req.silent_minutes.unwrap_or(30).min(60 * 24 * 7);
        let now_dt = chrono::Utc::now();
        let now = now_dt.to_rfc3339();
        let cutoff_dt = now_dt - chrono::Duration::minutes(i64::from(silent_minutes));
        let cutoff = cutoff_dt.to_rfc3339();
        let now_unix = now_dt.timestamp();
        let cutoff_q = cutoff.clone();
        let hosts = self
            .run_db(move |pool| db::silent_hosts(pool, &cutoff_q, now_unix))
            .await?;
        Ok(SilentHostsResponse {
            silent_minutes,
            cutoff,
            now,
            hosts,
        })
    }

    pub async fn clock_skew(&self, req: ClockSkewRequest) -> ServiceResult<ClockSkewResponse> {
        let since_str = match req.since {
            Some(s) => parse_optional_timestamp(Some(&s), "since")?
                .expect("parse_optional_timestamp returns Some when input is Some"),
            None => (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339(),
        };
        let q = since_str.clone();
        let hosts = self.run_db(move |pool| db::clock_skew(pool, &q)).await?;
        Ok(ClockSkewResponse {
            since: since_str,
            hosts,
        })
    }

    pub async fn anomalies(&self, req: AnomaliesRequest) -> ServiceResult<AnomaliesResponse> {
        let recent_minutes = req.recent_minutes.unwrap_or(15).clamp(1, 60 * 24);
        let baseline_minutes = req.baseline_minutes.unwrap_or(360).clamp(1, 60 * 24 * 7);
        let now_dt = chrono::Utc::now();
        let recent_to = now_dt.to_rfc3339();
        let recent_from_dt = now_dt - chrono::Duration::minutes(i64::from(recent_minutes));
        let recent_from = recent_from_dt.to_rfc3339();
        let baseline_to = recent_from.clone();
        let baseline_from =
            (recent_from_dt - chrono::Duration::minutes(i64::from(baseline_minutes))).to_rfc3339();

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
            hosts,
        })
    }

    pub async fn compare(&self, req: CompareRequest) -> ServiceResult<CompareResponse> {
        let a_from =
            parse_optional_timestamp(Some(&req.a_from), "a_from")?.expect("required field");
        let a_to = parse_optional_timestamp(Some(&req.a_to), "a_to")?.expect("required field");
        let b_from =
            parse_optional_timestamp(Some(&req.b_from), "b_from")?.expect("required field");
        let b_to = parse_optional_timestamp(Some(&req.b_to), "b_to")?.expect("required field");

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
            a,
            b,
            delta_total_logs,
            delta_total_errors,
        })
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
