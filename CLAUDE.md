<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-05-11 | Updated: 2026-05-11 -->

# syslog-mcp

## Purpose

**Syslog Intelligence for Homelabs** — Receives RFC 3164/5424 syslog from all homelab hosts (UDP/TCP), ingests Docker logs via socket proxy, stores everything in SQLite with FTS5, and exposes a comprehensive `syslog` MCP tool for AI agents.

**Status**: Active development, Production-ready
**Version**: 0.20.0

## Key Files

| File | Description |
|------|-------------|
| `src/main.rs` | Entry point — CLI initialization and server start |
| `Cargo.toml` | Rust crate definition and dependencies |
| `README.md` | Project overview, install, usage |
| `CLAUDE.md` | Dev environment rules and standard commands |
| `config.toml` | Local development configuration |
| `Justfile` | Command runner for dev, build, and test |
| `src/cli.rs` | Standalone CLI binary (`syslog` command) |
| `src/compose.rs` | Docker Compose lifecycle management |
| `src/scanner.rs` | AI transcript indexer (Claude/Codex sessions) |

## Project Structure

```
syslog-mcp/
├── src/
│   ├── main.rs                  # CLI Entrypoint (serve mcp / mcp stdio)
│   ├── lib.rs                   # Library root, module declarations
│   ├── cli.rs                   # Standalone CLI binary (~1300 lines)
│   ├── compose.rs               # Docker Compose lifecycle CLI (~1500 lines)
│   ├── scanner.rs               # AI transcript indexer (Claude/Codex/Gemini)
│   ├── runtime.rs               # RuntimeCore — wiring and lifecycle
│   ├── config.rs                # Configuration (Toml + Env)
│   ├── api.rs                   # HTTP API surface
│   ├── otlp.rs                  # OpenTelemetry/OTLP ingestion
│   ├── ingest.rs                # Log ingestion coordinator
│   ├── observability.rs         # Tracing and metrics
│   ├── app/                     # Service Layer (Business Logic)
│   │   ├── service.rs           # SyslogService implementation
│   │   ├── models.rs            # Request/Response types
│   │   ├── correlate.rs         # Event correlation logic
│   │   ├── error.rs             # Error types
│   │   └── time.rs              # Time utilities
│   ├── db/                      # Database Layer (SQLite + FTS5)
│   │   ├── pool.rs              # Pool management and schema
│   │   ├── queries.rs           # SQL queries and search logic
│   │   ├── analytics.rs         # Stats and timeline logic
│   │   ├── ingest.rs            # Log insertion logic
│   │   ├── maintenance.rs       # Retention and storage guardrails
│   │   └── models.rs            # DB model types
│   ├── mcp/                     # MCP Server Layer
│   │   ├── tools.rs             # syslog tool action dispatch
│   │   ├── routes.rs            # HTTP route handlers
│   │   ├── schemas.rs           # JSON Schema definitions
│   │   └── rmcp_server.rs       # RMCP transport implementation
│   ├── syslog/                  # Ingestion Layer
│   │   ├── parser.rs            # RFC 3164/5424 parsing
│   │   ├── listener.rs          # UDP/TCP listeners
│   │   ├── writer.rs            # Batch writer
│   │   └── enrichment.rs        # Log enrichment
│   ├── scanner/                 # AI transcript scanner
│   │   ├── claude.rs            # Claude transcript parsing
│   │   └── codex.rs             # Codex transcript parsing
│   └── docker_ingest/           # Docker Remote Ingestion
├── bin/                         # Installed binaries (syslog)
├── config/                      # Deployment config templates
├── deploy/                      # Host-side manifests (rsyslog, otel)
├── docs/                        # Deep-dive documentation
├── plugins/                     # Claude Code skills and hooks
├── scripts/                     # Maintenance and CI scripts
└── tests/                       # Integration and smoke tests
```

## For AI Agents

### Working In This Directory

1. **Adding MCP Actions**: Modify `src/mcp/tools.rs` to add the action to the dispatch table.
2. **Database Changes**: Queries go in `src/db/queries.rs`. Schema and pool management in `src/db/pool.rs`.
3. **Adding Ingest Types**: Add parsers in `src/syslog/parser.rs`.
4. **Testing**: Run `just test` before committing.
5. **Building**: Run `just build` to verify compilation.

### Architecture

```
Inbound Logs (UDP/TCP/Docker)
  ↓
Ingestion Layer (src/syslog/)
  ↓  Parse RFC 3164/5424 → mpsc channel
Runtime Core (src/runtime.rs)
  ↓  Batching → Transaction
Database Layer (src/db/)
  ↓
SQLite Database (/data/syslog.db)
  ↑
Service Layer (src/app/)
  ↑
MCP Layer (src/mcp/)
  ↑
AI Agents (Claude/Codex/Gemini)
```

### Key Design Patterns

- **Action Dispatch**: Single `syslog` MCP tool dispatches to handlers via an `action` argument.
- **Sidecar Tests**: `#[cfg(test)] #[path = "..._tests.rs"] mod tests;` pattern for unit tests.
- **SQLx + SQLite**: Async SQLx for database operations with WAL mode enabled.
- **FTS5 Search**: Full-text search with BM25-like ranking for log discovery.
- **RuntimeCore Lifecycle**: Centralized management of background tasks (retention, storage guardrails).
- **Transaction Pattern**: All batch inserts use explicit SQLx transactions for atomicity.
- **Storage Guardrails**: Automated cleanup of oldest logs when DB size or disk space limits are breached.

### Transaction Pattern (Rust/SQLx)

All batch ingestions follow this atomic pattern:

```rust
let mut tx = pool.begin().await?;
for log in logs {
    insert_log(&mut *tx, log).await?;
}
tx.commit().await?;
```

### Testing

```bash
just test               # Run all unit and integration tests
bash scripts/smoke-test.sh  # Run live smoke test against a running server
```

## CLI Commands

| Command | Purpose | Example |
|---------|---------|---------|
| `syslog serve mcp` | Start full server with ingest | `syslog serve mcp` |
| `syslog mcp` | Start stdio query-only mode | `syslog mcp` |
| `just health` | Check server health | `just health` |
| `just dev` | Run in dev mode | `just dev` |
| `just lint` | Run clippy (strict) | `just lint` |
| `just fmt` | Format code | `just fmt` |
| `just test-live` | Run live integration tests | `just test-live` |
| `just up` / `just down` | Start/stop Docker Compose | `just up` |

## Dependencies

| Dependency | Purpose |
|------------|---------|
| `sqlx` | Async SQLite driver |
| `axum` | HTTP server for MCP |
| `tokio` | Async runtime |
| `serde` | Serialization/Deserialization |
| `tracing` | Observability and logging |
