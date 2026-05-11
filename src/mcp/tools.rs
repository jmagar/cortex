use serde_json::{json, Value};

use crate::app::{
    AnomaliesRequest, ClockSkewRequest, CompareRequest, ContextRequest, CorrelateEventsRequest,
    GetErrorsRequest, GetLogRequest, IngestRateRequest, ListAppsRequest, ListSessionsRequest,
    PatternsRequest, SearchLogsRequest, SilentHostsRequest, TailLogsRequest, TimelineRequest,
};

use super::schemas::SYSLOG_ACTIONS;
use super::AppState;

/// Execute a tool by name
pub(super) async fn execute_tool(
    state: &AppState,
    name: &str,
    args: Value,
) -> anyhow::Result<Value> {
    match name {
        "syslog" => tool_syslog(state, args).await,
        _ => Err(anyhow::anyhow!("Unknown tool: {name}")),
    }
}

async fn tool_syslog(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let action =
        string_arg(&args, "action").ok_or_else(|| anyhow::anyhow!("action is required"))?;
    match action.as_str() {
        "search" => tool_search_logs(state, args).await,
        "tail" => tool_tail_logs(state, args).await,
        "errors" => tool_get_errors(state, args).await,
        "hosts" => tool_list_hosts(state, args).await,
        "correlate" => tool_correlate_events(state, args).await,
        "stats" => tool_get_stats(state, args).await,
        "status" => tool_get_status(state, args).await,
        "apps" => tool_list_apps(state, args).await,
        "sessions" => tool_list_sessions(state, args).await,
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
        "help" => tool_syslog_help().await,
        _ => Err(anyhow::anyhow!(
            "unknown syslog action: {action}; expected one of {}",
            SYSLOG_ACTIONS.join(", ")
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
            process_id: string_arg(&args, "process_id"),
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            limit: u32_arg(&args, "limit")?,
        })
        .await?;
    tracing::debug!(result_count = response.count, "search_logs completed");
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
        })
        .await?;
    tracing::debug!(app_count = response.apps.len(), "list_apps completed");
    Ok(serde_json::to_value(response)?)
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

async fn tool_list_source_ips(state: &AppState, _args: Value) -> anyhow::Result<Value> {
    let response = state.service.list_source_ips().await?;
    tracing::debug!(
        source_ip_count = response.source_ips.len(),
        "list_source_ips completed"
    );
    Ok(serde_json::to_value(response)?)
}

async fn tool_timeline(state: &AppState, args: Value) -> anyhow::Result<Value> {
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
    let response = state
        .service
        .patterns(PatternsRequest {
            from: string_arg(&args, "from"),
            to: string_arg(&args, "to"),
            hostname: string_arg(&args, "hostname"),
            app_name: string_arg(&args, "app_name"),
            severity_min: string_arg(&args, "severity_min"),
            scan_limit: u32_arg(&args, "scan_limit")?,
            top_n: u32_arg(&args, "top_n")?,
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
    Ok(json!({
        "status": if db_ok { "ok" } else { "error" },
        "db_ok": db_ok,
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

async fn tool_syslog_help() -> anyhow::Result<Value> {
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
- `source_ip` (string, optional) — filter by exact source identifier. Syslog uses verified `IP:port`; Docker ingest uses `docker://host/container/stream`.
- `severity` (string, optional) — one of: `emerg`, `alert`, `crit`, `err`, `warning`, `notice`, `info`, `debug`
- `app_name` (string, optional) — filter by application name, e.g. `sshd`, `dockerd`, `kernel`
- `facility` (string, optional) — filter by syslog facility name (e.g. `kern`, `auth`, `daemon`)
- `process_id` (string, optional) — filter by process_id (exact match)
- `from` (string, optional) — start of time range (ISO 8601 / RFC3339, e.g. `2025-01-15T00:00:00Z`)
- `to` (string, optional) — end of time range (ISO 8601)
- `limit` (integer, optional) — max results (default 100, max 1000)

---

## syslog tail
Get the N most recent log entries, optionally filtered by host, application, and/or severity floor.
Equivalent to `tail -f` across all hosts.

**Parameters:**
- `hostname` (string, optional) — filter to a specific host
- `source_ip` (string, optional) — filter by exact source identifier. Syslog uses verified `IP:port`; Docker ingest uses `docker://host/container/stream`.
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

---

## syslog hosts
List all hosts that have sent syslog messages, with first/last seen timestamps and total log counts.

**Parameters:** none

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

## syslog source_ips
List distinct source identifiers (network sender IP:port for syslog input,
`docker://host/container/stream` for Docker ingest) with log counts, the number
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
- `source_ip` (string, optional) — limit correlation to an exact source identifier. Syslog uses verified `IP:port`; Docker ingest uses `docker://host/container/stream`.
- `query` (string, optional) — optional FTS query to narrow results
- `limit` (integer, optional) — max total events to return (default 500, max 999)

---

## syslog timeline
Bucketed log counts over a time range. Use to answer "when did errors start"
or "is the incident still active". Each point reports `{bucket, group?, count}`.

**Parameters:**
- `bucket` (string, optional) — `minute`, `hour` (default), or `day`
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

## syslog help
Returns this markdown documentation.

**Parameters:** none
"#;
    Ok(json!({ "help": help }))
}

/// Parse an optional RFC3339 timestamp string and normalize it to UTC.
///
/// Returns `Ok(None)` when `raw` is `None`. Returns a descriptive error when
/// `raw` is `Some` but not valid RFC3339 — callers get a clear message rather
/// than a silent wrong-result query against UTC-stored timestamps.
#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
