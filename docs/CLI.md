# Direct CLI Reference -- cortex

The `cortex` binary includes direct query and deployment-lifecycle commands for
humans and shell scripts. Query commands read the configured SQLite database and
call the same shared `SyslogService` methods used by the MCP tool. Compose
lifecycle commands inspect Docker/Compose directly and do not load the SQLite
query runtime.

Direct query CLI mode does not start syslog listeners, the HTTP MCP server, the
REST API, OTLP routes, retention purge, Docker ingest, or storage-budget cleanup
tasks. Keep `cortex serve mcp` running somewhere for ingestion.

## Configuration

CLI commands use the normal config loader:

1. `config.toml` in the working directory, when present
2. `CORTEX_*`, `CORTEX_*`, `CORTEX_API_*`, and `CORTEX_DOCKER_*` environment overrides

For local query use, the important setting is:

```bash
CORTEX_DB_PATH=/data/cortex.db
```

`CORTEX_TOKEN` is not used by direct CLI mode because it is local database
access, not HTTP access.

## `cortex file-tail`

Manage Cortex-owned file-tail ingest sources. Sources are persisted beside the
configured database in `file-tails.json` and reconciled by the running
`cortex serve mcp` process.

```bash
cortex file-tail list [--json]
cortex file-tail status [--json]
cortex file-tail add --id ID --path PATH --tag TAG --host HOST [--facility FACILITY] [--severity SEVERITY] [--from-start] [--json]
cortex file-tail remove --id ID [--json]
cortex file-tail enable --id ID [--json]
cortex file-tail disable --id ID [--json]
```

The command maps to MCP action `file_tails` and REST `POST /api/file-tails`.
When `--http` or `CORTEX_USE_HTTP=true` is used, set both `CORTEX_API_TOKEN`
and `CORTEX_API_ADMIN_TOKEN`; the client sends the latter as
`X-Cortex-Admin-Token`.
By default `add` starts tailing at EOF; pass `--from-start` to ingest existing
file contents.

## Output

All commands print compact human-readable output by default. Add `--json` to
print the exact serialized `SyslogService` response shape. MCP uses the same
shape for matching actions; REST parity applies only to commands that also have
REST endpoints.

```bash
cortex stats --json
cortex search 'error AND nginx' --limit 5 --json
```

## Commands

### `cortex search`

Search logs with optional FTS5 query and filters. The query is a bare positional;
with no `--limit` the result count defaults to **50**.

```bash
cortex search "oom killer"                       # bare query, limit 50
cortex search 'error AND nginx' --host proxy --limit 10
cortex search --grep "smoke-test" --since 1h     # literal text, last hour
```

Flags:

| Flag | Description |
| --- | --- |
| positional query | Optional SQLite FTS5 query. Multiple words are joined with spaces. |
| `--grep TEXT` | Literal (FTS5-safe) substring; mutually exclusive with the positional query |
| `--host HOST` | Exact claimed hostname filter |
| `--source SOURCE` | Exact source identifier filter |
| `--severity LEVEL` | Syslog severity filter: `emerg`, `alert`, `crit`, `err`, `warning`, `notice`, `info`, `debug` |
| `--app APP` | Application/process name filter |
| `--since TIME` | Start of window — relative (`1h`, `2d`, `yesterday`) or RFC3339 |
| `--until TIME` | End of window — relative or RFC3339 |
| `--limit N` | Maximum returned rows (default 50) |
| `--json` | Print JSON response |

### `cortex tail`

Return recent log entries, optionally filtered by host, source, or app. A bare
positional argument is a hostname (shorthand for `--host`); with no `-n`/`--limit`
the row count defaults to **50**.

```bash
cortex tail                       # 50 most recent across all hosts
cortex tail dookie                # bare positional → --host dookie
cortex tail nas --app kernel -n 100
```

Flags:

| Flag | Description |
| --- | --- |
| positional `HOST` | Hostname filter — shorthand for `--host` |
| `-n N`, `--n N`, `--limit N` | Number of rows to return (default 50) |
| `--host HOST` | Exact claimed hostname filter |
| `--source SOURCE` | Exact source identifier filter |
| `--app APP` | Application/process name filter |
| `--json` | Print JSON response |

### `cortex errors`

Summarize error and warning counts by host and severity. With no `--since` the
window defaults to the **last hour**.

```bash
cortex errors                     # last hour
cortex errors --since 6h --limit 50
cortex errors --since 2026-01-01T00:00:00Z --until 2026-01-02T00:00:00Z --json
```

Flags:

| Flag | Description |
| --- | --- |
| `--since TIME` | Start of window — relative (`1h`, `6h`, `yesterday`) or RFC3339 (default: last hour) |
| `--until TIME` | End of window — relative or RFC3339 |
| `--json` | Print JSON response |

### `cortex hosts`

List all known hosts with log counts and last-seen timestamps.

```bash
cortex hosts
cortex hosts --json
```

### `cortex inventory`

Refresh and inspect the private homelab inventory cache under
`~/.cortex/inventory`.

```bash
cortex inventory refresh --json
cortex inventory status --json
```

`refresh` runs native Rust collectors for local host facts, Docker endpoints,
raw-but-redacted Compose YAML and reverse-proxy artifacts, Unraid, Tailscale,
UniFi, media services, and configured local project roots. Missing provider
credentials are warnings, not fatal errors for unrelated collectors. `status`
reads only the cache metadata and does not open SQLite.

Server-side refresh additionally projects the normalized inventory into the
investigation graph. It runs on the 5-minute baseline cadence and reacts to
local Compose/proxy config changes. Remote Docker container event streams over
SSH can be enabled explicitly with `CORTEX_INVENTORY_REMOTE_DOCKER_EVENTS=true`.

### `cortex sessions`

List AI transcript sessions grouped by project.

```bash
cortex sessions --project /home/jmagar/workspace/cortex --limit 20
```

Flags:

| Flag | Description |
| --- | --- |
| `--project PATH` | Exact project path filter |
| `--tool TOOL` | AI tool filter: `claude`, `codex`, or `gemini` |
| `--host HOST` | Filter by host |
| `--since TIME` | RFC3339 start timestamp |
| `--until TIME` | RFC3339 end timestamp |
| `--limit N` | Maximum returned rows |
| `--json` | Print JSON response |

### `cortex ai search`

Ranked grouped session search across AI transcript rows.

```bash
cortex ai search authentication --tool claude --limit 10
```

Human output now states that grouping is computed over the newest matching
candidate window. JSON includes `total_candidates`, `candidate_rows`,
`candidate_cap`, `candidate_window_truncated`, and `truncated`; when the
candidate window is truncated, narrow with `--project`, `--tool`, `--since`, or
`--until` for exact grouping within that filter.

### `cortex ai abuse`

Detect abuse in AI transcript rows and return surrounding rows from the same
AI session.

```bash
cortex ai abuse --project /home/jmagar/workspace/cortex --limit 10 --before 3 --after 3
cortex ai abuse --tool codex --term dang --term heck --json
```

By default this uses the built-in abuse list and returns 2 rows before and
after each hit. Use repeated `--term WORD` flags to replace the built-in list
with a custom detector. JSON includes `candidate_rows`, `candidate_cap`,
`candidate_window_truncated`, `truncated`, and `matches[].{term,entry,before,after}`.

### `cortex ai incidents`

Group abuse hits into scored incident candidates.

```bash
cortex ai incidents --project /home/jmagar/workspace/cortex --limit 10
cortex ai incidents --tool codex --term dang --term heck --json
```

Use `--window-minutes` to change how nearby abuse hits are grouped into one
incident. JSON includes `total_incidents`, `candidate_rows`, `candidate_cap`,
`candidate_window_truncated`, `truncated`, and `incidents[]`.

### `cortex ai investigate`

Expand top incidents into deterministic evidence bundles without calling an LLM.

```bash
cortex ai investigate --project /home/jmagar/workspace/cortex --limit 3
cortex ai investigate --correlation-window-minutes 15 --json
```

Each evidence bundle includes the incident, anchor transcript rows, same-session
before/after context, nearby non-AI logs, and nearby warning-or-higher logs.
The public command expands at most 10 incidents per run.

### `cortex ai assess`

Fetch one incident evidence bundle and run the local Gemini CLI to produce a
Markdown frustration assessment.

```bash
cortex ai incidents --limit 10
cortex ai assess inc-f9a1d8e70cad13e6 --limit 3
cortex ai assess inc-f9a1d8e70cad13e6 --model gemini-3.1-flash-lite-preview --json
```

`assess` is local-only and rejects `--http` because it spawns Gemini on the
local host. It can assess any incident ID returned by `cortex ai incidents`
within the incident-list cap, even when that incident is outside the top 10
investigation bundles.

### `cortex ai blocks`

Bucket AI activity into 5-hour UTC windows.

```bash
cortex ai blocks --project /home/jmagar/workspace/cortex
```

When `--since` is omitted, usage blocks default to the last 30 days. Returned
JSON includes `total_blocks` and `truncated`; at most 1000 buckets are returned.

### `cortex ai context`

Summarize one AI project path.

```bash
cortex ai context --project /home/jmagar/workspace/cortex --limit 5
```

Recent representative entries are capped at 20 rows, and message snippets are
bounded to 256 characters for predictable MCP/CLI payload size.

### `cortex ai correlate`

Cross-reference AI transcript rows against nearby non-AI logs.

```bash
cortex ai correlate --project /home/jmagar/workspace/cortex --limit 5
cortex ai correlate --ai-query deploy --log-query container --window-minutes 10 --severity-min warning --json
```

The AI side uses transcript rows as anchors. The related log side searches the
normal log corpus inside each anchor window and excludes AI transcript rows, so
the command surfaces host, Docker, OTLP, and syslog events around the session
without duplicating the transcript stream itself.

### `cortex ai tools`

List distinct AI tools with counts.

```bash
cortex ai tools --json
```

Returned JSON includes `total_tools` and `truncated`; at most 100 tools are
returned.

### `cortex ai projects`

List distinct AI projects with counts.

```bash
cortex ai projects --tool claude
```

Returned JSON includes `total_projects` and `truncated`; at most 200 projects
are returned.

### `cortex ai index`

Explicitly scan local transcript roots (`~/.claude/projects`, `~/.codex/sessions`) or one `--path`.

```bash
cortex ai index
cortex ai index --path ~/.claude/projects
cortex ai index --since 2026-05-14T00:00:00Z
cortex ai index --path ~/.codex/sessions --force
```

Path policy is intentionally narrow. Recursive `--path` scans are accepted only
for known transcript roots (`~/.claude/projects`, `~/.codex/sessions`) or their
children; one explicit `.jsonl` file can be imported outside those roots.
`--path /`, `--path $HOME`, and the repo root are rejected before walking, and
symlinks are skipped. Directories are scanned only for `.jsonl` transcript
files, unsupported files are counted but not parsed, and each file is streamed
line-by-line with chunked SQLite transactions. If storage guardrails cannot
recover enough space, indexing fails before committing additional chunks.

`--since TIME` skips files whose filesystem modification time is older than the
RFC3339 timestamp. `--force` clears existing import identities and previously
stored log rows for each scanned transcript path before reimporting, which is
the right option after parser fixes or scrubber changes.

### `cortex ai add`

Ingest one explicit transcript file.

```bash
cortex ai add --file ~/.claude/projects/example/session.jsonl
cortex ai add --file ~/.codex/sessions/2026/05/14/session.jsonl --force
```

`--force` reimports that one transcript from scratch without leaving duplicate
log rows.

### `cortex ai watch`

Watch local Claude/Codex transcript roots and index stable changed `.jsonl`
files as they are written.

```bash
cortex ai watch
cortex ai watch --path ~/.claude/projects --no-initial-scan
cortex ai watch --debounce-ms 750 --settle-ms 500 --max-retries 5 --json
```

The watcher is a host-local helper, not part of the Docker Compose runtime. It
reuses the same scanner root policy, file support checks, checkpoints,
append-offset indexing, duplicate suppression, parse-error persistence, and
storage guardrails as `cortex ai index` and `cortex ai add`. The watcher only
coalesces filesystem events, waits for files to stabilize, and retries
transient parse/storage/file errors up to the configured cap.

### `cortex ai checkpoints`

Inspect structured scanner checkpoints without opening SQLite directly.

```bash
cortex ai checkpoints --limit 20
cortex ai checkpoints --errors --json
cortex ai checkpoints --missing
```

The output shows source kind, imported record count, last successful checkpoint,
missing-source status, parse error count, and the last parser/indexing error
when present.

### `cortex ai errors`

Inspect persisted transcript parser errors.

```bash
cortex ai errors --limit 20
cortex ai errors --json
```

Errors include source path, source kind, line number, timestamp, and a bounded
scrubbed preview so parser failures can be investigated without opening the
database directly.

### `cortex ai prune-checkpoints`

Remove checkpoints for transcript files that no longer exist.

```bash
cortex ai prune-checkpoints --missing --dry-run
cortex ai prune-checkpoints --missing --limit 100
```

Pruning is deliberately limited to `--missing` checkpoints. It removes scanner
source metadata, import identities, and parse-error rows for missing files; it
does not delete already imported log rows.

### `cortex ai doctor`

Summarize the local AI indexing state.

```bash
cortex ai doctor
cortex ai doctor --json
cortex ai doctor --strict-permissions --json
```

The doctor reports the DB path in use, whether `~/.claude/projects` and
`~/.codex/sessions` exist, whether they are readable/writable by the current
user, owner uid/gid, mode, checkpoint counts, missing checkpoint counts,
imported record count, parse error count, and the newest indexed transcript.
Without `--strict-permissions`, this is a report-only command. With
`--strict-permissions`, it exits non-zero when either transcript root is
missing, unreadable, unwritable, or owned by another user.

### `cortex ai watch-status`

Inspect the supported user-systemd watcher without reading systemd internals by
hand.

```bash
cortex ai watch-status
cortex ai watch-status --json
```

The status command reports `syslog-ai-watch.service` active/enabled state, main
PID, ExecStart, and the latest bounded journal lines. It uses the same user bus
fallback as setup commands, so it still works from shells or tool environments
that do not export `DBUS_SESSION_BUS_ADDRESS`.

### `cortex ai smoke-watch`

Run a bounded live smoke test of the host-local watcher. The command writes a
temporary Claude transcript under `~/.claude/projects`, waits for the watcher to
ingest it into the configured database, deletes the temp file, then waits for
the missing-checkpoint pruner to clear scanner metadata.

```bash
cortex ai smoke-watch
cortex ai smoke-watch --json
```

This is a live command. It requires `syslog-ai-watch.service` to be running and
writing to the same `CORTEX_DB_PATH` used by the CLI process.

### `cortex shell index`

Backfill local shell history into the main log corpus.

```bash
cortex shell index --path ~/.zsh_history
cortex shell index --path ~/.zsh_history --shell zsh --json
```

The importer currently supports zsh extended history lines in the
`: <epoch>:<duration>;<command>` format. Plain history lines without timestamps
are counted as skipped because they cannot be correlated reliably. Commands are
scrubbed before storage, written with `source_kind="shell-history"`, and use
`source_ip` identities shaped like `shell-history://<hostname>/<user>/<shell>`.
Rows are deduped by source identity, timestamp, and scrubbed command text, and
the importer records a private byte-offset cursor under the cortex state
directory so repeated imports only read newly appended history.

### `cortex agent-command`

Capture shell commands launched by agent tools, then ingest the private JSONL
spool into SQLite.

```bash
cortex setup agent-command install
export CLAUDE_CODE_SHELL_PREFIX="$HOME/.local/bin/cortex-agent-command-wrapper"

cortex agent-command ingest-spool --path ~/.local/state/cortex/agent-command.jsonl
cortex agent-command wrap --spool ~/.local/state/cortex/agent-command.jsonl -- cargo test
```

`CLAUDE_CODE_SHELL_PREFIX` is the Claude Code hook point for commands spawned by
Claude Code, including Bash tool calls, hook commands, and stdio MCP server
startup commands. The generated wrapper executes the original command, preserves
stdio and exit code, then appends one scrubbed JSONL record. It removes
`CLAUDE_CODE_SHELL_PREFIX` and sets an internal recursion guard for the child
process so the wrapper does not wrap itself.

The wrapper does not capture environment variables, stdout, or stderr by
default. Command strings are scrubbed for known token, secret flag, assignment,
Authorization header, URL-userinfo, `curl -u`, and private-key forms before they
reach the spool. The spool directory is created as `0700`, the spool file as
`0600`, symlink paths are rejected, and import refuses group/world writable
parents or spools. Wrapper appends and spool imports use the same advisory file
lock; after a successful import the spool is truncated so repeated imports do
not rescan already-ingested commands. Imported rows use
`source_kind="agent-command"`, `facility`
`agent`, `app_name`/`ai_tool` set to the agent name, `event_action="command"`,
and `source_ip` identities shaped like
`agent-command://<hostname>/<agent>/<session_id>`.

### `cortex setup ai-watch-service`

Install, remove, or inspect the supported host-local user-systemd watcher for
near-real-time transcript ingestion.

```bash
cortex setup ai-watch-service install
cortex setup ai-watch-service check --json
cortex setup ai-watch-service remove
```

Install resolves an absolute `cortex` binary and a concrete SQLite DB path,
writes a private environment file under `~/.config/cortex/`, runs one
initial `cortex ai index --json` phase, disables the older polling timer, and
starts `syslog-ai-watch.service` with `cortex ai watch --no-initial-scan
--json`. The helper is intentionally outside the container because it must read
host-local Claude/Codex transcript files; Docker Compose remains the
server/query deployment. Remove events from watched transcript files trigger a
bounded missing-checkpoint prune pass, which keeps scanner/checkpoint metadata
from accumulating entries for deleted local session files without deleting
already imported log rows.

Initial-index transcript data quality issues are warnings, not install-blocking
errors. The setup JSON includes `blocking_errors`, `data_quality_warnings`,
`service_enabled`, and `watcher_healthy` so automation can distinguish a broken
watcher from historical transcript cleanup work. When data-quality warnings are
reported, inspect them with:

```bash
cortex ai errors --limit 20
cortex ai checkpoints --errors
cortex ai index --json
```

Storage-blocked writes, invalid JSON from the indexer, command failures, stale
unit content, permission failures, and failed `systemctl enable --now` phases
remain blocking errors. Installing the watch service disables the older
`syslog-ai-index.timer` to avoid duplicate background ingestion loops.

### `cortex setup debug-wrapper`

Install, remove, or inspect the host-local debug wrapper at
`~/.local/bin/cortex`.

```bash
cortex setup debug-wrapper install
cortex setup debug-wrapper check --json
cortex setup debug-wrapper remove
```

The wrapper is intentionally machine-local. It `cd`s into the configured repo
or worktree, builds `cargo build --bin cortex` into `.cache/cargo`, then execs
the fresh debug binary. For non-server commands it defaults Docker ingest off
and bearer auth mode on, so regular CLI checks do not accidentally start
container-log ingestion or OAuth-only config paths. Override the source checkout
with `CORTEX_REPO=/path/to/cortex cortex ...`.

### `cortex setup debug-compose`

Install, remove, or inspect the local debug Compose override under
`~/.cortex/compose/docker-compose.override.yml`.

```bash
cortex setup debug-compose install
cortex setup debug-compose check --json
cortex setup debug-compose remove
```

The override is machine-local. It points the canonical Docker Compose project at
the current repo/worktree and builds the `cortex:local-debug` image with the
debug profile. This keeps `docker compose up -d --build` aligned with the same
code that the host debug wrapper builds. Existing setup environments may still
carry the legacy `COMPOSE_PROJECT_NAME=syslog-jmagar-lab` for container-label
compatibility; use `cortex compose ...` when possible because it resolves the
live owner before mutating the stack.

### `cortex setup doctor`

Run the repo-owned local setup checks as one command.

```bash
cortex setup doctor
cortex setup doctor --json
```

The doctor checks setup directories, `.env`, Compose assets, the debug wrapper,
the debug Compose override, transcript-root permissions, disabled legacy index
timer state, active/enabled watcher state, and container freshness via
`scripts/check-runtime-current.sh --allow-local-image`.

### `cortex deploy`

Run the Compose-backed deployment workflow using operator-facing names.
`preflight` and `local` call the same setup engine as `cortex setup check`
and `cortex setup repair`; `remote` uses SSH plus the shared setup assets.

```bash
cortex deploy preflight
cortex deploy preflight --json
cortex deploy local
cortex deploy local --dry-run --json
cortex deploy remote tootie --dry-run
cortex deploy remote tootie --json
```

`deploy preflight` and `deploy local --dry-run` do not mutate Docker state.
`deploy local` repairs `~/.cortex/.env`, rewrites managed Compose assets,
pulls the configured image, starts the stack, and checks `/health`.
`deploy remote` uses SSH and Docker Compose on the target host. Non-dry-run
remote deploy writes/replaces `~/.cortex/.env`, the managed Compose YAML,
and `config/Dockerfile` on the target; set token/env values in the local
environment before running it when you need to preserve specific values. It is
CLI-only, requires an explicit host argument, and does not add REST or MCP
deploy mutation surfaces.

### `cortex setup ai-index-timer`

Install, remove, or inspect the optional host-local user-systemd polling
fallback that periodically runs `cortex ai index`.

```bash
cortex setup ai-index-timer install
cortex setup ai-index-timer check --json
cortex setup ai-index-timer remove
```

This helper is intentionally not part of the Docker container. It scans
host-local transcript roots (`~/.claude/projects`, `~/.codex/sessions`) using a
host `cortex` binary, then writes to the configured SQLite DB. Prefer
`cortex setup ai-watch-service install` for normal use; the watcher install
disables this timer to avoid duplicate background ingestion loops.

### `cortex doctor binary`

Check whether the shell binary and running container line up with this repo.

```bash
cortex doctor binary
cortex doctor binary --json
```

The doctor reports the current executable, `cortex` resolved from `PATH`, repo
version, container version when Docker is available, and the result of
`scripts/check-runtime-current.sh`.

For a one-command live check of the AI transcript workflow, run:

```bash
bash scripts/smoke-ai.sh
bash scripts/smoke-ai-mcp.sh
```

The smoke scripts resolve `CORTEX_BIN` first, then `cortex` on `PATH`, then the
repo-local debug binary at `target/debug/cortex`.

With `syslog-ai-watch.service` installed, new transcript lines usually become
searchable within a few seconds of the writer closing or flushing the file.
Imported transcript messages are scrubbed for known credential/token patterns
before storage and FTS indexing, but scrubbing is best-effort. Raw log actions
can still expose scrubbed AI messages and local `ai_transcript_path` values, so
do not expose the database or MCP endpoint to clients that should not see local
AI session content.

### `cortex correlate`

Find related events around a reference timestamp. Results are grouped by host.

```bash
cortex correlate --reference-time 2026-01-01T12:00:00Z --window-minutes 10
cortex correlate 2026-01-01T12:00:00Z --severity-min err --query timeout --limit 50
```

Flags:

| Flag | Description |
| --- | --- |
| positional reference time | RFC3339 center timestamp |
| `--reference-time TIME` | RFC3339 center timestamp |
| `--window-minutes N` | Minutes before and after the reference time |
| `--severity-min LEVEL` | Minimum severity to include |
| `--host HOST` | Exact claimed hostname filter |
| `--source SOURCE` | Exact source identifier filter |
| `--query FTS` | Optional FTS5 query |
| `--limit N` | Maximum total events |
| `--json` | Print JSON response |

### `cortex host-state`

Return the latest bounded heartbeat state for one host. A bare positional
argument is a hostname (shorthand for `--host`).

```bash
cortex host-state tootie          # bare positional → --host tootie
cortex host-state --host-id host-a --limit 5 --json
```

Flags:

| Flag | Description |
| --- | --- |
| positional `HOST` | Hostname — shorthand for `--host` |
| `--host-id ID` | Authoritative heartbeat host identity |
| `--host HOST` | Self-reported hostname fallback (must resolve to one host) |
| `--since TIME` | Minimum `sampled_at` timestamp (ISO 8601) |
| `--limit N` | Number of samples (default 1, max 100) |
| `--json` | Print JSON response |

### `cortex fleet-state`

Print a fleet-wide heartbeat snapshot with pressure flags and summary counts.

```bash
cortex fleet-state
cortex fleet-state --exclude-ok --sort freshness --json
```

Flags:

| Flag | Description |
| --- | --- |
| `--exclude-ok` | Omit hosts whose status is `ok` |
| `--include-ok` | Include `ok` hosts (default) |
| `--sort ORDER` | `pressure` (default), `freshness`, or `hostname` |
| `--json` | Print JSON response |

### `cortex correlate-state`

Correlate non-AI logs with per-host heartbeat window summaries around a
reference time. Bounded by default; never performs a full-history scan.

```bash
cortex correlate-state --reference-time 2026-01-01T12:00:00Z --window-minutes 10
cortex correlate-state --reference-time 2026-01-01T12:00:00Z --host tootie --severity-min warning --json
```

Flags:

| Flag | Description |
| --- | --- |
| `--reference-time TIME` | RFC3339 center timestamp (required) |
| `--window-minutes N` | Minutes before and after (default 10, max 120) |
| `--host HOST` | host_id or unique hostname; omit for bounded cross-host plan |
| `--severity-min LEVEL` | Minimum log severity (default `info`) |
| `--limit N` | Maximum log rows per host (default 100, max 500) |
| `--json` | Print JSON response |

### `cortex entity`

Resolve a derived graph entity by canonical type/key or by alias. Ambiguous
aliases return candidates instead of silently choosing one.

```bash
cortex entity host tootie
cortex entity host:tootie --json
cortex entity --alias-type hostname --alias-key tootie
```

Flags:

| Flag | Description |
| --- | --- |
| positional `TYPE KEY` | Exact graph entity type and key |
| positional `TYPE:KEY` | Forgiving exact lookup form |
| `--alias-type TYPE` | Alias type such as `hostname` or `heartbeat_host_id` |
| `--alias-key KEY` | Alias value to resolve |
| `--limit N` | Alias candidate cap |
| `--evidence-sample-limit N` | Accepted for response metadata symmetry |
| `--payload-budget BYTES` | Approximate response payload budget |
| `--json` | Print shared structured response |

### `cortex graph around`

Return a bounded one-hop neighborhood for a graph entity. Human output includes
relationship type, source/destination entity summaries, confidence/trust,
reason, evidence counts, safe samples, projection status, truncation reason,
and follow-up commands.

```bash
cortex graph around host tootie
cortex graph around host:tootie --limit 25
cortex graph around --entity-id 42 --json
```

Flags:

| Flag | Description |
| --- | --- |
| positional `TYPE KEY` | Entity to expand |
| positional `TYPE:KEY` | Forgiving entity form |
| `--entity-id ID` | Exact graph entity id to expand |
| `--alias-type TYPE` | Alias type for resolving the starting entity |
| `--alias-key KEY` | Alias value for resolving the starting entity |
| `--depth 1` | V1 supports one-hop only |
| `--limit N` | Relationship cap |
| `--evidence-sample-limit N` | Safe evidence samples per relationship |
| `--payload-budget BYTES` | Approximate response payload budget |
| `--json` | Print shared structured response |

### `cortex graph explain`

Generate a deterministic evidence-backed explanation over bounded graph
chains. Human output includes conservative confidence, cited relationship and
evidence ids, missing evidence, open questions, projection status, truncation
reason, and follow-up graph commands. Low-confidence output avoids causal
claims.

```bash
cortex graph explain host tootie
cortex graph explain host:tootie --depth 2 --beam-width 20
cortex graph explain --entity-id 42 --json
```

Flags:

| Flag | Description |
| --- | --- |
| positional `TYPE KEY` | Entity to explain |
| positional `TYPE:KEY` | Forgiving entity form |
| `--entity-id ID` | Exact graph entity id to explain |
| `--alias-type TYPE` | Alias type for resolving the starting entity |
| `--alias-key KEY` | Alias value for resolving the starting entity |
| `--depth N` | Explanation expansion depth, default 2, hard max 3 |
| `--beam-width N` | Relationships fetched per frontier entity |
| `--max-chains N` | Total candidate chain cap |
| `--evidence-sample-limit N` | Safe evidence samples per relationship |
| `--payload-budget BYTES` | Approximate response payload budget |
| `--json` | Print shared structured response |

### `cortex graph evidence`

Inspect the proof row behind one graph evidence id. Human output includes the
evidence id, owning relationship id, readable source/destination endpoints,
reason, trust, confidence, source ids, bounded source-log summary when present,
safe excerpt, metadata path, and follow-up graph/log commands.

```bash
cortex graph evidence 12345
cortex graph evidence 12345 --json
```

Flags:

| Flag | Description |
| --- | --- |
| positional `EVIDENCE_ID` | `graph_relationship_evidence.id` to inspect |
| `--payload-budget BYTES` | Approximate response payload budget |
| `--json` | Print shared structured response |

`source_log_summary` never includes the raw syslog frame or full
`metadata_json`. When a source log id points at a retained-out/deleted row,
the response keeps the evidence, relationship, and endpoint summaries while
returning `source_log_summary: null` and `missing_source_reason`.

### `cortex stats`

Print database and storage guardrail metrics.

```bash
cortex stats
cortex stats --json
```

### `cortex db status`

Print SQLite maintenance state for the configured database.

```bash
cortex db status
cortex db status --json
```

The status includes page counts, freelist count, page size, logical and physical
database size, WAL/SHM sidecar sizes when present, journal mode, auto-vacuum
mode, and no integrity scan. Use `cortex db integrity` for the full SQLite
integrity check on large databases.

### `cortex db integrity`

Run `PRAGMA integrity_check` against the configured database.

```bash
cortex db integrity
cortex db integrity --json
```

The command exits non-zero if SQLite reports anything other than `ok`.

### `cortex db checkpoint`

Run a WAL checkpoint.

```bash
cortex db checkpoint
cortex db checkpoint --mode full
cortex db checkpoint --mode truncate --json
```

Supported modes are `passive`, `full`, `restart`, and `truncate`. The command
exits non-zero if SQLite reports the checkpoint as busy.

### `cortex db vacuum`

Run SQLite vacuum maintenance.

```bash
cortex db vacuum
cortex db vacuum --pages 5000
cortex db vacuum --full
```

The default is `PRAGMA incremental_vacuum(1000)`. `--full` runs `VACUUM` and can
take longer on large databases.

### `cortex db backup`

Create a WAL-safe SQLite backup using the `sqlite3` CLI `.backup` command.

```bash
cortex db backup
cortex db backup --output ~/.cortex/backups
cortex db backup --output /tmp/syslog-copy.db --json
```

When `--output` is a directory or omitted, the command writes a timestamped
`syslog-YYYY-MM-DD-HHMMSS.db` backup. When `--output` has a file extension, it
is used as the exact destination file.

### `cortex compose`

Diagnose and manage the Docker Compose deployment without opening the SQLite
database.

```bash
cortex compose doctor
cortex compose status --json
cortex compose pull
cortex compose up
cortex compose restart
cortex compose logs --tail 20
cortex compose down --yes
```

Common target flags:

| Flag | Description |
| --- | --- |
| `--compose-file FILE` | Explicit Compose file |
| `--project-dir DIR` | Explicit Compose project directory |
| `--project-name NAME` | Compose project name, only safe with a file/dir or live labels |
| `--service NAME` | Compose service name, default `cortex` |
| `--container NAME` | Container name, default `cortex` |
| `--json` | Print JSON response |

Mutation flags:

| Flag | Description |
| --- | --- |
| `--dry-run` | Resolve and preflight without running Docker |
| `--allow-cwd-target` | Permit cwd `docker-compose.yml` fallback for mutation |
| `--yes` | Required for non-interactive destructive `down` |

`cortex compose` refuses ambiguous target discovery, mismatched requested
project/service selectors, cwd fallback without confirmation,
project-name-only mutations, missing Compose files, legacy service conflicts,
non-target listeners on syslog ports, and destructive service stop without
`--yes`. `down` is intentionally service-scoped (`docker compose stop
cortex`), not a project-wide `docker compose down`.

## Relationship to MCP

The direct CLI and MCP tool share the same business layer. Transport adapters
own argument parsing and rendering; shared defaults, limits, validation, audit
identity, and safety policy belong in `SyslogService` or service-owned request
models.

| CLI command | MCP action |
| --- | --- |
| `cortex search` | `cortex` with `action="search"` |
| `cortex filter` | `cortex` with `action="filter"` |
| `cortex tail` | `cortex` with `action="tail"` |
| `cortex errors` | `cortex` with `action="errors"` |
| `cortex hosts` | `cortex` with `action="hosts"` |
| `cortex sessions` | `cortex` with `action="sessions"` |
| `cortex ai search` | `cortex` with `action="search_sessions"` |
| `cortex ai abuse` | `cortex` with `action="abuse"` |
| `cortex ai incidents` | `cortex` with `action="abuse_incidents"` |
| `cortex ai investigate` | `cortex` with `action="abuse_investigate"` |
| `cortex ai correlate` | `cortex` with `action="ai_correlate"` |
| `cortex ai blocks` | `cortex` with `action="usage_blocks"` |
| `cortex ai context` | `cortex` with `action="project_context"` |
| `cortex ai tools` | `cortex` with `action="list_ai_tools"` |
| `cortex ai projects` | `cortex` with `action="list_ai_projects"` |
| `cortex ai similar` | `cortex` with `action="similar_incidents"` |
| `cortex ai ask-history` | `cortex` with `action="ask_history"` |
| `cortex ai incident-context` | `cortex` with `action="incident_context"` |
| `cortex correlate` | `cortex` with `action="correlate"` |
| `cortex host-state` | `cortex` with `action="host_state"` |
| `cortex fleet-state` | `cortex` with `action="fleet_state"` |
| `cortex correlate-state` | `cortex` with `action="correlate_state"` |
| `cortex apps` | `cortex` with `action="apps"` |
| `cortex source-ips` | `cortex` with `action="source_ips"` |
| `cortex timeline` | `cortex` with `action="timeline"` |
| `cortex patterns` | `cortex` with `action="patterns"` |
| `cortex context` | `cortex` with `action="context"` |
| `cortex get` | `cortex` with `action="get"` |
| `cortex ingest-rate` | `cortex` with `action="ingest_rate"` |
| `cortex silent-hosts` | `cortex` with `action="silent_hosts"` |
| `cortex clock-skew` | `cortex` with `action="clock_skew"` |
| `cortex anomalies` | `cortex` with `action="anomalies"` |
| `cortex compare` | `cortex` with `action="compare"` |
| `cortex sig list` | `cortex` with `action="unaddressed_errors"` |
| `cortex sig ack` | `cortex` with `action="ack_error"` |
| `cortex sig unack` | `cortex` with `action="unack_error"` |
| `cortex notify recent` | `cortex` with `action="notifications_recent"` |
| `cortex notify test` | `cortex` with `action="notifications_test"` |
| `cortex stats` | `cortex` with `action="stats"` |
| `cortex compose status` | `cortex` with `action="compose_status"` (redacted read-only projection only) |
| `cortex compose doctor` | `cortex` with `action="compose_doctor"` (redacted read-only projection only) |

The MCP-only `status` and `help` actions are runtime/protocol helpers, not
direct database queries. Compose mutations (`up`, `down`, `restart`, `pull`,
`logs`) are CLI-only and are not exposed over MCP. Admin MCP actions such as
`ack_error`, `unack_error`, and `notifications_test` require `cortex:admin`
when auth is mounted.

Use direct CLI mode for terminal queries and scripts on a host that can read the
SQLite database. Use MCP HTTP or `cortex mcp` when an MCP client needs tool
access.

## See also

- [README.md](../README.md) -- project overview and quick examples
- [mcp/TOOLS.md](mcp/TOOLS.md) -- MCP action reference
- [mcp/TRANSPORT.md](mcp/TRANSPORT.md) -- HTTP and stdio MCP transports
- [CONFIG.md](CONFIG.md) -- config file and environment reference
