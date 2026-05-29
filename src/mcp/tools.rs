use lab_auth::AuthContext;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::app::{
    AbuseSearchRequest, AiCorrelateRequest, AiIncidentRequest, AiInvestigateRequest,
    AnomaliesRequest, AskHistoryRequest, ClockSkewRequest, CompareRequest, ContextRequest,
    CorrelateEventsRequest, FilterLogsRequest, FleetStateRequest, GetErrorsRequest, GetLogRequest,
    HostStateRequest, IncidentContextRequest, IngestRateRequest, ListAiProjectsRequest,
    ListAiToolsRequest, ListAppsRequest, ListSessionsRequest, ListSourceIpsRequest,
    NotificationsRecentRequest, PatternsRequest, ProjectContextRequest, RequestActor,
    SearchLogsRequest, SearchSessionsRequest, SilentHostsRequest, SimilarIncidentsRequest,
    TailLogsRequest, TimelineRequest, UsageBlocksRequest,
};

use super::actions;
use super::AppState;

/// Execute a tool by name
pub(super) async fn execute_tool(
    state: &AppState,
    name: &str,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    match name {
        "syslog" => tool_syslog(state, args, auth).await,
        _ => Err(anyhow::anyhow!("Unknown tool: {name}")),
    }
}

async fn tool_syslog(
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    let action =
        string_arg(&args, "action").ok_or_else(|| anyhow::anyhow!("action is required"))?;
    match action.as_str() {
        "search" => tool_search_logs(state, args).await,
        "filter" => tool_filter_logs(state, args).await,
        "tail" => tool_tail_logs(state, args).await,
        "errors" => tool_get_errors(state, args).await,
        "hosts" => tool_list_hosts(state, args).await,
        "host_state" => tool_host_state(state, args).await,
        "fleet_state" => tool_fleet_state(state, args).await,
        "correlate" => tool_correlate_events(state, args).await,
        "stats" => tool_get_stats(state, args).await,
        "status" => tool_get_status(state, args).await,
        "apps" => tool_list_apps(state, args).await,
        "sessions" => tool_list_sessions(state, args).await,
        "search_sessions" => tool_search_sessions(state, args).await,
        "abuse" => tool_search_abuse(state, args).await,
        "abuse_incidents" => tool_abuse_incidents(state, args).await,
        "abuse_investigate" => tool_abuse_investigate(state, args).await,
        "ai_correlate" => tool_ai_correlate(state, args).await,
        "usage_blocks" => tool_usage_blocks(state, args).await,
        "project_context" => tool_project_context(state, args).await,
        "list_ai_tools" => tool_list_ai_tools(state, args).await,
        "list_ai_projects" => tool_list_ai_projects(state, args).await,
        "source_ips" => tool_list_source_ips(state, args).await,
        "timeline" => tool_timeline(state, args).await,
        "patterns" => tool_patterns(state, args).await,
        "context" => tool_context(state, args).await,
        "get" => tool_get_log(state, args).await,
        "ingest_rate" => tool_ingest_rate(state, args).await,
        "silent_hosts" => tool_silent_hosts(state, args).await,
        "clock_skew" => tool_clock_skew(state, args).await,
        "anomalies" => tool_anomalies(state, args).await,
        "compare" => tool_compare(state, args).await,
        "compose_status" => tool_compose_status(args).await,
        "compose_doctor" => tool_compose_doctor(args).await,
        "unaddressed_errors" => tool_unaddressed_errors(state, args).await,
        "ack_error" => tool_ack_error(state, args, auth).await,
        "unack_error" => tool_unack_error(state, args, auth).await,
        "notifications_recent" => tool_notifications_recent(state, args).await,
        "notifications_test" => tool_notifications_test(state, args, auth).await,
        "similar_incidents" => tool_similar_incidents(state, args).await,
        "ask_history" => tool_ask_history(state, args).await,
        "incident_context" => tool_incident_context(state, args).await,
        "help" => tool_syslog_help().await,
        _ => Err(anyhow::anyhow!(
            "unknown syslog action: {action}; expected one of {}",
            actions::action_names().join(", ")
        )),
    }
}

async fn tool_search_logs(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .search_logs(SearchLogsRequest {
            query: string_arg(&args, "query"),
            hostname: string_arg(&args, "hostname"),
            source_ip: string_arg(&args, "source_ip"),
            severity: string_arg(&args, "severity"),
            app_name: string_arg(&args, "app_name"),
            facility: string_arg(&args, "facility"),
            exclude_facility: string_arg(&args, "exclude_facility"),
            process_id: string_arg(&args, "process_id"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            received_from: string_arg(&args, "received_from"),
            received_to: string_arg(&args, "received_to"),
            limit: u32_arg(&args, "limit")?,
            source_kind: string_arg(&args, "source_kind"),
            tool: string_arg(&args, "tool"),
            project: string_arg(&args, "project"),
            session_id: string_arg(&args, "session_id"),
            container: string_arg(&args, "container"),
            docker_host: string_arg(&args, "docker_host"),
            stream: string_arg(&args, "stream"),
            event_action: string_arg(&args, "event_action"),
        })
        .await?;
    tracing::debug!(result_count = response.count, "search_logs completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_filter_logs(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: FilterLogsRequest = action_payload(args)?;
    let response = state.service.filter_logs(req).await?;
    tracing::debug!(result_count = response.count, "filter_logs completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_tail_logs(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .tail_logs(TailLogsRequest {
            hostname: string_arg(&args, "hostname"),
            source_ip: string_arg(&args, "source_ip"),
            app_name: string_arg(&args, "app_name"),
            severity_min: string_arg(&args, "severity_min"),
            n: u32_arg(&args, "n")?,
        })
        .await?;
    tracing::debug!(result_count = response.count, "tail_logs completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_get_errors(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .get_errors(GetErrorsRequest {
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            group_by: string_arg(&args, "group_by"),
            limit: u32_arg(&args, "limit")?,
        })
        .await?;
    tracing::debug!(
        summary_rows = response.summary.len(),
        "get_errors completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_list_apps(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .list_apps(ListAppsRequest {
            hostname: string_arg(&args, "hostname"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            limit: u32_arg(&args, "limit")?,
            offset: u32_arg(&args, "offset")?,
        })
        .await?;
    tracing::debug!(
        app_count = response.apps.len(),
        total = response.total,
        "list_apps completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_host_state(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: HostStateRequest = action_payload(args)?;
    Ok(serde_json::to_value(state.service.host_state(req).await?)?)
}

async fn tool_fleet_state(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: FleetStateRequest = action_payload(args)?;
    Ok(serde_json::to_value(state.service.fleet_state(req).await?)?)
}

async fn tool_list_sessions(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .list_sessions(ListSessionsRequest {
            project: string_arg(&args, "project"),
            tool: string_arg(&args, "tool"),
            hostname: string_arg(&args, "hostname"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            limit: u32_arg(&args, "limit")?,
        })
        .await?;
    tracing::debug!(session_count = response.count, "list_sessions completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_search_sessions(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let query = string_arg(&args, "query").ok_or_else(|| anyhow::anyhow!("query is required"))?;
    let response = state
        .service
        .search_sessions(SearchSessionsRequest {
            query,
            project: string_arg(&args, "project"),
            tool: string_arg(&args, "tool"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            limit: u32_arg(&args, "limit")?,
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_search_abuse(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let terms = args
        .get("terms")
        .map(|value| {
            if let Some(values) = value.as_array() {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(ToString::to_string))
                    .collect()
            } else {
                value
                    .as_str()
                    .map(|term| vec![term.to_string()])
                    .unwrap_or_default()
            }
        })
        .unwrap_or_default();
    let response = state
        .service
        .search_abuse(AbuseSearchRequest {
            project: string_arg(&args, "project"),
            tool: string_arg(&args, "tool"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            limit: u32_arg(&args, "limit")?,
            before: u32_arg(&args, "before")?,
            after: u32_arg(&args, "after")?,
            terms,
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_abuse_incidents(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let terms = args
        .get("terms")
        .map(|v| {
            if let Some(arr) = v.as_array() {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            } else {
                v.as_str().map(|s| vec![s.to_string()]).unwrap_or_default()
            }
        })
        .unwrap_or_default();
    let response = state
        .service
        .list_ai_incidents(AiIncidentRequest {
            project: string_arg(&args, "project"),
            tool: string_arg(&args, "tool"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            limit: u32_arg(&args, "limit")?,
            window_minutes: u32_arg(&args, "window_minutes")?,
            terms,
        })
        .await?;
    tracing::debug!(
        incident_count = response.incidents.len(),
        total = response.total_incidents,
        "abuse_incidents completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_abuse_investigate(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let terms = args
        .get("terms")
        .map(|v| {
            if let Some(arr) = v.as_array() {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            } else {
                v.as_str().map(|s| vec![s.to_string()]).unwrap_or_default()
            }
        })
        .unwrap_or_default();
    let response = state
        .service
        .investigate_ai_incidents(AiInvestigateRequest {
            incident_id: string_arg(&args, "incident_id"),
            project: string_arg(&args, "project"),
            tool: string_arg(&args, "tool"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            limit: u32_arg(&args, "limit")?,
            window_minutes: u32_arg(&args, "window_minutes")?,
            correlation_window_minutes: u32_arg(&args, "correlation_window_minutes")?,
            terms,
        })
        .await?;
    tracing::debug!(
        evidence_count = response.evidence.len(),
        total_incidents = response.total_incidents,
        "abuse_investigate completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_ai_correlate(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .correlate_ai_logs(AiCorrelateRequest {
            project: string_arg(&args, "project"),
            tool: string_arg(&args, "tool"),
            session_id: string_arg(&args, "session_id"),
            ai_query: string_arg(&args, "ai_query"),
            log_query: string_arg(&args, "log_query"),
            hostname: string_arg(&args, "hostname"),
            source_ip: string_arg(&args, "source_ip"),
            app_name: string_arg(&args, "app_name"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            window_minutes: u32_arg(&args, "window_minutes")?,
            severity_min: string_arg(&args, "severity_min"),
            limit: u32_arg(&args, "limit")?,
            events_per_anchor: u32_arg(&args, "events_per_anchor")?,
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_usage_blocks(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .usage_blocks(UsageBlocksRequest {
            project: string_arg(&args, "project"),
            tool: string_arg(&args, "tool"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_project_context(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let project =
        string_arg(&args, "project").ok_or_else(|| anyhow::anyhow!("project is required"))?;
    let response = state
        .service
        .project_context(ProjectContextRequest {
            project,
            tool: string_arg(&args, "tool"),
            limit: u32_arg(&args, "limit")?,
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_list_ai_tools(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .list_ai_tools(ListAiToolsRequest {
            project: string_arg(&args, "project"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_list_ai_projects(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .list_ai_projects(ListAiProjectsRequest {
            tool: string_arg(&args, "tool"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_list_source_ips(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .list_source_ips(ListSourceIpsRequest {
            limit: u32_arg(&args, "limit")?,
            offset: u32_arg(&args, "offset")?,
        })
        .await?;
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
            anyhow::bail!("compose MCP actions do not accept target override: {key}");
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
    // Default lookback is centralized in `SyslogService::timeline` (bead dyqw):
    // it applies a bucket-sized window only when neither `from` nor `to` is set,
    // preventing full table scans without recreating the logic per transport.
    let response = state
        .service
        .timeline(TimelineRequest {
            bucket: string_arg(&args, "bucket"),
            group_by: string_arg(&args, "group_by"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            hostname: string_arg(&args, "hostname"),
            app_name: string_arg(&args, "app_name"),
            severity_min: string_arg(&args, "severity_min"),
        })
        .await?;
    tracing::debug!(point_count = response.points.len(), "timeline completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_patterns(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let top_n = u32_arg(&args, "top_n")?.or(u32_arg(&args, "limit")?);
    let response = state
        .service
        .patterns(PatternsRequest {
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            hostname: string_arg(&args, "hostname"),
            app_name: string_arg(&args, "app_name"),
            severity_min: string_arg(&args, "severity_min"),
            scan_limit: u32_arg(&args, "scan_limit")?,
            top_n,
        })
        .await?;
    tracing::debug!(
        pattern_count = response.patterns.len(),
        scanned = response.scanned,
        truncated = response.truncated,
        "patterns completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_context(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .context(ContextRequest {
            log_id: i64_arg(&args, "log_id")?,
            hostname: string_arg(&args, "hostname"),
            timestamp: string_arg(&args, "timestamp"),
            before: u32_arg(&args, "before")?,
            after: u32_arg(&args, "after")?,
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_get_log(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let id = i64_arg(&args, "id")?.ok_or_else(|| anyhow::anyhow!("`id` is required"))?;
    let response = state.service.get_log(GetLogRequest { id }).await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_ingest_rate(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .ingest_rate(IngestRateRequest {
            by_host: bool_arg(&args, "by_host"),
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_silent_hosts(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .silent_hosts(SilentHostsRequest {
            silent_minutes: u32_arg(&args, "silent_minutes")?,
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_clock_skew(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .clock_skew(ClockSkewRequest {
            since: string_arg(&args, "since"),
            limit: u32_arg(&args, "limit")?,
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_anomalies(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let response = state
        .service
        .anomalies(AnomaliesRequest {
            recent_minutes: u32_arg(&args, "recent_minutes")?,
            baseline_minutes: u32_arg(&args, "baseline_minutes")?,
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_compare(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let a_from =
        string_arg(&args, "a_from").ok_or_else(|| anyhow::anyhow!("`a_from` is required"))?;
    let a_to = string_arg(&args, "a_to").ok_or_else(|| anyhow::anyhow!("`a_to` is required"))?;
    let b_from =
        string_arg(&args, "b_from").ok_or_else(|| anyhow::anyhow!("`b_from` is required"))?;
    let b_to = string_arg(&args, "b_to").ok_or_else(|| anyhow::anyhow!("`b_to` is required"))?;
    let response = state
        .service
        .compare(CompareRequest {
            a_from,
            a_to,
            b_from,
            b_to,
        })
        .await?;
    Ok(serde_json::to_value(response)?)
}

async fn tool_list_hosts(state: &AppState, _args: Value) -> anyhow::Result<Value> {
    let response = state.service.list_hosts().await?;
    tracing::debug!(host_count = response.hosts.len(), "list_hosts completed");
    Ok(serde_json::to_value(response)?)
}

async fn tool_correlate_events(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let reference_time = string_arg(&args, "reference_time")
        .ok_or_else(|| anyhow::anyhow!("reference_time is required"))?;
    let response = state
        .service
        .correlate_events(CorrelateEventsRequest {
            reference_time,
            window_minutes: u32_arg(&args, "window_minutes")?,
            severity_min: string_arg(&args, "severity_min"),
            hostname: string_arg(&args, "hostname"),
            source_ip: string_arg(&args, "source_ip"),
            query: string_arg(&args, "query"),
            limit: u32_arg(&args, "limit")?,
        })
        .await?;
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

fn action_payload<T: DeserializeOwned>(args: Value) -> anyhow::Result<T> {
    let mut object = args
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("tool arguments must be a JSON object"))?;
    object.remove("action");
    serde_json::from_value(Value::Object(object))
        .map_err(|err| anyhow::anyhow!("invalid filter arguments: {err}"))
}

fn u32_arg(args: &Value, name: &str) -> anyhow::Result<Option<u32>> {
    let Some(value) = args.get(name) else {
        return Ok(None);
    };
    let unsigned = value
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("{name} must be an unsigned integer"))?;
    u32::try_from(unsigned)
        .map(Some)
        .map_err(|_| anyhow::anyhow!("{name} must be <= {}", u32::MAX))
}

fn i64_arg(args: &Value, name: &str) -> anyhow::Result<Option<i64>> {
    let Some(value) = args.get(name) else {
        return Ok(None);
    };
    if let Some(n) = value.as_i64() {
        return Ok(Some(n));
    }
    if let Some(n) = value.as_u64() {
        return i64::try_from(n)
            .map(Some)
            .map_err(|_| anyhow::anyhow!("{name} must fit in i64"));
    }
    Err(anyhow::anyhow!("{name} must be an integer"))
}

fn bool_arg(args: &Value, name: &str) -> Option<bool> {
    args.get(name).and_then(|v| v.as_bool())
}

// ---------------------------------------------------------------------------
// Error detection actions

async fn tool_unaddressed_errors(state: &AppState, args: Value) -> anyhow::Result<Value> {
    use crate::app::UnaddressedErrorsRequest;
    let req = UnaddressedErrorsRequest {
        limit: u32_arg(&args, "limit")?,
        include_acknowledged: bool_arg(&args, "include_acknowledged"),
    };
    let resp = state.service.unaddressed_errors(req).await?;
    Ok(serde_json::to_value(resp)?)
}

async fn tool_ack_error(
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    use crate::app::AckErrorRequest;
    let hash = string_arg(&args, "signature_hash")
        .ok_or_else(|| anyhow::anyhow!("signature_hash is required"))?;
    let req = AckErrorRequest {
        signature_hash: hash,
        notes: string_arg(&args, "notes"),
    };
    let actor = extract_actor(state, auth);
    let resp = state.service.ack_error(req, actor).await?;
    Ok(serde_json::to_value(resp)?)
}

async fn tool_unack_error(
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    use crate::app::UnackErrorRequest;
    let hash = string_arg(&args, "signature_hash")
        .ok_or_else(|| anyhow::anyhow!("signature_hash is required"))?;
    let req = UnackErrorRequest {
        signature_hash: hash,
        reason: string_arg(&args, "reason"),
    };
    let actor = extract_actor(state, auth);
    let resp = state.service.unack_error(req, actor).await?;
    Ok(serde_json::to_value(resp)?)
}

async fn tool_notifications_recent(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let firings = state
        .service
        .notifications_recent_checked(NotificationsRecentRequest {
            limit: args.get("limit").and_then(|v| v.as_i64()),
            rule_id: string_arg(&args, "rule_id"),
            since: string_arg(&args, "since"),
        })
        .await?;
    Ok(serde_json::to_value(firings)?)
}

async fn tool_notifications_test(
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    let body = string_arg(&args, "body")
        .unwrap_or_else(|| "Test notification from syslog-mcp".to_string());
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
        action: "notifications_test",
        description: "Send a test notification via the server-configured Apprise URLs. Rate-limited to 10 per minute per actor.\nCaller-supplied Apprise URLs are ignored for security; the server uses its own configured URLs.",
        parameters: &[
            "`body` (string, optional) — notification body text (default: test message)",
        ],
    },
];

fn admin_action_help() -> String {
    let mut help = String::new();
    for action in ADMIN_ACTION_HELP {
        help.push_str("---\n\n");
        help.push_str("## syslog ");
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

async fn tool_syslog_help() -> anyhow::Result<Value> {
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
    let help = r#"# syslog-mcp Tool Reference

The MCP server exposes one tool, `syslog`. Set the required `action` argument
to select the operation.

## syslog search
Full-text search across all syslog messages with optional filters.
Uses SQLite FTS5 with porter stemming. Supports FTS5 query syntax: AND, OR, NOT,
phrase matching with quotes, prefix matching with *.

**Parameters:**
- `query` (string) — FTS5 search query, e.g. `'kernel panic'`, `'OOM AND killer'`, `'"connection refused"'`, `'error*'`
- `hostname` (string, optional) — filter by hostname (exact match); use `syslog hosts` to enumerate
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

## syslog filter
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

## syslog tail
Get the N most recent log entries, optionally filtered by host, application, and/or severity floor.
Equivalent to `tail -f` across all hosts.

**Parameters:**
- `hostname` (string, optional) — filter to a specific host
- `source_ip` (string, optional) — filter by exact source identifier. Syslog uses verified `IP:port`; OTLP uses verified peer IP; Docker stream rows use `docker://host/container/stream`; Docker lifecycle rows use `docker-event://host/container/action`.
- `app_name` (string, optional) — filter to a specific application
- `severity_min` (string, optional) — only return entries at or above this severity (e.g. `warning` returns warning + worse)
- `n` (integer, optional) — number of recent entries (default 50, max 500)

---

## syslog errors
Get a summary of errors and warnings across all hosts in a time window.
Groups by hostname and severity level (and optionally app_name), showing counts.

**Parameters:**
- `from` (string, optional) — start of time range (ISO 8601); defaults to all time
- `to` (string, optional) — end of time range (ISO 8601); defaults to now
- `group_by` (string, optional) — secondary grouping key. Currently `app_name` is supported; default groups only by hostname+severity.
- `limit` (integer, optional) — cap summary rows returned (max 100)

---

## syslog hosts
List all hosts that have sent syslog messages, with first/last seen timestamps and total log counts.

**Parameters:** none

---

## syslog host_state
Return the latest bounded heartbeat state for one host.

**Parameters:**
- `host_id` (string, optional) — authoritative heartbeat host identity
- `hostname` (string, optional) — self-reported hostname fallback; must resolve to exactly one host_id
- `since` (string, optional) — minimum sampled_at timestamp (ISO 8601)
- `limit` (integer, optional) — number of samples to return (default 1, max 100)

---

## syslog fleet_state
Return a fleet-wide heartbeat snapshot with pressure flags and summary counts.

**Parameters:**
- `include_ok` (boolean, optional) — when `false`, exclude hosts with `status == "ok"` (default `true`)
- `sort` (string, optional) — sort order: `pressure` (default), `freshness`, or `hostname`

---

## syslog apps
List distinct application names with log counts, host counts, and first/last seen timestamps.
Mirror of `syslog hosts` for the `app_name` dimension.

**Parameters:**
- `hostname` (string, optional) — restrict to apps seen on this host

---

## syslog sessions
Lists AI transcript sessions grouped by project/tool/session/host.

**Parameters:**
- `project` (string, optional) — exact project path, e.g. `/home/jmagar/workspace/syslog-mcp`
- `tool` (string, optional) — AI tool filter: `claude`, `codex`, or `gemini`
- `hostname` (string, optional) — restrict to one host
- `from`, `to` (string, optional) — time range (ISO 8601)
- `limit` (integer, optional) — max sessions (default 100, max 1000)

---

## syslog search_sessions
Session-ranked full-text search across AI transcript rows. Returns grouped sessions rather than flat log rows.

**Parameters:**
- `query` (string, **required**) — FTS5 search query
- `project` (string, optional) — exact project path filter
- `tool` (string, optional) — AI tool filter: `claude`, `codex`, or `gemini`
- `from`, `to` (string, optional) — time range (ISO 8601)
- `limit` (integer, optional) — max grouped sessions (default 20, max 100)

---

## syslog abuse
Detects abuse in AI transcript rows and returns each hit with surrounding rows from the same AI session.

**Parameters:**
- `project` (string, optional) — exact project path filter
- `tool` (string, optional) — AI tool filter
- `from`, `to` (string, optional) — time range (ISO 8601)
- `limit` (integer, optional) — max matches (default 20, max 100)
- `before`, `after` (integer, optional) — same-session context rows around each hit (default 2, max 20)
- `terms` (array of strings, optional) — custom detector terms; replaces the built-in list

---

## syslog abuse_incidents
Groups AI transcript abuse hits into scored incident candidates. Returns incidents ordered by priority score (abuse_count * 10 + density * 2 + term_variety) with priority labels: low / medium / high / critical. Response includes total_incidents, candidate_rows, and truncated metadata.

**Parameters:**
- `project` (string, optional) — exact project path filter
- `tool` (string, optional) — AI tool filter
- `from`, `to` (string, optional) — time range (ISO 8601)
- `limit` (integer, optional) — max incidents (default 20, max 100)
- `window_minutes` (integer, optional) — grouping window (default 10, max 120)
- `terms` (array of strings, optional) — custom detector terms

---

## syslog abuse_investigate
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

## syslog ai_correlate
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

## syslog usage_blocks
AI activity bucketed into deterministic 5-hour UTC windows.

**Parameters:**
- `project` (string, optional) — exact project path filter
- `tool` (string, optional) — AI tool filter
- `from`, `to` (string, optional) — time range (ISO 8601)

---

## syslog project_context
Summary of one project path including tools, sessions, hosts, counts, and recent representative entries.

**Parameters:**
- `project` (string, **required**) — exact project path
- `tool` (string, optional) — AI tool filter
- `limit` (integer, optional) — recent representative entry limit (default 5, max 20)

---

## syslog list_ai_tools
Distinct AI tools with counts and first/last seen timestamps.

**Parameters:**
- `project` (string, optional) — exact project path filter
- `from`, `to` (string, optional) — time range (ISO 8601)

---

## syslog list_ai_projects
Distinct AI projects with counts, tools used, and first/last seen timestamps.

**Parameters:**
- `tool` (string, optional) — AI tool filter
- `from`, `to` (string, optional) — time range (ISO 8601)

---

## syslog source_ips
List distinct source identifiers (network sender IP:port for syslog input,
peer IP for OTLP input,
`docker://host/container/stream` for Docker stream ingest, or
`docker-event://host/container/action` for Docker lifecycle ingest) with log counts, the number
of distinct hostnames each sender claims, and up to 10 top hostnames per sender.
`source_ip` is the only network-verified identity — useful for spoof detection
on hostname-spoofable formats (e.g. UniFi CEF).

**Parameters:** none

---

## syslog correlate
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

## syslog timeline
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

## syslog patterns
Cluster near-duplicate messages by template. Variable runs (numbers, IPv4
addresses, UUIDs, long hex strings) are normalised to placeholders so similar
messages aggregate. Returns top templates with counts, sample message, and
host distribution.

**Parameters:**
- `from` / `to` (string, optional) — time range (ISO 8601)
- `hostname`, `app_name` (string, optional) — narrow the population
- `severity_min` (string, optional) — only cluster entries at or above this severity
- `scan_limit` (integer, optional) — max messages to read (default 10000, max 50000)
- `top_n` (integer, optional) — max templates to return (default 20, max 200)
- `limit` (integer, optional) — alias for `top_n` for agent/CLI ergonomics

---

## syslog context
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

## syslog get
Fetch one log entry by `id`, including the unparsed `raw` syslog frame.

**Parameters:**
- `id` (integer, **required**) — primary key from any other action

---

## syslog ingest_rate
Recent ingest throughput: counts and per-second rates over the last 1m / 5m /
15m windows (using `received_at`, not message timestamp). Includes the current
write-block flag for live ingest health.

**Parameters:**
- `by_host` (boolean, optional) — also include per-host buckets

---

## syslog silent_hosts
Hosts whose `last_seen` is older than `silent_minutes` ago. Reports their
typical inter-arrival interval so you can spot devices that should be chatty.

**Parameters:**
- `silent_minutes` (integer, optional) — staleness threshold (default 30, max 10080)

---

## syslog clock_skew
Per-host distribution of `received_at - timestamp` (seconds), sorted by
absolute mean. Surfaces devices with a broken or drifting clock.

**Parameters:**
- `since` (string, optional) — only sample entries with `received_at >= since` (default last 24h)
- `limit` (integer, optional) — cap returned host rows (max 100)

---

## syslog anomalies
Per-host comparison of recent volume against a baseline window. Reports
`recent_per_min`, `baseline_per_min`, ratio, and a Poisson-style z-score so an
agent can rank hosts whose log rate or error count is unusual.

**Parameters:**
- `recent_minutes` (integer, optional) — recent window (default 15, max 1440)
- `baseline_minutes` (integer, optional) — baseline window before the recent one (default 360, max 10080)

---

## syslog compare
Side-by-side summary of two time ranges (volume, error count, severity mix,
top hosts, top apps) plus deltas. Answers "what changed since yesterday".

**Parameters:**
- `a_from`, `a_to` (string, **required**) — first range (ISO 8601)
- `b_from`, `b_to` (string, **required**) — second range (ISO 8601)

---

## syslog stats
Get database statistics plus runtime ingest observability: listener counters, queue depth,
writer flush/failure/drop counters, last activity timestamps, and OTLP receiver counters.

**Parameters:** none

---

## syslog status
Get lightweight runtime status without full DB statistics. Use this for dashboards and
doctor checks that need queue/backpressure/writer state quickly.

**Parameters:** none

---

## syslog compose_status
Read-only Docker Compose diagnostics for the canonical syslog-mcp deployment.
The response is MCP-safe: host paths, image ids, mount sources, and raw command
output are omitted.

**Parameters:** none. Target override fields are rejected.

---

## syslog compose_doctor
Strict deployment-health check for the canonical syslog-mcp Compose deployment.
Returns the same redacted diagnostic shape as `compose_status` when healthy, and
returns a tool error when Docker/Compose ownership or runtime checks are not
ready for lifecycle work. Lifecycle mutations remain CLI-only.

**Parameters:** none. Target override fields are rejected.

---

## syslog unaddressed_errors
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

## syslog notifications_recent
List recent notification firings from the `notification_firings` table.

**Parameters:**
- `limit` (integer, optional) — max rows to return (default 50, max 500)
- `rule_id` (string, optional) — filter by rule ID (e.g. `oom_kill`, `daily_digest`)
- `since` (string, optional) — ISO8601 lower bound for `fired_at`

---

## syslog similar_incidents

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

## syslog ask_history

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

## syslog incident_context

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

"#;
    let help = format!(
        "{help}\n{cost_guide}{}{}",
        admin_action_help(),
        r#"---

## syslog help
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
    let query = string_arg(&args, "query").ok_or_else(|| anyhow::anyhow!("query is required"))?;
    let response = state
        .service
        .similar_incidents(SimilarIncidentsRequest {
            query,
            hostname: string_arg(&args, "hostname"),
            app_name: string_arg(&args, "app_name"),
            severity_min: string_arg(&args, "severity_min"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            window_minutes: u32_arg(&args, "window_minutes")?,
            limit: u32_arg(&args, "limit")?,
        })
        .await?;
    tracing::debug!(
        cluster_count = response.total_clusters,
        "similar_incidents completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_ask_history(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let query = string_arg(&args, "query").ok_or_else(|| anyhow::anyhow!("query is required"))?;
    let response = state
        .service
        .ask_history(AskHistoryRequest {
            query,
            hostname: string_arg(&args, "hostname"),
            app_name: string_arg(&args, "app_name"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            limit: u32_arg(&args, "limit")?,
        })
        .await?;
    tracing::debug!(
        session_count = response.sessions.len(),
        "ask_history completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_incident_context(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let from = string_arg(&args, "from")
        .ok_or_else(|| anyhow::anyhow!("from is required for incident_context"))?;
    let to = string_arg(&args, "to")
        .ok_or_else(|| anyhow::anyhow!("to is required for incident_context"))?;
    let response = state
        .service
        .incident_context(IncidentContextRequest {
            from,
            to,
            hostname: string_arg(&args, "hostname"),
            app_name: string_arg(&args, "app_name"),
            query: string_arg(&args, "query"),
            severity_min: string_arg(&args, "severity_min"),
            limit: u32_arg(&args, "limit")?,
        })
        .await?;
    tracing::debug!(
        total_logs = response.total_logs,
        error_count = response.error_logs.len(),
        "incident_context completed"
    );
    Ok(serde_json::to_value(response)?)
}

/// Parse an optional RFC3339 timestamp string and normalize it to UTC.
///
/// Returns `Ok(None)` when `raw` is `None`. Returns a descriptive error when
/// `raw` is `Some` but not valid RFC3339 — callers get a clear message rather
/// than a silent wrong-result query against UTC-stored timestamps.
#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
