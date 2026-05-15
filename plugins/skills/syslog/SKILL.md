---
name: syslog
description: This skill should be used when the user asks to "search logs", "check errors", "tail logs", "show recent logs", "find log entries", "correlate events", "list hosts", "log stats", "syslog", "check homelab logs", or mentions system logs, syslog, log analysis, or log intelligence across homelab hosts.
---

# Syslog Skill

Rust-based syslog receiver and MCP server for homelab log intelligence. Receives RFC 3164/5424 syslog from all homelab hosts, stores in SQLite with FTS5 full-text search, and exposes one MCP tool with action dispatch for AI-driven log analysis.

## Tool

A single MCP tool, `mcp__syslog__syslog`, dispatches on a required `action` argument:

| action | purpose |
|--------|---------|
| `search` | Full-text search with FTS5 |
| `tail` | Most recent entries |
| `errors` | Error/warning summary by host and severity |
| `hosts` | List all known hosts with first/last seen |
| `sessions` | AI transcript sessions by project |
| `search_sessions` | Ranked grouped session search |
| `cuss` | Profanity hits in AI transcripts with same-session context |
| `ai_correlate` | AI transcript anchors cross-referenced against non-AI logs |
| `usage_blocks` | AI activity in 5-hour windows |
| `project_context` | Summary for one AI project path |
| `list_ai_tools` | Distinct AI tools with counts |
| `list_ai_projects` | Distinct AI projects with counts |
| `correlate` | Cross-host event correlation in a time window |
| `stats` | Database statistics |
| `status` | Lightweight runtime and DB health |
| `apps` | Distinct app names with log and host counts |
| `source_ips` | Source identifiers with hostname breakdown |
| `timeline` | Bucketed counts over time |
| `patterns` | Near-duplicate message template clusters |
| `context` | Surrounding logs around an event |
| `get` | One log by id, including raw frame |
| `ingest_rate` | Recent ingest throughput |
| `silent_hosts` | Hosts quiet beyond a threshold |
| `clock_skew` | Per-host clock skew distribution |
| `anomalies` | Recent vs baseline volume/error comparison |
| `compare` | Compare two time ranges |
| `compose_status` | Redacted read-only Compose deployment diagnostics |
| `compose_doctor` | Alias for Compose deployment health diagnostics |
| `help` | Canonical in-tree action reference (use as ground truth if this doc drifts) |

**Always prefer the MCP tool**. Fall back to HTTP only when MCP is unavailable.

The skill works identically in both server mode (this machine hosts the receiver) and client mode (connects to a remote server) — the connection details are configured at plugin install time. Source identity is captured per log: syslog entries carry the verified network sender as `IP:port`; Docker socket-proxy ingested entries carry `docker://host/container/stream`.

---

## Action Reference

### `action="search"` — Full-text log search

FTS5 search across all syslog messages with porter stemming.

| param | type | description |
|-------|------|-------------|
| `query` | string | FTS5 query (AND, OR, NOT, phrase, prefix*) |
| `hostname` | string | Exact hostname match |
| `source_ip` | string | Exact source identifier — `IP:port` for syslog, `docker://host/container/stream` for Docker ingest. The only network-verified identity (hostname can be spoofed in CEF/UniFi messages). |
| `severity` | string | One of: emerg, alert, crit, err, warning, notice, info, debug |
| `app_name` | string | Filter by application (e.g. sshd, dockerd, kernel) |
| `facility` | string | Filter by syslog facility |
| `process_id` | string | Filter by process id from the syslog frame |
| `from` | string | Start time (ISO 8601, e.g. `2025-01-15T00:00:00Z`) |
| `to` | string | End time (ISO 8601) |
| `limit` | integer | Max results (default 100, max 1000) |

**Examples** (illustrative — invoke via the MCP tool, not as code):

```
mcp__syslog__syslog(action="search", query="kernel panic")
mcp__syslog__syslog(action="search", query="OOM AND killer", limit=50)
mcp__syslog__syslog(action="search", query='"authentication failure"')
mcp__syslog__syslog(action="search", query="error", hostname="unraid", severity="err")
mcp__syslog__syslog(action="search", facility="auth", process_id="1234", limit=20)
mcp__syslog__syslog(action="search", query="connection refused",
                    from="2025-01-15T00:00:00Z", to="2025-01-15T23:59:59Z")
mcp__syslog__syslog(action="search", query="docker*")
```

**FTS5 syntax:**
- `AND`, `OR`, `NOT` — boolean operators (uppercase)
- `"phrase here"` — phrase match
- `term*` — prefix match
- **Hyphen `-` is the NOT operator** — to search hyphenated terms use phrase: `"smoke-test"` not `smoke-test`
- Invalid FTS5 syntax returns a db error

---

### `action="tail"` — Recent entries

Most recent N log entries across all hosts, like `tail -f` but multi-host.

| param | type | description |
|-------|------|-------------|
| `hostname` | string | Filter to a specific host |
| `source_ip` | string | Filter by exact source identifier (see `search`) |
| `app_name` | string | Filter to a specific application |
| `severity_min` | string | Minimum severity to return; `warning` returns warning and worse |
| `n` | integer | Number of entries (default 50, max 500) |

```
mcp__syslog__syslog(action="tail", n=20)
mcp__syslog__syslog(action="tail", hostname="unraid", n=50)
mcp__syslog__syslog(action="tail", app_name="dockerd", n=30)
mcp__syslog__syslog(action="tail", severity_min="warning", n=50)
```

---

### `action="errors"` — Error/warning summary

Errors and warnings grouped by hostname and severity with counts. Best for quick health assessments. Returns only: emerg, alert, crit, err, warning.

| param | type | description |
|-------|------|-------------|
| `from` | string | Start time (ISO 8601). Defaults to all time. |
| `to` | string | End time (ISO 8601). Defaults to now. |
| `group_by` | string | Optional secondary grouping. Currently supports `app_name`. |

```
mcp__syslog__syslog(action="errors")
mcp__syslog__syslog(action="errors", from="2025-01-15T13:00:00Z", to="2025-01-15T14:00:00Z")
mcp__syslog__syslog(action="errors", group_by="app_name")
```

**Response shape:**
```json
{
  "summary": [
    {"hostname": "unraid", "severity": "err", "count": 42},
    {"hostname": "unraid", "severity": "crit", "count": 3}
  ]
}
```

---

### `action="hosts"` — All known hosts

List every host that has sent syslog messages, with first/last seen and total log count.

```
mcp__syslog__syslog(action="hosts")
```

**Response shape:**
```json
{
  "hosts": [
    {
      "hostname": "unraid",
      "log_count": 145230,
      "first_seen": "2024-10-01T00:00:00Z",
      "last_seen": "2025-01-15T14:30:00Z"
    }
  ]
}
```

---

### `action="sessions"` — AI transcript sessions

List AI sessions grouped by project path and tool.

| param | type | description |
|-------|------|-------------|
| `project` | string | Exact project path filter |
| `tool` | string | AI tool filter: `claude`, `codex`, or `gemini` |
| `hostname` | string | Filter by hostname |
| `from` | string | Start time (ISO 8601) |
| `to` | string | End time (ISO 8601) |
| `limit` | integer | Max sessions (default 100, max 1000) |

```
mcp__syslog__syslog(action="sessions")
mcp__syslog__syslog(action="sessions", project="/home/jmagar/workspace/syslog-mcp")
mcp__syslog__syslog(action="sessions", tool="codex", limit=20)
```

**Response shape:**
```json
{
  "count": 1,
  "sessions": [
    {
      "project": "/home/jmagar/workspace/syslog-mcp",
      "tool": "codex",
      "session_id": "019e1506-dc81-7881-9926-4d6d4efda1ac",
      "hostname": "dookie",
      "first_seen": "2026-05-11T03:13:51.745Z",
      "last_seen": "2026-05-11T04:10:00.000Z",
      "event_count": 42
    }
  ]
}
```

---

### `action="cuss"` — AI transcript cuss detector

Detect profanity in AI transcript rows and return each hit with surrounding
rows from the same AI session.

| param | type | description |
|-------|------|-------------|
| `project` | string | Exact project path filter |
| `tool` | string | AI tool filter |
| `from` | string | Start time (ISO 8601) |
| `to` | string | End time (ISO 8601) |
| `limit` | integer | Max matches (default 20, max 100) |
| `before` | integer | Same-session rows before each hit (default 2, max 20) |
| `after` | integer | Same-session rows after each hit (default 2, max 20) |
| `terms` | array or string | Optional custom detector terms; replaces the built-in profanity list |

```text
mcp__syslog__syslog(action="cuss", project="/home/jmagar/workspace/syslog-mcp", limit=10)
mcp__syslog__syslog(action="cuss", tool="codex", terms=["dang", "heck"], before=3, after=3)
```

**Response shape:**

```json
{
  "terms": ["shit"],
  "candidate_rows": 1,
  "candidate_cap": 10000,
  "candidate_window_truncated": false,
  "truncated": false,
  "matches": [
    {"term": "shit", "entry": {"message": "..."}, "before": [], "after": []}
  ]
}
```

---

### `action="correlate"` — Cross-host event correlation

Find related events across all hosts within a time window. Ideal for debugging cascading failures.

| param | type | description |
|-------|------|-------------|
| `reference_time` | string | **Required.** Center timestamp (ISO 8601) |
| `window_minutes` | integer | Minutes before/after reference (default 5, max 60) |
| `severity_min` | string | Minimum severity (default `warning`) |
| `hostname` | string | Limit to a specific host |
| `source_ip` | string | Limit to an exact source identifier (see `search`) |
| `query` | string | Optional FTS5 query to narrow results |
| `limit` | integer | Max total events (default 500, max 999) |

```
mcp__syslog__syslog(action="correlate", reference_time="2025-01-15T14:30:00Z", window_minutes=10)
mcp__syslog__syslog(action="correlate", reference_time="2025-01-15T14:30:00Z",
                    window_minutes=30, severity_min="crit")
mcp__syslog__syslog(action="correlate", reference_time="2025-01-15T14:30:00Z",
                    hostname="unraid", query="OOM")
```

**Response shape:**
```json
{
  "reference_time": "2025-01-15T14:30:00Z",
  "window_minutes": 10,
  "total_events": 23,
  "truncated": false,
  "hosts_count": 3,
  "hosts": [
    {"hostname": "unraid", "event_count": 12, "events": [...]}
  ]
}
```

`limit` is silently capped at 999 because the implementation fetches `limit+1` rows to detect truncation.

---

### `action="stats"` — Database statistics

```
mcp__syslog__syslog(action="stats")
```

**Response fields:** `total_logs`, `total_hosts`, `oldest_log`, `newest_log`, `logical_db_size_mb`, `physical_db_size_mb`, `free_disk_mb`, `write_blocked`, plus configured threshold values.

---

### `action="status"` — Lightweight runtime status

Use this for dashboards and doctor checks that need current queue depth,
backpressure, writer failure/drop state, listener counters, and OTLP receiver
counters without the heavier DB statistics query.

```
mcp__syslog__syslog(action="status")
```

**Response fields:** `status`, `db_ok`, `runtime_observability`, and `otlp`.

---

### `action="apps"` — Applications seen in logs

Distinct non-empty `app_name` values with log count, host count, and first/last received times.

| param | type | description |
|-------|------|-------------|
| `hostname` | string | Optional host filter |

```
mcp__syslog__syslog(action="apps")
mcp__syslog__syslog(action="apps", hostname="unraid")
```

---

### `action="source_ips"` — Source identity breakdown

Network sender identifiers with counts and the top hostnames each source claimed.

```
mcp__syslog__syslog(action="source_ips")
```

Use `source_ip` as the trusted sender identity when a syslog hostname can be spoofed.

---

### `action="timeline"` — Bucketed log counts

Counts logs into minute, hour, or day buckets, optionally grouped by host, severity, or app.

| param | type | description |
|-------|------|-------------|
| `bucket` | string | `minute`, `hour`, or `day` (default `hour`) |
| `group_by` | string | Optional: `hostname`, `severity`, or `app_name` |
| `from` | string | Start time (ISO 8601) |
| `to` | string | End time (ISO 8601) |
| `hostname` | string | Exact host filter |
| `app_name` | string | Exact app filter |
| `severity_min` | string | Minimum severity to include |

```
mcp__syslog__syslog(action="timeline", bucket="hour", group_by="severity")
mcp__syslog__syslog(action="timeline", bucket="minute", severity_min="warning")
```

---

### `action="patterns"` — Message template clusters

Clusters near-duplicate messages after normalizing variable values such as numbers, IPs, UUIDs, and long hex strings.

| param | type | description |
|-------|------|-------------|
| `from` | string | Start time (ISO 8601) |
| `to` | string | End time (ISO 8601) |
| `hostname` | string | Exact host filter |
| `app_name` | string | Exact app filter |
| `severity_min` | string | Minimum severity to include |
| `scan_limit` | integer | Rows scanned before clustering (default 10000, max 50000) |
| `top_n` | integer | Max returned templates (default 20, max 200) |

```
mcp__syslog__syslog(action="patterns", severity_min="warning", top_n=10)
```

---

### `action="context"` — Surrounding logs

Returns logs before and after a reference event. Anchor by `log_id` when available, or by `hostname` plus `timestamp`.

| param | type | description |
|-------|------|-------------|
| `log_id` | integer | Stable row id anchor |
| `hostname` | string | Required with `timestamp` when `log_id` is omitted |
| `timestamp` | string | Required with `hostname` when `log_id` is omitted |
| `before` | integer | Entries before the reference (default 10, max 500) |
| `after` | integer | Entries after the reference (default 10, max 500) |

```
mcp__syslog__syslog(action="context", log_id=123, before=20, after=20)
```

---

### `action="get"` — Fetch one log by id

Returns a single log entry including its raw frame.

| param | type | description |
|-------|------|-------------|
| `id` | integer | Required log row id |

```
mcp__syslog__syslog(action="get", id=123)
```

---

### `action="ingest_rate"` — Recent ingest throughput

Returns last 1m/5m/15m ingest counts and per-second rates, plus write-block state.

| param | type | description |
|-------|------|-------------|
| `by_host` | boolean | Include per-host bucket counts |

```
mcp__syslog__syslog(action="ingest_rate")
mcp__syslog__syslog(action="ingest_rate", by_host=true)
```

---

### `action="silent_hosts"` — Quiet hosts

Lists hosts whose `last_seen` is older than the configured threshold.

| param | type | description |
|-------|------|-------------|
| `silent_minutes` | integer | Silence threshold (default 30, max 10080) |

```
mcp__syslog__syslog(action="silent_hosts", silent_minutes=60)
```

---

### `action="clock_skew"` — Sender clock skew

Compares each host's syslog timestamp with server receive time.

| param | type | description |
|-------|------|-------------|
| `since` | string | Start time for samples. Defaults to the last 24 hours. |

```
mcp__syslog__syslog(action="clock_skew")
```

---

### `action="anomalies"` — Recent vs baseline host activity

Compares recent per-host volume and error counts against a preceding baseline window. Hosts with recent logs and zero baseline sort to the top as new active hosts.

| param | type | description |
|-------|------|-------------|
| `recent_minutes` | integer | Recent window (default 15, max 1440) |
| `baseline_minutes` | integer | Baseline window before recent window (default 360, max 10080) |

```
mcp__syslog__syslog(action="anomalies", recent_minutes=30, baseline_minutes=720)
```

---

### `action="compare"` — Compare two ranges

Compares total logs, total errors, severity distribution, top hosts, and top apps across two time ranges.

| param | type | description |
|-------|------|-------------|
| `a_from` | string | Required start of range A |
| `a_to` | string | Required end of range A |
| `b_from` | string | Required start of range B |
| `b_to` | string | Required end of range B |

```
mcp__syslog__syslog(action="compare",
                    a_from="2025-01-15T00:00:00Z",
                    a_to="2025-01-15T12:00:00Z",
                    b_from="2025-01-15T12:00:00Z",
                    b_to="2025-01-16T00:00:00Z")
```

---

### `action="help"` — Canonical reference

Returns the authoritative in-tree action documentation. Use this as ground truth if the rest of this skill appears stale.

```
mcp__syslog__syslog(action="help")
```

---

## HTTP Fallback Mode

Use only when the MCP tool is unavailable. The plugin exports connection settings to Bash subprocesses as:

- `CLAUDE_PLUGIN_OPTION_SERVER_URL` — base URL (e.g. `http://localhost:3100`)
- `CLAUDE_PLUGIN_OPTION_API_TOKEN` — bearer token

**Sensitive value handling:** `api_token` is declared `sensitive: true` in the plugin manifest. It is **never** substituted into skill content as `${user_config.api_token}` — only the env var path above is valid. Do not inline the token in this document or any skill text.

### Health check (no auth required)

```bash
# /health is unauthenticated — no Authorization header needed
curl -s "$CLAUDE_PLUGIN_OPTION_SERVER_URL/health"
```

### Tail logs

```bash
curl -s -X POST "$CLAUDE_PLUGIN_OPTION_SERVER_URL/mcp" \
  -H "Authorization: Bearer $CLAUDE_PLUGIN_OPTION_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"syslog","arguments":{"action":"tail","n":20}}}'
```

### Search logs

```bash
curl -s -X POST "$CLAUDE_PLUGIN_OPTION_SERVER_URL/mcp" \
  -H "Authorization: Bearer $CLAUDE_PLUGIN_OPTION_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"syslog","arguments":{"action":"search","query":"error","limit":20}}}'
```

### Get stats

```bash
curl -s -X POST "$CLAUDE_PLUGIN_OPTION_SERVER_URL/mcp" \
  -H "Authorization: Bearer $CLAUDE_PLUGIN_OPTION_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"syslog","arguments":{"action":"stats"}}}'
```

---

## Severity Levels

| Level | Numeric | Description |
|-------|---------|-------------|
| emerg | 0 | System unusable |
| alert | 1 | Immediate action required |
| crit | 2 | Critical condition |
| err | 3 | Error condition |
| warning | 4 | Warning condition |
| notice | 5 | Normal but significant |
| info | 6 | Informational |
| debug | 7 | Debug messages |

`errors` returns only emerg through warning. `correlate` defaults `severity_min` to `warning` (returns warning through emerg).

---

## Log Intelligence Workflows

### Quick homelab health check

```
mcp__syslog__syslog(action="errors")
mcp__syslog__syslog(action="tail", hostname="unraid", n=50)
mcp__syslog__syslog(action="search", query='OOM OR "out of memory"', hostname="unraid")
```

### Incident investigation

```
# 1. Find the incident
mcp__syslog__syslog(action="search", query='panic OR crash OR "segmentation fault"', limit=10)

# 2. Correlate across hosts at that timestamp
mcp__syslog__syslog(action="correlate",
                    reference_time="<timestamp from step 1>",
                    window_minutes=15,
                    severity_min="warning")

# 3. Confirm which hosts were active
mcp__syslog__syslog(action="hosts")
```

### Trace a specific Docker container's logs

```
# Docker ingest sets source_ip to docker://host/container/stream
mcp__syslog__syslog(action="search", source_ip="docker://squirts/postgres/stdout", limit=50)
```

### Storage health

```
mcp__syslog__syslog(action="stats")
# Check: write_blocked, logical_db_size_mb vs threshold, free_disk_mb vs threshold
```

---

## Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| 1514 | UDP + TCP | Syslog receiver |
| 3100 | TCP | MCP HTTP endpoint (`POST /mcp`, `GET /health`) |

Port 1514 (not 514) avoids needing `CAP_NET_BIND_SERVICE`. iptables PREROUTING redirects 514→1514 for devices that can't be reconfigured — that's deployment-time setup, not runtime.
