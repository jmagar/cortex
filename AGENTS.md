# CLAUDE.md — syslog-mcp

Rust binary: syslog receiver (UDP/TCP) + MCP server for homelab log intelligence. Receives RFC 3164/5424 syslog from all homelab hosts, stores in SQLite with FTS5, exposes a single `syslog` MCP tool (with action dispatch) for AI agents.

## Commands

```bash
cargo build                      # debug build
cargo build --release            # release build
cargo run                        # run locally (reads config.toml)
cargo test                       # test suite
cargo clippy                     # lint (must pass before committing)
cargo fmt                        # format (enforced by CI)
docker compose up -d             # production deployment
docker compose down              # stop
docker compose logs -f           # follow logs
docker compose build             # rebuild image
syslog compose doctor            # diagnose live Compose/listener ownership
syslog compose status --json     # inspect canonical syslog-mcp container/project
syslog compose pull              # pull image for resolved Compose project
syslog compose up                # run docker compose up -d for resolved service
syslog compose restart           # restart resolved service
syslog compose logs --tail 20    # bounded compose logs
syslog db status                 # inspect SQLite maintenance state
syslog db integrity              # run SQLite integrity_check
syslog db backup                 # create WAL-safe SQLite backup
```

```bash
just dev                         # cargo run alias
just test                        # cargo test alias
just health                      # curl /health | jq (server must be running)
just gen-token                   # openssl rand -hex 32 (generate API token)
just build-plugin                # release build → installs binary to bin/ (Linux; requires git lfs install)
just publish [major|minor|patch] # bump version, tag, push (triggers CI)
just generate-cli                # build standalone CLI (server must be running)
```

## Architecture

Key modules in `src/` (most are directories with sidecar `*_tests.rs` files):

| Module | Purpose |
|--------|---------|
| `config.rs` | Config: `config.toml` + env vars (`SYSLOG_*`, `SYSLOG_MCP_*`, `SYSLOG_API_*`, `SYSLOG_DOCKER_*`) |
| `runtime.rs` | `RuntimeCore`: wires all subsystems, starts syslog ingest, spawns maintenance tasks |
| `app/` | Service layer: `SyslogService`, request/response models, business logic |
| `db/` | SQLite pool, FTS5 queries, maintenance (retention, storage enforcement) |
| `syslog/` | UDP + TCP listeners, RFC 3164/5424 parsing, mpsc batch writer |
| `mcp/` | RMCP Streamable HTTP server, single `syslog` tool with action dispatch |
| `api.rs` | Optional non-MCP REST API (enabled via `SYSLOG_API_ENABLED=true`) |
| `docker_ingest/` | Docker container log ingestion via remote docker-socket-proxy endpoints |
| `main.rs` | Entrypoint: `serve mcp` (full server with ingest) or `mcp` (stdio query-only) |

Tests: unit tests live in sidecar files beside their source modules (e.g. `src/db/queries_tests.rs`). Source files keep only the `#[cfg(test)] #[path = "..._tests.rs"] mod tests;` hook, so sidecar tests compile as module-local unit tests with `use super::*` access to private items. Run with `cargo test`.

## Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| 1514 | UDP + TCP | Syslog receiver (not 514 — avoids `CAP_NET_BIND_SERVICE`) |
| 3100 | TCP | Shared HTTP listener for MCP (`POST /mcp`, `GET /health`) and OTLP HTTP ingest (`POST /v1/logs`); non-loopback OAuth-only `/v1/logs` exposure is blocked at startup unless `SYSLOG_MCP_TOKEN` is set |

## MCP Tools

One MCP tool: **`syslog`** — dispatches by `action` argument.

| Action | Description |
|--------|-------------|
| `search` | Full-text search (FTS5 syntax) with host/severity/app/time filters |
| `tail` | Recent N entries, optionally filtered by host/app |
| `errors` | Error/warning summary grouped by host and severity |
| `hosts` | All known hosts with first/last seen + log counts |
| `correlate` | Cross-host event correlation in a time window |
| `stats` | DB stats (total logs, logical/physical size, free disk, configured thresholds, write-block state, time range) |
| `status` | Lightweight runtime and DB health |
| `apps` | Distinct application names with log and host counts |
| `sessions` | AI transcript sessions grouped by project/tool/session/host |
| `search_sessions` | Full-text search over indexed AI transcript sessions |
| `usage_blocks` | AI transcript activity grouped into time blocks |
| `project_context` | Recent AI transcript context for a project |
| `list_ai_tools` | AI tools present in transcript metadata |
| `list_ai_projects` | AI projects present in transcript metadata |
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
| `compose_status` | Redacted Docker Compose runtime status projection |
| `compose_doctor` | Redacted Docker Compose diagnostics projection |
| `help` | Built-in usage reference |

## Plugin Skills

Skills available after installing the Claude Code plugin (`plugins/skills/`):

| Command | Description |
|---------|-------------|
| `/syslog:dr` | Full health check: MCP, HTTP /health, service status, syslog port, Docker ingest, fleet drop-ins (named `dr` to avoid colliding with Claude Code's built-in `/doctor`) |
| `/syslog:deploy-dropins` | Push rsyslog forwarding configs to `fleet_hosts` via SSH (idempotent) |

## Config

`config.toml` at repo root for local dev. **Not copied into Docker** — the Dockerfile was cleaned up (no COPY for config.toml). In Docker, defaults + env vars apply exclusively.

```bash
# Syslog listener
SYSLOG_HOST=0.0.0.0              # host only, no port
SYSLOG_PORT=1514                 # shared by UDP + TCP
SYSLOG_MAX_MESSAGE_SIZE=8192
SYSLOG_BATCH_SIZE=100
SYSLOG_FLUSH_INTERVAL=500        # ms

# MCP server
SYSLOG_MCP_HOST=0.0.0.0
SYSLOG_MCP_PORT=3100
SYSLOG_MCP_TOKEN=your-secret-token      # optional; enables Bearer auth on /mcp
                                         # (SYSLOG_MCP_API_TOKEN still works, logs deprecation)
SYSLOG_MCP_ALLOWED_HOSTS=myhost.local   # optional; comma-separated extra Host allowlist
SYSLOG_MCP_ALLOWED_ORIGINS=https://app  # optional; comma-separated extra Origin allowlist

# Storage
SYSLOG_MCP_DB_PATH=data/syslog.db
SYSLOG_MCP_POOL_SIZE=4
SYSLOG_MCP_RETENTION_DAYS=90     # 0 = keep forever
SYSLOG_MCP_MAX_DB_SIZE_MB=1024        # 0 = disable logical DB size guard
SYSLOG_MCP_RECOVERY_DB_SIZE_MB=900    # cleanup target after DB-size breach
SYSLOG_MCP_MIN_FREE_DISK_MB=512       # 0 = disable free-disk guard
SYSLOG_MCP_RECOVERY_FREE_DISK_MB=768  # cleanup target after free-disk breach
SYSLOG_MCP_CLEANUP_INTERVAL_SECS=60   # storage-budget enforcement interval (>= 5)
SYSLOG_MCP_CLEANUP_CHUNK_SIZE=1000    # rows deleted per enforcement cycle

# OAuth / JWT auth (disabled by default — set SYSLOG_MCP_AUTH_MODE=oauth to activate)
SYSLOG_MCP_AUTH_MODE=bearer             # bearer (default) or oauth
SYSLOG_MCP_PUBLIC_URL=https://syslog.example.com  # required when SYSLOG_MCP_AUTH_MODE=oauth
SYSLOG_MCP_GOOGLE_CLIENT_ID=...         # required when SYSLOG_MCP_AUTH_MODE=oauth
SYSLOG_MCP_GOOGLE_CLIENT_SECRET=...     # required when SYSLOG_MCP_AUTH_MODE=oauth
# Paths, TTLs, allowlist → config.toml [mcp.auth] (not env vars). See docs/OAUTH.md.

# Non-MCP REST API (disabled by default)
SYSLOG_API_ENABLED=false                # set true to mount /api/* endpoints
SYSLOG_API_TOKEN=your-api-token         # required when SYSLOG_API_ENABLED=true

# Docker container log ingestion (disabled by default)
SYSLOG_DOCKER_INGEST_ENABLED=false      # set true to ingest from docker-socket-proxy hosts
SYSLOG_DOCKER_HOSTS=host-a,host-b      # comma-separated hostnames → http://<host>:2375
SYSLOG_DOCKER_RECONNECT_INITIAL_MS=1000
SYSLOG_DOCKER_RECONNECT_MAX_MS=60000

# Log verbosity (set to debug or trace for development)
RUST_LOG=info
```

## Key Files

| File | Purpose |
|------|---------|
| `config.toml` | Runtime config (syslog bind, DB path, retention) |
| `docker-compose.yml` | Production deployment (ports 1514, 3100) |
| `docs/SETUP.md` | Per-host syslog forwarding (rsyslog, UniFi, ATT router, WSL) |
| `src/db/queries.rs` | All SQL queries and FTS5 search implementation |
| `src/mcp/tools.rs` | Single `syslog` tool with action dispatch |
| `config/mcporter.json` | mcporter config (HTTP transport to localhost:3100) |
| `SYSLOG_DOCKER_HOSTS` env var | Docker ingest host list — comma-separated hostnames, each becomes `http://<host>:2375` |
| `scripts/smoke-test.sh` | Live smoke test — all MCP actions via mcporter, strict PASS/FAIL |
| `scripts/backup.sh` | WAL-safe SQLite backup script (checkpoint + `.backup` method) |
| `scripts/reset-db.sh` | WAL-safe backup + destructive DB reset helper for local/dev recovery |
| `scripts/bump-version.sh` | Bump version across all version-bearing files; called by `just publish` |
| `syslog db status\|integrity\|checkpoint\|vacuum\|backup` | Direct SQLite maintenance commands for the configured DB |
| `scripts/check-version-sync.sh` | Assert all version-bearing files have the same version (used in CI) |
| `scripts/block-env-commits.sh` | Pre-commit hook that blocks commits containing env credential patterns |
| `CHANGELOG.md` | Version history; entry required per version bump |
| `.lavra/memory/recall.sh` | Query the local knowledge DB: `bash .lavra/memory/recall.sh <keyword>` |

## Gotchas

- **Port 1514 not 514** — avoids needing root; use iptables PREROUTING to redirect 514→1514 for devices that can't be reconfigured (see docs/SETUP.md)
- **Cargo.lock is tracked** — binary crates should commit Cargo.lock for reproducible builds (Cargo docs guidance)
- **FTS5 query syntax** — `syslog action=search` uses SQLite FTS5: `error AND nginx`, `"disk full"`, `kern OR syslog`; invalid FTS5 syntax returns a db error. **Hyphen is the FTS5 NOT operator** — to search for hyphenated terms, use phrase syntax: `"smoke-test"` not `smoke-test`
- **WAL mode** — SQLite runs in WAL mode; copying `.db`, `.db-wal`, and `.db-shm` together without a checkpoint captures potentially inconsistent state. Safe backup options: (1) run `PRAGMA wal_checkpoint(FULL);` first, then copy all three files, or (2) use `sqlite3 source.db '.backup dest.db'` which is WAL-safe and requires no manual checkpoint
- **MCP transport** — HTTP MCP runs in stateless JSON-response mode on `POST /mcp`; SSE streams (`GET /mcp` or `/sse`) are not enabled in the current server.
- **Data volume** — DB lives in `./data/` (bind mount); `*.db` is gitignored so the database files won't be committed
- **Retention purge** — `retention_days` defaults to 90; logs older than 90 days are **permanently deleted hourly** with no recovery path. Set `SYSLOG_MCP_RETENTION_DAYS=0` to disable purging entirely.
- **Storage guardrail** — Logical DB size and free-disk limits are enabled by default (`1024/900 MB` DB, `512/768 MB` free disk). When thresholds are breached, the server deletes oldest logs by `received_at` until recovery targets are met. If cleanup still cannot recover enough space, the batch writer blocks new writes until storage becomes healthy again.
- **CEF hostname vs source_ip** — For UniFi CEF messages, the stored `hostname` comes from the CEF `UNIFIdeviceName` extension field (message body), **not** the syslog header. Any LAN device can spoof this value. `source_ip` is the only network-verified identity. See `src/syslog/parser.rs` for the trust boundary.
- **Batch writer failure** — If `insert_logs_batch` fails, the batch is retained for the next flush (up to 1000 entries, then discarded). A 250ms pause prevents hammering a failing DB. Persistent write failures will eventually cause data loss via the 10K-entry channel cap. The mpsc channel is in-memory only — no durable write-ahead log.
- **correlate action limit cap** — The `limit` parameter is silently capped at 999 (not 1000) because the implementation fetches `limit+1` rows to detect truncation, and `search` hard-caps at 1000.
- **Auth / trust model** — MCP endpoint is unauthenticated by default; any client reaching port 3100 has full log read access. Set `SYSLOG_MCP_TOKEN` to require Bearer auth. CORS is restricted to `localhost:3100` (browser-only; curl/mcporter unaffected). If exposing via SWAG/reverse proxy, add auth at the proxy layer or set the token. See README Security section for details.
- **FTS5 phantom rows** — When logs are deleted by retention purge or storage enforcement, their FTS5 index entries persist as phantom rows in `logs_fts` until the next merge cycle. The MCP query path is unaffected (the JOIN to `logs` prunes phantoms at query time), but direct SQLite access to `logs_fts` reveals porter-stemmed tokens for deleted messages. For right-to-erasure compliance (GDPR/HIPAA), use `INSERT INTO logs_fts(logs_fts) VALUES('rebuild')` after deletion instead of the periodic incremental merge. Monitor phantom row count via `stats` action → `phantom_fts_rows`.
- **OAuth refresh token TTL** — Refresh tokens default to 8h (`refresh_token_ttl_secs = 28800`). lab-auth's default is 30 days; syslog-mcp deliberately uses 8h for the read-only homelab profile. Adjustable via `[mcp.auth].refresh_token_ttl_secs` in config.toml.
- **Stdio mode always uses LoopbackDev** — `cargo run -- mcp` (stdio query-only) always uses `AuthPolicy::LoopbackDev` regardless of config. No auth is enforced. This is intentional: the local process boundary is the trust boundary for stdio clients.
- **Docker bind-mount ownership** — `auth.db` and `auth-jwt.pem` are written by the container UID. Host-side backup scripts or file managers may need `sudo` or a sidecar copy step to read them without permission errors.

## Testing MCP Tools

```bash
# Full smoke test (requires server running)
bash scripts/smoke-test.sh

# WAL-safe backup, then destructive DB reset (service should be stopped first)
bash scripts/reset-db.sh

# Using mcporter (project config at config/mcporter.json)
mcporter list syslog-mcp --config config/mcporter.json
mcporter call --config config/mcporter.json syslog-mcp.syslog action=stats
mcporter call --config config/mcporter.json syslog-mcp.syslog action=tail n=10
mcporter call --config config/mcporter.json syslog-mcp.syslog action=search query=error limit=5

# Health check
curl http://localhost:3100/health

# Tail recent logs (raw JSON-RPC)
curl -s -X POST http://localhost:3100/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"syslog","arguments":{"action":"tail","n":10}}}'

# Search
curl -s -X POST http://localhost:3100/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"syslog","arguments":{"action":"search","query":"error","limit":5}}}'

# Stats
curl -s -X POST http://localhost:3100/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"syslog","arguments":{"action":"stats"}}}'

# stdio mode (query-only, no ingest — useful for Claude Desktop)
cargo run -- mcp
```

`syslog compose` commands resolve the live Compose owner before mutation. They refuse ambiguous cwd fallback, stale Compose labels, listener conflicts, and destructive `down` without `--yes`.


<!-- BEGIN BEADS INTEGRATION v:1 profile:compact hash:f65d5d33 -->
## Issue Tracking with bd (beads)

This project uses **bd (beads)** for ALL issue tracking. Full beads context is injected by the session hook — see `bd --help` for complete command reference.

**Essential workflow:** `bd ready` → `bd update <id> --claim` → work → `bd close <id>`

Do NOT use markdown TODOs, TaskCreate, or external trackers. Always use `--json` for programmatic output. Link discovered work with `discovered-from` dependencies.

## Session Completion

Work is NOT complete until `git push` succeeds. The session-close hook enforces this checklist:
`git status` → `git add` → `git commit` → `bd dolt push` → `git push`

<!-- END BEADS INTEGRATION -->


## Version Bumping

**Every feature branch push MUST bump the version in ALL version-bearing files.**

Bump type is determined by the commit message prefix:
- `feat!:` or `BREAKING CHANGE` → **major** (X+1.0.0)
- `feat` or `feat(...)` → **minor** (X.Y+1.0)
- Everything else (`fix`, `chore`, `refactor`, `test`, `docs`, etc.) → **patch** (X.Y.Z+1)

**Files to update (if they exist in this repo):**
- `Cargo.toml` — `version = "X.Y.Z"` in `[package]`
- `package.json` — `"version": "X.Y.Z"`
- `pyproject.toml` — `version = "X.Y.Z"` in `[project]`
- `.claude-plugin/plugin.json` — `"version": "X.Y.Z"`
- `.codex-plugin/plugin.json` — `"version": "X.Y.Z"`
- `gemini-extension.json` — `"version": "X.Y.Z"`
- `README.md` — version badge or header
- `CHANGELOG.md` — new entry under the bumped version

All files MUST have the same version. Never bump only one file.
CHANGELOG.md must have an entry for every version bump.
