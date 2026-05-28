use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use chrono::{TimeDelta, Utc};
use tokio::sync::Semaphore;

use super::correlate::{group_by_host, severity_at_or_above};
use super::models::{
    AbuseSearchRequest, AbuseSearchResponse, AiAssessEvidenceSummary, AiAssessRequest,
    AiAssessResponse, AiCorrelateLimitPolicy, AiCorrelateRequest, AiCorrelateResponse,
    AiCorrelationAnchor, AiIncidentRequest, AiIncidentResponse, AiInvestigateRequest,
    AiInvestigateResponse, AiLimitPolicy, AiSessionEntry, AnomaliesRequest, AnomaliesResponse,
    AskHistoryRequest, AskHistoryResponse, ClockSkewRequest, ClockSkewResponse, CompareRequest,
    CompareResponse, ContextRequest, ContextResponse, CorrelateEventsRequest,
    CorrelateEventsResponse, CorrelateStateHostEntry, CorrelateStateRequest,
    CorrelateStateResponse, CorrelateStateWindow, DbBackupResult, DbCheckpointRequest,
    DbCheckpointResult, DbIntegrityResult, DbMaintenanceStatus, DbStats, DbVacuumRequest,
    DbVacuumResult, FilterLogsRequest, FleetStateHostRow, FleetStateRequest, FleetStateResponse,
    FleetStateSummary, GetErrorsRequest, GetErrorsResponse, GetLogRequest, GetLogResponse,
    IncidentContextRequest, IncidentContextResponse, IncidentEvent, IncidentRequest,
    IncidentResponse, IngestRateRequest, IngestRateResponse, ListAiProjectsRequest,
    ListAiProjectsResponse, ListAiToolsRequest, ListAiToolsResponse, ListAppsRequest,
    ListAppsResponse, ListHostsResponse, ListSessionsRequest, ListSessionsResponse,
    ListSourceIpsRequest, ListSourceIpsResponse, LogEntry, NotificationsRecentRequest,
    PatternsRequest, PatternsResponse, ProjectContextRequest, ProjectContextResponse, RequestActor,
    SearchLogsRequest, SearchLogsResponse, SearchSessionsRequest, SearchSessionsResponse,
    ServiceJournalEntry, ServiceLogsRequest, ServiceLogsResponse, SilentHostsRequest,
    SilentHostsResponse, SimilarIncidentsRequest, SimilarIncidentsResponse, TailLogsRequest,
    TimelineRequest, TimelineResponse, UsageBlocksRequest, UsageBlocksResponse,
};
use super::os_adapter::{OsAdapter, SystemOsAdapter};
use super::time::{parse_optional_timestamp, parse_required_timestamp, rfc3339_z};
use super::{ServiceError, ServiceResult};
use crate::assessment::{build_assessment_prompt, run_gemini_assessment, GeminiAssessConfig};
use crate::command_log::{self, CommandLogImportResult};
use crate::config::StorageConfig;
use crate::db::{self, Bucket, ContextRef, DbPool, SearchParams, TimelineGroupBy};
use crate::scanner;

const DB_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);
const SLOW_DB_MS: u128 = 500;
const SYSLOG_OWNED_USER_SERVICES: &[&str] = &[
    "syslog-ai-watch.service",
    "syslog-ai-index.service",
    "syslog-mcp.service",
];

fn normalize_syslog_owned_service(service: &str) -> ServiceResult<String> {
    let unit = if service.ends_with(".service") {
        service.to_string()
    } else {
        format!("{service}.service")
    };
    if SYSLOG_OWNED_USER_SERVICES.contains(&unit.as_str()) {
        Ok(unit)
    } else {
        Err(ServiceError::InvalidInput(format!(
            "unsupported syslog-owned service '{service}'; expected one of {}",
            SYSLOG_OWNED_USER_SERVICES.join(", ")
        )))
    }
}

// `command_output`, `inferred_user_bus_env`, and `current_uid` were extracted
// to `os_adapter.rs` as part of Arch-C2. OS-level shell-outs now go through
// the `OsAdapter` trait so they can be injected in tests.

/// Parse journalctl `-o json` output into entries, tolerating malformed lines.
///
/// Returns `(entries, dropped)` so callers can surface a warning when the
/// journal contains corrupt rows — `service logs` is a self-debugging surface
/// and must not nuke a 5000-line response because one line failed to parse.
fn parse_journal_json_lines(raw: &str) -> (Vec<ServiceJournalEntry>, usize) {
    let mut entries = Vec::new();
    let mut dropped: usize = 0;
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        match parse_journal_json_line(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => dropped = dropped.saturating_add(1),
        }
    }
    (entries, dropped)
}

fn parse_journal_json_line(line: &str) -> ServiceResult<ServiceJournalEntry> {
    let value: serde_json::Value = serde_json::from_str(line).map_err(anyhow::Error::from)?;
    Ok(ServiceJournalEntry {
        timestamp: journal_string(&value, "__REALTIME_TIMESTAMP")
            .and_then(|micros| journal_realtime_timestamp(&micros)),
        realtime_timestamp_us: journal_string(&value, "__REALTIME_TIMESTAMP"),
        unit: journal_string(&value, "_SYSTEMD_USER_UNIT")
            .or_else(|| journal_string(&value, "_SYSTEMD_UNIT")),
        priority: journal_string(&value, "PRIORITY"),
        syslog_identifier: journal_string(&value, "SYSLOG_IDENTIFIER"),
        pid: journal_string(&value, "_PID"),
        message: journal_string(&value, "MESSAGE"),
        cursor: journal_string(&value, "__CURSOR"),
    })
}

fn journal_string(value: &serde_json::Value, key: &str) -> Option<String> {
    match value.get(key)? {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Array(values) => values.iter().find_map(|value| match value {
            serde_json::Value::String(value) => Some(value.clone()),
            _ => None,
        }),
        other => Some(other.to_string()),
    }
}

fn journal_realtime_timestamp(micros: &str) -> Option<String> {
    let micros = micros.parse::<i64>().ok()?;
    let secs = micros.div_euclid(1_000_000);
    let nanos = micros.rem_euclid(1_000_000) as u32 * 1_000;
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos).map(super::time::rfc3339_z)
}

fn service_app_filter(service: &str) -> String {
    service
        .strip_suffix(".service")
        .unwrap_or(service)
        .to_string()
}

fn incident_event_from_log(log: db::LogEntry) -> IncidentEvent {
    let source = if log.ai_tool.is_some()
        || log.facility.as_deref() == Some("transcript")
        || log.source_ip.starts_with("transcript://")
    {
        "transcript"
    } else if log.source_ip.starts_with("docker://") || log.source_ip.starts_with("docker-event://")
    {
        "docker"
    } else {
        "syslog"
    };
    IncidentEvent {
        timestamp: log.timestamp,
        source: source.to_string(),
        host: Some(log.hostname),
        severity: Some(log.severity),
        app: log.app_name,
        message: log.message,
        log_id: Some(log.id),
    }
}

fn incident_event_from_journal(entry: ServiceJournalEntry) -> Option<IncidentEvent> {
    Some(IncidentEvent {
        timestamp: entry.timestamp?,
        source: "service-log".to_string(),
        host: entry.unit,
        severity: entry.priority,
        app: entry.syslog_identifier,
        message: entry.message.unwrap_or_default(),
        log_id: None,
    })
}

fn incident_sort_key(timestamp: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.timestamp_micros())
        .unwrap_or(i64::MAX)
}

/// Read a syslog-owned service's journal via `journalctl`. Free function so
/// callers can invoke it without standing up a [`SyslogService`] (and the
/// SQLite pool that backs it) — `syslog service logs` is a self-debugging
/// surface that must work when the DB is corrupted, locked, or full.
///
/// The `os` parameter is the `OsAdapter` to use for the journalctl shell-out.
/// Pass `&SystemOsAdapter` for production; inject a mock for tests.
pub async fn run_service_logs(
    req: ServiceLogsRequest,
    os: &(dyn super::os_adapter::OsAdapter + Send + Sync),
) -> ServiceResult<ServiceLogsResponse> {
    let service = normalize_syslog_owned_service(&req.service)?;
    let mut args = vec![
        "--user".to_string(),
        "-u".to_string(),
        service.clone(),
        "--no-pager".to_string(),
        "--output".to_string(),
        "json".to_string(),
    ];
    if let Some(from) = &req.from {
        // Validate as RFC 3339 before passing to journalctl to prevent
        // argument injection (e.g. "--rotate", "--vacuum-size=1").
        chrono::DateTime::parse_from_rfc3339(from)
            .map_err(|_| ServiceError::InvalidInput(format!("invalid `from` timestamp: {from}")))?;
        args.push("--since".to_string());
        args.push(from.clone());
    }
    if let Some(to) = &req.to {
        chrono::DateTime::parse_from_rfc3339(to)
            .map_err(|_| ServiceError::InvalidInput(format!("invalid `to` timestamp: {to}")))?;
        args.push("--until".to_string());
        args.push(to.clone());
    }
    let tail = req.tail.map(|tail| tail.clamp(1, 5_000));
    if let Some(tail) = tail {
        args.push("-n".to_string());
        args.push(tail.to_string());
    }

    let raw = os.run_command("journalctl", &args).await?;
    let (entries, dropped_lines) = parse_journal_json_lines(&raw);
    if dropped_lines > 0 {
        tracing::warn!(
            service = %service,
            dropped_lines,
            "service_logs: skipped malformed journal lines"
        );
    }
    Ok(ServiceLogsResponse {
        service,
        from: req.from,
        to: req.to,
        tail,
        entries,
        dropped_lines,
    })
}

pub async fn run_compose_status() -> ServiceResult<crate::compose::ComposeStatus> {
    static COMPOSE_DIAGNOSTICS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let permit = COMPOSE_DIAGNOSTICS
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| ServiceError::Busy(format!("compose diagnostics limiter closed: {e}")))?;
    let service = crate::compose::ComposeService::new(
        crate::compose::CliDockerInspect,
        crate::compose::ProcessRunner,
        crate::compose::ComposeDefaults::default(),
    );
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        service.status(&crate::compose::ComposeTarget::default())
    })
    .await
    .map_err(|e| anyhow::anyhow!("compose status task failed: {e}"))?
    .map_err(ServiceError::from)
}

/// Service-layer entry point bridging request structs to SQLite.
///
/// `Clone` is cheap — every field is either `Arc`-wrapped or a small `Copy`
/// scalar — and the type is intentionally Clone-friendly so callers like
/// `ai_watch::run` can take ownership without forcing a borrow through async
/// task boundaries (bead 0p8r.24). The other 5 LOCAL-only command dispatchers
/// take `&SyslogService` because they don't move the service into a spawned
/// task; both patterns are correct.
/// Facade for all syslog MCP service operations.
///
/// # Architecture note (Arch-C2 — partial)
///
/// `SyslogService` is a 1,983 LOC god class. The immediate win delivered here
/// is extracting OS-level shell-outs (`journalctl`, `sqlite3`) behind the
/// `OsAdapter` trait so they are testable and injectable. The full split into
/// `LogQueryService`, `AiAnalyticsService`, `MaintenanceService`, and
/// `AbuseService` sub-structs is deferred to a follow-up bead — it requires
/// touching every call site in `tools.rs`, `api.rs`, and `routes.rs`.
///
/// The `os` field carries an `Arc<dyn OsAdapter>`. Production code sets it to
/// `SystemOsAdapter`; tests can swap in a `MockOsAdapter` that returns canned
/// output without spawning real processes.
#[derive(Clone)]
pub struct SyslogService {
    pool: Arc<DbPool>,
    pub(super) storage: StorageConfig,
    db_permits: Arc<Semaphore>,
    acquire_timeout: Duration,
    /// OS-level adapter for journalctl / systemd shell-outs.
    pub(super) os: Arc<dyn OsAdapter + Send + Sync>,
}

impl SyslogService {
    pub(crate) fn new(pool: Arc<DbPool>, storage: StorageConfig) -> Self {
        let permits = storage.pool_size.max(1) as usize;
        Self {
            pool,
            storage,
            db_permits: Arc::new(Semaphore::new(permits)),
            acquire_timeout: DB_ACQUIRE_TIMEOUT,
            os: Arc::new(SystemOsAdapter),
        }
    }

    /// Test constructor that injects a custom `OsAdapter`.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn with_os_adapter(
        pool: Arc<DbPool>,
        storage: StorageConfig,
        os: Arc<dyn OsAdapter + Send + Sync>,
    ) -> Self {
        let permits = storage.pool_size.max(1) as usize;
        Self {
            pool,
            storage,
            db_permits: Arc::new(Semaphore::new(permits)),
            acquire_timeout: DB_ACQUIRE_TIMEOUT,
            os,
        }
    }

    pub async fn service_logs(
        &self,
        req: ServiceLogsRequest,
    ) -> ServiceResult<ServiceLogsResponse> {
        run_service_logs(req, self.os.as_ref()).await
    }

    pub async fn import_shell_history(
        &self,
        path: PathBuf,
        shell: String,
    ) -> ServiceResult<CommandLogImportResult> {
        self.run_db("import_shell_history", move |pool| {
            command_log::import_zsh_history(pool, &path, &shell)
        })
        .await
    }

    pub async fn import_atuin_history(
        &self,
        path: PathBuf,
    ) -> ServiceResult<CommandLogImportResult> {
        self.run_db("import_atuin_history", move |pool| {
            command_log::import_atuin_history(pool, &path)
        })
        .await
    }

    pub async fn import_agent_command_spool(
        &self,
        path: PathBuf,
    ) -> ServiceResult<CommandLogImportResult> {
        self.run_db("import_agent_command_spool", move |pool| {
            command_log::import_agent_command_spool(pool, &path)
        })
        .await
    }

    pub async fn incident(&self, req: IncidentRequest) -> ServiceResult<IncidentResponse> {
        if req.hostname.is_some() && req.service.is_some() {
            return Err(ServiceError::InvalidInput(
                "hostname and service cannot be combined: journal entries are always local \
                 and cannot be filtered by remote hostname"
                    .into(),
            ));
        }
        let around_dt = parse_required_timestamp(&req.around, "around")?;
        let window = req.minutes.unwrap_or(5).clamp(1, 120);
        let delta = TimeDelta::try_minutes(i64::from(window))
            .ok_or_else(|| ServiceError::InvalidInput("duration overflow".into()))?;
        let from = rfc3339_z(around_dt - delta);
        let to = rfc3339_z(around_dt + delta);
        let limit = req.limit.unwrap_or(500).clamp(1, 2_000);
        let app_name = req.service.as_deref().map(service_app_filter);
        let params = SearchParams {
            query: None,
            hostname: req.hostname.clone(),
            source_ip: None,
            source_ip_prefix: None,
            severity: None,
            severity_in: None,
            app_name,
            facility: None,
            exclude_facility: None,
            process_id: None,
            from: Some(from.clone()),
            to: Some(to.clone()),
            received_from: None,
            received_to: None,
            limit: Some(limit + 1),
            ai_tool: None,
            ai_project: None,
            ai_session_id: None,
            event_action: None,
            exclude_ai: false,
        };
        let mut rows = self
            .run_db("incident.search", move |pool| {
                db::search_logs(pool, &params)
            })
            .await?;
        let mut truncated = rows.len() > limit as usize;
        rows.truncate(limit as usize);

        let mut events: Vec<IncidentEvent> =
            rows.into_iter().map(incident_event_from_log).collect();
        let mut warnings = Vec::new();

        if let Some(service) = req.service {
            match self
                .service_logs(ServiceLogsRequest {
                    service,
                    from: Some(from.clone()),
                    to: Some(to.clone()),
                    tail: Some(limit.saturating_add(1)),
                })
                .await
            {
                Ok(report) => {
                    if report.dropped_lines > 0 {
                        warnings.push(format!(
                            "service_logs: dropped {} malformed journal line(s)",
                            report.dropped_lines
                        ));
                    }
                    let mut dropped_timestamps: usize = 0;
                    for entry in report.entries {
                        match incident_event_from_journal(entry) {
                            Some(event) => events.push(event),
                            None => dropped_timestamps = dropped_timestamps.saturating_add(1),
                        }
                    }
                    if dropped_timestamps > 0 {
                        warnings.push(format!(
                            "service_logs: dropped {dropped_timestamps} entries with unparseable timestamps"
                        ));
                    }
                }
                Err(error) => warnings.push(format!("service_logs: {error}")),
            }
        }

        events.sort_by_key(|event| incident_sort_key(&event.timestamp));
        if events.len() > limit as usize {
            truncated = true;
            events.truncate(limit as usize);
        }

        Ok(IncidentResponse {
            around: req.around,
            window_minutes: window,
            window_from: from,
            window_to: to,
            event_count: events.len(),
            truncated,
            warnings,
            events,
        })
    }

    async fn run_db<F, T>(&self, op: &'static str, f: F) -> ServiceResult<T>
    where
        F: FnOnce(&DbPool) -> anyhow::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let wait_start = Instant::now();
        let permit_result = tokio::time::timeout(
            self.acquire_timeout,
            Arc::clone(&self.db_permits).acquire_owned(),
        )
        .await;
        let permit_ms = wait_start.elapsed().as_millis();

        let permit = match permit_result {
            Err(_) => {
                tracing::warn!(op, permit_ms, "db acquire timeout");
                return Err(ServiceError::Busy("database worker limit reached".into()));
            }
            Ok(Err(_)) => {
                tracing::warn!(op, permit_ms, "db semaphore closed");
                return Err(ServiceError::Busy("database worker limit closed".into()));
            }
            Ok(Ok(p)) => p,
        };

        let exec_start = Instant::now();
        let pool = Arc::clone(&self.pool);
        let join_result = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f(&pool)
        })
        .await;
        let exec_ms = exec_start.elapsed().as_millis();

        let result = match join_result {
            Err(e) => {
                if e.is_cancelled() {
                    tracing::warn!(op, permit_ms, exec_ms, "db task cancelled");
                } else {
                    tracing::warn!(op, permit_ms, exec_ms, error = %e, "db task panic");
                }
                return Err(ServiceError::Internal(anyhow::anyhow!(
                    "Task join error: {e}"
                )));
            }
            Ok(r) => r.map_err(ServiceError::Internal),
        };

        if exec_ms > SLOW_DB_MS {
            match &result {
                Ok(_) => tracing::warn!(op, permit_ms, exec_ms, "db op ok"),
                Err(e) => tracing::warn!(op, permit_ms, exec_ms, error = %e, "db op err"),
            }
        } else {
            match &result {
                Ok(_) => tracing::debug!(op, permit_ms, exec_ms, "db op ok"),
                Err(e) => tracing::debug!(op, permit_ms, exec_ms, error = %e, "db op err"),
            }
        }
        result
    }

    pub async fn health_check(&self) -> ServiceResult<()> {
        self.run_db("health_check", |pool| {
            let conn = pool.get()?;
            conn.query_row("SELECT 1", [], |_| Ok(()))?;
            Ok(())
        })
        .await
    }

    pub async fn search_logs(&self, req: SearchLogsRequest) -> ServiceResult<SearchLogsResponse> {
        let query = req.query.clone();
        let params = search_request_to_params(req)?;
        let params = SearchParams { query, ..params };
        let logs = self
            .run_db("search_logs", move |pool| db::search_logs(pool, &params))
            .await?;
        let logs: Vec<LogEntry> = logs.into_iter().map(Into::into).collect();
        Ok(SearchLogsResponse {
            count: logs.len(),
            logs,
        })
    }

    pub async fn host_state(
        &self,
        req: super::models::HostStateRequest,
    ) -> ServiceResult<super::models::HostStateResponse> {
        let lookup = match (req.host_id, req.hostname) {
            (Some(host_id), _) if !host_id.trim().is_empty() => {
                db::HeartbeatHostLookup::HostId(host_id)
            }
            (_, Some(hostname)) if !hostname.trim().is_empty() => {
                db::HeartbeatHostLookup::Hostname(hostname)
            }
            _ => {
                return Err(ServiceError::InvalidInput(
                    "host_state requires host_id or hostname".into(),
                ));
            }
        };
        let limit = req.limit.unwrap_or(1).clamp(1, 100) as usize;
        let since = parse_optional_timestamp(req.since.as_deref(), "since")?;
        self.run_db("host_state", move |pool| {
            db::heartbeat_host_state(pool, lookup, since.as_deref(), limit).map_err(|error| {
                match error.to_string().as_str() {
                    "not_found" => anyhow::anyhow!("not_found"),
                    "ambiguous_host" => anyhow::anyhow!("ambiguous_host"),
                    _ => error,
                }
            })
        })
        .await
        .map_err(|error| match error {
            ServiceError::Internal(error) if error.to_string() == "not_found" => {
                ServiceError::NotFound("host_state host not found".into())
            }
            ServiceError::Internal(error) if error.to_string() == "ambiguous_host" => {
                ServiceError::InvalidInput("ambiguous_host".into())
            }
            other => other,
        })
    }

    pub async fn fleet_state(&self, req: FleetStateRequest) -> ServiceResult<FleetStateResponse> {
        let include_ok = req.include_ok.unwrap_or(true);
        let sort = req.sort.clone().unwrap_or_else(|| "pressure".into());

        let entries = self
            .run_db("fleet_state.latest", db::heartbeat_latest_all)
            .await?;

        let mut rows: Vec<FleetStateHostRow> = Vec::with_capacity(entries.len());
        for entry in &entries {
            let hb_id = entry.heartbeat_id;
            let metrics = self
                .run_db("fleet_state.metrics", move |pool| {
                    db::heartbeat_metric_snapshot(pool, hb_id)
                })
                .await?;
            let flags = super::heartbeat_flags::from_latest_and_metrics(entry, &metrics);
            let pressure = super::heartbeat_flags::pressure_names(&flags);
            let status = super::heartbeat_flags::host_status_label(&flags);
            if !include_ok && status == "ok" {
                continue;
            }
            rows.push(FleetStateHostRow {
                host_id: entry.host_id.clone(),
                hostname: entry.hostname.clone(),
                last_heartbeat_at: entry.sampled_at.clone(),
                status: status.to_owned(),
                pressure,
                partial: flags.collector_partial,
                clock_skew: flags.clock_skew,
            });
        }

        // Sort
        match sort.as_str() {
            "freshness" => rows.sort_by(|a, b| b.last_heartbeat_at.cmp(&a.last_heartbeat_at)),
            "hostname" => rows.sort_by(|a, b| a.hostname.cmp(&b.hostname)),
            _ => {
                // pressure sort: late > partial > pressure > ok, then hostname
                let rank = |status: &str| match status {
                    "late" => 0,
                    "partial" => 1,
                    "pressure" => 2,
                    _ => 3,
                };
                rows.sort_by(|a, b| {
                    rank(a.status.as_str())
                        .cmp(&rank(b.status.as_str()))
                        .then_with(|| a.hostname.cmp(&b.hostname))
                });
            }
        }

        let total = rows.len();
        let mut summary = FleetStateSummary {
            total,
            ..Default::default()
        };
        for row in &rows {
            match row.status.as_str() {
                "ok" => summary.ok += 1,
                "late" => summary.late += 1,
                "partial" => summary.partial += 1,
                "pressure" => summary.pressure += 1,
                _ => {}
            }
        }

        Ok(FleetStateResponse {
            hosts: rows,
            summary,
        })
    }

    pub async fn correlate_state(
        &self,
        req: CorrelateStateRequest,
    ) -> ServiceResult<CorrelateStateResponse> {
        let reference_time = req.reference_time.trim().to_owned();
        if reference_time.is_empty() {
            return Err(ServiceError::InvalidInput(
                "correlate_state requires reference_time".into(),
            ));
        }
        let ref_dt = parse_required_timestamp(&reference_time, "reference_time")
            .map_err(|_| ServiceError::InvalidInput("invalid reference_time format".into()))?;

        let window_minutes = req.window_minutes.unwrap_or(10).clamp(1, 120) as i64;
        let limit = req.limit.unwrap_or(100).clamp(1, 500) as usize;
        let severity_min = req.severity_min.clone().unwrap_or_else(|| "info".into());

        let from_dt = ref_dt - TimeDelta::minutes(window_minutes);
        let to_dt = ref_dt + TimeDelta::minutes(window_minutes);
        let from = rfc3339_z(from_dt);
        let to = rfc3339_z(to_dt);

        // Resolve optional host filter
        let host_id: Option<String> = if let Some(host) = req.host.as_deref() {
            let h = host.to_owned();
            let resolved = self
                .run_db("correlate_state.resolve_host", move |pool| {
                    let conn = pool.get()?;
                    // Try host_id first
                    let exists: bool = conn
                        .query_row(
                            "SELECT COUNT(*) FROM host_heartbeats_latest WHERE host_id = ?1",
                            [&h],
                            |row| row.get::<_, i64>(0),
                        )
                        .map(|c| c > 0)?;
                    if exists {
                        return Ok(Some(h));
                    }
                    // Hostname fallback (unique only)
                    let mut stmt = conn.prepare(
                        "SELECT host_id FROM host_heartbeats_latest
                         WHERE hostname = ?1",
                    )?;
                    let ids: Vec<String> = stmt
                        .query_map([&h], |row| row.get(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    match ids.as_slice() {
                        [] => Err(anyhow::anyhow!("not_found")),
                        [id] => Ok(Some(id.clone())),
                        _ => Err(anyhow::anyhow!("ambiguous_host")),
                    }
                })
                .await
                .map_err(|e| match e {
                    ServiceError::Internal(ref inner) if inner.to_string() == "not_found" => {
                        ServiceError::NotFound("correlate_state host not found".into())
                    }
                    ServiceError::Internal(ref inner) if inner.to_string() == "ambiguous_host" => {
                        ServiceError::InvalidInput("ambiguous_host".into())
                    }
                    other => other,
                })?;
            resolved
        } else {
            None
        };

        let from2 = from.clone();
        let to2 = to.clone();
        let hid = host_id.clone();
        let heartbeat_summaries = self
            .run_db("correlate_state.heartbeats", move |pool| {
                db::heartbeat_window_summaries(pool, &from2, &to2, hid.as_deref())
            })
            .await?;

        // Fetch logs for each host in the window
        let mut host_entries: Vec<CorrelateStateHostEntry> = Vec::new();
        let mut global_truncated = false;
        for summary in heartbeat_summaries {
            let host_id_filter = summary.host_id.clone();
            let hostname_filter = summary.hostname.clone();
            let from3 = from.clone();
            let to3 = to.clone();
            let sev_levels = super::correlate::severity_at_or_above(&severity_min)?;
            let fetch_limit = limit + 1;
            let logs = self
                .run_db("correlate_state.logs", move |pool| {
                    db::search_logs(
                        pool,
                        &db::SearchParams {
                            hostname: Some(hostname_filter.clone()),
                            from: Some(from3),
                            to: Some(to3),
                            severity_in: Some(sev_levels),
                            limit: Some(fetch_limit as u32),
                            ..Default::default()
                        },
                    )
                })
                .await?;
            let truncated = logs.len() > limit;
            if truncated {
                global_truncated = true;
            }
            let log_entries: Vec<LogEntry> = logs
                .into_iter()
                .take(limit)
                .filter(|log| {
                    // Filter to the specific host_id when known
                    log.hostname == summary.hostname || log.hostname == host_id_filter
                })
                .map(LogEntry::from)
                .collect();

            host_entries.push(CorrelateStateHostEntry {
                host_id: summary.host_id.clone(),
                hostname: summary.hostname.clone(),
                heartbeat_summary: summary,
                logs: log_entries,
            });
        }

        Ok(CorrelateStateResponse {
            window: CorrelateStateWindow { from, to },
            hosts: host_entries,
            truncated: global_truncated,
        })
    }

    pub async fn filter_logs(&self, req: FilterLogsRequest) -> ServiceResult<SearchLogsResponse> {
        let params = filter_request_to_params(req)?;
        let logs = self
            .run_db("filter_logs", move |pool| db::search_logs(pool, &params))
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
            .run_db("tail_logs", move |pool| {
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
            .run_db("get_errors", move |pool| {
                db::get_error_summary(
                    pool,
                    from.as_deref(),
                    to.as_deref(),
                    group_by_app,
                    req.limit.map(|limit| limit.clamp(1, 100)),
                )
            })
            .await?;
        Ok(GetErrorsResponse {
            summary: rows.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn list_hosts(&self) -> ServiceResult<ListHostsResponse> {
        let rows = self.run_db("list_hosts", db::list_hosts).await?;
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
            .run_db("list_sessions", move |pool| {
                db::list_ai_sessions(pool, &params)
            })
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
        self.search_sessions_with_limit_policy(req, None).await
    }

    pub async fn search_sessions_with_limit_policy(
        &self,
        mut req: SearchSessionsRequest,
        policy: Option<AiLimitPolicy>,
    ) -> ServiceResult<SearchSessionsResponse> {
        let limit_clamped_to = policy.and_then(|policy| {
            req.limit
                .filter(|limit| *limit > policy.limit_cap)
                .map(|_| policy)
        });
        if let Some(policy) = limit_clamped_to {
            req.limit = Some(policy.limit_cap);
        }
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let params = db::SearchAiSessionsParams {
            query: req.query,
            ai_project: req.project,
            ai_tool: req.tool,
            hostname: None,
            app_name: None,
            from,
            to,
            limit: req.limit,
        };
        let result = self
            .run_db("search_sessions", move |pool| {
                db::search_ai_sessions(pool, &params)
            })
            .await?;
        let mut response: SearchSessionsResponse = result.into();
        if let Some(policy) = limit_clamped_to.filter(|policy| policy.report_limit_clamp) {
            response.limit_clamped_to = Some(policy.limit_cap);
            response.truncated = true;
        }
        Ok(response)
    }

    pub async fn search_abuse(
        &self,
        req: AbuseSearchRequest,
    ) -> ServiceResult<AbuseSearchResponse> {
        self.search_abuse_with_limit_policy(req, None).await
    }

    pub async fn search_abuse_with_limit_policy(
        &self,
        mut req: AbuseSearchRequest,
        policy: Option<AiLimitPolicy>,
    ) -> ServiceResult<AbuseSearchResponse> {
        let limit_clamped_to = policy.and_then(|policy| {
            req.limit
                .filter(|limit| *limit > policy.limit_cap)
                .map(|_| policy)
        });
        if let Some(policy) = limit_clamped_to {
            req.limit = Some(policy.limit_cap);
        }
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let params = db::AiAbuseParams {
            ai_project: req.project,
            ai_tool: req.tool,
            from,
            to,
            limit: req.limit,
            before: req.before,
            after: req.after,
            terms: req.terms,
        };
        let result = self
            .run_db("search_abuse", move |pool| {
                db::search_ai_abuse(pool, &params)
            })
            .await?;
        let mut response: AbuseSearchResponse = result.into();
        if let Some(policy) = limit_clamped_to.filter(|policy| policy.report_limit_clamp) {
            response.limit_clamped_to = Some(policy.limit_cap);
            response.truncated = true;
        }
        Ok(response)
    }

    pub async fn list_ai_incidents(
        &self,
        req: AiIncidentRequest,
    ) -> ServiceResult<AiIncidentResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let result = self
            .run_db("list_ai_incidents", move |pool| {
                db::search_ai_incidents(
                    pool,
                    &db::AiIncidentParams {
                        ai_project: req.project,
                        ai_tool: req.tool,
                        from,
                        to,
                        limit: req.limit,
                        window_minutes: req.window_minutes,
                        terms: req.terms,
                    },
                )
            })
            .await?;
        Ok(AiIncidentResponse {
            incidents: result.incidents.into_iter().map(Into::into).collect(),
            total_incidents: result.total_incidents,
            candidate_rows: result.candidate_rows,
            candidate_cap: result.candidate_cap,
            candidate_window_truncated: result.candidate_window_truncated,
            truncated: result.truncated,
        })
    }

    pub async fn investigate_ai_incidents(
        &self,
        req: AiInvestigateRequest,
    ) -> ServiceResult<AiInvestigateResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let result = self
            .run_db("investigate_ai_incidents", move |pool| {
                db::investigate_ai_incidents(
                    pool,
                    &db::AiInvestigateParams {
                        incident_id: req.incident_id,
                        ai_project: req.project,
                        ai_tool: req.tool,
                        from,
                        to,
                        limit: req.limit,
                        window_minutes: req.window_minutes,
                        correlation_window_minutes: req.correlation_window_minutes,
                        terms: req.terms,
                    },
                )
            })
            .await?;
        Ok(AiInvestigateResponse {
            evidence: result.evidence.into_iter().map(Into::into).collect(),
            total_incidents: result.total_incidents,
            truncated: result.truncated,
        })
    }

    pub async fn correlate_ai_logs(
        &self,
        req: AiCorrelateRequest,
    ) -> ServiceResult<AiCorrelateResponse> {
        self.correlate_ai_logs_with_limit_policy(req, AiCorrelateLimitPolicy::MCP)
            .await
    }

    pub async fn correlate_ai_logs_with_limit_policy(
        &self,
        req: AiCorrelateRequest,
        policy: AiCorrelateLimitPolicy,
    ) -> ServiceResult<AiCorrelateResponse> {
        let (req, events_per_anchor_clamped_to) = req.normalize_limits(policy);
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let window = req.window_minutes.unwrap_or(5).clamp(1, 120);
        let related_limit = req
            .events_per_anchor
            .unwrap_or(25)
            .clamp(1, policy.events_per_anchor_cap);
        let anchor_limit = req.limit.unwrap_or(10).clamp(1, 50);
        let severity_min = req.severity_min.unwrap_or_else(|| "warning".into());
        let severity_levels = severity_at_or_above(&severity_min)?;

        // Both DB calls share one connection snapshot inside a single spawn_blocking
        // boundary, eliminating the double cross-thread wakeup and the intermediate
        // allocation that was required to transfer anchor rows back to the async task
        // just to build window params and re-enter spawn_blocking.
        let anchor_params = db::AiCorrelateParams {
            ai_project: req.project,
            ai_tool: req.tool,
            ai_session_id: req.session_id,
            ai_query: req.ai_query,
            from,
            to,
            limit: Some(anchor_limit),
        };
        let log_query = req.log_query;
        let hostname = req.hostname;
        let source_ip = req.source_ip;
        let app_name = req.app_name;

        type CorrelateDbResult = (
            bool,
            Vec<(db::LogEntry, String, String)>,
            Vec<db::AiRelatedLogsForAnchor>,
        );
        let (anchors_truncated, anchor_entries, related_by_anchor) = self
            .run_db(
                "correlate_ai_logs",
                move |pool| -> anyhow::Result<CorrelateDbResult> {
                    let mut anchors = db::search_ai_anchors(pool, &anchor_params)?;
                    let anchors_truncated = anchors.len() > anchor_limit as usize;
                    anchors.truncate(anchor_limit as usize);

                    let delta = TimeDelta::try_minutes(i64::from(window))
                        .ok_or_else(|| anyhow::anyhow!("window_minutes overflow"))?;

                    let mut anchor_entries = Vec::with_capacity(anchors.len());
                    let mut windows = Vec::with_capacity(anchors.len());
                    for (anchor_index, anchor) in anchors.into_iter().enumerate() {
                        let ref_dt =
                            parse_required_timestamp(&anchor.timestamp, "anchor.timestamp")
                                .map_err(anyhow::Error::from)?;
                        let window_from = rfc3339_z(ref_dt - delta);
                        let window_to = rfc3339_z(ref_dt + delta);
                        windows.push(db::AiRelatedWindow {
                            anchor_index,
                            window_from: window_from.clone(),
                            window_to: window_to.clone(),
                        });
                        anchor_entries.push((anchor, window_from, window_to));
                    }

                    let related_params = db::AiRelatedLogsParams {
                        windows,
                        query: log_query,
                        hostname,
                        source_ip,
                        severity_in: severity_levels,
                        app_name,
                        limit_per_anchor: related_limit,
                    };
                    let related_by_anchor = db::search_ai_related_logs(pool, &related_params)?;
                    Ok((anchors_truncated, anchor_entries, related_by_anchor))
                },
            )
            .await?;

        let mut by_anchor: std::collections::HashMap<usize, db::AiRelatedLogsForAnchor> =
            related_by_anchor
                .into_iter()
                .map(|group| (group.anchor_index, group))
                .collect();

        let mut correlated = Vec::with_capacity(anchor_entries.len());
        let mut total_related_events = 0usize;
        for (anchor_index, (anchor, window_from, window_to)) in
            anchor_entries.into_iter().enumerate()
        {
            let (related_logs, related_truncated) = by_anchor
                .remove(&anchor_index)
                .map(|group| (group.logs, group.truncated))
                .unwrap_or((Vec::new(), false));
            total_related_events += related_logs.len();
            correlated.push(AiCorrelationAnchor {
                entry: anchor.into(),
                window_from,
                window_to,
                related: related_logs.into_iter().map(Into::into).collect(),
                related_truncated,
            });
        }

        Ok(AiCorrelateResponse {
            window_minutes: window,
            severity_min,
            total_anchors: correlated.len(),
            anchor_rows: correlated.len(),
            anchor_limit: anchor_limit as usize,
            anchors_truncated,
            related_limit_per_anchor: related_limit as usize,
            total_related_events,
            anchors: correlated,
            events_per_anchor_clamped_to,
        })
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
            .run_db("usage_blocks", move |pool| {
                db::get_ai_usage_blocks(pool, &params)
            })
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
            .run_db("project_context", move |pool| {
                db::get_ai_project_context(pool, &params)
            })
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
            .run_db("list_ai_tools", move |pool| {
                db::list_ai_tools(pool, &params)
            })
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
            .run_db("list_ai_projects", move |pool| {
                db::list_ai_projects(pool, &params)
            })
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
            source_ip_prefix: None,
            severity: None,
            severity_in: Some(severity_levels),
            app_name: None,
            facility: None,
            exclude_facility: None,
            process_id: None,
            from: Some(from.clone()),
            to: Some(to.clone()),
            received_from: None,
            received_to: None,
            limit: Some(limit + 1),
            ai_tool: None,
            ai_project: None,
            ai_session_id: None,
            event_action: None,
            exclude_ai: false,
        };
        let mut rows = self
            .run_db("correlate_events", move |pool| {
                db::search_logs(pool, &params)
            })
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
            .run_db("get_stats", move |pool| db::get_stats(pool, &storage))
            .await?
            .into();
        Ok(stats)
    }

    pub async fn db_status(&self) -> ServiceResult<DbMaintenanceStatus> {
        let storage = self.storage.clone();
        self.run_db("db_status", move |pool| {
            let page_count = db::db_pragma_i64(pool, db::PragmaName("page_count"))?;
            let freelist_count = db::db_pragma_i64(pool, db::PragmaName("freelist_count"))?;
            let page_size = db::db_pragma_i64(pool, db::PragmaName("page_size"))?;
            let auto_vacuum = db::db_pragma_i64(pool, db::PragmaName("auto_vacuum"))?;
            let journal_mode = db::db_pragma_string(pool, db::PragmaName("journal_mode"))?;
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

    pub async fn db_integrity(&self, quick: bool) -> ServiceResult<DbIntegrityResult> {
        self.run_db("db_integrity", move |pool| {
            let messages = db::db_integrity_check(pool, quick)?;
            Ok(DbIntegrityResult {
                ok: messages.len() == 1 && messages.first().is_some_and(|value| value == "ok"),
                messages,
            })
        })
        .await
    }

    async fn db_checkpoint(&self, mode: String) -> ServiceResult<DbCheckpointResult> {
        self.run_db("db_checkpoint", move |pool| {
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

    pub async fn db_checkpoint_checked(
        &self,
        req: DbCheckpointRequest,
    ) -> ServiceResult<DbCheckpointResult> {
        let mode = req.normalized_mode()?;
        self.db_checkpoint(mode).await
    }

    /// Read the live `page_count * page_size` (logical size, in bytes) via a
    /// fresh PRAGMA pair. Used by the `POST /api/db/vacuum` pre-flight in
    /// `src/api.rs::db_vacuum` so the 2GB guard cannot be defeated by a stale
    /// startup snapshot (bead 0p8r.17). Cheap enough to call per-request:
    /// two `PRAGMA` reads on a held connection inside `spawn_blocking`.
    pub async fn db_logical_size_bytes(&self) -> ServiceResult<u64> {
        self.run_db("db_logical_size_bytes", move |pool| {
            let page_count = db::db_pragma_i64(pool, db::PragmaName("page_count"))?;
            let page_size = db::db_pragma_i64(pool, db::PragmaName("page_size"))?;
            Ok((page_count.max(0) as u64).saturating_mul(page_size.max(0) as u64))
        })
        .await
    }

    async fn db_vacuum(&self, full: bool, incremental_pages: u32) -> ServiceResult<DbVacuumResult> {
        let storage = self.storage.clone();
        self.run_db("db_vacuum", move |pool| {
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

    pub async fn db_vacuum_checked(
        &self,
        req: DbVacuumRequest,
        full_vacuum_size_guard_bytes: u64,
    ) -> ServiceResult<DbVacuumResult> {
        if req.full && !req.force_enabled() {
            let size = self.db_logical_size_bytes().await?;
            if size > full_vacuum_size_guard_bytes {
                let gb = size as f64 / (1024.0 * 1024.0 * 1024.0);
                return Err(ServiceError::Busy(format!(
                    "DB size {gb:.2} GB; full VACUUM would block ingest. Pass {{\"force\":true}} or use incremental"
                )));
            }
        }
        self.db_vacuum(req.full, req.incremental_pages).await
    }

    pub async fn db_backup(&self, output: Option<PathBuf>) -> ServiceResult<DbBackupResult> {
        let db_path = self.storage.db_path.clone();
        self.run_db("db_backup", move |_pool| {
            let backup_path = backup_path_for(&db_path, output)?;
            if let Some(parent) = backup_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let escaped = backup_path.to_string_lossy().replace('\'', "''");
            let output = std::process::Command::new("sqlite3")
                .arg(&db_path)
                .arg(format!(".backup '{escaped}'"))
                .output()
                .map_err(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        anyhow::anyhow!(
                            "sqlite3 command not found in PATH; install sqlite3 to use database backup"
                        )
                    } else {
                        error.into()
                    }
                })?;
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
        self.run_db("index_ai_roots", move |pool| {
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
        self.run_db("add_ai_file", move |pool| {
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
        self.run_db("list_ai_checkpoints", move |pool| {
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
        self.run_db("list_ai_parse_errors", move |pool| {
            scanner::list_parse_errors(pool, &scanner::ParseErrorListOptions { limit })
        })
        .await
    }

    async fn prune_ai_checkpoints(
        &self,
        missing_only: bool,
        dry_run: bool,
        limit: Option<u32>,
    ) -> ServiceResult<scanner::PruneCheckpointsResult> {
        self.run_db("prune_ai_checkpoints", move |pool| {
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

    pub async fn prune_ai_checkpoints_checked(
        &self,
        req: super::models::AiPruneCheckpointsRequest,
    ) -> ServiceResult<scanner::PruneCheckpointsResult> {
        req.validate_admin()?;
        self.prune_ai_checkpoints(req.missing_only, req.dry_run, req.limit)
            .await
    }

    pub async fn ai_doctor(&self) -> ServiceResult<scanner::AiDoctorReport> {
        let db_path = self.storage.db_path.clone();
        self.run_db("ai_doctor", move |pool| scanner::ai_doctor(pool, &db_path))
            .await
    }

    pub async fn ai_indexing_health(
        &self,
        process_start_time: Option<String>,
    ) -> ServiceResult<scanner::AiIndexingHealth> {
        self.run_db("ai_indexing_health", move |pool| {
            scanner::ai_indexing_health(pool, process_start_time.as_deref())
        })
        .await
    }

    pub async fn list_apps(&self, req: ListAppsRequest) -> ServiceResult<ListAppsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let result = self
            .run_db("list_apps", move |pool| {
                db::list_apps(
                    pool,
                    &db::ListAppsParams {
                        hostname: req.hostname.as_deref(),
                        from: from.as_deref(),
                        to: to.as_deref(),
                        limit: req.limit.unwrap_or(500) as usize,
                        offset: req.offset.unwrap_or(0) as usize,
                    },
                )
            })
            .await?;
        Ok(ListAppsResponse {
            apps: result.apps.into_iter().map(Into::into).collect(),
            total: result.total,
        })
    }

    pub async fn list_source_ips(
        &self,
        req: ListSourceIpsRequest,
    ) -> ServiceResult<ListSourceIpsResponse> {
        let result = self
            .run_db("list_source_ips", move |pool| {
                db::list_source_ips(
                    pool,
                    &db::ListSourceIpsParams {
                        limit: req.limit.unwrap_or(500) as usize,
                        offset: req.offset.unwrap_or(0) as usize,
                    },
                )
            })
            .await?;
        Ok(ListSourceIpsResponse {
            source_ips: result.source_ips.into_iter().map(Into::into).collect(),
            total: result.total,
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
            .run_db("timeline", move |pool| {
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
            .run_db("patterns", move |pool| {
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
            .run_db("context", move |pool| -> anyhow::Result<_> {
                let (reference, hostname, timestamp, id): (LogEntry, String, String, Option<i64>) =
                    if let Some(id) = req.log_id {
                        let row = db::fetch_log_by_id(pool, id)?
                            .ok_or_else(|| anyhow::anyhow!("context_log_not_found:{id}"))?;
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
                            metadata_json: row.metadata_json.clone(),
                        };
                        (entry, row.hostname, row.timestamp, Some(row.id))
                    } else {
                        let hostname = req
                            .hostname
                            .clone()
                            .ok_or_else(|| anyhow::anyhow!("context_missing_pivot"))?;
                        let timestamp = synthetic_timestamp
                            .ok_or_else(|| anyhow::anyhow!("context_missing_pivot"))?;
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
                            metadata_json: None,
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
            .await
            .map_err(|error| match error {
                ServiceError::Internal(inner) => {
                    let msg = inner.to_string();
                    if msg == "context_missing_pivot" {
                        ServiceError::InvalidInput(
                            "Either `log_id` or both `hostname` + `timestamp` are required".into(),
                        )
                    } else if let Some(id) = msg.strip_prefix("context_log_not_found:") {
                        ServiceError::NotFound(format!("No log found for id {id}"))
                    } else {
                        ServiceError::Internal(inner)
                    }
                }
                other => other,
            })?;
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
            .run_db("get_log", move |pool| db::fetch_log_by_id(pool, id))
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
            .run_db("ingest_rate", move |pool| -> anyhow::Result<_> {
                let buckets = db::ingest_rate(pool, &now_clone, &cut_1m_q, &cut_5m_q, &cut_15m_q)?;
                let by_host = if want_by_host {
                    Some(db::ingest_rate_by_host(
                        pool, &now_clone, &cut_1m_q, &cut_5m_q, &cut_15m_q,
                    )?)
                } else {
                    None
                };
                let metrics = db::get_storage_metrics(pool, &storage)?;
                let write_blocked = db::exceeds_trigger(&metrics, &storage);
                Ok((buckets, by_host, write_blocked))
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
            .run_db("silent_hosts", move |pool| {
                db::silent_hosts(pool, &cutoff_q, now_unix)
            })
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
        let limit = req.limit.map(|limit| limit.clamp(1, 100));
        let hosts = self
            .run_db("clock_skew", move |pool| db::clock_skew(pool, &q, limit))
            .await?;
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
            .run_db("anomalies", move |pool| {
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
            .run_db("compare", move |pool| -> anyhow::Result<_> {
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

    // ---- Error detection MCP actions ----------------------------------------

    pub async fn unaddressed_errors(
        &self,
        req: super::models::UnaddressedErrorsRequest,
    ) -> ServiceResult<super::models::UnaddressedErrorsResponse> {
        let limit = req.limit.unwrap_or(50) as i64;
        let include_acked = req.include_acknowledged.unwrap_or(false);
        self.run_db("unaddressed_errors", move |pool| {
            let rows = crate::db::error_signatures::read_unaddressed(pool, limit, include_acked)?;
            let signatures = rows
                .into_iter()
                .map(|r| super::models::ErrorSignatureEntry {
                    signature_hash: r.signature_hash,
                    template: r.template,
                    sample_message: r.sample_message,
                    severity: r.severity,
                    sample_hostname: r.sample_hostname,
                    sample_app_name: r.sample_app_name,
                    first_seen_at: r.first_seen_at,
                    last_seen_at: r.last_seen_at,
                    total_count: r.total_count,
                    count_last_1h: r.count_last_1h,
                    acknowledged_at: r.acknowledged_at,
                })
                .collect();
            Ok(super::models::UnaddressedErrorsResponse { signatures })
        })
        .await
    }

    pub async fn ack_error(
        &self,
        req: super::models::AckErrorRequest,
        actor: impl Into<RequestActor>,
    ) -> ServiceResult<super::models::AckErrorResponse> {
        if let Some(ref n) = req.notes {
            if n.len() > 4096 {
                return Err(ServiceError::InvalidInput(
                    "notes exceeds 4096 chars".into(),
                ));
            }
        }
        let hash = req.signature_hash.clone();
        let notes = req.notes.clone();
        let actor = actor.into();
        let actor_owned = actor.display.clone();
        // Check it exists first
        let h = hash.clone();
        let exists = self
            .run_db("ack_error.exists", move |pool| {
                Ok(crate::db::error_signatures::read_signature_by_hash(
                    pool,
                    &h,
                    crate::app::error_detection::NORMALIZER_VERSION,
                )?
                .is_some())
            })
            .await?;
        if !exists {
            return Err(ServiceError::NotFound(format!(
                "Signature '{}' not found",
                hash
            )));
        }
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let now_clone = now.clone();
        let actor_clone = actor_owned.clone();
        let hash_clone = hash.clone();
        self.run_db("ack_error.commit", move |pool| {
            let mut conn = pool.get()?;
            let tx = conn.transaction()?;
            crate::db::error_signatures::record_ack_event(
                &tx,
                &hash_clone,
                crate::app::error_detection::NORMALIZER_VERSION,
                "ack",
                &actor_clone,
                notes.as_deref(),
            )?;
            crate::db::error_signatures::update_ack_projection(
                &tx,
                &hash_clone,
                crate::app::error_detection::NORMALIZER_VERSION,
                Some(&now_clone),
                Some(&actor_clone),
            )?;
            tx.commit()?;
            Ok(())
        })
        .await?;
        Ok(super::models::AckErrorResponse {
            signature_hash: hash,
            acknowledged_at: now,
            actor: actor_owned,
        })
    }

    pub async fn unack_error(
        &self,
        req: super::models::UnackErrorRequest,
        actor: impl Into<RequestActor>,
    ) -> ServiceResult<super::models::UnackErrorResponse> {
        if let Some(ref r) = req.reason {
            if r.len() > 4096 {
                return Err(ServiceError::InvalidInput(
                    "reason exceeds 4096 chars".into(),
                ));
            }
        }
        let hash = req.signature_hash.clone();
        let reason = req.reason.clone();
        let actor = actor.into();
        let actor_owned = actor.display.clone();
        // Check it exists first
        let h = hash.clone();
        let exists = self
            .run_db("unack_error.exists", move |pool| {
                Ok(crate::db::error_signatures::read_signature_by_hash(
                    pool,
                    &h,
                    crate::app::error_detection::NORMALIZER_VERSION,
                )?
                .is_some())
            })
            .await?;
        if !exists {
            return Err(ServiceError::NotFound(format!(
                "Signature '{}' not found",
                hash
            )));
        }
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let actor_clone = actor_owned.clone();
        let hash_clone = hash.clone();
        self.run_db("unack_error.commit", move |pool| {
            let mut conn = pool.get()?;
            let tx = conn.transaction()?;
            crate::db::error_signatures::record_ack_event(
                &tx,
                &hash_clone,
                crate::app::error_detection::NORMALIZER_VERSION,
                "unack",
                &actor_clone,
                reason.as_deref(),
            )?;
            crate::db::error_signatures::update_ack_projection(
                &tx,
                &hash_clone,
                crate::app::error_detection::NORMALIZER_VERSION,
                None,
                None,
            )?;
            tx.commit()?;
            Ok(())
        })
        .await?;
        Ok(super::models::UnackErrorResponse {
            signature_hash: hash,
            unacked_at: now,
            actor: actor_owned,
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

// ---------------------------------------------------------------------------
// Notifications service methods

impl SyslogService {
    /// List recent notification firings.
    pub async fn notifications_recent(
        &self,
        limit: i64,
        rule_id: Option<String>,
        since: Option<String>,
    ) -> ServiceResult<Vec<crate::db::notifications::FiringRow>> {
        self.notifications_recent_checked(NotificationsRecentRequest {
            limit: Some(limit),
            rule_id,
            since,
        })
        .await
    }

    pub async fn notifications_recent_checked(
        &self,
        req: NotificationsRecentRequest,
    ) -> ServiceResult<Vec<crate::db::notifications::FiringRow>> {
        let limit = req.effective_limit();
        self.run_db("notifications_recent", move |pool| {
            let conn = pool.get()?;
            crate::db::notifications::firings_recent(
                &conn,
                limit,
                req.rule_id.as_deref(),
                req.since.as_deref(),
            )
            .map_err(anyhow::Error::from)
        })
        .await
    }

    /// Send a test notification via configured Apprise destinations.
    ///
    /// Rate-limited to 10/min per actor using an in-memory counter that resets
    /// after 60s of inactivity per actor.
    pub async fn notifications_test_checked(
        &self,
        body: String,
        actor: impl Into<RequestActor>,
        config: &crate::config::NotificationsConfig,
    ) -> ServiceResult<String> {
        self.notifications_test_with_destinations(
            body,
            actor,
            config.apprise_url.clone(),
            config.apprise_urls.clone(),
        )
        .await
    }

    async fn notifications_test_with_destinations(
        &self,
        body: String,
        actor: impl Into<RequestActor>,
        apprise_url: String,
        apprise_urls: Vec<String>,
    ) -> ServiceResult<String> {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        use std::time::Instant;

        const MAX_PER_MIN: u32 = 10;
        let actor = actor.into().display;

        // In-memory rate limiter: actor -> (count, window_start)
        static RATE_LIMITER: OnceLock<Mutex<HashMap<String, (u32, Instant)>>> = OnceLock::new();
        let limiter = RATE_LIMITER.get_or_init(|| Mutex::new(HashMap::new()));

        {
            let mut map = limiter.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            // Evict stale entries (window elapsed) to prevent unbounded map growth.
            map.retain(|_, entry| entry.1.elapsed().as_secs() < 60);
            let entry = map.entry(actor.clone()).or_insert((0, now));
            // Reset window if > 60s has elapsed (belt-and-suspenders after retain)
            if entry.1.elapsed().as_secs() >= 60 {
                *entry = (0, now);
            }
            entry.0 += 1;
            if entry.0 > MAX_PER_MIN {
                return Err(crate::app::ServiceError::InvalidInput(format!(
                    "Rate limit exceeded for actor '{actor}': max {MAX_PER_MIN} test notifications per minute"
                )));
            }
        }

        // Send test notification asynchronously
        let client = crate::notifications::apprise::AppriseClient::new(apprise_url);
        let escaped_body = crate::notifications::apprise::escape_for_notification(&body);
        let result = client
            .notify(
                &apprise_urls,
                "Test Notification",
                &escaped_body,
                crate::notifications::apprise::NotifyType::Info,
            )
            .await;

        match result {
            Ok(resp) => Ok(format!(
                "Test notification sent (status {})",
                resp.status_code
            )),
            Err(e) => Err(crate::app::ServiceError::Internal(anyhow::anyhow!(
                "Apprise delivery failed: {e}"
            ))),
        }
    }

    // -------------------------------------------------------------------------
    // RAG v1 methods
    // -------------------------------------------------------------------------

    pub async fn similar_incidents(
        &self,
        req: SimilarIncidentsRequest,
    ) -> ServiceResult<SimilarIncidentsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let severity_min = validate_optional_severity(req.severity_min)?;
        let result = self
            .run_db("similar_incidents", move |pool| {
                db::similar_incidents_clusters(
                    pool,
                    &db::SimilarIncidentsParams {
                        query: req.query,
                        hostname: req.hostname,
                        app_name: req.app_name,
                        severity_min,
                        from,
                        to,
                        window_minutes: req.window_minutes,
                        limit: req.limit,
                    },
                )
            })
            .await?;
        Ok(result.into())
    }

    pub async fn ask_history(&self, req: AskHistoryRequest) -> ServiceResult<AskHistoryResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let result = self
            .run_db("ask_history", move |pool| {
                db::ask_history_sessions(
                    pool,
                    &db::AskHistoryParams {
                        query: req.query,
                        hostname: req.hostname,
                        app_name: req.app_name,
                        from,
                        to,
                        limit: req.limit,
                    },
                )
            })
            .await?;
        Ok(result.into())
    }

    pub async fn incident_context(
        &self,
        req: IncidentContextRequest,
    ) -> ServiceResult<IncidentContextResponse> {
        // Both from and to are required — validate and normalize to rfc3339_z format.
        let from = rfc3339_z(parse_required_timestamp(&req.from, "from")?);
        let to = rfc3339_z(parse_required_timestamp(&req.to, "to")?);
        let result = self
            .run_db("incident_context", move |pool| {
                db::incident_context_summary(
                    pool,
                    &db::IncidentContextParams {
                        from,
                        to,
                        hostname: req.hostname,
                        app_name: req.app_name,
                        // req.query accepted but deferred to v2 FTS integration
                        severity_min: req.severity_min,
                        limit: req.limit,
                    },
                )
            })
            .await?;
        Ok(result.into())
    }

    pub async fn run_gemini_assess(&self, req: AiAssessRequest) -> ServiceResult<AiAssessResponse> {
        self.run_gemini_assess_with_delta(req, |_| Ok(())).await
    }

    pub async fn run_gemini_assess_with_delta<F>(
        &self,
        req: AiAssessRequest,
        on_delta: F,
    ) -> ServiceResult<AiAssessResponse>
    where
        F: FnMut(&str) -> anyhow::Result<()> + Send,
    {
        let incident_id = req.incident_id.clone();
        let gemini_config = GeminiAssessConfig::from_env(req.model);
        let invest_req = AiInvestigateRequest {
            incident_id: Some(incident_id.clone()),
            project: req.project,
            tool: req.tool,
            from: req.from,
            to: req.to,
            limit: Some(req.limit.unwrap_or(200).max(200)),
            window_minutes: req.window_minutes,
            correlation_window_minutes: req.correlation_window_minutes,
            terms: req.terms,
        };
        let invest_resp = self.investigate_ai_incidents(invest_req).await?;

        let matching: Vec<_> = invest_resp
            .evidence
            .iter()
            .filter(|e| e.incident.incident_id == incident_id)
            .collect();

        if matching.is_empty() {
            return Err(ServiceError::InvalidInput(format!(
                "no incident found with id '{}'; run `syslog ai incidents` to list available ids",
                incident_id
            )));
        }

        let evidence_json = serde_json::to_string_pretty(&matching)
            .map_err(|e| ServiceError::Internal(anyhow::anyhow!("json serialize failed: {e}")))?;
        let prompt = build_assessment_prompt(&evidence_json);
        let prompt_preview = prompt.chars().take(500).collect::<String>();
        let evidence_summary = AiAssessEvidenceSummary {
            total_incidents: invest_resp.total_incidents,
            evidence_bundle_count: matching.len(),
            total_anchors: matching.iter().map(|e| e.anchors.len()).sum(),
        };

        let assessment = run_gemini_assessment(&prompt, &gemini_config, on_delta)
            .await
            .map_err(ServiceError::Internal)?;

        Ok(AiAssessResponse {
            incident_id,
            assessment,
            prompt_preview,
            evidence_summary,
        })
    }
}

fn search_request_to_params(req: SearchLogsRequest) -> ServiceResult<SearchParams> {
    request_parts_to_params(FilterRequestParts {
        hostname: req.hostname.clone(),
        source_ip: req.source_ip.clone(),
        severity: req.severity,
        app_name: req.app_name.clone(),
        facility: req.facility.clone(),
        exclude_facility: req.exclude_facility.clone(),
        process_id: req.process_id.clone(),
        from: req.from,
        to: req.to,
        received_from: req.received_from,
        received_to: req.received_to,
        limit: req.limit,
        source_kind: req.source_kind,
        tool: req.tool,
        project: req.project,
        session_id: req.session_id,
        container: req.container,
        docker_host: req.docker_host,
        stream: req.stream,
        event_action: req.event_action.clone(),
    })
}

fn filter_request_to_params(req: FilterLogsRequest) -> ServiceResult<SearchParams> {
    request_parts_to_params(FilterRequestParts {
        hostname: req.hostname,
        source_ip: req.source_ip,
        severity: req.severity,
        app_name: req.app_name,
        facility: req.facility,
        exclude_facility: req.exclude_facility,
        process_id: req.process_id,
        from: req.from,
        to: req.to,
        received_from: req.received_from,
        received_to: req.received_to,
        limit: req.limit,
        source_kind: req.source_kind,
        tool: req.tool,
        project: req.project,
        session_id: req.session_id,
        container: req.container,
        docker_host: req.docker_host,
        stream: req.stream,
        event_action: req.event_action.clone(),
    })
}

struct FilterRequestParts {
    hostname: Option<String>,
    source_ip: Option<String>,
    severity: Option<String>,
    app_name: Option<String>,
    facility: Option<String>,
    exclude_facility: Option<String>,
    process_id: Option<String>,
    from: Option<String>,
    to: Option<String>,
    received_from: Option<String>,
    received_to: Option<String>,
    limit: Option<u32>,
    source_kind: Option<String>,
    tool: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
    container: Option<String>,
    docker_host: Option<String>,
    stream: Option<String>,
    event_action: Option<String>,
}

fn request_parts_to_params(req: FilterRequestParts) -> ServiceResult<SearchParams> {
    let severity = validate_optional_severity(req.severity.clone())?;
    let mut params = SearchParams {
        query: None,
        hostname: req.hostname.clone(),
        source_ip: req.source_ip.clone(),
        source_ip_prefix: None,
        severity,
        severity_in: None,
        app_name: req.app_name.clone(),
        facility: req.facility.clone(),
        exclude_facility: req.exclude_facility.clone(),
        process_id: req.process_id.clone(),
        from: parse_optional_timestamp(req.from.as_deref(), "from")?,
        to: parse_optional_timestamp(req.to.as_deref(), "to")?,
        received_from: parse_optional_timestamp(req.received_from.as_deref(), "received_from")?,
        received_to: parse_optional_timestamp(req.received_to.as_deref(), "received_to")?,
        limit: req.limit,
        ai_tool: req.tool.clone(),
        ai_project: req.project.clone(),
        ai_session_id: req.session_id.clone(),
        event_action: req.event_action.clone(),
        exclude_ai: false,
    };

    apply_log_filter_aliases(&mut params, req.source_kind.as_deref(), &req)?;
    Ok(params)
}

fn apply_log_filter_aliases(
    params: &mut SearchParams,
    source_kind: Option<&str>,
    req: &FilterRequestParts,
) -> ServiceResult<()> {
    if req.stream.is_some() && source_kind != Some("docker-stream") {
        return Err(ServiceError::InvalidInput(
            "`stream` is only supported with source_kind=docker-stream".into(),
        ));
    }
    if req.stream.is_some() && (req.docker_host.is_none() || req.container.is_none()) {
        return Err(ServiceError::InvalidInput(
            "`stream` requires docker_host and container".into(),
        ));
    }
    match source_kind {
        None => {
            if let Some(container) = &req.container {
                params.app_name.get_or_insert_with(|| container.clone());
            }
        }
        Some("docker-stream") => {
            params.source_ip_prefix = Some(docker_source_prefix("docker://", req));
            if let Some(container) = &req.container {
                params.app_name.get_or_insert_with(|| container.clone());
            }
        }
        Some("docker-event") => {
            params.source_ip_prefix = Some(docker_source_prefix("docker-event://", req));
            if let Some(container) = &req.container {
                params.app_name.get_or_insert_with(|| container.clone());
            }
        }
        Some("agent-command") => {
            params.source_ip_prefix = Some("agent-command://".to_string());
        }
        Some("shell-history") => {
            params.source_ip_prefix = Some("shell-history://".to_string());
        }
        Some("claude") | Some("claude-transcript") => {
            apply_source_kind_tool_alias(params, "claude")?;
        }
        Some("codex") | Some("codex-transcript") => {
            apply_source_kind_tool_alias(params, "codex")?;
        }
        Some("gemini") | Some("gemini-transcript") => {
            apply_source_kind_tool_alias(params, "gemini")?;
        }
        Some("transcript") => {
            params.source_ip_prefix = Some("transcript://".to_string());
        }
        Some("syslog-udp") | Some("syslog-tcp") | Some("otlp") => {
            return Err(ServiceError::InvalidInput(format!(
                "source_kind={} is not indexed separately in v1; filter by hostname, source_ip, app_name, facility, and time range instead",
                source_kind.unwrap()
            )));
        }
        Some(other) => {
            return Err(ServiceError::InvalidInput(format!(
                "unsupported source_kind '{other}'. Supported: docker-stream, docker-event, agent-command, shell-history, transcript, claude, codex, gemini"
            )));
        }
    }
    Ok(())
}

fn apply_source_kind_tool_alias(
    params: &mut SearchParams,
    expected_tool: &str,
) -> ServiceResult<()> {
    match params.ai_tool.as_deref() {
        Some(actual_tool) if actual_tool != expected_tool => Err(ServiceError::InvalidInput(
            format!("source_kind={expected_tool} conflicts with tool={actual_tool}"),
        )),
        Some(_) => Ok(()),
        None => {
            params.ai_tool = Some(expected_tool.to_string());
            Ok(())
        }
    }
}

fn docker_source_prefix(scheme: &str, req: &FilterRequestParts) -> String {
    let mut prefix = scheme.to_string();
    if let Some(host) = &req.docker_host {
        prefix.push_str(host);
        prefix.push('/');
        if let Some(container) = &req.container {
            prefix.push_str(container);
            prefix.push('/');
            if let Some(stream) = &req.stream {
                prefix.push_str(stream);
            }
        }
    }
    prefix
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
