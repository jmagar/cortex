# Architecture

cortex is a single Rust binary with three operational sub-products sharing a SQLite database and service layer.

## Three Sub-Products

1. **Log Intelligence Core**
   - Syslog UDP/TCP ingest (RFC 3164/5424 + CEF)
   - OTLP HTTP ingest (`/v1/logs`)
   - Docker log streaming (host-local agent + central pull)
   - AI transcript indexing and event extraction
   - FTS5 full-text search
   - 57-action MCP tool + 63 REST routes + CLI

2. **Fleet SSH Inventory / Investigation Graph**
   - SSH/API-based inventory collection
   - Heartbeat telemetry (`POST /v1/heartbeats`)
   - Rebuildable graph projection
   - Topology queries and evidence-backed explanations

3. **Deployment Tooling**
   - Docker Compose lifecycle management
   - Owner resolution and diagnostics
   - Config management

## Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│                      Ingestion Sources                        │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  rsyslog/syslog-ng ──▶ UDP :1514 ──────────────┐            │
│  network devices  ──▶ TCP :1514 ──▶ receiver ───┤            │
│                                               │             │
│  OTLP clients ──▶ POST /v1/logs ──▶ otlp ────┤──┐          │
│                                             │  │          │
│  Docker containers ──▶ socket ──▶ agent ───┤  │          │
│                          │                   │  │          │
│                          │                   │  │          │
│  AI transcripts ──▶ scanner ──▶ session watch │  │          │
│                                               │  │          │
└───────────────────────────────────────────────┼──┼──────────┘
                                                │  │
                                                ▼  ▼
                                        ┌───────────────┐
                                        │ Ingest Channel│
                                        │  (mpsc batch) │
                                        └───────────────┘
                                                │
                                                ▼
                                        ┌───────────────┐
                                        │  SQLite + FTS5│
                                        │  (WAL mode)   │
                                        └───────────────┘
                                                │
                    ┌───────────────────────────┼───────────────────────┐
                    │                           │                       │
                    ▼                           ▼                       ▼
            ┌───────────────┐           ┌───────────────┐       ┌───────────────┐
            │   MCP Server  │           │   REST API    │       │  Direct CLI   │
            │  RMCP HTTP    │           │   Axum        │       │  (read-only)  │
            │   :3100/mcp   │           │   :3100/api/* │       │  (stdio)      │
            └───────────────┘           └───────────────┘       └───────────────┘
                    │                           │                       │
                    └───────────────────────────┼───────────────────────┘
                                                │
                                                ▼
                                        ┌───────────────┐
                                        │CortexService  │
                                        │Shared Limits  │
                                        │Validation     │
                                        └───────────────┘
```

## Module Map

| Module | Sub-product | Purpose |
|--------|-------------|---------|
| `config.rs` | all | Layered config: defaults → `config.toml` → `~/.cortex/.env` → env vars; startup validation |
| `runtime.rs` | all | `RuntimeCore`: composition root, pool, ingest, auth, spawns maintenance tasks |
| `app/` | core | `CortexService`: shared service layer for MCP/REST/CLI; limits, validation, business logic |
| `db/` | core | SQLite pool, 31 migrations, FTS5 queries, retention, storage budgets |
| `receiver/` | core | UDP + TCP listeners, RFC 3164/5424 + CEF parsing, mpsc batch writer |
| `ingest.rs` | core | mpsc channel + batch writer (one pool connection reserved) |
| `otlp.rs` | core | OTLP/HTTP `POST /v1/logs` (protobuf, 4 MiB cap) |
| `agent/` | inventory | Host-local agent: heartbeat, syslog forwarding, Docker log streaming |
| `docker_ingest/` | core | Legacy central pull from remote Docker Engine HTTP endpoints |
| `mcp/` | core | RMCP Streamable HTTP server, `ACTION_SPECS` registry, scope gates |
| `api.rs` | core | Always-on `/api/*` REST surface (63 routes), bearer-token or OAuth/JWT |
| `scanner/` | core | AI transcript scanning, scrubbing, event extraction (skills/MCP/hooks) |
| `sessions_watch.rs` | core | Host-side systemd daemon for AI transcript watching |
| `inventory/` | inventory | Collectors (SSH, Docker, UniFi, Unraid, media APIs), redaction, cache |
| `heartbeat.rs` | inventory | `POST /v1/heartbeats` ingest |
| `heartbeat_agent.rs` | inventory | Host-local heartbeat telemetry agent |
| `notifications/` | core | Apprise dispatcher, rule evaluators, daily digest |
| `compose/` | deployment | Compose owner resolution, lifecycle commands |
| `setup/` | deployment | Idempotent setup/repair, drift diagnostics |
| `deploy.rs` | deployment | Agent deployment (push rsyslog via SSH) |
| `doctor.rs` | deployment | Health diagnostics and troubleshooting |
| `cli/` | all | Direct CLI with full MCP parity; routes via `/api/*` when `CORTEX_USE_HTTP=true` |

## Background Tasks

Spawned by `RuntimeCore::spawn_maintenance_tasks` (see `src/runtime.rs`):

| Task | Cadence | Knob |
|------|---------|------|
| Retention purge (global + AdGuard 7d + heartbeats 14d) | hourly (fixed) | `CORTEX_RETENTION_DAYS` (0 disables) |
| Storage budget enforcement (DB-size delete / free-disk write-block) | 60s | `CORTEX_CLEANUP_INTERVAL_SECS`, `CORTEX_MAX_DB_SIZE_MB`, `CORTEX_MIN_FREE_DISK_MB` |
| Error-signature scan | 3600s | `CORTEX_ERROR_DETECTION_ENABLED`, `CORTEX_ERROR_DETECTION_SCAN_INTERVAL_SECS` |
| Notification dispatcher | 30s | `[notifications] dispatcher_interval_secs` |
| Notification evaluator (oom_kill, ingest_silence, ...) | 300s | `[notifications.evaluators] evaluator_interval_secs` |
| Notification daily digest | cron `0 8 * * *` (local) | `[notifications] digest_cron_local` |
| Inventory refresh (+ graph projection) | 300s + file watchers | `CORTEX_INVENTORY_REFRESH_INTERVAL_SECS` (0 disables) |
| Inventory graph backfill | one-shot at startup (skipped once complete) | — |
| AI session rollup refresh | 300s (eager first run) | fixed (`SESSION_ROLLUP_REFRESH_SECS`) |
| Timeline hourly rollup (incremental) | 60s | fixed (`TIMELINE_ROLLUP_REFRESH_SECS`) |
| `PRAGMA optimize` (planner stats) | 6h | fixed (`OPTIMIZE_INTERVAL_SECS`) |
| Host-local agent Docker streams | continuous (deployed agents) | agent deployment config + local Docker socket |
| Legacy central pull Docker streams | continuous, reconnect with backoff | `CORTEX_DOCKER_INGEST_ENABLED`, `CORTEX_DOCKER_RECONNECT_INITIAL_MS` / `_MAX_MS` |
| Syslog UDP/TCP listeners | continuous, supervised restart + backoff | `CORTEX_RECEIVER_HOST` / `_PORT` |

### Maintenance Permits

All background DB-heavy work serializes on a single `maintenance_permit` semaphore so maintenance tasks never contend with each other for the write lock. A separate `dispatcher_permit` handles notification dispatch (makes outbound HTTP calls) to prevent backpressure from starving DB maintenance.

## Caller → Database Paths (Post v0.26)

After the v0.26 CLI-over-HTTP cutover:

```
AI clients ──▶ /mcp (RMCP streamable HTTP)        ─┐
                                                   │
CLI default ──▶ /api/* (REST)                      ├──▶ container CortexService ──▶ SQLite (/data)
   [CORTEX_USE_HTTP=true since v0.26]              │       (db_permits pool + MAINTENANCE_PERMIT)
                                                   │
CLI explicit "unset CORTEX_USE_HTTP"  ─────────────┘
   ──▶ direct SQLite (RuntimeCore::load_query_only, read-only)

cortex sessions watch (systemd) ────────────────────────────▶ direct SQLite
   (service.add_ai_file; long-running daemon on the host)

cortex mcp stdio (spawned by AI clients) ─────────────▶ direct SQLite
   (one-shot stdio session bound to the host's DB path)
```

### Ownership

The container is the **canonical query-path owner**: every `/api/*` caller — REST CLI, AI client over `/mcp`, anything else — funnels through one `CortexService` instance with shared `db_permits` and `MAINTENANCE_PERMIT` gates.

Direct-SQLite access remains for two consumers that cannot reasonably go through HTTP:

1. **Write-path**: `cortex sessions watch` (systemd daemon) – adds AI transcript rows to `ai_sessions` table
2. **Read-path**: `cortex mcp stdio` (spawned by AI clients) – stdio-only MCP query process

## Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| 1514 | UDP + TCP | Syslog receiver (not 514 — avoids `CAP_NET_BIND_SERVICE`) |
| 3100 | TCP | Shared HTTP listener for MCP (`POST /mcp`), OTLP (`POST /v1/logs`), and REST (`/api/*`); non-loopback `/v1/logs` exposure blocked at startup unless `CORTEX_TOKEN` is set |

## Service Layer Boundaries

`CortexService` in `src/app/` is the single owner of business logic for all three exposure surfaces:

### Invariants
- All MCP actions, REST routes, and CLI commands route through `CortexService`
- Query limits, timeouts, and validation are enforced once, at the service layer
- Response models are shared across MCP, REST, and CLI

### Key Methods
- `search_logs()`: Full-text search with filters
- `filter_logs()`: Structured filter-only retrieval
- `tail_logs()`: Recent log entries
- `errors_summary()`: Error/warning aggregation
- `correlate_events()`: Cross-host/time correlation
- `list_sessions()`: AI transcript inventory
- `abuse_search()`, `abuse_incidents()`, `abuse_investigate()`: Abuse detection
- `ai_correlate()`, `topic_correlate()`: AI-transcript-aware correlation
- `skill_events()`, `skill_incidents()`, `skill_investigate()`: Skill-usage incidents
- `mcp_events()`, `mcp_incidents()`, `mcp_investigate()`: MCP-usage incidents
- `hook_events()`, `hook_incidents()`, `hook_investigate()`: Hook-usage incidents
- `graph_resolve()`: Investigation graph entity resolution
- `stats()`, `status()`: Database and runtime observability

## Database Schema

31 sequential migrations (see `src/db/pool.rs`):

### Core Tables
- `logs`: Main log table with FTS5 full-text index
- `apps`, `hosts`, `source_ips`: Materialized distinct-value tables
- `timeline_hourly`, `timeline_daily`: Time-series rollups
- `error_signatures`: Repeating error pattern detection
- `llm_invocations`: LLM invocation audit (concurrency/rate-limit/circuit-breaker denials)

### AI Transcript Tables
- `ai_sessions`: AI transcript sessions (Claude, Codex, Gemini)
- `ai_skill_events`: Skill-invocation event tracking
- `ai_skill_incidents`: Skill-usage incident grouping
- `ai_mcp_events`: MCP tool-call event tracking
- `ai_mcp_incidents`: MCP-usage incident grouping
- `ai_hook_events`: Hook event tracking (runtime + config inventory)
- `ai_hook_incidents`: Hook-usage incident grouping

### Evidence Tables (Incident Investigation)
- `ai_skill_incident_evidence`: Deterministic evidence bundles for skill incidents
- `ai_mcp_incident_evidence`: Deterministic evidence bundles for MCP incidents
- `ai_hook_incident_evidence`: Deterministic evidence bundles for hook incidents

### Investigation Graph Tables (Rebuildable)
- `graph_hosts`, `graph_containers`, `graph_apps`, `graph_services`: Graph entities
- `graph_ai_projects`, `graph_ai_sessions`: AI entity projections
- `graph_edges`: Entity relationships
- `graph_*_evidence`: Evidence backing graph claims

### Heartbeat & Inventory
- `heartbeats`: Host telemetry (pressure flags, resource usage)
- `notifications`: Notification firing history

## Runtime Composition

`RuntimeCore` in `src/runtime.rs` is the composition root:

### Construction
1. Load `Config` (layered: defaults → `config.toml` → `~/.cortex/.env` → env vars)
2. Initialize `DbPool` with migrations
3. Resolve `AuthPolicy` (OAuth/JWT or static bearer)
4. Create `CortexService`
5. Spawn maintenance tasks with `spawn_maintenance_tasks()`
6. Start syslog listeners with `start_syslog()`

### Shutdown
1. Cancel `CancellationToken`
2. Drain ingest channel (flush pending logs)
3. Join maintenance tasks (with timeout)
4. Checkpoint SQLite WAL

## Error Handling

- ** anyhow**: Generic error propagation in CLI and runtime code
- **thiserror**: Typed error enums in service layer
- **tracing**: Structured logging with error context
- All listener failures are logged and trigger supervised restart

## Testing Strategy

- **Unit tests**: Sidecar `*_tests.rs` files (e.g., `src/db/queries_tests.rs`)
- **Integration tests**: `tests/` directory with `--features test-support`
- **Test support**: `src/lib.rs::testing` module provides factory helpers
- **Coverage**: ~80% target (see `scripts/coverage.sh`)

## References

- **[docs/architecture.md](../docs/architecture.md)** – Detailed architecture with module map
- **[docs/api.md](../docs/api.md)** – REST API endpoint matrix and versioning
- **[docs/contracts/data-layout.md](../docs/contracts/data-layout.md)** – Database schema contracts
- **[docs/contracts/runtime-lifecycle.md](../docs/contracts/runtime-lifecycle.md)** – Runtime lifecycle contract
