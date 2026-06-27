# cortex

[![crates.io](https://img.shields.io/crates/v/cortex)](https://crates.io/crates/cortex) [![ghcr.io](https://img.shields.io/badge/ghcr.io-jmagar%2Fcortex-blue?logo=docker)](https://github.com/jmagar/cortex/pkgs/container/cortex)

Rust syslog receiver and MCP server for homelab log intelligence. Ingests syslog over UDP and TCP, stores it in SQLite with FTS5 full-text indexing, and exposes action-based log search, inventory, correlation, status, and analysis tools through MCP, REST, and CLI adapters backed by the shared service layer.

cortex also maintains derived projection tables for future investigation graph
features. Those graph tables connect source IPs, claimed hosts, apps, services,
containers, AI projects/sessions, and error signatures with evidence, but raw
logs, heartbeats, inventory, signatures, and session rows remain the source of
truth. The graph projection is rebuildable and intentionally has no ingest
triggers. Graph rebuilds use staging tables plus a short serialized swap and
record explicit projection status, source watermarks, row counts, runtime
metrics, and degraded failure state.

## Overview

```
                    ┌─────────────────────────────────┐
  rsyslog/syslog-ng ─▶  UDP :1514 / TCP :1514          │
  network devices   ─▶  ┌──────────────────────────┐   │
                    │   │  parse → batch writer     │   │
                    │   │  SQLite + FTS5 (WAL mode) │   │
                    │   └──────────────────────────┘   │
  Claude / MCP ◀──── ▶  RMCP HTTP :3100/mcp             │
  local MCP client ◀──▶  syslog mcp query process       │
                    └─────────────────────────────────┘
```

The daemon listens on a single port for both UDP and TCP syslog (default `1514`). All inbound messages are parsed, batched, and written to SQLite with full-text indexing. The MCP HTTP server runs on a separate port (default `3100`) and uses RMCP Streamable HTTP in stateless JSON-response mode. Local stdio-only MCP clients can launch `cortex mcp`, a query-only MCP process that reads the same SQLite database without starting syslog listeners or the HTTP server.

MCP is an exposure surface, not the owner of log-intelligence business policy. Shared defaults, limits, validation, audit identity, correlation behavior, and safety gates should live in `SyslogService` or service-owned operation models so MCP, REST, and CLI remain consistent.

---

## Tools

One MCP tool, `cortex`, is exposed. Use the required `action` argument to run `search`, `filter`, `tail`, `errors`, `hosts`, `map`, `sessions`, `search_sessions`, `abuse`, `abuse_incidents`, `abuse_investigate`, `ai_correlate`, `usage_blocks`, `project_context`, `list_ai_tools`, `list_ai_projects`, `correlate`, `stats`, `status`, `apps`, `source_ips`, `timeline`, `patterns`, `context`, `get`, `ingest_rate`, `silent_hosts`, `clock_skew`, `anomalies`, `compare`, `compose_status`, `compose_doctor`, `unaddressed_errors`, `ack_error`, `unack_error`, `notifications_recent`, `notifications_test`, `similar_incidents`, `ask_history`, `incident_context`, `graph`, or `help`.

For the complete action-specific parameter reference, see [`docs/mcp/SCHEMA.md`](docs/mcp/SCHEMA.md). For correlation behavior and AI/non-AI inclusion rules, see [`docs/mcp/CORRELATION.md`](docs/mcp/CORRELATION.md).

| Action | Purpose |
| --- | --- |
| `search` | Full-text search with filters |
| `filter` | Structured filter-only log retrieval |
| `tail` | Recent log entries |
| `errors` | Error/warning summary by host and severity |
| `hosts` | Host registry with first/last seen |
| `map` | Cached homelab inventory plus graph-backed topology answers |
| `sessions` | AI transcript sessions by project |
| `search_sessions` | Ranked grouped session search |
| `abuse` | Abuse hits in AI transcripts with same-session context |
| `abuse_incidents` | Groups abuse hits into scored incident candidates |
| `abuse_investigate` | Expands incidents into deterministic evidence bundles |
| `ai_correlate` | AI transcript anchors cross-referenced against non-AI logs |
| `usage_blocks` | AI activity in 5-hour UTC windows |
| `project_context` | Summary for one AI project path |
| `list_ai_tools` | Distinct AI tools with counts |
| `list_ai_projects` | Distinct AI projects with counts |
| `correlate` | Cross-host event correlation in a time window |
| `stats` | Database statistics and storage health |
| `status` | Lightweight runtime and DB health |
| `apps` | Distinct application names with log and host counts |
| `source_ips` | Distinct source identifiers with hostname breakdown |
| `timeline` | Bucketed counts over time |
| `patterns` | Near-duplicate message template clusters |
| `context` | Surrounding logs around a log id or timestamp |
| `get` | One log entry by id, including raw frame |
| `ingest_rate` | Recent ingest throughput and write-block state |
| `silent_hosts` | Hosts whose last_seen is older than a threshold |
| `clock_skew` | Per-host received_at minus timestamp distribution |
| `anomalies` | Recent vs baseline volume/error comparison |
| `compare` | Side-by-side comparison of two time ranges |
| `compose_status` | Redacted read-only Compose deployment diagnostics |
| `compose_doctor` | Strict Compose deployment health diagnostics |
| `unaddressed_errors` | Repeating unacknowledged error signatures |
| `ack_error` | Acknowledge an error signature |
| `unack_error` | Revoke an error acknowledgement |
| `notifications_recent` | Recent notification firings |
| `notifications_test` | Send a test notification via Apprise |
| `similar_incidents` | FTS5 cluster search over historical system logs |
| `ask_history` | Search AI transcript history with nearby log context |
| `incident_context` | Full context bundle for a known time window |
| `graph` | Resolve graph entities, neighborhoods, and evidence-backed explanations |
| `help` | Markdown reference for all actions |

## Homelab Inventory

`cortex inventory refresh --json` collects native Rust inventory into
`~/.cortex/inventory` and writes:

- `normalized/homelab.json` — typed `cortex.homelab_inventory.v1` cache
- `collection-state.json` — per-collector status, warnings, timings, and artifact refs
- `raw/<run_id>/*.txt` — raw-but-redacted Compose and reverse proxy artifacts

`cortex inventory status --json` reports cache freshness and warnings without
opening SQLite. The MCP `map` action is read-only: it reads the normalized cache
and overlays bounded live Cortex host/heartbeat data, but never triggers refresh
or returns raw artifact bodies.

`map` defaults to the inventory snapshot. Graph-backed modes add a typed
`graph_answer` envelope with `answer_status`, bounded topology `rows`, safe
evidence samples, map follow-up queries, and graph proof queries:

```json
{"action":"map","mode":"host_services","host":"squirts"}
{"action":"map","mode":"domain_routes","domain":"adguard.tootie.tv"}
{"action":"map","mode":"service_dependencies","host":"squirts","service":"swag"}
{"action":"map","mode":"findings","finding_types":["potential_public_route","risky_mounts","collector_health"]}
```

`mode=findings` returns bounded topology risk and hygiene findings derived from
the graph plus normalized inventory/cache state. Findings include severity,
confidence, reason code, affected entities, safe evidence IDs/excerpts, and
remediation hints. They deliberately avoid raw config contents, raw artifact or
cache paths, credential-bearing upstream URLs, and raw collector warning text;
`potential_public_route` means configured reverse-proxy routing, not proof of
unauthenticated public internet exposure.

When the server is running, inventory refresh also projects topology evidence
into the investigation graph. The baseline refresh interval is 5 minutes, with
local Compose/proxy config watchers as lower-latency refresh triggers. Remote
Docker `events` streams over SSH are opt-in via
`CORTEX_INVENTORY_REMOTE_DOCKER_EVENTS=true`.

On first run, before `normalized/homelab.json` exists, `map` and
`cortex inventory status --json` report `cache_status: "missing"`. Run
`cortex inventory refresh --json` to seed `~/.cortex/inventory` and clear that
missing-cache state.

## Prompts

The MCP server also exposes reusable prompts for common infrastructure debugging
workflows: `infra.incident-triage`, `infra.host-health`,
`infra.service-outage`, `infra.security-auth-review`,
`infra.noise-reduction`, and `infra.agent-change-correlation`.

For the prompt catalog and argument reference, see
[`docs/mcp/PROMPTS.md`](docs/mcp/PROMPTS.md).

## MCP Apps query widget

cortex ships one interactive UI surface as **progressive enhancement**: a simple
log-search widget for MCP hosts that support [MCP Apps](https://modelcontextprotocol.io/extensions/apps/overview)
/ MCP-UI (`_meta.ui.resourceUri`). It is a single self-contained HTML resource —
no browser build step, no external dependencies, no new server routes.

- **Resource URI:** `ui://cortex/query-widget`
- **MIME type:** `text/html;profile=mcp-app`
- The `cortex` tool advertises it via `_meta.ui.resourceUri`; the widget calls the
  same `cortex` tool with `action=search` over the host bridge and renders the
  rows in a compact table.
- It is a **simple search UI, not a dashboard** — query (FTS5) plus hostname,
  severity, and limit filters.

**Non-UI hosts are unaffected.** Plain MCP clients keep reading the normal
text/JSON tool results; only hosts that detect `_meta.ui.resourceUri` fetch and
render the `ui://` resource.

Confirm the wire contract with raw JSON-RPC (no UI host required):

```bash
# Widget resource is listed
curl -s -X POST http://localhost:3100/mcp \
  -H "Content-Type: application/json" -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}'

# Widget HTML is served with the MCP Apps MIME type
curl -s -X POST http://localhost:3100/mcp \
  -H "Content-Type: application/json" -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"ui://cortex/query-widget"}}'

# Search returns both structuredContent (for UI rows) and text (for plain clients)
curl -s -X POST http://localhost:3100/mcp \
  -H "Content-Type: application/json" -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"cortex","arguments":{"action":"search","query":"error","limit":5}}}'
```

If `CORTEX_TOKEN` is set, add `-H "Authorization: Bearer $CORTEX_TOKEN"`.
`scripts/smoke-test.sh` runs these same checks automatically. For the wire-format
details see [`docs/mcp/MCPUI.md`](docs/mcp/MCPUI.md).

### `cortex search`

Full-text search across all syslog messages with optional filters. Uses SQLite FTS5 with porter stemming.

**Parameters**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | no | — | FTS5 search query (see [FTS5 query syntax](#fts5-query-syntax)) |
| `host` | string | no | — | Exact hostname match. Use `cortex` with `action: "hosts"` to enumerate. |
| `source` | string | no | — | Exact source identifier. Syslog entries use the verified network sender address (`IP:port`); OTLP rows use the verified peer IP; Docker ingest stream rows use `docker://host/container/stream`; Docker lifecycle event rows use `docker-event://host/container/action`. |
| `severity` | string | no | — | One of: `emerg alert crit err warning notice info debug` |
| `app` | string | no | — | Application name, e.g. `sshd`, `dockerd`, `kernel` |
| `since` | string | no | — | Start of time range (relative like `1h`/`yesterday`, or ISO 8601 / RFC 3339, e.g. `2025-01-15T00:00:00Z`) |
| `until` | string | no | — | End of time range (relative or ISO 8601) |
| `limit` | integer | no | 100 | Max results (hard cap: 1000) |

**Response**

```json
{
  "count": 3,
  "logs": [
    {
      "id": 12345,
      "timestamp": "2025-01-15T14:30:00Z",
      "hostname": "router",
      "facility": "kern",
      "severity": "err",
      "app_name": "kernel",
      "process_id": null,
      "message": "kernel panic: unable to mount root",
      "received_at": "2025-01-15T14:30:01.123Z",
      "source_ip": "10.0.0.1:51234"
    }
  ]
}
```

**FTS5 examples**

```
query: "kernel panic"           # implicit AND: both terms must appear
query: "OOM AND killer"        # explicit AND
query: "sshd OR pam"           # boolean OR
query: "failed NOT sudo"       # boolean NOT
query: '"connection refused"'  # exact phrase (bypasses stemming)
query: "error*"                # prefix wildcard
query: "restart*"              # matches restart, restarted, restarting
```

### `cortex filter`

Structured filter-only retrieval for correlation workflows. This action rejects `query`; use `search` for FTS5 message-body search.

Common filters match `search`: `host`, `source`, `severity`, `app`, `facility`, `exclude_facility`, `process_id`, `since`, `until`, `received_since`, `received_until`, and `limit`.

Correlation aliases include `source_kind` (`docker-stream`, `docker-event`, `agent-command`, `shell-history`, `transcript`, `claude`, `codex`, `gemini`), plus `tool`, `project`, `session_id`, `container`, `docker_host`, `stream`, and `event_action`.
`source_kind=file-tail` filters managed file-tail rows (`source_ip` prefix `file-tail://`).

---

### `cortex tail`

Return the N most recent log entries. Equivalent to `tail -f` across all hosts.

**Parameters**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `host` | string | no | — | Filter to a specific host |
| `source` | string | no | — | Filter to an exact source identifier. Syslog entries use the verified network sender address (`IP:port`); OTLP rows use the verified peer IP; Docker ingest stream rows use `docker://host/container/stream`; Docker lifecycle event rows use `docker-event://host/container/action`. |
| `app` | string | no | — | Filter to a specific application |
| `n` | integer | no | 50 | Number of recent entries (hard cap: 500) |

**Response**

Same structure as `cortex search`: `{ "count": N, "logs": [...] }`.

---

### `cortex errors`

Summarize warnings and errors across all hosts in a time window. Groups by hostname and severity, showing counts. Use this for quick health assessments.

**Parameters**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `since` | string | no | all time | Start of time range (ISO 8601) |
| `until` | string | no | now | End of time range (ISO 8601) |

Severities included: `emerg`, `alert`, `crit`, `err`, `warning`.

**Response**

```json
{
  "summary": [
    { "hostname": "router",  "severity": "err",     "count": 42 },
    { "hostname": "router",  "severity": "warning",  "count": 17 },
    { "hostname": "storage", "severity": "crit",     "count":  3 }
  ]
}
```

---

### `cortex hosts`

List all hosts that have sent syslog messages, with first/last seen timestamps and total log counts.

**Parameters:** none

**Response**

```json
{
  "hosts": [
    {
      "hostname": "router",
      "first_seen": "2025-01-01T00:00:00.000Z",
      "last_seen":  "2025-01-15T14:30:00.000Z",
      "log_count":  18432
    }
  ]
}
```

---

### `cortex sessions`

List AI transcript sessions grouped by project, tool, session, and host.

**Parameters**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `project` | string | no | — | Exact project path, e.g. `/home/jmagar/workspace/cortex` |
| `tool` | string | no | — | AI tool filter: `claude`, `codex`, or `gemini` |
| `host` | string | no | — | Restrict to one host |
| `since` | string | no | — | Start of time range (ISO 8601) |
| `until` | string | no | — | End of time range (ISO 8601) |
| `limit` | integer | no | 100 | Max sessions (hard cap: 1000) |

**Response**

```json
{
  "count": 1,
  "sessions": [
    {
      "project": "/home/jmagar/workspace/cortex",
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

### `cortex correlate`

Search for related events across multiple hosts within a ±N minute window around a reference timestamp. Useful for debugging cascading failures. Results are grouped by host and ordered by time.

**Parameters**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `reference_time` | string | **yes** | — | Center timestamp (ISO 8601, e.g. `2025-01-15T14:30:00Z`) |
| `window_minutes` | integer | no | 5 | Minutes before and after `reference_time` (max 60) |
| `severity_min` | string | no | `warning` | Minimum severity to include. `warning` returns `warning/err/crit/alert/emerg`. `debug` returns everything. |
| `host` | string | no | — | Limit correlation to one host |
| `source` | string | no | — | Limit correlation to an exact source identifier. Syslog entries use the verified network sender address (`IP:port`); OTLP rows use the verified peer IP; Docker ingest stream rows use `docker://host/container/stream`; Docker lifecycle event rows use `docker-event://host/container/action`. |
| `query` | string | no | — | FTS5 query to narrow results |
| `limit` | integer | no | 500 | Max total events (hard cap: 999) |

**Response**

```json
{
  "reference_time": "2025-01-15T14:30:00Z",
  "window_minutes": 5,
  "window_from": "2025-01-15T14:25:00+00:00",
  "window_to":   "2025-01-15T14:35:00+00:00",
  "severity_min": "warning",
  "total_events": 12,
  "truncated": false,
  "hosts_count": 3,
  "hosts": [
    {
      "hostname": "router",
      "event_count": 7,
      "events": [...]
    }
  ]
}
```

**Note on clock skew:** `cortex correlate` uses the `timestamp` field from the syslog message, which reflects the sending device's clock. If a device clock is skewed, events may fall outside the correlation window. See [Time synchronization](#time-synchronization).

---

### `cortex stats`

Return database statistics including total logs, total hosts, time range covered, logical and physical DB size, free disk, configured thresholds, current write-block status, and runtime ingest observability.

**Parameters:** none

**Response**

```json
{
  "total_logs": 284917,
  "total_hosts": 12,
  "oldest_log": "2024-10-15T00:00:01Z",
  "newest_log": "2025-01-15T14:30:00Z",
  "logical_db_size_mb": "312.45",
  "physical_db_size_mb": "328.00",
  "free_disk_mb": "14200.00",
  "max_db_size_mb": 1024,
  "min_free_disk_mb": 0,
  "write_blocked": false,
  "runtime_observability": {
    "syslog_udp_packets_received": 280000,
    "syslog_tcp_connections_active": 3,
    "ingest_entries_enqueued": 284917,
    "ingest_queue_depth": 0,
    "ingest_queue_capacity": 10000,
    "ingest_queue_utilization_pct": "0.00",
    "writer_batches_flushed": 2850,
    "writer_logs_written": 284917,
    "writer_flush_failures": 0,
    "writer_logs_retained": 0,
    "writer_logs_discarded": 0,
    "writer_storage_blocked": false,
    "last_ingest_at": "2025-01-15T14:30:05.123Z",
    "last_write_at": "2025-01-15T14:30:05.400Z",
    "last_error_at": null
  },
  "otlp": {
    "logs_received": 42,
    "decode_errors": 0
  }
}
```

`write_blocked: true` means the storage budget is exceeded and new log ingestion is paused. See [Storage budget enforcement](#storage-budget-enforcement).

---

### `cortex status`

Return lightweight runtime status without the heavier DB statistics query. Use this for dashboards and doctor checks that need current queue depth, backpressure, writer failure/drop state, listener counters, and last activity timestamps.

**Parameters:** none

---

### `cortex help`

Return markdown documentation for all tools in this toolset.

**Parameters:** none

---

The sections above document only the most common actions in detail. For the full 45-action surface with per-action parameters, see [`docs/mcp/SCHEMA.md`](docs/mcp/SCHEMA.md) or call `action=help` against a running server.

---

## FTS5 Query Syntax

The `cortex search` and `cortex correlate` actions use SQLite FTS5 with porter stemming (`tokenize='porter unicode61'`). Valid query forms:

| Syntax | Example | Matches |
|--------|---------|---------|
| Single term | `panic` | Any message containing "panic" or stemmed variants |
| Porter stemming | `restart` | restart, restarted, restarting, restarts |
| AND (default) | `disk error` or `disk AND error` | Both terms present |
| OR | `sshd OR pam` | Either term present |
| NOT | `failed NOT sudo` | "failed" present, "sudo" absent |
| Phrase | `"connection refused"` | Exact phrase in that order |
| Prefix wildcard | `error*` | Any word starting with "error" |
| Grouped | `(kernel OR oom) AND panic` | Grouped boolean logic |

**Limits:** max 512 characters, max 16 whitespace-separated terms.

**Porter stemming** means `connect`, `connected`, `connecting`, and `connection` all match the query `connect`. Phrase queries (`"..."`) bypass stemming and require exact token order.

---

## Log Schema

Each stored log entry has these fields:

| Field | Type | Description |
|-------|------|-------------|
| `id` | integer | Auto-increment primary key |
| `timestamp` | text | Message timestamp (RFC 3339, UTC). From the syslog message header. |
| `hostname` | text | Hostname from the syslog message (user-controlled, not verified) |
| `facility` | text\|null | Syslog facility name (see facilities below) |
| `severity` | text | Syslog severity level name |
| `app_name` | text\|null | Application/process name from the syslog message |
| `process_id` | text\|null | PID from the syslog message |
| `message` | text | Log message body (FTS5-indexed) |
| `received_at` | text | Server-side receipt timestamp (RFC 3339, UTC). Used for retention. |
| `source_ip` | text | Source identifier. Syslog entries use the exact network sender address (`IP:port`) captured from the packet/connection peer. OTLP rows use the peer IP without the ephemeral source port. Docker ingest stream rows use `docker://host/container/stream`; Docker lifecycle event rows use `docker-event://host/container/action`. |
| `ai_tool` | text\|null | AI tool name (e.g. `claude`, `codex`, `gemini`) |
| `ai_project` | text\|null | AI project path |
| `ai_session_id` | text\|null | AI session unique identifier |
| `ai_transcript_path` | text\|null | Full path to the source transcript file |
| `metadata_json` | text\|null | Source-specific JSON metadata. Syslog rows include parser/source provenance; OTLP rows include resource/log attributes plus trace/span ids; Docker rows include host/container/image/compose/action details; transcript rows include source kind, file path, line number, record key, and scrub status. |

### AI transcript indexing

`cortex sessions index` scans the default local transcript roots
`~/.claude/projects`, `~/.codex/sessions`, and `~/.gemini/tmp`; `cortex sessions index --path PATH`
can scan a known transcript directory or one explicit supported transcript file, and
`cortex sessions add --file FILE` imports one file. Recursive scans are limited to
`~/.claude/projects`, `~/.codex/sessions`, `~/.gemini/tmp`, or their children; broad roots such
as `/`, `$HOME`, and the current repo root are rejected before walking. The
scanner skips symlinks, counts unsupported files without parsing them, and
streams JSONL transcript files line-by-line in bounded SQLite chunks. Gemini
chat files are imported from `~/.gemini/tmp/*/chats/session-*.json`; when a
Gemini file has only `projectHash`, Cortex stores the project as
`gemini://project/<hash>` so session inventory remains queryable. Use
`--force` to reimport a transcript path from scratch after parser changes,
`--since RFC3339` to scan only recently modified files, and
`cortex sessions checkpoints --errors` plus `cortex sessions errors` to inspect structured
scanner failures.

For real-time local Claude/Codex/Gemini transcript ingestion, install the host-local
watch service:

```bash
cortex setup sessions-watch-service install
cortex setup sessions-watch-service check
cortex setup sessions-watch-service remove
```

The watcher runs outside Docker because it needs host access to
`~/.claude/projects`, `~/.codex/sessions`, and `~/.gemini/tmp`. It writes to the configured live
SQLite DB and delegates every stable changed supported transcript file to the
same scanner path used by `cortex sessions add --file FILE`; Gemini `session-*.json`
chat files use the same checkpoint and duplicate-suppression path. Installing the watcher
disables the older polling timer so both helpers do not scan the same files.

The optional polling fallback is still available:

```bash
cortex setup sessions-index-timer install
cortex setup sessions-index-timer check
cortex setup sessions-index-timer remove
```

Both helpers are deliberately not inside the Docker container. Docker Compose
owns only the server/query runtime.

Imported AI transcript messages are scrubbed for known credential/token patterns
before storage and FTS indexing. The rows still live in the main `logs` table,
so raw actions such as `search`, `tail`, `context`, and `get` can return
scrubbed transcript text and local `ai_transcript_path` values within seconds of
the transcript write. Scrubbing is best-effort, not a compliance boundary.
If storage guardrails cannot recover enough space, indexing fails before
committing additional chunks.

### Shell and agent command history

Local command history can be correlated with system logs without introducing a
separate table:

```bash
cortex shell index --path ~/.zsh_history --shell zsh
cortex setup agent-command install
export CLAUDE_CODE_SHELL_PREFIX="$HOME/.local/bin/cortex-agent-command-wrapper"
cortex agent-command ingest-spool --path ~/.local/state/cortex/agent-command.jsonl
```

`cortex shell index` imports zsh extended history lines with timestamps and
durations as `source_kind="shell-history"` rows. Plain untimestamped history is
skipped because it cannot support time-window correlation.

`cortex setup agent-command install` writes a small local wrapper for Claude
Code's `CLAUDE_CODE_SHELL_PREFIX`. Claude Code invokes that prefix for spawned
shell commands, including Bash tool calls, hook commands, and stdio MCP server
startup commands. The wrapper preserves stdio and exit code, appends one
scrubbed JSONL record under `~/.local/state/cortex/`, and
`cortex agent-command ingest-spool` imports those records as
`source_kind="agent-command"` rows, then truncates the locked spool after a
successful import so repeated runs only process new commands. The wrapper
records command text, cwd, duration, exit status, agent name, PID, host/user, and
`CLAUDE_CODE_SESSION_ID` when present. It does not capture environment
variables, stdout, or stderr by default.

Both command import paths run the AI scrubber plus command-specific redaction
for token flags, sensitive assignments, Authorization headers, URL userinfo,
`curl -u`, and private-key blocks before storage. Scrubbing is best-effort, not
a compliance boundary.

**Important:** `hostname` is taken from the syslog message body, which any LAN device can set to an arbitrary value over UDP. For syslog entries, `source_ip` is the only trustworthy network identifier. For Docker log entries from the current host-local cortex agent, trust follows the deployed agent host and its local Docker socket access. For legacy central pull entries, `source_ip` identifies the configured Docker host/container/stream and should be trusted only as far as the explicit remote Docker Engine endpoint and network path are trusted. `metadata_json` preserves source-specific context for debugging and correlation, but it is not an authorization boundary. Retention cutoffs use `received_at` (server clock) so that devices with misconfigured clocks cannot cause premature or indefinite log retention.

### Severity levels

Ordered from most to least severe:

| Level | Numeric | Meaning |
|-------|---------|---------|
| `emerg` | 0 | System is unusable |
| `alert` | 1 | Action must be taken immediately |
| `crit` | 2 | Critical conditions |
| `err` | 3 | Error conditions |
| `warning` | 4 | Warning conditions |
| `notice` | 5 | Normal but significant condition |
| `info` | 6 | Informational messages |
| `debug` | 7 | Debug-level messages |

### Facilities

`kern`, `user`, `mail`, `daemon`, `auth`, `cortex`, `lpr`, `news`, `uucp`, `cron`, `authpriv`, `ftp`, `ntp`, `audit`, `alert`, `clock`, `local0`–`local7`.

---

## Installation

### One-line installer

```bash
curl -fsSL https://raw.githubusercontent.com/jmagar/cortex/main/install.sh | sh
```

The installer puts the host `cortex` binary in `~/.local/bin` and then runs
`cortex setup`. Setup is idempotent and owns the shared host layout:

- `~/.cortex/.env` — secrets, ports, Compose interpolation, runtime values
- `~/.cortex/compose/docker-compose.yml` — Docker Compose deployment assets
- `~/.cortex/data/cortex.db` — SQLite database and WAL/SHM sidecars

Setup writes the compose project name used by the shared host deployment.
Existing installations may still use the legacy `syslog-jmagar-lab` project
name for container-label compatibility; prefer `cortex compose ...` commands
because they resolve the live owner before mutating the stack.

Useful installer controls:

```bash
CORTEX_INSTALL_DRY_RUN=1 ./install.sh
CORTEX_INSTALL_PREFIX=/opt/cortex ./install.sh
CORTEX_VERSION=<version> ./install.sh
CORTEX_INSTALL_SKIP_SETUP=1 ./install.sh
```

Useful setup commands:

```bash
cortex setup          # first-run or normal repair
cortex setup check    # inspect only; does not mutate files or start services
cortex setup repair   # repair env/assets and restart the Docker stack
cortex deploy preflight       # clearer alias for setup check
cortex deploy local           # clearer local Compose deploy/reconcile command
cortex deploy local --dry-run # run the deploy preflight without mutating Docker
cortex setup sessions-watch-service install  # host-local real-time transcript watcher
cortex doctor binary  # check host/container binary freshness
```

### Claude Code plugin (recommended)

Install as a Claude Code plugin. The plugin handles deployment automatically — you choose between server mode (this machine hosts the syslog receiver + MCP server) and client mode (connect to a remote server).

**Prompted at install time** (via `userConfig`):

| Field | Required | Default | Notes |
|-------|----------|---------|-------|
| `is_server` | yes | `true` | Server mode hosts the receiver; client mode connects to a remote server |
| `server_url` | no | `http://localhost:3100` | Server mode: leave default. Client mode: remote host URL (e.g. `http://shart:3100`) |
| `api_token` | yes | — | Bearer token used by the plugin MCP client. Server mode: this becomes the token the server enforces unless `no_auth=true`. Client mode: token from the server admin. Stored in the system keychain. |
| `syslog_host` / `syslog_port` | no | `0.0.0.0` / `1514` | Syslog listener bind (server mode) |
| `mcp_host` / `mcp_port` | no | `0.0.0.0` / `3100` | MCP HTTP server bind (server mode) |
| `data_dir` | no | `~/.cortex/data` | Optional SQLite directory override; default shared setup data persists outside plugin cache |
| `max_db_size_mb` | no | `8192` | DB size cap; oldest logs deleted when exceeded |
| `retention_days` | no | `90` | `0` = keep forever |
| `batch_size` | no | `100` | Number of parsed messages per SQLite batch |
| `write_channel_capacity` | no | `10000` | Internal parsed-message queue capacity before listener backpressure |
| `docker_ingest_enabled` | no | `false` | Legacy central pull compatibility mode for explicit remote Docker Engine endpoints; current deployments use the host-local agent |
| `fleet_hosts` | no | — | SSH aliases of fleet hosts. Used for Docker ingest (when enabled, each becomes `http://<alias>:2375`) and the `cortex-deploy-dropins` skill |

**SessionStart hook automation** (in server mode):

- Ensures the host `cortex` binary is on `PATH`; the installer defaults to `~/.local/bin`
- Exports plugin userConfig as `CORTEX_*` / `CORTEX_*` environment values
- Runs `cortex setup repair`, the same setup path used by the one-line installer
- Repairs shared assets under `~/.cortex` and removes stale user-level `cortex.service` units/drop-ins left by older plugin versions
- All idempotent — safe to run on every session

**Bundled skills** (all 9, from `plugins/cortex/skills/`):

- `cortex` — primary log-intelligence skill: search, tail, errors, correlate, stats, and the rest of the MCP action surface
- `cortex-dr` — health check covering MCP, service status, syslog port, fleet drop-ins, and live log flow; tails service logs on failure
- `cortex-deploy-dropins` — SSH-based one-shot rsyslog drop-in deployment to every host in `fleet_hosts`
- `cortex-frustration-assessment` — analyze an `abuse_investigate` evidence bundle into a frustration/abuse report
- `cortex-logs` — Docker Compose service log tailing (the service's own stdout/stderr, not client syslog)
- `cortex-redeploy` — re-run plugin setup after config or plugin changes
- `cortex-report` — time-bounded homelab health/log-analysis markdown reports
- `cortex-troubleshoot` — diagnose connection failures, missing logs, unhealthy containers, and restart loops
- `cortex-version-check` — check whether the running Docker container matches the local Compose image; add `--pull` to pull first, otherwise checks only the local image cache

The plugin deploys the server with Docker Compose through the same `cortex setup`
path as the one-line installer. You can still build and run the binary locally
for development, but automated deployment is Compose-only.

`cortex deploy local` is the operator-facing name for the same local
Compose-backed reconcile path. It exists so deploy workflows do not need to call
a command named `setup repair` directly.

### Docker

```bash
git clone https://github.com/jmagar/cortex
cd cortex
cp .env.example .env
# Edit .env — set CORTEX_TOKEN at minimum
docker compose up -d
```

The container binds:
- `UDP :1514` and `TCP :1514` for syslog ingestion (published on all interfaces — senders must reach it)
- `TCP :3100` for the MCP HTTP API, published on `127.0.0.1` only by default. Set `CORTEX_MCP_BIND=0.0.0.0` (plus `CORTEX_TOKEN`) to expose it; containers on the same Docker network (e.g. the Labby gateway) reach `http://cortex:3100` either way.

### Local build

Requires Rust 1.86+.

```bash
cargo build --release
./target/release/cortex serve mcp
```

---

## Authentication

cortex supports two auth modes, selectable via `CORTEX_AUTH_MODE`.

**Bearer-only (default)** — set `CORTEX_TOKEN` and all `/mcp` requests must present that token as `Authorization: Bearer <token>`. No OAuth routes are mounted.

**Loopback no-auth** — set `CORTEX_NO_AUTH=true` only for local development on loopback binds.

**Gateway-protected no-auth (TrustedGatewayUnscoped)** — on non-loopback binds, set both `CORTEX_NO_AUTH=true` and `CORTEX_TRUSTED_GATEWAY_NO_AUTH=true` only when an upstream gateway or reverse proxy enforces auth before traffic reaches cortex. This intentionally disables service-local MCP auth **and the read/admin scope gates** — every caller can run the write actions `ack_error`, `unack_error`, and `notifications_test`. **Never combine this mode with host-published ports**; keep `CORTEX_MCP_BIND=127.0.0.1` (the default) so only the gateway's Docker network path reaches cortex. See [docs/SECURITY.md](docs/SECURITY.md).

**OAuth (Google)** — set `CORTEX_AUTH_MODE=oauth`, the OAuth provider env vars, and an allowlisted admin email. The server issues RS256 JWTs after users authenticate via Google. Bearer tokens and OAuth JWTs can coexist (OAuth mode disables the static token by default; set `CORTEX_AUTH_DISABLE_STATIC_TOKEN_WITH_OAUTH=false` or `disable_static_token_with_oauth = false` in `config.toml` for break-glass access).

Both modes leave `/health` unauthenticated so health probes always work.

See [docs/OAUTH.md](docs/OAUTH.md) for full setup instructions, architecture diagram, and operator FAQ.

---

## Configuration

Configuration is loaded from three sources in priority order (highest wins):

1. Environment variables
2. `config.toml` (if present)
3. Built-in defaults

### Environment variables

#### MCP server

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `CORTEX_TOKEN` | no | — | Bearer token for `/mcp`. Omit to disable auth (loopback binds only). Required when exposing port 3100 beyond loopback. |
| `CORTEX_HOST` | no | `127.0.0.1` | Bind host for the MCP HTTP server (loopback by default) |
| `CORTEX_PORT` | no | `3100` | Bind port for the MCP HTTP server |
| `CORTEX_MCP_BIND` | no | `127.0.0.1` | Docker Compose only: host interface port 3100 is published on. Set `0.0.0.0` together with `CORTEX_TOKEN` to expose it. |
| `CORTEX_ALLOWED_HOSTS` | no | — | Extra comma-separated Host header values accepted by RMCP Host validation |
| `CORTEX_ALLOWED_ORIGINS` | no | — | Extra comma-separated browser origins accepted by RMCP Origin validation |

#### Non-MCP API

The plain JSON API is always mounted under `/api/*` on the same HTTP listener and requires its own bearer token — the server fails to start (on the server path) without it. `cortex setup repair` generates one if missing.

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `CORTEX_API_TOKEN` | yes | — | Bearer token for `/api/*` routes |

#### Syslog listener

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `CORTEX_RECEIVER_HOST` | no | `0.0.0.0` | Bind host for UDP + TCP syslog listeners |
| `CORTEX_RECEIVER_PORT` | no | `1514` | Bind port for UDP + TCP syslog listeners |
| `CORTEX_RECEIVER_HOST_PORT` | no | `1514` | Docker Compose host port published to container port `1514` |
| `CORTEX_MAX_MESSAGE_SIZE` | no | `8192` | Max bytes per UDP datagram or newline-delimited TCP frame. Oversized newline-delimited TCP frames are dropped and the connection stays open; oversized unterminated frames are dropped and the connection is closed. |
| `CORTEX_MAX_TCP_CONNECTIONS` | no | `512` | Maximum simultaneous TCP syslog connections |
| `CORTEX_TCP_IDLE_TIMEOUT_SECS` | no | `300` | Idle timeout per TCP read before closing inactive connections |
| `CORTEX_BATCH_SIZE` | no | `100` | Number of messages per batch write |
| `CORTEX_FLUSH_INTERVAL` | no | `500` | Batch flush interval in milliseconds |
| `CORTEX_WRITE_CHANNEL_CAPACITY` | no | `10000` | Internal parsed-message queue capacity |

#### Docker log ingest

The current deployment path is the host-local cortex agent. Each deployed agent reads Docker logs from that host's local Docker socket (`unix:///var/run/docker.sock`) and forwards the normalized rows into cortex. This keeps Docker's normal local logging behavior intact, avoids daemon-level syslog drivers, and does not require exposing a Docker API endpoint on the network.

The `CORTEX_DOCKER_*` settings below remain as a legacy central pull compatibility mode for explicit remote Docker Engine HTTP endpoints. Use them for fixtures or transitional deployments where cortex itself should connect to a Docker-compatible API. Older deployments used `docker-socket-proxy` for this endpoint, but that is no longer the recommended homelab path.

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `CORTEX_DOCKER_INGEST_ENABLED` | no | `false` | Enable legacy central pull Docker log ingestion |
| `CORTEX_DOCKER_HOSTS` | one of the two | — | Comma-separated hostnames; each becomes `http://<name>:2375` with `allow_insecure_http = true`. Takes priority over `CORTEX_DOCKER_HOSTS_FILE`. |
| `CORTEX_DOCKER_HOSTS_FILE` | one of the two | — | Path to a TOML file with a `[[hosts]]` array (use when you need per-host `base_url` or TLS). If the file does not exist, a warning is logged and no hosts are loaded — the container will not crash. Mount the file into the container (e.g. under `/cortex-home` via `CORTEX_HOME_VOLUME`). |
| `CORTEX_DOCKER_RECONNECT_INITIAL_MS` | no | `1000` | Initial reconnect delay after host stream failure |
| `CORTEX_DOCKER_RECONNECT_MAX_MS` | no | `30000` | Maximum reconnect delay after repeated failures |

The hosts file uses this shape:

```toml
[[hosts]]
name = "edge-host-a"
base_url = "http://edge-host-a:2375"
allow_insecure_http = true

[[hosts]]
name = "app-host-b"
base_url = "http://app-host-b:2375"
allow_insecure_http = true
```

If this legacy pull path points at `docker-socket-proxy`, the proxy side only needs read access to containers, events, ping, and version endpoints: `CONTAINERS=1`, `EVENTS=1`, `PING=1`, `VERSION=1`, `POST=0`. `CONTAINERS=1` exposes the broader read-only Docker container API to anything that can reach the proxy, so bind it only on a trusted private network, firewall it to cortex, or put it behind authenticated TLS. Plain `http://` endpoints require `allow_insecure_http = true` in the hosts file so that this trust decision is explicit.

Docker log ingest has two test boundaries: agent parity tests verify that deployed agents preserve local Docker socket streaming, and the legacy central pull client has a mocked Docker HTTP fixture. The default smoke test stays focused on live UDP/TCP syslog, MCP/REST actions, CLI parity, and managed file-tail ingest. For a full legacy pull integration check, run cortex with `CORTEX_DOCKER_INGEST_ENABLED=true` against a disposable Docker-compatible HTTP fixture, emit a unique line from a short-lived container, then verify it with `cortex search` or `mcporter call ... action=search`. Container stdout/stderr rows use `source_ip=docker://<host>/<container>/<stream>`. Container lifecycle rows for actions such as `create`, `start`, `restart`, `die`, `stop`, `destroy`, `rename`, `oom`, and `health_status:*` use `source_ip=docker-event://<host>/<container>/<sanitized-action>`, `facility=docker`, and preserve the raw Docker event JSON.

#### Managed file-tail ingest

Cortex can tail local files directly without rsyslog `imfile` drop-ins. In
Docker, mount the host log tree read-only at `/file-tail-root` with
`CORTEX_FILE_TAIL_LOG_VOLUME` and register paths inside that mount. Sources are
stored next to the SQLite database in `file-tails.json`, managed through
`cortex file-tail ...`, REST `POST /api/file-tails` (requires
`Authorization: Bearer $CORTEX_API_TOKEN` plus
`X-Cortex-Admin-Token: $CORTEX_API_ADMIN_TOKEN`), or MCP action `file_tails`,
and emitted as `source_kind="file-tail"` rows. Row metadata includes
`file_tail_id`, `tag`, and `path_basename`, not the full filesystem path.
The documented safe default is to keep managed tails inside `/file-tail-root`.
Set `CORTEX_FILE_TAIL_ALLOWED_ROOTS` explicitly only when an operator has
mounted and reviewed broader read-only roots such as `/var/log` or `/logs`.

```bash
cortex file-tail add --id swag-access \
  --path /file-tail-root/swag/log/nginx/access.log \
  --tag swag-access --host squirts --facility local4
cortex file-tail add --id swag-error \
  --path /file-tail-root/swag/log/nginx/error.log \
  --tag swag-error --host squirts --facility local4 --severity warning
cortex file-tail add --id fail2ban \
  --path /file-tail-root/swag/log/fail2ban/fail2ban.log \
  --tag fail2ban --host squirts --facility local5
cortex file-tail add --id authelia \
  --path /file-tail-root/authelia/logs/authelia.log \
  --tag authelia --host squirts --facility local5
cortex file-tail add --id adguard-query \
  --path /file-tail-root/adguard/var/data/querylog.json \
  --tag adguard-query --host squirts --facility local6
```

The default starts at EOF. Add `--from-start` only when you intentionally want
to backfill the current file contents. After startup, Cortex checkpoints
`dev`/`inode`/offset in `file-tails.json`, resumes from that cursor, and
reopens files on rename/create rotation or truncation. Lines are bounded by
`CORTEX_MAX_MESSAGE_SIZE`; oversized records are truncated before enqueue.

#### Storage

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `CORTEX_DB_PATH` | no | `/data/cortex.db` | SQLite database path |
| `CORTEX_POOL_SIZE` | no | `8` | SQLite connection pool size. MCP/REST reads get `pool_size - 1` permits; one connection is reserved for the ingest writer. |
| `CORTEX_SQLITE_PAGE_CACHE_MB` | no | `128` | Total SQLite page-cache budget across the pool; divided by `pool_size` before `PRAGMA cache_size`. |
| `CORTEX_SQLITE_MMAP_MB` | no | `256` | Bounded SQLite mmap size. Resident mapped pages may still count toward cgroup memory. |
| `CORTEX_HEAVY_READ_CONCURRENCY` | no | `1` | Shared service-layer limiter for expensive read actions. |
| `CORTEX_WAL_CHECKPOINT_MB` | no | `256` | WAL size threshold for bounded PASSIVE checkpoint attempts. |
| `CORTEX_RETENTION_DAYS` | no | `90` | Days to retain logs. `0` = keep forever. Purge runs hourly; err+ severities are exempt (see [Retention Policy](#retention-policy)). |
| `CORTEX_MAX_DB_SIZE_MB` | no | `1024` | Logical DB size trigger: breach deletes oldest logs. `0` = disabled. |
| `CORTEX_RECOVERY_DB_SIZE_MB` | no | `900` | Cleanup target after DB size trigger. Must be less than max. |
| `CORTEX_MIN_FREE_DISK_MB` | no | `0` | Free disk threshold. **Disabled by default.** A breach blocks writes (it does not delete data). |
| `CORTEX_RECOVERY_FREE_DISK_MB` | no | `0` | Hysteresis target before writes resume after a free-disk breach. Must be greater than min when enabled. |
| `CORTEX_CLEANUP_INTERVAL_SECS` | no | `60` | Storage budget enforcement interval. Minimum `5`. |
| `CORTEX_CLEANUP_CHUNK_SIZE` | no | `2000` | Rows deleted per enforcement chunk |
| `CORTEX_ERR_FLOOR_WINDOW_HOURS` | no | `24` | err+ rows received within this window are protected from disk-pressure deletion. `0` = disable the floor. |
| `CORTEX_ERR_FLOOR_PER_SOURCE_CAP` | no | `10000` | Max protected err+ rows per source IP within the window. `0` = disable the floor. |

#### Container

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `CORTEX_UID` | no | `1000` | Container user ID for data volume ownership |
| `CORTEX_GID` | no | `1000` | Container group ID for data volume ownership |
| `CORTEX_DATA_VOLUME` | no | `cortex-data` | Docker volume name or bind-mount path |
| `CORTEX_HOME_VOLUME` | no | `~/.cortex` | Shared cortex home (inventory cache, setup env) mounted at `/cortex-home` |
| `CORTEX_SSH_VOLUME` | no | `~/.cortex/ssh` | Dedicated SSH key dir mounted read-only at `/home/cortex/.ssh`. Never point at `~/.ssh` — see Security Model |
| `DOCKER_NETWORK` | no | `cortex` | Docker network name (must exist) |
| `RUST_LOG` | no | `info` | Log level (`trace`, `debug`, `info`, `warn`, `error`) |
| `TZ` | no | `UTC` | Container timezone |

### config.toml

Place `config.toml` next to the binary (or in the working directory). Environment variables override values set here.

```toml
[syslog]
host = "0.0.0.0"
port = 1514
max_message_size = 8192
max_tcp_connections = 512
tcp_idle_timeout_secs = 300

[storage]
db_path = "data/cortex.db"
pool_size = 8
sqlite_page_cache_mb = 128
sqlite_mmap_mb = 256
heavy_read_concurrency = 1
wal_checkpoint_mb = 256
retention_days = 90   # 0 = keep forever
wal_mode = true
max_db_size_mb = 1024
recovery_db_size_mb = 900
min_free_disk_mb = 0      # 0 = free-disk guard disabled (default)
recovery_free_disk_mb = 0
cleanup_interval_secs = 60

[mcp]
host = "127.0.0.1"
port = 3100
server_name = "cortex"
# api_token = "your-secret-token"

[docker_ingest]
enabled = false
reconnect_initial_ms = 1000
reconnect_max_ms = 30000

[[docker_ingest.hosts]]
name = "edge-host-a"
base_url = "http://edge-host-a:2375"
allow_insecure_http = true
```

---

## Security Model

### Syslog ingest is unauthenticated by design

The UDP and TCP syslog listeners (port 1514) accept log frames from **any reachable host** with no authentication. This matches the RFC 3164/5424 syslog protocol design and is intentional for homelab deployments where the network perimeter is the trust boundary.

Consequences:
- **`hostname` in stored records is caller-controlled** for vendor formats (CEF/UniFi). Any host on the network can claim any hostname. Use `source_ip` for trusted origin identification.
- **Log injection is possible** from any host that can reach port 1514. Do not use cortex for security-critical audit trails without network-level access controls.
- **Retention exemption**: `severity=err` and above are excluded from time-based purge. A host flooding with high-severity frames can exhaust disk space.

**Mitigations**: Bind the syslog port to a specific interface, use a firewall rule to restrict sources, or set `CORTEX_ALLOWED_SOURCE_CIDRS` (comma-separated CIDR list) to allowlist sending hosts.

### MCP API authentication

The MCP query API (port 3100, default loopback) supports two auth modes:

| Mode | Config | Effect |
|------|--------|--------|
| Bearer token | `CORTEX_TOKEN=<token>` | Static token grants `cortex:read` by default; set `CORTEX_STATIC_TOKEN_ADMIN=true` to also grant `cortex:admin` |
| Google OAuth | `CORTEX_AUTH_MODE=oauth` | OAuth users authenticated via `CORTEX_AUTH_ADMIN_EMAIL` |

**Important**: Admin actions such as `ack_error`, `unack_error`, and `notifications_test` require `cortex:admin`. Static bearer tokens are read-only unless `CORTEX_STATIC_TOKEN_ADMIN=true` is explicitly set.

The MCP port defaults to `127.0.0.1:3100` (loopback only), and the Docker Compose files publish container port 3100 on `127.0.0.1` by default (`CORTEX_MCP_BIND` overrides the host interface). The Labby gateway reaches cortex over the Docker network at `http://cortex:3100` regardless of the host publish address. To expose port 3100 on a network interface, set `CORTEX_MCP_BIND=0.0.0.0` (Compose) or `CORTEX_HOST=0.0.0.0` (bare binary), **set `CORTEX_TOKEN`**, and put a TLS-terminating reverse proxy in front of it.

### SSH key exposure (inventory mount)

The Compose files mount an SSH key directory read-only at `/home/cortex/.ssh` for the fleet inventory collectors. The default source is a **dedicated key dir, `~/.cortex/ssh`** (override with `CORTEX_SSH_VOLUME`). **Never point `CORTEX_SSH_VOLUME` at `~/.ssh`** — mounting your personal SSH directory gives the container every identity you own and creates a lateral-movement path across the fleet.

Provision a least-privilege deploy key instead: generate a dedicated keypair in `~/.cortex/ssh`, write a minimal `config` listing only the hosts cortex should collect from, curate a `known_hosts` file (`ssh-keyscan`), and restrict the key on each fleet host with a `restrict,command="..."` `authorized_keys` entry under a low-privilege user. Full walkthrough: [docs/SECURITY.md](docs/SECURITY.md) "SSH Key Exposure".

---

## Command modes

```bash
cortex serve mcp  # UDP/TCP syslog ingest plus HTTP MCP on /mcp
cortex mcp        # query-only MCP stdio transport
cortex setup      # install/repair shared ~/.cortex Docker Compose setup
cortex deploy preflight  # check deploy prerequisites without mutating Docker
cortex deploy local      # reconcile local Compose deployment
cortex stats      # query the SQLite DB directly from the CLI
cortex db status  # inspect SQLite maintenance state
cortex db backup  # create a WAL-safe SQLite backup
cortex compose doctor          # diagnose live Compose/listener ownership
cortex compose status --json   # inspect canonical cortex container/project
```

Both modes use the same config and environment variable loader. `cortex mcp` is for local child-process MCP clients that can read `CORTEX_DB_PATH`; it does not bind network ports or run retention/storage cleanup jobs.

The direct CLI uses the same shared service layer as the MCP tool, so results and validation match the MCP actions without needing an MCP client:

```bash
cortex search "oom killer"                 # bare query; default limit 50
cortex tail dookie                         # bare positional → --host; default n=50
cortex errors                              # defaults to the last hour
cortex host-state tootie                   # bare positional → --host
cortex search 'error AND nginx' --host proxy --limit 10
cortex tail -n 20 --app kernel
cortex errors --since 2026-01-01T00:00:00Z
cortex hosts
cortex correlate --reference-time 2026-01-01T12:00:00Z --window-minutes 10 --severity-min warning
cortex entity host tootie
cortex graph around host tootie --limit 25
cortex graph explain host tootie --depth 2
cortex stats --json
cortex db integrity            # run PRAGMA integrity_check
cortex db checkpoint --mode full
cortex db vacuum --pages 1000
cortex compose pull            # pull image for resolved Compose project
cortex compose up              # run docker compose up -d for resolved service
cortex compose restart         # restart resolved service
cortex compose logs --tail 20  # bounded compose logs

# Surface parity (2026-05-22) — each is also a REST GET on /api/<command>
cortex silent-hosts --silent-minutes 60
cortex clock-skew   --since 2026-05-20T00:00:00Z
cortex anomalies    --recent-minutes 30 --baseline-minutes 720
cortex compare      --a-from 2026-05-20T00:00:00Z --a-to 2026-05-20T23:59:59Z \
                    --b-from 2026-05-21T00:00:00Z --b-to 2026-05-21T23:59:59Z
cortex apps         --host dookie --limit 50
```

### REST endpoints (2026-05-22 surface parity)

The 12 new routes mirror existing MCP actions one-for-one. All require the
`CORTEX_API_TOKEN` bearer; AI endpoints with `terms[]=` parameters are served
via `serde_qs` to handle repeated keys.

```
GET  /api/silent-hosts?silent_minutes=60
GET  /api/clock-skew?since=<RFC3339>
GET  /api/anomalies?recent_minutes=15&baseline_minutes=360
GET  /api/compare?a_from=...&a_to=...&b_from=...&b_to=...
GET  /api/apps?hostname=&from=&to=&limit=&offset=
GET  /api/similar-incidents?query=...&window_minutes=30
GET  /api/incident-context?from=...&to=...
GET  /api/sessions/ask-history?query=...
GET  /api/sessions/incidents?terms[]=foo&terms[]=bar
GET  /api/sessions/investigate?correlation_window_minutes=30
GET  /api/graph/entity?entity_type=host&key=tootie
GET  /api/graph/around?entity_type=host&key=tootie&depth=1
GET  /api/graph/explain?entity_type=host&key=tootie&depth=2
GET  /api/compose/status
GET  /api/compose/doctor
```

`/api/compose/status` is a redacted read-only projection and can report
`runtime_state="docker_unavailable"` when Docker inspection is unavailable from
inside the container. `/api/compose/doctor` is stricter: unready Docker,
ownership, or runtime states return HTTP 503 with the same structured projection.

`cortex compose` commands resolve the live Compose owner before mutation. They refuse ambiguous cwd fallback, stale Compose labels, listener conflicts, and destructive `down` without `--yes`.

See [docs/CLI.md](docs/CLI.md) for the full direct CLI reference, including flags, JSON output, and how CLI commands map to MCP actions.

---

## Syslog Forwarder Setup

The server listens on port `1514` by default. Configure senders to forward to this port. If a device cannot use a non-privileged port, see [Exposing port 514](#exposing-port-514).

### rsyslog

Create `/etc/rsyslog.d/99-remote.conf` on each host:

```conf
# TCP (reliable, recommended for persistent connections)
*.* @@CORTEX_SERVER:1514

# UDP (lower overhead, no delivery guarantee)
# *.* @CORTEX_SERVER:1514
```

Restart: `sudo systemctl restart rsyslog`

For hosts running pure journald without rsyslog, first enable forwarding in `/etc/systemd/journald.conf`:

```ini
[Journal]
ForwardToSyslog=yes
```

Then install and configure rsyslog as above.

### syslog-ng

Add to `/etc/syslog-ng/conf.d/remote.conf`:

```conf
destination d_remote_tcp {
    network("CORTEX_SERVER"
        port(1514)
        transport("tcp")
    );
};

destination d_remote_udp {
    network("CORTEX_SERVER"
        port(1514)
        transport("udp")
    );
};

log {
    source(s_src);
    destination(d_remote_tcp);
};
```

Restart: `sudo systemctl restart syslog-ng`

### WSL2 (systemd enabled)

Enable systemd in `/etc/wsl.conf`:

```ini
[boot]
systemd=true
```

Install rsyslog and use the rsyslog config above. Use the Tailscale IP of the cortex host — WSL has its own network namespace and cannot reach the Docker host IP directly.

### UniFi Cloud Gateway

Option A — via SSH:

```bash
ssh admin@<gateway-ip>
# Create /etc/rsyslog.d/remote.conf (persists on newer firmware):
echo "*.* @CORTEX_SERVER:1514" | sudo tee /etc/rsyslog.d/remote.conf
sudo systemctl restart rsyslog
```

Option B — via UI (survives firmware updates):

Settings → System → Advanced → Remote Syslog Server. Set host and port `1514`.

### Routers and appliances (UDP-only devices)

Set the syslog server address to your `CORTEX_SERVER` and port to `1514` in the device's syslog settings. Most consumer routers and network appliances expose this under Diagnostics or Logging settings.

### Exposing port 514

Syslog's privileged port 514 requires root or `CAP_NET_BIND_SERVICE`. The recommended approach is to redirect at the host with iptables:

```bash
# Redirect UDP and TCP 514 → 1514 on the host
sudo iptables -t nat -A PREROUTING -p udp --dport 514 -j REDIRECT --to-port 1514
sudo iptables -t nat -A PREROUTING -p tcp --dport 514 -j REDIRECT --to-port 1514

# Persist across reboots (Debian/Ubuntu)
sudo apt install iptables-persistent
sudo netfilter-persistent save
```

For Docker Compose, set `CORTEX_RECEIVER_HOST_PORT=514` to publish host port `514` while the container keeps binding unprivileged port `1514`. On Unraid, map host port `514` to container port `1514` for both UDP and TCP in the Docker template (`514:1514/udp` and `514:1514/tcp`).

### Firewall rules

Open the syslog port on the Docker host firewall:

```bash
# ufw
sudo ufw allow 1514/udp
sudo ufw allow 1514/tcp

# firewalld
sudo firewall-cmd --permanent --add-port=1514/udp
sudo firewall-cmd --permanent --add-port=1514/tcp
sudo firewall-cmd --reload
```

---

## Heartbeats

The heartbeat agent is a small host-local loop (the same `cortex` binary) that collects bounded system state (load, memory, disk, top processes) and POSTs it to the server's `POST /v1/heartbeats` endpoint on port 3100 every 30 seconds. Heartbeat rows feed the `host_state`, `fleet_state`, and `correlate_state` MCP actions and are retained for 14 days.

Install it as a user systemd service on each fleet host:

```bash
cortex setup heartbeat-agent install   # write + enable the systemd unit
cortex setup heartbeat-agent check     # inspect unit/env state
cortex setup heartbeat-agent remove    # remove the unit
cortex heartbeat agent --once --emit   # one-shot foreground run for debugging
```

Configuration:

- `CORTEX_HEARTBEAT_TARGET` — server base URL (default `http://127.0.0.1:3100`; falls back to `CORTEX_URL`)
- `CORTEX_HEARTBEAT_TOKEN` — bearer token sent with each POST (falls back to `CORTEX_TOKEN`)

When the server has `CORTEX_TOKEN` set, heartbeat POSTs must carry that token; on an unauthenticated loopback-only server no token is needed.

---

## OTLP Ingest

The shared HTTP listener on port 3100 also accepts OpenTelemetry logs at `POST /v1/logs` (logs only — `/v1/metrics` and `/v1/traces` return 404). The endpoint decodes **binary protobuf** (`ExportLogsServiceRequest`; OTLP/JSON is not supported) and enforces a **4 MiB** request body limit (413 responses include `Retry-After: 86400`).

Auth matches MCP: when `CORTEX_TOKEN` is set, requests need `Authorization: Bearer <token>`. Exposing `/v1/logs` on a non-loopback bind without a static bearer token is blocked at startup (OAuth JWTs do not authorize OTLP ingest today).

Minimal OpenTelemetry Collector exporter config:

```yaml
exporters:
  otlphttp/cortex:
    endpoint: http://CORTEX_SERVER:3100
    encoding: proto
    headers:
      Authorization: "Bearer ${env:CORTEX_TOKEN}"

service:
  pipelines:
    logs:
      exporters: [otlphttp/cortex]
```

---

## Retention Policy

Logs are retained for `CORTEX_RETENTION_DAYS` days (default `90`). Set to `0` to disable the global age-based purge (the AdGuard and heartbeat caps below still apply).

The retention purge runs **hourly** (the separate storage-budget enforcement loop runs on `CORTEX_CLEANUP_INTERVAL_SECS`). It deletes logs in chunks of 10,000 rows, releasing the write lock between chunks so ingest can proceed. Retention cutoff uses `received_at` (the server-side ingestion timestamp), not the `timestamp` in the message. This prevents devices with misconfigured clocks from causing premature or indefinite retention.

Severity-based exemptions and per-source caps:

- **err+ exemption** — rows with `severity IN (err, crit, alert, emerg)` are never aged out by retention. They can still be deleted under DB-size pressure, but only outside the err+ floor (`CORTEX_ERR_FLOOR_WINDOW_HOURS=24`, `CORTEX_ERR_FLOOR_PER_SOURCE_CAP=10000`).
- **AdGuard tags** — `adguard-allowed` / `adguard-query` / `adguard-rewrite` rows are hard-capped at **7 days** regardless of `retention_days` (DNS query volume would otherwise dominate the FTS index).
- **Heartbeats** — heartbeat telemetry rows are capped at **14 days**.

After large deletions, an incremental FTS5 merge runs to reclaim index space without long write-lock durations.

---

## Storage Budget Enforcement

Two independent guards protect against disk exhaustion:

**DB size guard** (`CORTEX_MAX_DB_SIZE_MB`, default 1024 MB — enabled)

When the logical SQLite DB size exceeds `max_db_size_mb`, the oldest logs are deleted in chunks of `CORTEX_CLEANUP_CHUNK_SIZE` rows until the size drops below `recovery_db_size_mb`. High-severity rows inside the err+ floor (`CORTEX_ERR_FLOOR_WINDOW_HOURS=24`, capped at `CORTEX_ERR_FLOOR_PER_SOURCE_CAP=10000` rows per source IP) are excluded from the deletable set.

**Free disk guard** (`CORTEX_MIN_FREE_DISK_MB`, default 0 — **disabled**)

Whole-filesystem free space is an external condition cortex cannot fix by deleting its own data, so a free-disk breach **blocks new writes** rather than self-trimming. Writes resume once free disk rises above `recovery_free_disk_mb` (hysteresis prevents oscillation). Enable by setting both `min_free_disk_mb` and a higher `recovery_free_disk_mb`.

**Write-blocking behavior**

If enforcement cannot free enough space (or the free-disk guard trips), the batch writer enters write-blocked state. New log messages accumulate in an in-memory buffer (`CORTEX_WRITE_CHANNEL_CAPACITY`, default 10,000 messages). Writes resume automatically when space recovers. The `write_blocked` field in `cortex stats` reflects the current state.

Disable either guard by setting its trigger to `0` (also set the recovery target to `0`).

### Heavy SQLite migrations

Most schema setup runs automatically during startup. Heavy migrations, such as creating a new index on a populated multi-million-row `logs` table, can hold SQLite's write lock for several minutes before syslog listeners and `/health` are available. During that window TCP senders may back up and UDP packets may be dropped by kernel buffers.

Before upgrading a populated database:

1. Take a WAL-safe backup with `scripts/backup.sh` or `sqlite3 /data/cortex.db ".backup /data/syslog-pre-upgrade.db"`.
2. Schedule a short ingest maintenance window for large databases.
3. Start the new version and monitor logs for `Migration N: starting ...` and `Migration N: ... created`.
4. Keep the previous image or binary available until `/health` returns `ok` and `cortex stats` reports sane counts.

See [docs/RELEASE.md](docs/RELEASE.md) for the current release and deploy
gate checklist.

---

## Batch Writer

The batch writer improves throughput by collecting parsed syslog messages into batches before writing to SQLite.

| Variable | Default | Description |
|----------|---------|-------------|
| `CORTEX_BATCH_SIZE` | `100` | Write when this many messages are queued |
| `CORTEX_FLUSH_INTERVAL` | `500` ms | Write every N ms even if batch is not full |
| `CORTEX_WRITE_CHANNEL_CAPACITY` | `10000` | Parsed-message queue capacity before listener backpressure |

Batches are written in a single SQLite transaction. If the DB is busy (locked), the writer retries up to 3 times with exponential backoff (25 ms, 100 ms, 250 ms). Batches that fail insertion are retained in memory and retried on the next flush cycle. If a retained batch grows beyond 1,000 entries, it is discarded to prevent unbounded memory growth.

The internal write channel holds up to `CORTEX_WRITE_CHANNEL_CAPACITY` parsed messages. When the channel is full, backpressure is logged and further UDP/TCP receives block until space is available.

---

## Multi-Host Deployment

Point multiple hosts at the same cortex instance. Each sender's `hostname` field (from the syslog message) is recorded and indexed. Use `cortex hosts` to see all senders. Filter by `hostname` in `cortex search` and `cortex tail`. Use `cortex correlate` to find related events across hosts within a time window.

For large fleets, consider:
- Increasing `CORTEX_POOL_SIZE` (default 8) for higher read concurrency
- Increasing `CORTEX_BATCH_SIZE` and `CORTEX_FLUSH_INTERVAL` to reduce write overhead
- Setting `CORTEX_RETENTION_DAYS` to balance history depth against disk cost

---

## Time Synchronization

All timestamps are stored in UTC. `cortex correlate` uses the `timestamp` field from the syslog message, which reflects the sending device's clock. Devices with drifted clocks will have their events shifted relative to the correlation window. Run NTP on all senders to minimize skew. `received_at` (the server-side ingestion time) is unaffected by sender clock drift and is used for retention.

---

## HTTPS / Reverse Proxy

Add a SWAG proxy conf to expose the MCP API over TLS:

```nginx
# /config/nginx/proxy-confs/cortex.subdomain.conf
server {
    listen 443 ssl;
    server_name cortex.*;

    include /config/nginx/ssl.conf;

    location / {
        include /config/nginx/proxy.conf;
        include /config/nginx/resolver.conf;

        # RMCP Streamable HTTP in stateless JSON-response mode.
        # Clients use POST /mcp; GET/DELETE /mcp are not supported.
        proxy_http_version 1.1;

        set $upstream_app cortex;
        set $upstream_port 3100;
        set $upstream_proto http;
        proxy_pass $upstream_proto://$upstream_app:$upstream_port;
    }
}
```

---

## Development

```bash
just dev       # cargo run -- serve mcp
just check     # cargo check
just lint      # cargo clippy -- -D warnings
just fmt       # cargo fmt
just test      # cargo test
just build     # cargo build
just release   # cargo build --release
```

Docker:

```bash
just up        # docker compose up -d
just logs      # docker compose logs -f
just down      # docker compose down
just restart   # docker compose restart
cortex compose doctor
cortex compose status --json
cortex compose logs --tail 20
```

Generate a bearer token:

```bash
just gen-token   # openssl rand -hex 32
```

---

## Verification

After deploying, verify the stack:

```bash
# Health probe (no auth required)
curl -sf http://localhost:3100/health | jq .
# → {"status":"ok"}

# Send a test message from any Linux host
logger -n CORTEX_SERVER -P 1514 --tcp "test from $(hostname)"

# Tail recent logs via MCP (replace token if auth is enabled)
curl -s -X POST http://localhost:3100/mcp \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
      "method": "tools/call",
      "params": {
      "name": "cortex",
      "arguments": {"action": "tail", "n": 10}
    }
  }' | jq .

# DB stats
curl -s -X POST http://localhost:3100/mcp \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -d '{
    "jsonrpc": "2.0",
    "id": 2,
    "method": "tools/call",
    "params": {"name": "cortex", "arguments": {"action": "stats"}}
  }' | jq .result.content[0].text | jq -r . | jq .
```

Run the full test suite:

```bash
just check
just lint
just test
```

Run the live smoke test against a running server:

```bash
bash scripts/smoke-test.sh
```

The smoke test seeds UDP and TCP syslog messages and verifies MCP search/tail results. Docker log coverage is split by path: host-local agent parity is covered by agent deployment tests, the default live smoke covers file-tail/REST/CLI paths, and legacy central pull coverage uses the mocked Docker HTTP fixture or an explicit Docker-compatible endpoint.

---

## Performance

At typical homelab scale (1–20 hosts, thousands of messages per day):

- SQLite with WAL mode handles concurrent reads and writes without contention
- The batch writer sustains thousands of messages per second on commodity hardware
- FTS5 with porter stemming adds minimal overhead over plain SQL queries
- `PRAGMA cache_size=-64000` allocates ~64 MB page cache per connection
- `PRAGMA synchronous=NORMAL` balances durability and throughput
- Connection pool (default 8) satisfies concurrent MCP requests without blocking

For higher ingest rates (IoT, high-traffic network devices):

- Increase `CORTEX_BATCH_SIZE` (e.g. `500`) to reduce transaction overhead
- Increase `CORTEX_FLUSH_INTERVAL` (e.g. `1000` ms) to widen batch windows
- Increase `CORTEX_WRITE_CHANNEL_CAPACITY` (e.g. `100000`) to absorb bursts
- Increase `CORTEX_POOL_SIZE` (e.g. `8`) for more read concurrency
- Place the database on an SSD or tmpfs-backed volume

---

## MCP Transport

The daemon implements MCP through RMCP Streamable HTTP in stateless JSON-response mode.

- `POST /mcp` — RMCP Streamable HTTP request/response endpoint
- `GET /mcp` and `DELETE /mcp` — `405 Method Not Allowed` in stateless mode
- `GET /health` — unauthenticated health probe
- `cortex mcp` — local query-only stdio MCP mode for clients that launch MCP servers as child processes

When `CORTEX_TOKEN` is set, `/mcp` requires:

```
Authorization: Bearer <token>
```

`/health` is always unauthenticated (required for Docker health checks and reverse-proxy probes).

Stdio mode does not use bearer auth because it is local child-process access. It does require `CORTEX_DB_PATH` to point at the same SQLite database populated by the daemon:

```json
{
  "mcpServers": {
    "cortex": {
      "command": "/path/to/cortex",
      "args": ["mcp"],
      "env": {
        "CORTEX_DB_PATH": "/data/cortex.db",
        "RUST_LOG": "warn"
      }
    }
  }
}
```

Use `mcp-remote` instead of direct stdio when the database is only reachable through the running HTTP daemon or a reverse proxy.

The Docker image remains daemon-focused and exposes HTTP MCP via `cortex serve mcp`; use `cortex mcp` on a host that can read the SQLite DB for direct local stdio.

---

## Related Files

| File | Description |
|------|-------------|
| `Cargo.toml` | Crate metadata and dependency surface |
| `config.toml` | Default runtime configuration |
| `.env.example` | Canonical environment variable reference |
| `docs/SETUP.md` | Per-device syslog forwarder setup notes |
| `CHANGELOG.md` | Release history |
| `config/Dockerfile` | Container image definition |
| `docker-compose.yml` | Docker Compose stack |
| `Justfile` | Development command shortcuts |
| `src/main.rs` | `cortex` binary entrypoint for HTTP and stdio MCP modes |
| `src/lib.rs` | Reusable library boundary |
| `src/app/` | Shared typed log application service |
| `src/runtime.rs` | Config, DB, syslog, and maintenance orchestration |
| `src/api.rs` | Always-on non-MCP JSON API routes (`/api/*`, token-gated) |
| `src/config.rs` | Configuration loading and validation |
| `src/db.rs` + `src/db/` | SQLite schema, FTS5, retention, storage budget |
| `src/syslog.rs` + `src/syslog/` | UDP/TCP listeners, syslog parser, batch writer |
| `src/mcp.rs` + `src/mcp/` | MCP HTTP server, RMCP adapter, auth middleware, tools, health endpoint |
| `.claude-plugin/plugin.json` | Claude plugin manifest |

---

## Related plugins

| Plugin | Category | Description |
|--------|----------|-------------|
| [homelab-core](https://github.com/jmagar/claude-homelab) | core | Core agents, commands, skills, and setup/health workflows for homelab management. |
| [overseerr-mcp](https://github.com/jmagar/overseerr-mcp) | media | Search movies and TV shows, submit requests, and monitor failed requests via Overseerr. |
| [unraid-mcp](https://github.com/jmagar/unraid-mcp) | infrastructure | Query, monitor, and manage Unraid servers: Docker, VMs, array, parity, and live telemetry. |
| [unifi-mcp](https://github.com/jmagar/unifi-mcp) | infrastructure | Monitor and manage UniFi devices, clients, firewall rules, and network health. |
| [gotify-mcp](https://github.com/jmagar/gotify-mcp) | utilities | Send and manage push notifications via a self-hosted Gotify server. |
| [swag-mcp](https://github.com/jmagar/swag-mcp) | infrastructure | Create, edit, and manage SWAG nginx reverse proxy configurations. |
| [synapse-mcp](https://github.com/jmagar/synapse-mcp) | infrastructure | Docker management (Flux) and SSH remote operations (Scout) across homelab hosts. |
| [arcane-mcp](https://github.com/jmagar/arcane-mcp) | infrastructure | Manage Docker environments, containers, images, volumes, networks, and GitOps via Arcane. |
| [plugin-lab](https://github.com/jmagar/plugin-lab) | dev-tools | Scaffold, review, align, and deploy homelab MCP plugins with agents and canonical templates. |

## License

MIT
