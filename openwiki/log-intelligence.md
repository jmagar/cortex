# Log Intelligence

The core log intelligence system ingests logs from multiple sources, stores them in SQLite with FTS5 full-text search, and provides powerful search and correlation capabilities.

## Ingestion Sources

### 1. Syslog (UDP/TCP)
- **Port**: 1514 (both UDP and TCP)
- **Protocols**: RFC 3164, RFC 5424, CEF
- **Parser**: `syslog_loose` crate with custom CEF extensions
- **Listeners**: Supervised with restart + backoff on failure
- **Source**: `src/receiver/` (UDP + TCP listeners, parsing, enrichment)

**Key files**:
- `src/receiver/mod.rs`: Receiver coordinator
- `src/receiver/udp.rs`: UDP listener with supervision
- `src/receiver/tcp.rs`: TCP listener with supervision
- `src/receiver/enrichment/`: Hostname, app, and severity enrichment

### 2. OTLP HTTP
- **Endpoint**: `POST /v1/logs`
- **Protocol**: OTLP/HTTP with protobuf payloads
- **Cap**: 4 MiB per request
- **Use case**: Application telemetry from OTLP exporters
- **Source**: `src/otlp.rs`

**Key files**:
- `src/otlp.rs`: OTLP HTTP handler, protobuf decoding, log normalization

### 3. Docker Container Logs
Two ingestion modes:

#### Host-local Agent (Recommended)
- Runs on each Docker host as a systemd service
- Streams container stdout/stderr from local Docker socket
- Uses Docker Engine API over Unix socket
- Attaches structured identity metadata per line: the agent prefixes each
  forwarded message with `[cortex-agent-docker-meta:{json}]`, which receiver
  enrichment extracts into `metadata_json.agent_docker` (required: `host`,
  `container_id`, `container_name`, `stream`; optional: `compose_project`,
  `compose_service`, `image`) and strips from `message`. This is the
  supported Docker identity source for the resolver-backed graph
  (`logical_service` / `service_instance`) — the 48-char syslog APP-NAME
  fallback never loses canonical identity.
- **Source**: `src/agent/`, `src/docker_ingest/`

**Key files**:
- `src/agent/docker.rs`: Docker log stream client + identity metadata
- `src/docker_ingest/file_tail.rs`: File-tail registry for non-Docker sources

#### Legacy Central Pull
- Central cortex connects to remote Docker Engine HTTP endpoints
- Reconnects with exponential backoff
- Not resolver proof: `docker://` / `docker-event://` rows are a
  compatibility path only; the canonical graph contract requires
  `metadata_json.agent_docker` from host-local agents
- **Source**: `src/docker_ingest/`

**Key files**:
- `src/docker_ingest/central_pull.rs`: Central pull client with reconnect

### 4. AI Transcripts
- **Watch daemon**: `cortex sessions watch` (systemd service on host)
- **Parser**: Extracts skill/MCP/hook events from Claude, Codex, Gemini transcripts
- **Scrubbing**: Removes credentials and sensitive data
- **Source**: `src/scanner/`, `src/sessions_watch.rs`

**Key files**:
- `src/scanner/claude.rs`: Claude transcript parser
- `src/scanner/codex.rs`: Codex transcript parser
- `src/scanner/gemini.rs`: Gemini transcript parser
- `src/scanner/skill_events.rs`: Skill-invocation event extraction
- `src/scanner/mcp_events.rs`: MCP tool-call event extraction
- `src/scanner/hook_events.rs`: Hook event collection

## Storage

### Database
- **Engine**: SQLite with bundled engine
- **Mode**: WAL (Write-Ahead Logging) for concurrent reads/writes
- **FTS5**: Full-text search index on `logs.message` column
- **Migrations**: 31 sequential migrations (see `src/db/pool.rs`)
- **Connection Pool**: `r2d2` with scheduled-thread-pool for maintenance

**Key files**:
- `src/db/pool.rs`: Pool initialization, migrations, connection management
- `src/db/models.rs`: Core database models (log rows, sessions, etc.)
- `src/db/queries.rs`: FTS5 search queries, correlation, aggregation

### Schema
**Core tables**:
- `logs`: Main log table with FTS5 full-text index
- `timeline_hourly`, `timeline_daily`: Pre-aggregated time-series buckets
- `apps`, `hosts`, `source_ips`: Materialized distinct-value tables
- `error_signatures`: Repeating error patterns with acknowledgment state

**AI transcript tables**:
- `ai_sessions`: AI transcript session metadata
- `ai_skill_events`: Skill-invocation event tracking
- `ai_mcp_events`: MCP tool-call event tracking
- `ai_hook_events`: Hook event tracking

## Ingest Pipeline

```
┌──────────────┐
│ Source       │
│ (syslog/OTLP │
│  /Docker/AI) │
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ Parse        │
│ (normalize)  │
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ Enrich       │
│ (hostname,   │
│  app, etc.)  │
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ Batch        │
│ (mpsc)       │
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ Write        │
│ (SQLite)     │
└──────────────┘
```

**Key files**:
- `src/ingest.rs`: Ingest channel coordinator, batch writer
- `src/db/ingest.rs`: Insert batch helpers
- `src/normalize.rs`: Log message normalization (cleanup, deduplication)

## Search & Query

### Full-Text Search (FTS5)
- **Syntax**: SQLite FTS5 query syntax (AND, OR, NOT, phrase search, column filters)
- **Filters**: Host, source IP, severity, app, time range
- **Ranking**: BM25 relevance scoring
- **Example**: `error "connection failed" hostname:web-server severity:error`

**Key methods**:
- `CortexService::search_logs()`: FTS5 search with filters
- `db::queries::search_logs()`: Raw FTS5 query execution

### Structured Filtering
- Filter by indexed fields without full-text query
- Supports: host, source_ip, app, severity, time range
- Faster than FTS5 for pure filter queries

**Key methods**:
- `CortexService::filter_logs()`: Filter-only retrieval
- `db::queries::filter_logs()`: Raw filter query execution

### Aggregation
- **Timeline**: Bucketed counts over time (hourly/daily)
- **Stats**: Total logs, hosts, apps, DB size, time range
- **Errors**: Grouped by hostname and severity
- **Patterns**: Near-duplicate message template clusters

**Key methods**:
- `CortexService::timeline()`: Time-series aggregation
- `CortexService::stats()`: Database statistics
- `db::analytics.rs`: Complex analytics queries

## Maintenance Tasks

### Retention Purge
- **Cadence**: Hourly
- **Scope**: Global age purge + AdGuard 7-day tags + heartbeats 14-day
- **Config**: `CORTEX_RETENTION_DAYS` (0 disables global purge)

**Key files**:
- `src/db/maintenance.rs::purge_retention()`: Retention cleanup

### Storage Budget Enforcement
- **Cadence**: 60s
- **Checks**:
  - DB size > `CORTEX_MAX_DB_SIZE_MB`: Delete oldest logs
  - Free disk < `CORTEX_MIN_FREE_DISK_MB`: Write-block new logs
- **Config**: `CORTEX_CLEANUP_INTERVAL_SECS`, `CORTEX_MAX_DB_SIZE_MB`, `CORTEX_MIN_FREE_DISK_MB`

**Key files**:
- `src/db/maintenance.rs::enforce_storage_budget()`: Storage checks

### Error-Signature Scan
- **Cadence**: 3600s
- **Operation**: Scan for repeating error patterns, group by template
- **Config**: `CORTEX_ERROR_DETECTION_ENABLED`, `CORTEX_ERROR_DETECTION_SCAN_INTERVAL_SECS`

**Key files**:
- `src/db/error_signatures.rs::scan_error_signatures()`: Pattern detection

### Timeline Rollup
- **Cadence**: 60s (incremental)
- **Operation**: Aggregate logs into hourly/daily buckets
- **Purpose**: Fast timeline queries without scanning raw logs

**Key files**:
- `src/db/analytics.rs::refresh_timeline_rollup()`: Incremental rollup

### Session Rollup
- **Cadence**: 300s (eager first run)
- **Operation**: Refresh AI session aggregates
- **Purpose**: Fast session queries

**Key files**:
- `src/db/analytics.rs::refresh_session_rollup()`: Session materialization

### PRAGMA Optimize
- **Cadence**: 6h
- **Operation**: Run `PRAGMA optimize` to update planner statistics
- **Purpose**: Improve query performance

**Key files**:
- `src/db/maintenance.rs::run_pragma_optimize()`: Planner optimization

## Correlation

### Cross-Host Correlation
- **Method**: `CortexService::correlate_events()`
- **Operation**: Find related events across hosts in a time window
- **Use case**: Track failures across services

### AI-Transcript-Aware Correlation
- **Methods**:
  - `ai_correlate()`: AI transcript anchors cross-referenced with non-AI logs
  - `topic_correlate()`: Resolve a topic to graph entities and correlate all related logs
- **Use case**: Understand what happened before/after an AI action

**Key files**:
- `src/app/services/correlate.rs`: Correlation logic
- `src/db/graph.rs`: Graph-backed correlation queries

## Performance

### Batch Writes
- Logs are buffered in memory and written in batches
- One pool connection reserved for the writer
- Reduces SQLite write contention

### Read Pool
- Remaining connections in the pool serve read queries
- Maintenance tasks serialize on `maintenance_permit` semaphore

### Query Limits
- All queries have default row limits (configurable via `CORTEX_MAX_RESULTS`)
- Timeout enforcement prevents runaway queries

## Troubleshooting

### Ingest Issues
- **Check listener health**: `cortex status` or `GET /health/full` for `syslog_udp_listener_state` / `syslog_tcp_listener_state`
- **Check rate**: `cortex ingest-rate` for recent throughput
- **Check write-block**: `cortex stats` for `write_blocked` flag

### Search Issues
- **FTS5 syntax**: Verify query syntax (avoid invalid operators)
- **Timezone**: Timestamps are stored in UTC, convert queries to UTC
- **Filters**: Ensure indexed fields (host, app, severity) are correctly spelled

### Performance Issues
- **PRAGMA optimize**: Run manually if queries are slow
- **Timeline rollup**: Check `timeline_hourly` table is fresh
- **Storage budget**: Ensure DB isn't constantly purging (set appropriate limits)

## References

- **[docs/CLI.md](../docs/CLI.md)** – Complete CLI reference
- **[docs/mcp/SCHEMA.md](../docs/mcp/SCHEMA.md)** – MCP action reference
- **[docs/contracts/log-row-shape.md](../docs/contracts/log-row-shape.md)** – Log row schema
- **[docs/contracts/retention-policy.md](../docs/contracts/retention-policy.md)** – Retention rules
