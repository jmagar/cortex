# Component Inventory -- cortex

Complete listing of all plugin components.

MCP, REST, and CLI are transport surfaces over the shared service layer. The
runtime MCP action schema is derived from `src/mcp/actions.rs::ACTION_SPECS`
through `src/mcp/schemas.rs::tool_definitions()` and exposed as
`cortex://schema/mcp-tool`; maintained Markdown docs are drift-checked rather
than automatically generated.

## MCP tools

cortex exposes one MCP tool named `cortex`. The required `action`
argument selects the operation. The authoritative action registry lives in
`src/mcp/actions.rs::ACTION_SPECS`; the runtime schema enum is derived from
that registry by `src/mcp/schemas.rs::tool_definitions()`.

| Action | Description | Destructive |
| --- | --- | --- |
| `search` | Full-text search across syslog messages with FTS5 syntax, host/source_ip/severity/app/time filters | no |
| `filter` | Structured filter-only log retrieval for indexed fields and source aliases | no |
| `tail` | Get N most recent log entries, optionally filtered by host, source_ip, and/or application | no |
| `errors` | Error/warning summary grouped by hostname and severity level with counts | no |
| `hosts` | List all hosts with first/last seen timestamps and total log counts | no |
| `host_state` | Latest bounded heartbeat state for one host | no |
| `fleet_state` | Fleet-wide heartbeat snapshot with pressure flags and summary counts | no |
| `correlate` | Cross-host event correlation within a time window around a reference timestamp | no |
| `correlate_state` | Correlate logs with heartbeat window summaries around a reference time | no |
| `stats` | Database statistics: total logs, hosts, time range, DB size, free disk, write-block status | no |
| `status` | Lightweight runtime status: DB health, queue/backpressure state, listener/writer counters, OTLP counters | no |
| `sessions` | AI transcript sessions grouped by project/tool/session/host | no |
| `search_sessions` | Ranked grouped session search | no |
| `abuse` | Abuse-term detector with same-session context | no |
| `abuse_incidents` | Groups abuse hits into scored incident candidates | no |
| `abuse_investigate` | Expands incidents into deterministic evidence bundles | no |
| `ai_correlate` | AI transcript anchors cross-referenced against nearby non-AI logs | no |
| `usage_blocks` | AI transcript activity grouped into deterministic 5-hour UTC blocks | no |
| `project_context` | Summary and recent entries for one AI project path | no |
| `list_ai_tools` | Distinct AI tools with counts | no |
| `list_ai_projects` | Distinct AI projects with counts | no |
| `apps` | Distinct application names with log and host counts | no |
| `source_ips` | Distinct source identifiers with hostname breakdown | no |
| `timeline` | Bucketed log counts over time | no |
| `patterns` | Near-duplicate message template clusters | no |
| `context` | Surrounding logs around a log id or timestamp | no |
| `get` | One log entry by id, including raw frame | no |
| `ingest_rate` | Recent ingest throughput and write-block state | no |
| `silent_hosts` | Hosts whose last_seen is older than a threshold | no |
| `clock_skew` | Per-host received_at minus timestamp distribution | no |
| `anomalies` | Recent vs baseline volume/error comparison | no |
| `compare` | Side-by-side comparison of two time ranges | no |
| `compose_status` | Redacted Docker Compose runtime projection | no |
| `compose_doctor` | Strict redacted Docker Compose health diagnostics | no |
| `unaddressed_errors` | Repeating unacknowledged error signatures | no |
| `notifications_recent` | Recent notification firings | no |
| `similar_incidents` | FTS5 historical incident clusters with overlapping AI sessions | no |
| `ask_history` | AI transcript history search with nearby non-AI log context | no |
| `incident_context` | Window bundle of non-AI log aggregates/errors and active AI sessions | no |
| `graph` | Resolve graph entities and return bounded one-hop neighborhoods with evidence | no |
| `ack_error` | Acknowledge an error signature | yes |
| `unack_error` | Revoke an error acknowledgement | yes |
| `notifications_test` | Send a test Apprise notification | yes |
| `help` | Returns markdown documentation for all actions | no |

Most MCP actions are read-only. `ack_error`, `unack_error`, and
`notifications_test` require `syslog:admin`; they mutate acknowledgement/audit
or notification state through service-owned actor and safety policy.

## Direct CLI commands

The `cortex` binary also exposes direct local commands backed by the same service
methods as the MCP actions.

| Command | Matches MCP action | Description |
| --- | --- | --- |
| `cortex search` | `search` | Full-text search with filters |
| `cortex tail` | `tail` | Recent log entries |
| `cortex errors` | `errors` | Error/warning summary |
| `cortex hosts` | `hosts` | Known host list |
| `cortex filter` | `filter` | Structured filter-only log retrieval |
| `cortex correlate` | `correlate` | Cross-host event correlation |
| `cortex host-state` | `host_state` | Latest bounded heartbeat state for one host |
| `cortex fleet-state` | `fleet_state` | Fleet-wide heartbeat snapshot with pressure flags |
| `cortex correlate-state` | `correlate_state` | Logs plus heartbeat summaries around a reference time |
| `cortex ai correlate` | `ai_correlate` | AI transcript anchors cross-referenced against nearby non-AI logs |
| `cortex ai incidents` | `abuse_incidents` | Grouped abuse incident candidates |
| `cortex ai investigate` | `abuse_investigate` | Abuse incident evidence bundles |
| `cortex ai similar` | `similar_incidents` | Historical incident clusters |
| `cortex ai ask-history` | `ask_history` | AI transcript history search |
| `cortex ai incident-context` | `incident_context` | Full context bundle for a time window |
| `cortex stats` | `stats` | Database and storage metrics |

## MCP resources

| URI | Description | MIME type |
| --- | --- | --- |
| `cortex://schema/mcp-tool` | JSON schema for the `cortex` MCP tool and action-based parameters | `application/json` |

## Environment variables

| Variable | Required | Default | Sensitive |
| --- | --- | --- | --- |
| `CORTEX_RECEIVER_HOST` | no | `0.0.0.0` | no |
| `CORTEX_RECEIVER_PORT` | no | `1514` | no |
| `CORTEX_MAX_MESSAGE_SIZE` | no | `8192` | no |
| `CORTEX_BATCH_SIZE` | no | `100` | no |
| `CORTEX_FLUSH_INTERVAL` | no | `500` | no |
| `CORTEX_HOST` | no | `0.0.0.0` | no |
| `CORTEX_PORT` | no | `3100` | no |
| `CORTEX_TOKEN` | no | (none) | yes |
| `CORTEX_ALLOWED_HOSTS` | no | (none) | no |
| `CORTEX_ALLOWED_ORIGINS` | no | (none) | no |
| `CORTEX_API_ENABLED` | no | `false` | no |
| `CORTEX_API_TOKEN` | yes when enabled | (none) | yes |
| `CORTEX_DB_PATH` | no | `/data/cortex.db` | no |
| `CORTEX_POOL_SIZE` | no | `4` | no |
| `CORTEX_RETENTION_DAYS` | no | `90` | no |
| `CORTEX_MAX_DB_SIZE_MB` | no | `1024` | no |
| `CORTEX_RECOVERY_DB_SIZE_MB` | no | `900` | no |
| `CORTEX_MIN_FREE_DISK_MB` | no | `512` | no |
| `CORTEX_RECOVERY_FREE_DISK_MB` | no | `768` | no |
| `CORTEX_CLEANUP_INTERVAL_SECS` | no | `60` | no |
| `CORTEX_CLEANUP_CHUNK_SIZE` | no | `2000` | no |
| `RUST_LOG` | no | `info` | no |

## Plugin surfaces

| Surface | Present | Path |
| --- | --- | --- |
| Skills | yes | `plugins/syslog/skills/` |
| Agents | no | -- |
| Commands | no | -- |
| Hooks | yes | `plugins/syslog/hooks/` |
| Channels | no | -- |
| Output styles | no | -- |
| Schedules | no | -- |

## Network ports

| Port | Protocol | Purpose |
| --- | --- | --- |
| 1514 | UDP + TCP | Syslog receiver (RFC 3164/5424) |
| 3100 | TCP | RMCP Streamable HTTP endpoint |

## HTTP endpoints

| Endpoint | Method | Auth required | Description |
| --- | --- | --- | --- |
| `/mcp` | POST | yes (when token set) | RMCP stateless Streamable HTTP endpoint |
| `/mcp` | GET, DELETE | yes (when token set) | 401 first if token auth is enabled and the bearer token is missing/invalid; otherwise 405 in stateless mode |
| `/health` | GET | no | Health check -- verifies DB connectivity |
| `/api/search` | GET | yes when API enabled | Plain JSON log search |
| `/api/tail` | GET | yes when API enabled | Plain JSON recent logs |
| `/api/errors` | GET | yes when API enabled | Plain JSON error summary |
| `/api/hosts` | GET | yes when API enabled | Plain JSON host list |
| `/api/correlate` | GET | yes when API enabled | Plain JSON event correlation |
| `/api/stats` | GET | yes when API enabled | Plain JSON database stats |
| `/api/filter` | GET | yes when API enabled | Plain JSON structured log filtering |
| `/api/timeline` | GET | yes when API enabled | Plain JSON bucketed timeline |
| `/api/patterns` | GET | yes when API enabled | Plain JSON message pattern clusters |
| `/api/notifications/recent` | GET | yes when API enabled | Recent notification firings |

## Docker

| Component | Value |
| --- | --- |
| Image | `ghcr.io/jmagar/cortex:latest` |
| Syslog port | `1514/udp`, `1514/tcp` |
| MCP port | `3100/tcp` |
| Health endpoint | `GET /health` (unauthenticated) |
| Compose file | `docker-compose.yml` |
| Entrypoint | `cortex` binary |
| User | `1000:1000` |
| Data volume | `/data` (SQLite database) |

## CI/CD workflows

| Workflow | Trigger | Purpose |
| --- | --- | --- |
| `ci.yml` | push, PR | Lint (clippy), check, test |
| `docker-publish.yml` | tag push | Build and publish Docker image to GHCR |
| `publish-crates.yml` | tag push | Publish to crates.io |
| `codex-plugin-scanner.yml` | PR | Validate Codex plugin manifest |

## Scripts

| Script | Purpose |
| --- | --- |
| `scripts/smoke-test.sh` | Live smoke test -- current MCP action set via mcporter |
| `scripts/backup.sh` | WAL-safe SQLite backup (checkpoint + `.backup` method) |
| `scripts/reset-db.sh` | WAL-safe backup + destructive DB reset for dev recovery |





## Dependencies

### Runtime

| Crate | Purpose |
| --- | --- |
| `tokio` | Async runtime (full features) |
| `axum` | HTTP framework for MCP server |
| `tower-http` | CORS and tracing middleware |
| `rusqlite` | SQLite driver (bundled, with FTS5) |
| `r2d2` / `r2d2_sqlite` | Connection pooling |
| `syslog_loose` | RFC 3164/5424 syslog parsing |
| `serde` / `serde_json` | Serialization |
| `chrono` | Timestamps |
| `toml` | Config file parsing |
| `tracing` / `tracing-subscriber` | Structured logging |
| `anyhow` | Error handling |
| `subtle` | Constant-time token comparison |
| `rustix` | Filesystem stats (free disk space) |

### Development

| Crate | Purpose |
| --- | --- |
| `tempfile` | Temporary directories for test databases |
| `serial_test` | Serialized test execution for env var tests |
| `tower` | HTTP testing utilities |
