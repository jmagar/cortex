use crate::db::SEVERITY_LEVELS;
use serde_json::{json, Value};

use super::actions;

/// Define the public MCP tool surface.
///
/// The action `enum` list is derived from [`actions::ACTION_SPECS`] so it
/// stays in sync with the scope-gating table automatically. Previously it was
/// a separate `CORTEX_ACTIONS` const that could drift.
pub(super) fn tool_definitions() -> Vec<Value> {
    let action_names: Vec<&str> = actions::action_names();
    let action_metadata: Vec<Value> = actions::ACTION_SPECS
        .iter()
        .map(|spec| {
            json!({
                "name": spec.name,
                "cost": spec.cost.as_str(),
                "description": spec.description,
            })
        })
        .collect();
    let action_desc = action_names
        .iter()
        .map(|n| format!("cortex {n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let description = format!("Query cortex logs with action-based subcommands: {action_desc}.");
    vec![json!({
        "name": "cortex",
        "description": description,
        "x-cortex-action-metadata": action_metadata,
        "x-cortex-agent-guidance": {
            "cost_order": ["cheap", "moderate", "expensive", "write"],
            "first_pass": ["status", "errors", "tail", "search", "timeline", "context"],
            "escalate_only_when_scoped": ["stats", "patterns", "anomalies", "compare", "clock_skew", "ingest_rate", "compose_doctor"],
            "default_bounds": {
                "search_limit": 5,
                "summary_limit": 10,
                "context_before": 3,
                "context_after": 3,
                "timeline_bucket": "minute"
            }
        },
        "inputSchema": {
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": action_names,
                    "description": "Action to run. Supported actions are listed in the enum."
                },
                "query": {
                    "type": "string",
                    "description": "FTS5 query string. Required for action=similar_incidents (FTS5 match over system logs) and action=ask_history (FTS5 match over AI transcripts). Also used for action=search, search_sessions, and correlate. Examples: 'kernel panic', 'OOM AND killer', '\"connection refused\"', 'error*'. Hyphen is the FTS5 NOT operator; search hyphenated terms as phrases, e.g. '\"smoke-test\"'. Use ai_query/log_query for action=ai_correlate."
                },
                "mode": {
                    "type": "string",
                    "enum": ["entity", "around", "explain", "evidence"],
                    "description": "For action=graph: entity resolves an entity by key or alias; around returns a bounded one-hop neighborhood; explain returns conservative evidence-backed chains; evidence resolves one evidence row by evidence_id. Defaults to around."
                },
                "evidence_id": {
                    "type": "integer",
                    "description": "For action=graph mode=evidence: graph_relationship_evidence id to inspect."
                },
                "entity_id": {
                    "type": "integer",
                    "description": "For action=graph mode=around or explain: exact graph entity id to expand."
                },
                "entity_type": {
                    "type": "string",
                    "enum": ["host", "container", "service", "app", "source_ip", "ai_project", "ai_session", "error_signature"],
                    "description": "For action=graph: entity type for exact canonical-key lookup."
                },
                "key": {
                    "type": "string",
                    "description": "For action=graph with entity_type: canonical key/display value to resolve. Values are normalized server-side."
                },
                "alias_type": {
                    "type": "string",
                    "description": "For action=graph: alias kind for alias lookup, e.g. hostname or heartbeat_host_id."
                },
                "alias_key": {
                    "type": "string",
                    "description": "For action=graph: alias value to resolve. Ambiguous aliases return candidates instead of guessing."
                },
                "hostname": {
                    "type": "string",
                    "description": "For action=search, filter, tail, correlate, ai_correlate, apps, sessions, timeline, patterns, context, similar_incidents, ask_history, or incident_context: exact hostname filter. For action=host_state: resolve a host by unique hostname when host_id is omitted. Use action=hosts to enumerate."
                },
                "host_id": {
                    "type": "string",
                    "description": "For action=host_state: exact heartbeat host_id. Takes precedence over hostname and is authoritative over hostname metadata."
                },
                "host": {
                    "type": "string",
                    "description": "For action=correlate_state: optional host filter accepting a host_id or a unique hostname. When omitted, a bounded cross-host plan is used over all hosts with heartbeats in the window."
                },
                "include_ok": {
                    "type": "boolean",
                    "description": "For action=fleet_state: when false, hosts whose status is 'ok' are excluded. Default true."
                },
                "sort": {
                    "type": "string",
                    "enum": ["pressure", "freshness", "hostname"],
                    "description": "For action=fleet_state: row ordering. 'pressure' (default) ranks late > partial > pressure > ok; 'freshness' orders by most-recent heartbeat; 'hostname' is alphabetical."
                },
                "project": {
                    "type": "string",
                    "description": "For action=filter, sessions, search_sessions, abuse, ai_correlate, usage_blocks, project_context, or list_ai_tools: exact project path, e.g. /home/jmagar/workspace/cortex."
                },
                "tool": {
                    "type": "string",
                    "enum": ["claude", "codex", "gemini"],
                    "description": "For action=filter, sessions, search_sessions, abuse, ai_correlate, usage_blocks, project_context, or list_ai_projects: AI tool filter."
                },
                "source_ip": {
                    "type": "string",
                    "description": "For action=search, filter, tail, correlate, or ai_correlate: exact source identifier. Syslog uses IP:port; OTLP uses peer IP; Docker stream rows use docker://host/container/stream; Docker lifecycle rows use docker-event://host/container/action."
                },
                "source_kind": {
                    "type": "string",
                    "enum": ["docker-stream", "docker-event", "agent-command", "shell-history", "transcript", "claude", "codex", "gemini"],
                    "description": "For action=filter: structured source alias. syslog-udp, syslog-tcp, and otlp are rejected in v1 because transport is not indexed separately."
                },
                "severity": {
                    "type": "string",
                    "enum": SEVERITY_LEVELS,
                    "description": "For action=search or filter: syslog severity filter."
                },
                "severity_min": {
                    "type": "string",
                    "enum": SEVERITY_LEVELS,
                    "description": "For action=tail, correlate, correlate_state, ai_correlate, timeline, patterns, similar_incidents, or incident_context: minimum severity to include. Defaults to 'warning' for incident_context and 'info' for correlate_state."
                },
                "app_name": {
                    "type": "string",
                    "description": "For action=search, filter, tail, ai_correlate, timeline, patterns, similar_incidents, ask_history, or incident_context: application name filter, e.g. sshd, dockerd, kernel."
                },
                "session_id": {
                    "type": "string",
                    "description": "For action=filter or ai_correlate: exact AI session id filter."
                },
                "ai_query": {
                    "type": "string",
                    "description": "For action=ai_correlate: FTS5 query over AI transcript anchor rows."
                },
                "log_query": {
                    "type": "string",
                    "description": "For action=ai_correlate: FTS5 query over related non-AI logs inside each anchor window."
                },
                "facility": {
                    "type": "string",
                    "description": "For action=search or filter: syslog facility filter, e.g. kern, auth, daemon, clockd."
                },
                "exclude_facility": {
                    "type": "string",
                    "description": "For action=search or filter: exclude one syslog facility while retaining rows with unknown facility."
                },
                "process_id": {
                    "type": "string",
                    "description": "For action=search or filter: exact process_id filter."
                },
                "received_from": {
                    "type": "string",
                    "description": "For action=search or filter: filter rows with received_at >= this timestamp."
                },
                "received_to": {
                    "type": "string",
                    "description": "For action=search or filter: filter rows with received_at <= this timestamp."
                },
                "container": {
                    "type": "string",
                    "description": "For action=filter: Docker container/app name refiner."
                },
                "docker_host": {
                    "type": "string",
                    "description": "For action=filter with source_kind=docker-stream or docker-event: Docker host refiner."
                },
                "stream": {
                    "type": "string",
                    "enum": ["stdout", "stderr"],
                    "description": "For action=filter with source_kind=docker-stream: Docker log stream refiner."
                },
                "event_action": {
                    "type": "string",
                    "description": "For action=filter: Docker lifecycle/enrichment event action filter."
                },
                "from": {
                    "type": "string",
                    "description": "For action=search, filter, sessions, search_sessions, abuse, abuse_incidents, abuse_investigate, ai_correlate, usage_blocks, list_ai_tools, list_ai_projects, errors, timeline, patterns, apps, similar_incidents, or ask_history: start of time range as ISO 8601/RFC3339. Required for incident_context. For action=timeline: when both from and to are omitted, a bucket-sized default lookback window applies (≈7 days for hour, 30 for day, longer for week/month) — no full-history scan. Strongly recommended for patterns — omitting from/to causes a full-history scan."
                },
                "to": {
                    "type": "string",
                    "description": "For action=search, filter, sessions, search_sessions, abuse, abuse_incidents, abuse_investigate, ai_correlate, usage_blocks, list_ai_tools, list_ai_projects, errors, timeline, patterns, apps, similar_incidents, or ask_history: end of time range as ISO 8601/RFC3339. Required for incident_context. For action=timeline: a bucket-sized default lookback bounds the query when from/to are omitted (no full-history scan). Strongly recommended for patterns — omitting from/to causes a full-history scan."
                },
                "limit": {
                    "type": "integer",
                    "description": "For action=search or filter: max results, default 100, max 1000. For action=errors: max summary rows, max 100. For action=sessions: max results, default 100, max 1000. For action=search_sessions: max grouped results, default 20, max 100 and returns total_candidates, candidate_rows, candidate_cap, candidate_window_truncated, and truncated. For action=abuse: max matches, default 20, max 100, each with same-session context. For action=abuse_incidents: max incidents, default 20, max 100; response includes total_incidents, candidate_rows, truncated. For action=abuse_investigate: max incidents to expand into evidence bundles, default 3, max 10. For action=ai_correlate: max AI anchors, default 10, max 50. For action=project_context: recent representative entries, default 5, max 20 with 256-char message snippets and recent_entries_truncated. For action=list_ai_tools/list_ai_projects: inventory results are capped at 100/200 and include total/truncated metadata. For action=correlate: max total events, default 500, max 999. For action=correlate_state: max log rows per host, default 100, max 500. For action=host_state: max heartbeat samples, default 1, max 100. For action=patterns: alias for top_n, default 20, max 200. For action=clock_skew: max host rows, max 100. For action=apps: page size, default 500, max 5000; use with offset to paginate; response includes total count of all matching apps. For action=source_ips: page size, default 500, max 5000; use with offset to paginate; response includes total count of all distinct source IPs. For action=similar_incidents: max incident clusters, default 10, max 50. For action=ask_history: max sessions, default 10, max 50. For action=incident_context: max error log rows, default 50, max 200. For action=graph: alias candidate or relationship cap; entity lookup default 20 max 100, around default 100 max 500."
                },
                "depth": {
                    "type": "integer",
                    "description": "For action=graph mode=around or explain: traversal depth. Around supports depth=1 only; explain clamps to 1..3."
                },
                "evidence_sample_limit": {
                    "type": "integer",
                    "description": "For action=graph mode=around or explain: safe evidence samples per relationship, default 3 for around and 2 for explain, max 5."
                },
                "payload_budget": {
                    "type": "integer",
                    "description": "For action=graph: approximate response payload budget in bytes, default 32768, clamped 4096..65536."
                },
                "offset": {
                    "type": "integer",
                    "description": "For action=apps or source_ips: number of items to skip for pagination. Default 0. Use with limit to page through all results: if total > offset + limit, increment offset by limit to fetch the next page."
                },
                "n": {
                    "type": "integer",
                    "description": "For action=tail: number of recent entries, default 50, max 500."
                },
                "reference_time": {
                    "type": "string",
                    "description": "For action=correlate or correlate_state: required center timestamp for the correlation window as ISO 8601/RFC3339."
                },
                "window_minutes": {
                    "type": "integer",
                    "description": "For action=correlate: minutes before and after reference_time to search, default 5, max 60. For action=correlate_state: minutes before and after reference_time, default 10, max 120. For action=ai_correlate: minutes before and after each AI anchor, default 5, max 120. For action=abuse_incidents or abuse_investigate: incident grouping window, default 10, max 120. For action=similar_incidents: cluster grouping window in minutes, default 30, clamp 5..=120."
                },
                "correlation_window_minutes": {
                    "type": "integer",
                    "description": "For action=abuse_investigate: minutes before first and after last anchor for nearby non-AI log correlation, default 5, max 120."
                },
                "group_by": {
                    "type": "string",
                    "enum": ["app_name", "hostname", "host", "severity", "sev", "app"],
                    "description": "For action=errors: app_name. For action=timeline: hostname, severity, or app_name."
                },
                "bucket": {
                    "type": "string",
                    "enum": ["minute", "min", "m", "hour", "h", "day", "d", "week", "w", "month"],
                    "description": "For action=timeline: time bucket size (minute, hour, day, week, month)."
                },
                "scan_limit": {
                    "type": "integer",
                    "description": "For action=patterns: max messages to scan, default 10000, max 50000."
                },
                "top_n": {
                    "type": "integer",
                    "description": "For action=patterns: max templates to return, default 20, max 200. `limit` is accepted as an alias for agent/CLI ergonomics."
                },
                "log_id": {
                    "type": "integer",
                    "description": "For action=context: existing log id to anchor surrounding logs."
                },
                "timestamp": {
                    "type": "string",
                    "description": "For action=context: anchor timestamp when log_id is not provided."
                },
                "before": {
                    "type": "integer",
                    "description": "For action=context: entries before the reference, default 10, max 500. For action=abuse: same-session entries before each hit, default 2, max 20."
                },
                "after": {
                    "type": "integer",
                    "description": "For action=context: entries after the reference, default 10, max 500. For action=abuse: same-session entries after each hit, default 2, max 20."
                },
                "terms": {
                    "oneOf": [
                        {"type": "array", "items": {"type": "string"}},
                        {"type": "string"}
                    ],
                    "description": "For action=abuse: optional custom abuse terms. Defaults to the built-in abuse list. String form is accepted for CLI bridges that cannot send arrays."
                },
                "events_per_anchor": {
                    "type": "integer",
                    "description": "For action=ai_correlate: max non-AI related log events per AI anchor, default 25, max 200."
                },
                "id": {
                    "type": "integer",
                    "description": "For action=get: log id to fetch."
                },
                "by_host": {
                    "type": "boolean",
                    "description": "For action=ingest_rate: include per-host buckets."
                },
                "silent_minutes": {
                    "type": "integer",
                    "description": "For action=silent_hosts: staleness threshold, default 30, max 10080."
                },
                "since": {
                    "type": "string",
                    "description": "For action=clock_skew: sample entries with received_at >= since. Use `limit` to cap returned host rows. For action=host_state: only include heartbeat samples with sampled_at >= this ISO 8601/RFC3339 timestamp."
                },
                "recent_minutes": {
                    "type": "integer",
                    "description": "For action=anomalies: recent window, default 15, max 1440."
                },
                "baseline_minutes": {
                    "type": "integer",
                    "description": "For action=anomalies: baseline window before recent window, default 360, max 10080."
                },
                "a_from": {
                    "type": "string",
                    "description": "For action=compare: first range start as ISO 8601/RFC3339."
                },
                "a_to": {
                    "type": "string",
                    "description": "For action=compare: first range end as ISO 8601/RFC3339."
                },
                "b_from": {
                    "type": "string",
                    "description": "For action=compare: second range start as ISO 8601/RFC3339."
                },
                "b_to": {
                    "type": "string",
                    "description": "For action=compare: second range end as ISO 8601/RFC3339."
                },
                "include_acknowledged": {
                    "type": "boolean",
                    "description": "For action=unaddressed_errors: include already-acknowledged signatures. Default false."
                },
                "signature_hash": {
                    "type": "string",
                    "description": "For action=ack_error or unack_error: the SHA-256 signature hash to acknowledge or un-acknowledge."
                },
                "notes": {
                    "type": "string",
                    "description": "For action=ack_error: optional human-readable acknowledgement notes (max 4096 chars)."
                },
                "reason": {
                    "type": "string",
                    "description": "For action=unack_error: optional reason for removing acknowledgement (max 4096 chars)."
                }
            },
            "required": ["action"]
        }
    })]
}

#[cfg(test)]
#[path = "schemas_tests.rs"]
mod tests;
