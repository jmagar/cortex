# MCP Tools Reference -- syslog-mcp

## Design Philosophy

syslog-mcp exposes one read-only MCP tool named `syslog`. The required
`action` argument selects the operation:

| Action | Purpose |
| --- | --- |
| `search` | Full-text search with filters |
| `tail` | Recent log entries |
| `errors` | Error/warning summary by host and severity |
| `hosts` | Host registry with first/last seen |
| `sessions` | AI transcript sessions by project |
| `search_sessions` | Ranked grouped session search |
| `cuss` | Profanity hits in AI transcripts with same-session context |
| `ai_correlate` | AI transcript anchors cross-referenced against non-AI logs |
| `usage_blocks` | AI activity in deterministic 5-hour windows |
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
| `help` | Markdown reference for all actions |

## syslog search

Full-text search across all syslog messages. Uses SQLite FTS5 with porter stemming.

Required argument: `action = "search"`

Optional arguments: `query`, `hostname`, `source_ip`, `severity`, `app_name`, `facility`, `process_id`, `from`, `to`, `limit`.

## syslog tail

Get the N most recent log entries. Equivalent to `tail -f` across all hosts.

Required argument: `action = "tail"`

Optional arguments: `hostname`, `source_ip`, `app_name`, `severity_min`, `n`.

## syslog errors

Get a summary of errors and warnings across all hosts in a time window, grouped by hostname and severity.

Required argument: `action = "errors"`

Optional arguments: `from`, `to`, `group_by`.

`group_by` currently supports `app_name` for hostname + app + severity grouping.

## syslog hosts

List all hosts that have sent syslog messages.

Required argument: `action = "hosts"`

## syslog sessions

List AI transcript sessions grouped by project, tool, session, and host.

Required argument: `action = "sessions"`

Optional arguments: `project`, `tool`, `hostname`, `from`, `to`, `limit`.

## syslog search_sessions

Search AI transcript rows with FTS5 and return grouped session results ranked by relevance.

Required arguments: `action = "search_sessions"`, `query`

Optional arguments: `project`, `tool`, `from`, `to`, `limit`.

## syslog cuss

Detect profanity in AI transcript rows and return the hit plus surrounding rows
from the same AI session.

Required argument: `action = "cuss"`

Optional arguments: `project`, `tool`, `from`, `to`, `limit`, `before`, `after`, `terms`.

`terms` replaces the built-in profanity detector list when provided. `before`
and `after` default to 2 and are capped at 20.

## syslog ai_correlate

Use AI transcript rows as timeline anchors and pull nearby non-AI syslog,
Docker, OTLP, and host events from the same database. Related logs explicitly
exclude AI transcript rows so session logs do not correlate with themselves.

Required argument: `action = "ai_correlate"`

Optional arguments: `project`, `tool`, `session_id`, `ai_query`, `log_query`,
`hostname`, `source_ip`, `app_name`, `from`, `to`, `window_minutes`,
`severity_min`, `limit`, `events_per_anchor`.

`limit` caps AI anchors at 50. `events_per_anchor` caps related non-AI rows at
200 per anchor. `window_minutes` searches before and after each AI timestamp.

## syslog usage_blocks

Bucket AI activity into deterministic 5-hour UTC windows.

Required argument: `action = "usage_blocks"`

Optional arguments: `project`, `tool`, `from`, `to`.

## syslog project_context

Summarize one AI project path with tools, sessions, hosts, counts, and recent representative entries.

Required arguments: `action = "project_context"`, `project`

Optional arguments: `tool`, `limit`.

## syslog list_ai_tools

List distinct AI tools with counts and first/last seen timestamps.

Required argument: `action = "list_ai_tools"`

Optional arguments: `project`, `from`, `to`.

## syslog list_ai_projects

List distinct AI projects with counts, tools used, and first/last seen timestamps.

Required argument: `action = "list_ai_projects"`

Optional arguments: `tool`, `from`, `to`.

## syslog correlate

Search for related events across multiple hosts within a time window.

Required arguments: `action = "correlate"`, `reference_time`.

Optional arguments: `window_minutes`, `severity_min`, `hostname`, `source_ip`, `query`, `limit`.

## syslog stats

Get database statistics including storage health, runtime ingest counters, queue depth, writer failure/drop state, and OTLP receiver counters.

Required argument: `action = "stats"`

## syslog status

Get lightweight runtime status without the full DB statistics query.

Required argument: `action = "status"`

## syslog compose_status

Get redacted read-only Docker Compose diagnostics for the canonical syslog-mcp deployment. MCP output omits host paths, mount sources, image ids, and raw command output.

Required argument: `action = "compose_status"`

Target override arguments such as `project_dir`, `compose_file`, `project_name`, `container`, and `container_name` are rejected.

## syslog compose_doctor

Run strict deployment-health checks for the canonical syslog-mcp Compose deployment. It returns the same redacted diagnostic shape as `compose_status` when healthy, and returns a tool error when Docker/Compose ownership or runtime checks are not ready for lifecycle work. Compose lifecycle mutations are CLI-only.

Required argument: `action = "compose_doctor"`

## syslog help

Return markdown documentation for all actions.

Required argument: `action = "help"`

## Error Responses

Errors follow the MCP content format with `isError: true`:

```json
{
  "content": [
    {"type": "text", "text": "Tool execution failed"}
  ],
  "isError": true
}
```

JSON-RPC level errors use standard codes:

- `-32602`: Missing or invalid parameter, such as an unknown action or missing `reference_time`
- `-32601`: Unknown method
- `-32001`: Unauthorized, missing, or invalid bearer token

## Transcript Visibility Policy

AI transcript rows imported through `syslog ai index` or `syslog ai add` are stored in the main `logs` table. They are therefore visible through raw log actions such as `search`, `tail`, `context`, and `get`. Scanner imports scrub known credential/token patterns before storage and FTS indexing, but local `ai_transcript_path` values remain visible. Treat MCP log-read access as access to scrubbed transcript content plus local path metadata.

OTLP AI metadata (`ai.tool`/`ai_tool`, `session.id`/`session_id`, `project.path`, `codebase.root_path`, and `session.cwd`) is producer-supplied, not network-verified identity. Oversized AI tool, project, and session values are rejected before storage; accepted OTLP metadata should be used for grouping/search convenience, not as an authorization or provenance boundary.

Rows can include `metadata_json`, a source-specific JSON payload. Syslog rows
record parser/source provenance, OTLP rows record resource/log attributes plus
trace/span ids, Docker rows record host/container/image/compose/action details,
and transcript rows record source kind, path, line number, record key, and scrub
status. This metadata is for debugging and correlation, not authorization.

## See Also

- [../CLI.md](../CLI.md) -- direct CLI commands backed by the same service methods
- [SCHEMA.md](SCHEMA.md) -- JSON Schema definitions for tool inputs
- [AUTH.md](AUTH.md) -- authentication required before tool calls
- [ENV.md](ENV.md) -- environment variables affecting tool behavior
