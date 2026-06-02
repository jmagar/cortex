---
name: cortex
description: This skill should be used when the user asks to "search logs", "check errors", "tail logs", "show recent logs", "find log entries", "correlate events", "list hosts", "log stats", "syslog", "check homelab logs", or mentions system logs, syslog, log analysis, or log intelligence across homelab hosts.
---

# Syslog Skill

Rust-based syslog receiver and MCP server for homelab log intelligence. Receives RFC 3164/5424 syslog from all homelab hosts, stores in SQLite with FTS5 full-text search, and exposes one MCP tool with action dispatch for AI-driven log analysis.

## Tool

A single MCP tool, `mcp__syslog__syslog`, dispatches on a required `action` argument:

| action | purpose |
|--------|---------|
| `search` | Full-text search with FTS5 |
| `filter` | Structured filter-only log retrieval |
| `tail` | Most recent entries |
| `errors` | Error/warning summary by host and severity |
| `hosts` | List all known hosts with first/last seen |
| `host_state` | Latest bounded heartbeat state for one host |
| `fleet_state` | Fleet-wide heartbeat snapshot with pressure flags and summary counts |
| `correlate_state` | Correlate logs with heartbeat summaries around a reference time |
| `sessions` | AI transcript sessions by project |
| `search_sessions` | Ranked grouped session search |
| `abuse` | Abuse hits in AI transcripts with same-session context |
| `abuse_incidents` | Groups abuse hits into scored incident candidates |
| `abuse_investigate` | Expands incidents into deterministic evidence bundles |
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
| `compose_doctor` | Strict Compose deployment health diagnostics |
| `unaddressed_errors` | List unacknowledged repeating error signatures |
| `ack_error` | Acknowledge an error signature |
| `unack_error` | Revoke an existing acknowledgement |
| `notifications_recent` | List recent notification firings |
| `notifications_test` | Send a test notification via Apprise |
| `similar_incidents` | FTS5 cluster search over historical system logs matching a query |
| `ask_history` | Search AI transcript history for sessions related to a topic |
| `incident_context` | Full log context bundle for a known time window |
| `graph` | Resolve graph entities and return bounded one-hop neighborhoods with evidence |
| `help` | Canonical in-tree action reference (use as ground truth if this doc drifts) |

**Always prefer the MCP tool**. Fall back to HTTP only when MCP is unavailable.

The skill works identically in both server mode (this machine hosts the receiver) and client mode (connects to a remote server) — the connection details are configured at plugin install time. Source identity is captured per log: syslog entries carry the verified network sender as `IP:port`; Docker socket-proxy ingested entries carry `docker://host/container/stream`.

---

## Action Reference

For parameter-level details and response shapes, use the live action reference:

```text
mcp__syslog__syslog(action="help")
```

When working from the repository instead of a live server, use `docs/mcp/TOOLS.md`, `docs/mcp/SCHEMA.md`, and `docs/mcp/CORRELATION.md` as the canonical references. Keep this skill focused on when to use the tool, safe invocation patterns, and common workflows rather than duplicating every action schema.

FTS5 reminders for `search` and other query-bearing actions:
- `AND`, `OR`, `NOT` are uppercase boolean operators.
- Quote phrases and hyphenated terms, for example `"smoke-test"`.
- Invalid FTS5 syntax returns a database error.

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
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"cortex","arguments":{"action":"tail","n":20}}}'
```

### Search logs

```bash
curl -s -X POST "$CLAUDE_PLUGIN_OPTION_SERVER_URL/mcp" \
  -H "Authorization: Bearer $CLAUDE_PLUGIN_OPTION_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"cortex","arguments":{"action":"search","query":"error","limit":20}}}'
```

### Get stats

```bash
curl -s -X POST "$CLAUDE_PLUGIN_OPTION_SERVER_URL/mcp" \
  -H "Authorization: Bearer $CLAUDE_PLUGIN_OPTION_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"cortex","arguments":{"action":"stats"}}}'
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
