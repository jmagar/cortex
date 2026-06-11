//! Handler implementations for the single action-dispatched `cortex` MCP tool
//! (log intelligence core).
//!
//! The authoritative action registry is `ACTION_SPECS` in `actions.rs`; this
//! module supplies the executable branch for each `ActionHandler`. Handlers
//! parse the tool arguments into typed `src/app` request models and delegate
//! to `CortexService` — business policy (limits, validation, correlation
//! rules) lives in the service layer so MCP, REST, and CLI stay consistent.
//!
//! Invariants: scope gating (`cortex:read` / `cortex:admin`) happens before
//! dispatch in `rmcp_server.rs`; unknown actions are denied fail-closed.
//! Handlers return typed service errors that the server maps to MCP error
//! classes (invalid_params / retryable / not_found / conflict / internal).

use lab_auth::AuthContext;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::app::{
    AbuseSearchRequest, AckErrorRequest, AiCorrelateRequest, AiIncidentRequest,
    AiInvestigateRequest, AnomaliesRequest, AskHistoryRequest, ClockSkewRequest, CompareRequest,
    ContextRequest, CorrelateEventsRequest, CorrelateStateRequest, FilterLogsRequest,
    FleetStateRequest, GetErrorsRequest, GetLogRequest, GraphAroundRequest,
    GraphEntityLookupRequest, GraphEvidenceLookupRequest, GraphExplainRequest, HomelabMapRequest,
    HostStateRequest, IncidentContextRequest, IngestRateRequest, ListAiProjectsRequest,
    ListAiToolsRequest, ListAppsRequest, ListSessionsRequest, ListSourceIpsRequest,
    NotificationsRecentRequest, PatternsRequest, ProjectContextRequest, RequestActor,
    SearchLogsRequest, SearchSessionsRequest, SilentHostsRequest, SimilarIncidentsRequest,
    TailLogsRequest, TimelineRequest, UnackErrorRequest, UnaddressedErrorsRequest,
    UsageBlocksRequest,
};

use super::AppState;
use super::actions;

/// Execute a tool by name
pub(super) async fn execute_tool(
    state: &AppState,
    name: &str,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    match name {
        "cortex" => tool_cortex(state, args, auth).await,
        _ => Err(anyhow::anyhow!("Unknown tool: {name}")),
    }
}

async fn tool_cortex(
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    let action = string_arg(&args, "action")
        .ok_or_else(|| invalid_input("action is required".to_string()))?;
    let Some(handler) = actions::handler_for(&action) else {
        return Err(invalid_input(format!(
            "unknown cortex action: {action}; expected one of {}",
            actions::action_names().join(", ")
        )));
    };
    dispatch_cortex_action(handler, state, args, auth).await
}

async fn dispatch_cortex_action(
    handler: actions::ActionHandler,
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    use actions::ActionHandler as H;

    match handler {
        H::SearchLogs => tool_search_logs(state, args).await,
        H::FilterLogs => tool_filter_logs(state, args).await,
        H::TailLogs => tool_tail_logs(state, args).await,
        H::GetErrors => tool_get_errors(state, args).await,
        H::ListHosts => tool_list_hosts(state, args).await,
        H::HomelabMap => tool_homelab_map(state, args).await,
        H::HostState => tool_host_state(state, args).await,
        H::FleetState => tool_fleet_state(state, args).await,
        H::CorrelateEvents => tool_correlate_events(state, args).await,
        H::CorrelateState => tool_correlate_state(state, args).await,
        H::GetStats => tool_get_stats(state, args).await,
        H::GetStatus => tool_get_status(state, args).await,
        H::ListApps => tool_list_apps(state, args).await,
        H::ListSessions => tool_list_sessions(state, args).await,
        H::SearchSessions => tool_search_sessions(state, args).await,
        H::SearchAbuse => tool_search_abuse(state, args).await,
        H::AbuseIncidents => tool_abuse_incidents(state, args).await,
        H::AbuseInvestigate => tool_abuse_investigate(state, args).await,
        H::AiCorrelate => tool_ai_correlate(state, args).await,
        H::UsageBlocks => tool_usage_blocks(state, args).await,
        H::ProjectContext => tool_project_context(state, args).await,
        H::ListAiTools => tool_list_ai_tools(state, args).await,
        H::ListAiProjects => tool_list_ai_projects(state, args).await,
        H::ListSourceIps => tool_list_source_ips(state, args).await,
        H::Timeline => tool_timeline(state, args).await,
        H::Patterns => tool_patterns(state, args).await,
        H::Context => tool_context(state, args).await,
        H::GetLog => tool_get_log(state, args).await,
        H::IngestRate => tool_ingest_rate(state, args).await,
        H::SilentHosts => tool_silent_hosts(state, args).await,
        H::ClockSkew => tool_clock_skew(state, args).await,
        H::Anomalies => tool_anomalies(state, args).await,
        H::Compare => tool_compare(state, args).await,
        H::ComposeStatus => tool_compose_status(args).await,
        H::ComposeDoctor => tool_compose_doctor(args).await,
        H::UnaddressedErrors => tool_unaddressed_errors(state, args).await,
        H::AckError => tool_ack_error(state, args, auth).await,
        H::UnackError => tool_unack_error(state, args, auth).await,
        H::NotificationsRecent => tool_notifications_recent(state, args).await,
        H::FileTails => tool_file_tails(state, args).await,
        H::NotificationsTest => tool_notifications_test(state, args, auth).await,
        H::SimilarIncidents => tool_similar_incidents(state, args).await,
        H::AskHistory => tool_ask_history(state, args).await,
        H::IncidentContext => tool_incident_context(state, args).await,
        H::Graph => tool_graph(state, args).await,
        H::Help => tool_cortex_help().await,
    }
}

async fn tool_search_logs(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: SearchLogsRequest = action_payload(args, "search")?;
    let response = state.service.search_logs(req).await?;
    tracing::debug!(result_count = response.count, "search_logs completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_filter_logs(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: FilterLogsRequest = action_payload(args, "filter")?;
    let response = state.service.filter_logs(req).await?;
    tracing::debug!(result_count = response.count, "filter_logs completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_homelab_map(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: HomelabMapRequest = action_payload(args, "map")?;
    let response = state.service.homelab_map(req).await?;
    tracing::debug!(node_count = response.nodes.len(), "homelab_map completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_tail_logs(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: TailLogsRequest = action_payload(args, "tail")?;
    let response = state.service.tail_logs(req).await?;
    tracing::debug!(result_count = response.count, "tail_logs completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_get_errors(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: GetErrorsRequest = action_payload(args, "errors")?;
    let response = state.service.get_errors(req).await?;
    tracing::debug!(
        summary_rows = response.summary.len(),
        "get_errors completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_list_apps(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: ListAppsRequest = action_payload(args, "apps")?;
    let response = state.service.list_apps(req).await?;
    tracing::debug!(
        app_count = response.apps.len(),
        total = response.total,
        "list_apps completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_host_state(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: HostStateRequest = action_payload(args, "host_state")?;
    Ok(serde_json::to_value(state.service.host_state(req).await?)?)
}

async fn tool_fleet_state(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: FleetStateRequest = action_payload(args, "fleet_state")?;
    Ok(serde_json::to_value(state.service.fleet_state(req).await?)?)
}
async fn tool_correlate_state(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: CorrelateStateRequest = action_payload(args, "correlate_state")?;
    Ok(serde_json::to_value(
        state.service.correlate_state(req).await?,
    )?)
}

async fn tool_list_sessions(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: ListSessionsRequest = action_payload(args, "sessions")?;
    let response = state.service.list_sessions(req).await?;
    tracing::debug!(session_count = response.count, "list_sessions completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_search_sessions(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: SearchSessionsRequest = action_payload(args, "search_sessions")?;
    let response = state.service.search_sessions(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_search_abuse(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AbuseSearchRequest = action_payload(args, "abuse")?;
    let response = state.service.search_abuse(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_abuse_incidents(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AiIncidentRequest = action_payload(args, "abuse_incidents")?;
    let response = state.service.list_ai_incidents(req).await?;
    tracing::debug!(
        incident_count = response.incidents.len(),
        total = response.total_incidents,
        "abuse_incidents completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_abuse_investigate(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AiInvestigateRequest = action_payload(args, "abuse_investigate")?;
    let response = state.service.investigate_ai_incidents(req).await?;
    tracing::debug!(
        evidence_count = response.evidence.len(),
        total_incidents = response.total_incidents,
        "abuse_investigate completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_ai_correlate(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AiCorrelateRequest = action_payload(args, "ai_correlate")?;
    let response = state.service.correlate_ai_logs(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_usage_blocks(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: UsageBlocksRequest = action_payload(args, "usage_blocks")?;
    let response = state.service.usage_blocks(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_project_context(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: ProjectContextRequest = action_payload(args, "project_context")?;
    let response = state.service.project_context(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_list_ai_tools(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: ListAiToolsRequest = action_payload(args, "list_ai_tools")?;
    let response = state.service.list_ai_tools(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_list_ai_projects(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: ListAiProjectsRequest = action_payload(args, "list_ai_projects")?;
    let response = state.service.list_ai_projects(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_list_source_ips(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: ListSourceIpsRequest = action_payload(args, "source_ips")?;
    let response = state.service.list_source_ips(req).await?;
    tracing::debug!(
        source_ip_count = response.source_ips.len(),
        total = response.total,
        "list_source_ips completed"
    );
    Ok(serde_json::to_value(response)?)
}

fn reject_compose_target_overrides(args: &Value) -> anyhow::Result<()> {
    for key in [
        "container",
        "container_name",
        "project_dir",
        "compose_file",
        "project_name",
        "service",
    ] {
        if args.get(key).is_some() {
            return Err(invalid_input(format!(
                "compose MCP actions do not accept target override: {key}"
            )));
        }
    }
    Ok(())
}

async fn tool_compose_status(args: Value) -> anyhow::Result<Value> {
    reject_compose_target_overrides(&args)?;
    let status = crate::app::run_compose_status().await?;
    Ok(serde_json::to_value(crate::compose::mcp_projection(
        &status,
    ))?)
}

async fn tool_compose_doctor(args: Value) -> anyhow::Result<Value> {
    reject_compose_target_overrides(&args)?;
    let status = crate::app::run_compose_status().await?;
    crate::compose::ensure_doctor_ready(&status)?;
    Ok(serde_json::to_value(crate::compose::mcp_projection(
        &status,
    ))?)
}

async fn tool_timeline(state: &AppState, args: Value) -> anyhow::Result<Value> {
    // Default lookback is centralized in `CortexService::timeline` (bead dyqw):
    // it applies a bucket-sized window only when neither `from` nor `to` is set,
    // preventing full table scans without recreating the logic per transport.
    let req: TimelineRequest = action_payload(args, "timeline")?;
    let response = state.service.timeline(req).await?;
    tracing::debug!(point_count = response.points.len(), "timeline completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_patterns(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: PatternsRequest = action_payload(args, "patterns")?;
    let response = state.service.patterns(req).await?;
    tracing::debug!(
        pattern_count = response.patterns.len(),
        scanned = response.scanned,
        truncated = response.truncated,
        "patterns completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_context(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: ContextRequest = action_payload(args, "context")?;
    let response = state.service.context(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_get_log(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: GetLogRequest = action_payload(args, "get")?;
    let response = state.service.get_log(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_ingest_rate(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: IngestRateRequest = action_payload(args, "ingest_rate")?;
    let response = state.service.ingest_rate(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_silent_hosts(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: SilentHostsRequest = action_payload(args, "silent_hosts")?;
    let response = state.service.silent_hosts(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_clock_skew(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: ClockSkewRequest = action_payload(args, "clock_skew")?;
    let response = state.service.clock_skew(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_anomalies(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AnomaliesRequest = action_payload(args, "anomalies")?;
    let response = state.service.anomalies(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_compare(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: CompareRequest = action_payload(args, "compare")?;
    let response = state.service.compare(req).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_list_hosts(state: &AppState, _args: Value) -> anyhow::Result<Value> {
    let response = state.service.list_hosts().await?;
    tracing::debug!(host_count = response.hosts.len(), "list_hosts completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_correlate_events(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: CorrelateEventsRequest = action_payload(args, "correlate")?;
    let response = state.service.correlate_events(req).await?;
    Ok(serde_json::to_value(response)?)
}

pub(super) async fn tool_get_stats(state: &AppState, _args: Value) -> anyhow::Result<Value> {
    let stats = state.service.get_stats().await?;
    let mut value = serde_json::to_value(&stats)?;
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "runtime_observability".into(),
            serde_json::to_value(state.observability.snapshot())?,
        );
        object.insert(
            "otlp".into(),
            json!({
                "logs_received": state.otlp_counters.logs_received.load(std::sync::atomic::Ordering::Relaxed),
                "decode_errors": state.otlp_counters.decode_errors.load(std::sync::atomic::Ordering::Relaxed),
            }),
        );
    }
    tracing::debug!(
        total_logs = stats.total_logs,
        total_hosts = stats.total_hosts,
        logical_db_size_mb = %stats.logical_db_size_mb,
        physical_db_size_mb = %stats.physical_db_size_mb,
        write_blocked = stats.write_blocked,
        phantom_fts_rows = stats.phantom_fts_rows,
        "get_stats completed"
    );
    Ok(value)
}

pub(super) async fn tool_get_status(state: &AppState, _args: Value) -> anyhow::Result<Value> {
    let db_ok = state.service.health_check().await.is_ok();
    let db_maintenance = state.service.db_status().await.ok();
    Ok(json!({
        "status": if db_ok { "ok" } else { "error" },
        "db_ok": db_ok,
        "db_maintenance": db_maintenance,
        "runtime_observability": state.observability.snapshot(),
        "otlp": {
            "logs_received": state.otlp_counters.logs_received.load(std::sync::atomic::Ordering::Relaxed),
            "decode_errors": state.otlp_counters.decode_errors.load(std::sync::atomic::Ordering::Relaxed),
        }
    }))
}

fn string_arg(args: &Value, name: &str) -> Option<String> {
    args.get(name).and_then(|v| v.as_str()).map(String::from)
}

/// Return a stable actor identifier for mutating/admin actions.
///
/// Mounted MCP requests carry caller identity in `AuthContext`. Prefer the
/// verified email when available, then the subject. Loopback mode has no
/// per-request credential, so it falls back to the local trust-boundary actor.
fn extract_actor(state: &AppState, auth: Option<&AuthContext>) -> RequestActor {
    if let Some(auth) = auth {
        return RequestActor::mcp_identity(
            (!auth.sub.is_empty()).then(|| auth.sub.clone()),
            auth.email
                .as_deref()
                .filter(|email| !email.is_empty())
                .map(str::to_string),
        );
    }

    match &state.auth_policy {
        super::AuthPolicy::LoopbackDev => RequestActor::mcp_loopback(),
        super::AuthPolicy::TrustedGatewayUnscoped => "mcp:trusted-gateway".to_string().into(),
        super::AuthPolicy::Mounted {
            auth_state: Some(_),
        } => RequestActor::mcp_oauth(),
        super::AuthPolicy::Mounted { auth_state: None } => RequestActor::mcp_bearer(),
    }
}

/// Build a caller-input error that the MCP error classifier maps to
/// `invalid_params` (full-review AH1). All argument-shape failures in this
/// module MUST go through this so classification is type-driven, not
/// string-matched.
fn invalid_input(message: String) -> anyhow::Error {
    anyhow::Error::from(crate::app::ServiceError::InvalidInput(message))
}

fn action_payload<T: DeserializeOwned>(args: Value, action: &str) -> anyhow::Result<T> {
    let mut object = args
        .as_object()
        .cloned()
        .ok_or_else(|| invalid_input("tool arguments must be a JSON object".to_string()))?;
    object.remove("action");
    serde_json::from_value(Value::Object(object))
        .map_err(|err| invalid_input(format!("invalid {action} arguments: {err}")))
}

// ---------------------------------------------------------------------------
// Error detection actions

async fn tool_unaddressed_errors(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: UnaddressedErrorsRequest = action_payload(args, "unaddressed_errors")?;
    let resp = state.service.unaddressed_errors(req).await?;
    Ok(serde_json::to_value(resp)?)
}

async fn tool_ack_error(
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    let req: AckErrorRequest = action_payload(args, "ack_error")?;
    let actor = extract_actor(state, auth);
    let resp = state.service.ack_error(req, actor).await?;
    Ok(serde_json::to_value(resp)?)
}

async fn tool_unack_error(
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    let req: UnackErrorRequest = action_payload(args, "unack_error")?;
    let actor = extract_actor(state, auth);
    let resp = state.service.unack_error(req, actor).await?;
    Ok(serde_json::to_value(resp)?)
}

async fn tool_notifications_recent(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: NotificationsRecentRequest = action_payload(args, "notifications_recent")?;
    let firings = state.service.notifications_recent_checked(req).await?;
    Ok(serde_json::to_value(firings)?)
}

async fn tool_file_tails(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: crate::app::FileTailRequest = action_payload(args, "file_tails")?;
    let resp = state.service.file_tails(req).await?;
    Ok(serde_json::to_value(resp)?)
}

async fn tool_notifications_test(
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    let body =
        string_arg(&args, "body").unwrap_or_else(|| "Test notification from cortex".to_string());
    // Actor is derived from request auth context, not caller-supplied args.
    let actor = extract_actor(state, auth);
    let result = state
        .service
        .notifications_test_checked(body, actor, &state.notifications_config)
        .await?;
    Ok(serde_json::json!({ "result": result }))
}

struct AdminActionHelp {
    action: &'static str,
    description: &'static str,
    parameters: &'static [&'static str],
}

const ADMIN_ACTION_HELP: &[AdminActionHelp] = &[
    AdminActionHelp {
        action: "ack_error",
        description: "Acknowledge an error signature so it is suppressed from future `unaddressed_errors`\nresults. Writes an audit event and updates the acknowledgement projection. Use\n`unack_error` to revoke.",
        parameters: &[
            "`signature_hash` (string, **required**) — the SHA-256 hash from `unaddressed_errors`",
            "`notes` (string, optional) — acknowledgement notes (max 4096 chars)",
        ],
    },
    AdminActionHelp {
        action: "unack_error",
        description: "Revoke an existing acknowledgement on an error signature so it reappears in\n`unaddressed_errors`. Writes an unack audit event; does NOT delete the ack history.",
        parameters: &[
            "`signature_hash` (string, **required**) — the SHA-256 hash of the signature",
            "`reason` (string, optional) — reason for removing the acknowledgement (max 4096 chars)",
        ],
    },
    AdminActionHelp {
        action: "file_tails",
        description: "Manage Cortex-owned file-tail ingest sources. Sources are stored in the local file-tail registry and reconciled by the runtime supervisor.",
        parameters: &[
            "`op` (string, **required**) — list, add, remove, enable, disable, or status",
            "`id` (string, required for add/remove/enable/disable) — stable file-tail source id",
            "`path` (string, required for add) — local log file path",
            "`tag` (string, required for add) — app/tag stored on ingested rows",
            "`hostname`, `facility`, `severity`, `start_at_end` (optional) — row envelope defaults",
        ],
    },
    AdminActionHelp {
        action: "notifications_test",
        description: "Send a test notification via the server-configured Apprise URLs. Rate-limited to 10 per minute per actor.\nCaller-supplied Apprise URLs are ignored for security; the server uses its own configured URLs.",
        parameters: &["`body` (string, optional) — notification body text (default: test message)"],
    },
];

fn admin_action_help() -> String {
    let mut help = String::new();
    for action in ADMIN_ACTION_HELP {
        help.push_str("---\n\n");
        help.push_str("## cortex ");
        help.push_str(action.action);
        help.push('\n');
        help.push_str(action.description);
        help.push_str("\n\n**Parameters:**\n");
        for parameter in action.parameters {
            help.push_str("- ");
            help.push_str(parameter);
            help.push('\n');
        }
        help.push('\n');
    }
    help
}

async fn tool_cortex_help() -> anyhow::Result<Value> {
    let mut cheap = Vec::new();
    let mut moderate = Vec::new();
    let mut expensive = Vec::new();
    let mut write = Vec::new();
    for spec in actions::ACTION_SPECS {
        match spec.cost {
            actions::Cost::Cheap => cheap.push(spec.name),
            actions::Cost::Moderate => moderate.push(spec.name),
            actions::Cost::Expensive => expensive.push(spec.name),
            actions::Cost::Write => write.push(spec.name),
        }
    }
    let cost_guide = format!(
        r#"## Agent Planning Cost Metadata

Use action cost metadata to keep first-class agents token-efficient:
- `cheap`: {}.
- `moderate`: {}.
- `expensive`: {}.
- `write`: {}.

Recommended flow: start with cheap bounded calls, use moderate actions after
the scope is narrowed, and reserve expensive actions for a specific unanswered
question. Write actions require admin scope and must never be used for read-only
diagnosis.

"#,
        cheap.join(", "),
        moderate.join(", "),
        expensive.join(", "),
        write.join(", ")
    );
    let help = r#"# cortex Tool Reference

The MCP server exposes one tool, `cortex`. Set the required `action` argument
to select the operation.

## cortex search
Full-text search across all syslog messages with optional filters.
Uses SQLite FTS5 with porter stemming. Supports FTS5 query syntax: AND, OR, NOT,
phrase matching with quotes, prefix matching with *.

**Parameters:**
- `query` (string) — FTS5 search query, e.g. `'kernel panic'`, `'OOM AND killer'`, `'"connection refused"'`, `'error*'`
- `hostname` (string, optional) — filter by hostname (exact match); use `cortex hosts` to enumerate
- `source_ip` (string, optional) — filter by exact source identifier. Syslog uses verified `IP:port`; OTLP uses verified peer IP; Docker stream rows use `docker://host/container/stream`; Docker lifecycle rows use `docker-event://host/container/action`.
- `severity` (string, optional) — one of: `emerg`, `alert`, `crit`, `err`, `warning`, `notice`, `info`, `debug`
- `app_name` (string, optional) — filter by application name, e.g. `sshd`, `dockerd`, `kernel`
- `facility` (string, optional) — filter by syslog facility name (e.g. `kern`, `auth`, `daemon`)
- `exclude_facility` (string, optional) — exclude a syslog facility name (e.g. `kern` to suppress kernel noise)
- `process_id` (string, optional) — filter by process_id (exact match)
- `from` (string, optional) — start of time range (ISO 8601 / RFC3339, e.g. `2025-01-15T00:00:00Z`)
- `to` (string, optional) — end of time range (ISO 8601)
- `received_from` (string, optional) — restrict to entries received after this time (server-side ingestion clock, ISO 8601)
- `received_to` (string, optional) — restrict to entries received before this time (server-side ingestion clock, ISO 8601)
- `limit` (integer, optional) — max results (default 100, max 1000)

---

## cortex filter
Filter log rows by structured fields only. This action never accepts `query`;
use `search` for message-body FTS5 queries.

**Parameters:**
- `hostname` (string, optional) — filter by hostname (exact match)
- `source_ip` (string, optional) — filter by exact source identifier
- `severity` (string, optional) — one of: `emerg`, `alert`, `crit`, `err`, `warning`, `notice`, `info`, `debug`
- `app_name` (string, optional) — filter by application/container name
- `facility` / `exclude_facility` (string, optional) — include or exclude syslog facility
- `process_id` (string, optional) — filter by process_id
- `from` / `to` (string, optional) — event timestamp window
- `received_from` / `received_to` (string, optional) — ingest timestamp window
- `source_kind` (string, optional) — `docker-stream`, `docker-event`, `agent-command`, `shell-history`, `transcript`, `claude`, `codex`, or `gemini`
- `tool`, `project`, `session_id` (string, optional) — AI transcript filters
- `docker_host`, `container`, `stream`, `event_action` (string, optional) — Docker refiners
- `limit` (integer, optional) — max results (default 100, max 1000)

---

## cortex tail
Get the N most recent log entries, optionally filtered by host, application, and/or severity floor.
Equivalent to `tail -f` across all hosts.

**Parameters:**
- `hostname` (string, optional) — filter to a specific host
- `source_ip` (string, optional) — filter by exact source identifier. Syslog uses verified `IP:port`; OTLP uses verified peer IP; Docker stream rows use `docker://host/container/stream`; Docker lifecycle rows use `docker-event://host/container/action`.
- `app_name` (string, optional) — filter to a specific application
- `severity_min` (string, optional) — only return entries at or above this severity (e.g. `warning` returns warning + worse)
- `n` (integer, optional) — number of recent entries (default 50, max 500)

---

## cortex errors
Get a summary of errors and warnings across all hosts in a time window.
Groups by hostname and severity level (and optionally app_name), showing counts.

**Parameters:**
- `from` (string, optional) — start of time range (ISO 8601); defaults to all time
- `to` (string, optional) — end of time range (ISO 8601); defaults to now
- `group_by` (string, optional) — secondary grouping key. Currently `app_name` is supported; default groups only by hostname+severity.
- `limit` (integer, optional) — cap summary rows returned (max 100)

---

## cortex hosts
List all hosts that have sent syslog messages, with first/last seen timestamps and total log counts.

**Parameters:** none

---

## cortex map
Return a bounded homelab infrastructure snapshot from Cortex's current database.
The map includes known host nodes, verified source identities, top observed
applications per host, latest heartbeat status when available, and the external
inventory sources that complement Cortex's DB-backed view.
When `mode` is `host_services`, `domain_routes`, `service_dependencies`, or `findings`, the
response also includes `graph_answer` with answer status, topology rows,
candidates, safe evidence samples, map follow-up queries, graph proof queries,
and for `findings` a bounded `findings` array with topology risk/hygiene
findings.

**Parameters:**
- `mode` (string, optional) — `snapshot` (default), `host_services`, `domain_routes`, `service_dependencies`, or `findings`
- `host` (string, optional) — target host for `host_services`; also used with bare `service` names for `service_dependencies`
- `domain` (string, optional) — target domain for `domain_routes`
- `service` (string, optional) — target service for `service_dependencies`, either `host:name` or a bare name with `host`
- `host_limit` (integer, optional) — maximum host nodes to return (default 100, max 500)
- `section_limit` (integer, optional) — maximum rows per inventory section (default 100, max 250)
- `include_sections` (array, optional) — section names to include; defaults to all sections
- `answer_limit` (integer, optional) — graph relationship cap for graph-backed modes (default 100, max 500)
- `evidence_sample_limit` (integer, optional) — evidence samples per relationship for graph-backed modes (default 3, max 5)
- `payload_budget` (integer, optional) — approximate graph payload budget in bytes (default 32768, max 65536)
- `finding_limit` (integer, optional) — findings returned by `mode=findings` (default 25, max 100)
- `evidence_per_finding` (integer, optional) — safe evidence samples per finding (default 2, max 5)
- `finding_types` (array, optional) — subset of `potential_public_route`, `risky_mounts`, `collector_health`
- `per_host_limit` (integer, optional) — deprecated map v1 compatibility option; ignored by map v2

---

## cortex host_state
Return the latest bounded heartbeat state for one host.

**Parameters:**
- `host_id` (string, optional) — authoritative heartbeat host identity
- `hostname` (string, optional) — self-reported hostname fallback; must resolve to exactly one host_id
- `since` (string, optional) — minimum sampled_at timestamp (ISO 8601)
- `limit` (integer, optional) — number of samples to return (default 1, max 100)

---

## cortex fleet_state
Return a fleet-wide heartbeat snapshot with pressure flags and summary counts.

**Parameters:**
- `include_ok` (boolean, optional) — when `false`, exclude hosts with `status == "ok"` (default `true`)
- `sort` (string, optional) — sort order: `pressure` (default), `freshness`, or `hostname`

---

## cortex correlate_state
Correlate non-AI logs with per-host heartbeat window summaries around a reference time.
Bounded by default and never performs a full-history scan.

**Parameters:**
- `reference_time` (string, required) — center timestamp for the window (ISO 8601)
- `window_minutes` (integer, optional) — minutes before/after reference_time (default 10, max 120)
- `host` (string, optional) — host_id or unique hostname; omit for a bounded cross-host plan
- `severity_min` (string, optional) — minimum log severity (default `info`)
- `limit` (integer, optional) — max log rows per host (default 100, max 500)

Response includes the resolved `window`, per-host `heartbeat_summary` plus matching `logs`, and a `truncated` flag.

---

## cortex apps
List distinct application names with log counts, host counts, and first/last seen timestamps.
Mirror of `cortex hosts` for the `app_name` dimension.

**Parameters:**
- `hostname` (string, optional) — restrict to apps seen on this host

---

## cortex sessions
Lists AI transcript sessions grouped by project/tool/session/host.

**Parameters:**
- `project` (string, optional) — exact project path, e.g. `/home/jmagar/workspace/cortex`
- `tool` (string, optional) — AI tool filter: `claude`, `codex`, or `gemini`
- `hostname` (string, optional) — restrict to one host
- `from`, `to` (string, optional) — time range (ISO 8601)
- `limit` (integer, optional) — max sessions (default 100, max 1000)

---

## cortex search_sessions
Session-ranked full-text search across AI transcript rows. Returns grouped sessions rather than flat log rows.

**Parameters:**
- `query` (string, **required**) — FTS5 search query
- `project` (string, optional) — exact project path filter
- `tool` (string, optional) — AI tool filter: `claude`, `codex`, or `gemini`
- `from`, `to` (string, optional) — time range (ISO 8601)
- `limit` (integer, optional) — max grouped sessions (default 20, max 100)

---

## cortex abuse
Detects abuse in AI transcript rows and returns each hit with surrounding rows from the same AI session.

**Parameters:**
- `project` (string, optional) — exact project path filter
- `tool` (string, optional) — AI tool filter
- `from`, `to` (string, optional) — time range (ISO 8601)
- `limit` (integer, optional) — max matches (default 20, max 100)
- `before`, `after` (integer, optional) — same-session context rows around each hit (default 2, max 20)
- `terms` (array of strings, optional) — custom detector terms; replaces the built-in list

---

## cortex abuse_incidents
Groups AI transcript abuse hits into scored incident candidates. Returns incidents ordered by priority score (abuse_count * 10 + density * 2 + term_variety) with priority labels: low / medium / high / critical. Response includes total_incidents, candidate_rows, and truncated metadata.

**Parameters:**
- `project` (string, optional) — exact project path filter
- `tool` (string, optional) — AI tool filter
- `from`, `to` (string, optional) — time range (ISO 8601)
- `limit` (integer, optional) — max incidents (default 20, max 100)
- `window_minutes` (integer, optional) — grouping window (default 10, max 120)
- `terms` (array of strings, optional) — custom detector terms

---

## cortex abuse_investigate
Expands top abuse incidents into deterministic evidence bundles. Each bundle includes transcript context before/after the incident, the abuse anchor entries, and nearby non-AI syslog/Docker logs.

**Parameters:**
- `project` (string, optional) — exact project path filter
- `tool` (string, optional) — AI tool filter
- `from`, `to` (string, optional) — time range (ISO 8601)
- `limit` (integer, optional) — max incidents to expand (default 3, max 10)
- `window_minutes` (integer, optional) — grouping window (default 10, max 120)
- `correlation_window_minutes` (integer, optional) — minutes before/after incident for nearby log correlation (default 5, max 120)
- `terms` (array of strings, optional) — custom detector terms

---

## cortex ai_correlate
Cross-reference AI transcript anchor rows against nearby non-AI logs in the same database.
Related rows explicitly exclude AI transcript rows, so the result surfaces host, Docker, OTLP, and syslog context around the AI session instead of duplicating transcript rows.

**Parameters:**
- `project` (string, optional) — exact AI project path filter
- `tool` (string, optional) — AI tool filter
- `session_id` (string, optional) — exact AI session id filter
- `ai_query` (string, optional) — FTS5 query over AI transcript anchor rows
- `log_query` (string, optional) — FTS5 query over related non-AI logs
- `hostname`, `source_ip`, `app_name` (string, optional) — related log filters
- `from`, `to` (string, optional) — AI anchor time range (ISO 8601)
- `window_minutes` (integer, optional) — minutes before and after each AI anchor (default 5, max 120)
- `severity_min` (string, optional) — minimum related log severity (default `warning`)
- `limit` (integer, optional) — max AI anchors (default 10, max 50)
- `events_per_anchor` (integer, optional) — max related non-AI rows per anchor (default 25, max 200)

---

## cortex usage_blocks
AI activity bucketed into deterministic 5-hour UTC windows.

**Parameters:**
- `project` (string, optional) — exact project path filter
- `tool` (string, optional) — AI tool filter
- `from`, `to` (string, optional) — time range (ISO 8601)

---

## cortex project_context
Summary of one project path including tools, sessions, hosts, counts, and recent representative entries.

**Parameters:**
- `project` (string, **required**) — exact project path
- `tool` (string, optional) — AI tool filter
- `limit` (integer, optional) — recent representative entry limit (default 5, max 20)

---

## cortex list_ai_tools
Distinct AI tools with counts and first/last seen timestamps.

**Parameters:**
- `project` (string, optional) — exact project path filter
- `from`, `to` (string, optional) — time range (ISO 8601)

---

## cortex list_ai_projects
Distinct AI projects with counts, tools used, and first/last seen timestamps.

**Parameters:**
- `tool` (string, optional) — AI tool filter
- `from`, `to` (string, optional) — time range (ISO 8601)

---

## cortex source_ips
List distinct source identifiers (network sender IP:port for syslog input,
peer IP for OTLP input,
`docker://host/container/stream` for Docker stream ingest, or
`docker-event://host/container/action` for Docker lifecycle ingest) with log counts, the number
of distinct hostnames each sender claims, and up to 10 top hostnames per sender.
`source_ip` is the only network-verified identity — useful for spoof detection
on hostname-spoofable formats (e.g. UniFi CEF).

**Parameters:** none

---

## cortex correlate
Search for related events across multiple hosts within a time window.
Useful for debugging cascading failures — finds events on all hosts within ±N minutes
of a reference timestamp. Results are grouped by host and ordered by time.

**Parameters:**
- `reference_time` (string, **required**) — center timestamp (ISO 8601, e.g. `2025-01-15T14:30:00Z`)
- `window_minutes` (integer, optional) — minutes before and after reference_time to search (default 5, max 60)
- `severity_min` (string, optional) — minimum severity to include (default `warning`); `debug` returns everything
- `hostname` (string, optional) — limit correlation to a specific host
- `source_ip` (string, optional) — limit correlation to an exact source identifier. Syslog uses verified `IP:port`; OTLP uses verified peer IP; Docker stream rows use `docker://host/container/stream`; Docker lifecycle rows use `docker-event://host/container/action`.
- `query` (string, optional) — optional FTS query to narrow results
- `limit` (integer, optional) — max total events to return (default 500, max 999)

---

## cortex timeline
Bucketed log counts over a time range. Use to answer "when did errors start"
or "is the incident still active". Each point reports `{bucket, group?, count}`.

**Parameters:**
- `bucket` (string, optional) — `minute`, `hour` (default), `day`, `week`, or `month`
- `group_by` (string, optional) — split each bucket by `hostname`, `severity`, or `app_name`
- `from` (string, optional) — start of time range (ISO 8601)
- `to` (string, optional) — end of time range (ISO 8601)
- `hostname` (string, optional) — restrict to one host
- `app_name` (string, optional) — restrict to one app
- `severity_min` (string, optional) — only count entries at or above this severity

---

## cortex patterns
Cluster near-duplicate messages by template. Variable runs (numbers, IPv4
addresses, UUIDs, long hex strings) are normalised to placeholders so similar
messages aggregate. Returns top templates with counts, sample message, and
host distribution.

**Parameters:**
- `from` / `to` (string, optional) — time range (ISO 8601)
- `hostname`, `app_name` (string, optional) — narrow the population
- `severity_min` (string, optional) — only cluster entries at or above this severity
- `scan_limit` (integer, optional) — max messages to read (default 10000, max 10000)
- `top_n` (integer, optional) — max templates to return (default 20, max 200)
- `limit` (integer, optional) — alias for `top_n` for agent/CLI ergonomics

---

## cortex context
Surrounding logs around a single point of interest, on the same host. Pass
either `log_id` (preferred — uses (timestamp, id) for stable ordering) or both
`hostname` + `timestamp` to anchor on a synthetic reference.

**Parameters:**
- `log_id` (integer, optional) — id of an existing log entry (e.g. from `search`)
- `hostname` (string, optional) — required when `log_id` is not given
- `timestamp` (string, optional) — required when `log_id` is not given (ISO 8601)
- `before` (integer, optional) — entries before the reference (default 10, max 500)
- `after` (integer, optional) — entries after the reference (default 10, max 500)

---

## cortex get
Fetch one log entry by `id`, including the unparsed `raw` syslog frame.

**Parameters:**
- `id` (integer, **required**) — primary key from any other action

---

## cortex ingest_rate
Recent ingest throughput: counts and per-second rates over the last 1m / 5m /
15m windows (using `received_at`, not message timestamp). Includes the current
write-block flag for live ingest health.

**Parameters:**
- `by_host` (boolean, optional) — also include per-host buckets

---

## cortex silent_hosts
Hosts whose `last_seen` is older than `silent_minutes` ago. Reports their
typical inter-arrival interval so you can spot devices that should be chatty.

**Parameters:**
- `silent_minutes` (integer, optional) — staleness threshold (default 30, max 10080)

---

## cortex clock_skew
Per-host distribution of `received_at - timestamp` (seconds), sorted by
absolute mean. Surfaces devices with a broken or drifting clock.

**Parameters:**
- `since` (string, optional) — only sample entries with `received_at >= since` (default last 24h)
- `limit` (integer, optional) — cap returned host rows (max 100)

---

## cortex anomalies
Per-host comparison of recent volume against a baseline window. Reports
`recent_per_min`, `baseline_per_min`, ratio, and a Poisson-style z-score so an
agent can rank hosts whose log rate or error count is unusual.

**Parameters:**
- `recent_minutes` (integer, optional) — recent window (default 15, max 1440)
- `baseline_minutes` (integer, optional) — baseline window before the recent one (default 360, max 10080)

---

## cortex compare
Side-by-side summary of two time ranges (volume, error count, severity mix,
top hosts, top apps) plus deltas. Answers "what changed since yesterday".

**Parameters:**
- `a_from`, `a_to` (string, **required**) — first range (ISO 8601)
- `b_from`, `b_to` (string, **required**) — second range (ISO 8601)

---

## cortex stats
Get database statistics plus runtime ingest observability: listener counters, queue depth,
writer flush/failure/drop counters, last activity timestamps, and OTLP receiver counters.

**Parameters:** none

---

## cortex status
Get lightweight runtime status without full DB statistics. Use this for dashboards and
doctor checks that need queue/backpressure/writer state quickly.

**Parameters:** none

---

## cortex compose_status
Read-only Docker Compose diagnostics for the canonical cortex deployment.
The response is MCP-safe: host paths, image ids, mount sources, and raw command
output are omitted.

**Parameters:** none. Target override fields are rejected.

---

## cortex compose_doctor
Strict deployment-health check for the canonical cortex Compose deployment.
Returns the same redacted diagnostic shape as `compose_status` when healthy, and
returns a tool error when Docker/Compose ownership or runtime checks are not
ready for lifecycle work. Lifecycle mutations remain CLI-only.

**Parameters:** none. Target override fields are rejected.

---

## cortex unaddressed_errors
List the top unacknowledged repeating error signatures — log message patterns
that have been firing repeatedly without acknowledgement. Motivating case: an
OTLP exporter POSTing to `/v1/metrics` every 10s, getting 404d, for 7 days
unnoticed.

Returns signatures sorted by `last_seen_at` descending. Each entry includes a
normalized template, sample message, severity, counts, and acknowledgement state.

**Parameters:**
- `limit` (integer, optional) — max signatures to return (default 50)
- `include_acknowledged` (boolean, optional) — include already-acked sigs (default false)

---

## cortex notifications_recent
List recent notification firings from the `notification_firings` table.

**Parameters:**
- `limit` (integer, optional) — max rows to return (default 50, max 500)
- `rule_id` (string, optional) — filter by rule ID (e.g. `oom_kill`, `daily_digest`)
- `since` (string, optional) — ISO8601 lower bound for `fired_at`

---

## cortex similar_incidents

Find historical incidents similar to a query. Groups FTS5-matched system log hits
into time-windowed clusters by host+app_name. Returns ranked clusters (most
log hits first) with representative message snippets and correlated AI sessions
whose transcript timestamps overlap the cluster window.

**Required:** `query` (FTS5 syntax, e.g. `nginx upstream error` or `OOM killed`)
**Optional:** `hostname`, `app_name`, `severity_min`, `from`, `to`,
             `window_minutes` (cluster window, default 30, clamp 5..=120),
             `limit` (default 10, max 50)

Example: `{"action":"similar_incidents","query":"upstream connect error","app_name":"nginx"}`

Response fields: `query`, `total_clusters`, `truncated`, `clusters` where each
cluster has: `hostname`, `app_name`, `window_start`, `window_end`, `log_count`,
`severity_peak`, `representative_messages` (up to 3), `correlated_sessions` (up to 5).

---

## cortex ask_history

Search AI session transcripts for past work related to a topic. Returns sessions
ranked by match count with system log context from the top session's time window.
Use this to answer "what did an AI agent work on related to X?".

**Required:** `query` (FTS5 syntax, e.g. `nginx ssl certificate` or `OOM postgres`)
**Optional:** `hostname`, `app_name`, `from`, `to`, `limit` (default 10, max 50)

Example: `{"action":"ask_history","query":"nginx ssl certificate"}`

Response fields: `query`, `total_candidates`, `truncated`,
`sessions` (SearchedSessionEntry array ranked by match_count),
`context_logs` (system log entries from the top session's time window).

---

## cortex incident_context

Return full context for a known time window: log counts by severity and app,
error-level log rows, and AI sessions active in that window. Useful for
post-incident review of a time range you already know was problematic.

**Required:** `from`, `to` (ISO 8601/RFC3339)
**Optional:** `hostname`, `app_name`, `severity_min` (default warning),
             `limit` (max error log rows, default 50, max 200)
**Note:** `query` is accepted but reserved for v2 FTS5 filtering; it is
          currently ignored — omit it for incident_context.

Example: `{"action":"incident_context","from":"2024-01-15T10:00:00Z","to":"2024-01-15T11:00:00Z"}`

Response fields: `window_from`, `window_to`, `total_logs`, `by_severity` (array),
`by_app` (array, top 20), `error_logs` (array), `error_logs_truncated`, `ai_sessions`.

---

## cortex graph

Resolve graph entities, return bounded one-hop graph neighborhoods, produce
deterministic evidence-backed explanations, or inspect one evidence row with a
safe source-log summary. The graph projection is rebuildable state; this read
action never triggers a rebuild implicitly and returns projection/degraded
status in `metadata`.

**Required:** exact entity lookup uses `entity_type` + `key`; alias lookup uses
              `alias_type` + `alias_key`; neighborhood lookup uses either
              `entity_id` or the same entity lookup fields; evidence lookup uses
              `mode="evidence"` + `evidence_id`.
**Optional:** `mode` (`entity`, `around`, `explain`, or `evidence`, default `around`),
             `limit`, `depth` (`around` supports only 1; `explain` defaults
             2 and clamps to 3), `beam_width`, `max_chains`,
             `evidence_sample_limit`, `payload_budget`

Examples:
`{"action":"graph","mode":"entity","entity_type":"host","key":"tootie"}`
`{"action":"graph","mode":"around","entity_type":"host","key":"tootie","depth":1}`
`{"action":"graph","mode":"explain","entity_type":"host","key":"tootie","depth":2}`
`{"action":"graph","mode":"evidence","evidence_id":123}`

Response fields: `resolved_entity`, `entities`, `relationships`, `evidence`,
`src_entity`, `dst_entity`, `source_log_summary`, `missing_source_reason`,
`chains`, `narrative`, `open_questions`, `missing_evidence`, `next_queries`,
`candidates`, and `metadata`. Narrative mode is deterministic and cites
relationship/evidence ids; weak evidence returns open questions instead of
causal claims. Evidence/source-log summaries exclude raw frames and raw metadata
by default; excerpts are truncated and redacted.

"#;
    let help = format!(
        "{help}\n{cost_guide}{}{}",
        admin_action_help(),
        r#"---

## cortex help
Returns this markdown documentation.

**Parameters:** none
"#
    );
    Ok(json!({ "help": help }))
}

// ---------------------------------------------------------------------------
// RAG v1 handlers
// ---------------------------------------------------------------------------

async fn tool_similar_incidents(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: SimilarIncidentsRequest = action_payload(args, "similar_incidents")?;
    let response = state.service.similar_incidents(req).await?;
    tracing::debug!(
        cluster_count = response.total_clusters,
        "similar_incidents completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_ask_history(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AskHistoryRequest = action_payload(args, "ask_history")?;
    let response = state.service.ask_history(req).await?;
    tracing::debug!(
        session_count = response.sessions.len(),
        "ask_history completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_incident_context(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: IncidentContextRequest = action_payload(args, "incident_context")?;
    let response = state.service.incident_context(req).await?;
    tracing::debug!(
        total_logs = response.total_logs,
        error_count = response.error_logs.len(),
        "incident_context completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_graph(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let mode = string_arg(&args, "mode").unwrap_or_else(|| "around".to_string());
    match mode.as_str() {
        "entity" => {
            let req: GraphEntityLookupRequest = action_payload(args, "graph")?;
            Ok(serde_json::to_value(
                state.service.graph_entity_lookup(req).await?,
            )?)
        }
        "around" => {
            let req: GraphAroundRequest = action_payload(args, "graph")?;
            Ok(serde_json::to_value(
                state.service.graph_around(req).await?,
            )?)
        }
        "explain" => {
            let req: GraphExplainRequest = action_payload(args, "graph")?;
            Ok(serde_json::to_value(
                state.service.graph_explain(req).await?,
            )?)
        }
        "evidence" => {
            let req: GraphEvidenceLookupRequest = action_payload(args, "graph")?;
            Ok(serde_json::to_value(
                state.service.graph_evidence_lookup(req).await?,
            )?)
        }
        other => Err(anyhow::anyhow!(
            "unsupported graph mode '{other}'; expected entity, around, explain, or evidence"
        )),
    }
}

/// Parse an optional RFC3339 timestamp string and normalize it to UTC.
///
/// Returns `Ok(None)` when `raw` is `None`. Returns a descriptive error when
/// `raw` is `Some` but not valid RFC3339 — callers get a clear message rather
/// than a silent wrong-result query against UTC-stored timestamps.
#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
