use crate::db::SEVERITY_LEVELS;
use serde_json::{json, Value};

pub(super) const SYSLOG_ACTIONS: &[&str] = &[
    "search",
    "tail",
    "errors",
    "hosts",
    "apps",
    "sessions",
    "search_sessions",
    "cuss",
    "ai_correlate",
    "usage_blocks",
    "project_context",
    "list_ai_tools",
    "list_ai_projects",
    "correlate",
    "stats",
    "status",
    "source_ips",
    "timeline",
    "patterns",
    "context",
    "get",
    "ingest_rate",
    "silent_hosts",
    "clock_skew",
    "anomalies",
    "compare",
    "compose_status",
    "compose_doctor",
    "help",
];

/// Define the public MCP tool surface.
pub(super) fn tool_definitions() -> Vec<Value> {
    vec![json!({
        "name": "syslog",
        "description": "Query syslog-mcp logs with action-based subcommands: syslog search, syslog tail, syslog errors, syslog hosts, syslog correlate, syslog stats, syslog status, syslog apps, syslog sessions, syslog search_sessions, syslog cuss, syslog ai_correlate, syslog usage_blocks, syslog project_context, syslog list_ai_tools, syslog list_ai_projects, syslog source_ips, syslog timeline, syslog patterns, syslog context, syslog get, syslog ingest_rate, syslog silent_hosts, syslog clock_skew, syslog anomalies, syslog compare, syslog compose_status, syslog compose_doctor, and syslog help.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": SYSLOG_ACTIONS,
                    "description": "Action to run. Supported actions are listed in the enum."
                },
                "query": {
                    "type": "string",
                    "description": "For action=search, search_sessions, or correlate: FTS5 query. Examples: 'kernel panic', 'OOM AND killer', '\"connection refused\"', 'error*'. Hyphen is the FTS5 NOT operator; search hyphenated terms as phrases, e.g. '\"smoke-test\"'. Use ai_query/log_query for action=ai_correlate."
                },
                "hostname": {
                    "type": "string",
                    "description": "For action=search, tail, correlate, ai_correlate, apps, sessions, timeline, patterns, or context: exact hostname filter. Use action=hosts to enumerate."
                },
                "project": {
                    "type": "string",
                    "description": "For action=sessions, search_sessions, cuss, ai_correlate, usage_blocks, project_context, or list_ai_tools: exact project path, e.g. /home/jmagar/workspace/syslog-mcp."
                },
                "tool": {
                    "type": "string",
                    "enum": ["claude", "codex", "gemini"],
                    "description": "For action=sessions, search_sessions, cuss, ai_correlate, usage_blocks, project_context, or list_ai_projects: AI tool filter."
                },
                "source_ip": {
                    "type": "string",
                    "description": "For action=search, tail, correlate, or ai_correlate: exact source identifier. Syslog uses IP:port; OTLP uses peer IP; Docker stream rows use docker://host/container/stream; Docker lifecycle rows use docker-event://host/container/action."
                },
                "severity": {
                    "type": "string",
                    "enum": SEVERITY_LEVELS,
                    "description": "For action=search: syslog severity filter."
                },
                "severity_min": {
                    "type": "string",
                    "enum": SEVERITY_LEVELS,
                    "description": "For action=tail, correlate, ai_correlate, timeline, or patterns: minimum severity to include."
                },
                "app_name": {
                    "type": "string",
                    "description": "For action=search, tail, ai_correlate, timeline, or patterns: application name filter, e.g. sshd, dockerd, kernel."
                },
                "session_id": {
                    "type": "string",
                    "description": "For action=ai_correlate: exact AI session id filter."
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
                    "description": "For action=search: syslog facility filter, e.g. kern, auth, daemon."
                },
                "process_id": {
                    "type": "string",
                    "description": "For action=search: exact process_id filter."
                },
                "from": {
                    "type": "string",
                    "description": "For action=search, sessions, search_sessions, cuss, ai_correlate, usage_blocks, list_ai_tools, list_ai_projects, errors, timeline, or patterns: start of time range as ISO 8601/RFC3339."
                },
                "to": {
                    "type": "string",
                    "description": "For action=search, sessions, search_sessions, cuss, ai_correlate, usage_blocks, list_ai_tools, list_ai_projects, errors, timeline, or patterns: end of time range as ISO 8601/RFC3339."
                },
                "limit": {
                    "type": "integer",
                    "description": "For action=search: max results, default 100, max 1000. For action=sessions: max results, default 100, max 1000. For action=search_sessions: max grouped results, default 20, max 100 and returns total_candidates, candidate_rows, candidate_cap, candidate_window_truncated, and truncated. For action=cuss: max matches, default 20, max 100, each with same-session context. For action=ai_correlate: max AI anchors, default 10, max 50. For action=project_context: recent representative entries, default 5, max 20 with 256-char message snippets and recent_entries_truncated. For action=list_ai_tools/list_ai_projects: inventory results are capped at 100/200 and include total/truncated metadata. For action=correlate: max total events, default 500, max 999."
                },
                "n": {
                    "type": "integer",
                    "description": "For action=tail: number of recent entries, default 50, max 500."
                },
                "reference_time": {
                    "type": "string",
                    "description": "For action=correlate: required center timestamp for the correlation window as ISO 8601/RFC3339."
                },
                "window_minutes": {
                    "type": "integer",
                    "description": "For action=correlate: minutes before and after reference_time to search, default 5, max 60. For action=ai_correlate: minutes before and after each AI anchor, default 5, max 120."
                },
                "group_by": {
                    "type": "string",
                    "enum": ["app_name", "hostname", "host", "severity", "sev", "app"],
                    "description": "For action=errors: app_name. For action=timeline: hostname, severity, or app_name."
                },
                "bucket": {
                    "type": "string",
                    "enum": ["minute", "min", "m", "hour", "h", "day", "d"],
                    "description": "For action=timeline: time bucket size."
                },
                "scan_limit": {
                    "type": "integer",
                    "description": "For action=patterns: max messages to scan, default 10000, max 50000."
                },
                "top_n": {
                    "type": "integer",
                    "description": "For action=patterns: max templates to return, default 20, max 200."
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
                    "description": "For action=context: entries before the reference, default 10, max 500. For action=cuss: same-session entries before each hit, default 2, max 20."
                },
                "after": {
                    "type": "integer",
                    "description": "For action=context: entries after the reference, default 10, max 500. For action=cuss: same-session entries after each hit, default 2, max 20."
                },
                "terms": {
                    "oneOf": [
                        {"type": "array", "items": {"type": "string"}},
                        {"type": "string"}
                    ],
                    "description": "For action=cuss: optional custom cuss terms. Defaults to the built-in profanity list. String form is accepted for CLI bridges that cannot send arrays."
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
                    "description": "For action=clock_skew: sample entries with received_at >= since."
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
                }
            },
            "required": ["action"]
        }
    })]
}

#[cfg(test)]
#[path = "schemas_tests.rs"]
mod tests;
