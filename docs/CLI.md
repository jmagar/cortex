# Direct CLI Reference -- syslog-mcp

The `syslog` binary includes direct query and deployment-lifecycle commands for
humans and shell scripts. Query commands read the configured SQLite database and
call the same shared `SyslogService` methods used by the MCP tool. Compose
lifecycle commands inspect Docker/Compose directly and do not load the SQLite
query runtime.

Direct query CLI mode does not start syslog listeners, the HTTP MCP server, the
REST API, OTLP routes, retention purge, Docker ingest, or storage-budget cleanup
tasks. Keep `syslog serve mcp` running somewhere for ingestion.

## Configuration

CLI commands use the normal config loader:

1. `config.toml` in the working directory, when present
2. `SYSLOG_*`, `SYSLOG_MCP_*`, `SYSLOG_API_*`, and `SYSLOG_DOCKER_*` environment overrides

For local query use, the important setting is:

```bash
SYSLOG_MCP_DB_PATH=/data/syslog.db
```

`SYSLOG_MCP_TOKEN` is not used by direct CLI mode because it is local database
access, not HTTP access.

## Output

All commands print compact human-readable output by default. Add `--json` to
print the exact serialized `SyslogService` response shape. MCP uses the same
shape for matching actions; REST parity applies only to commands that also have
REST endpoints.

```bash
syslog stats --json
syslog search 'error AND nginx' --limit 5 --json
```

## Commands

### `syslog search`

Search logs with optional FTS5 query and filters.

```bash
syslog search 'error AND nginx' --hostname proxy --limit 10
syslog search '"disk full"' --source-ip 10.0.0.5:514 --from 2026-01-01T00:00:00Z
```

Flags:

| Flag | Description |
| --- | --- |
| positional query | Optional SQLite FTS5 query. Multiple words are joined with spaces. |
| `--hostname HOST` | Exact claimed hostname filter |
| `--source-ip SOURCE` | Exact source identifier filter |
| `--severity LEVEL` | Syslog severity filter: `emerg`, `alert`, `crit`, `err`, `warning`, `notice`, `info`, `debug` |
| `--app-name APP` | Application/process name filter |
| `--from TIME` | RFC3339 start timestamp |
| `--to TIME` | RFC3339 end timestamp |
| `--limit N` | Maximum returned rows |
| `--json` | Print JSON response |

### `syslog tail`

Return recent log entries, optionally filtered by host, source, or app.

```bash
syslog tail -n 20
syslog tail 50 --hostname nas --app-name kernel
```

Flags:

| Flag | Description |
| --- | --- |
| positional `N` | Number of rows to return |
| `-n N`, `--n N` | Number of rows to return |
| `--hostname HOST` | Exact claimed hostname filter |
| `--source-ip SOURCE` | Exact source identifier filter |
| `--app-name APP` | Application/process name filter |
| `--json` | Print JSON response |

### `syslog errors`

Summarize error and warning counts by host and severity.

```bash
syslog errors
syslog errors --from 2026-01-01T00:00:00Z --to 2026-01-02T00:00:00Z --json
```

Flags:

| Flag | Description |
| --- | --- |
| `--from TIME` | RFC3339 start timestamp |
| `--to TIME` | RFC3339 end timestamp |
| `--json` | Print JSON response |

### `syslog hosts`

List all known hosts with log counts and last-seen timestamps.

```bash
syslog hosts
syslog hosts --json
```

### `syslog sessions`

List AI transcript sessions grouped by project.

```bash
syslog sessions --project /home/jmagar/workspace/syslog-mcp --limit 20
```

Flags:

| Flag | Description |
| --- | --- |
| `--project PATH` | Exact project path filter |
| `--tool TOOL` | AI tool filter: `claude`, `codex`, or `gemini` |
| `--hostname HOST` | Filter by host |
| `--from TIME` | RFC3339 start timestamp |
| `--to TIME` | RFC3339 end timestamp |
| `--limit N` | Maximum returned rows |
| `--json` | Print JSON response |

### `syslog ai search`

Ranked grouped session search across AI transcript rows.

```bash
syslog ai search authentication --tool claude --limit 10
```

Human output now states that grouping is computed over the newest matching
candidate window. JSON includes `total_candidates`, `candidate_rows`,
`candidate_cap`, `candidate_window_truncated`, and `truncated`; when the
candidate window is truncated, narrow with `--project`, `--tool`, `--from`, or
`--to` for exact grouping within that filter.

### `syslog ai abuse`

Detect abuse in AI transcript rows and return surrounding rows from the same
AI session.

```bash
syslog ai abuse --project /home/jmagar/workspace/syslog-mcp --limit 10 --before 3 --after 3
syslog ai abuse --tool codex --term dang --term heck --json
```

By default this uses the built-in abuse list and returns 2 rows before and
after each hit. Use repeated `--term WORD` flags to replace the built-in list
with a custom detector. JSON includes `candidate_rows`, `candidate_cap`,
`candidate_window_truncated`, `truncated`, and `matches[].{term,entry,before,after}`.

### `syslog ai incidents`

Group abuse hits into scored incident candidates.

```bash
syslog ai incidents --project /home/jmagar/workspace/syslog-mcp --limit 10
syslog ai incidents --tool codex --term dang --term heck --json
```

Use `--window-minutes` to change how nearby abuse hits are grouped into one
incident. JSON includes `total_incidents`, `candidate_rows`, `candidate_cap`,
`candidate_window_truncated`, `truncated`, and `incidents[]`.

### `syslog ai investigate`

Expand top incidents into deterministic evidence bundles without calling an LLM.

```bash
syslog ai investigate --project /home/jmagar/workspace/syslog-mcp --limit 3
syslog ai investigate --correlation-window-minutes 15 --json
```

Each evidence bundle includes the incident, anchor transcript rows, same-session
before/after context, nearby non-AI logs, and nearby warning-or-higher logs.
The public command expands at most 10 incidents per run.

### `syslog ai assess`

Fetch one incident evidence bundle and run the local Gemini CLI to produce a
Markdown frustration assessment.

```bash
syslog ai incidents --limit 10
syslog ai assess inc-f9a1d8e70cad13e6 --limit 3
syslog ai assess inc-f9a1d8e70cad13e6 --model gemini-3.1-flash-lite-preview --json
```

`assess` is local-only and rejects `--http` because it spawns Gemini on the
local host. It can assess any incident ID returned by `syslog ai incidents`
within the incident-list cap, even when that incident is outside the top 10
investigation bundles.

### `syslog ai blocks`

Bucket AI activity into 5-hour UTC windows.

```bash
syslog ai blocks --project /home/jmagar/workspace/syslog-mcp
```

When `--from` is omitted, usage blocks default to the last 30 days. Returned
JSON includes `total_blocks` and `truncated`; at most 1000 buckets are returned.

### `syslog ai context`

Summarize one AI project path.

```bash
syslog ai context --project /home/jmagar/workspace/syslog-mcp --limit 5
```

Recent representative entries are capped at 20 rows, and message snippets are
bounded to 256 characters for predictable MCP/CLI payload size.

### `syslog ai correlate`

Cross-reference AI transcript rows against nearby non-AI logs.

```bash
syslog ai correlate --project /home/jmagar/workspace/syslog-mcp --limit 5
syslog ai correlate --ai-query deploy --log-query container --window-minutes 10 --severity-min warning --json
```

The AI side uses transcript rows as anchors. The related log side searches the
normal log corpus inside each anchor window and excludes AI transcript rows, so
the command surfaces host, Docker, OTLP, and syslog events around the session
without duplicating the transcript stream itself.

### `syslog ai tools`

List distinct AI tools with counts.

```bash
syslog ai tools --json
```

Returned JSON includes `total_tools` and `truncated`; at most 100 tools are
returned.

### `syslog ai projects`

List distinct AI projects with counts.

```bash
syslog ai projects --tool claude
```

Returned JSON includes `total_projects` and `truncated`; at most 200 projects
are returned.

### `syslog ai index`

Explicitly scan local transcript roots (`~/.claude/projects`, `~/.codex/sessions`) or one `--path`.

```bash
syslog ai index
syslog ai index --path ~/.claude/projects
syslog ai index --since 2026-05-14T00:00:00Z
syslog ai index --path ~/.codex/sessions --force
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

### `syslog ai add`

Ingest one explicit transcript file.

```bash
syslog ai add --file ~/.claude/projects/example/session.jsonl
syslog ai add --file ~/.codex/sessions/2026/05/14/session.jsonl --force
```

`--force` reimports that one transcript from scratch without leaving duplicate
log rows.

### `syslog ai watch`

Watch local Claude/Codex transcript roots and index stable changed `.jsonl`
files as they are written.

```bash
syslog ai watch
syslog ai watch --path ~/.claude/projects --no-initial-scan
syslog ai watch --debounce-ms 750 --settle-ms 500 --max-retries 5 --json
```

The watcher is a host-local helper, not part of the Docker Compose runtime. It
reuses the same scanner root policy, file support checks, checkpoints,
append-offset indexing, duplicate suppression, parse-error persistence, and
storage guardrails as `syslog ai index` and `syslog ai add`. The watcher only
coalesces filesystem events, waits for files to stabilize, and retries
transient parse/storage/file errors up to the configured cap.

### `syslog ai checkpoints`

Inspect structured scanner checkpoints without opening SQLite directly.

```bash
syslog ai checkpoints --limit 20
syslog ai checkpoints --errors --json
syslog ai checkpoints --missing
```

The output shows source kind, imported record count, last successful checkpoint,
missing-source status, parse error count, and the last parser/indexing error
when present.

### `syslog ai errors`

Inspect persisted transcript parser errors.

```bash
syslog ai errors --limit 20
syslog ai errors --json
```

Errors include source path, source kind, line number, timestamp, and a bounded
scrubbed preview so parser failures can be investigated without opening the
database directly.

### `syslog ai prune-checkpoints`

Remove checkpoints for transcript files that no longer exist.

```bash
syslog ai prune-checkpoints --missing --dry-run
syslog ai prune-checkpoints --missing --limit 100
```

Pruning is deliberately limited to `--missing` checkpoints. It removes scanner
source metadata, import identities, and parse-error rows for missing files; it
does not delete already imported log rows.

### `syslog ai doctor`

Summarize the local AI indexing state.

```bash
syslog ai doctor
syslog ai doctor --json
syslog ai doctor --strict-permissions --json
```

The doctor reports the DB path in use, whether `~/.claude/projects` and
`~/.codex/sessions` exist, whether they are readable/writable by the current
user, owner uid/gid, mode, checkpoint counts, missing checkpoint counts,
imported record count, parse error count, and the newest indexed transcript.
Without `--strict-permissions`, this is a report-only command. With
`--strict-permissions`, it exits non-zero when either transcript root is
missing, unreadable, unwritable, or owned by another user.

### `syslog ai watch-status`

Inspect the supported user-systemd watcher without reading systemd internals by
hand.

```bash
syslog ai watch-status
syslog ai watch-status --json
```

The status command reports `syslog-ai-watch.service` active/enabled state, main
PID, ExecStart, and the latest bounded journal lines. It uses the same user bus
fallback as setup commands, so it still works from shells or tool environments
that do not export `DBUS_SESSION_BUS_ADDRESS`.

### `syslog ai smoke-watch`

Run a bounded live smoke test of the host-local watcher. The command writes a
temporary Claude transcript under `~/.claude/projects`, waits for the watcher to
ingest it into the configured database, deletes the temp file, then waits for
the missing-checkpoint pruner to clear scanner metadata.

```bash
syslog ai smoke-watch
syslog ai smoke-watch --json
```

This is a live command. It requires `syslog-ai-watch.service` to be running and
writing to the same `SYSLOG_MCP_DB_PATH` used by the CLI process.

### `syslog shell index`

Backfill local shell history into the main log corpus.

```bash
syslog shell index --path ~/.zsh_history
syslog shell index --path ~/.zsh_history --shell zsh --json
```

The importer currently supports zsh extended history lines in the
`: <epoch>:<duration>;<command>` format. Plain history lines without timestamps
are counted as skipped because they cannot be correlated reliably. Commands are
scrubbed before storage, written with `source_kind="shell-history"`, and use
`source_ip` identities shaped like `shell-history://<hostname>/<user>/<shell>`.
Rows are deduped by source identity, timestamp, and scrubbed command text, and
the importer records a private byte-offset cursor under the syslog-mcp state
directory so repeated imports only read newly appended history.

### `syslog agent-command`

Capture shell commands launched by agent tools, then ingest the private JSONL
spool into SQLite.

```bash
syslog setup agent-command install
export CLAUDE_CODE_SHELL_PREFIX="$HOME/.local/bin/syslog-agent-command-wrapper"

syslog agent-command ingest-spool --path ~/.local/state/syslog-mcp/agent-command.jsonl
syslog agent-command wrap --spool ~/.local/state/syslog-mcp/agent-command.jsonl -- cargo test
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

### `syslog setup ai-watch-service`

Install, remove, or inspect the supported host-local user-systemd watcher for
near-real-time transcript ingestion.

```bash
syslog setup ai-watch-service install
syslog setup ai-watch-service check --json
syslog setup ai-watch-service remove
```

Install resolves an absolute `syslog` binary and a concrete SQLite DB path,
writes a private environment file under `~/.config/syslog-mcp/`, runs one
initial `syslog ai index --json` phase, disables the older polling timer, and
starts `syslog-ai-watch.service` with `syslog ai watch --no-initial-scan
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
syslog ai errors --limit 20
syslog ai checkpoints --errors
syslog ai index --json
```

Storage-blocked writes, invalid JSON from the indexer, command failures, stale
unit content, permission failures, and failed `systemctl enable --now` phases
remain blocking errors. Installing the watch service disables the older
`syslog-ai-index.timer` to avoid duplicate background ingestion loops.

### `syslog setup debug-wrapper`

Install, remove, or inspect the host-local debug wrapper at
`~/.local/bin/syslog`.

```bash
syslog setup debug-wrapper install
syslog setup debug-wrapper check --json
syslog setup debug-wrapper remove
```

The wrapper is intentionally machine-local. It `cd`s into the configured repo
or worktree, builds `cargo build --bin syslog` into `.cache/cargo`, then execs
the fresh debug binary. For non-server commands it defaults Docker ingest off
and bearer auth mode on, so regular CLI checks do not accidentally start
container-log ingestion or OAuth-only config paths. Override the source checkout
with `SYSLOG_MCP_REPO=/path/to/syslog-mcp syslog ...`.

### `syslog setup debug-compose`

Install, remove, or inspect the local debug Compose override under
`~/.syslog-mcp/compose/docker-compose.override.yml`.

```bash
syslog setup debug-compose install
syslog setup debug-compose check --json
syslog setup debug-compose remove
```

The override is machine-local. It points the canonical Docker Compose project at
the current repo/worktree and builds the `syslog-mcp:local-debug` image with the
debug profile. This keeps `docker compose up -d --build` aligned with the same
code that the host debug wrapper builds. `syslog setup` also writes
`COMPOSE_PROJECT_NAME=syslog-jmagar-lab` to the setup `.env`, so direct
`docker compose` commands target the canonical project instead of a cwd-derived
project name.

### `syslog setup doctor`

Run the repo-owned local setup checks as one command.

```bash
syslog setup doctor
syslog setup doctor --json
```

The doctor checks setup directories, `.env`, Compose assets, the debug wrapper,
the debug Compose override, transcript-root permissions, disabled legacy index
timer state, active/enabled watcher state, and container freshness via
`scripts/check-runtime-current.sh --allow-local-image`.

### `syslog deploy`

Run the Compose-backed deployment workflow using operator-facing names.
`preflight` and `local` call the same setup engine as `syslog setup check`
and `syslog setup repair`; `remote` uses SSH plus the shared setup assets.

```bash
syslog deploy preflight
syslog deploy preflight --json
syslog deploy local
syslog deploy local --dry-run --json
syslog deploy remote tootie --dry-run
syslog deploy remote tootie --json
```

`deploy preflight` and `deploy local --dry-run` do not mutate Docker state.
`deploy local` repairs `~/.syslog-mcp/.env`, rewrites managed Compose assets,
pulls the configured image, starts the stack, and checks `/health`.
`deploy remote` uses SSH and Docker Compose on the target host. Non-dry-run
remote deploy writes/replaces `~/.syslog-mcp/.env`, the managed Compose YAML,
and `config/Dockerfile` on the target; set token/env values in the local
environment before running it when you need to preserve specific values. It is
CLI-only, requires an explicit host argument, and does not add REST or MCP
deploy mutation surfaces.

### `syslog setup ai-index-timer`

Install, remove, or inspect the optional host-local user-systemd polling
fallback that periodically runs `syslog ai index`.

```bash
syslog setup ai-index-timer install
syslog setup ai-index-timer check --json
syslog setup ai-index-timer remove
```

This helper is intentionally not part of the Docker container. It scans
host-local transcript roots (`~/.claude/projects`, `~/.codex/sessions`) using a
host `syslog` binary, then writes to the configured SQLite DB. Prefer
`syslog setup ai-watch-service install` for normal use; the watcher install
disables this timer to avoid duplicate background ingestion loops.

### `syslog doctor binary`

Check whether the shell binary and running container line up with this repo.

```bash
syslog doctor binary
syslog doctor binary --json
```

The doctor reports the current executable, `syslog` resolved from `PATH`, repo
version, container version when Docker is available, and the result of
`scripts/check-runtime-current.sh`.

For a one-command live check of the AI transcript workflow, run:

```bash
bash scripts/smoke-ai.sh
bash scripts/smoke-ai-mcp.sh
```

The smoke scripts resolve `SYSLOG_BIN` first, then `syslog` on `PATH`, then the
repo-local debug binary at `target/debug/syslog`.

With `syslog-ai-watch.service` installed, new transcript lines usually become
searchable within a few seconds of the writer closing or flushing the file.
Imported transcript messages are scrubbed for known credential/token patterns
before storage and FTS indexing, but scrubbing is best-effort. Raw log actions
can still expose scrubbed AI messages and local `ai_transcript_path` values, so
do not expose the database or MCP endpoint to clients that should not see local
AI session content.

### `syslog correlate`

Find related events around a reference timestamp. Results are grouped by host.

```bash
syslog correlate --reference-time 2026-01-01T12:00:00Z --window-minutes 10
syslog correlate 2026-01-01T12:00:00Z --severity-min err --query timeout --limit 50
```

Flags:

| Flag | Description |
| --- | --- |
| positional reference time | RFC3339 center timestamp |
| `--reference-time TIME` | RFC3339 center timestamp |
| `--window-minutes N` | Minutes before and after the reference time |
| `--severity-min LEVEL` | Minimum severity to include |
| `--hostname HOST` | Exact claimed hostname filter |
| `--source-ip SOURCE` | Exact source identifier filter |
| `--query FTS` | Optional FTS5 query |
| `--limit N` | Maximum total events |
| `--json` | Print JSON response |

### `syslog stats`

Print database and storage guardrail metrics.

```bash
syslog stats
syslog stats --json
```

### `syslog db status`

Print SQLite maintenance state for the configured database.

```bash
syslog db status
syslog db status --json
```

The status includes page counts, freelist count, page size, logical and physical
database size, WAL/SHM sidecar sizes when present, journal mode, auto-vacuum
mode, and no integrity scan. Use `syslog db integrity` for the full SQLite
integrity check on large databases.

### `syslog db integrity`

Run `PRAGMA integrity_check` against the configured database.

```bash
syslog db integrity
syslog db integrity --json
```

The command exits non-zero if SQLite reports anything other than `ok`.

### `syslog db checkpoint`

Run a WAL checkpoint.

```bash
syslog db checkpoint
syslog db checkpoint --mode full
syslog db checkpoint --mode truncate --json
```

Supported modes are `passive`, `full`, `restart`, and `truncate`. The command
exits non-zero if SQLite reports the checkpoint as busy.

### `syslog db vacuum`

Run SQLite vacuum maintenance.

```bash
syslog db vacuum
syslog db vacuum --pages 5000
syslog db vacuum --full
```

The default is `PRAGMA incremental_vacuum(1000)`. `--full` runs `VACUUM` and can
take longer on large databases.

### `syslog db backup`

Create a WAL-safe SQLite backup using the `sqlite3` CLI `.backup` command.

```bash
syslog db backup
syslog db backup --output ~/.syslog-mcp/backups
syslog db backup --output /tmp/syslog-copy.db --json
```

When `--output` is a directory or omitted, the command writes a timestamped
`syslog-YYYY-MM-DD-HHMMSS.db` backup. When `--output` has a file extension, it
is used as the exact destination file.

### `syslog compose`

Diagnose and manage the Docker Compose deployment without opening the SQLite
database.

```bash
syslog compose doctor
syslog compose status --json
syslog compose pull
syslog compose up
syslog compose restart
syslog compose logs --tail 20
syslog compose down --yes
```

Common target flags:

| Flag | Description |
| --- | --- |
| `--compose-file FILE` | Explicit Compose file |
| `--project-dir DIR` | Explicit Compose project directory |
| `--project-name NAME` | Compose project name, only safe with a file/dir or live labels |
| `--service NAME` | Compose service name, default `syslog-mcp` |
| `--container NAME` | Container name, default `syslog-mcp` |
| `--json` | Print JSON response |

Mutation flags:

| Flag | Description |
| --- | --- |
| `--dry-run` | Resolve and preflight without running Docker |
| `--allow-cwd-target` | Permit cwd `docker-compose.yml` fallback for mutation |
| `--yes` | Required for non-interactive destructive `down` |

`syslog compose` refuses ambiguous target discovery, mismatched requested
project/service selectors, cwd fallback without confirmation,
project-name-only mutations, missing Compose files, legacy service conflicts,
non-target listeners on syslog ports, and destructive service stop without
`--yes`. `down` is intentionally service-scoped (`docker compose stop
syslog-mcp`), not a project-wide `docker compose down`.

## Relationship to MCP

The direct CLI and MCP tool share the same business layer. Transport adapters
own argument parsing and rendering; shared defaults, limits, validation, audit
identity, and safety policy belong in `SyslogService` or service-owned request
models.

| CLI command | MCP action |
| --- | --- |
| `syslog search` | `syslog` with `action="search"` |
| `syslog tail` | `syslog` with `action="tail"` |
| `syslog errors` | `syslog` with `action="errors"` |
| `syslog hosts` | `syslog` with `action="hosts"` |
| `syslog sessions` | `syslog` with `action="sessions"` |
| `syslog ai search` | `syslog` with `action="search_sessions"` |
| `syslog ai abuse` | `syslog` with `action="abuse"` |
| `syslog ai correlate` | `syslog` with `action="ai_correlate"` |
| `syslog ai blocks` | `syslog` with `action="usage_blocks"` |
| `syslog ai context` | `syslog` with `action="project_context"` |
| `syslog ai tools` | `syslog` with `action="list_ai_tools"` |
| `syslog ai projects` | `syslog` with `action="list_ai_projects"` |
| `syslog correlate` | `syslog` with `action="correlate"` |
| `syslog stats` | `syslog` with `action="stats"` |
| `syslog compose status` | `syslog` with `action="compose_status"` (redacted read-only projection only) |
| `syslog compose doctor` | `syslog` with `action="compose_doctor"` (redacted read-only projection only) |

The MCP-only `status` and `help` actions are runtime/protocol helpers, not
direct database queries. Compose mutations (`up`, `down`, `restart`, `pull`,
`logs`) are CLI-only and are not exposed over MCP.

Use direct CLI mode for terminal queries and scripts on a host that can read the
SQLite database. Use MCP HTTP or `syslog mcp` when an MCP client needs tool
access.

## See also

- [README.md](../README.md) -- project overview and quick examples
- [mcp/TOOLS.md](mcp/TOOLS.md) -- MCP action reference
- [mcp/TRANSPORT.md](mcp/TRANSPORT.md) -- HTTP and stdio MCP transports
- [CONFIG.md](CONFIG.md) -- config file and environment reference
