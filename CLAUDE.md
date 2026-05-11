<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-05-11 | Updated: 2026-05-11 -->

# syslog-mcp

## Purpose

**Syslog Intelligence for Homelabs** — Receives RFC 3164/5424 syslog from all homelab hosts (UDP/TCP), ingests Docker logs via socket proxy, stores everything in SQLite with FTS5, and exposes a comprehensive `syslog` MCP tool for AI agents.

**Status**: Active development, Production-ready
**Version**: 0.7.13

## Key Files

| File | Description |
|------|-------------|
| `src/main.rs` | Entry point — CLI initialization and server start |
| `Cargo.toml` | Rust crate definition and dependencies |
| `README.md` | Project overview, install, usage |
| `CLAUDE.md` | Dev environment rules and standard commands |
| `config.toml` | Local development configuration |
| `Justfile` | Command runner for dev, build, and test |

## Project Structure

```
syslog-mcp/
├── src/
│   ├── main.rs                  # CLI Entrypoint (serve mcp / mcp stdio)
│   ├── lib.rs                   # Library root
│   ├── config.rs                # Configuration (Toml + Env)
│   ├── runtime.rs               # RuntimeCore — wiring and lifecycle
│   ├── db/                      # Database Layer (SQLite + FTS5)
│   │   ├── mod.rs               # Pool management
│   │   ├── queries.rs           # SQL queries and search logic
│   │   ├── analytics.rs         # Stats and timeline logic
│   │   └── ingest.rs            # Log insertion logic
│   ├── app/                     # Service Layer (Business Logic)
│   │   ├── service.rs           # SyslogService implementation
│   │   ├── correlate.rs         # Event correlation logic
│   │   └── models.rs            # Request/Response types
│   ├── syslog/                  # Ingestion Layer
│   │   ├── mod.rs               # Listener orchestration
│   │   ├── parser.rs            # RFC 3164/5424 parsing
│   │   └── server.rs            # UDP/TCP listeners
│   ├── mcp/                     # MCP Server Layer
│   │   ├── mod.rs               # HTTP/Stdio transport
│   │   ├── tools.rs             # syslog tool action dispatch
│   │   └── server.rs            # RMCP implementation
│   ├── docker_ingest/           # Docker Remote Ingestion
│   └── observability.rs         # Tracing and metrics
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
2. **Database Changes**: Queries go in `src/db/queries.rs`. Schema is managed in `src/db/mod.rs`.
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

## Dependencies

| Dependency | Purpose |
|------------|---------|
| `sqlx` | Async SQLite driver |
| `axum` | HTTP server for MCP |
| `tokio` | Async runtime |
| `serde` | Serialization/Deserialization |
| `tracing` | Observability and logging |
