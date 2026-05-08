use crate::db::SEVERITY_LEVELS;
use serde_json::{json, Value};

pub(super) const SYSLOG_ACTIONS: &[&str] = &[
    "search",
    "tail",
    "errors",
    "hosts",
    "correlate",
    "stats",
    "status",
    "apps",
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
    "help",
];

/// Define the public MCP tool surface.
pub(super) fn tool_definitions() -> Vec<Value> {
    vec![json!({
        "name": "syslog",
        "description": "Query syslog-mcp logs with action-based subcommands: syslog search, syslog tail, syslog errors, syslog hosts, syslog correlate, syslog stats, syslog status, syslog apps, syslog source_ips, syslog timeline, syslog patterns, syslog context, syslog get, syslog ingest_rate, syslog silent_hosts, syslog clock_skew, syslog anomalies, syslog compare, and syslog help.",
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
                    "description": "For action=search or action=correlate: FTS5 query. Examples: 'kernel panic', 'OOM AND killer', '\"connection refused\"', 'error*'."
                },
                "hostname": {
                    "type": "string",
                    "description": "For action=search, tail, correlate, apps, timeline, patterns, or context: exact hostname filter. Use action=hosts to enumerate."
                },
                "source_ip": {
                    "type": "string",
                    "description": "For action=search, tail, or correlate: exact source identifier. Syslog uses IP:port; Docker ingest uses docker://host/container/stream."
                },
                "severity": {
                    "type": "string",
                    "enum": SEVERITY_LEVELS,
                    "description": "For action=search: syslog severity filter."
                },
                "severity_min": {
                    "type": "string",
                    "enum": SEVERITY_LEVELS,
                    "description": "For action=tail, correlate, timeline, or patterns: minimum severity to include."
                },
                "app_name": {
                    "type": "string",
                    "description": "For action=search, tail, timeline, or patterns: application name filter, e.g. sshd, dockerd, kernel."
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
                    "description": "For action=search, errors, timeline, or patterns: start of time range as ISO 8601/RFC3339."
                },
                "to": {
                    "type": "string",
                    "description": "For action=search, errors, timeline, or patterns: end of time range as ISO 8601/RFC3339."
                },
                "limit": {
                    "type": "integer",
                    "description": "For action=search: max results, default 100, max 1000. For action=correlate: max total events, default 500, max 999."
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
                    "description": "For action=correlate: minutes before and after reference_time to search, default 5, max 60."
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
                    "description": "For action=context: entries before the reference, default 10, max 500."
                },
                "after": {
                    "type": "integer",
                    "description": "For action=context: entries after the reference, default 10, max 500."
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
