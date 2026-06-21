# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.33.1] - 2026-06-20

### Fixed

- **Gemini transcript indexing no longer loses data on a single bad record.** A
  malformed per-message timestamp is now recorded as a per-record parse error and
  skipped while the rest of the file still ingests, instead of aborting the whole
  file and re-failing it forever. A chat file missing its `messages` array is
  surfaced as a recorded parse error rather than being silently checkpointed as
  "fully indexed", and messages with no extractable text are counted/logged. All
  failures now flow through the checkpoint store, so they appear in the AI
  doctor's `checkpoint_error_count` like every other source kind.
- **`cortex ai doctor --strict-permissions` now gates `gemini_root`.** Bad
  permissions on `~/.gemini/tmp` previously passed the strict check, which only
  inspected the Claude and Codex roots.
- Removed the unreachable `gemini::parse_line` dead code path; Gemini sessions are
  whole-file JSON and never flow through the per-line dispatch.

### Changed

- The MCP `source_kinds` schema enum is now generated from
  `SourceKind::all_wire_names()` so the wire contract can never drift from the
  Rust enum, with a regression test asserting the two stay in sync.
- Split the per-transcript-root printing in `cortex ai doctor` into a shared
  helper, bringing `src/cli/output_ai.rs` back under the module-size lint.

## [1.33.0] - 2026-06-20

### Added

- **Gemini CLI transcript watcher.** Discover, index, and report
  `~/.gemini/tmp/**/chats/session-*.json` sessions alongside the existing Claude
  and Codex transcript ingestion, including a `gemini_root` in the AI doctor.

## [1.32.5] - 2026-06-20

### Fixed

- **Topic-correlation source-kind filters now match the advertised API contract.**
  MCP/REST callers may pass `source_kinds` as either a single string or an array,
  invalid source-kind values now fail fast instead of silently widening the query,
  and the schema/help text names the exact kebab-case source kinds that runtime
  validation accepts.
- **Unaddressed warning noise no longer hides real incidents.** Health/probe
  chatter is recorded by the scanner, presentation filtering is narrowed to
  warning-only exact probe shapes, and `unaddressed_errors` pages through filtered
  candidates so benign warning noise does not underfill the requested result set.
- **Remote Docker event-stream unsupported hosts are visible in observability.**
  Hosts without a remote `docker` binary now emit a warning and update the remote
  event-stream failure counters instead of disappearing as an info-only loop exit.

## [1.32.4] - 2026-06-19

### Fixed

- **`cortex deploy agent` upgrades are now fully config-preserving.** 1.32.3 began
  carrying forward custom env (file-tails) and mounts, but the heartbeat **token**,
  **target**, and the **docker/journald** booleans were still rebuilt from flags —
  so a flag-less redeploy would drop the token (breaking heartbeat auth) and reset
  docker ingest to off. The Unraid deploy now resolves every managed key with
  **flag > persisted > default** precedence: an explicit flag wins, otherwise the
  value from the host's `heartbeat-agent.env` is kept, otherwise a built-in default
  fills first-time deploys. `--docker`/`--journald` became `Option<bool>` so "not
  passed" is distinct from "false" (and thus preserves the existing setting);
  journald stays forced-false inside the container. Custom keys (file-tails) and
  non-standard mounts continue to carry through. New pure helpers
  (`parse_env_file`, `resolve_agent_env`) with unit tests covering flag-less
  preservation, flag override, and first-deploy defaults.

## [1.32.3] - 2026-06-19

### Fixed

- **Versioned GHCR image was never published for 1.32.x.** `docker-publish.yml`
  derives the version image tag from `type=semver`, which only fires on `v*`
  git-tag pushes — but the auto-tag workflow creates those tags with the default
  `GITHUB_TOKEN`, and GitHub does not trigger workflows from `GITHUB_TOKEN`-pushed
  tags (loop prevention). So `ghcr.io/jmagar/cortex:1.32.x` was never built, and
  the fleet agents (pinned to `ghcr:1.31.2`) had no image to update to. The build
  now also tags the image with the `Cargo.toml` version on default-branch pushes
  (`type=raw,value=<version>,enable={{is_default_branch}}`), so every merge to
  `main` publishes `ghcr:<version>` independent of the tag trigger.
- **`cortex deploy agent` reset custom agent config on every redeploy.** It built
  the container env from a fixed flag set, overwrote the persisted
  `heartbeat-agent.env`, and mounted only the three standard paths — so a custom
  `CORTEX_AGENT_FILE_TAILS` (e.g. a host's Plex log) **and** the host mount it
  needs were silently dropped on upgrade, even though the value was sitting in the
  persisted env file. The Unraid deploy is now config-preserving: it carries
  forward any non-managed env keys from the existing `heartbeat-agent.env` and any
  non-standard mounts from the running container, so upgrading the binary keeps
  per-host customizations. (Managed keys still come from the deploy flags.)

## [1.32.2] - 2026-06-19

### Fixed

- **Stale test assertions for the HTTP-flag guard (regression from 1.32.1).**
  1.32.1 reworded the "global HTTP flags only apply to query commands" error, but
  two existing `Mode::parse` tests (`mode_parse_rejects_http_flag_on_serve_mcp`,
  `..._on_deploy`) still asserted the old text and failed. The guard behavior is
  unchanged (HTTP flags are still rejected for `serve mcp`/`deploy`); only the
  assertions are updated to match the new message. No runtime change.

## [1.32.1] - 2026-06-19

### Fixed

- **`cortex topic-correlate` panicked from the CLI.** `topic-correlate` is in
  `TOP_LEVEL_COMMANDS` and `parse_command`, but was missing from `Mode::parse`'s
  top-level command gate in `main.rs`, so it parsed successfully and then hit the
  `unreachable!()` fallthrough — `cortex topic-correlate <topic>` panicked with
  `internal error: entered unreachable code`. Added it to the allowlist (it now
  also accepts the global `--http`/`--token` flags like its siblings) with a
  `Mode::parse` regression test covering both the single-term and multi-term
  (`squirts dockersocket`) forms.
- **`cortex correlate <non-time>` gave a cryptic error.** `correlate`'s sole
  positional is a reference *time*, so `cortex correlate squirts dockersocket`
  fed `squirts` into the time parser and surfaced
  `unrecognized time value 'squirts'`. The error is now actionable: it explains
  the positional is a reference time and points at
  `cortex topic-correlate squirts …` (entity correlation) or
  `--reference-time <time> --host squirts`.
- **`silent_hosts` flagged dormant case/FQDN host variants as silent.** It read
  the raw, case-sensitive `hosts` table, so a quiet `shart` identity was reported
  silent even while the live `SHART` kept forwarding. It now routes through
  `list_hosts()` (the same case/FQDN dedupe added in 1.32.0, taking the latest
  `last_seen`), so the merged machine is correctly considered alive; genuinely
  dormant hosts are still flagged.
- **`clock_skew` reported one machine once per casing.** Its `GROUP BY hostname`
  was case-sensitive, so `SHART`/`shart` showed as two skew rows. The case/FQDN
  canonicalization is now factored into a shared `canonical_host_keys` helper
  (reused by `hosts`) and applied to `clock_skew`, merging variants into one row
  with summed samples, a sample-weighted average skew, and widened min/max.
- **The "HTTP flags only apply to query commands" error listed a stale command
  set.** Its hardcoded enumeration had drifted (it omitted `topic-correlate`,
  `correlate-state`, `silent-hosts`, `clock-skew`, `map`, … — implying those
  reject `--http`). It no longer enumerates: it names the offending argument and
  points at `--server <url>` / `--http=<url>`, so it can't drift again.

### Added

- **`--http=<url>` shortcut.** Enables HTTP transport and sets the server in one
  curl-style flag (e.g. `cortex --http=http://host:3100 search foo`). Bare
  `--http` is unchanged (a value-less flag, so `cortex --http search foo` still
  treats `search` as the command); a server URL after bare `--http` was silently
  left as a stray positional before.

## [1.32.0] - 2026-06-19

### Fixed

- **Relative time now works over MCP/REST, not just the CLI.** `topic_correlate`
  (and ~10 other actions that bind `since`/`until`/`reference_time`:
  `ai_correlate`, `incident_context`, `correlate_state`, the `ai *` family, …)
  rejected relative/keyword forms like `30m`/`yesterday` over MCP with
  `Invalid since '30m': premature end of input`, because the relative-time
  normalizer was wired only into the CLI argument parser. The normalizer moved
  into the library (`cortex::app::parse_time_arg`) and the service-layer
  `parse_required_timestamp` now routes every value through it, so all entry
  points (CLI, MCP, REST) accept `1h`/`30m`/`2d`/`90s`/`now`/`today`/`yesterday`/
  bare dates/RFC3339 uniformly. RFC3339 input passes through unchanged
  (normalization is idempotent). The CLI-only `cli::timearg` module was removed.

### Changed

- **`hosts` deduplicates case and FQDN variants of the same machine.** The host
  list collapses `SHART`/`shart` (case) and folds an FQDN into its short name
  **only when that short name independently exists** as a host
  (`tootie.manatee-triceratops.ts.net` → `tootie`, but `host.docker.internal` is
  left alone — no bare `host`). Merged rows sum `log_count` and span the widest
  first/last-seen window. Ambiguous self-identifiers (`localhost`, empty
  hostname) are left untouched pending `source_ip`-based resolution.
- **`graph` `around` neighborhoods fair-share the payload budget across neighbor
  types.** A host's neighborhood was sorted purely by recency, so high-churn
  `error_signature` edges (re-touched every error scan) crowded out apps and
  source_ips under the budget (a host with ~46 apps surfaced only ~32).
  Selection is now a round-robin across neighbor entity types, so every type
  gets representation; raise `payload_budget` for a fuller view.

### Added

- **`graph` resolves `compose_project` by bare project name.** Compose-project
  canonical keys are host-scoped (`dookie:axon`), so
  `entity_type=compose_project key=axon` previously returned nothing. It now
  falls back to project-name resolution: a unique hit resolves, and a project
  running on multiple hosts is returned as `candidates`.

### Documentation

- **cortex skill** gains Time-windows, Graph & correlation, and action-cost-tier
  guidance, and the HTTP-fallback `curl` examples now send the required
  `Accept: application/json, text/event-stream` header (the streamable-HTTP
  transport returns `406 Not Acceptable` without it).

## [1.31.4] - 2026-06-19

### Changed

- Split the analytics CLI dispatch commands (silent-hosts, clock-skew,
  anomalies, compare, apps) — and their unit tests — out of
  `src/cli/dispatch_surface_gap.rs` into a new `dispatch_surface_analytics.rs`
  sibling (with its own sidecar test file), bringing both modules back under
  the 500-line production module-size budget (the file had crossed the limit
  when the graph-correlation epic landed). No behavior change.

## [1.31.3] - 2026-06-19

### Fixed

- **`CORTEX_AGENT_FILE_TAILS` no longer drops malformed entries silently.**
  `parse_file_tails` now emits a `warn` for any entry missing a usable `:TAG`
  suffix (colon-less path, empty path/tag) instead of skipping it silently — a
  typo'd source previously tailed nothing with no diagnostic.

### Tests

- Added coverage surfaced by PR review: `notify_min_severity` config validation
  (rejects unparseable severities — the guardrail the scanner fail-open relies
  on), AdGuard `CID` client field + `IP`>`CID` precedence, and severity-floor
  direction (a signature more severe than the floor still pages; a `warning`
  floor admits warnings).

## [1.31.2] - 2026-06-19

### Changed

- **Containerized host agents now run the published image with the binary baked
  in, instead of a generic `ubuntu:24.04` container with the binary bind-mounted
  from the host.** Both deployment generators — `cortex agent deploy` (Unraid
  `docker run`) and `cortex setup` (compose) — now `docker pull
  ghcr.io/jmagar/cortex:<version>` and run `cortex heartbeat agent` directly from
  the image, pinned to the deploying binary's version for server/agent lockstep.
  The agent container runs `--user 0:0` (it reads root-owned host files —
  `docker.sock`, `/var/log/syslog`) with the image's server healthcheck disabled
  (the agent runs no HTTP server). Only host *data* is mounted now (Docker
  socket, host syslog, appdata for host-id); the binary, the `/opt/cortex/bin`
  dir mount from 1.31.1, the `ubuntu:24.04` base, and the `/etc/ssl/certs` mount
  are all gone. This supersedes the 1.31.1 self-update mount fix: there is no
  host binary to swap, so the failure mode it fixed no longer exists. **Deploy
  now requires the matching image tag to be published to the registry first.**

## [1.31.1] - 2026-06-18

### Fixed

- **Host agent can now self-update inside its container.** The generated
  heartbeat-agent deployments (`cortex setup` compose + `cortex agent deploy`
  Unraid `docker run`) mounted the binary as a single read-only file at
  `/usr/local/bin/cortex`. Agent self-update stages the new binary alongside the
  running one and atomic-renames it into place, but a single-file bind mount
  makes that target a mount point, so the rename failed (`EBUSY`/`EXDEV`) and
  every containerized agent was stranded on its build version (observed: tootie
  and shart stuck on 1.30.0 while the server ran 1.31.0). Both generators now
  bind-mount the binary's *directory* writable at `/opt/cortex/bin` and run
  `/opt/cortex/bin/cortex`, so the swap lands on the host and persists across
  restarts.

## [1.31.0] - 2026-06-18

### Changed

- **Recurring-error notifications now have a severity floor (`err` by default).**
  The error scanner records `warning`-level recurring signatures (still
  searchable and ack-able) but no longer pages on them — only `err`/`crit`/
  `alert`/`emerg` signatures fire notifications. Busy dev/service hosts emit
  enormous volumes of recurring warnings that flooded the notification channel
  with non-actionable alerts. Configurable via
  `[error_detection].notify_min_severity` / `CORTEX_ERROR_DETECTION_NOTIFY_MIN_SEVERITY`.

## [1.30.0] - 2026-06-18

### Added

- **Agent multi-file tailing (`CORTEX_AGENT_FILE_TAILS`).** The host agent's
  file forwarder now accepts a comma-separated list of `PATH:TAG` sources and
  forwards each line **raw** under the configured tag (so the cortex parser is
  selected by `app_name`). This lets the agent ship file-only log sources —
  AdGuard's JSON `querylog.json`, SWAG `access.log`, fail2ban, Plex — that are
  neither on container stdout nor in journald, **retiring the per-host rsyslog
  imfile drop-ins**. The existing `CORTEX_AGENT_SYSLOG_FILE` (RFC 3164
  syslog-format single file) is unchanged.

### Fixed

- **AdGuard client IP now parsed from the file query log.** AdGuard Home ≥0.107
  records the client address in the `IP` field of `querylog.json`, but the
  parser only read `Client`/`client`. It now also reads `IP`/`CID`, so DNS
  queries produce `client` metadata and the graph can build `device --accessed-->
  domain` edges.

## [1.29.0] - 2026-06-18

### Added

- **Graph-anchored universal correlation (`topic_correlate`).** New MCP action and
  `cortex topic-correlate <topic>` CLI subcommand: resolve a free-text topic
  (e.g. `axon`, `dookie dns adguard`) to investigation-graph entities, expand the
  graph N hops, and return a unified timeline of every related log annotated by
  source kind and discovery lane, plus resolved entities, graph expansion,
  discovered hosts, and heartbeat pressure. Graph-first query order (traverse →
  entity set → log join) backed by a new `(hostname, app_name, timestamp)`
  covering index.
- **Session-anchored graph correlation for `ai_correlate`.** When a `session_id`
  is supplied, `ai_correlate` adds a `graph_correlation` block: it traverses the
  graph from the session entity to discover related hosts and fans logs out across
  all source kinds within the session's time bounds, annotated by lane
  (`agent_command` / `shell_history` / `graph:host:<host>`) with heartbeat
  summaries. Additive — existing fields and the time-windowed path are unchanged.
- **New graph topology projected from logs:** explicit agent-command → AI-session
  edges (with cwd-based project inference that converges agent-command and
  transcript sessions); `git_commit` entities linking sessions/hosts to commits;
  `user`/`device` identity entities with `authenticated_as`/`accessed` edges from
  AdGuard, Authelia, and shell-history rows; and a log-derived `compose_config`
  path (`compose_project --defines_service--> service`).
- **Reusable N-hop graph traversal** (`graph_walk_n_hops`) and graph-anchored log
  fan-out (`search_logs_from_graph_related_entities`).

### Changed

- **Confidence model upgraded** to noisy-OR combination, BEWA same-source
  diminishing returns, and CountTRuCoLa-style per-relationship temporal decay,
  applied at query time in graph_explain chain scoring (no schema change) so stale
  volatile edges fall behind structural ones.
- **Investigation-graph vocabulary widened** (schema migrations 32–36): new
  `git_commit`/`user`/`device` entity types, `authenticated_as`/`accessed`/
  `communicates_with` relationship types, a `refuted` trust level (refuted edges
  are excluded from all traversals), a `correlated` confidence ceiling, and a
  hierarchical v2 reason-code namespace registry (stored v1 values unchanged).

## [1.28.1] - 2026-06-18

### Fixed

- **Recurring-error notifications were unreadable.** `escape_for_notification` stripped `<` and `>` from notification titles/bodies as injection hardening, but the payload is sent as markdown and the error-signature normalizer emits placeholder tokens (`<n>`, `<hex>`, `<ip>`, `<path>`, `<str>`, `<json>`, `<uuid>`, `<ts>`). Stripping the brackets collapsed every signature into unreadable runs (`<n>:<n>:<n>` → `n:n:n`, `<str>` → `str`, `<path>` → `path`). The function now HTML-escapes `&`/`<`/`>` (and keeps the `@` → `＠` mention guard), so placeholders render verbatim while tag/markup injection stays neutralised.

### Added

- **Error-detection exclusion patterns break the notification feedback loop.** New `[error_detection].exclude_patterns` (env `CORTEX_ERROR_DETECTION_EXCLUDE_PATTERNS`, comma-separated): case-insensitive message substrings whose log rows are skipped during scanning — they are never tracked as signatures and never notified, while the scan cursor still advances past them. Defaults cover Apprise's own delivery log lines (`Delivered Stateless Notification`, `Sent Gotify notification`, `POST /notify`), so cortex no longer re-detects its own POST-to-Apprise delivery records (forwarded back in via syslog) as recurring errors and notifies on them in a loop. Set to `[]` to disable.

## [1.28.0] - 2026-06-17

### Added

- **Self-maintaining investigation graph** — the server now keeps the derived graph projection current in-process instead of relying on the local-only `cortex graph rebuild` CLI (which drifted stale on long-running servers and risked dual-writer contention when run as a second process). A new `graph_refresh` background task does a one-time full build on first run, then **incremental delta passes** on an interval (`CORTEX_GRAPH_REFRESH_INTERVAL_SECS`, default 300; `0` disables). Incremental passes project only logs newer than the recorded watermark plus the bounded heartbeat/error-signature snapshots, merging the delta into the live tables by natural key — so steady-state refreshes complete in seconds rather than rescanning all history. The long log scan builds into per-connection TEMP staging without the write lock; only the final merge transaction briefly blocks ingestion, so refreshes are safe while serving (no offline window).

### Changed

- **Versioning system replaced with a declarative `cargo xtask` port of axon's.** Retired the shell scripts (`bump-version.sh`, `check-version-sync.sh`, `check-plugin-manifest-versions.sh`) in favor of a workspace `xtask` crate driven by `release/components.toml` — the single source of truth listing every version-bearing file and how each is read/rewritten. New commands: `cargo xtask bump-version <patch|minor|major>`, `cargo xtask check-version-sync`, and `cargo xtask check-release-versions` (the release gate). The manifest adds a `regex_version` kind covering the `cortex:vX.Y.Z` image tag in `server.json` and the `${CORTEX_VERSION:-X.Y.Z}` default in `docker-compose.prod.yml`, and a `json_no_version` kind enforcing the unversioned-plugin-manifest policy.
- **Default `CORTEX_POOL_SIZE` raised 4 → 8.** The graph projection scheduler can hold one pooled connection for the duration of a full rebuild, so the larger pool preserves read/writer headroom; incremental passes are short.

### Fixed

- **Notification config now fails loud instead of silently dropping.** `validate_notifications_config` rejects `enabled = true` unless BOTH the Apprise API base URL (`apprise_url`) and at least one delivery target (`apprise_urls`) are set — previously a base URL with no targets passed validation and the dispatcher logged "no apprise URLs configured" and dropped every firing. Added `CORTEX_NOTIFICATIONS_APPRISE_URLS` env override; the dev `config.toml` template now ships with notifications disabled.

## [1.27.1] - 2026-06-16

### Fixed

- **Silent wrong results from relative time on `apps`/`clock-skew`/`compare`/`correlate-state`/`host-state`/`notify recent`/`ai *`** — these commands captured their time-window flags (`--since`/`--until`, `--reference-time`, compare's `--a-from`/`--a-to`/`--b-from`/`--b-to`, notify's `--since`) as raw strings and bound them straight into SQL timestamp comparisons. A relative value like `1h` was compared *lexically* against RFC3339 timestamps (`timestamp >= '1h'`), silently returning wrong rows with no error. All such flags now route through a shared `norm_time` helper, so relative/keyword forms normalize to RFC3339 and non-time input is rejected. (`timeline`/`patterns`/`incident`/`correlate` in `parse_logs.rs` are addressed by the Plan 1 review fix.) `service logs` is unaffected — its `--since`/`--until` are forwarded to `docker compose logs`, not bound into cortex SQL.

## [1.27.0] - 2026-06-16

### Added

- **Flagless common case (positionals + smart defaults)** — a bare positional now binds to each action's primary selector: `cortex tail dookie` and `cortex host-state dookie` set `--host`, `cortex search "oom killer"` sets the query. `tail` and `search` apply a default limit of 50 and `errors` defaults to a last-hour window when you omit the flags, so the everyday commands need no flags at all. The positional mapping and defaults are declarative metadata on `ACTION_SPECS` (`positional`/`defaults`), applied through shared `argdefaults` helpers — not per-command special-casing. Per-command `--help` examples now lead with the flagless form.

### Changed

- **`cortex tail <arg>` now treats `<arg>` as a hostname, not a line count.** Use `-n`/`--limit` for the count (e.g. `cortex tail dookie -n 100`). This is the only behavioral break in the positional rework; the old bare-number-as-count form is removed.

## [1.26.1] - 2026-06-16

### Fixed

- **Canonical-rename stragglers** — fixed user-facing strings that still referenced the pre-rename flag/arg vocabulary after the 1.26.0 canonical rename: `file-tail add` usage and required-flag error (`--hostname`→`--host`), the `ai incident-context` required-flag errors (`--from`/`--to`→`--since`/`--until`), the search/abuse/ask-history "scan capped" hints (`--from`/`--to`→`--since`/`--until`), the `file_tails op=add` admin help parameter (`hostname`→`host`), and the `similar_incidents` MCP input example (`app_name`→`app`). Also corrected the `similar_incidents` doc "Response fields" line back to the unchanged output names (`hostname`/`app_name`), matching the actual response contract.

## [1.26.0] - 2026-06-16

### Added

- **Shell completion** — `cortex completions zsh` installs a tab-completion function that completes command names (with descriptions), per-command flags, and **live values** (hostnames, apps, source IDs pulled from the DB, cached ~60s with a 150 ms timeout that degrades silently). Driven by a hidden `cortex __complete` command and the single `ACTION_SPECS` registry.
- **Per-command examples in help** — `cortex <command> --help` now shows copy-paste examples sourced from `ACTION_SPECS`, kept in lockstep with the canonical flags.

### Changed

- **Canonical flag/argument vocabulary (breaking)** — unified the CLI flags and MCP tool-argument names so they match everywhere: `--hostname`→`--host`, `--source-ip`→`--source`, `--app-name`→`--app`, `--from`→`--since`, `--to`→`--until`, `--received-from`→`--received-since`, `--received-to`→`--received-until` (with `-s`/`-n` short forms for severity/limit). The rename spans every CLI command, the MCP tool arguments (the request-arg domain fields were renamed so the wire key = field name, no serde aliases), help text, the bundled skills, and docs. Response/output field names (e.g. a log entry's `hostname`/`source_ip`) are unchanged — the output contract is unaffected.
- `ACTION_SPECS` now carries per-action flag and example metadata; the CLI parser, completion, and help all derive from it.

## [1.25.1] - 2026-06-16

### Added

- **Relative & natural time arguments** — every CLI time flag now accepts relative durations (`1h`, `30m`, `2d`, `90s`), keywords (`now`, `today`, `yesterday`), and `YYYY-MM-DD`/`YYYY-MM-DD HH:MM` forms in addition to RFC3339: `--from`/`--to`/`--received-from`/`--received-to` on `search`/`filter`/`sessions`/`errors`, `--from`/`--to` on `timeline`/`patterns`, `incident --around`, and `correlate --reference-time` (including its positional form). Values are normalized to RFC3339 at parse time (e.g. a trailing `Z` becomes `+00:00`); a non-time value is now rejected with a clear error instead of being passed through to the query layer.
- **`--grep` literal search** — `cortex search --grep "smoke-test"` matches literal text (including hyphenated terms) by wrapping it as a safe FTS5 phrase, bypassing FTS5 operator syntax. Mutually exclusive with a positional query.

### Changed

- **FTS5 query fix-it errors** — searches with an unquoted infix-hyphen term (the `smoke-test` → `smoke NOT test` trap) or an unbalanced quote now return an actionable message suggesting a phrase quote or `--grep`, instead of a raw database error.

### Docs

- Added the CLI ergonomics design spec and three implementation plans under `docs/superpowers/` (this change implements Plan 1; Plans 2–3 cover dynamic completion, the canonical flag rename, and smart defaults/positionals).

## [1.22.0] - 2026-06-15

### Added

- **Heartbeat-coordinated agent auto-update** — the server now keeps fleet agent binaries in lockstep with itself, preventing the version drift that silently broke Docker/syslog forwarding (an agent binary predating unified forwarding). The heartbeat `202` ack carries `server_version` plus an optional `agent_update` directive (`{version, path, sha256}`), and a new authenticated `GET /v1/agent/binary?os=&arch=` streams the server's own running binary (phase 1: `linux/x86_64`). When an agent reports a different version, it downloads the binary over the bearer-authenticated channel, verifies the SHA-256, validates that the staged binary runs and self-reports the expected version, keeps a `.bak`, atomically swaps, and re-execs. Bounded auto-rollback restores the `.bak` if the new binary fails to confirm a healthy heartbeat within 3 restarts. Gated by `CORTEX_AGENT_AUTO_UPDATE` (default on); when disabled, version drift is logged only. Phase 2 (a GitHub-release fallback for windows/non-x86_64 agents) is tracked separately.

## [1.21.6] - 2026-06-15

### Fixed

- Fix the Windows release build, which had been failing since the `file_tail` ingest module landed and was blocking the `publish` job (it `needs` both the Linux and Windows builds). `src/file_tail/path_policy.rs` and `src/file_tail/supervisor.rs` called Unix-only APIs unconditionally — `MetadataExt::dev`/`ino`, `OpenOptionsExt::custom_flags`, and `libc::O_NOFOLLOW` — but `libc`/`rustix` are `[target.'cfg(unix)'.dependencies]`, so the Windows target failed to compile (E0599/E0433). Introduced `src/file_tail/platform.rs`, a small `#[cfg(unix)]`/`#[cfg(windows)]` shim for file identity and no-follow open. Linux behavior is byte-identical (`(st_dev, st_ino)` + `O_NOFOLLOW`); on Windows the no-follow open maps to `FILE_FLAG_OPEN_REPARSE_POINT` and identity-based rotation detection falls back to the content fingerprint + length.

## [1.21.5] - 2026-06-15

### Fixed

- Add a `.gitleaks.toml` so the weekly scheduled full-history Secret Scan stays green. The scan flagged eight placeholder tokens committed in docs and test scripts (the `secret-token-opaque-base64url-32bytes` base64 example in `docs/contracts/*.md`, plus `YOUR_TOKEN`, `bad-token`, and `intentionally-wrong-token-for-testing` in `README.md` and `tests/`). All built-in gitleaks rules remain active; only these exact placeholder strings are allowlisted by secret content, so a real credential on any of those lines would still be caught.

## [1.21.4] - 2026-06-15

### Added

- Expand focused unit coverage across the setup, runtime, inventory, notification, and ingest layers to lift total line coverage to ~80%. New sidecar suites cover setup doctor/agent/first-run/systemd/AI-index/AI-watch/heartbeat-agent helpers, CLI global-flag and local-only routing, command-dispatch guardrails, runtime auth and inventory-refresh seams, inventory device/project/remote-config collectors, notification evaluator/dispatcher/queue/digest SQL helpers, Aurora logging helpers, receiver listener wiring, journald/syslog formatting, and error-signature DB contracts.
- Add a mocked Docker Engine HTTP fixture for the legacy central pull client plus log-frame mapping, doc-drift tests for Docker ingest guidance and coverage tooling, and deterministic admin REST coverage for `/api/file-tails` in the live smoke harness when `CORTEX_API_ADMIN_TOKEN` is set.

### Changed

- Reframe Docker log ingest docs around the host-local cortex agent path, documenting `CORTEX_DOCKER_*` as legacy central-pull compatibility for explicit Docker HTTP endpoints.
- Add repeatable coverage commands (`just coverage`, `just coverage-html`) and a CI coverage-summary job using `cargo-llvm-cov` with nextest.
- Extract the `dispatch_surface_gap` argument-mapper tests into a sidecar `dispatch_surface_gap_tests.rs`, honouring the repo's sidecar-test convention and keeping the production module under the 500-line size limit.
- Run CI tests via `cargo nextest run --locked` (with an explicit `cargo test --doc` step) in the `ci`, `docker-publish`, and `publish-crates` workflows, matching `just test` and the coverage job. The expanded env-mutating setup/heartbeat/plugin tests require per-process isolation, which libtest's shared-process model cannot provide.

### Fixed

- Drop empty host entries from `cortex deploy agent --hosts` comma lists and assert the evaluator future-row age test against `received_at`.
- Relax the graph-inventory lock-scope test timeout so coverage-instrumented builds do not fail on instrumentation overhead while still proving the write lock is released promptly.

## [1.21.3] - 2026-06-15

### Security

- Add a defense-in-depth Content-Security-Policy to the query widget (`img-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'`) capping egress/injection vectors it never uses. `script-src`/`connect-src` are intentionally left open so an MCP Apps host can inject its bridge; document the widget's origin-bound message handling and the `ui://` `targetOrigin: "*"` constraint in `docs/mcp/MCPUI.md`.
- Stop interpolating server-controlled values into `python3 -c` programs in `scripts/smoke-test.sh`: `assert_gte` now reads the compared value via stdin, and `json_get` documents its literal-accessor-only contract.

### Changed

- Factor the repeated PUBLIC_URL `McpConfig` literals in the rmcp server tests into a `public_url_config()` helper, and fix a stray tab in `scripts/smoke-test.sh`.

## [1.21.2] - 2026-06-15

### Changed

- Consolidate duplicated rmcp server test helpers: `test_state`/`mounted_state` now delegate to a single `make_state(auth_policy)` builder, and the byte-identical `rmcp_router_no_auth_middleware` is folded into `rmcp_router`. Test-only; no behavior change.

## [1.21.1] - 2026-06-15

### Added

- Document the MCP Apps query widget as progressive enhancement (README + `docs/SETUP.md`): canonical URI `ui://cortex/query-widget`, `text/html;profile=mcp-app` MIME, host-agnostic JSON-RPC verification snippets, and an explicit note that non-UI hosts keep receiving normal text/JSON tool results.
- Add host-agnostic widget wire-contract smoke coverage to `scripts/smoke-test.sh`: `resources/list` exposure, `resources/read` MIME + anchors, `tools/list` `_meta.ui.resourceUri`, and `tools/call action=search` returning both `structuredContent` and text. No browser, Node, or named UI host required.

## [1.21.0] - 2026-06-15

### Added

- Build out the MCP Apps query widget (`ui://cortex/query-widget`) into a self-contained, dependency-free interactive UI. It exposes an FTS5 query input plus hostname/severity/limit filters, calls the `cortex` tool with `action=search` over an MCP Apps host bridge (`window.openai.callTool` when present, otherwise an mcp-ui `postMessage` adapter), and renders results in a compact Aurora-dark table with idle/loading/empty/error/bridge-unavailable states. Replaces the previous placeholder form.

### Security

- Render all log fields in the query widget via `textContent`, preventing HTML/script injection from untrusted log message contents.

## [1.20.1] - 2026-06-12

### Changed

- Require an explicit `hostname` when adding managed file-tail sources so rows are attributed to the intended host instead of falling back to the Cortex container identity.
- Report file-tail durable-writer backpressure in the `status` action and mark overall status as `degraded` when tailers are blocked on writer ack.

### Fixed

- Prevent duplicate file-tail `add` calls from resetting checkpoints, avoid retrying stateful HTTP file-tail mutations after 503 responses, and make committed-but-reconcile-failed mutations explicit.
- Harden file-tail resume and rotation handling: stale checkpoints now restart replacement files from the beginning, same-inode copytruncate/regrow is detected by prefix fingerprint, and rename-create rotation gets a short old-file drain window before switching.
- Fix file-tail live smoke harness permissions so non-root containers can read the mounted smoke log, and tighten admin-scope predicates across smoke scripts.
- Canonicalize configured file-tail allowed roots so symlinked operational roots validate against canonical file paths.

## [1.20.0] - 2026-06-11

### Added

- Added managed file-tail ingest sources with CLI, REST API, and MCP control.
- Added `file-tail` source kind for rows ingested from local log files.
- Documented SWAG, fail2ban, Authelia, and AdGuard file-tail recipes for replacing rsyslog `imfile` drop-ins.

## [1.19.0] - 2026-06-11

### Added

- Heartbeat agents can now tail a host syslog file via `CORTEX_AGENT_SYSLOG_FILE`, wrapping new RFC 3164-style lines as RFC 5424 for Cortex without replaying historical backlog on startup.
- Unraid heartbeat-agent deployment now mounts `/var/log/syslog` and `/var/run/docker.sock`, sets `CORTEX_AGENT_SYSLOG_FILE=/host/var/log/syslog`, and uses the Docker unix socket for container log streaming.

### Changed

- `CORTEX_AGENT_DOCKER_URL` now defaults to `unix:///var/run/docker.sock`, matching the local Linux agent behavior instead of requiring a Docker socket proxy by default.
- Agent deployment writes `CORTEX_SYSLOG_TARGET` when supplied so heartbeat HTTPS and raw syslog TCP can use different routes.

## [1.18.0] - 2026-06-11

### Added

- `cortex deploy agent` — SSH-based fleet deployment subcommand. Reads `~/.ssh/config`, probes all concrete host aliases in parallel (SSH BatchMode, 5 s timeout), presents an `inquire::MultiSelect` TUI picker, then scp's the local cortex binary and runs `cortex setup heartbeat-agent install` on each selected host. Flags: `--hosts` (comma-separated override), `--target`, `--heartbeat-token`, `--docker`, `--journald`, `--binary`.
- `cortex setup heartbeat-agent install` auto-detects when systemd is absent (Unraid, etc.) and falls back to writing a Docker Compose service (`~/.cortex/compose/docker-compose.yml`) backed by `debian:bookworm-slim` with the binary bind-mounted, then runs `docker compose up -d`.
- Atomic binary update on deploy: scp to `cortex.new` then `mv -f` so the running service is never interrupted mid-write.

## [1.17.0] - 2026-06-10

### Added

- `cortex heartbeat agent` now accepts `--docker` and `--journald` flags (and env vars `CORTEX_AGENT_DOCKER`, `CORTEX_AGENT_JOURNALD`) to stream local Docker container logs and journald entries as RFC 5424 syslog to the cortex receiver, enabling single-binary host onboarding without separate rsyslog/forwarder setup.
- `--docker-url` / `CORTEX_AGENT_DOCKER_URL` — Docker HTTP endpoint (default `http://127.0.0.1:2375`).
- `--syslog-target` / `CORTEX_SYSLOG_TARGET` — override the TCP syslog target; derived from `--target` host when not set.
- `cortex setup heartbeat-agent install` writes `CORTEX_AGENT_DOCKER` and `CORTEX_AGENT_JOURNALD` to the generated `heartbeat-agent.env`.

## [1.16.5] - 2026-06-10

### Changed

- Move completed plans (rmcp-streamable-http-refactor, compose-lifecycle-cli) to `docs/plans/complete/`

## [1.16.4] - 2026-06-10

### Fixed

- `resolve_cortex_binary()` replaces `sh -c "command -v cortex"` with a cross-platform PATH walk — eliminates "Unix shell script not found" from `cortex doctor` on Windows

## [1.16.3] - 2026-06-10

### Fixed

- Windows build: `unsafe extern "system"` block required by Rust edition 2024
  for the inline `GetDiskFreeSpaceExW` declaration in `src/db/maintenance.rs`.

## [1.16.2] - 2026-06-10

### Fixed

- Windows build: replaced `rustix::fs::statvfs` (Unix-only) in
  `src/db/maintenance.rs` with a `#[cfg]`-gated implementation that calls
  `GetDiskFreeSpaceExW` on Windows via an inline `extern "system"` declaration
  (no new dependencies).
- Windows build: replaced `libc::gethostname` (Unix-only) in `src/scanner.rs`
  with a `#[cfg(unix)]`/`#[cfg(not(unix))]` split that falls back to
  `COMPUTERNAME`/`HOSTNAME` env vars on Windows.

## [1.16.1] - 2026-06-10

### Changed

- Plugin installer pattern: the bundled `plugins/cortex/bin/cortex` binary has
  been removed. The plugin now installs the `cortex` binary from GitHub Releases
  via `install.sh` / `install.ps1` on first use. `plugin-setup.sh` falls back
  to running `install.sh` (or fetching it from GitHub) when `cortex` is not
  already on PATH. The `hooks.json` SessionStart/ConfigChange hooks now call
  `plugin-setup.sh` directly rather than the (now-absent) bundled binary.
- Added `install.sh` (Linux/macOS one-liner bootstrap) and `install.ps1`
  (Windows PowerShell mirror) at the repository root.
- Added `.github/workflows/release.yml` — publishes `cortex-linux-x86_64.tar.gz`
  and `cortex-windows-x86_64.zip` with SHA256 checksums as GitHub Release assets
  on every `v*` tag push.

## [1.16.0] - 2026-06-09

### Security

- Docker Compose now publishes the MCP/HTTP port 3100 on `127.0.0.1` by
  default (`CORTEX_MCP_BIND` overrides the host interface). The Labby gateway
  still reaches cortex over the Docker network at `http://cortex:3100`;
  syslog 1514 remains published wide. Set `CORTEX_TOKEN` before exposing 3100.
- The SSH inventory mount now defaults to a dedicated key directory
  (`~/.cortex/ssh`, override via `CORTEX_SSH_VOLUME`) instead of encouraging a
  `~/.ssh` mount. Documented least-privilege deploy-key provisioning and an
  explicit "never mount ~/.ssh" warning in `docs/SECURITY.md`.
- Documented the TrustedGatewayUnscoped mode (`CORTEX_NO_AUTH=true` +
  `CORTEX_TRUSTED_GATEWAY_NO_AUTH=true`): it disables both auth and the
  read/admin scope gates, including the write actions
  `ack_error`/`unack_error`/`notifications_test`, and must never be combined
  with host-published ports.

### Added

- `ingest_silence` notification rule (`[notifications.evaluators]
  ingest_silence = true`, `ingest_silence_threshold_secs = 900`): fires a
  critical Apprise alert when the DB has logs but the newest row is older
  than the threshold. An empty DB never fires.
- Syslog listener supervision with restart + backoff; `/health` now returns
  503 when a started listener has died, and per-listener liveness
  (`syslog_udp_listener_state` / `syslog_tcp_listener_state`:
  `not_started|alive|down`) appears in the `/health/full` ingest snapshot.
- Migration-chain upgrade test coverage.
- `config/systemd/cortex-backup.service` + `cortex-backup.timer` — daily
  WAL-safe backup units (randomized delay) invoking `scripts/backup.sh` with
  a configurable `BACKUP_DIR`, plus an install section in the deploy runbook.
- Restore runbook (`docs/runbooks/deploy.md` "Restore"): stop → copy backup →
  fix bind-mount ownership → `cortex db integrity` → restart → verify
  `/health` and `ingest_rate`.

### Fixed

- Parser char-boundary panic that a LAN sender could trigger to kill a syslog
  listener.
- Command-log preview panic.
- FTS search fast path no longer materializes unbounded match sets — capped
  at the 200K most-recent matches; severity-only filtered searches use the
  same capped candidate plan.
- MCP tool errors are now typed (`invalid_params`, `retryable` for database
  busy, `not_found`, `conflict`, `internal`) instead of a single opaque class.
- MCP read permits are now `pool_size - 1`, reserving one pool connection for
  the ingest writer so slow concurrent reads can no longer starve ingest.
- `docker-compose.prod.yml` default image tag is kept in version canon
  (currently 1.15.1).
- Startup `auto_vacuum` conversion VACUUM now logs loudly before it runs.

### Notes

- Retroactive: the `auto_vacuum=INCREMENTAL` conversion (shipped in an
  earlier release) runs a one-time full VACUUM at startup on pre-existing
  databases. It can take minutes on large DBs — treat the first startup
  after upgrading like a heavy migration (backup first).

## [1.15.1] - 2026-06-08

### Fixed

- Allow the Labby gateway's `Host: cortex:3100` header via
  `CORTEX_ALLOWED_HOSTS` in `docker-compose.yml`, so the gateway can proxy this
  service over the docker network without RMCP Host validation rejecting calls
  with "Host header is not allowed". Keeps the allowlist version-controlled
  instead of living only in the gitignored `.env`.

## [1.15.0] - 2026-06-07

### Added

- Added `map` `mode=findings` for bounded topology risk and hygiene findings:
  `potential_public_route`, `risky_mounts`, and `collector_health`.
- Added relationship-type graph finding helpers and an index for bounded
  route/mount proof queries without broad graph scans.
- Documented findings semantics and redaction boundaries for route, mount, and
  collector-health evidence.

## [1.14.2] - 2026-06-07

### Fixed

- Shortened inventory graph projection write-lock scope by planning expensive
  projection work in memory before applying graph mutations, and avoided the
  redundant graph count query on successful projection.
- Hardened SSH-backed inventory and drop-in deploy flows with shared host
  validation, strict host key checking by default, explicit TOFU opt-in, and a
  shared SSH argument policy. Inventory collectors and remote Docker event
  streams also share bounded concurrency and retry backoff.
- Aligned MCP argument parsing with typed request structs so unknown fields are
  rejected consistently with HTTP validation, and moved action dispatch behind
  the action registry.
- Moved expensive log pattern clustering out of the DB closure, bound SQL
  limits instead of interpolating them on touched query paths, and reused the
  freshly collected inventory snapshot for graph projection.

### Changed

- Rebranded current docs, plugin docs, schema metadata, mcporter examples, and
  release guidance around the `cortex` tool name, `cortex:*` scopes, and the
  current plugin/package naming.
- Clarified current-versus-archive documentation authority, release/version
  policy, live smoke gates, security trust assumptions, OAuth non-Unix behavior,
  cargo-deny exception ownership, and rmcp lower-bound intent.
- Made `CLAUDE.md` the agent-memory source of truth and restored sibling
  `AGENTS.md` symlinks wherever local `CLAUDE.md` files exist.

### CI

- Ran clippy with `--all-targets` and added checks for agent-memory symlinks,
  unversioned plugin manifests, and stale public project identity strings.

## [1.14.1] - 2026-06-06

### Fixed

- Made graph-backed `map` modes use a lightweight graph-only response path
  instead of paying the full inventory snapshot and heartbeat overlay cost.
- Filtered map answer rows by each mode's topology relationship contract and
  expanded `domain_routes` through reverse proxies to include routed services.
- Added action-conditional MCP schema constraints so graph-only and map-only
  `mode` values validate against the correct action.

## [1.14.0] - 2026-06-06

### Added

- Added graph-backed `map` answer modes for host services, domain routes, and
  service dependencies, returning bounded graph answers with safe evidence
  samples alongside the existing inventory snapshot.
- Documented the new map mode request fields in MCP schema/help text and added
  MCP tests for the graph-backed map workflows.

## [1.13.3] - 2026-06-04

### Fixed

- Mounted the canonical `~/.cortex` home, SSH config, and workspace into the
  Compose container so deployed inventory refreshes can collect remote hosts,
  config artifacts, and project metadata instead of producing a local-only
  partial cache.
- Added runtime `ssh`, `git`, and network inspection tools to the container
  image, and made SSH invocations tolerate newer host config options when using
  an older container OpenSSH client.

## [1.13.2] - 2026-06-04

### Fixed

- Tightened inventory graph projection failure handling so optional inventory
  cache failures report degraded metadata without aborting explicit graph
  rebuilds, successful inventory projection clears only stale inventory
  degradation, and cache misses are distinguished from unreadable cache files.

## [1.13.1] - 2026-06-04

### Fixed

- Hardened inventory graph projection so inventory refreshes no longer mark the
  full graph projection lifecycle as ready or overwrite log-derived entity
  ownership metadata.
- Scoped Compose project and Docker network graph keys by source host, avoided
  ambiguous bare service-name links, and kept raw config/cache paths out of
  graph labels, aliases, source ids, and evidence excerpts.
- Made remote Docker event streams opt-in and serialized background inventory
  projection through the maintenance limiter.

## [1.13.0] - 2026-06-04

### Added

- Projected the cached homelab inventory into the investigation graph with
  evidence-backed topology nodes for Compose projects, reverse proxies, domains,
  networks, storage, and redacted config artifacts.
- Added server-side inventory refresh triggers from local Compose/proxy config
  file watchers, plus opt-in remote Docker `events` streams over SSH, while
  preserving the 5-minute polling baseline.

### Changed

- Expanded graph vocabulary and schema constraints for inventory topology
  relationships such as `defines_service`, `routes_to`, `exposes_domain`,
  `attached_to`, `mounts`, `backed_by`, and `has_artifact`.
- Documented optional inventory collector env vars, realtime refresh toggles,
  and media API collector configuration in the env example and inventory docs.

## [1.12.0] - 2026-06-04

### Added

- Added SSH-backed remote device inventory for concrete `~/.ssh/config` hosts,
  collecting host OS, CPU, memory, IPs, listeners, storage, Tailscale IPs, and
  GPU summaries without depending on external shell scripts.
- Added SSH-backed remote Docker inventory that maps containers across reachable
  hosts with status/health, image, ports, networks, mounts, compose/route
  labels, domains, and environment key names without storing environment values.
- Added server-side periodic homelab inventory refresh. `cortex serve mcp`
  refreshes `~/.cortex/inventory` shortly after startup and every
  `CORTEX_INVENTORY_REFRESH_INTERVAL_SECS` seconds; set the value to `0` to
  disable background refresh.

## [1.11.0] - 2026-06-04

### Added

- Added SSH-backed homelab config collection from concrete `Host` aliases in
  `~/.ssh/config`, capturing remote Compose YAML and SWAG/reverse-proxy confs as
  redacted private artifacts under `~/.cortex/inventory`.

### Changed

- Batched remote config reads per host and config kind so large SWAG
  proxy-confs directories are collected without one SSH round trip per file.

## [1.10.1] - 2026-06-04

### Fixed

- Passed the resolved Compose env file through `CORTEX_ENV_FILE` for compose
  mutations and allowed `~/.cortex/.env` to drive compose data-volume checks.

## [1.10.0] - 2026-06-03

### Added

- Added native homelab inventory refresh/status CLI commands that write private
  normalized, collection-state, and raw redacted artifacts under
  `~/.cortex/inventory`.
- Added independent Rust collectors for local host facts, Docker, redacted
  Compose/proxy configs, Tailscale, Unraid, UniFi, media stack topology, and
  configured local Git projects.
- Upgraded MCP `action=map` to `cortex.homelab_map.v2`, reading the cached
  inventory and overlaying bounded live Cortex host/heartbeat data without
  triggering refresh or returning raw artifact bodies.

## [1.9.1] - 2026-06-03

### Changed

- Integrated the split app service/model layout on top of the graph proof UX
  release, replacing the monolithic app service with focused `src/app/services/`
  modules while preserving graph evidence, endpoint summaries, redaction, and
  payload-budget behavior.

## [1.9.0] - 2026-06-03

### Added

- Added graph evidence lookup across service, REST `/api/graph/evidence`, MCP
  `action=graph mode=evidence`, HTTP client, and direct CLI
  `cortex graph evidence <evidence-id>`.
- Added compact relationship endpoint summaries to graph relationship payloads
  and proof-oriented CLI output for evidence, around, and explain graph flows.
- Added graph proof UX docs and optional smoke coverage for bounded output,
  compatibility ids, endpoint summaries, source-log summaries, and privacy
  invariants.

### Fixed

- Fixed MCP graph schema drift by documenting the existing `explain` mode while
  adding `evidence` and `evidence_id`.
- Hardened graph evidence/source-summary redaction for auth headers, bearer
  tokens, cookies, credential/client-secret/access-token markers, URL userinfo,
  private-key markers, home paths, and terminal controls.

## [1.8.0] - 2026-06-03

### Added

- Added MCP `action=map`, a bounded homelab infrastructure snapshot that
  combines known hosts, verified source identities, top observed applications,
  latest heartbeat status when available, and external inventory-source
  metadata inspired by `~/docs/scripts`.
- Added map action coverage to MCP schema/help docs, plugin skill references,
  and live smoke harnesses.

## [1.7.1] - 2026-06-02

### Added

- Added investigation graph spec and contract documents covering current graph
  sources, entity/relationship vocabulary, evidence semantics, projection
  lifecycle, public query surfaces, and compatibility rules.

## [1.7.0] - 2026-06-02

### Added

- **CLI help now supports nested subcommand help.** `cortex ai search --help`, `cortex db status --help`, `cortex compose logs --help`, and other nested command paths now render focused usage instead of the whole namespace. The top-level `cortex --help` is regrouped into an axon-style sectioned layout with cyan headers.
- **CLI parser recovery now suggests close command and subcommand matches.** Mistyped top-level and nested commands now include `Did you mean ...` hints while preserving strict parsing.
- **AI investigation output now has compact/default controls.** `cortex ai investigate` and `cortex ai blocks` support `--detail compact|full`, with `--include-transcript`, `--max-bytes`, and `--limit` controls for bounded agent-friendly output.

### Fixed

- **Cortex CLI diagnostics no longer point operators at stale `syslog` binary invocations.** Backup guidance, help text, rollout notes, and parser comments now use the `cortex` binary where they refer to CLI commands.
- **`host-state` now fails with actionable usage when no host selector is supplied.** The parser now reports the required `--host-id` or `--hostname` selector before dispatching.
- **`--max-bytes` truncation is now byte-accurate on multibyte input** and yields a leading prefix for budgets smaller than the ellipsis instead of an empty string.

## [1.6.1] - 2026-06-02

### Added

- Added operator-facing `cortex graph status` and `cortex graph rebuild`
  commands so graph projections can be seeded and inspected from the CLI.
  Rebuild now publishes chunk/source-row progress while processing large
  databases and batches extraction work in chunk transactions before the final
  graph-table swap.

## [1.6.0] - 2026-06-02

### Added

- Added evidence-backed graph explanations with service, MCP, REST, HTTP
  client, and CLI support. `cortex graph explain` and MCP `action=graph
  mode=explain` expand bounded graph chains, cite relationship/evidence ids,
  return conservative confidence, missing evidence, open questions, follow-up
  graph queries, projection/truncation metadata, and decline to emit a
  narrative when graph evidence is insufficient.

## [1.5.0] - 2026-06-02

### Added

- Added direct CLI graph commands: `cortex entity` resolves graph entities by
  canonical type/key or alias, and `cortex graph around` renders one-hop
  neighborhoods with relationship type, related entities, confidence/trust,
  reason codes, safe evidence samples, projection status, truncation reasons,
  follow-up commands, and shared JSON output for agents.

## [1.4.0] - 2026-06-02

### Added

- Added the Graph v1 entity lookup and one-hop neighborhood read surface across
  the service layer, MCP `graph` action, REST `/api/graph/entity` and
  `/api/graph/around` routes, and the shared CLI HTTP client. Responses include
  normalized entities, relationships, allowlisted/redacted evidence,
  ambiguity candidates, next-query hints, hard traversal/payload limits, and
  projection/degraded status metadata without triggering implicit rebuilds.

## [1.3.0] - 2026-06-02

### Added

- Added the Graph v1 rebuild extractor for existing logs, Docker source
  metadata, AI session metadata, heartbeat latest rows, and error signatures.
  The rebuild uses connection-local staging tables plus a short serialized swap,
  exposes projection status/failure metrics, deduplicates evidence buckets, and
  prunes stale graph evidence on rebuild after source rows are deleted.

## [1.2.0] - 2026-06-02

### Added

- Added Graph v1 schema tables for derived investigation entities, aliases,
  relationships, evidence, and projection metadata. The schema keeps raw logs,
  heartbeats, inventory, signatures, and sessions as the source of truth while
  preparing rebuildable graph projections for future entity lookup and incident
  narrative features.

## [1.1.5] - 2026-06-02

### Fixed

- **SessionStart hook no longer flags the running server's own port as a problem.** The `mcp-port` setup phase bound-tested `127.0.0.1:<CORTEX_PORT>` and reported `Warn: port already in use` — but for an already-running cortex, the port being bound is the *healthy* state, so every session start emitted a misleading warning. The hook does not (re)deploy, so a port-in-use is now reported as `Ok` (`port <n> in use (cortex already running)`). Genuine port-conflict diagnosis remains in `cortex doctor`.

## [1.1.4] - 2026-06-01

### Fixed

- **Migration 22 startup-brick crash-loop (P0, `syslog-mcp-tfr0`).** Two `ALTER TABLE
  ADD COLUMN` + the version `INSERT` ran in a bare `execute_batch` (auto-commit per
  statement); a crash between them left the column committed but unmarked, so restart
  re-ran the ALTER → `duplicate column name` → `init_pool` error → `restart: unless-stopped`
  crash-loop needing manual SQL. Rewrote Migration 22 as a single transactional unit
  (`BEGIN IMMEDIATE` + `add_column_if_missing` guards + `INSERT OR IGNORE`), so a crash
  rolls back atomically and restart converges. Added partial-apply/idempotency tests.
- **AI rollup refresh starved the single WAL writer (P0, `syslog-mcp-rvcz`).** The 5-minute
  `refresh_ai_session_rollup` held the write lock across `DELETE` + a full-partition
  `GROUP BY` (~4s at scale), dropping ingest. Reworked to staging+swap: the full recompute
  is built into a connection-local TEMP table on a read snapshot (no write lock), then a
  sub-millisecond `BEGIN IMMEDIATE` swap. Stays correct under retention DELETEs (incremental
  refresh would corrupt MIN/MAX). Empty-staging guard gates on the full rollup predicate so
  `ai_project`-only (OTLP) rows no longer error the refresh forever.
- **Storage-budget enforcement self-wiped under external disk pressure (P0, `syslog-mcp-w4hh`).**
  `min_free_disk_mb` measured whole-filesystem free space but only deleted cortex's own
  oldest rows, so a noisy neighbor on the shared volume erased the entire log history
  (including err+). Split the triggers: `max_db_size_mb` self-trims; `min_free_disk_mb`
  now sets `write_blocked` + alerts instead of deleting (default 0, paired with
  `recovery_free_disk_mb=0` to satisfy validation). Added a time-windowed, per-source-IP
  err+ retention floor on both delete paths; probe failure now fails safe (treated as
  worst-case) instead of fail-open.

### Changed

- **`syslog` → `cortex` rebrand sweep.** Renamed CLI/MCP/docs references, added CLI
  plugin-options, refreshed docs/contracts and plugin manifests, removed stale `bin/`
  wrappers.

## [1.1.3] - 2026-06-01

### Fixed

- **Serialize SQLite writes to eliminate `database is locked` errors and dropped log batches.**
  cortex ran an r2d2 pool of 4 write-capable connections while 6 writer subsystems
  (syslog/docker ingest, heartbeat, notifications, AI index, retention maintenance)
  raced SQLite's single write lock, exceeding `busy_timeout` (~1000+ lock errors/day and
  silently discarded log batches). Added a process-wide reentrant write-serialization
  lock (`db::write_lock()`) acquired by every write transaction and standalone mutating
  statement (purge DELETEs, VACUUM, incremental_vacuum, merge). Reads stay concurrent on
  the pool (WAL); PASSIVE checkpoints are left unguarded.


## [1.1.2] - 2026-05-31

### Fixed

- **`host-state` / `fleet-state` / `correlate-state` CLI commands were
  unreachable** — added in 1.1.0 (cxih.4) and routed in the parser + dispatch,
  but omitted from the top-level command allowlist in `Mode::parse`, so every
  invocation printed the usage banner and exited 1. They are now routed, listed
  in `--help` with correct flags, and covered by a dispatch-level regression
  test. (syslog-mcp-8fww)

## [1.1.1] - 2026-05-31

### Fixed

- **`correlate_state` no longer leaks AI transcript rows** — the action built
  its `SearchParams` with `..Default::default()`, leaving `exclude_ai` false, so
  AI transcript logs could surface in results despite the action being
  documented as correlating non-AI logs with per-host heartbeat state. It now
  sets `exclude_ai: true`. Regression test added.

## [1.1.0] - 2026-05-31

### Added

- **`correlate_state` action** — correlate non-AI logs with per-host heartbeat
  window summaries around a reference time. Exposed across all surfaces: MCP
  (`action=correlate_state`), REST (`GET /api/correlate-state`), and CLI
  (`syslog correlate-state`). Bounded by default (window/limit capped); never
  performs a full-history scan. Returns the resolved window, per-host heartbeat
  summary plus matching logs, and a `truncated` flag. (cxih.3, cxih.4)
- **CLI parity for heartbeat fleet state** — new `syslog host-state`,
  `syslog fleet-state`, and `syslog correlate-state` top-level commands with
  human and `--json` output, mirroring the existing MCP/REST surfaces. (cxih.4)
- **Deterministic abuse-incident findings** — `abuse_investigate` bundles now
  carry a `findings` object with rule-based `likely_failure_modes` (conservative
  confidence, cited evidence ids), `contributing_factors`, category-tied
  `prevention_hints`, and `open_questions`. Findings are computed locally with no
  external LLM call and surface `unknown` + open questions when evidence is weak.
  Categories: `command_failure`, `tool_timeout`, `auth_or_permission_failure`,
  `stale_binary_or_version_drift`, `test_failure`,
  `docker_or_service_runtime_failure`, `db_busy_or_performance_bottleneck`,
  `unclear_instruction_or_scope_drift`, `unknown`. (kmib.4)

### Fixed

- **Heartbeat window summaries** — fixed a latent `misuse of aggregate: MAX()`
  SQL error in `heartbeat_window_summaries` (used by `correlate_state`); the
  latest heartbeat id is now resolved with a scalar subquery instead of an
  aggregate inside a correlated subquery under `GROUP BY`. (cxih.3)

## [1.0.0] - 2026-05-28

### Breaking Changes — Renamed from syslog-mcp to cortex

This is a hard break. No backwards-compatibility shims. Update every deployed
instance as part of the upgrade (see migration notes below).

- **Product / crate** renamed `syslog-mcp` → `cortex`.
- **Binary** renamed `syslog` → `cortex`.
- **MCP tool** renamed `syslog` → `cortex`. All 42+ action strings are unchanged.
- **Env vars** — the `SYSLOG_MCP_*` figment prefix is now `CORTEX`, and the entire
  bare `SYSLOG_*` family is renamed to `CORTEX_*`. Collision-disambiguated cases:
  - `SYSLOG_PORT` → `CORTEX_RECEIVER_PORT` (the UDP/TCP receiver port; distinct from
    `CORTEX_PORT`, the MCP/HTTP server port formerly `SYSLOG_MCP_PORT`).
  - `SYSLOG_HOST` → `CORTEX_RECEIVER_HOST`; `SYSLOG_HOST_PORT` → `CORTEX_RECEIVER_HOST_PORT`.
- **Removed** the deprecated `SYSLOG_MCP_API_TOKEN` MCP-token alias. Its post-rename
  name `CORTEX_API_TOKEN` now belongs exclusively to the API/OTLP token; use
  `CORTEX_TOKEN` for the MCP static token.
- **Config** — the `[syslog]` section is now `[receiver]` (struct field `Config.syslog`
  → `Config.receiver`).
- **Database file** renamed `syslog.db` → `cortex.db` (a data-file migration — move the
  existing DB during upgrade).
- **Docker image** `jmagar/syslog-mcp` → `ghcr.io/jmagar/cortex` (Docker Hub mirror).
- **Plugin** renamed `syslog` → `cortex` (plugin data dir `syslog-jmagar-lab` →
  `cortex-jmagar-lab`).
- **Internal** — module `src/syslog/` → `src/receiver/`; types `SyslogService` →
  `CortexService`, `SyslogRmcpServer` → `CortexRmcpServer`, `SyslogConfig` →
  `ReceiverConfig`.

Unchanged: SQLite schema and on-disk format, HTTP API routes, RFC 3164/5424 wire
protocol support, the `syslog-udp`/`syslog-tcp` source aliases, and observability
metric names.

Per-host migration: stop the service; checkpoint and `mv syslog.db cortex.db`; rename
`SYSLOG_*` env vars to `CORTEX_*` in `.env`; rename the `[syslog]` config section to
`[receiver]`; point clients at the `cortex` MCP tool; pull `ghcr.io/jmagar/cortex:1.0.0`;
start and verify with `cortex --http db status`.

## [0.36.1] - 2026-05-29

### Changed

- `run_db` now emits structured `tracing` events on every code path, including
  acquire-timeout, semaphore-closed, and task-panic early returns that
  previously returned silently. All ~60 callsites carry an `op` label for
  correlation in log queries.
- Ops exceeding 500 ms escalate from `debug` to `warn` level so slow queries
  are visible at production `RUST_LOG=info` without configuration changes.
- `JoinError` from `spawn_blocking` now distinguishes task-cancelled (graceful
  shutdown) from task-panic in the log message.

### Fixed

- Fixed silent-failure in `correlate_state.resolve_host`: a `COUNT(*)` query
  error was previously swallowed via `.unwrap_or(false)`, converting any DB
  failure into a misleading `NotFound` response.

## [0.36.0] - 2026-05-29

### Added

- `feat(llto)`: week and month bucket sizes for the `timeline` action.
- `feat(cli)`: Aurora color palette wired into all CLI output, porting the
  formatter patterns from `axon_rust`.

### Changed

- `chore(dthv)`: `just test` now runs via `cargo nextest` for parallel test
  execution.
- `chore(421t)`: split oversized CLI modules below the 500-line limit.
- `perf(zl9y)`: `timeline` defaults to the last 30 days to prevent a full
  table scan (skipped when `--to` is already specified).
- `perf(fvw4)`: per-phase tracing added to `PhaseTimer`.
- `perf(2rap)`: index on `error_signature_windows(window_end)` to speed up
  signature window queries.
- Ignore the local `.superpowers/` working directory.

### Fixed

- `fix(xknb)`: path-traversal confinement, partial-file cleanup, and a backup
  HTTP timeout.
- `fix(z4eg)`: `just test-live` token handling — guard the `--token` arg and
  inject `SYSLOG_API_TOKEN`.
- `fix(soq2)`: remove the forbidden `version` field from the plugin manifest.
- `fix(llto)`: document the `Bucket::default_lookback_days` sync constraint in
  the CLI helper.

### Docs

- CLI performance benchmark report.
- Cortex v1.0.0 rebrand design and implementation plan.

## [0.35.0] - 2026-05-27

### Added

- Three new HTTP routes that close MCP/HTTP parity gaps:
  - `GET /api/host-state` — bounded per-host heartbeat snapshot. 400 for
    missing `host_id`/`hostname` or invalid `since` timestamp; 404 for
    unknown host.
  - `GET /api/context` — pivot-window log context around a `log_id` or
    `hostname`+`timestamp`. 400 for missing pivot; 404 for unknown log.
  - `GET /api/fleet-state` — fleet-wide heartbeat snapshot with pressure
    flags and summary counts (`include_ok`, `sort` query params).
- Registered `fleet_state` as a first-class MCP action so all three
  surfaces (CLI, REST, MCP) see the same catalog. Tool dispatch, schema,
  help text, and the registry-coverage fence files were updated.
- `HttpClient` wrappers for `similar_incidents`, `ask_history`, and
  `incident_context` — three CLI actions that previously had REST routes
  but no client wrappers, so `--http` mode was blocked. The CLI now
  reaches the REST surface end-to-end with Ctrl-C cancellation.

### Changed

- `SyslogService::host_state` now validates the `since` query parameter
  via `parse_optional_timestamp` before passing it to SQL. Previously
  garbage strings silently produced wrong results against
  `sampled_at >= ?2`.
- `SyslogService::context` now returns `ServiceError::InvalidInput` for
  missing pivot and `ServiceError::NotFound` for unknown `log_id`. The
  REST surface returns 400/404 instead of 500 for these client-side
  conditions.
- `fleet_state` MCP action cost classification: `Moderate` → `Expensive`.
  The implementation issues N+1 DB calls across the fleet; the new
  cost is honest about the resource profile.
- `UnaddressedErrorsQuery` gains `#[serde(deny_unknown_fields)]` for
  consistency with the rest of the query-string structs.

## [0.33.0] - 2026-05-25

### Added

- Added heartbeat telemetry V1: SQLite heartbeat storage, `POST /v1/heartbeats`
  ingest, the bounded `host_state` MCP action, and a Linux `syslog heartbeat
  agent` collector with binary-owned setup.

## [0.32.6] - 2026-05-24

### Added

- Added first-class structured log filtering across CLI, REST, and MCP.
  Use `syslog filter`, `GET /api/filter`, or MCP `action=filter` for
  queryless correlation by host/source/app/time, Docker source aliases,
  AI transcript fields, agent commands, and shell history.

## [0.32.5] - 2026-05-24

### Fixed

- **REST AI incident queries**: Avoid unbounded FTS result sorting for broad
  incident terms by walking timestamp-ordered log rows and probing FTS by rowid,
  and reject unsupported indexed `terms[0]=...` query syntax instead of silently
  ignoring it.
- **REST Compose doctor**: Return HTTP 503 with the redacted structured Compose
  projection when Docker/ownership/runtime readiness checks fail, instead of a
  generic 500 error envelope.
- **Clock skew analytics**: Force the `received_at` range index and apply host
  limits in SQL so recent-window clock skew checks stay responsive on large
  databases.

## [0.32.4] - 2026-05-24

### Fixed

- **Headless Gemini assessment runner**: Prefer recovered `write_file`
  assessment content over streamed assistant preamble text, with regression
  coverage for mixed preamble-plus-file Gemini streams.

## [0.32.3] - 2026-05-24

### Fixed

- **Headless Gemini assessment runner**: Recover Markdown assessments from
  Gemini `write_file` stream events and reinforce the prompt to return
  Markdown directly instead of creating artifacts. Also pass a non-empty
  `--prompt` stub so Gemini does not exit before consuming stdin evidence.

## [0.32.2] - 2026-05-24

### Fixed

- **Agent command setup**: Harden existing telemetry spool files during install
  and reject symlink or non-file spool targets during setup checks.
- **Agent command wrapper**: Keep recursion prevention scoped to argv-level
  `syslog agent-command ingest-spool` invocations, execute wrapped multi-arg
  commands without shell re-parsing, and preserve wrapped command flags after
  `--`.
- **Command source identity**: Percent-encode shell and agent command source
  URI path segments without lossy character replacement.

## [0.32.1] - 2026-05-24

### Fixed

- **Agent command wrapper**: Preserve wrapped command exit status when telemetry
  spool append fails, and avoid mutating permissions on existing spool parent
  directories.

## [0.32.0] - 2026-05-24

### Added

- **Shell history ingestion**: Add `syslog shell index` for zsh extended
  history backfill into the main log corpus with `shell-history` source
  metadata.
- **Agent command capture**: Add `syslog agent-command` spool ingestion and a
  `syslog setup agent-command` Claude Code shell-prefix wrapper for Bash tool,
  hook, and MCP startup command correlation.

## [0.31.3] - 2026-05-24

### Fixed

- **AI incident assessment**: Allow `syslog ai assess <incident_id>` to build
  evidence for any listed incident ID instead of only incidents in the top 10
  investigation bundle page.
- **AI incident CLI docs**: List `syslog ai incidents`, `syslog ai investigate`,
  and `syslog ai assess` in top-level usage and CLI documentation.

## [0.31.2] - 2026-05-24

### Changed

- **Headless prompt eval**: Add live MCP preflight, headless-agent MCP
  visibility checks, timeout/token budget controls, compact JSON reports, and
  explicit resource-read documentation for prompt evaluation runs.

## [0.31.1] - 2026-05-24

### Fixed

- **Headless prompt eval**: Align the Codex runner with the installed
  `codex exec` CLI flags and tighten the prompt-output schema resource so
  strict structured-output clients accept evidence entries.
- **Prompt smoke tests**: Carry bearer auth into mcporter-backed smoke calls and
  accept the scope-denied unknown-action response as the expected negative path.

## [0.31.0] - 2026-05-24

### Added

- **Agent-first MCP prompts**: Add focused infrastructure debugging prompts for
  Docker regressions, DNS failures, storage pressure, auth brute force,
  forwarding gaps, and after-deploy checks.
- **Prompt contracts**: Expose action cost metadata, agent planning guidance,
  and a structured prompt-output schema resource for incident-style answers.
- **Headless prompt eval**: Add a Codex/Claude headless prompt evaluation
  runner plus documentation for live MCP prompt rendering and schema scoring.

### Changed

- **Bounded diagnostics**: Add `limit` support for high-volume error and clock
  skew summaries, accept `limit` as a `patterns` alias, and include prompt
  coverage in the MCP smoke test.

## [0.30.3] - 2026-05-23

### Fixed

- **Frustration assessment prompt**: Require evidence-backed trend language,
  preserve uncertainty in summaries, and distinguish real frustration from
  incidental profanity.

## [0.30.2] - 2026-05-23

### Fixed

- **Headless Gemini assessment runner**: Preserve Gemini child-process status
  and stderr when stdin closes early, so diagnostics are not masked by broken
  pipe errors.

## [0.30.1] - 2026-05-23

### Fixed

- **MCP prompt runbooks**: Tighten infrastructure debugging prompts with bounded
  query guidance, valid `timeline` bucket examples, cheap-first escalation
  steps, and a consistent operator synthesis format.

## [0.30.0] - 2026-05-23

### Added

- **MCP prompts**: Expose infrastructure debugging prompts for incident
  triage, host health checks, service outages, security auth review, log noise
  reduction, and AI-agent change correlation.

- **Headless Gemini assessment runner**: `syslog ai assess` now runs Gemini in
  an isolated temporary HOME, installs the bundled
  `syslog-frustration-assessment` skill, parses `stream-json` output, streams
  assistant deltas in text mode, rejects unexpected tool calls, and exposes
  syslog-specific Gemini command/model/home/timeout environment knobs.

## [0.29.0] - 2026-05-23

### Added

- **MCPB packaging**: Add a Linux MCP Bundle manifest and
  `scripts/build-mcpb.sh` so the existing `syslog mcp` stdio server can be
  packed as `dist/syslog-mcp-<version>-linux.mcpb`.

## [0.28.2] - 2026-05-23

### Added

- **Remote deploy CLI**: Add `syslog deploy remote <host>` for SSH-based
  Compose deployment without adding REST or MCP mutation surfaces.

## [0.28.1] - 2026-05-23

### Added

- **Session artifacts**: Add saved CLI refactor and P0/P1 surface parity
  session notes.
- **Aurora logo generator**: Add the script used to generate the Aurora server
  logo pack.

### Changed

- **Python cache hygiene**: Ignore Python bytecode caches so generated
  `__pycache__` artifacts are not committed.

## [0.28.0] - 2026-05-23

### Added

- **Deploy CLI**: Add `syslog deploy preflight` and `syslog deploy local`
  as operator-facing names for the existing local Compose setup/reconcile path.
- **MCP Apps query widget contract**: Expose a `ui://syslog/query-widget`
  resource, advertise it through `syslog` tool metadata, and return structured
  tool data alongside readable JSON text for UI-capable MCP hosts.

### Changed

- **Plugin deploy config**: Remove the stale `use_docker` user setting now that
  automated server deployment is Compose-only.

## [0.27.4] - 2026-05-22

### Fixed

- **CLI config writes**: Rewrite `.env` and `config.toml` via same-directory
  temporary files and atomic rename so failed writes cannot truncate live
  configuration.
- **CLI review follow-ups**: Key doctor cache entries by container/unit name,
  recursively flatten nested TOML inline tables, accept negative signed flag
  values where appropriate, and replace placeholder CLI sidecar tests with
  behavior-focused coverage.

## [0.27.3] - 2026-05-22

### Refactor

- **CLI modules**: Split the CLI monolith into focused parser, dispatch, output, setup, config, coordination, and AI-watch modules with sidecar tests and a CLI module-size guard.

## [0.27.2] - 2026-05-21

## [0.27.1] - 2026-05-20

### Fixed

- **Journalctl timeout**: `command_output` now enforces a 30-second timeout so
  a wedged user bus or stalled `journalctl` cannot block `syslog ai doctor`
  indefinitely.
- **Incident hostname+service conflict**: `syslog incident` now returns a clear
  error when both `--host` and `--service` are supplied, since journal entries
  cannot be filtered by remote hostname.
- **Service-log dropped lines**: Human-readable output of `syslog service logs`
  now prints a stderr warning when `dropped_lines > 0` (malformed journal
  lines were previously silently discarded).
- **HTTP-flags error message**: The error for passing `--http`/`--server`/
  `--token` to local-only commands now correctly lists `incident` alongside
  the other query commands.
- **Docker image tag**: `docker-compose.prod.yml` defaults to `0.27.1` instead
  of `latest` for reproducible production deploys.
- **MCP search help**: `syslog help` now documents `exclude_facility`,
  `received_from`, and `received_to` for the `search` action.

### Refactor

- **Doctor tests**: Moved inline `#[cfg(test)] mod tests` block in `doctor.rs`
  to the standard sidecar `doctor_tests.rs` per repo layout rule.

## [0.27.0] - 2026-05-20

### Added

- **Self-debugging incident view**: Added `syslog incident --around ...` as a
  service-layer timeline that combines matching log rows with syslog-owned
  service journal entries.
- **Syslog service logs**: Added `syslog service logs SERVICE --json` backed by
  `SyslogService`, including safe allowlisting for syslog-owned user units.
- **Search filters**: Added facility exclusion and received-time filtering to
  search across CLI, API/MCP adapters, service models, and DB queries.

### Changed

- **AI watcher health**: `syslog ai watch-status --json` and `syslog doctor`
  now report watcher start time, DB schema version/currentness, schema drift,
  recent indexing failures, schema-like errors, affected paths, and stale
  indicators.

### Fixed

- **Facility naming**: Facility code 15 now stores `clockd`, matching
  `syslog_loose` and common syslog facility naming.
- **Release enforcement**: Pinned CI and crates publish GitHub Actions to full
  commit SHAs, added version-sync checks to CI/publish, made release
  changelog checks fail closed, and routed `just publish` through the repo
  release scripts plus test/clippy gates.

## [0.26.0] - 2026-05-18

### Breaking

- **`SYSLOG_API_ENABLED` removed**: the REST API at `/api/*` is now
  unconditionally mounted. Container startup requires a non-empty
  `SYSLOG_API_TOKEN` and fails fast without one. Run
  `syslog setup repair` BEFORE upgrading the container so the token is
  provisioned automatically (see `docs/rollout.md`).
- **`--local` CLI flag removed**: dropped in the cutover series — its
  behaviour is now the default unless `SYSLOG_USE_HTTP=true` is set
  (which `setup repair` writes on first install).

### Behavior change

- **CLI defaults to HTTP transport** via `/api/*` for every command
  with an HTTP backend (queries, AI, DB status/integrity/checkpoint/
  vacuum). Drift between the container's view of the database and the
  CLI's view is no longer possible for these commands. To opt out
  (e.g., for ad-hoc direct-DB queries during incident response),
  `unset SYSLOG_USE_HTTP` in the shell or remove the line from
  `~/.syslog-mcp/.env`. The CLI bails with a descriptive error if
  `--http` is passed to a local-only command (`db backup`,
  `ai index`/`add`/`doctor`/`smoke-watch`/`watch-status`/`watch`).
- **`setup repair`** now writes `SYSLOG_USE_HTTP=true` on first install
  using the same idempotent `entry().or_insert_with()` pattern as
  `SYSLOG_API_TOKEN`. Existing operator overrides (including
  `SYSLOG_USE_HTTP=false` and the empty value) are preserved
  byte-for-byte.

### Added

- **`docs/rollout.md`**: manual rollout playbook with pre-deploy
  checklist, deploy order, post-deploy verification windows, token
  rotation, and rollback procedure.
- **`scripts/smoke-test-http.sh`**: post-deploy smoke harness that
  exercises every HTTP-supported CLI command plus the local-only
  fallbacks. Run against a healthy container to verify the cutover.

## [0.25.4] - 2026-05-18

### Changed

- **Doctor orchestration boundaries**: Moved full doctor report collection and
  formatting out of `main.rs` into a dedicated doctor module.
- **HTTP CORS header allowlists**: MCP and non-MCP API CORS preflights now
  allow only the request headers required by browser clients instead of
  reflecting arbitrary headers.

### Fixed

- **Migration 13 drift recovery**: Startup now tolerates enrichment columns
  that already exist without a matching migration row and restores missing
  migration indexes/version metadata.
- **Review artifact preservation**: Copied the consolidated full-review issue
  register into tracked docs.
- **AI analytics query cost**: `search_ai_sessions` now computes session event
  counts with a grouped join backed by an AI session/host/time index, avoiding a
  full-history count per grouped result. `ai_correlate` now batches related-log
  window lookups into one bounded query instead of issuing one database search
  per anchor.
- **OTLP deferred endpoint auth parity**: `/v1/traces` now checks the same
  bearer token policy as `/v1/logs` and `/v1/metrics` before returning its
  deferred 404 response.
- **MCP action inventory docs**: Updated the README action list to include
  unaddressed error and notification administration actions.

## [0.25.2] - 2026-05-16

### Changed

- **AI abuse detector terminology**: Renamed the AI transcript detector surface
  from the legacy wording to abuse across the CLI, MCP action, docs, smoke
  tests, and plugin skill reference.

## [0.25.1] - 2026-05-15

### Fixed

- **AI abuse detector CLI responsiveness**: `syslog ai abuse` now uses the
  existing FTS5 index to find abuse candidates before applying the
  boundary-aware detector, making unfiltered local scans return quickly.
- **Query-only CLI noise**: CLI commands now suppress serve-mode config
  warnings so `syslog ai abuse` output starts with detector results instead of
  Docker/OAuth startup warnings.

## [0.25.0] - 2026-05-15

### Added

- **Source metadata JSON**: Added nullable `logs.metadata_json` storage and
  query response exposure for source-specific ingest metadata. Syslog rows now
  record parser/source provenance, OTLP rows preserve resource/log attributes
  plus trace/span ids, Docker rows preserve host/container/image/compose/action
  details, and transcript rows preserve source kind, file path, line number,
  record key, and scrub status.

### Fixed

- **AI tool schema parity**: Restored `gemini` to the MCP `tool` schema enum so
  validation matches the runtime parser, docs, and query behavior.

## [0.24.1] - 2026-05-15

### Fixed

- **OTLP source identity**: OTLP log ingest now stores the verified peer IP
  without the ephemeral source port, keeping source inventory and correlation
  stable across exporter reconnects.
- **Docker lifecycle event classification**: Docker event ingest now sanitizes
  health-status action names in `source_ip`, maps unhealthy health events to
  warnings, and maps clean `die exitCode=0` events to notice instead of warning.

## [0.24.0] - 2026-05-15

### Added

- **Docker lifecycle event ingest**: Docker ingest now persists container
  lifecycle events such as `create`, `start`, `restart`, `die`, `stop`,
  `destroy`, `rename`, and `oom` as searchable rows with
  `source_ip=docker-event://host/container/action`, enabling AI-session
  correlation against container restarts and rebuild/recreate activity.

## [0.23.1] - 2026-05-15

### Fixed

- **Docker log severity inference**: Docker ingest now uses explicit severity
  levels inside container log payloads before falling back to stream defaults,
  so stderr `INFO` lines remain informational while unclassified stderr lines
  still land as warnings.

## [0.23.0] - 2026-05-15

### Added

- **AI/log cross-reference**: Added `syslog ai correlate` and MCP
  `action="ai_correlate"` to use AI transcript rows as timeline anchors and
  pull nearby non-AI syslog, Docker, OTLP, and host events from the same DB.

### Fixed

- **Transcript exclusion for correlation**: Related log searches now exclude
  structured AI rows and legacy/plain transcript app rows such as
  `codex-transcript`, preventing AI session streams from correlating with
  themselves.

## [0.22.0] - 2026-05-15

### Added

- **AI abuse detector**: Added `syslog ai abuse` and MCP `action="abuse"` to
  detect abuse in AI transcript rows and return surrounding rows from the
  same AI session.

## [0.21.9] - 2026-05-14

### Added

- **DB maintenance CLI**: Added `syslog db status`, `integrity`,
  `checkpoint`, `vacuum`, and `backup` for direct SQLite maintenance from the
  same configured database used by MCP and stdio query mode.
- **Live watcher smoke command**: Added `syslog ai smoke-watch` to write a
  temporary transcript, prove the watcher ingests it, delete the file, and
  verify missing-checkpoint pruning.
- **Local debug setup doctoring**: Added `syslog setup debug-compose` and
  `syslog setup doctor` so the repo can install/check the local debug Compose
  override, debug wrapper, watcher service, transcript-root permissions, and
  runtime freshness from first-class commands.

### Fixed

- **Compose diagnostics from tool shells**: Compose/systemd inspection now
  retries `systemctl --user` with the inferred `/run/user/<uid>/bus`
  environment when the caller lacks `DBUS_SESSION_BUS_ADDRESS`.

## [0.21.8] - 2026-05-14

### Added

- **AI watcher hardening**: Added `syslog ai watch-status`,
  `syslog ai doctor --strict-permissions`, and
  `syslog setup debug-wrapper install|check|remove` for live watcher status,
  strict transcript-root ownership checks, and repo-managed local debug binary
  execution.
- **Local debug runtime checks**: `scripts/check-runtime-current.sh` now treats
  the repo-supported `syslog-mcp:local-debug` Compose image as a valid current
  runtime target while still rejecting arbitrary local images by default.

### Fixed

- **Deleted transcript cleanup**: The real-time transcript watcher now reacts to
  remove events by pruning missing scanner checkpoints, keeping structured
  checkpoint metadata bounded without deleting imported log rows.

## [0.21.7] - 2026-05-14

### Added

- **Real-time AI transcript ingestion**: Added `syslog ai watch` and
  `syslog setup ai-watch-service install|check|remove` for host-local
  filesystem watching of Claude and Codex transcript JSONL files. The watcher
  reuses scanner checkpoints, duplicate suppression, parse-error persistence,
  storage guardrails, and append-offset indexing while disabling the older
  polling timer during service install to avoid duplicate background ingestion.

## [0.21.6] - 2026-05-14

### Fixed

- **AI index timer activation**: `syslog setup ai-index-timer` now retries
  `systemctl --user` with the inferred `/run/user/<uid>/bus` environment when
  the caller lacks `DBUS_SESSION_BUS_ADDRESS`, so non-login tool contexts can
  still enable, disable, and check the host-local timer.

## [0.21.5] - 2026-05-14

### Added

- **Host-local AI index timer setup**: Added
  `syslog setup ai-index-timer install|check|remove` to make the optional
  transcript indexing timer syslog-owned while keeping it outside Docker
  Compose.
- **AI scanner diagnostics**: Added `syslog ai doctor`, `syslog ai errors`,
  `syslog ai checkpoints --missing`, and
  `syslog ai prune-checkpoints --missing` for live DB visibility and cleanup.
- **Binary freshness doctor**: Added `syslog doctor binary` to report host
  binary resolution, container version, and runtime-current status.
- **AI MCP smoke coverage**: Added `scripts/smoke-ai-mcp.sh` to seed a fixture
  and exercise the AI MCP actions over HTTP.

### Changed

- **Parse error storage**: Scanner parse failures are now persisted in
  `transcript_parse_errors` with bounded scrubbed previews and are cleared when
  a source indexes cleanly.

## [0.21.4] - 2026-05-14

### Added

- **AI scanner operations**: Added `syslog ai checkpoints`, `syslog ai index
  --force`, `syslog ai index --since`, and `syslog ai add --file` so scanner
  state, parser backfills, and selective reindexing are first-class CLI
  workflows.
- **AI smoke coverage**: Added `scripts/smoke-ai.sh` for live AI transcript
  indexing/search/tail/checkpoint verification.

### Fixed

- **Runtime freshness check**: `scripts/check-runtime-current.sh` now verifies
  the running Compose container binary version against the repo version in
  addition to comparing Docker image IDs.
- **Legacy unit cleanup**: Setup repair now removes stale `mnemo-index.*` user
  units alongside the removed `syslog-mcp.service` systemd deployment.
- **AI search truncation visibility**: Search responses now expose candidate
  window metadata so capped grouping is explicit in JSON and human output.

## [0.21.3] - 2026-05-14

### Fixed

- **AI transcript review cleanup**: Limited transcript-style CLI rendering to
  actual transcript rows, kept the tool column bounded, made explicit `ai add
  --file` inputs detect Codex JSONL shape outside the default session tree, and
  tightened scanner checkpoints so same-size rewrites and concurrently appended
  files are not skipped as fully indexed.
- **AI session search ordering**: Ordered capped FTS candidates by newest row so
  recent matching sessions are not hidden behind older high-volume transcript
  history.
- **Plugin metadata**: Removed the stale SSE endpoint claim from the Claude
  plugin MCP port description.

## [0.21.2] - 2026-05-14

### Fixed

- **AI transcript CLI rendering**: Render transcript rows in human CLI output
  with AI tool, project, and session context instead of the synthetic
  `localhost` hostname used by the storage layer.

## [0.21.1] - 2026-05-13

### Fixed

- **AI transcript CLI reliability**: Pointed the local debug CLI at the live
  Compose data volume, made default indexing tolerate unreadable discovered
  transcript directories without failing, and raised production scanner limits
  for large Codex session metadata records.
- **AI indexing performance**: Added metadata checkpoint short-circuiting for
  unchanged Claude/Codex transcript files so repeated `syslog ai index` runs do
  not reread the full transcript history.
- **AI search latency**: Removed the expensive per-row FTS relevance sort from
  grouped AI session search so common transcript terms stay usable on the live
  multi-hundred-thousand-row database.

## [0.21.0] - 2026-05-13

### Added

- **Shared setup command**: Added `syslog setup`, `syslog setup check`, and
  `syslog setup repair` so the one-line installer and Claude Code plugin use
  the same canonical host layout under `~/.syslog-mcp`.
- **One-line installer**: Added `install.sh` to install the `syslog` binary and
  optionally run Docker Compose setup from the installed binary.

### Changed

- **Plugin setup convergence**: Reworked the plugin hook into a thin
  userConfig-to-env bridge that ensures the binary exists and delegates all
  host repair to `syslog setup repair`.
- **Installed CLI config loading**: Installed commands now load
  `$SYSLOG_MCP_HOME/.env` or `~/.syslog-mcp/.env` automatically, while explicit
  process environment variables still win.

## [0.20.2] - 2026-05-13

### Changed

- **Compose-only plugin deployment**: Removed the systemd deployment mode,
  deploy-mode cutover skill, and mode-aware setup paths. Server-mode plugin
  installs now manage syslog-mcp only with Docker Compose while still removing
  stale user units/drop-ins left by older versions.

### Fixed

- **Runtime freshness checks**: Narrowed the runtime-current checker and related
  plugin skills to Docker Compose so stale systemd units or plugin-cache
  binaries are no longer treated as valid deployment targets.

## [0.20.1] - 2026-05-13

### Fixed

- **AI transcript indexing hardening**: Stream scanner reads line-by-line with
  bounded chunked transactions, broad path rejection, symlink/unsupported-file
  counters, storage-budget preflight checks, and checkpoint timestamps that only
  advance after successful imports.
- **Codex/Claude transcript parsing**: Preserve Codex file-level session
  metadata instead of treating response item ids as session ids, and parse
  Claude object-array content shapes.
- **AI analytics bounds**: Add truncation metadata for tool/project inventories,
  default usage blocks to a bounded lookback, and return bounded project-context
  message snippets.

### Changed

- Updated CLI, MCP, README, and expansion docs to describe scanner path policy,
  redaction behavior, storage blocking, and AI action result limits.

## [0.20.0] - 2026-05-12

### Added

- **Compose lifecycle CLI**: Added `syslog compose status`, `doctor`, `pull`,
  `up`, `restart`, `down`, and `logs` commands with live Compose target
  discovery, mutation preflight checks, bounded subprocess output, and JSON
  output support.
- **Compose MCP diagnostics**: Added read-only `compose_status` and
  `compose_doctor` MCP actions with redacted deployment state, published port
  summaries, and existing `syslog:read` scope enforcement.

### Changed

- Updated deployment, CLI, MCP schema, smoke-test, and plugin docs for the
  compose lifecycle surface.

## [0.19.2] - 2026-05-11

### Fixed

- **Scanner error resilience**: Replace hard-fail error propagation with graceful
  per-path error accumulation so a single unreadable directory or file no longer
  aborts the entire scan; errors are collected into `IndexResult` and reported
  at the end.
- **Config db_path default**: Changed default `db_path` from `/data/syslog.db`
  to the relative `data/syslog.db` so local dev builds work out of the box.

### Changed

- Renamed `supported_file` → `supported_discovered_file` for clarity.
- Extended test coverage for scanner path validation and config defaults.

## [0.19.1] - 2026-05-11

### Fixed

- **AI transcript indexing safety**: Scrub manually indexed transcript content
  before FTS storage, parse Codex JSONL records with a Codex-aware parser, and
  derive Claude project paths from `sessions-index.json` during scanner imports.
- **Transcript checkpointing**: Use stable event/content checkpoint keys and
  commit transcript log rows plus checkpoint records in one transaction.
- **Scanner reporting**: Report per-file indexing errors with paths and make CLI
  indexing fail when any file could not be indexed.
- **AI session search counts**: Report total session event counts separately from
  FTS match counts.
- **OTLP metrics endpoint**: Return an unsupported response for `/v1/metrics`
  instead of acknowledging and discarding metrics.

### Changed

- Removed remaining `mod.rs` module files in favor of modern Rust module files.
- Updated MCP/CLI documentation and live smoke coverage for the AI action
  surface.

## [0.19.0] - 2026-05-11

### Added

- **AI session analytics**: Added ranked `search_sessions`, 5-hour `usage_blocks`,
  `project_context`, `list_ai_tools`, and `list_ai_projects` across the existing
  `logs` AI metadata columns.
- **CLI AI namespace**: Added `syslog ai search|blocks|context|tools|projects|index|add`
  for explicit AI-session querying and transcript indexing from the terminal.
- **Transcript indexing**: Added local transcript scanning with checkpoint tables,
  duplicate prevention, and explicit `syslog ai index` / `syslog ai add` flows.

### Changed

- **OTLP AI metadata mapping**: OTLP ingestion now accepts trusted explicit
  `ai.tool` attributes for known tools and enforces length caps on AI metadata fields.
- **MCP action surface**: The single `syslog` MCP tool now exposes the new AI
  analytics actions while preserving existing `sessions` compatibility.

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

[Unreleased]: https://github.com/jmagar/cortex/compare/v1.27.1...HEAD
[1.27.1]: https://github.com/jmagar/cortex/compare/v1.27.0...v1.27.1
[1.27.0]: https://github.com/jmagar/cortex/compare/v1.26.1...v1.27.0
[1.26.1]: https://github.com/jmagar/cortex/compare/v1.26.0...v1.26.1
[1.26.0]: https://github.com/jmagar/cortex/compare/v1.25.1...v1.26.0
[1.25.1]: https://github.com/jmagar/cortex/compare/v1.20.0...v1.25.1
[1.20.0]: https://github.com/jmagar/cortex/compare/v1.19.0...v1.20.0
[1.14.0]: https://github.com/jmagar/cortex/compare/v1.13.3...v1.14.0
[1.13.3]: https://github.com/jmagar/cortex/compare/v1.13.2...v1.13.3
[1.13.2]: https://github.com/jmagar/cortex/compare/v1.13.1...v1.13.2
[1.13.1]: https://github.com/jmagar/cortex/compare/v1.13.0...v1.13.1
[1.13.0]: https://github.com/jmagar/cortex/compare/v1.12.0...v1.13.0
[1.12.0]: https://github.com/jmagar/cortex/compare/v1.11.0...v1.12.0
[1.11.0]: https://github.com/jmagar/cortex/compare/v1.10.1...v1.11.0
[1.10.1]: https://github.com/jmagar/cortex/compare/v1.10.0...v1.10.1
[1.10.0]: https://github.com/jmagar/cortex/compare/v1.9.1...v1.10.0
[1.9.1]: https://github.com/jmagar/cortex/compare/v1.9.0...v1.9.1
[1.9.0]: https://github.com/jmagar/cortex/compare/v1.8.0...v1.9.0
[1.8.0]: https://github.com/jmagar/cortex/compare/v0.32.3...v1.8.0
[0.32.3]: https://github.com/jmagar/syslog-mcp/compare/v0.32.2...v0.32.3
[0.32.2]: https://github.com/jmagar/syslog-mcp/compare/v0.32.1...v0.32.2
[0.32.1]: https://github.com/jmagar/syslog-mcp/compare/v0.32.0...v0.32.1
[0.32.0]: https://github.com/jmagar/syslog-mcp/compare/v0.31.3...v0.32.0
[0.31.3]: https://github.com/jmagar/syslog-mcp/compare/v0.31.2...v0.31.3
[0.31.2]: https://github.com/jmagar/syslog-mcp/compare/v0.31.1...v0.31.2
[0.31.1]: https://github.com/jmagar/syslog-mcp/compare/v0.31.0...v0.31.1
[0.31.0]: https://github.com/jmagar/syslog-mcp/compare/v0.30.3...v0.31.0
[0.30.3]: https://github.com/jmagar/syslog-mcp/compare/v0.30.2...v0.30.3
[0.30.2]: https://github.com/jmagar/syslog-mcp/compare/v0.30.1...v0.30.2
[0.30.1]: https://github.com/jmagar/syslog-mcp/compare/v0.30.0...v0.30.1
[0.30.0]: https://github.com/jmagar/syslog-mcp/compare/v0.29.0...v0.30.0
[0.29.0]: https://github.com/jmagar/syslog-mcp/compare/v0.28.2...v0.29.0
[0.28.2]: https://github.com/jmagar/syslog-mcp/compare/v0.28.1...v0.28.2
[0.28.1]: https://github.com/jmagar/syslog-mcp/compare/v0.28.0...v0.28.1
[0.28.0]: https://github.com/jmagar/syslog-mcp/compare/v0.27.4...v0.28.0
[0.27.4]: https://github.com/jmagar/syslog-mcp/compare/v0.27.3...v0.27.4
[0.27.3]: https://github.com/jmagar/syslog-mcp/compare/v0.27.2...v0.27.3
[0.27.2]: https://github.com/jmagar/syslog-mcp/compare/v0.27.1...v0.27.2
[0.27.1]: https://github.com/jmagar/syslog-mcp/compare/v0.27.0...v0.27.1
[0.27.0]: https://github.com/jmagar/syslog-mcp/compare/v0.26.0...v0.27.0
[0.26.0]: https://github.com/jmagar/syslog-mcp/compare/v0.25.4...v0.26.0
[0.25.4]: https://github.com/jmagar/syslog-mcp/compare/v0.25.3...v0.25.4
[0.25.3]: https://github.com/jmagar/syslog-mcp/compare/v0.25.2...v0.25.3
[0.25.2]: https://github.com/jmagar/syslog-mcp/compare/v0.25.1...v0.25.2
[0.25.1]: https://github.com/jmagar/syslog-mcp/compare/v0.25.0...v0.25.1
[0.25.0]: https://github.com/jmagar/syslog-mcp/compare/v0.24.1...v0.25.0
[0.24.1]: https://github.com/jmagar/syslog-mcp/compare/v0.24.0...v0.24.1
[0.24.0]: https://github.com/jmagar/syslog-mcp/compare/v0.23.1...v0.24.0
[0.23.1]: https://github.com/jmagar/syslog-mcp/compare/v0.23.0...v0.23.1
[0.23.0]: https://github.com/jmagar/syslog-mcp/compare/v0.22.0...v0.23.0
[0.22.0]: https://github.com/jmagar/syslog-mcp/compare/v0.21.9...v0.22.0
[0.21.9]: https://github.com/jmagar/syslog-mcp/compare/v0.21.8...v0.21.9
[0.21.8]: https://github.com/jmagar/syslog-mcp/compare/v0.21.7...v0.21.8
[0.21.7]: https://github.com/jmagar/syslog-mcp/compare/v0.21.6...v0.21.7
[0.21.6]: https://github.com/jmagar/syslog-mcp/compare/v0.21.5...v0.21.6
[0.21.5]: https://github.com/jmagar/syslog-mcp/compare/v0.21.4...v0.21.5
[0.21.4]: https://github.com/jmagar/syslog-mcp/compare/v0.21.3...v0.21.4
[0.21.3]: https://github.com/jmagar/syslog-mcp/compare/v0.21.2...v0.21.3
[0.21.2]: https://github.com/jmagar/syslog-mcp/compare/v0.21.1...v0.21.2
[0.21.1]: https://github.com/jmagar/syslog-mcp/compare/v0.21.0...v0.21.1
[0.21.0]: https://github.com/jmagar/syslog-mcp/compare/v0.20.2...v0.21.0
[0.20.2]: https://github.com/jmagar/syslog-mcp/compare/v0.20.1...v0.20.2
[0.20.1]: https://github.com/jmagar/syslog-mcp/compare/v0.20.0...v0.20.1
[0.20.0]: https://github.com/jmagar/syslog-mcp/compare/v0.19.2...v0.20.0
[0.19.2]: https://github.com/jmagar/syslog-mcp/compare/v0.19.1...v0.19.2
[0.19.1]: https://github.com/jmagar/syslog-mcp/compare/v0.19.0...v0.19.1
[0.19.0]: https://github.com/jmagar/syslog-mcp/compare/v0.18.0...v0.19.0
[0.18.0]: https://github.com/jmagar/syslog-mcp/compare/v0.17.7...v0.18.0
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
