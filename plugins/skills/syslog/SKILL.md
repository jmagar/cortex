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
| `correlate` | Cross-host event correlation in a time window |
| `stats` | Database statistics |
| `status` | Lightweight runtime and DB health |
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
| `from` | string | Start time (ISO 8601, e.g. `2025-01-15T00:00:00Z`) |
| `to` | string | End time (ISO 8601) |
| `limit` | integer | Max results (default 100, max 1000) |

**Examples** (illustrative — invoke via the MCP tool, not as code):

```
mcp__syslog__syslog(action="search", query="kernel panic")
mcp__syslog__syslog(action="search", query="OOM AND killer", limit=50)
mcp__syslog__syslog(action="search", query='"authentication failure"')
mcp__syslog__syslog(action="search", query="error", hostname="unraid", severity="err")
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
| `n` | integer | Number of entries (default 50, max 500) |

```
mcp__syslog__syslog(action="tail", n=20)
mcp__syslog__syslog(action="tail", hostname="unraid", n=50)
mcp__syslog__syslog(action="tail", app_name="dockerd", n=30)
```

---

### `action="errors"` — Error/warning summary

Errors and warnings grouped by hostname and severity with counts. Best for quick health assessments. Returns only: emerg, alert, crit, err, warning.

| param | type | description |
|-------|------|-------------|
| `from` | string | Start time (ISO 8601). Defaults to all time. |
| `to` | string | End time (ISO 8601). Defaults to now. |

```
mcp__syslog__syslog(action="errors")
mcp__syslog__syslog(action="errors", from="2025-01-15T13:00:00Z", to="2025-01-15T14:00:00Z")
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
