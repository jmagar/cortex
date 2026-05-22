#![allow(unused_imports)]
use super::dispatch::http_or_cancel;
// Per-arm dispatch for query commands (bead 0p8r.7).
//
// For each of the 7 query commands (search/tail/errors/hosts/correlate/
// stats/sessions) we expose:
//
// - A `Cli*Args::into_request()` conversion that constructs the `*Request`
//   struct shared by the service layer and the REST surface. Sharing the
//   constructor between the Local and HTTP arms is what guards against
//   per-arm field drift (eng-review #A37). The unit tests below pin the
//   shape via `format!("{req:?}")` snapshots.
// - A `run_X(mode, args)` free `async fn` that branches on [`CliMode`] and
//   either calls the local [`SyslogService`] directly or routes through
//   [`HttpClient`]. The HTTP arm is wrapped in [`http_or_cancel`] so a
//   SIGINT during a long-running request bails with `"interrupted"`
//   (eng-review #A29). The Local arm is sync SQL — no cancellation needed.
//
// `--json` printing reuses the existing `print_*_response` formatters from
// `super::*`, so output is byte-identical between modes: the HTTP path
// proxies the same service the Local path would invoke server-side.

use anyhow::{bail, Result};
use std::future::Future;
use std::path::PathBuf;
use syslog_mcp::app::{
    AbuseSearchRequest, AckErrorRequest, AiAssessRequest, AiCheckpointsRequest, AiCorrelateRequest,
    AiIncidentRequest, AiInvestigateRequest, AiParseErrorsRequest, AiPruneCheckpointsRequest,
    AskHistoryRequest, CorrelateEventsRequest, DbCheckpointRequest, DbIntegrityRequest,
    DbVacuumRequest, GetErrorsRequest, IncidentContextRequest, IncidentRequest, IngestRateRequest,
    ListAiProjectsRequest, ListAiToolsRequest, ListSessionsRequest, ListSourceIpsRequest,
    PatternsRequest, ProjectContextRequest, SearchLogsRequest, SearchSessionsRequest,
    SimilarIncidentsRequest, TailLogsRequest, TimelineRequest, UnackErrorRequest,
    UnaddressedErrorsRequest, UsageBlocksRequest,
};

use super::{
    ai_smoke_watch, ai_watch_status, ensure_ai_doctor_success, ensure_index_success,
    print_abuse_search_response, print_ai_correlate_response, print_ai_doctor_response,
    print_ai_incidents_response, print_ai_investigate_response, print_ai_parse_errors_response,
    print_ai_projects_response, print_ai_smoke_watch_response, print_ai_tools_response,
    print_ai_watch_status_response, print_ask_history_response, print_checkpoints_response,
    print_correlate_response, print_db_backup_response, print_db_checkpoint_response,
    print_db_integrity_response, print_db_status_response, print_db_vacuum_response,
    print_errors_response, print_hosts_response, print_incident_context_response,
    print_incident_response, print_index_response, print_project_context_response,
    print_prune_checkpoints_response, print_search_response, print_search_sessions_response,
    print_sessions_response, print_similar_incidents_response, print_stats_response,
    print_usage_blocks_response, run_coordination_phases, AiAbuseArgs, AiAddArgs, AiAskHistoryArgs,
    AiAssessArgs, AiBlocksArgs, AiCheckpointsArgs, AiContextArgs, AiCorrelateArgs, AiDoctorArgs,
    AiErrorsArgs, AiIncidentContextArgs, AiIncidentsArgs, AiIndexArgs, AiInvestigateArgs,
    AiListArgs, AiPruneCheckpointsArgs, AiSearchArgs, AiSimilarArgs, AiWatchArgs, CliMode,
    CorrelateArgs, DbBackupArgs, DbCheckpointArgs, DbIntegrityArgs, DbStatusArgs, DbVacuumArgs,
    IncidentArgs, IngestRateArgs, NotifyRecentArgs, NotifyTestArgs, OutputArgs, PatternsArgs,
    SearchArgs, SessionsArgs, SigAckArgs, SigListArgs, SigUnackArgs, SourceIpsArgs, TailArgs,
    TimeRangeArgs, TimelineArgs,
};

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
    pub(crate) fn into_request(self) -> DbVacuumRequest {
        DbVacuumRequest {
            full: self.full,
            incremental_pages: self.pages,
            force: if self.force { Some(true) } else { None },
        }
    }
}

// ─── DB Per-command dispatch (bead 0p8r.9) ──────────────────────────────────

pub(crate) async fn run_db_status(mode: &CliMode, args: DbStatusArgs) -> Result<()> {
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

pub(crate) async fn run_db_integrity(mode: &CliMode, args: DbIntegrityArgs) -> Result<()> {
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

pub(crate) async fn run_db_checkpoint(mode: &CliMode, args: DbCheckpointArgs) -> Result<()> {
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

pub(crate) async fn run_db_vacuum(mode: &CliMode, args: DbVacuumArgs) -> Result<()> {
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

pub(crate) async fn run_db_backup(mode: &CliMode, args: DbBackupArgs) -> Result<()> {
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

#[cfg(test)]
#[path = "dispatch_db_tests.rs"]
mod tests;
