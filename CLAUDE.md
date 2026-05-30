# cortex

## Purpose

**Syslog Intelligence for Homelabs** — Receives RFC 3164/5424 syslog from all homelab hosts (UDP/TCP), ingests Docker logs via socket proxy, stores everything in SQLite with FTS5, and exposes a comprehensive `syslog` MCP tool for AI agents.

**Status**: Active development, Production-ready
**Version**: 0.29.0

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
| `src/doctor.rs` | Self-debugging diagnostics — binary, DB, and AI-watch health |
| `src/deploy.rs` | CLI remote deploy — provisions cortex on remote hosts |
| `src/api.rs` | HTTP API surface — all routes for CLI HTTP transport |

## Project Structure

```
cortex/
├── src/
│   ├── main.rs                  # CLI entrypoint (serve mcp / mcp stdio)
│   ├── lib.rs                   # Library root, module declarations
│   ├── cli.rs                   # Standalone CLI binary
│   ├── compose.rs               # Docker Compose lifecycle CLI
│   ├── scanner.rs               # AI transcript indexer (Claude/Codex/Gemini)
│   ├── setup.rs                 # First-run setup + plugin bootstrap
│   ├── runtime.rs               # RuntimeCore — wiring and lifecycle
│   ├── config.rs                # Configuration (TOML + env)
│   ├── api.rs                   # HTTP API surface
│   ├── otlp.rs                  # OpenTelemetry/OTLP ingestion
│   ├── ingest.rs                # Log ingestion coordinator
│   ├── ingest_metadata.rs       # Ingestion metadata helpers
│   ├── ai_watch.rs              # AI transcript watcher (live indexing)
│   ├── observability.rs         # Tracing and metrics
│   ├── logging.rs               # Service log setup
│   ├── app.rs / db.rs / mcp.rs / syslog.rs / docker_ingest.rs   # Module entrypoints
│   ├── deploy.rs                # CLI remote deploy
│   ├── cli/                     # CLI command implementations
│   ├── compose/                 # Docker Compose helpers
│   ├── setup/                   # First-run setup internals
│   ├── app/                     # Service Layer (business logic)
│   │   ├── service.rs           # SyslogService implementation
│   │   ├── models.rs            # Request/response types
│   │   ├── correlate.rs         # Event correlation logic
│   │   ├── error.rs             # Error types
│   │   └── time.rs              # Time utilities
│   ├── db/                      # Database Layer (SQLite + FTS5)
│   │   ├── pool.rs              # Pool management and schema
│   │   ├── queries.rs           # SQL queries and search logic
│   │   ├── analytics.rs         # Stats and timeline logic
│   │   ├── ingest.rs            # Log insertion logic
│   │   ├── maintenance.rs       # Retention and storage guardrails
│   │   ├── notifications.rs     # Push notification persistence
│   │   ├── error_signatures.rs  # Error pattern signatures
│   │   ├── error_detection/     # Error detection rules + scoring
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
│   │   └── enrichment.rs        # Log enrichment (legacy path)
│   ├── enrich/                  # Enrichment framework — structured field extraction at ingest
│   ├── notifications/           # Push notification dispatch (Apprise, digest, rules)
│   ├── logging/                 # Structured service logging
│   ├── scanner/                 # AI transcript scanner
│   │   ├── claude.rs            # Claude transcript parsing
│   │   ├── codex.rs             # Codex transcript parsing
│   │   └── checkpoint.rs        # Scan progress checkpointing
│   ├── doctor.rs                # Self-debugging diagnostics (binary, DB, AI-watch)
│   └── docker_ingest/           # Docker remote ingestion
├── bin/                         # Installed binaries (syslog)
├── config/                      # Deployment config templates
├── deploy/                      # Host-side manifests (rsyslog, otel)
├── docs/                        # Deep-dive documentation
├── plugins/                     # Claude Code skills and hooks
├── scripts/                     # Maintenance and CI scripts
└── tests/                       # Integration tests + tests/test_live.sh smoke runner
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
SQLite Database (/data/cortex.db)
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
- **Self-Debugging Surfaces**: `syslog ai doctor` checks binary-vs-container version parity, DB health, and AI-watch coordination in one command. CI-safe and idempotent.

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

**Prerequisite:** `just test` uses [cargo-nextest](https://nexte.st), which is **not**
bundled with the Rust toolchain. Fresh checkouts must install it once, or `just test`
fails with `error: no such subcommand: nextest`:

```bash
cargo install cargo-nextest --locked
```

```bash
just test               # Run all unit and integration tests (requires cargo-nextest)
just test-doc           # Run doc tests (nextest does not execute these)
just test-live          # Live smoke test against a running server (tests/test_live.sh)
bash scripts/smoke-test.sh  # Lower-level smoke harness (used by CI; superset of test-live)
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
| `just build` | Cargo release build | `just build` |
| `just validate-skills` | Validate plugin skill manifests | `just validate-skills` |
| `just gen-token` | Generate a random API token | `just gen-token` |
| `just build-plugin` | Copy release binary into bin/ | `just build-plugin` |
| `just publish [bump]` | Version bump + tag + push | `just publish patch` |
| `just setup` | Initialize .env from .env.example | `just setup` |
| `syslog ai doctor` | Self-debug: binary vs container version, DB health, AI-watch | `syslog ai doctor` |
| `syslog db status` | DB size, WAL, page count, drift check | `syslog db status` |
| `syslog db integrity` | SQLite integrity_check | `syslog db integrity --quick` |
| `syslog db vacuum` | Reclaim DB space | `syslog db vacuum` |
| `syslog db backup` | Backup DB to path | `syslog db backup --output /tmp/out.db` |
| `syslog setup check` | Validate config and env | `syslog setup check` |
| `syslog setup repair` | Auto-fix missing config | `syslog setup repair` |
| `syslog compose status` | Container running status | `syslog compose status` |
| `syslog compose doctor` | Full coordination diagnostics | `syslog compose doctor` |
| `syslog source-ips` | List unique source IPs with log counts | `syslog source-ips --limit 50` |
| `syslog timeline` | Log volume over time (bucketed) | `syslog timeline --bucket hour` |
| `syslog patterns` | Recurring message patterns | `syslog patterns --top-n 25` |
| `syslog ingest-rate` | Current ingest rate (logs/sec) | `syslog ingest-rate --by-host` |
| `syslog sig list` | List unaddressed error signatures | `syslog sig list` |
| `syslog sig ack HASH` | Acknowledge/suppress an error signature | `syslog sig ack ab12cd --notes "fixed"` |
| `syslog sig unack HASH` | Revoke an acknowledgement | `syslog sig unack ab12cd` |
| `syslog notify recent` | Recent notification firings | `syslog notify recent --limit 25` |
| `syslog notify test` | Send a test notification via Apprise (HTTP-only) | `syslog --http notify test --body "ping"` |

## Diagnostics: host/container drift

Two coordination diagnostics guard against the CLI and the container talking
to different SQLite files:

- `data-mount` — verifies the host directory bind-mounted at `/data` matches
  `CORTEX_DATA_VOLUME`.
- `ai-watch-coord` — verifies the host systemd `syslog-ai-watch.service`
  resolves `CORTEX_DB_PATH` to the same canonical directory as the
  container's `/data` bind.

Where they run:

- `syslog compose doctor` — always runs both phases. `--json` includes them
  under a `coordination` array. A canonical mismatch is fatal (exit 1).
- `syslog db status --check-coord` — opt-in. Adds both phases to the JSON
  payload under `coordination`. The default `syslog db status` path is
  unchanged (no shell-outs).

Both phases shell out to `docker inspect` and `systemctl --user show`, which
adds roughly 100-200ms per invocation. Within a single `compose doctor`
invocation the results are cached so each shell-out fires only once.

Status semantics for these phases:

- `ok` — canonical paths match.
- `skipped` — ai-watch unit is not installed/loadable, or container is not
  running (data-mount only). Reserved for "ai-watch absent" — never used to
  hide failures.
- `warn` — could not enumerate inputs (docker/systemctl failed, canonicalize
  hit `ENOENT` / `EACCES`). Emits the OS error verbatim; we never silently
  fall back to literal-string compare.
- `error` — both sides resolved and the canonical paths differ.

## Dependencies

| Dependency | Purpose |
|------------|---------|
| `sqlx` | Async SQLite driver |
| `axum` | HTTP server for MCP |
| `tokio` | Async runtime |
| `serde` | Serialization/Deserialization |
| `tracing` | Observability and logging |


<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->
