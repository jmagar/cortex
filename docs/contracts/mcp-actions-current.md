# MCP Actions Contract -- Current Production Surface

## Purpose

This contract summarizes the live MCP action surface for downstream consumers.
The implementation source of truth is the Rust action registry, not this
markdown file:

- `src/mcp/actions.rs::ACTION_SPECS` registers action names, scopes, costs, and descriptions.
- `src/mcp/actions.rs::action_names()` derives the schema enum.
- `src/mcp/schemas.rs::tool_definitions()` builds `tools/list` and `cortex://schema/mcp-tool`.
- `src/mcp/tools.rs::tool_syslog()` dispatches handlers.
- `src/app/models.rs` defines typed request and response payloads.

The MCP server exposes a single tool named `cortex`; every operation is selected
by the required string parameter `action`.

## Stability

Existing action names, required parameters, caps/defaults, and top-level
response keys are stable. Renaming, removing, or tightening them is a breaking
change. Adding optional parameters or optional response fields is non-breaking.

Most actions require `syslog:read` when auth is mounted. `ack_error`,
`unack_error`, and `notifications_test` require `syslog:admin`. `help` has no
action-level scope requirement, though the protected endpoint still requires
transport auth when configured.

## Current Action Index

The live registry currently contains 40 actions:

| Action | Scope | Cost | Purpose |
| --- | --- | --- | --- |
| `search` | `syslog:read` | cheap | Full-text search over syslog messages |
| `filter` | `syslog:read` | cheap | Filter logs by indexed fields without FTS5 |
| `tail` | `syslog:read` | cheap | Most recent log entries |
| `errors` | `syslog:read` | cheap | Error/warning summary |
| `hosts` | `syslog:read` | cheap | Known source hostnames |
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

## Schema Shape

The generated MCP schema is an action-dispatched flat JSON schema. The
`action` enum is derived from `ACTION_SPECS`; shared properties are declared at
the top level and action handlers perform per-action validation.

The runtime schema also includes syslog-specific metadata:

- `x-syslog-action-metadata`: action names, costs, and descriptions.
- `x-syslog-agent-guidance`: cost ordering and suggested first-pass actions.

## Response Envelope

Successful action responses are returned as one MCP text content block. The
text is pretty-printed JSON for the action-specific response struct.

Execution failures surface as MCP tool errors with `isError: true`.
Validation failures usually surface as invalid-params errors before the handler
runs.

## Correlation Notes

`correlate` is a general timestamp-window search over log rows. It is not the
AI-aware correlation path. Use:

- `ai_correlate` for AI transcript anchors plus nearby non-AI logs.
- `abuse_investigate` for deterministic abuse evidence bundles.
- `similar_incidents` for FTS5 historical incident clusters.
- `ask_history` for AI transcript history with nearby log context.
- `incident_context` for a caller-provided time-window bundle.

See `docs/mcp/CORRELATION.md` for inclusion rules, caps, defaults, and current
SQLite-only behavior.

## Drift Checks

The test suite enforces the main invariants:

- `src/mcp/schemas_tests.rs` checks that the schema action enum equals `action_names()`.
- `src/mcp/tools_tests.rs::schema_actions_are_dispatchable` dispatches every registered action.
- `src/mcp/tools_tests.rs::public_action_references_cover_schema_registry` checks public docs and scripts for each registered action.

## References

- `docs/mcp/SCHEMA.md` -- schema source-of-truth map and common arguments.
- `docs/mcp/TOOLS.md` -- user-facing action reference.
- `docs/mcp/CORRELATION.md` -- correlation behavior and inclusion rules.
- `docs/mcp/RESOURCES.md` -- runtime schema resource.
