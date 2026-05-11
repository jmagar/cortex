# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.18.0] - 2026-05-11

### Added

- **AI Session Tracking**: Added dedicated columns (`ai_tool`, `ai_project`,
  `ai_session_id`, `ai_transcript_path`) and aggregation logic to track AI
  sessions by project across transcripts and OTel telemetry.
- **Sessions MCP Action**: New `sessions` action for the `syslog` tool to list
  and filter AI sessions grouped by project, tool, and host.
- **OTel AI Metadata Extraction**: Automatic extraction of session and project
  metadata from OpenTelemetry log and resource attributes.

### Fixed

- **Config testing**: Fixed a flaky Docker ingest config test by ensuring
  environment variable isolation during test runs.

## [0.17.7] - 2026-05-09

### Fixed

- **Docker syslog port mapping**: Compose now maps the container-side syslog
  port to `SYSLOG_PORT`, keeping Docker publishes aligned with the server bind
  port and avoiding silent ingest breaks when the bind port is customized.
- **Security audit gate**: documented the temporary `rsa` RustSec exception for
  `lab-auth` RS256/JWK support while preserving cargo-audit enforcement for all
  other advisories.
- **Review hardening**: added cross-references for MCP read scope drift and
  asserted that unsafe OAuth-only OTLP startup rejection happens before DB
  initialization side effects.

## [0.17.6] - 2026-05-09

### Fixed

- **OAuth-only OTLP exposure**: non-loopback OAuth deployments now refuse to
  start without `SYSLOG_MCP_TOKEN`, because OTLP `/v1/logs` currently supports
  only static Bearer-token auth.
- **MCP OAuth scopes**: all current public read-only MCP actions now require
  `syslog:read`, with exhaustive mounted-auth coverage to prevent action/scope
  drift.
- **MCP resource auth**: mounted-auth deployments now require an auth context
  before listing or reading MCP resources, matching the tools surface.
- **Plugin deployment config**: setup writes the primary `SYSLOG_MCP_TOKEN`,
  permits tokenless OAuth only for loopback server mode, validates configured
  ports early, migrates legacy `SYSLOG_MCP_API_TOKEN`, exposes Docker host
  syslog port mapping separately from the in-container bind port, and validates
  the current plugin marketplace layout.
- **Docker and docs drift**: Docker metadata and Compose privileged-port
  guidance now match the actual `1514` container listener, and MCP docs list
  the current action surface without stale `/sse` guidance.

## [0.17.5] - 2026-05-08

### Fixed

- **MCP status scope mapping**: `syslog status` now requires `syslog:read`
  like the other read-only actions, instead of falling through to the
  fail-closed unknown-action scope sentinel.

## [0.17.4] - 2026-05-08

### Fixed

- **Plugin Docker deploy source detection**: installed plugin caches may include
  source files, so Docker setup now pulls the published GHCR image by default
  and only builds locally when `CLAUDE_PLUGIN_OPTION_BUILD_LOCAL=true` is set.

## [0.17.3] - 2026-05-08

### Fixed

- **Plugin setup OAuth persistence**: generated server env now uses the single
  canonical `.env`, preserves OAuth configuration, supports explicit `NO_AUTH`,
  and no longer requires a static API token when `auth_mode=oauth`.
- **Docker plugin redeploys**: source checkouts build the local image instead of
  pulling stale `latest`, and compose can take over an existing named
  `syslog-mcp` container during cutover.
- **Codex OAuth discovery**: OAuth metadata is now also available under
  `/mcp/.well-known/*` so Codex can discover authorization and protected
  resource metadata from path-based MCP endpoints.

## [0.17.2] - 2026-05-08

### Fixed

- **Plugin setup pre-flight checks**: `setup_docker` now fails fast if the
  Docker daemon is unreachable, a required port is already in use, or the data
  directory is not writable; warns on low disk space; validates compose config
  before touching the running container; auto-creates the external Docker
  network if missing.
- **Plugin setup systemd parity**: `setup_systemd` gains the same pre-flight
  checks — binary existence, port conflict detection (skipped when service is
  already running), data-dir write test, and low-disk warning.
- **Systemd unit fully removed on docker cutover**: `setup_docker` now stops,
  disables, and deletes the unit file so systemd cannot restart it on boot;
  `restart: unless-stopped` in the compose file owns the lifecycle instead.

## [0.17.1] - 2026-05-08

### Fixed

- **Integration test support**: Enabled the crate's `test-support` feature for
  integration tests so `cargo test` can compile `syslog_mcp::testing` helpers
  without requiring callers to pass `--features test-support`.

## [0.17.0] - 2026-05-07

### Added

- **Integration tests for auth modes** (`tests/auth_modes.rs`) — 12 tests covering discovery
  endpoints (200/404 by policy), `/register` and `/auth/login` 404 in all modes, `/health`
  unauthenticated in all modes, `/mcp` credential enforcement, and `tools/list` scope gate.
- **JWT-level OAuth flow tests** (`tests/oauth_flow.rs`) — 6 tests: valid JWT with
  `syslog:read`/`syslog:admin` succeeds, expired JWT rejected (401), wrong-issuer JWT rejected
  (401), empty-scope JWT denied at MCP layer (200 + JSON-RPC error), `tools/list` with JWT.
- **`syslog_mcp::testing` module** — public test-support helpers (`loopback_state`,
  `bearer_state`, `oauth_state`, `oauth_state_with_auth_state`) for building `AppState`
  variants in integration tests without `pub(crate)` access.
- **`docs/OAUTH.md`** — full OAuth setup guide: architecture diagram, Google Console setup,
  env var + TOML reference, gotchas, operator FAQ (revoke user, rotate JWT key).
- **OAuth section in `README.md`** — brief two-mode summary with link to docs/OAUTH.md.
- **OAuth subsection in `docs/SETUP.md`** — pointer to docs/OAUTH.md.
- **OAuth env vars in `CLAUDE.md` config section** and three new gotchas (refresh token TTL,
  stdio LoopbackDev policy, Docker bind-mount ownership).
- **OAuth env vars in `.env.example`** — all four OAuth vars commented with guidance.
- **OAuth discovery checks in `scripts/smoke-test.sh`** — unconditional check of
  `/.well-known/oauth-authorization-server` and `/jwks`; gracefully skips when 404.
- **`.github/workflows/lab-auth-bump.yml`** — weekly scheduled workflow to bump the
  `lab-auth` SHA via `cargo update -p lab-auth` (active once dep migrates from path to
  git+rev per the TODO in Cargo.toml).

### Notes

- RFC 9700 refresh-token rotation is tracked as known follow-up debt.
- `lab-auth` is currently a path dependency; the bump workflow is a no-op until it
  migrates to a `git+rev` reference.

## [0.16.0] - 2026-05-07

### Added

- **Fail-closed scope-based authorization on MCP tool dispatch** (syslog-mcp-brt0.8) — all
  `tools/call` and `tools/list` dispatches now enforce `AuthPolicy` at the entry point of
  `call_tool` and `list_tools`, before any DB query fires.
  - `AuthPolicy::LoopbackDev`: scope check bypassed entirely (loopback bind is the trust boundary).
  - `AuthPolicy::Mounted(_)`: `AuthContext` must be present in request extensions (injected by
    `lab-auth`'s `AuthLayer` middleware). Missing context → `-32600 forbidden` immediately.
  - Scope mapping: `search`, `tail`, `errors`, `hosts`, `correlate`, `stats` require `syslog:read`;
    `help` requires no scope (but still requires `AuthContext` when Mounted); unknown actions default
    to `syslog:read` (fail-conservative).
  - `tools/list` requires `AuthContext` when Mounted but no scope (MCP spec conformance: clients
    must be able to discover the tool before authenticating to call it, but only if they hold a
    valid credential).
  - Denied invocations logged at `warn` level with `subject` + `action` for audit trail.
  - Pattern: `AuthContext` read from `ctx.extensions.get::<axum::http::request::Parts>()?.extensions.get::<AuthContext>()`
    (Pattern (a) locked by spike syslog-mcp-brt0.10; no AppState map, no task-local needed).
  - 9 new unit tests covering all branches: LoopbackDev permit, Mounted+read scope, Mounted+admin
    scope (syslog:admin is treated as a superset of syslog:read and satisfies read requirements),
    Mounted+both scopes, empty scopes+read (denied), empty scopes+help (permitted), missing
    AuthContext fail-closed, tools/list with AuthContext, and scope-check-before-DB verification.

## [0.15.1] - 2026-05-07

### Fixed

- **Timestamp normalization**: All time-window query bounds (`correlate`, `context`, `compare`, `anomalies`) and stored `timestamp` from the syslog and Docker parsers now produce the canonical `Z`-suffixed RFC3339 form (`rfc3339_z` helper, lifted to `app::time`). Previously, mixed `+00:00`/`Z` forms could silently drop boundary rows under SQLite TEXT comparison.
- **`compare` panic**: Replaced four `.expect("required field")` calls on parsed user input with `parse_required_timestamp`, returning a clean `InvalidInput` error instead of panicking the request thread.
- **`tail` placeholder index**: Severity-IN block in `tail_logs` now advances the placeholder index, preventing latent `?N` collisions if future filters are appended after it.
- **`anomalies` ranking**: Hosts active in the recent window with no baseline (`recent_count > 0 && baseline_count == 0`) now sort to the top of the response, matching the docstring's promise to flag new-but-active hosts.
- **`source_ips` dispatch**: `tool_list_source_ips` now accepts `(state, args)` for parity with the other action handlers, so future filters won't silently swallow client args.
- **App response boundary**: New action responses now use app-layer DTOs instead of exporting database model types directly.
- **Action documentation**: Added the new `search`, `tail`, and `errors` parameters to MCP docs and expanded the syslog skill reference for every new action.
- **Test comments/helpers**: Clarified smoke/live script action inventories and removed duplicated source-IP test fixture setup.

### Changed

- **`normalize_template`** (`db::patterns` helper) is no longer re-exported at the crate root; it is `pub(super)` and only reachable from inside the `analytics` module.
- **Test assertion tightened**: `context` neighbor bounds are now strict (`<` / `>`) for the id-anchored case, matching the documented contract.

## [0.15.0] - 2026-05-07

### Added

- **OAuth router mount** — when `AuthPolicy::Mounted { auth_state: Some(_) }` (OAuth mode),
  the `lab_auth::routes::bearer_only_router` is merged onto the main axum router, exposing
  `GET /.well-known/oauth-authorization-server`, `GET /.well-known/oauth-protected-resource`,
  `GET /jwks`, `GET /authorize`, `GET /auth/google/callback`, and `POST /token`.
  Not mounted in bearer-only or LoopbackDev modes. `/register` and `/auth/login` excluded
  per Locked Decision.
- **`SYSLOG_MCP_PUBLIC_URL` in host/origin allowlists** — `allowed_hosts()` and
  `allowed_origins()` now derive the host and origin from `SYSLOG_MCP_PUBLIC_URL` (set
  automatically when OAuth mode is active), so the OAuth callback origin is accepted by
  rmcp's DNS-rebinding guard and the tower-http CORS layer.
- **Eleven new `syslog` actions** for log intelligence beyond raw search/tail:
  - `apps` — distinct application names with log/host counts and first/last seen (mirror of `hosts` for the `app_name` dimension).
  - `source_ips` — distinct source identifiers with hostname breakdown; supports spoof detection on hostname-spoofable formats (e.g. UniFi CEF).
  - `timeline` — bucketed counts (`minute`/`hour`/`day`) over a time range, optionally split by `hostname` / `severity` / `app_name`.
  - `patterns` — cluster near-duplicate messages by template (numbers, IPv4, UUIDs, long hex strings normalised to placeholders); returns top templates with counts, sample, and host distribution.
  - `context` — surrounding logs around a single point of interest by `log_id` or `hostname`+`timestamp`.
  - `get` — fetch one log by `id`, including the unparsed `raw` syslog frame.
  - `ingest_rate` — recent throughput (last 1m / 5m / 15m using `received_at`) plus current `write_blocked` flag and optional per-host buckets.
  - `silent_hosts` — hosts whose `last_seen` is older than `silent_minutes` ago, with their typical inter-arrival interval.
  - `clock_skew` — per-host distribution of `received_at - timestamp` (seconds), sorted by absolute mean.
  - `anomalies` — per-host comparison of recent volume/error count against a baseline window; returns ratio and Poisson-style z-score.
  - `compare` — side-by-side summary of two time ranges (volume, error count, severity mix, top hosts/apps) with deltas.
- **New filters on existing actions**:
  - `search` accepts `facility` and `process_id`.
  - `tail` accepts `severity_min` (returns entries at or above the threshold).
  - `errors` accepts `group_by=app_name` for hostname x app_name x severity grouping.
- **`logs.raw` column is now exposed** via the new `get` action.

### Changed

- **Replace bearer middleware with `lab_auth::AuthLayer`** — deleted the
  duplicated `require_auth` / `bearer_token` / `token_matches` helpers from
  both `src/mcp/routes.rs` and `src/api.rs`. Both surfaces now apply
  `lab_auth::AuthLayer` (bearer-only, `allow_session_cookie=false`) when
  `AuthPolicy::Mounted` is active; `LoopbackDev` skips the layer entirely.
  Static-token path is identical for existing users; JWT path activates when
  `AuthState` is `Some` (OAuth mode).
- `src/auth.rs` deleted; `src/otlp.rs` migrated to `lab_auth::middleware`
  equivalents (`parse_bearer_token`, `tokens_equal`).
- `subtle` removed as a direct dependency (still transitive via `lab-auth →
  jsonwebtoken`).
- `ApiState` gains `auth_policy: AuthPolicy` field; `main.rs` passes
  `runtime.auth_policy().clone()` when constructing `ApiState`.
- `tail_logs` query and `get_error_summary` query gained additional parameters (`severity_in`, `group_by_app`); internal callers updated.
- Help text (`syslog help`) expanded to cover all 19 actions and updated parameters.

## [0.14.2] - 2026-05-07

### Added

- Added bounded TCP syslog frame handling with oversized-frame regression coverage.
- Added fail-fast validation for zero-valued syslog ingest settings.
- Added Docker ingest supervisor policy/backoff tests, sidecar supervisor tests,
  and Docker ingest producer observability in status/stats.
- Added TCP ingest smoke coverage and a tracked ingestion full-review artifact.

### Changed

- Hardened Docker ingest reconnect backoff with deterministic jitter and durable
  stream-duration reset semantics.
- Improved failed batch handling so retryable storage failures retain bounded
  rows, while permanent failures retry chunks and isolate bad rows.
- Capped ingest summary cardinality with overflow buckets while preserving total
  log counts.
- Documented Docker ingest trust boundaries and heavy SQLite migration runbooks.

## [0.14.1] - 2026-05-07

### Fixed

- Tightened MCP numeric argument validation so present wrong-type values for
  `n`, `limit`, and `window_minutes` now return invalid params instead of
  silently falling back to defaults.
- Validated `search.severity` against the shared syslog severity list across
  MCP, REST, and CLI callers.
- Added `syslog status` to live smoke coverage, mcporter coverage, plugin skill
  docs, and active MCP documentation.
- Added a cross-surface action registry test to keep schema, dispatch, help,
  smoke scripts, and action docs aligned.

## [0.14.0] - 2026-05-07

### Added

- **Three new slash commands** for routine server management:
  - `/syslog:redeploy` — runs `plugin-setup.sh` directly so config
    changes apply without waiting for `SessionStart` or `ConfigChange`
  - `/syslog:logs [N|--follow]` — mode-aware tail/follow of service
    logs (`docker compose logs` or `journalctl --user`)
  - `/syslog:cutover docker|systemd` — one-shot deploy-mode switch
    with health verification and rollback guidance
- **`syslog-troubleshoot` skill** (`plugins/skills/syslog-troubleshoot/`)
  — auto-triggers when the user reports MCP connection failures,
  missing logs from specific hosts, service crashes, or vague
  "syslog isn't working" reports. Walks a decision tree (MCP /
  ingest / service / unknown) instead of running every diagnostic;
  uses runtime observability counters from v0.13.0 to localize
  ingest-path vs writer-path failures.
- Updated `CLAUDE.md` and `docs/plugin/COMMANDS.md` command tables
  to register the three new commands.

## [0.13.0] - 2026-05-07

### Added

- **OAuth config schema (`[mcp.auth]`)** — new TOML section + 5 env vars
  (`SYSLOG_MCP_AUTH_MODE`, `SYSLOG_MCP_PUBLIC_URL`, `SYSLOG_MCP_GOOGLE_CLIENT_ID`,
  `SYSLOG_MCP_GOOGLE_CLIENT_SECRET`, plus the existing `SYSLOG_MCP_TOKEN`)
  wiring the dual-mode bearer/OAuth policy that the upcoming runtime
  integration (S2/S3/S4) will consume. All policy knobs (TTLs, rate limits,
  paths, allowlists) live in `config.toml`; only secrets, URLs, and the mode
  toggle flow through env vars per the OAuth epic's locked decisions.
- **Non-loopback safety gate** — `Config::load()` now refuses to start when
  `mcp.host` is bound to a non-loopback address with no static token AND no
  OAuth configured. Loopback detection uses `IpAddr::is_loopback()`
  (covering `127.0.0.0/8` and `::1`); strings that fail to parse as IP are
  treated as non-loopback. Loopback + no-auth remains permitted for
  developer convenience.
- **OAuth allowlist enforcement** — when `mode == oauth`, startup fails
  unless `[mcp.auth].admin_email` or `[mcp.auth].allowed_emails` is
  non-empty, preventing the "any Google account that completes OAuth gets
  in" footgun.
- New `lab-auth` dependency (path-dep against the L1 SHA pin in the
  development worktree; will be swapped to `git+rev` before merge per the
  S6 bead).
- **Direct CLI commands** (`src/cli.rs`, `docs/CLI.md`): `syslog search /
  tail / errors / hosts / correlate / stats` queries now run directly
  against the SQLite database without starting the MCP server, syslog
  listeners, REST API, retention, or Docker ingest. Reuses the same
  `SyslogService` methods as the MCP tool, with a `--json` output flag
  for shell-script consumption.
- **Runtime observability counters** (`src/observability.rs`): atomic
  counters for syslog UDP/TCP packets and bytes, ingest queue depth,
  writer batches and flush failures, plus last-ingest/write/error
  timestamps. Surfaced via the existing `/health` endpoint and `stats`
  MCP action.

### Changed

- **`rusqlite` 0.32 → 0.39 / `r2d2_sqlite` 0.25 → 0.33** — required so
  `lab-auth` (which uses rusqlite 0.39) can coexist with syslog-mcp under
  the `links = "sqlite3"` constraint. No source changes needed at the
  syslog-mcp callsites; the bumps are API-compatible for the patterns we
  use.
- **Plugin docker deploy now pulls the published image** instead of
  building from source. `docker-compose.yml` adds
  `image: ghcr.io/jmagar/syslog-mcp:${SYSLOG_MCP_VERSION:-latest}`
  alongside the existing `build:`. `setup_docker()` runs
  `docker compose pull` then `up -d --no-build`, so plugin installs
  never require the Dockerfile or source code that the plugin doesn't
  ship. Source-repo development paths still work unchanged via
  `docker compose build` / `up --build`.
- **`/config:ro` volume removed** from the compose file. It was
  vestigial — runtime config flows through env vars, and the missing
  `${COMPOSE_DIR}/config` directory was the literal cause of failed
  plugin deploys. The TOML alternative (`SYSLOG_MCP_DOCKER_HOSTS_FILE`)
  is still supported via direct path env var if needed.
- **Plugin hook resilience**:
  - `ConfigChange` event added to `plugins/hooks/hooks.json` (matcher
    `user_settings`) so editing `/plugin` re-runs deployment without a
    session restart.
  - 600 s timeout set on both `SessionStart` and `ConfigChange` to
    cover first-time docker pulls or builds.
  - `setup_docker()` stops a running systemd unit before bringing the
    container up; `setup_systemd()` does `docker compose down`
    symmetrically. Cutovers between deploy modes no longer port-conflict.
  - `SYSLOG_UID` and `SYSLOG_GID` written to the env file in docker
    mode so the container writes `syslog.db` with host-user ownership;
    same file remains readable by the systemd binary if you switch
    modes back.
- **`max_db_size_mb` default raised from 1024 → 8192** (1 GB → 8 GB)
  in both `plugin.json` and `plugin-setup.sh` fallback. The 1 GB
  default was too aggressive for fleets ingesting Docker stdout from
  multiple hosts.

### Fixed

- **Empty `server_url` no longer breaks MCP client connection** —
  documented in plugin.json description that an empty value cannot be
  used (substitution is literal text replacement, not a shell
  expansion).

## [0.12.0] - 2026-05-07

### Added

- **MCP resource: `syslog://schema/mcp-tool`** — the `syslog` tool's full input
  schema (all actions, parameters, enums, descriptions) is now exposed as an
  MCP resource. Agents that negotiate the handshake see `resources: {}` in the
  server capabilities and can fetch the schema via `resources/list` and
  `resources/read` without invoking `tools/list`. Useful for clients that want
  to introspect the tool surface as a discoverable, cached document.

## [0.11.1] - 2026-05-07

### Changed

- **Plugin user config (`/.claude-plugin/plugin.json`)**: rewrote every field's
  description to be self-explanatory — naming the consequence of each setting,
  the mode it applies in, and the recommended fix when defaults aren't right.
  Marked `use_docker`, `server_url`, `docker_ingest_enabled`, and `fleet_hosts`
  as `required: true` so first-run users see them in the TUI flow.
- **Plugin command rename**: `/syslog:doctor` → `/syslog:dr` to avoid colliding
  with Claude Code's built-in `/doctor`. Command file moved
  `plugins/commands/doctor.md` → `plugins/commands/dr.md`. Doc references
  updated in `README.md`, `CLAUDE.md`, `docs/plugin/CLAUDE.md`,
  `docs/plugin/COMMANDS.md`, and `plugins/commands/deploy-dropins.md`.
  Historical session notes left intact.
- **`/syslog:dr` scope expanded** — now doubles as a first-run preflight in
  addition to ongoing health check. Added pre-deployment checks: environment
  prerequisites (kernel/virt info, systemd vs docker availability per mode),
  storage & permissions on `data_dir` (existence, writability, free space ≥
  120% of `max_db_size_mb`), binary symlink + PATH validation, API token
  quality (empty / weak placeholder / length warning, never echoes the value),
  and port availability (free or held by our PID, fail otherwise). Existing
  health checks (MCP, HTTP, service state, listener reachability, fleet
  drop-ins) preserved. Result table now uses PASS / WARN / FAIL with a
  one-line verdict and concrete next-step fixes per failure.

## [0.11.0] - 2026-05-07

### Added

- **OTLP HTTP receiver** (`src/otlp.rs`): `POST /v1/logs` decodes
  `ExportLogsServiceRequest` protobuf and feeds records through the existing
  ingest pipeline. `POST /v1/metrics` returns 200 and discards. `POST /v1/traces`
  returns 404 — span flattening was deferred (FTS5 cannot meaningfully query
  hex trace IDs). New deps: `opentelemetry-proto = 0.31` (logs feature only,
  no full gRPC) and `prost = 0.14`.
- **OTLP receiver hardening**: `RequestBodyLimitLayer` of 4 MiB on `/v1/*`
  with automatic `Retry-After: 86400` header on 413 to prevent OTel exporter
  retry storms. Optional Bearer auth via the existing `SYSLOG_MCP_API_TOKEN`.
  Peer IP captured via `ConnectInfo<SocketAddr>` and stored as `source_ip`
  to mirror the syslog provenance model — OTLP `host.name` is client-asserted
  and untrusted.
- **`/health` enrichment**: response now includes `otlp_logs_received` and
  `otlp_decode_errors` counters so operators can see ingest activity at a
  glance.
- **Pre-insert enrichment** (`src/syslog/enrichment.rs`): Authelia
  `level=` parsing maps `info/warn/error/fatal` to syslog severities;
  AdGuard JSON query log gets reclassified to `adguard-blocked` /
  `adguard-allowed` / `adguard-rewrite`. Source-IP gating via
  `SYSLOG_MCP_AUTHELIA_SOURCE_IP` and `SYSLOG_MCP_ADGUARD_SOURCE_IP`
  prevents other tailnet hosts from spoofing the classification.
- **Best-effort secret scrubbing** for AI-source records (Claude/Codex
  transcripts and OTLP records carrying `service.name=claude-code|codex`).
  Eight pattern classes plus the `SYSLOG_MCP_API_TOKEN` literal value.
  Toggle with `SYSLOG_MCP_SCRUB_PROMPTS` (default `true`). Documented as
  defense-in-depth, **not** a compliance control — regex has structural
  bypass classes.
- **Tag-based retention** (`db::purge_by_tag_window`): adguard-allowed,
  adguard-query, and adguard-rewrite are purged at 7 days regardless of
  the global `retention_days`. High-severity rows are exempt from time-based
  purge in both `purge_by_tag_window` and `purge_old_logs`.
- **Configurable FTS merge** via `SYSLOG_MCP_FTS_MERGE_PAGES` (default 0
  = unconditional merge after every purge cycle).
- **Deploy artifacts** (`deploy/`): rsyslog drop-ins for imjournal +
  AI transcripts + squirts specialty sources, OTel client config examples
  for Claude Code and Codex, and a step-by-step manual SSH deploy runbook.

### Changed

- **Migration v3**: composite index `idx_logs_app_name_received_at ON
  logs(app_name, received_at)` added to make tag-based retention
  O(rows-deleted) instead of O(rows-in-tag-partition × chunks). On a
  multi-million-row DB the first-run `CREATE INDEX` may hold the write
  lock for several minutes; `/health` will not respond and syslog UDP may
  drop during that window. Plan a brief health-check gap when upgrading.
- **`enforce_storage_budget` instrumentation**: emits `tracing::warn!`
  when deleting rows with `severity IN ('err','crit','alert','emerg')` so
  operators are not surprised when disk pressure overrides the time-based
  retention exemption.
- **Maintenance task ordering**: tag-window purges now run *before* global
  retention purge to avoid SQLite write-lock contention from concurrent
  chunked DELETEs.
- **`IngestTx::try_send`**: new non-blocking send used by the OTLP handler
  so HTTP requests return 503 on backpressure instead of awaiting and
  holding the connection open.

### Notes

- **Phases 3–5 are deploy-only.** The Rust binary is complete; deploying
  rsyslog drop-ins and OTel client configs to dookie / squirts /
  steamy-wsl / vivobook-wsl requires manual SSH per `deploy/README.md`.

## [0.10.2] - 2026-05-07

### Fixed

- **Docker hosts file**: Missing `SYSLOG_DOCKER_HOSTS_FILE` no longer crashes the container at startup. Logs a warning and continues with no hosts loaded. Other read errors (permissions, I/O) still hard-fail.

### Changed

- **Plugin restructure**: Moved plugin manifests under `plugins/`. Removed top-level `Dockerfile`, `entrypoint.sh`, `gemini-extension.json`, `.codex-plugin/`, `.mcp.json`, and obsolete `skills/syslog/`. Added `config/Dockerfile` and `scripts/plugin-setup.sh`.
- **Docs**: Clarified `SYSLOG_DOCKER_HOSTS` (simple) vs `SYSLOG_DOCKER_HOSTS_FILE` (advanced) in README and `.env.example`. Documented graceful behavior when the hosts file is missing.

## [0.10.1] - 2026-05-06

### Changed

- **CLAUDE.md**: Updated architecture overview for current module layout (`app/`, `db/`, `syslog/`, `mcp/`, `docker_ingest/`, `api.rs`, `runtime.rs`); updated MCP tool description to single `syslog` action-dispatch tool.
- **Scripts**: Updated `scripts/` path references in `backup.sh`, `bump-version.sh`, `reset-db.sh`, `smoke-test.sh`.
- **Docs**: Updated inventory, mcporter, pre-commit, tests, hooks, scripts, and deploy runbook to reflect current module/binary layout.
- **Local dev MCP config**: Switched `.mcp.json` from HTTP transport to stdio (`./bin/syslog mcp`) for local development.
- **Gitignore**: Added `config/docker-hosts.toml` (local-only Docker host config).
- **Docs**: Added `docs/expansion.md` — fleet topology and ingestion expansion planning doc.

## [0.10.0] - 2026-05-05

### Changed

- **Single MCP tool surface**: Collapsed the public MCP tool list to one `syslog` tool with action-based calls: `search`, `tail`, `errors`, `hosts`, `correlate`, `stats`, and `help`.
- **Schemas/tests/docs**: Updated RMCP HTTP, stdio, mcporter smoke coverage, and tool documentation for the new `action` contract.

## [0.9.0] - 2026-05-05

### Changed

- **Single `syslog` binary**: Collapsed the HTTP and stdio MCP transports behind one installed executable. `syslog serve mcp` starts the daemon with syslog ingest and HTTP MCP, while `syslog mcp` starts query-only MCP over stdio.
- **Packaging**: Plugin builds now install only `bin/syslog`; the separate `syslog-mcp-stdio` artifact and legacy `syslog-cli` binary target were removed.
- **Docs/tests**: Updated transport docs and stdio child-process tests for the new command shape.

## [0.8.0] - 2026-05-05

### Added

- **RMCP stdio transport**: Added `syslog-mcp-stdio`, a query-only MCP child-process binary that exposes the same seven read-only tools as HTTP without starting syslog listeners, HTTP routes, or cleanup tasks.
- **Stdio integration tests**: Added child-process RMCP coverage for tool listing, `get_stats`, and parameterized `search_logs` over stdio.
- **Packaging/docs**: Release/plugin builds now install both `syslog-mcp` and `syslog-mcp-stdio`; docs distinguish HTTP daemon mode, direct stdio query mode, and `mcp-remote` bridge mode.

## [0.7.0] - 2026-05-05

### Added

- **Docker socket-proxy ingest**: Added optional pull-based Docker container log ingestion through read-only docker-socket-proxy endpoints, including host reconnect loops, container start event handling, stdout/stderr parsing, and per-container checkpoints in SQLite.
- **Shared ingest writer**: Routed syslog listener input and Docker log input through one bounded batch writer so retention, storage guardrails, and write blocking remain centralized.
- **Configuration/docs**: Added Docker ingest config, env vars, setup guidance, Compose `/config` mount, and `.env.example` entries for remote Docker hosts.

## [0.6.1] - 2026-05-05

### Changed

- **Test sidecars**: Split `src/app/` unit tests into per-module sidecar files and moved `syslog-cli` parser tests into a bin-local sidecar directory so Cargo does not treat them as a standalone binary target.
- **Repository hygiene**: Ignore local `storage/` data and remove stale `.app.json` metadata from the committed tree.

## [0.6.0] - 2026-05-05

### Added

- **RMCP transport**: Production `/mcp` now uses RMCP Streamable HTTP in stateless JSON-response mode.
- **RMCP validation**: Added compatibility and route tests for JSON responses, Host validation, auth, header behavior, unsupported protocol versions, method handling, and all seven tools.
- **Reverse proxy config**: Added `SYSLOG_MCP_ALLOWED_HOSTS` and `SYSLOG_MCP_ALLOWED_ORIGINS` for RMCP Host/Origin validation behind public DNS names or browser clients.

### Changed

- **App module layout**: Split the shared syslog service from `src/app.rs` into focused `src/app/` modules and renamed `LogService` to `SyslogService` across runtime, MCP, API, tests, and docs.
- **Protocol path**: Removed the hand-rolled MCP protocol dispatch module; RMCP now owns MCP lifecycle, tool listing, and tool calls.
- **Transport contract**: Removed the legacy `/sse` discovery endpoint. Stateless RMCP supports `POST /mcp`; `GET /mcp` and `DELETE /mcp` return `405 Method Not Allowed`.
- **Manifests/docs**: Updated plugin and registry metadata to describe HTTP/RMCP behavior instead of direct stdio execution.

## [0.5.0] - 2026-05-04

### Added

- **Shared app layer**: Added `LogService` with typed request/response models for search, tail, errors, hosts, correlation, and stats. MCP, CLI, and API surfaces now call this shared layer instead of duplicating business rules.
- **CLI**: Added `syslog-cli` for direct JSON search/tail/errors/hosts/correlate/stats queries without requiring the MCP server to run.
- **Non-MCP API**: Added disabled-by-default `/api/*` JSON routes with separate `SYSLOG_API_TOKEN` bearer authentication.

### Changed

- **Runtime**: Moved reusable config/DB/service/syslog/maintenance construction into `RuntimeCore`; `main.rs` is now a thin server entrypoint.
- **Source identity**: Added first-class `source_ip` filters across the shared service, MCP schemas/help, CLI, and API.
- **MCP tools**: Refactored tool handlers into thin JSON adapters while preserving MCP response envelopes and existing behavior.

## [0.4.2] - 2026-05-04

### Changed
- **Module layout**: Split `src/db.rs`, `src/mcp.rs`, and `src/syslog.rs` into focused submodules under `src/db/`, `src/mcp/`, and `src/syslog/` while preserving facade modules for existing callers.
- **Docs/tooling**: Updated project guidance, MCP tool docs, and Justfile helpers for the new module/test layout.

## [0.4.1] - 2026-05-04

### Changed
- **Test layout**: Inline `#[cfg(test)] mod tests { ... }` blocks moved out of `src/{config,db,main,mcp,syslog}.rs` into sibling `*_tests.rs` files, included via `#[path = "..."] #[cfg(test)] mod tests;`. Pure refactor — same tests, smaller production source files. Reduces noise in `cargo doc`, makes the production code easier to scan, and lets the test files grow without bloating the module they test. No behavior change; 87 tests still pass.

## [0.4.0] - 2026-05-04

### Added
- **`src/syslog.rs`**: TCP connections now log the parsed syslog hostname on the first message via `"TCP syslog sender identified"`, and include `hostname` in the connection-close summary alongside `peer`/`close_reason`/`line_count`/`total_bytes` — makes it easier to correlate misbehaving senders by hostname rather than only by ephemeral source port.

### Changed
- **`lefthook.yml`**: `diff_check` now runs `git --no-pager diff --check --cached` so the hook never invokes `less` under lefthook's pseudo-TTY (caused indefinite hangs in non-interactive shells).

### Removed
- **`src/mcp.rs`**: dead `test_state()` helper.

## [0.3.5] - 2026-05-04

### Removed
- Stale plugin scaffolding: `assets/icon.png`, `assets/logo.svg`, `assets/screenshots/.gitkeep`, `hooks/CLAUDE.md`, `hooks/hooks.json` — leftover stubs no longer referenced by any manifest.

## [0.3.4] - 2026-04-15

### Changed
- Repository maintenance updates committed from the current working tree.
- Version-bearing manifests synchronized to 0.3.4.


## [0.3.3] — 2026-04-05

### Fixed

- **`tests/test_live.sh`**: Added `uid=1000,gid=1000` to the `--tmpfs /data` mount so the `syslog` container user (uid 1000) can write the SQLite database; previously the tmpfs was owned by root, causing `unable to open database file: /data/syslog.db` and CI health-check timeout
- **`.github/workflows/docker-publish.yml`**: Trivy scan now references `steps.meta.outputs.version` (e.g. `main`) instead of the bare `github.sha` (full 40-char SHA); the image is pushed with the branch/tag name, not the full commit SHA, so the old ref caused `MANIFEST_UNKNOWN` scan failures

## [0.3.2] — 2026-04-04

### Fixed

- **Version sync**: Aligned `.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`, and `gemini-extension.json` from 0.2.6 to 0.3.2 to match `Cargo.toml`

### Added

- **`tests/TEST_COVERAGE.md`**: Test coverage documentation
- **`tests/mcporter/`**: MCPorter test infrastructure
- **Full documentation structure**: Added plugin-lab template docs (README, CLAUDE.md, references, runbooks)

## [0.3.1] — 2026-04-04

### Fixed

- **`src/config.rs`**: `cleanup_chunk_size` upper bound replaced from `i64::MAX` with operational limit of `1_000_000`; values above this hold the SQLite write lock indefinitely. Error message now explains why the limit exists.

## [0.3.0] — 2026-04-04

### Added

- **`src/db.rs`**: `DbStats` now includes `phantom_fts_rows` — count of FTS5 index entries without a matching live log row (merge lag indicator, visible via `get_stats`)
- **`src/db.rs`**: `schema_migrations` table guards idempotent migrations; DROP TRIGGER migration now runs exactly once per database (version 1)
- **`src/db.rs`**: Composite index `idx_logs_hostname_received_at ON logs(hostname, received_at)` — makes `reconcile_hosts` MIN/MAX queries O(1) instead of O(rows_per_host)
- **`CLAUDE.md`**: FTS5 phantom row gotcha with GDPR/HIPAA compliance guidance

### Fixed

- **`src/db.rs`**: P1 — `delete_oldest_logs_chunk` rewritten with subquery DELETE; old dynamic IN-list exceeded SQLite's 1000-node expression depth limit at default `cleanup_chunk_size=2000`, silently failing every storage enforcement cycle
- **`src/db.rs`**: P1 — `fts_incremental_merge` now runs `ceil(deleted_rows/5000)` iterations capped at 20; escalates to forced `rebuild` after 3 consecutive failures
- **`src/db.rs`**: `reconcile_hosts` moved outside the enforcement delete loop — batches all host updates to one call per enforcement cycle instead of one per chunk
- **`src/syslog.rs`**: TCP rejection `warn!` is now rate-limited (once on first rejection, once per 10s thereafter with `total_rejected` count) to prevent log storms under connection floods

### Changed

- **`src/config.rs`**: `StorageConfig::for_test` demoted to `pub(crate)`
- **`src/mcp.rs`**: `TestHarness` struct wraps `AppState + TempDir` in tests to prevent accidental early TempDir drop; all 10 test functions updated
- **`src/mcp.rs`**: `test_storage_config` wrapper removed; callers use `StorageConfig::for_test` directly

## [0.2.6] — 2026-04-04

### Changed

- **`src/db.rs`**: Extracted `fts_incremental_merge()` helper — eliminates duplicated FTS merge string across `purge_old_logs` and `enforce_storage_budget`
- **`src/mcp.rs`**: `test_state()` now delegates to `test_state_with_token(None)`; `mcp_post()` gains optional `auth` param — auth integration tests no longer inline the request builder

### Fixed

- **`src/config.rs`**: Added `accepts_cleanup_chunk_size_at_i64_max` boundary test; tightened overflow test to assert error message; added `SYSLOG_MCP_CLEANUP_CHUNK_SIZE` to `defaults_are_applied_without_env_vars` clear list and assertion
- **`src/db.rs`**: Migrated `test_storage_config()` to `StorageConfig::for_test()`
- **`src/syslog.rs`**: `TryAcquireError::Closed` branch now logs at `error!` before breaking
- **`CHANGELOG.md`**: Corrected v0.2.2 date (`2026-04-04` → `2026-04-03`)

## [0.2.5] — 2026-04-03

### Added

- **`src/mcp.rs`**: 9 HTTP-level integration tests for all 6 MCP tools and auth middleware using `tower::util::ServiceExt::oneshot` — covers health endpoint, initialize, tools/list, get_stats, tail_logs, search_logs, unknown method error, auth rejection (missing token), and auth success (correct token)
- **`Cargo.toml`**: `tower` 0.5 added to dev-dependencies for axum router integration testing

## [0.2.4] — 2026-04-03

### Fixed

- **`src/db.rs`**: FTS5 write-lock contention during retention purge and storage-budget bulk deletes — removed `logs_ad` (AFTER DELETE) and `logs_au` (AFTER UPDATE) triggers that fired per-row inside 10k-chunk transactions, starving the batch writer. Added migration to drop triggers from existing databases. FTS5 phantom rows are cleaned up by incremental merge (syslog-mcp-eg5)

### Added

- **`src/db.rs`**: Incremental FTS merge (`merge=500,250`) after storage-budget enforcement bulk deletes, matching the existing `purge_old_logs` pattern

## [0.2.3] — 2026-04-03

### Fixed

- **`src/syslog.rs`**: TCP accept loop blocked when connection semaphore was at capacity — replaced blocking `acquire_owned().await` with non-blocking `try_acquire_owned()` so the accept loop rejects new connections immediately instead of stalling for up to 300s (idle timeout)

## [0.2.2] — 2026-04-03

### Fixed

- **`src/mcp.rs`**: `summarize_json_value` panicked on multi-byte UTF-8 input (non-ASCII syslog messages) — replaced `&raw[..limit]` with a char-boundary-aware walk-back loop; added test covering Greek/CJK input
- **`src/db.rs`**: Storage enforcement deleted 1 row per cycle (extremely slow for large overages) — now configurable via `SYSLOG_MCP_CLEANUP_CHUNK_SIZE` (default 2000); WAL checkpoint moved outside the recovery loop
- **`src/config.rs`**: Added validation rejecting `cleanup_chunk_size == 0` (would cause an infinite enforcement loop)
- **Clippy**: Fixed 4 `-D warnings` errors blocking `cargo test` — `derivable_impls` on `Config::Default`, `match_like_matches_macro` in `is_transient_sqlite_lock`, `needless_late_init` for `close_reason`, `len_zero` in `batch_writer`

### Added

- **`src/config.rs`**: `cleanup_chunk_size` field in `StorageConfig` with env var `SYSLOG_MCP_CLEANUP_CHUNK_SIZE` (default 2000 rows per enforcement chunk)
- **`src/config.rs`**: `#[cfg(test)] StorageConfig::for_test()` constructor — centralizes test config; `mcp.rs` and `syslog.rs` test helpers now delegate to it
- **`docs/sessions/2026-04-03-mcp-test-code-review-simplify.md`**: Full session documentation

## [0.2.1] - 2026-04-03

### Fixed
- **OAuth discovery 401 cascade**: BearerAuthMiddleware was blocking GET /.well-known/oauth-protected-resource, causing MCP clients to surface generic "unknown error". Added WellKnownMiddleware (RFC 9728) to return resource metadata.

### Added
- **docs/AUTHENTICATION.md**: New setup guide covering token generation and client config.
- **README Authentication section**: Added quick-start examples and link to full guide.



## [0.2.0] — 2026-03-31

### Added
- `docker-compose.yml` / `.env.example` / `README.md`: `SYSLOG_UID` and `SYSLOG_GID` env vars — container now runs as a configurable user/group (default `1000:1000`) for bind-mounted data directories
- `src/db.rs`: `StorageBudgetState` struct — write-blocked flag and storage metrics shared via `Arc<Mutex<>>` across syslog and MCP modules
- `src/db.rs`: Transient SQLite lock retry in batch insert — 3-attempt backoff (25/100/250ms) on `SQLITE_BUSY` / `SQLITE_LOCKED` before failing
- `src/db.rs`: `configure_connection_pragmas()` helper — WAL mode and PRAGMA setup extracted from `init_pool` so every pooled connection is configured consistently
- `src/main.rs`: Initial storage budget enforcement check on startup before accepting syslog traffic
- `src/main.rs`: `background_interval()` helper — interval fires after the first period (not at t=0), preventing a burst on startup
- `src/syslog.rs`: `start_with_storage_state()` replaces `start()` — storage state shared with batch writer for write-blocking under pressure

### Fixed
- `src/syslog.rs`: TCP handler now enforces `max_message_size` **per line** instead of per connection — persistent forwarders (rsyslog, syslog-ng) that reuse a single TCP session no longer hit the connection-level byte limit and cause an OOM/disconnect
- `src/mcp.rs`: Auth rejection now logs `method`, `path`, and `has_auth_header` for diagnostics

### Changed
- `src/db.rs`, `src/main.rs`, `src/mcp.rs`, `src/syslog.rs`: Structured `tracing` fields added throughout — storage enforcement, batch insert, retention purge, TCP/UDP listeners, health check, and MCP request lifecycle all emit structured events
- `.dockerignore`: Reorganized with categorized sections; AI tooling dirs (`.claude`, `.omc`, `.lavra`, `.beads`) explicitly excluded
- `.gitignore`: Reorganized with categorized sections; editor dirs, cache, doc artifacts, and worktree dirs added

---

## [0.1.9] — 2026-03-30

### Changed
- **Breaking: env var rename** — dropped figment's nested `SYSLOG_MCP_SECTION__KEY` format for flat `SYSLOG_*` and `SYSLOG_MCP_*` prefixes. See `.env.example` for the new names.
- `src/config.rs`: Replaced `figment` with `toml` crate + manual env var overlay — simpler, supports two prefixes
- `src/config.rs`: Merged `udp_bind`/`tcp_bind` into `host` + `port` (UDP and TCP always share the same address)
- `src/config.rs`: Renamed `flush_interval_ms` to `flush_interval`
- `docker-compose.yml`: Host-side ports use `${SYSLOG_PORT}` and `${SYSLOG_MCP_PORT}` env vars
- `docker-compose.yml`: Data volume uses `${SYSLOG_MCP_DATA_VOLUME}` (defaults to named volume `syslog-mcp-data`)
- `docker-compose.yml`: Replaced `environment:` block with `env_file: .env`
- `docker-compose.yml`: Removed SWAG labels; network uses `external: true`
- `Dockerfile`: `SYSLOG_MCP_STORAGE__DB_PATH` → `SYSLOG_MCP_DB_PATH`
- `Cargo.toml`: `figment` dependency replaced with `toml`

### Added
- `src/config.rs`: `SyslogConfig::bind_addr()` and `McpConfig::bind_addr()` helper methods
- `src/config.rs`: `validate_host()` rejects host strings containing ports
- `src/config.rs`: 2 new tests — `env_var_overrides_syslog_port`, `host_with_port_is_rejected`

---

## [0.1.7] — 2026-03-30

### Fixed
- `src/db.rs`: Retention purge now uses `received_at` (server clock) instead of `timestamp` (device clock) — prevents misconfigured device clocks from causing immediate purge or infinite retention (syslog-mcp-x6l)
- `src/db.rs`: Added composite `(severity, timestamp)` index for `get_error_summary` query performance (syslog-mcp-ctj)
- `src/db.rs`: `std::collections::HashMap` imported at module level instead of inline paths (syslog-mcp-rva)
- `src/mcp.rs`: `/health` endpoint now runs `SELECT 1` instead of `COUNT(*)` over entire logs table (syslog-mcp-068)
- `src/mcp.rs`: `severity_to_num` moved to `db.rs` as single source of truth (syslog-mcp-nu6)
- `src/mcp.rs`: 401 response uses JSON-RPC 2.0 envelope; replaced `futures` crate with `futures-core` (syslog-mcp-zr4)
- `src/syslog.rs`: TCP accept error now uses exponential backoff (100ms → 5s cap) instead of flat 100ms sleep (syslog-mcp-ve1)
- `src/syslog.rs`: `looks_like_timestamp` now validates digit positions, not just separator offsets (syslog-mcp-qus)
- `src/syslog.rs`: Removed false "octet-counting" claim from TCP listener doc comment (syslog-mcp-jsv)
- `src/syslog.rs`: Flush retry adds 250ms pause to avoid hammering a failing DB (syslog-mcp-rjt)
- `src/config.rs`: Renamed `parse_addr` to `validate_addr` for clarity (syslog-mcp-e5m)
- `bin/smoke-test.sh`: `assert_no_error` now fails on non-JSON output instead of silently passing (syslog-mcp-tef)
- `Cargo.toml`: Removed unused `ws` feature from axum; removed unused `json` feature from tracing-subscriber (syslog-mcp-3ou, syslog-mcp-avg)
- `docker-compose.yml`: SWAG labels updated to `swag=enable` + url/port/proto format (syslog-mcp-j4m)

### Added
- `src/db.rs`: `PRAGMA wal_checkpoint(PASSIVE)` after hourly purge to prevent unbounded WAL growth (syslog-mcp-dah)
- `src/db.rs`: `pub fn severity_to_num()` for reuse across modules (syslog-mcp-nu6)
- `src/config.rs`: `batch_size` and `flush_interval_ms` fields in `SyslogConfig` with serde defaults (syslog-mcp-7uv)
- `src/db.rs`: 4 new unit tests — timestamp range filtering, severity_to_num edge cases, error summary severity filter, severity_in filter (syslog-mcp-063, syslog-mcp-v9r, syslog-mcp-3su, syslog-mcp-94p)
- `bin/backup.sh`: WAL-safe SQLite backup script with cron scheduling and 30-day pruning (syslog-mcp-8zi)
- `docs/runbooks/deploy.md`: Rolling update, rollback, health check, and pre-deploy checklist (syslog-mcp-8np)
- `.env.example`: Added `max_message_size`, `batch_size`, `flush_interval_ms` documentation (syslog-mcp-vri)
- `README.md`: SSE endpoint stub behavior documented; Docker network prereq documented (syslog-mcp-3t7, syslog-mcp-7r4)
- `CLAUDE.md`: CEF hostname trust boundary, batch writer failure path, correlate_events 999 limit cap documented (syslog-mcp-dum, syslog-mcp-2oj, syslog-mcp-y1n)

---

## [0.1.6] — 2026-03-30

### Security
- `src/main.rs`: Redact `api_token` from startup log — log individual fields with `auth_enabled=bool` instead of printing full config struct (syslog-mcp-4yw)
- `src/mcp.rs`: Add optional Bearer token auth middleware; restrict CORS to localhost origins only (syslog-mcp-gm3)

### Fixed
- `Dockerfile`: Fix `ENV SYSLOG_MCP__STORAGE__DB_PATH` → `SYSLOG_MCP_STORAGE__DB_PATH` — double-underscore prefix was silently ignored by figment (syslog-mcp-s9b)
- `src/syslog.rs`: Drop TCP lines exceeding `max_message_size` to prevent OOM from unbounded lines (syslog-mcp-zu9)
- `src/syslog.rs`: Warn when CEF heuristic fires but all fields extract as None — malformed CEF body now emits a log line instead of silently falling back (syslog-mcp-w5e)
- `src/syslog.rs`: Cap TCP connections at 512 with semaphore + 300s wall-clock timeout per connection (syslog-mcp-ct2)
- `src/db.rs`: Chunked DELETE + incremental FTS merge to release WAL write-lock during retention purge (syslog-mcp-75i)
- `src/config.rs`: Replace blocking `to_socket_addrs()` DNS call with non-blocking `SocketAddr::parse()` at config load time
- `Dockerfile`: Run container as non-root user uid/gid 10001 (syslog-mcp-ab8)
- `.lavra/memory/recall.sh`: Remove stray `local` keyword outside function scope (syslog-mcp-1mg)

### Added
- `.github/workflows/ci.yml`: GitHub Actions CI — fmt check, clippy `-D warnings`, test, cargo audit (syslog-mcp-7ee)
- `src/db.rs`: 7 unit tests covering insert, FTS search, severity filter, purge, stats, host aggregation (syslog-mcp-sd0)
- `.env.example`: Document `SYSLOG_MCP_MCP__API_TOKEN` bearer token option

---

## [0.1.5] — 2026-03-28

### Fixed
- `syslog.rs`: Normalize stored timestamps to UTC (`dt.with_timezone(&Utc)`) — mixed-offset sources no longer misorder SQLite rows or break retention purges
- `smoke-test.sh`: `--url` flag now creates a temp mcporter config so health checks and tool calls always target the same server; guard `$2` dereference under `set -u`; fix `limit=0` boundary test that was silently passing `limit=1`
- `recall.sh`: Fix `--all --recent` ordering (archive first → newest entries last in `tail`); use `grep -F` for literal bead matching; fix auto-build to `source + kb_sync` (subprocess call was a no-op)
- `knowledge.jsonl`: Strip embedded shell command fragments from `content` and `bead` fields

### Changed
- `knowledge-db.sh`: Quoted temp file path in `sqlite3 .import`; consolidated 7→1 jq invocations per JSONL line and 2→1 per beads-import row
- `.gitignore`: Narrow `*.db` to `data/*.db` to avoid hiding fixture files
- `README.md` / `CLAUDE.md`: Correct env var prefix `SYSLOG_MCP__` → `SYSLOG_MCP_`
- `docker-compose.yml`: Switch network from internal `syslog-mcp` to external `jakenet`
- Session docs: blank lines after subsection headings; complete rollback command

---

## [0.1.4] — 2026-03-28

### Added
- Session docs for syslog host onboarding (tootie, dookie, squirts, steamy-wsl, vivobook-wsl) and systemd service cleanup

---

## [0.1.3] — 2026-03-28

### Fixed
- Clippy `type_complexity` errors: introduced `LogBatchEntry` type alias for the 8-field batch tuple (`src/db.rs`, `src/syslog.rs`)
- `ORDER BY timestamp` → `ORDER BY l.timestamp` for consistency with table alias in non-FTS search path
- `#[allow(dead_code)]` → `#[expect(dead_code, reason = "...")]` on `jsonrpc` field for self-cleaning lint suppression

### Changed
- Removed single-insert `insert_log` in favour of batch-only path via `insert_logs_batch`
- `search_logs` non-FTS path now uses `FROM logs l` alias, consistent with the FTS join path
- `syslog_loose::parse_message` updated to explicit `Variant::Either` API; timestamp handling simplified from 5-arm `IncompleteDate` match to direct `dt.to_rfc3339()`
- Removed unused imports (`NaiveDateTime`, `StreamExt`, `error`/`info` from tracing, `uuid`, `thiserror`, `axum-extra`, `tower`)
- Removed dead `idx += 1` at end of `tail_logs`

---

## [0.1.2] — 2026-03-27

### Added
- Project documentation (`SETUP.md`, `docs/`)
- Lavra project config and codebase profile (`.lavra/`)
- Beads issue tracking init (`.beads/`)
- Session doc for 2026-03-27 repo init and restructure

### Changed
- Updated Rust base image in `Dockerfile`

### Fixed
- Removed root-level source files after `src/` migration (duplicate artifact cleanup)

---

## [0.1.1] — 2026-03-27

### Changed
- Restructured project to standard Rust layout (`src/` modules)
- Migrated flat source files into `src/config.rs`, `src/db.rs`, `src/mcp.rs`, `src/syslog.rs`, `src/main.rs`

---

## [0.1.0] — 2026-03-27

### Added
- Initial release: syslog receiver + MCP server in Rust
- UDP + TCP syslog listeners on port 1514 (RFC 3164 / RFC 5424 / loose via `syslog_loose`)
- SQLite storage with FTS5 full-text index, WAL mode, and hourly retention purge
- Six MCP tools over JSON-RPC 2.0 (`POST /mcp`):
  - `search_logs` — FTS5 search with host/severity/app/time filters
  - `tail_logs` — most recent N entries
  - `get_errors` — error/warning summary grouped by host and severity
  - `list_hosts` — all known hosts with first/last seen and log counts
  - `correlate_events` — cross-host event correlation in a time window
  - `get_stats` — DB stats (total logs, size, time range)
- SSE endpoint (`GET /sse`) for legacy MCP transport
- Health check endpoint (`GET /health`)
- figment-based config (`config.toml` + `SYSLOG_MCP_` env vars)
- Docker Compose deployment with bind-mounted `./data/` volume
- Batch writer with mpsc channel, 100-entry batches, 500ms flush interval

---

[Unreleased]: https://github.com/jmagar/syslog-mcp/compare/v0.17.7...HEAD
[0.17.7]: https://github.com/jmagar/syslog-mcp/compare/v0.17.6...v0.17.7
[0.17.6]: https://github.com/jmagar/syslog-mcp/compare/v0.17.5...v0.17.6
[0.17.5]: https://github.com/jmagar/syslog-mcp/compare/v0.17.4...v0.17.5
[0.17.4]: https://github.com/jmagar/syslog-mcp/compare/v0.17.3...v0.17.4
[0.17.3]: https://github.com/jmagar/syslog-mcp/compare/v0.17.2...v0.17.3
[0.17.2]: https://github.com/jmagar/syslog-mcp/compare/v0.17.1...v0.17.2
[0.17.1]: https://github.com/jmagar/syslog-mcp/compare/v0.17.0...v0.17.1
[0.17.0]: https://github.com/jmagar/syslog-mcp/compare/v0.16.0...v0.17.0
[0.16.0]: https://github.com/jmagar/syslog-mcp/compare/v0.15.0...v0.16.0
[0.15.0]: https://github.com/jmagar/syslog-mcp/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/jmagar/syslog-mcp/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/jmagar/syslog-mcp/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/jmagar/syslog-mcp/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/jmagar/syslog-mcp/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/jmagar/syslog-mcp/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/jmagar/syslog-mcp/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/jmagar/syslog-mcp/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/jmagar/syslog-mcp/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/jmagar/syslog-mcp/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/jmagar/syslog-mcp/compare/v0.5.0...v0.6.0
[0.1.7]: https://github.com/jmagar/syslog-mcp/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/jmagar/syslog-mcp/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/jmagar/syslog-mcp/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/jmagar/syslog-mcp/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/jmagar/syslog-mcp/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/jmagar/syslog-mcp/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/jmagar/syslog-mcp/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/jmagar/syslog-mcp/releases/tag/v0.1.0
