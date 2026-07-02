# CLAUDE.md — cortex

Rust binary: syslog receiver (UDP/TCP) + MCP server for homelab log intelligence. Receives RFC 3164/5424 syslog from all homelab hosts, stores in SQLite with FTS5, exposes a single `cortex` MCP tool (with action dispatch) for AI agents.

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
cortex compose doctor            # diagnose live Compose/listener ownership
cortex compose status --json     # inspect canonical cortex container/project
cortex compose pull              # pull image for resolved Compose project
cortex compose up                # run docker compose up -d for resolved service
cortex compose restart           # restart resolved service
cortex compose logs --tail 20    # bounded compose logs
cortex db status                 # inspect SQLite maintenance state
cortex db integrity              # run SQLite integrity_check
cortex db backup                 # create WAL-safe SQLite backup
cortex assess skill <skill> [--since 7d] [--tool codex] [--all|--limit N] [--no-llm]
cortex assess abuse [--incident-id ID] [--no-llm]  # unified assess namespace; see README "Skill and abuse assessment"
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
| `config.rs` | Config: `config.toml` + `CORTEX_*` env vars (listener, MCP, storage, API, Docker ingest, notifications) |
| `runtime.rs` | `RuntimeCore`: wires all subsystems, starts syslog ingest, spawns maintenance tasks |
| `app/` | Service layer: `SyslogService`, request/response models, business logic |
| `db/` | SQLite pool, FTS5 queries, maintenance (retention, storage enforcement) |
| `receiver/` | UDP + TCP listeners, RFC 3164/5424 parsing, mpsc batch writer |
| `mcp/` | RMCP Streamable HTTP server, single `cortex` tool with action dispatch |
| `api.rs` | Always-on non-MCP REST API (`/api/*`); requires `CORTEX_API_TOKEN` at startup |
| `agent/`, `heartbeat_agent.rs` | Host-local cortex agent: heartbeat, syslog forwarding, and Docker log streaming from the local Docker socket |
| `docker_ingest/` | Legacy central pull compatibility path for explicit remote Docker Engine HTTP endpoints |
| `main.rs` | Entrypoint: `serve mcp` (full server with ingest) or `mcp` (stdio query-only) |

Tests: unit tests live in sidecar files beside their source modules (e.g. `src/db/queries_tests.rs`). Source files keep only the `#[cfg(test)] #[path = "..._tests.rs"] mod tests;` hook, so sidecar tests compile as module-local unit tests with `use super::*` access to private items. Run with `cargo test`.

## Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| 1514 | UDP + TCP | Syslog receiver (not 514 — avoids `CAP_NET_BIND_SERVICE`) |
| 3100 | TCP | Shared HTTP listener for MCP (`POST /mcp`, `GET /health`) and OTLP HTTP ingest (`POST /v1/logs`); non-loopback OAuth-only `/v1/logs` exposure is blocked at startup unless `CORTEX_TOKEN` is set |

## MCP Tools

One MCP tool: **`cortex`** — dispatches by `action` argument. 51 actions, generated from `ACTION_SPECS` in `src/mcp/actions.rs` (the single authoritative registry — regenerate this table from there).

Scope taxonomy: every action requires `cortex:read` except the five **admin** actions `ack_error`, `unack_error`, `file_tails`, `notifications_test`, and `llm_invocations`, which require `cortex:admin` (static bearer tokens get read-only unless `CORTEX_STATIC_TOKEN_ADMIN=true`); `help` is info-only (no scope gate).

| Action | Description |
|--------|-------------|
| `search` | Full-text search over syslog messages |
| `filter` | Filter logs by indexed fields without a full-text query |
| `tail` | Stream the most recent log entries |
| `errors` | List recent error-level log entries |
| `hosts` | Enumerate all known source hostnames |
| `map` | Map homelab inventory and answer graph-backed topology questions |
| `host_state` | Fetch latest bounded heartbeat state for a host |
| `fleet_state` | Fleet-wide heartbeat snapshot with pressure flags |
| `correlate` | Correlate events across hosts/services |
| `correlate_state` | Correlate logs with heartbeat summaries around a reference time |
| `stats` | Aggregate log statistics |
| `status` | Server health and ingestion status |
| `apps` | Enumerate all known application names |
| `sessions` | List AI transcript sessions |
| `search_sessions` | Full-text search over AI transcript sessions |
| `abuse` | Detect resource-abuse patterns in AI sessions |
| `abuse_incidents` | List detected abuse incidents |
| `abuse_investigate` | Deep-dive investigation of an abuse incident |
| `ai_correlate` | Correlate AI transcript events with syslog |
| `topic_correlate` | Resolve a topic to graph entities and correlate all related logs into a unified timeline |
| `usage_blocks` | Summarise AI session usage by project |
| `project_context` | Full project context from AI transcripts |
| `list_ai_tools` | List AI tools observed in transcripts |
| `list_ai_projects` | List AI projects with transcript activity |
| `source_ips` | Enumerate unique source IP addresses |
| `timeline` | Log volume over time (bucketed) |
| `patterns` | Recurring message patterns |
| `context` | Contextual log entries around a pivot |
| `get` | Fetch a single log entry by ID |
| `ingest_rate` | Current log ingestion rate |
| `silent_hosts` | Hosts that have gone silent |
| `clock_skew` | Detect clock skew between hosts |
| `anomalies` | Detect log-volume anomalies |
| `compare` | Compare log patterns between time windows |
| `compose_status` | Docker Compose stack status |
| `compose_doctor` | Docker Compose coordination diagnostics |
| `unaddressed_errors` | List unacknowledged error signatures |
| `notifications_recent` | Recent notification firings |
| `similar_incidents` | Find similar past incidents |
| `ask_history` | Query AI transcript history |
| `incident_context` | Full context for an incident |
| `graph` | Resolve graph entities, neighborhoods, and evidence-backed explanations |
| `skill_events` | List extracted AI skill-invocation events |
| `skill_incidents` | List detected skill-usage incidents (negative signals after a skill loaded) |
| `skill_investigate` | Deep-dive investigation of a skill-usage incident, skill-first (accepts a skill name directly) |
| `file_tails` | **(admin)** Manage Cortex-owned file-tail ingest sources |
| `ack_error` | **(admin)** Acknowledge an error signature |
| `unack_error` | **(admin)** Revoke an error signature acknowledgement |
| `notifications_test` | **(admin)** Send a test notification via Apprise |
| `llm_invocations` | **(admin)** Recent LLM invocation audit records (concurrency/rate-limit/circuit-breaker denials included) |
| `help` | List available actions and their parameters |

## Plugin Skills

9 skills ship with the Claude Code plugin — see `plugins/cortex/skills/<skill>/SKILL.md` for each:

`cortex` (primary log-intelligence skill), `cortex-deploy-dropins` (push rsyslog forwarding configs to `fleet_hosts` via SSH), `cortex-dr` (full health check; named `dr` to avoid colliding with Claude Code's built-in `/doctor`), `cortex-frustration-assessment` (analyze `abuse_investigate` evidence bundles), `cortex-logs` (Compose service log tailing), `cortex-redeploy` (re-run plugin setup hook), `cortex-report` (time-bounded markdown health reports), `cortex-troubleshoot` (connection/ingest failure triage), `cortex-version-check` (running container vs local Compose image).

## Config

`config.toml` at repo root for local dev. **Not copied into Docker** — the Dockerfile was cleaned up (no COPY for config.toml). In Docker, defaults + env vars apply exclusively.

```bash
# Syslog listener
CORTEX_RECEIVER_HOST=0.0.0.0              # host only, no port
CORTEX_RECEIVER_PORT=1514                 # shared by UDP + TCP
CORTEX_MAX_MESSAGE_SIZE=8192
CORTEX_BATCH_SIZE=100
CORTEX_FLUSH_INTERVAL=500        # ms

# MCP server
CORTEX_HOST=127.0.0.1               # default loopback; 0.0.0.0 requires CORTEX_TOKEN/OAuth
CORTEX_PORT=3100
CORTEX_MCP_BIND=127.0.0.1           # Compose host publish interface for port 3100
CORTEX_TOKEN=your-secret-token      # optional on loopback; required on non-loopback binds
CORTEX_ALLOWED_HOSTS=myhost.local   # optional; comma-separated extra Host allowlist
CORTEX_ALLOWED_ORIGINS=https://app  # optional; comma-separated extra Origin allowlist

# Storage
CORTEX_DB_PATH=data/cortex.db
CORTEX_POOL_SIZE=8                # MCP reads get pool_size - 1 permits (1 reserved for writer)
CORTEX_SQLITE_PAGE_CACHE_MB=128   # total SQLite page-cache budget across pool
CORTEX_SQLITE_MMAP_MB=256         # bounded mmap; resident mapped pages may count toward cgroup memory
CORTEX_HEAVY_READ_CONCURRENCY=1   # shared service-layer limiter for expensive reads
CORTEX_WAL_CHECKPOINT_MB=256      # WAL size threshold for bounded PASSIVE checkpoint attempts
CORTEX_GRAPH_REFRESH_INTERVAL_SECS=0    # in-server graph projection scheduler; disabled by default, CLI rebuild still works
CORTEX_INVENTORY_GRAPH_PROJECTION_ENABLED=false # inventory cache refresh still runs; graph projection is opt-in
CORTEX_RETENTION_DAYS=90     # 0 = keep forever; hourly purge, err+ exempt (see Retention)
CORTEX_MAX_DB_SIZE_MB=1024        # 0 = disable logical DB size guard (breach deletes oldest)
CORTEX_RECOVERY_DB_SIZE_MB=900    # cleanup target after DB-size breach
CORTEX_MIN_FREE_DISK_MB=0         # disabled by default; breach BLOCKS WRITES (no deletes)
CORTEX_RECOVERY_FREE_DISK_MB=0    # hysteresis target before writes resume
CORTEX_CLEANUP_INTERVAL_SECS=60   # storage-budget enforcement interval (>= 5)
CORTEX_CLEANUP_CHUNK_SIZE=2000    # rows deleted per enforcement cycle
CORTEX_ERR_FLOOR_WINDOW_HOURS=24  # err+ floor: protect recent err+ rows from disk-pressure deletes
CORTEX_ERR_FLOOR_PER_SOURCE_CAP=10000  # max protected err+ rows per source IP

# OAuth / JWT auth (disabled by default — set CORTEX_AUTH_MODE=oauth to activate)
CORTEX_AUTH_MODE=bearer             # bearer (default) or oauth
CORTEX_PUBLIC_URL=https://cortex.example.com  # required when CORTEX_AUTH_MODE=oauth
CORTEX_GOOGLE_CLIENT_ID=...         # required when CORTEX_AUTH_MODE=oauth
CORTEX_GOOGLE_CLIENT_SECRET=...     # required when CORTEX_AUTH_MODE=oauth
# Paths, TTLs, allowlist → config.toml [mcp.auth] (not env vars). See docs/OAUTH.md.

# Non-MCP REST API (always on; gated by its token)
CORTEX_API_TOKEN=your-api-token         # REQUIRED at startup — /api/* is always mounted

# Managed file-tail sources
# Stored in the parent directory of CORTEX_DB_PATH as file-tails.json.
# Manage with: cortex ingest file-tail list|status|add|remove|enable|disable

# Legacy central pull Docker ingestion compatibility mode (disabled by default)
# Current deployments use the host-local cortex agent, which streams Docker logs
# from unix:///var/run/docker.sock on each host. Keep CORTEX_DOCKER_* for
# compatibility fixtures or explicit remote Docker Engine HTTP endpoints only.
CORTEX_DOCKER_INGEST_ENABLED=false
CORTEX_DOCKER_HOSTS=host-a,host-b      # comma-separated hosts → http://<host>:2375
CORTEX_DOCKER_RECONNECT_INITIAL_MS=1000
CORTEX_DOCKER_RECONNECT_MAX_MS=30000

# Log verbosity (set to debug or trace for development)
RUST_LOG=info
```

## Key Files

| File | Purpose |
|------|---------|
| `config.toml` | Runtime config (syslog bind, DB path, retention) |
| `docker-compose.yml` | Production deployment (ports 1514, 3100) |
| `docs/SETUP.md` | Setup guide (clone, build, configure, deploy, verify); per-host forwarder configs (rsyslog, UniFi, ATT router, WSL) live in README "Syslog Forwarder Setup" |
| `src/db/queries.rs` | All SQL queries and FTS5 search implementation |
| `src/mcp/actions.rs` | `ACTION_SPECS` — authoritative registry of all 51 MCP actions and their scopes |
| `src/mcp/tools.rs` | Single `cortex` tool with action dispatch |
| `config/mcporter.json` | mcporter config (HTTP transport to localhost:3100) |
| `config/systemd/` | `cortex-backup.service` / `.timer` — daily WAL-safe backup units |
| `scripts/smoke-test.sh` | Live smoke test — all MCP actions via mcporter, strict PASS/FAIL |
| `scripts/backup.sh` | WAL-safe SQLite backup script (checkpoint + `.backup` method) |
| `scripts/reset-db.sh` | WAL-safe backup + destructive DB reset helper for local/dev recovery |
| `release/components.toml` | Declarative source of truth for version-bearing files; consumed by `cargo xtask` |
| `xtask/` | `cargo xtask` workspace crate: `bump-version`, `check-version-sync`, `check-release-versions` |
| `cortex db status\|integrity\|checkpoint\|vacuum\|backup` | Direct SQLite maintenance commands for the configured DB |
| `scripts/block-env-commits.sh` | Pre-commit hook that blocks commits containing env credential patterns |
| `CHANGELOG.md` | Version history; entry required per version bump |
| `.lavra/memory/recall.sh` | Query the local knowledge DB: `bash .lavra/memory/recall.sh <keyword>` |

## Retention

(Referenced by the `purge_old_logs` rustdoc in `src/db/maintenance.rs`.)

- The age-based purge runs **hourly** (fixed cadence; `CORTEX_CLEANUP_INTERVAL_SECS` controls the separate storage-budget loop). Cutoff uses `received_at` (server clock).
- `CORTEX_RETENTION_DAYS=0` disables the global age purge entirely.
- **err+ exemption**: `severity IN (err, crit, alert, emerg)` rows are never aged out by retention. They are deletable only under DB-size pressure, and even then only outside the err+ floor (`CORTEX_ERR_FLOOR_WINDOW_HOURS=24`, `CORTEX_ERR_FLOOR_PER_SOURCE_CAP=10000` rows per source IP). Permanent err+ retention is therefore only guaranteed while `max_db_size_mb` is not breached.
- **AdGuard tags** (`adguard-allowed`/`adguard-query`/`adguard-rewrite`) are hard-capped at **7 days** regardless of `retention_days`; **heartbeats** at **14 days**.
- **`llm_invocations`** (migration 37, the LLM-call audit table — see `src/db/llm_invocations.rs`) rides the same `CORTEX_RETENTION_DAYS` knob as `logs` (0 disables it too), purged by `started_at` age via `purge_old_llm_invocations`. No severity concept, so no err+-style exemption; no dedicated short cap like AdGuard/heartbeats since invocation volume is bounded by `LlmRunner`'s own per-minute/per-hour caps.
- Deletes run in 10,000-row chunks, releasing the write lock between chunks; an incremental FTS5 merge follows.

## Gotchas

- **Port 1514 not 514** — avoids needing root; use iptables PREROUTING to redirect 514→1514 for devices that can't be reconfigured (see docs/SETUP.md)
- **Cargo.lock is tracked** — binary crates should commit Cargo.lock for reproducible builds (Cargo docs guidance)
- **FTS5 query syntax** — `cortex action=search` uses SQLite FTS5: `error AND nginx`, `"disk full"`, `kern OR syslog`; invalid FTS5 syntax returns a db error. **Hyphen is the FTS5 NOT operator** — to search for hyphenated terms, use phrase syntax: `"smoke-test"` not `smoke-test`
- **WAL mode** — SQLite runs in WAL mode; copying `.db`, `.db-wal`, and `.db-shm` together without a checkpoint captures potentially inconsistent state. Safe backup options: (1) run `PRAGMA wal_checkpoint(FULL);` first, then copy all three files, or (2) use `sqlite3 source.db '.backup dest.db'` which is WAL-safe and requires no manual checkpoint
- **MCP transport** — HTTP MCP runs in stateless JSON-response mode on `POST /mcp`; SSE streams (`GET /mcp` or `/sse`) are not enabled in the current server.
- **Data volume** — DB lives in `./data/` (bind mount); `*.db` is gitignored so the database files won't be committed
- **Retention purge** — `retention_days` defaults to 90; logs older than 90 days are **permanently deleted hourly** with no recovery path, **except err/crit/alert/emerg rows, which are exempt from retention aging** (deletable only under disk pressure within the err+ floor bounds). Set `CORTEX_RETENTION_DAYS=0` to disable the global age purge (AdGuard 7-day and heartbeat 14-day caps still apply). See "Retention" above.
- **Storage guardrail** — The logical DB-size guard is enabled by default (`1024/900 MB`): a breach deletes oldest logs by `received_at` until the recovery target, sparing the err+ floor. The free-disk guard is **disabled by default** (`0/0`); when enabled, a breach **blocks writes** (with hysteresis) instead of deleting data. If cleanup cannot recover enough space, the batch writer blocks new writes until storage becomes healthy again.
- **CEF hostname vs source_ip** — For UniFi CEF messages, the stored `hostname` comes from the CEF `UNIFIdeviceName` extension field (message body), **not** the syslog header. Any LAN device can spoof this value. `source_ip` is the only network-verified identity. See `src/receiver/parser.rs` for the trust boundary.
- **Batch writer failure** — If `insert_logs_batch` fails, the batch is retained for the next flush (up to 1000 entries, then discarded). A 250ms pause prevents hammering a failing DB. Persistent write failures will eventually cause data loss via the 10K-entry channel cap. The mpsc channel is in-memory only — no durable write-ahead log.
- **correlate action limit cap** — The `limit` parameter is silently capped at 999 (not 1000) because the implementation fetches `limit+1` rows to detect truncation, and `search` hard-caps at 1000.
- **Auth / trust model** — MCP endpoint is unauthenticated by default; any client reaching port 3100 has full log read access. Set `CORTEX_TOKEN` to require Bearer auth. CORS is restricted to `localhost:3100` (browser-only; curl/mcporter unaffected). If exposing via SWAG/reverse proxy, add auth at the proxy layer or set the token. See README Security section for details.
- **FTS5 phantom rows** — When logs are deleted by retention purge or storage enforcement, their FTS5 index entries persist as phantom rows in `logs_fts` until the next merge cycle. The MCP query path is unaffected (the JOIN to `logs` prunes phantoms at query time), but direct SQLite access to `logs_fts` reveals porter-stemmed tokens for deleted messages. For right-to-erasure compliance (GDPR/HIPAA), use `INSERT INTO logs_fts(logs_fts) VALUES('rebuild')` after deletion instead of the periodic incremental merge. Monitor phantom row count via `stats` action → `phantom_fts_rows`.
- **OAuth refresh token TTL** — Refresh tokens default to 8h (`refresh_token_ttl_secs = 28800`). lab-auth's default is 30 days; cortex deliberately uses 8h for the read-only homelab profile. Adjustable via `[mcp.auth].refresh_token_ttl_secs` in config.toml.
- **Stdio mode always uses LoopbackDev** — `cargo run -- mcp` (stdio query-only) always uses `AuthPolicy::LoopbackDev` regardless of config. No auth is enforced. This is intentional: the local process boundary is the trust boundary for stdio clients.
- **Docker bind-mount ownership** — `auth.db` and `auth-jwt.pem` are written by the container UID. Host-side backup scripts or file managers may need `sudo` or a sidecar copy step to read them without permission errors.

## Testing MCP Tools

```bash
# Full smoke test (requires server running)
bash scripts/smoke-test.sh

# WAL-safe backup, then destructive DB reset (service should be stopped first)
bash scripts/reset-db.sh

# Using mcporter (project config at config/mcporter.json)
mcporter list cortex --config config/mcporter.json
mcporter call --config config/mcporter.json cortex.cortex action=stats
mcporter call --config config/mcporter.json cortex.cortex action=tail n=10
mcporter call --config config/mcporter.json cortex.cortex action=search query=error limit=5

# Health check
curl http://localhost:3100/health

# Tail recent logs (raw JSON-RPC)
curl -s -X POST http://localhost:3100/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"cortex","arguments":{"action":"tail","n":10}}}'

# Search
curl -s -X POST http://localhost:3100/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"cortex","arguments":{"action":"search","query":"error","limit":5}}}'

# Stats
curl -s -X POST http://localhost:3100/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"cortex","arguments":{"action":"stats"}}}'

# stdio mode (query-only, no ingest — useful for Claude Desktop)
cargo run -- mcp
```

`cortex compose` commands resolve the live Compose owner before mutation. They refuse ambiguous cwd fallback, stale Compose labels, listener conflicts, and destructive `down` without `--yes`.


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


## Version Bumping

**Every feature branch push MUST bump the version in ALL version-bearing files.**

Versioning is managed by `cargo xtask`, a declarative port of axon's release-
version system. `release/components.toml` is the single source of truth for the
version-bearing files and how each one is read/rewritten — add a row there to
track a new carrier, no code change needed. `Cargo.toml` `[package]` is the
canonical version; every other file must agree with it.

```bash
cargo xtask bump-version patch|minor|major   # bump every version-bearing file at once
cargo xtask check-version-sync               # all files agree (CI gate)
cargo xtask check-release-versions           # sync + CHANGELOG entry (release gate)
```

Bump type is determined by the commit message prefix:
- `feat!:` or `BREAKING CHANGE` → **major** (X+1.0.0)
- `feat` or `feat(...)` → **minor** (X.Y+1.0)
- Everything else (`fix`, `chore`, `refactor`, `test`, `docs`, etc.) → **patch** (X.Y.Z+1)

**Version-bearing files (declared in `release/components.toml`):**
- `Cargo.toml` — `version = "X.Y.Z"` in `[package]` (canonical source)
- `Cargo.lock` — the `cortex` package entry
- `server.json` — MCP Registry `"version"` plus the `cortex:vX.Y.Z` image tag
- `mcpb/manifest.json` — MCP Bundle `"version"`
- `docker-compose.prod.yml` — `${CORTEX_VERSION:-X.Y.Z}` default image tag
- `CHANGELOG.md` — new entry under the bumped version

Claude/Codex/Gemini plugin manifests are intentionally unversioned;
`release/components.toml` lists `.claude-plugin/plugin.json` with the
`json_no_version` kind, so `check-version-sync` rejects any top-level
`version` key. To version-track a new file (e.g. `package.json`), add a row to
`release/components.toml`.

All files MUST have the same version. Never bump only one file.
CHANGELOG.md must have an entry for every version bump.

## Plugin setup hooks

Plugin setup is owned by the binary. Keep `scripts/plugin-setup.sh` as a thin adapter that maps `CLAUDE_PLUGIN_OPTION_*` values to environment variables, prepares appdata, ensures `cortex` is on `PATH`, and then calls `cortex setup plugin-hook "$@"`.

`cortex setup check` is read-only, `cortex setup repair` is idempotent, and `cortex setup plugin-hook --no-repair` is audit mode. Do not add Docker Compose, systemd, or service bootstrap logic back into the hook script.
