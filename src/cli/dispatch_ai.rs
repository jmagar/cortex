use super::dispatch::http_or_cancel;

use anyhow::{Result, bail};
use cortex::app::{
    AbuseSearchRequest, AiAssessRequest, AiCheckpointsRequest, AiCorrelateRequest,
    AiIncidentRequest, AiInvestigateRequest, AiParseErrorsRequest, AiPruneCheckpointsRequest,
    AskHistoryRequest, IncidentContextRequest, ListAiProjectsRequest, ListAiToolsRequest,
    ProjectContextRequest, SearchSessionsRequest, SimilarIncidentsRequest, UsageBlocksRequest,
};
use std::io::Write;

use super::ai_watch::ai_smoke_watch;
use super::output_ai::{
    ensure_ai_doctor_success, ensure_index_success, print_ai_doctor_response,
    print_ai_parse_errors_response, print_ai_smoke_watch_response, print_ai_watch_status_response,
    print_checkpoints_response, print_index_response, print_prune_checkpoints_response,
};
use super::output_ai_more::{
    AiInvestigatePrintOptions, print_ai_incidents_response,
    print_ai_investigate_response_with_options, print_ask_history_response,
    print_incident_context_response, print_similar_incidents_response,
};
use super::output_logs::{
    UsageBlocksPrintOptions, print_abuse_search_response, print_ai_correlate_response,
    print_ai_projects_response, print_ai_tools_response, print_project_context_response,
    print_search_sessions_response, print_usage_blocks_response_with_options,
};
use super::{
    AiAbuseArgs, AiAddArgs, AiAskHistoryArgs, AiAssessArgs, AiBlocksArgs, AiCheckpointsArgs,
    AiContextArgs, AiCorrelateArgs, AiDoctorArgs, AiErrorsArgs, AiIncidentContextArgs,
    AiIncidentsArgs, AiIndexArgs, AiInvestigateArgs, AiListArgs, AiPruneCheckpointsArgs,
    AiSearchArgs, AiSimilarArgs, AiWatchArgs, CliMode, OutputArgs,
};

// ─── AI Arg → Request conversions (bead 0p8r.8) ─────────────────────────────

impl AiSearchArgs {
    pub(crate) fn into_request(self) -> SearchSessionsRequest {
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
    pub(crate) fn into_request(self) -> AbuseSearchRequest {
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
    pub(crate) fn into_request(self) -> AiCorrelateRequest {
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
    pub(crate) fn into_request(self) -> UsageBlocksRequest {
        UsageBlocksRequest {
            project: self.project,
            tool: self.tool,
            from: self.from,
            to: self.to,
        }
    }
}

impl AiContextArgs {
    pub(crate) fn into_request(self) -> ProjectContextRequest {
        ProjectContextRequest {
            project: self.project,
            tool: self.tool,
            limit: self.limit,
        }
    }
}

impl AiListArgs {
    pub(crate) fn into_tools_request(self) -> ListAiToolsRequest {
        ListAiToolsRequest {
            project: self.project,
            from: self.from,
            to: self.to,
        }
    }

    pub(crate) fn into_projects_request(self) -> ListAiProjectsRequest {
        ListAiProjectsRequest {
            tool: self.tool,
            from: self.from,
            to: self.to,
        }
    }
}

impl AiCheckpointsArgs {
    pub(crate) fn into_request(self) -> AiCheckpointsRequest {
        AiCheckpointsRequest {
            errors_only: self.errors_only,
            missing_only: self.missing_only,
            limit: self.limit,
        }
    }
}

impl AiErrorsArgs {
    pub(crate) fn into_request(self) -> AiParseErrorsRequest {
        AiParseErrorsRequest { limit: self.limit }
    }
}

impl AiPruneCheckpointsArgs {
    pub(crate) fn into_request(self) -> AiPruneCheckpointsRequest {
        AiPruneCheckpointsRequest {
            dry_run: self.dry_run,
            missing_only: self.missing_only,
            limit: self.limit,
        }
    }
}

impl AiSimilarArgs {
    pub(crate) fn into_request(self) -> SimilarIncidentsRequest {
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
    pub(crate) fn into_request(self) -> AskHistoryRequest {
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
    pub(crate) fn into_request(self) -> IncidentContextRequest {
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

pub(crate) async fn run_ai_search(mode: &CliMode, args: AiSearchArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.search_sessions(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_search(&req)).await?,
    };
    print_search_sessions_response(&response, json)
}

pub(crate) async fn run_ai_abuse(mode: &CliMode, args: AiAbuseArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.search_abuse(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_abuse(&req)).await?,
    };
    print_abuse_search_response(&response, json)
}

pub(crate) async fn run_ai_correlate(mode: &CliMode, args: AiCorrelateArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.correlate_ai_logs(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_correlate(&req)).await?,
    };
    print_ai_correlate_response(&response, json)
}

pub(crate) async fn run_ai_blocks(mode: &CliMode, args: AiBlocksArgs) -> Result<()> {
    let json = args.json;
    let detail = args.detail;
    let limit = args.limit;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.usage_blocks(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_blocks(&req)).await?,
    };
    print_usage_blocks_response_with_options(
        &response,
        json,
        UsageBlocksPrintOptions { detail, limit },
    )
}

pub(crate) async fn run_ai_context(mode: &CliMode, args: AiContextArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.project_context(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_context(&req)).await?,
    };
    print_project_context_response(&response, json)
}

pub(crate) async fn run_ai_tools(mode: &CliMode, args: AiListArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_tools_request();
    let response = match mode {
        CliMode::Local(service) => service.list_ai_tools(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_tools(&req)).await?,
    };
    print_ai_tools_response(&response, json)
}

pub(crate) async fn run_ai_projects(mode: &CliMode, args: AiListArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_projects_request();
    let response = match mode {
        CliMode::Local(service) => service.list_ai_projects(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_projects(&req)).await?,
    };
    print_ai_projects_response(&response, json)
}

pub(crate) async fn run_ai_checkpoints(mode: &CliMode, args: AiCheckpointsArgs) -> Result<()> {
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

pub(crate) async fn run_ai_errors(mode: &CliMode, args: AiErrorsArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.list_ai_parse_errors(req.limit).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_parse_errors(&req)).await?,
    };
    print_ai_parse_errors_response(&response, json)
}

pub(crate) async fn run_ai_prune_checkpoints(
    mode: &CliMode,
    args: AiPruneCheckpointsArgs,
) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.prune_ai_checkpoints_checked(req.clone()).await?,
        CliMode::Http(client) => http_or_cancel(client.prune_ai_checkpoints(&req)).await?,
    };
    print_prune_checkpoints_response(&response, json)
}

// ─── LOCAL-only AI commands (6) — error in HTTP mode ────────────────────────

pub(crate) async fn run_ai_index(mode: &CliMode, args: AiIndexArgs) -> Result<()> {
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

pub(crate) async fn run_ai_add(mode: &CliMode, args: AiAddArgs) -> Result<()> {
    let service = match mode {
        CliMode::Http(_) => bail!("ai add reads a host file path; omit --http"),
        CliMode::Local(service) => service,
    };
    let response = service.add_ai_file(args.file, args.force).await?;
    print_index_response(&response, args.json)?;
    ensure_index_success(&response)
}

pub(crate) async fn run_ai_doctor(mode: &CliMode, args: AiDoctorArgs) -> Result<()> {
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

pub(crate) async fn run_ai_smoke_watch(mode: &CliMode, args: OutputArgs) -> Result<()> {
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

pub(crate) async fn run_ai_watch_status(mode: &CliMode, args: OutputArgs) -> Result<()> {
    if matches!(mode, CliMode::Http(_)) {
        bail!("ai watch-status shells out to systemctl on host; omit --http");
    }
    let CliMode::Local(service) = mode else {
        unreachable!("http mode returned above");
    };
    let response = service.ai_watch_status().await?;
    print_ai_watch_status_response(&response, args.json)
}

pub(crate) async fn run_ai_watch(mode: &CliMode, args: AiWatchArgs) -> Result<()> {
    let service = match mode {
        CliMode::Http(_) => bail!("ai watch is a long-running daemon; omit --http"),
        CliMode::Local(service) => service.clone(),
    };
    let options = cortex::ai_watch::WatchOptions {
        path: args.path.map(std::path::PathBuf::from),
        debounce: std::time::Duration::from_millis(args.debounce_ms),
        settle: std::time::Duration::from_millis(args.settle_ms),
        max_retries: args.max_retries,
        initial_scan: !args.no_initial_scan,
        json: args.json,
    };
    cortex::ai_watch::run(service, options).await
}

// ─── RAG v1 dispatch (LOCAL-only) ────────────────────────────────────────────

pub(crate) async fn run_ai_similar_incidents(mode: &CliMode, args: AiSimilarArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Http(client) => http_or_cancel(client.similar_incidents(&req)).await?,
        CliMode::Local(service) => service.similar_incidents(req).await?,
    };
    print_similar_incidents_response(&response, json)
}

pub(crate) async fn run_ai_ask_history(mode: &CliMode, args: AiAskHistoryArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Http(client) => http_or_cancel(client.ask_history(&req)).await?,
        CliMode::Local(service) => service.ask_history(req).await?,
    };
    print_ask_history_response(&response, json)
}

pub(crate) async fn run_ai_incident_context(
    mode: &CliMode,
    args: AiIncidentContextArgs,
) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Http(client) => http_or_cancel(client.incident_context(&req)).await?,
        CliMode::Local(service) => service.incident_context(req).await?,
    };
    print_incident_context_response(&response, json)
}

impl AiIncidentsArgs {
    pub(crate) fn into_request(self) -> AiIncidentRequest {
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
    pub(crate) fn into_request(self) -> AiInvestigateRequest {
        AiInvestigateRequest {
            incident_id: None,
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

pub(crate) async fn run_ai_incidents(mode: &CliMode, args: AiIncidentsArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.list_ai_incidents(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_incidents(&req)).await?,
    };
    print_ai_incidents_response(&response, json)
}

pub(crate) async fn run_ai_investigate(mode: &CliMode, args: AiInvestigateArgs) -> Result<()> {
    let json = args.json;
    let print_options = AiInvestigatePrintOptions {
        detail: args.detail,
        include_transcript: args.include_transcript,
        max_bytes: args.max_bytes.unwrap_or(240),
    };
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.investigate_ai_incidents(req).await?,
        CliMode::Http(client) => http_or_cancel(client.ai_investigate(&req)).await?,
    };
    print_ai_investigate_response_with_options(&response, json, print_options)
}

pub(crate) async fn run_ai_assess(mode: &CliMode, args: AiAssessArgs) -> Result<()> {
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
    if args.json {
        let response = service.run_gemini_assess(req).await?;
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        let mut streamed = false;
        let response = service
            .run_gemini_assess_with_delta(req, |delta| {
                streamed = true;
                print!("{delta}");
                std::io::stdout().flush()?;
                Ok(())
            })
            .await?;
        if !streamed {
            println!("{}", response.assessment);
        } else if !response.assessment.ends_with('\n') {
            println!();
        }
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
#[path = "dispatch_ai_tests.rs"]
mod tests;
