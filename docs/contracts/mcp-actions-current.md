# MCP Actions Contract — Current Production Surface (Baseline)

## 1. Purpose & Pinning

Contract derived from `src/mcp/tools.rs::tool_syslog` dispatch and
`src/mcp/schemas.rs::tool_definitions` as of commit `6640f5d` (branch `main`,
2026-05-16). The wider request/response shapes come from `src/app/models.rs`
and the dispatch sites in `src/mcp/tools.rs`.

This file pins the **currently-existing** MCP action surface of `syslog-mcp`.
Changing any action's parameter names, parameter caps/defaults, or top-level
response keys is a **breaking change** — coordinate with downstream consumers
before editing:

- `plugins/skills/syslog-*/SKILL.md` (Claude Code skills consume these
  shapes by name)
- `plugins/skills/syslog-report/REPORT-SCHEMA.md` (and any operator-side
  scripting)
- Out-of-tree fleet runbooks that depend on the JSON envelope

Additive changes (new optional parameters, new optional response fields) are
non-breaking provided existing callers continue to work without modification.
Renaming, removing, or tightening defaults/caps requires a major version bump.

### 1.1 Tool surface

The MCP server exposes a single tool named `syslog`. Every operation is
selected by the required string parameter `action`. The valid `action` set
is `SYSLOG_ACTIONS` in `src/mcp/schemas.rs` (29 actions, listed below). The
JSON schema in `schemas.rs::tool_definitions()` is a **union** over all
actions: every property is declared at the top level with a `description`
indicating which actions consume it. Per-action validation lives in the
dispatch handlers in `src/mcp/tools.rs` and in `src/app/service.rs`.

### 1.2 Response envelope

Successful action responses are returned as a single MCP text content block
whose `text` is pretty-printed JSON of the action's response struct (defined
in `src/app/models.rs`). Errors propagate as `anyhow::Error` from the
handler, surfaced by the RMCP transport as a tool error (`isError: true`)
with the error message in `content[0].text`.

Each "Response shape" block below is the JSON projection of the
corresponding Rust response struct — fields with
`#[serde(skip_serializing_if = "Option::is_none")]` are documented inline.

## 2. Action Index

| Action | Purpose | Params summary | Response summary | Introduced |
|---|---|---|---|---|
| `search` | FTS5 + structured search over `logs` | `query?`, `hostname?`, `source_ip?`, `severity?`, `app_name?`, `facility?`, `process_id?`, `from?`, `to?`, `limit?` | `{count, logs[]}` | early (pre-0.1) |
| `tail` | Most-recent N entries with filters | `hostname?`, `source_ip?`, `app_name?`, `severity_min?`, `n?` | `{count, logs[]}` | early (pre-0.1) |
| `errors` | Error/warning summary grouped by host + severity | `from?`, `to?`, `group_by?` | `{summary[]}` | early (pre-0.1) |
| `hosts` | Host registry with first/last seen | (none) | `{hosts[]}` | early (pre-0.1) |
| `apps` | Application inventory with log counts | `hostname?` | `{apps[]}` | 0.1.x |
| `sessions` | AI transcript sessions grouped by project/tool/session/host | `project?`, `tool?`, `hostname?`, `from?`, `to?`, `limit?` | `{count, sessions[]}` | 0.1.x (AI scanner) |
| `search_sessions` | Session-ranked FTS over AI transcript rows | `query`, `project?`, `tool?`, `from?`, `to?`, `limit?` | `{total_candidates, candidate_rows, candidate_cap, candidate_window_truncated, truncated, sessions[]}` | 0.1.x |
| `abuse` | Abuse-term detector with same-session context | `project?`, `tool?`, `from?`, `to?`, `limit?`, `before?`, `after?`, `terms?` | `{terms[], candidate_rows, candidate_cap, candidate_window_truncated, truncated, matches[]}` | 0.1.x |
| `ai_correlate` | AI transcript anchors → nearby non-AI logs | `project?`, `tool?`, `session_id?`, `ai_query?`, `log_query?`, `hostname?`, `source_ip?`, `app_name?`, `from?`, `to?`, `window_minutes?`, `severity_min?`, `limit?`, `events_per_anchor?` | `{window_minutes, severity_min, total_anchors, anchor_rows, anchor_limit, anchors_truncated, related_limit_per_anchor, total_related_events, anchors[]}` | 0.1.x |
| `usage_blocks` | AI activity bucketed into 5-hour UTC windows | `project?`, `tool?`, `from?`, `to?` | `{total_blocks, truncated, blocks[]}` | 0.1.x |
| `project_context` | Summary of one project (tools, sessions, hosts, recent) | `project`, `tool?`, `limit?` | `{project, tools[], sessions[], hostnames[], first_seen?, last_seen?, event_count, recent_entries_truncated, recent_entries[]}` | 0.1.x |
| `list_ai_tools` | Distinct AI tools with counts | `project?`, `from?`, `to?` | `{total_tools, truncated, tools[]}` | 0.1.x |
| `list_ai_projects` | Distinct AI projects with counts | `tool?`, `from?`, `to?` | `{total_projects, truncated, projects[]}` | 0.1.x |
| `correlate` | Multi-host event correlation around a reference time | `reference_time`, `window_minutes?`, `severity_min?`, `hostname?`, `source_ip?`, `query?`, `limit?` | `{reference_time, window_minutes, window_from, window_to, severity_min, total_events, truncated, hosts_count, hosts[]}` | early (pre-0.1) |
| `stats` | DB stats + runtime observability + OTLP counters | (none) | `DbStats + {runtime_observability, otlp}` | early (pre-0.1) |
| `status` | Lightweight health + runtime/OTLP counters | (none) | `{status, db_ok, runtime_observability, otlp}` | early (pre-0.1) |
| `source_ips` | Distinct verified source identifiers | (none) | `{source_ips[]}` | 0.1.x |
| `timeline` | Bucketed log counts (minute/hour/day) | `bucket?`, `group_by?`, `from?`, `to?`, `hostname?`, `app_name?`, `severity_min?` | `{bucket, group_by?, points[]}` | 0.1.x |
| `patterns` | Cluster near-duplicate messages by template | `from?`, `to?`, `hostname?`, `app_name?`, `severity_min?`, `scan_limit?`, `top_n?` | `{patterns[], scanned, truncated}` | 0.1.x |
| `context` | Surrounding logs around a single point of interest | `log_id?`, `hostname?`, `timestamp?`, `before?`, `after?` | `{reference, before[], after[]}` | 0.1.x |
| `get` | Fetch one log entry by id (incl. raw frame) | `id` | `{log}` | 0.1.x |
| `ingest_rate` | 1m / 5m / 15m throughput + write-block flag | `by_host?` | `{now, buckets, write_blocked, by_host?}` | 0.1.x |
| `silent_hosts` | Hosts stale for > N minutes | `silent_minutes?` | `{silent_minutes, cutoff, now, hosts[]}` | 0.1.x |
| `clock_skew` | Per-host `received_at - timestamp` distribution | `since?` | `{since, hosts[]}` | 0.1.x |
| `anomalies` | Recent vs baseline volume z-score per host | `recent_minutes?`, `baseline_minutes?` | `{recent_from, recent_to, baseline_from, baseline_to, recent_minutes, baseline_minutes, hosts[]}` | 0.1.x |
| `compare` | Side-by-side summary of two ranges with deltas | `a_from`, `a_to`, `b_from`, `b_to` | `{a, b, delta_total_logs, delta_total_errors}` | 0.1.x |
| `compose_status` | Read-only Docker Compose diagnostics (MCP-safe) | (none, rejects target overrides) | `ComposeMcpStatus` | 0.20.x |
| `compose_doctor` | Strict Compose deployment-health check | (none, rejects target overrides) | `ComposeMcpStatus` (or tool error) | 0.20.x |
| `help` | Markdown reference for the `syslog` tool | (none) | `{help}` | 0.1.x |

29 actions total. Every action listed in `SYSLOG_ACTIONS` (`src/mcp/schemas.rs`)
is present in `tool_syslog`'s dispatch match (`src/mcp/tools.rs`).

## 3. Common values

### 3.1 Severity enum
`SEVERITY_LEVELS` (`src/db/queries.rs`):

```json
["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"]
```

Numeric values 0..7 (emerg=0, debug=7). `severity_min` filters by **<=**
numeric level (lower number = higher severity).

### 3.2 AI tool enum
The `tool` parameter accepts: `"claude"`, `"codex"`, `"gemini"`.

### 3.3 `group_by` enum (cross-action union)
Declared in the union schema: `["app_name", "hostname", "host", "severity", "sev", "app"]`.
Per-action interpretation:
- `errors`: only `app_name` is supported (anything else is ignored as the default host+severity grouping).
- `timeline`: `hostname`, `severity`, or `app_name`; aliases `host` → `hostname`, `sev` → `severity`, `app` → `app_name`.

### 3.4 `bucket` enum (`timeline` only)
`["minute", "min", "m", "hour", "h", "day", "d"]`. Default: `hour`.

### 3.5 `source_ip` semantics
Network-verified identity:
- Syslog: `IP:port` (verified from the UDP/TCP socket peer).
- OTLP: peer IP.
- Docker stream: `docker://host/container/stream`.
- Docker lifecycle event: `docker-event://host/container/action`.

### 3.6 `LogEntry` shape (used by every action that returns log rows)

```json
{
  "id": 0,
  "timestamp": "2026-05-16T00:00:00.000Z",
  "hostname": "string",
  "facility": "string|null",
  "severity": "string",
  "app_name": "string|null",
  "process_id": "string|null",
  "message": "string",
  "received_at": "2026-05-16T00:00:00.000Z",
  "source_ip": "string",
  "ai_tool": "string|null",
  "ai_project": "string|null",
  "ai_session_id": "string|null",
  "ai_transcript_path": "string|null",
  "metadata_json": "string|null"
}
```

`LogEntryWithRaw` (only returned by `get`) adds `"raw": "string"`.

---

## 4. Action specifications

### search

**Source:** `src/mcp/tools.rs::tool_search_logs` → `src/app/service.rs::search_logs`.
Schema fields drawn from `src/mcp/schemas.rs::tool_definitions` and the
help text in `tool_syslog_help`.

**Purpose:** Full-text + structured search across `logs`. Uses SQLite FTS5
with `porter unicode61` tokenizer. Supports FTS5 syntax: AND, OR, NOT
(bare `-`), phrase matching (double quotes), prefix matching (`*`).

**Params (JSON Schema, draft 2020-12):**

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "action":     { "const": "search" },
    "query":      { "type": "string", "description": "FTS5 query. Hyphen is the FTS5 NOT operator; quote hyphenated terms as phrases, e.g. \"smoke-test\"." },
    "hostname":   { "type": "string", "description": "Exact hostname filter." },
    "source_ip":  { "type": "string", "description": "Exact source identifier." },
    "severity":   { "type": "string", "enum": ["emerg","alert","crit","err","warning","notice","info","debug"] },
    "app_name":   { "type": "string" },
    "facility":   { "type": "string" },
    "process_id": { "type": "string" },
    "from":       { "type": "string", "description": "ISO 8601 / RFC 3339" },
    "to":         { "type": "string", "description": "ISO 8601 / RFC 3339" },
    "limit":      { "type": "integer" }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{ "count": 0, "logs": [ /* LogEntry */ ] }
```

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `limit` | 100 | 1000 |

**Example:**

Request:
```json
{ "action": "search", "query": "OOM AND killer", "severity": "warning", "limit": 50 }
```

Response:
```json
{ "count": 2, "logs": [{ "id": 421, "hostname": "dookie", "severity": "err", "message": "Out of memory: Killed process 1234 (chrome)", "...": "..." }] }
```

---

### tail

**Source:** `src/mcp/tools.rs::tool_tail_logs` → `src/app/service.rs::tail_logs`.

**Purpose:** N most-recent entries, optionally filtered by host / source /
app / severity floor.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":       { "const": "tail" },
    "hostname":     { "type": "string" },
    "source_ip":    { "type": "string" },
    "app_name":     { "type": "string" },
    "severity_min": { "type": "string", "enum": ["emerg","alert","crit","err","warning","notice","info","debug"] },
    "n":            { "type": "integer" }
  },
  "required": ["action"]
}
```

**Response shape:** identical to `search` — `{count, logs[]}`. (Internally
returns a `SearchLogsResponse`; documented here as a separate shape because
operators key off `tail`'s narrower semantic.)

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `n` | 50 | 500 |

**Example:**
```json
{ "action": "tail", "hostname": "tootie", "severity_min": "warning", "n": 20 }
```

---

### errors

**Source:** `src/mcp/tools.rs::tool_get_errors` → `src/app/service.rs::get_errors`.

**Purpose:** Summary of errors/warnings across all hosts in a time window.
Always groups by hostname + severity; optional secondary key `app_name`.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":   { "const": "errors" },
    "from":     { "type": "string", "description": "ISO 8601; defaults to all time" },
    "to":       { "type": "string", "description": "ISO 8601; defaults to now" },
    "group_by": { "type": "string", "enum": ["app_name"] }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "summary": [
    { "hostname": "string", "app_name": "string?", "severity": "string", "count": 0 }
  ]
}
```

`app_name` is present iff `group_by=app_name` was requested.

**Caps + defaults:** none beyond the time-window inputs.

---

### hosts

**Source:** `src/mcp/tools.rs::tool_list_hosts` → `src/app/service.rs::list_hosts`.

**Purpose:** Every host that has ever sent syslog, with first/last seen and
total log count.

**Params:** `{ "action": "hosts" }`. No other inputs.

**Response shape:**

```json
{
  "hosts": [
    { "hostname": "string", "first_seen": "RFC3339", "last_seen": "RFC3339", "log_count": 0 }
  ]
}
```

---

### apps

**Source:** `src/mcp/tools.rs::tool_list_apps` → `src/app/service.rs::list_apps`.

**Purpose:** Distinct `app_name` values with log counts, host counts, and
first/last seen timestamps. Mirror of `hosts` for the app dimension.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":   { "const": "apps" },
    "hostname": { "type": "string", "description": "Restrict to apps seen on this host." }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "apps": [
    { "app_name": "string", "log_count": 0, "host_count": 0, "first_seen": "RFC3339", "last_seen": "RFC3339" }
  ]
}
```

---

### sessions

**Source:** `src/mcp/tools.rs::tool_list_sessions` → `src/app/service.rs::list_sessions`.

**Purpose:** AI transcript sessions grouped by host/tool/project/session.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":   { "const": "sessions" },
    "project":  { "type": "string", "description": "Exact project path filter." },
    "tool":     { "type": "string", "enum": ["claude","codex","gemini"] },
    "hostname": { "type": "string" },
    "from":     { "type": "string" },
    "to":       { "type": "string" },
    "limit":    { "type": "integer" }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "count": 0,
  "sessions": [
    {
      "session_key": "string",
      "project": "string",
      "tool": "string",
      "session_id": "string",
      "transcript_path": "string?",
      "hostname": "string",
      "first_seen": "RFC3339",
      "last_seen": "RFC3339",
      "event_count": 0
    }
  ]
}
```

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `limit` | 100 | 1000 |

---

### search_sessions

**Source:** `src/mcp/tools.rs::tool_search_sessions` → `src/app/service.rs::search_sessions`.

**Purpose:** Session-ranked FTS over AI transcript rows. Returns grouped
sessions (not flat rows) with match counts and best snippets.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":  { "const": "search_sessions" },
    "query":   { "type": "string", "description": "FTS5 query (required)." },
    "project": { "type": "string" },
    "tool":    { "type": "string", "enum": ["claude","codex","gemini"] },
    "from":    { "type": "string" },
    "to":      { "type": "string" },
    "limit":   { "type": "integer" }
  },
  "required": ["action", "query"]
}
```

**Response shape:**

```json
{
  "total_candidates": 0,
  "candidate_rows": 0,
  "candidate_cap": 0,
  "candidate_window_truncated": false,
  "truncated": false,
  "sessions": [
    {
      "session_key": "string",
      "project": "string",
      "tool": "string",
      "session_id": "string",
      "hostname": "string",
      "first_seen": "RFC3339",
      "last_seen": "RFC3339",
      "event_count": 0,
      "match_count": 0,
      "best_snippet": "string?"
    }
  ]
}
```

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `limit` | 20 | 100 |

---

### abuse

**Source:** `src/mcp/tools.rs::tool_search_abuse` → `src/app/service.rs::search_abuse`.

**Purpose:** Detect abuse-terms in AI transcript rows; surface each hit with
same-session before/after context.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":  { "const": "abuse" },
    "project": { "type": "string" },
    "tool":    { "type": "string", "enum": ["claude","codex","gemini"] },
    "from":    { "type": "string" },
    "to":      { "type": "string" },
    "limit":   { "type": "integer" },
    "before":  { "type": "integer" },
    "after":   { "type": "integer" },
    "terms":   {
      "oneOf": [
        { "type": "array", "items": { "type": "string" } },
        { "type": "string" }
      ],
      "description": "Optional custom detector terms; replaces built-in list. String form accepted for CLI bridges."
    }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "terms": ["string"],
  "candidate_rows": 0,
  "candidate_cap": 0,
  "candidate_window_truncated": false,
  "truncated": false,
  "matches": [
    { "term": "string", "entry": { /* LogEntry */ }, "before": [/* LogEntry */], "after": [/* LogEntry */] }
  ]
}
```

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `limit` | 20 | 100 |
| `before` | 2 | 20 |
| `after` | 2 | 20 |

---

### ai_correlate

**Source:** `src/mcp/tools.rs::tool_ai_correlate` → `src/app/service.rs::correlate_ai_logs`.

**Purpose:** Cross-reference AI transcript anchor rows against nearby
**non-AI** logs in the same DB. Related rows explicitly exclude AI
transcript rows so the result surfaces host / Docker / OTLP / syslog
context around each anchor.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":            { "const": "ai_correlate" },
    "project":           { "type": "string" },
    "tool":              { "type": "string", "enum": ["claude","codex","gemini"] },
    "session_id":        { "type": "string" },
    "ai_query":          { "type": "string", "description": "FTS5 over AI anchor rows." },
    "log_query":         { "type": "string", "description": "FTS5 over related non-AI logs." },
    "hostname":          { "type": "string" },
    "source_ip":         { "type": "string" },
    "app_name":          { "type": "string" },
    "from":              { "type": "string" },
    "to":                { "type": "string" },
    "window_minutes":    { "type": "integer" },
    "severity_min":      { "type": "string", "enum": ["emerg","alert","crit","err","warning","notice","info","debug"] },
    "limit":             { "type": "integer" },
    "events_per_anchor": { "type": "integer" }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "window_minutes": 0,
  "severity_min": "string",
  "total_anchors": 0,
  "anchor_rows": 0,
  "anchor_limit": 0,
  "anchors_truncated": false,
  "related_limit_per_anchor": 0,
  "total_related_events": 0,
  "anchors": [
    {
      "entry": { /* LogEntry (anchor AI row) */ },
      "window_from": "RFC3339",
      "window_to": "RFC3339",
      "related": [ /* LogEntry */ ],
      "related_truncated": false
    }
  ]
}
```

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `window_minutes` | 5 | 120 |
| `severity_min` | `warning` | n/a |
| `limit` | 10 | 50 |
| `events_per_anchor` | 25 | 200 |

---

### usage_blocks

**Source:** `src/mcp/tools.rs::tool_usage_blocks` → `src/app/service.rs::usage_blocks`.

**Purpose:** AI activity bucketed into deterministic 5-hour UTC windows.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":  { "const": "usage_blocks" },
    "project": { "type": "string" },
    "tool":    { "type": "string", "enum": ["claude","codex","gemini"] },
    "from":    { "type": "string" },
    "to":      { "type": "string" }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "total_blocks": 0,
  "truncated": false,
  "blocks": [
    {
      "bucket_start": "RFC3339",
      "bucket_end": "RFC3339",
      "project": "string",
      "tool": "string",
      "session_count": 0,
      "event_count": 0
    }
  ]
}
```

**Caps + defaults:** truncation flag set when the underlying DB cap is hit
(documented in `src/db/queries.rs`); not separately tunable from the MCP
surface.

---

### project_context

**Source:** `src/mcp/tools.rs::tool_project_context` → `src/app/service.rs::project_context`.

**Purpose:** Summary of one project (path) including tools used, distinct
sessions, hostnames, total event count, and recent representative entries
with 256-char message snippets.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":  { "const": "project_context" },
    "project": { "type": "string", "description": "Exact project path (required)." },
    "tool":    { "type": "string", "enum": ["claude","codex","gemini"] },
    "limit":   { "type": "integer" }
  },
  "required": ["action", "project"]
}
```

**Response shape:**

```json
{
  "project": "string",
  "tools": ["string"],
  "sessions": ["string"],
  "hostnames": ["string"],
  "first_seen": "RFC3339?",
  "last_seen": "RFC3339?",
  "event_count": 0,
  "recent_entries_truncated": false,
  "recent_entries": [ /* LogEntry with message snippet capped at 256 chars */ ]
}
```

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `limit` | 5 | 20 |

---

### list_ai_tools

**Source:** `src/mcp/tools.rs::tool_list_ai_tools` → `src/app/service.rs::list_ai_tools`.

**Purpose:** Distinct AI tools with event/session counts and first/last seen.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":  { "const": "list_ai_tools" },
    "project": { "type": "string" },
    "from":    { "type": "string" },
    "to":      { "type": "string" }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "total_tools": 0,
  "truncated": false,
  "tools": [
    { "tool": "string", "event_count": 0, "session_count": 0, "first_seen": "RFC3339", "last_seen": "RFC3339" }
  ]
}
```

**Caps + defaults:** results capped at 100 (DB layer); `truncated` set when
hit.

---

### list_ai_projects

**Source:** `src/mcp/tools.rs::tool_list_ai_projects` → `src/app/service.rs::list_ai_projects`.

**Purpose:** Distinct AI projects with counts, tools used, and first/last seen.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action": { "const": "list_ai_projects" },
    "tool":   { "type": "string", "enum": ["claude","codex","gemini"] },
    "from":   { "type": "string" },
    "to":     { "type": "string" }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "total_projects": 0,
  "truncated": false,
  "projects": [
    { "project": "string", "tools": ["string"], "event_count": 0, "session_count": 0, "first_seen": "RFC3339", "last_seen": "RFC3339" }
  ]
}
```

**Caps + defaults:** results capped at 200 (DB layer); `truncated` set when
hit.

---

### correlate

**Source:** `src/mcp/tools.rs::tool_correlate_events` → `src/app/service.rs::correlate_events`.

**Purpose:** Find related events across multiple hosts within ±N minutes of
a reference timestamp. Used for debugging cascading failures.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":         { "const": "correlate" },
    "reference_time": { "type": "string", "description": "Center timestamp (ISO 8601), required." },
    "window_minutes": { "type": "integer" },
    "severity_min":   { "type": "string", "enum": ["emerg","alert","crit","err","warning","notice","info","debug"] },
    "hostname":       { "type": "string" },
    "source_ip":      { "type": "string" },
    "query":          { "type": "string", "description": "Optional FTS5 to narrow." },
    "limit":          { "type": "integer" }
  },
  "required": ["action", "reference_time"]
}
```

**Response shape:**

```json
{
  "reference_time": "RFC3339",
  "window_minutes": 0,
  "window_from": "RFC3339",
  "window_to": "RFC3339",
  "severity_min": "string",
  "total_events": 0,
  "truncated": false,
  "hosts_count": 0,
  "hosts": [
    { "hostname": "string", "event_count": 0, "events": [ /* LogEntry */ ] }
  ]
}
```

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `window_minutes` | 5 | 60 |
| `severity_min` | `warning` (`debug` returns everything) | n/a |
| `limit` | 500 | 999 |

---

### stats

**Source:** `src/mcp/tools.rs::tool_get_stats` → `src/app/service.rs::get_stats`,
then augmented inline with `runtime_observability` (from
`state.observability.snapshot()`) and `otlp` (`state.otlp_counters`).

**Purpose:** Database statistics + runtime ingest observability + OTLP
receiver counters. Heavyweight: walks the `logs` and `hosts` tables.

**Params:** `{ "action": "stats" }`. No other inputs.

**Response shape:**

```json
{
  "total_logs": 0,
  "total_hosts": 0,
  "oldest_log": "RFC3339?",
  "newest_log": "RFC3339?",
  "logical_db_size_mb": "string",
  "physical_db_size_mb": "string",
  "free_disk_mb": "string?",
  "max_db_size_mb": 0,
  "min_free_disk_mb": 0,
  "write_blocked": false,
  "phantom_fts_rows": 0,
  "runtime_observability": { /* opaque; counters + queue depth + writer flush/failure/drop */ },
  "otlp": {
    "logs_received": 0,
    "decode_errors": 0
  }
}
```

---

### status

**Source:** `src/mcp/tools.rs::tool_get_status`.

**Purpose:** Lightweight runtime status WITHOUT walking the `logs` table.
Intended for dashboards and doctor checks that need queue/backpressure
state quickly.

**Params:** `{ "action": "status" }`. No other inputs.

**Response shape:**

```json
{
  "status": "ok|error",
  "db_ok": true,
  "runtime_observability": { /* same shape as in stats */ },
  "otlp": {
    "logs_received": 0,
    "decode_errors": 0
  }
}
```

---

### source_ips

**Source:** `src/mcp/tools.rs::tool_list_source_ips` → `src/app/service.rs::list_source_ips`.

**Purpose:** List distinct verified source identifiers (see §3.5) with log
counts, claimed hostname counts, and the top hostnames per sender. Useful
for spoof detection on hostname-spoofable formats (UniFi CEF, etc.).

**Params:** `{ "action": "source_ips" }`. No other inputs.

**Response shape:**

```json
{
  "source_ips": [
    {
      "source_ip": "string",
      "log_count": 0,
      "host_count": 0,
      "first_seen": "RFC3339",
      "last_seen": "RFC3339",
      "hostnames": [
        { "hostname": "string", "log_count": 0 }
      ]
    }
  ]
}
```

**Caps + defaults:** top hostnames per sender capped at 10 in the DB layer.

---

### timeline

**Source:** `src/mcp/tools.rs::tool_timeline` → `src/app/service.rs::timeline`.

**Purpose:** Bucketed log counts over a time range. Answers "when did errors
start" / "is the incident still active".

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":       { "const": "timeline" },
    "bucket":       { "type": "string", "enum": ["minute","min","m","hour","h","day","d"] },
    "group_by":     { "type": "string", "enum": ["hostname","host","severity","sev","app_name","app"] },
    "from":         { "type": "string" },
    "to":           { "type": "string" },
    "hostname":     { "type": "string" },
    "app_name":     { "type": "string" },
    "severity_min": { "type": "string", "enum": ["emerg","alert","crit","err","warning","notice","info","debug"] }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "bucket": "minute|hour|day",
  "group_by": "hostname|severity|app_name|null",
  "points": [
    { "bucket": "RFC3339 (bucket start)", "group": "string?", "count": 0 }
  ]
}
```

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `bucket` | `hour` | n/a |

---

### patterns

**Source:** `src/mcp/tools.rs::tool_patterns` → `src/app/service.rs::patterns`.

**Purpose:** Cluster near-duplicate messages by template. Variable runs
(numbers, IPv4, UUIDs, long hex) are normalised to placeholders. Returns
top templates with counts, a sample, and host distribution.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":       { "const": "patterns" },
    "from":         { "type": "string" },
    "to":           { "type": "string" },
    "hostname":     { "type": "string" },
    "app_name":     { "type": "string" },
    "severity_min": { "type": "string", "enum": ["emerg","alert","crit","err","warning","notice","info","debug"] },
    "scan_limit":   { "type": "integer" },
    "top_n":        { "type": "integer" }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "patterns": [
    {
      "template": "string",
      "count": 0,
      "host_count": 0,
      "sample": "string",
      "first_seen": "RFC3339",
      "last_seen": "RFC3339",
      "hostnames": ["string"]
    }
  ],
  "scanned": 0,
  "truncated": false
}
```

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `scan_limit` | 10000 | 50000 |
| `top_n` | 20 | 200 |

---

### context

**Source:** `src/mcp/tools.rs::tool_context` → `src/app/service.rs::context`.

**Purpose:** Surrounding logs around a single anchor on the same host. Pass
`log_id` (preferred — stable `(timestamp, id)` ordering) OR
`hostname` + `timestamp` for a synthetic reference.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":    { "const": "context" },
    "log_id":    { "type": "integer" },
    "hostname":  { "type": "string", "description": "Required when log_id is absent." },
    "timestamp": { "type": "string", "description": "Required when log_id is absent (ISO 8601)." },
    "before":    { "type": "integer" },
    "after":     { "type": "integer" }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "reference": { /* LogEntry */ },
  "before": [ /* LogEntry */ ],
  "after":  [ /* LogEntry */ ]
}
```

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `before` | 10 | 500 |
| `after`  | 10 | 500 |

---

### get

**Source:** `src/mcp/tools.rs::tool_get_log` → `src/app/service.rs::get_log`.

**Purpose:** Fetch one log entry by `id`, including the unparsed `raw`
syslog frame. The only action that returns `raw`.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action": { "const": "get" },
    "id":     { "type": "integer", "description": "Required." }
  },
  "required": ["action", "id"]
}
```

**Response shape:**

```json
{
  "log": { /* LogEntryWithRaw — LogEntry plus "raw": "string" */ }
}
```

---

### ingest_rate

**Source:** `src/mcp/tools.rs::tool_ingest_rate` → `src/app/service.rs::ingest_rate`.

**Purpose:** Recent ingest throughput (1m / 5m / 15m, counts + per-second
rates) computed against `received_at`. Includes the current write-block
flag for live ingest health.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":  { "const": "ingest_rate" },
    "by_host": { "type": "boolean", "description": "Also include per-host buckets." }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "now": "RFC3339",
  "buckets": {
    "last_1m": 0, "last_5m": 0, "last_15m": 0,
    "per_sec_1m": 0.0, "per_sec_5m": 0.0, "per_sec_15m": 0.0
  },
  "write_blocked": false,
  "by_host": [
    { "hostname": "string", "last_1m": 0, "last_5m": 0, "last_15m": 0 }
  ]
}
```

`by_host` is present iff `by_host=true` was requested.

---

### silent_hosts

**Source:** `src/mcp/tools.rs::tool_silent_hosts` → `src/app/service.rs::silent_hosts`.

**Purpose:** Hosts whose `last_seen` is older than `silent_minutes` ago.
Reports the typical inter-arrival interval so noisy devices that go silent
are easy to spot.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":         { "const": "silent_hosts" },
    "silent_minutes": { "type": "integer" }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "silent_minutes": 0,
  "cutoff": "RFC3339",
  "now": "RFC3339",
  "hosts": [
    {
      "hostname": "string",
      "first_seen": "RFC3339",
      "last_seen": "RFC3339",
      "log_count": 0,
      "typical_interval_secs": 0.0,
      "silent_for_secs": 0
    }
  ]
}
```

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `silent_minutes` | 30 | 10080 (one week) |

---

### clock_skew

**Source:** `src/mcp/tools.rs::tool_clock_skew` → `src/app/service.rs::clock_skew`.

**Purpose:** Per-host distribution of `received_at - timestamp` (seconds),
sorted by absolute mean. Surfaces devices with a broken or drifting clock.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action": { "const": "clock_skew" },
    "since":  { "type": "string", "description": "Sample entries with received_at >= since (default last 24h)." }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "since": "RFC3339",
  "hosts": [
    {
      "hostname": "string",
      "samples": 0,
      "avg_skew_secs": 0.0,
      "min_skew_secs": 0.0,
      "max_skew_secs": 0.0
    }
  ]
}
```

---

### anomalies

**Source:** `src/mcp/tools.rs::tool_anomalies` → `src/app/service.rs::anomalies`.

**Purpose:** Per-host comparison of recent volume vs a baseline window.
Reports per-min rates, a ratio, and a Poisson-style z-score so callers can
rank hosts with unusual log rate or error count.

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action":           { "const": "anomalies" },
    "recent_minutes":   { "type": "integer" },
    "baseline_minutes": { "type": "integer" }
  },
  "required": ["action"]
}
```

**Response shape:**

```json
{
  "recent_from": "RFC3339", "recent_to": "RFC3339",
  "baseline_from": "RFC3339", "baseline_to": "RFC3339",
  "recent_minutes": 0,
  "baseline_minutes": 0,
  "hosts": [
    {
      "hostname": "string",
      "recent_count": 0,
      "baseline_count": 0,
      "recent_per_min": 0.0,
      "baseline_per_min": 0.0,
      "ratio": 0.0,
      "z_score": 0.0,
      "recent_errors": 0,
      "baseline_errors": 0
    }
  ]
}
```

`ratio` and `z_score` may be null when the baseline is empty.

**Caps + defaults:**

| Param | Default | Max |
|---|---|---|
| `recent_minutes` | 15 | 1440 |
| `baseline_minutes` | 360 | 10080 |

---

### compare

**Source:** `src/mcp/tools.rs::tool_compare` → `src/app/service.rs::compare`.

**Purpose:** Side-by-side summary of two time ranges with deltas. Answers
"what changed since yesterday".

**Params:**

```json
{
  "type": "object",
  "properties": {
    "action": { "const": "compare" },
    "a_from": { "type": "string" },
    "a_to":   { "type": "string" },
    "b_from": { "type": "string" },
    "b_to":   { "type": "string" }
  },
  "required": ["action", "a_from", "a_to", "b_from", "b_to"]
}
```

**Response shape:**

```json
{
  "a": {
    "from": "RFC3339", "to": "RFC3339",
    "total_logs": 0, "total_errors": 0,
    "by_severity": [["severity", 0]],
    "top_hosts":   [["hostname", 0]],
    "top_apps":    [["app_name", 0]]
  },
  "b": { /* same shape */ },
  "delta_total_logs": 0,
  "delta_total_errors": 0
}
```

`by_severity`, `top_hosts`, `top_apps` are serialised as JSON tuples
`[string, integer]` per row.

---

### compose_status

**Source:** `src/mcp/tools.rs::tool_compose_status` → `src/compose::ComposeService::status` →
`src/compose::mcp_projection`.

**Purpose:** Read-only Docker Compose diagnostics for the canonical
syslog-mcp deployment. MCP-safe projection: host paths, image ids, mount
sources, and raw command output are stripped.

**Params:** `{ "action": "compose_status" }`. The handler rejects target
override fields (`container`, `container_name`, `project_dir`,
`compose_file`, `project_name`, `service`) with a tool error.

**Response shape (`ComposeMcpStatus` from `src/compose.rs`):**

```json
{
  "container_name": "string",
  "ownership": "compose_owned|owner_mismatch|systemd_owned|unknown",
  "runtime_state": "healthy|degraded|stopped|docker_unavailable|unknown",
  "health": "string?",
  "published_ports": [ { "port": 0, "protocol": "tcp|udp" } ],
  "diagnostics": [ { "severity": "string", "code": "string" } ]
}
```

---

### compose_doctor

**Source:** `src/mcp/tools.rs::tool_compose_doctor` → `src/compose::ensure_doctor_ready`.

**Purpose:** Strict deployment-health check for the canonical syslog-mcp
Compose deployment. Returns the same redacted `ComposeMcpStatus` shape as
`compose_status` when healthy. Returns a **tool error** (not a structured
response) when Docker/Compose ownership or runtime checks are not ready for
lifecycle work. Lifecycle mutations themselves remain CLI-only.

**Params:** identical to `compose_status` (rejects the same target override
fields).

**Response shape:** identical to `compose_status` on success; tool error
otherwise.

---

### help

**Source:** `src/mcp/tools.rs::tool_syslog_help`.

**Purpose:** Returns the in-tree markdown reference for the `syslog` tool.

**Params:** `{ "action": "help" }`. No other inputs.

**Response shape:**

```json
{ "help": "string (markdown)" }
```

---

## 5. Modifications Introduced by the 6 Epics

The six 2026-05-16 epics layer ONTO this baseline. For new-action specs, see
[`mcp-actions.md`](./mcp-actions.md) (15 new actions). For changes to the
existing actions documented above, the additive deltas are:

- **`status` (Epic C — API Pollers, `syslog-mcp-awvr`)**: response gains a
  `pollers` block (per-poller cursor / last_tick_at / last_error rolled up
  from `poller_checkpoints`). Existing keys (`status`, `db_ok`,
  `runtime_observability`, `otlp`) are unchanged.
- **`status` (Epic A — Agent Mode, `syslog-mcp-qgnx`)**: response gains an
  `agents` block summarising connection-state buckets from the new `agents`
  table (Active / Disconnected / NeverConnected / Revoked counts plus
  last_seen aggregates).
- **`search` (Epic B — Enrichment Framework, `syslog-mcp-1wjr`)**: filter
  params expand to include the four new indexed columns added by Epic B —
  `http_status` (integer), `auth_outcome` (enum), `dns_blocked` (0/1), and
  `event_action` (string). The existing FTS / hostname / severity / etc.
  filters are unchanged; the new filters AND-compose with them.
- **`correlate` (Epic B, transparent)**: the action signature does not
  change. It transparently benefits from the new structured columns because
  correlate's joins use the existing time-window predicate; downstream
  consumers can post-filter on the new columns from `LogEntry`-shaped rows.

These are all additive. No existing parameter is renamed or repurposed.
Note that `LogEntry` itself does NOT gain Epic B's `http_status`,
`auth_outcome`, `dns_blocked`, `event_action`, or `parse_error` in this
contract — adding them is a separate (breaking) version event.

## 6. Stability

| Property | Policy |
|---|---|
| Existing action name | `stable`. Renaming requires a major version bump. |
| Existing required param | `stable`. Removing or marking optional requires a major version bump. |
| Existing param cap / default | `stable`. Tightening (lower max, smaller default) requires a major version bump. Raising the max is non-breaking. |
| New optional param | Non-breaking. Allowed at any version. |
| Existing top-level response key | `stable`. Renaming or removing requires a major version bump. |
| New top-level response key | Non-breaking. Allowed at any version. |
| New enum value in a response field | Requires client tolerance. Add to the contract when introducing it; clients SHOULD treat unknown enum values as opaque strings. |
| Tool error vs structured error | `stable`. `compose_doctor` is the only action that returns a tool error on a recognised failure mode; others must keep returning structured responses. |

All 29 actions listed in §2 are `stable`. Removing any of them is a major
version event.
