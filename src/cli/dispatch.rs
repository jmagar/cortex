//! Per-arm dispatch for query commands (bead 0p8r.7).
//!
//! For each of the 7 query commands (search/tail/errors/hosts/correlate/
//! stats/sessions) we expose:
//!
//! - A `Cli*Args::into_request()` conversion that constructs the `*Request`
//!   struct shared by the service layer and the REST surface. Sharing the
//!   constructor between the Local and HTTP arms is what guards against
//!   per-arm field drift (eng-review #A37). The unit tests below pin the
//!   shape via `format!("{req:?}")` snapshots.
//! - A `run_X(mode, args)` free `async fn` that branches on [`CliMode`] and
//!   either calls the local [`SyslogService`] directly or routes through
//!   [`HttpClient`]. The HTTP arm is wrapped in [`http_or_cancel`] so a
//!   SIGINT during a long-running request bails with `"interrupted"`
//!   (eng-review #A29). The Local arm is sync SQL — no cancellation needed.
//!
//! `--json` printing reuses the existing `print_*_response` formatters from
//! `super::*`, so output is byte-identical between modes: the HTTP path
//! proxies the same service the Local path would invoke server-side.

use anyhow::{bail, Result};
use std::future::Future;
use std::path::PathBuf;
use syslog_mcp::app::{
    AbuseSearchRequest, AckErrorRequest, AiCheckpointsRequest, AiCorrelateRequest,
    AiParseErrorsRequest, AiPruneCheckpointsRequest, CorrelateEventsRequest, DbCheckpointRequest,
    DbIntegrityRequest, DbVacuumRequest, GetErrorsRequest, IncidentRequest, IngestRateRequest,
    ListAiProjectsRequest, ListAiToolsRequest, ListSessionsRequest, ListSourceIpsRequest,
    PatternsRequest, ProjectContextRequest, SearchLogsRequest, SearchSessionsRequest,
    TailLogsRequest, TimelineRequest, UnackErrorRequest, UnaddressedErrorsRequest,
    UsageBlocksRequest,
};

use super::{
    ai_smoke_watch, ai_watch_status, ensure_ai_doctor_success, ensure_index_success,
    print_abuse_search_response, print_ai_correlate_response, print_ai_doctor_response,
    print_ai_parse_errors_response, print_ai_projects_response, print_ai_smoke_watch_response,
    print_ai_tools_response, print_ai_watch_status_response, print_ask_history_response,
    print_checkpoints_response, print_correlate_response, print_db_backup_response,
    print_db_checkpoint_response, print_db_integrity_response, print_db_status_response,
    print_db_vacuum_response, print_errors_response, print_hosts_response,
    print_incident_context_response, print_incident_response, print_index_response,
    print_project_context_response, print_prune_checkpoints_response, print_search_response,
    print_search_sessions_response, print_sessions_response, print_similar_incidents_response,
    print_stats_response, print_usage_blocks_response, run_coordination_phases, AiAbuseArgs,
    AiAddArgs, AiAskHistoryArgs, AiBlocksArgs, AiCheckpointsArgs, AiContextArgs, AiCorrelateArgs,
    AiDoctorArgs, AiErrorsArgs, AiIncidentContextArgs, AiIndexArgs, AiListArgs,
    AiPruneCheckpointsArgs, AiSearchArgs, AiSimilarArgs, AiWatchArgs, CliMode, CorrelateArgs,
    DbBackupArgs, DbCheckpointArgs, DbIntegrityArgs, DbStatusArgs, DbVacuumArgs, IncidentArgs,
    IngestRateArgs, NotifyRecentArgs, NotifyTestArgs, OutputArgs, PatternsArgs, SearchArgs,
    SessionsArgs, SigAckArgs, SigListArgs, SigUnackArgs, SourceIpsArgs, TailArgs, TimeRangeArgs,
    TimelineArgs,
};

// ─── Arg → Request conversions ──────────────────────────────────────────────
//
// One per `Cli*Args` struct in scope. No `IntoRequest` trait — per locked
// decision (memo from the bead description), a trait with one impl per type
// would be premature. The free `into_request()` methods are simpler and
// individually inlinable.

impl SearchArgs {
    pub(super) fn into_request(self) -> SearchLogsRequest {
        SearchLogsRequest {
            query: self.query,
            hostname: self.hostname,
            source_ip: self.source_ip,
            severity: self.severity,
            app_name: self.app_name,
            facility: self.facility,
            exclude_facility: self.exclude_facility,
            process_id: None,
            from: self.from,
            to: self.to,
            received_from: self.received_from,
            received_to: self.received_to,
            limit: self.limit,
        }
    }
}

impl IncidentArgs {
    pub(super) fn into_request(self) -> IncidentRequest {
        IncidentRequest {
            around: self.around,
            minutes: self.minutes,
            service: self.service,
            hostname: self.hostname,
            limit: self.limit,
        }
    }
}

impl TailArgs {
    pub(super) fn into_request(self) -> TailLogsRequest {
        TailLogsRequest {
            hostname: self.hostname,
            source_ip: self.source_ip,
            app_name: self.app_name,
            severity_min: None,
            n: self.n,
        }
    }
}

impl TimeRangeArgs {
    pub(super) fn into_errors_request(self) -> GetErrorsRequest {
        GetErrorsRequest {
            from: self.from,
            to: self.to,
            group_by: None,
        }
    }
}

impl SessionsArgs {
    pub(super) fn into_request(self) -> ListSessionsRequest {
        ListSessionsRequest {
            project: self.project,
            tool: self.tool,
            hostname: self.hostname,
            from: self.from,
            to: self.to,
            limit: self.limit,
        }
    }
}

impl CorrelateArgs {
    pub(super) fn into_request(self) -> CorrelateEventsRequest {
        CorrelateEventsRequest {
            reference_time: self.reference_time,
            window_minutes: self.window_minutes,
            severity_min: self.severity_min,
            hostname: self.hostname,
            source_ip: self.source_ip,
            query: self.query,
            limit: self.limit,
        }
    }
}

// ─── Cancellation helper ────────────────────────────────────────────────────

/// Wrap an HTTP call so SIGINT (`ctrl_c`) cancels the in-flight request and
/// bails with `"interrupted"` (eng-review #A29).
///
/// Indirects through [`http_or_cancel_with`] so unit tests can pass a
/// deterministic cancellation future instead of `tokio::signal::ctrl_c()`,
/// which is impractical to trigger from inside the test process.
pub(super) async fn http_or_cancel<T>(fut: impl Future<Output = Result<T>>) -> Result<T> {
    http_or_cancel_with(fut, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}

/// Test-visible variant of [`http_or_cancel`] that accepts an arbitrary
/// cancellation future. Production code calls the wrapper above; tests in
/// `dispatch_tests.rs` plug in `tokio::time::sleep(...)` so the cancel branch
/// is deterministic.
pub(super) async fn http_or_cancel_with<T>(
    fut: impl Future<Output = Result<T>>,
    cancel: impl Future<Output = ()>,
) -> Result<T> {
    tokio::select! {
        r = fut => r,
        _ = cancel => bail!("interrupted"),
    }
}

// ─── Per-command dispatch ───────────────────────────────────────────────────

pub(super) async fn run_search(mode: &CliMode, args: SearchArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.search_logs(req).await?,
        CliMode::Http(client) => http_or_cancel(client.search(&req)).await?,
    };
    print_search_response(&response, json)
}

pub(super) async fn run_tail(mode: &CliMode, args: TailArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.tail_logs(req).await?,
        CliMode::Http(client) => http_or_cancel(client.tail(&req)).await?,
    };
    print_search_response(&response, json)
}

pub(super) async fn run_errors(mode: &CliMode, args: TimeRangeArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_errors_request();
    let response = match mode {
        CliMode::Local(service) => service.get_errors(req).await?,
        CliMode::Http(client) => http_or_cancel(client.errors(&req)).await?,
    };
    print_errors_response(&response, json)
}

pub(super) async fn run_hosts(mode: &CliMode, args: super::OutputArgs) -> Result<()> {
    let response = match mode {
        CliMode::Local(service) => service.list_hosts().await?,
        CliMode::Http(client) => http_or_cancel(client.hosts()).await?,
    };
    print_hosts_response(&response, args.json)
}

pub(super) async fn run_incident(mode: &CliMode, args: IncidentArgs) -> Result<()> {
    let json = args.json;
    match mode {
        CliMode::Http(_) => bail!("incident reads host-local service logs; omit --http"),
        CliMode::Local(service) => {
            let response = service.incident(args.into_request()).await?;
            print_incident_response(&response, json)
        }
    }
}

pub(super) async fn run_correlate(mode: &CliMode, args: CorrelateArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.correlate_events(req).await?,
        CliMode::Http(client) => http_or_cancel(client.correlate(&req)).await?,
    };
    print_correlate_response(&response, json)
}

pub(super) async fn run_stats(mode: &CliMode, args: super::OutputArgs) -> Result<()> {
    let response = match mode {
        CliMode::Local(service) => service.get_stats().await?,
        CliMode::Http(client) => http_or_cancel(client.stats()).await?,
    };
    print_stats_response(&response, args.json)
}

pub(super) async fn run_sessions(mode: &CliMode, args: SessionsArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.list_sessions(req).await?,
        CliMode::Http(client) => http_or_cancel(client.sessions(&req)).await?,
    };
    print_sessions_response(&response, json)
}

// ─── AI Arg → Request conversions (bead 0p8r.8) ─────────────────────────────

impl AiSearchArgs {
    pub(super) fn into_request(self) -> SearchSessionsRequest {
        SearchSessionsRequest {
            query: self.query,
            project: self.project,
            tool: self.tool,
            from: self.from,
            to: self.to,
            limit: self.limit,
        }
    }
}

impl AiAbuseArgs {
    pub(super) fn into_request(self) -> AbuseSearchRequest {
        AbuseSearchRequest {
            project: self.project,
            tool: self.tool,
            from: self.from,
            to: self.to,
            limit: self.limit,
            before: self.before,
            after: self.after,
            terms: self.terms,
        }
    }
}

impl AiCorrelateArgs {
    pub(super) fn into_request(self) -> AiCorrelateRequest {
        AiCorrelateRequest {
            project: self.project,
            tool: self.tool,
            session_id: self.session_id,
            ai_query: self.ai_query,
            log_query: self.log_query,
            hostname: self.hostname,
            source_ip: self.source_ip,
            app_name: self.app_name,
            from: self.from,
            to: self.to,
            window_minutes: self.window_minutes,
            severity_min: self.severity_min,
            limit: self.limit,
            events_per_anchor: self.events_per_anchor,
        }
    }
}

impl AiBlocksArgs {
    pub(super) fn into_request(self) -> UsageBlocksRequest {
        UsageBlocksRequest {
            project: self.project,
            tool: self.tool,
            from: self.from,
            to: self.to,
        }
    }
}

impl AiContextArgs {
    pub(super) fn into_request(self) -> ProjectContextRequest {
        ProjectContextRequest {
            project: self.project,
            tool: self.tool,
            limit: self.limit,
        }
    }
}

impl AiListArgs {
    pub(super) fn into_tools_request(self) -> ListAiToolsRequest {
        ListAiToolsRequest {
            project: self.project,
            from: self.from,
            to: self.to,
        }
    }

    pub(super) fn into_projects_request(self) -> ListAiProjectsRequest {
        ListAiProjectsRequest {
            tool: self.tool,
            from: self.from,
            to: self.to,
        }
    }
}

impl AiCheckpointsArgs {
    pub(super) fn into_request(self) -> AiCheckpointsRequest {
        AiCheckpointsRequest {
            errors_only: self.errors_only,
            missing_only: self.missing_only,
            limit: self.limit,
        }
    }
}

impl AiErrorsArgs {
    pub(super) fn into_request(self) -> AiParseErrorsRequest {
        AiParseErrorsRequest { limit: self.limit }
    }
}

impl AiPruneCheckpointsArgs {
    pub(super) fn into_request(self) -> AiPruneCheckpointsRequest {
        AiPruneCheckpointsRequest {
            dry_run: self.dry_run,
            missing_only: self.missing_only,
            limit: self.limit,
        }
    }
}

impl AiSimilarArgs {
    pub(super) fn into_request(self) -> SimilarIncidentsRequest {
        SimilarIncidentsRequest {
            query: self.query,
            hostname: self.hostname,
            app_name: self.app_name,
            severity_min: self.severity_min,
            from: self.from,
            to: self.to,
            window_minutes: self.window_minutes,
            limit: self.limit,
        }
    }
}

impl AiAskHistoryArgs {
    pub(super) fn into_request(self) -> AskHistoryRequest {
        AskHistoryRequest {
            query: self.query,
            hostname: self.hostname,
            app_name: self.app_name,
            from: self.from,
            to: self.to,
            limit: self.limit,
        }
    }
}

impl AiIncidentContextArgs {
    pub(super) fn into_request(self) -> IncidentContextRequest {
        IncidentContextRequest {
            from: self.from,
            to: self.to,
            hostname: self.hostname,
            app_name: self.app_name,
            query: self.query,
            severity_min: self.severity_min,
            limit: self.limit,
        }
    }
}

// ─── AI per-command dispatch (bead 0p8r.8) ──────────────────────────────────
//
// HTTP-capable (10): search, abuse, correlate, blocks, context, tools,
//   projects, checkpoints, errors, prune_checkpoints.
// LOCAL-only (6): index, add, doctor, smoke_watch, watch_status, watch.
//   These bail in HTTP mode with an inline message per the bead table
//   (no shared helper — eng-review #S4).

pub(super) async fn run_ai_search(mode: &CliMode, args: AiSearchArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.search_sessions(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_search(&req)).await?,
    };
    print_search_sessions_response(&response, json)
}

pub(super) async fn run_ai_abuse(mode: &CliMode, args: AiAbuseArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.search_abuse(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_abuse(&req)).await?,
    };
    print_abuse_search_response(&response, json)
}

pub(super) async fn run_ai_correlate(mode: &CliMode, args: AiCorrelateArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.correlate_ai_logs(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_correlate(&req)).await?,
    };
    print_ai_correlate_response(&response, json)
}

pub(super) async fn run_ai_blocks(mode: &CliMode, args: AiBlocksArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.usage_blocks(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_blocks(&req)).await?,
    };
    print_usage_blocks_response(&response, json)
}

pub(super) async fn run_ai_context(mode: &CliMode, args: AiContextArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.project_context(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_context(&req)).await?,
    };
    print_project_context_response(&response, json)
}

pub(super) async fn run_ai_tools(mode: &CliMode, args: AiListArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_tools_request();
    let response = match mode {
        CliMode::Local(service) => service.list_ai_tools(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_tools(&req)).await?,
    };
    print_ai_tools_response(&response, json)
}

pub(super) async fn run_ai_projects(mode: &CliMode, args: AiListArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_projects_request();
    let response = match mode {
        CliMode::Local(service) => service.list_ai_projects(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_projects(&req)).await?,
    };
    print_ai_projects_response(&response, json)
}

pub(super) async fn run_ai_checkpoints(mode: &CliMode, args: AiCheckpointsArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => {
            service
                .list_ai_checkpoints(req.errors_only, req.missing_only, req.limit)
                .await?
        }
        CliMode::Http(client) => http_or_cancel(client.ai_checkpoints(&req)).await?,
    };
    print_checkpoints_response(&response, json)
}

pub(super) async fn run_ai_errors(mode: &CliMode, args: AiErrorsArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.list_ai_parse_errors(req.limit).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_parse_errors(&req)).await?,
    };
    print_ai_parse_errors_response(&response, json)
}

pub(super) async fn run_ai_prune_checkpoints(
    mode: &CliMode,
    args: AiPruneCheckpointsArgs,
) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => {
            service
                .prune_ai_checkpoints(req.missing_only, req.dry_run, req.limit)
                .await?
        }
        CliMode::Http(client) => http_or_cancel(client.prune_ai_checkpoints(&req)).await?,
    };
    print_prune_checkpoints_response(&response, json)
}

// ─── LOCAL-only AI commands (6) — error in HTTP mode ────────────────────────

pub(super) async fn run_ai_index(mode: &CliMode, args: AiIndexArgs) -> Result<()> {
    let service = match mode {
        CliMode::Http(_) => {
            bail!("ai index reads host ~/.claude/projects; omit --http")
        }
        CliMode::Local(service) => service,
    };
    let response = service
        .index_ai_roots(args.path, args.force, args.since)
        .await?;
    print_index_response(&response, args.json)?;
    ensure_index_success(&response)
}

pub(super) async fn run_ai_add(mode: &CliMode, args: AiAddArgs) -> Result<()> {
    let service = match mode {
        CliMode::Http(_) => bail!("ai add reads a host file path; omit --http"),
        CliMode::Local(service) => service,
    };
    let response = service.add_ai_file(args.file, args.force).await?;
    print_index_response(&response, args.json)?;
    ensure_index_success(&response)
}

pub(super) async fn run_ai_doctor(mode: &CliMode, args: AiDoctorArgs) -> Result<()> {
    let service = match mode {
        CliMode::Http(_) => {
            bail!("ai doctor checks host filesystem permissions; omit --http")
        }
        CliMode::Local(service) => service,
    };
    let response = service.ai_doctor().await?;
    print_ai_doctor_response(&response, args.json)?;
    ensure_ai_doctor_success(&response, args.strict_permissions)
}

pub(super) async fn run_ai_smoke_watch(mode: &CliMode, args: OutputArgs) -> Result<()> {
    let service = match mode {
        CliMode::Http(_) => {
            bail!("ai smoke-watch writes synthetic transcript to host fs; omit --http")
        }
        CliMode::Local(service) => service,
    };
    let response = ai_smoke_watch(service).await?;
    print_ai_smoke_watch_response(&response, args.json)?;
    if !response.pruned_missing_checkpoint {
        bail!("AI watch smoke checkpoint was not pruned within 30s");
    }
    Ok(())
}

pub(super) async fn run_ai_watch_status(mode: &CliMode, args: OutputArgs) -> Result<()> {
    if matches!(mode, CliMode::Http(_)) {
        bail!("ai watch-status shells out to systemctl on host; omit --http");
    }
    let CliMode::Local(service) = mode else {
        unreachable!("http mode returned above");
    };
    let response = ai_watch_status(service).await?;
    print_ai_watch_status_response(&response, args.json)
}

pub(super) async fn run_ai_watch(mode: &CliMode, args: AiWatchArgs) -> Result<()> {
    let service = match mode {
        CliMode::Http(_) => bail!("ai watch is a long-running daemon; omit --http"),
        CliMode::Local(service) => service.clone(),
    };
    let options = syslog_mcp::ai_watch::WatchOptions {
        path: args.path.map(std::path::PathBuf::from),
        debounce: std::time::Duration::from_millis(args.debounce_ms),
        settle: std::time::Duration::from_millis(args.settle_ms),
        max_retries: args.max_retries,
        initial_scan: !args.no_initial_scan,
        json: args.json,
    };
    syslog_mcp::ai_watch::run(service, options).await
}

// ─── DB Arg → Request conversions (bead 0p8r.9) ─────────────────────────────
//
// DbIntegrityArgs / DbCheckpointArgs were identity maps to their *Request
// counterparts (bead 0p8r.29). Inlined at the call sites. DbVacuumArgs keeps
// `into_request` because `bool → Option<bool>` is non-trivial.

impl DbVacuumArgs {
    /// CLI `force: bool` maps to server `Option<bool>` as
    /// `true → Some(true)`, `false → None` (NOT `Some(false)`). The size
    /// pre-flight on `--full` is bypassed only when the body carries
    /// `Some(true)`. `None` and `Some(false)` are equivalent on the wire and
    /// both leave the pre-flight in force. See [`DbVacuumRequest`] docs and
    /// bead 0p8r.4 eng-review C3.
    pub(super) fn into_request(self) -> DbVacuumRequest {
        DbVacuumRequest {
            full: self.full,
            incremental_pages: self.pages,
            force: if self.force { Some(true) } else { None },
        }
    }
}

// ─── DB Per-command dispatch (bead 0p8r.9) ──────────────────────────────────

pub(super) async fn run_db_status(mode: &CliMode, args: DbStatusArgs) -> Result<()> {
    let response = match mode {
        CliMode::Local(service) => service.db_status().await?,
        CliMode::Http(client) => http_or_cancel(client.db_status()).await?,
    };
    // Coordination phases shell out to docker/systemctl on the host. They
    // make sense in either mode — even with --http, the operator may want
    // to verify that the host's ai-watch unit agrees with the container's
    // /data bind. Keep the opt-in flag mode-agnostic.
    let coordination = if args.check_coord {
        Some(run_coordination_phases())
    } else {
        None
    };
    print_db_status_response(&response, coordination.as_deref(), args.json)
}

pub(super) async fn run_db_integrity(mode: &CliMode, args: DbIntegrityArgs) -> Result<()> {
    let DbIntegrityArgs { quick, json } = args;
    let req = DbIntegrityRequest { quick };
    let response = match mode {
        CliMode::Local(service) => service.db_integrity(quick).await?,
        CliMode::Http(client) => http_or_cancel(client.db_integrity(&req)).await?,
    };
    print_db_integrity_response(&response, json)?;
    if !response.ok {
        bail!("database integrity check failed");
    }
    Ok(())
}

pub(super) async fn run_db_checkpoint(mode: &CliMode, args: DbCheckpointArgs) -> Result<()> {
    let DbCheckpointArgs {
        mode: chk_mode,
        json,
    } = args;
    let req = DbCheckpointRequest {
        mode: chk_mode.clone(),
    };
    let response = match mode {
        CliMode::Local(service) => service.db_checkpoint(chk_mode).await?,
        CliMode::Http(client) => http_or_cancel(client.db_checkpoint(&req)).await?,
    };
    print_db_checkpoint_response(&response, json)?;
    if response.busy != 0 {
        bail!("database WAL checkpoint was busy");
    }
    Ok(())
}

pub(super) async fn run_db_vacuum(mode: &CliMode, args: DbVacuumArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => {
            // Mirror the API's 2GB pre-flight (bead 0p8r.4 / eng-review C3)
            // so a local invocation can't bypass the guard just by skipping
            // --http. Read the LIVE logical size, not a cached snapshot.
            if req.full && req.force != Some(true) {
                let size = service.db_logical_size_bytes().await?;
                if size > crate::api::FULL_VACUUM_SIZE_GUARD_BYTES {
                    let gb = size as f64 / (1024.0 * 1024.0 * 1024.0);
                    bail!(
                        "DB size {gb:.2} GB; full VACUUM would block ingest. \
                         Pass --force to override, or use incremental (--pages N) instead."
                    );
                }
            }
            service.db_vacuum(req.full, req.incremental_pages).await?
        }
        CliMode::Http(client) => http_or_cancel(client.db_vacuum(&req)).await?,
    };
    print_db_vacuum_response(&response, json)
}

pub(super) async fn run_db_backup(mode: &CliMode, args: DbBackupArgs) -> Result<()> {
    let service = match mode {
        CliMode::Http(_) => bail!(
            "db backup currently runs locally; --output writes to a host filesystem path. \
             Omit --http."
        ),
        CliMode::Local(service) => service,
    };
    let response = service.db_backup(args.output.map(PathBuf::from)).await?;
    print_db_backup_response(&response, args.json)
}

// ─── Surface parity (Task 5/6) ──────────────────────────────────────────────

impl SourceIpsArgs {
    pub(super) fn into_request(self) -> ListSourceIpsRequest {
        ListSourceIpsRequest {
            limit: self.limit,
            offset: self.offset,
        }
    }
}

impl TimelineArgs {
    pub(super) fn into_request(self) -> TimelineRequest {
        TimelineRequest {
            bucket: self.bucket,
            group_by: self.group_by,
            from: self.from,
            to: self.to,
            hostname: self.hostname,
            app_name: self.app_name,
            severity_min: self.severity_min,
        }
    }
}

impl PatternsArgs {
    pub(super) fn into_request(self) -> PatternsRequest {
        PatternsRequest {
            from: self.from,
            to: self.to,
            hostname: self.hostname,
            app_name: self.app_name,
            severity_min: self.severity_min,
            scan_limit: self.scan_limit,
            top_n: self.top_n,
        }
    }
}

impl IngestRateArgs {
    pub(super) fn into_request(self) -> IngestRateRequest {
        IngestRateRequest {
            by_host: if self.by_host { Some(true) } else { None },
        }
    }
}

impl SigListArgs {
    pub(super) fn into_request(self) -> UnaddressedErrorsRequest {
        UnaddressedErrorsRequest {
            limit: self.limit,
            include_acknowledged: Some(self.include_acknowledged),
        }
    }
}

impl SigAckArgs {
    pub(super) fn into_request(self) -> AckErrorRequest {
        AckErrorRequest {
            signature_hash: self.signature_hash,
            notes: self.notes,
        }
    }
}

impl SigUnackArgs {
    pub(super) fn into_request(self) -> UnackErrorRequest {
        UnackErrorRequest {
            signature_hash: self.signature_hash,
            reason: self.reason,
        }
    }
}

pub(super) async fn run_source_ips(mode: &CliMode, args: SourceIpsArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.list_source_ips(req).await?,
        CliMode::Http(client) => http_or_cancel(client.source_ips(&req)).await?,
    };
    if json {
        return super::print_json(&response);
    }
    println!("{} source IP(s) (total {}):", response.source_ips.len(), response.total);
    for ip in &response.source_ips {
        println!(
            "  {:<20} logs={} hosts={} last_seen={}",
            ip.source_ip, ip.log_count, ip.host_count, ip.last_seen
        );
    }
    Ok(())
}

pub(super) async fn run_timeline(mode: &CliMode, args: TimelineArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.timeline(req).await?,
        CliMode::Http(client) => http_or_cancel(client.timeline(&req)).await?,
    };
    if json {
        return super::print_json(&response);
    }
    println!("bucket={}{}", response.bucket,
        response.group_by.as_deref().map(|g| format!(" group_by={g}")).unwrap_or_default());
    for pt in &response.points {
        let group = pt.group.as_deref().map(|g| format!(" [{g}]")).unwrap_or_default();
        println!("  {}  {:>8}{}", pt.bucket, pt.count, group);
    }
    Ok(())
}

pub(super) async fn run_patterns(mode: &CliMode, args: PatternsArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.patterns(req).await?,
        CliMode::Http(client) => http_or_cancel(client.patterns(&req)).await?,
    };
    if json {
        return super::print_json(&response);
    }
    println!(
        "{} pattern(s) (scanned {} logs{})",
        response.patterns.len(),
        response.scanned,
        if response.truncated { ", truncated" } else { "" }
    );
    for p in &response.patterns {
        println!("  {:>6}  {}", p.count, p.template);
    }
    Ok(())
}

pub(super) async fn run_ingest_rate(mode: &CliMode, args: IngestRateArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.ingest_rate(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ingest_rate(&req)).await?,
    };
    if json {
        return super::print_json(&response);
    }
    let b = &response.buckets;
    println!(
        "ingest rate (per_sec): 1m={:.2} 5m={:.2} 15m={:.2}  (counts 1m={} 5m={} 15m={}; write_blocked={})",
        b.per_sec_1m, b.per_sec_5m, b.per_sec_15m, b.last_1m, b.last_5m, b.last_15m, response.write_blocked
    );
    if let Some(hosts) = &response.by_host {
        for h in hosts {
            println!(
                "  {:<20} 1m={} 5m={} 15m={}",
                h.hostname, h.last_1m, h.last_5m, h.last_15m
            );
        }
    }
    Ok(())
}

pub(super) async fn run_sig_list(mode: &CliMode, args: SigListArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.unaddressed_errors(req).await?,
        CliMode::Http(client) => http_or_cancel(client.unaddressed_errors(&req)).await?,
    };
    if json {
        return super::print_json(&response);
    }
    if response.signatures.is_empty() {
        println!("No unaddressed error signatures.");
        return Ok(());
    }
    println!("{} signature(s):", response.signatures.len());
    for sig in &response.signatures {
        let acked = if sig.acknowledged_at.is_some() {
            " [acked]"
        } else {
            ""
        };
        let hash_short = sig
            .signature_hash
            .get(..16)
            .unwrap_or(sig.signature_hash.as_str());
        println!(
            "  {:>6}x  {}  {}{}",
            sig.total_count, hash_short, sig.template, acked
        );
        println!(
            "         app={} host={}",
            sig.sample_app_name.as_deref().unwrap_or("-"),
            sig.sample_hostname
        );
    }
    Ok(())
}

pub(super) async fn run_sig_ack(mode: &CliMode, args: SigAckArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.ack_error(req, "cli").await?,
        CliMode::Http(client) => http_or_cancel(client.ack_error(&req)).await?,
    };
    if json {
        return super::print_json(&response);
    }
    println!(
        "acknowledged {} at {} by {}",
        response.signature_hash, response.acknowledged_at, response.actor
    );
    Ok(())
}

pub(super) async fn run_sig_unack(mode: &CliMode, args: SigUnackArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.unack_error(req, "cli").await?,
        CliMode::Http(client) => http_or_cancel(client.unack_error(&req)).await?,
    };
    if json {
        return super::print_json(&response);
    }
    println!(
        "unacknowledged {} at {} by {}",
        response.signature_hash, response.unacked_at, response.actor
    );
    Ok(())
}

pub(super) async fn run_notify_recent(mode: &CliMode, args: NotifyRecentArgs) -> Result<()> {
    let json = args.json;
    let raw_limit = args.limit.unwrap_or(50);
    if !(1..=500).contains(&raw_limit) {
        anyhow::bail!("--limit must be between 1 and 500 (got {raw_limit})");
    }
    let limit = raw_limit;
    match mode {
        CliMode::Local(service) => {
            let firings = service
                .notifications_recent(limit, args.rule_id, args.since)
                .await?;
            if json {
                return super::print_json(&firings);
            }
            if firings.is_empty() {
                println!("No recent notification firings.");
                return Ok(());
            }
            for f in &firings {
                let status = f
                    .status_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "{} rule={} host={} status={}",
                    f.fired_at, f.rule_id, f.hostname, status
                );
            }
        }
        CliMode::Http(client) => {
            let firings = http_or_cancel(client.notifications_recent(limit, args.rule_id, args.since)).await?;
            if json {
                return super::print_json(&firings);
            }
            // Returned as a JSON array of objects matching FiringRow shape.
            let arr = firings
                .as_array()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("unexpected response shape: expected JSON array, got {}", firings))?;
            if arr.is_empty() {
                println!("No recent notification firings.");
                return Ok(());
            }
            for f in &arr {
                let fired_at = f.get("fired_at").and_then(|v| v.as_str()).unwrap_or("-");
                let rule_id = f.get("rule_id").and_then(|v| v.as_str()).unwrap_or("-");
                let hostname = f.get("hostname").and_then(|v| v.as_str()).unwrap_or("-");
                let status = f
                    .get("status_code")
                    .and_then(|v| v.as_i64())
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "{fired_at} rule={rule_id} host={hostname} status={status}"
                );
            }
        }
    }
    Ok(())
}

pub(super) async fn run_notify_test(mode: &CliMode, args: NotifyTestArgs) -> Result<()> {
    let json = args.json;
    match mode {
        CliMode::Http(client) => {
            let result = http_or_cancel(client.notifications_test(args.body)).await?;
            if json {
                return super::print_json(&result);
            }
            println!("{result}");
        }
        CliMode::Local(_) => {
            bail!("notify test requires --http (apprise config lives in the server process)");
        }
    }
    Ok(())
}

impl AiIncidentsArgs {
    pub(super) fn into_request(self) -> AiIncidentRequest {
        AiIncidentRequest {
            project: self.project,
            tool: self.tool,
            from: self.from,
            to: self.to,
            limit: self.limit,
            window_minutes: self.window_minutes,
            terms: self.terms,
        }
    }
}

impl AiInvestigateArgs {
    pub(super) fn into_request(self) -> AiInvestigateRequest {
        AiInvestigateRequest {
            project: self.project,
            tool: self.tool,
            from: self.from,
            to: self.to,
            limit: self.limit,
            window_minutes: self.window_minutes,
            correlation_window_minutes: self.correlation_window_minutes,
            terms: self.terms,
        }
    }
}

pub(super) async fn run_ai_incidents(mode: &CliMode, args: AiIncidentsArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.list_ai_incidents(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_incidents(&req)).await?,
    };
    print_ai_incidents_response(&response, json)
}

pub(super) async fn run_ai_investigate(mode: &CliMode, args: AiInvestigateArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.investigate_ai_incidents(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_investigate(&req)).await?,
    };
    print_ai_investigate_response(&response, json)
}

pub(super) async fn run_ai_assess(mode: &CliMode, args: AiAssessArgs) -> Result<()> {
    let service = match mode {
        CliMode::Http(_) => {
            bail!("ai assess spawns Gemini CLI on the local host; omit --http")
        }
        CliMode::Local(service) => service,
    };
    let req = AiAssessRequest {
        incident_id: args.incident_id,
        model: args.model,
        project: args.project,
        tool: args.tool,
        from: args.from,
        to: args.to,
        window_minutes: args.window_minutes,
        correlation_window_minutes: args.correlation_window_minutes,
        terms: args.terms,
        limit: args.limit,
    };
    let response = service.run_gemini_assess(req).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!("{}", response.assessment);
        eprintln!(
            "\n[assessed incident={} anchors={} bundles={}]",
            response.incident_id,
            response.evidence_summary.total_anchors,
            response.evidence_summary.evidence_bundle_count,
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
