# Tool Schema Documentation -- cortex

## Source Of Truth

The live MCP JSON schema is built in Rust, not generated from this markdown file.

Current source of truth:

- `src/mcp/actions.rs::ACTION_SPECS` registers every action, its scope, cost, and description.
- `src/mcp/actions.rs::action_names()` derives the schema action enum from `ACTION_SPECS`.
- `src/mcp/schemas.rs::tool_definitions()` builds the MCP `tools/list` definition and the `cortex://schema/mcp-tool` resource from that action table.
- `src/mcp/tools.rs::tool_syslog()` dispatches the action handlers.
- `src/app/models.rs` defines request and response structs for typed action payloads.

`docs/mcp/SCHEMA.md` is a human-maintained reference for that generated runtime
schema with drift tests; it is not itself automatically generated. If it
disagrees with `src/mcp/actions.rs` or `src/mcp/schemas.rs`, the Rust source
wins.

## Current Actions

cortex exposes one MCP tool named `syslog`. The required `action` argument
selects one of these 42 actions:

| Action | Scope | Cost | Purpose |
| --- | --- | --- | --- |
| `search` | `syslog:read` | cheap | Full-text search over syslog messages |
| `filter` | `syslog:read` | cheap | Filter logs by indexed fields without FTS5 |
| `tail` | `syslog:read` | cheap | Most recent log entries |
| `errors` | `syslog:read` | cheap | Error/warning summary |
| `hosts` | `syslog:read` | cheap | Known source hostnames |
| `host_state` | `syslog:read` | moderate | Latest bounded heartbeat state for one host |
| `fleet_state` | `syslog:read` | expensive | Fleet-wide heartbeat snapshot with pressure flags |
| `correlate` | `syslog:read` | moderate | Time-window event correlation |
| `stats` | `syslog:read` | expensive | DB statistics and runtime observability |
| `status` | `syslog:read` | cheap | Lightweight health and runtime status |
| `apps` | `syslog:read` | cheap | Distinct application names with counts |
| `sessions` | `syslog:read` | cheap | AI transcript session inventory |
| `search_sessions` | `syslog:read` | cheap | FTS5 search over AI transcript sessions |
| `abuse` | `syslog:read` | moderate | Abuse-term hits with same-session context |
| `abuse_incidents` | `syslog:read` | moderate | Grouped abuse incident candidates |
| `abuse_investigate` | `syslog:read` | expensive | Evidence bundles for abuse incidents |
| `ai_correlate` | `syslog:read` | moderate | AI transcript anchors with nearby non-AI logs |
| `usage_blocks` | `syslog:read` | cheap | AI activity in 5-hour UTC blocks |
| `project_context` | `syslog:read` | moderate | AI project summary and recent entries |
| `list_ai_tools` | `syslog:read` | cheap | AI tools observed in transcripts |
| `list_ai_projects` | `syslog:read` | cheap | AI projects observed in transcripts |
| `source_ips` | `syslog:read` | cheap | Distinct source identifiers with counts |
| `timeline` | `syslog:read` | cheap | Bucketed log counts over time |
| `patterns` | `syslog:read` | expensive | Near-duplicate message template clusters |
| `context` | `syslog:read` | cheap | Logs surrounding a pivot id or timestamp |
| `get` | `syslog:read` | cheap | One log entry by id, including raw frame |
| `ingest_rate` | `syslog:read` | expensive | Recent ingest throughput and write-block state |
| `silent_hosts` | `syslog:read` | moderate | Hosts older than a staleness threshold |
| `clock_skew` | `syslog:read` | expensive | Per-host received_at minus timestamp distribution |
| `anomalies` | `syslog:read` | expensive | Recent vs baseline volume/error comparison |
| `compare` | `syslog:read` | expensive | Side-by-side comparison of two time ranges |
| `compose_status` | `syslog:read` | moderate | Redacted self Compose status projection |
| `compose_doctor` | `syslog:read` | expensive | Strict self Compose health diagnostics |
| `unaddressed_errors` | `syslog:read` | moderate | Unacknowledged repeating error signatures |
| `notifications_recent` | `syslog:read` | cheap | Recent notification firings |
| `similar_incidents` | `syslog:read` | moderate | FTS5 historical incident clusters |
| `ask_history` | `syslog:read` | moderate | AI transcript history with nearby log context |
| `incident_context` | `syslog:read` | moderate | Window bundle: log aggregates, errors, AI sessions |
| `ack_error` | `syslog:admin` | write | Acknowledge an error signature |
| `unack_error` | `syslog:admin` | write | Revoke an error acknowledgement |
| `notifications_test` | `syslog:admin` | write | Send a test Apprise notification |
| `help` | none | cheap | Markdown action reference |

## Schema Pattern

The runtime tool definition is a flat action-dispatched JSON schema:

```json
{
  "name": "syslog",
  "description": "Query cortex logs with action-based subcommands...",
  "x-syslog-action-metadata": [
    { "name": "search", "cost": "cheap", "description": "..." }
  ],
  "x-syslog-agent-guidance": {
    "cost_order": ["cheap", "moderate", "expensive", "write"],
    "first_pass": ["status", "errors", "tail", "search", "timeline", "context"],
    "escalate_only_when_scoped": [
      "stats",
      "patterns",
      "anomalies",
      "compare",
      "clock_skew",
      "ingest_rate",
      "compose_doctor"
    ]
  },
  "inputSchema": {
    "type": "object",
    "properties": {
      "action": {
        "type": "string",
        "enum": ["...derived from ACTION_SPECS..."]
      }
    },
    "required": ["action"]
  }
}
```

All properties are declared at the top level because MCP clients receive one
tool schema for the `syslog` super-tool. Per-action validation happens in the
handler and service layers.

## Common Arguments

| Argument | Used by |
| --- | --- |
| `query` | `search`, `search_sessions`, `correlate`, `similar_incidents`, `ask_history` |
| `hostname` | `search`, `filter`, `tail`, `correlate`, `host_state`, `ai_correlate`, `apps`, `sessions`, `timeline`, `patterns`, `context`, `similar_incidents`, `ask_history`, `incident_context` |
| `host_id` | Authoritative heartbeat identity for `host_state` |
| `source_ip` | `search`, `filter`, `tail`, `correlate`, `ai_correlate` |
| `source_kind` | `filter` only; aliases Docker, command-history, shell-history, transcript, and AI-tool rows |
| `project` | `filter`, `sessions`, `search_sessions`, `abuse`, `ai_correlate`, `usage_blocks`, `project_context`, `list_ai_tools` |
| `tool` | `filter`, `sessions`, `search_sessions`, `abuse`, `ai_correlate`, `usage_blocks`, `project_context`, `list_ai_projects` |
| `session_id` | `filter`, `ai_correlate` |
| `ai_query` | AI transcript anchor FTS5 query for `ai_correlate` |
| `log_query` | Related non-AI log FTS5 query for `ai_correlate` |
| `severity` | Exact severity filter for `search` and `filter` |
| `severity_min` | Severity floor for `tail`, `correlate`, `ai_correlate`, `timeline`, `patterns`, `similar_incidents`, `incident_context` |
| `app_name` | `search`, `filter`, `tail`, `ai_correlate`, `timeline`, `patterns`, `similar_incidents`, `ask_history`, `incident_context` |
| `from`, `to` | Time range for search/session/AI/analytics actions; required for `incident_context` |
| `limit`, `offset` | Action-specific bounds; `offset` is for `apps` and `source_ips` pagination |

## Correlation Arguments

See [CORRELATION.md](CORRELATION.md) for the full behavior matrix.

| Action | Key arguments |
| --- | --- |
| `correlate` | `reference_time`, `window_minutes`, `severity_min`, `hostname`, `source_ip`, `query`, `limit` |
| `ai_correlate` | `project`, `tool`, `session_id`, `ai_query`, `log_query`, `hostname`, `source_ip`, `app_name`, `from`, `to`, `window_minutes`, `severity_min`, `limit`, `events_per_anchor` |
| `abuse_investigate` | `project`, `tool`, `from`, `to`, `limit`, `window_minutes`, `correlation_window_minutes`, `terms` |
| `similar_incidents` | `query`, `hostname`, `app_name`, `severity_min`, `from`, `to`, `window_minutes`, `limit` |
| `ask_history` | `query`, `hostname`, `app_name`, `from`, `to`, `limit` |
| `incident_context` | `from`, `to`, `hostname`, `app_name`, `severity_min`, `limit`; `query` is accepted by the request shape but intentionally ignored in v1 |

## Validation

Input validation is action-specific:

- `action` is required and must match `ACTION_SPECS`.
- Read actions require `syslog:read` when auth is mounted.
- Admin actions require `syslog:admin`.
- `help` has no scope gate, but auth policy still applies when the endpoint is protected.
- Numeric parameters are capped by each action.
- Timestamp parameters are parsed as RFC3339 and normalized where needed.
- FTS5 parameters use SQLite FTS5 syntax; quote hyphenated terms because bare `-` means NOT.
- Unknown parameters may be ignored by legacy extractor-style handlers, but typed payload handlers use `deny_unknown_fields` and reject unknown fields.

## Response Format

All MCP tool responses use one text content block containing pretty-printed JSON:

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"count\": 3, \"logs\": [...]}"
    }
  ]
}
```

Validation failures usually surface as MCP invalid-params errors. Execution
failures return tool errors with `isError: true`.

## Drift Checks

The test suite enforces several schema/documentation invariants:

- `src/mcp/schemas_tests.rs` checks that the schema action enum equals `actions::action_names()`.
- `src/mcp/tools_tests.rs::schema_actions_are_dispatchable` dispatches every registered action.
- `src/mcp/tools_tests.rs::public_action_references_cover_schema_registry` checks public references for every registered action.

There is no checked-in generator that rewrites this markdown file today.

## See Also

- [TOOLS.md](TOOLS.md) -- action reference
- [CORRELATION.md](CORRELATION.md) -- correlation behavior and inclusion rules
- [PATTERNS.md](PATTERNS.md) -- code patterns for tool dispatch
- [RESOURCES.md](RESOURCES.md) -- runtime schema resource
