#!/usr/bin/env bash
# =============================================================================
# scripts/smoke-test-http.sh — bead syslog-mcp-0p8r.10 cutover smoke test
#
# Exercises every CLI command that was routed through the container REST API
# in waves 7-9 (queries + AI + DB), plus verifies that LOCAL-only commands
# still work in default mode and that `db backup --http` bails with the
# documented error.
#
# Required environment:
#   SYSLOG_API_TOKEN     Bearer token for the /api/* endpoints. If unset,
#                        this script falls back to grep'ing
#                        ${SYSLOG_ENV_FILE:-${HOME}/.syslog-mcp/.env}.
#
# Optional environment:
#   SYSLOG_BIN           Path to the `syslog` binary (default: `syslog` on PATH).
#   SYSLOG_SERVER        Base URL of the REST API (default: http://localhost:3100).
#   SYSLOG_ENV_FILE      Path to .env containing SYSLOG_API_TOKEN
#                        (default: ${HOME}/.syslog-mcp/.env).
#
# Exit:
#   0  — every assertion passed.
#   1  — first failure (descriptive line on stderr).
#   2  — prerequisite missing (binary, token, server unreachable).
#
# This script is intentionally serial: a failure on any command should halt
# the run so the operator can diagnose before the rest of the deploy
# pipeline continues.
# =============================================================================

set -euo pipefail

SCRIPT_NAME="$(basename "$0")"
SYSLOG_BIN="${SYSLOG_BIN:-syslog}"
SYSLOG_SERVER="${SYSLOG_SERVER:-http://localhost:3100}"
SYSLOG_ENV_FILE="${SYSLOG_ENV_FILE:-${HOME}/.syslog-mcp/.env}"

pass() { printf 'PASS  %s\n' "$1"; }
info() { printf 'INFO  %s\n' "$1"; }
fail() {
  local fmt="$1"; shift
  # shellcheck disable=SC2059
  printf "FAIL  ${fmt}\n" "$@" >&2
  exit 1
}
need() { printf 'NEED  %s\n' "$1" >&2; exit 2; }

# ─── Prereqs ────────────────────────────────────────────────────────────────

command -v "$SYSLOG_BIN" >/dev/null 2>&1 || need "syslog binary not on PATH (set SYSLOG_BIN=...)"
command -v jq >/dev/null 2>&1            || need "jq is required (install jq)"

if [[ -z "${SYSLOG_API_TOKEN:-}" ]]; then
  if [[ -r "$SYSLOG_ENV_FILE" ]]; then
    # shellcheck disable=SC2155
    SYSLOG_API_TOKEN="$(grep -E '^SYSLOG_API_TOKEN=' "$SYSLOG_ENV_FILE" | head -n1 | cut -d= -f2-)"
  fi
fi
# Trim leading/trailing whitespace so blank-padded values are rejected.
# Guard against `set -u` blowing up if the var is entirely unset.
SYSLOG_API_TOKEN="${SYSLOG_API_TOKEN:-}"
SYSLOG_API_TOKEN="${SYSLOG_API_TOKEN#"${SYSLOG_API_TOKEN%%[![:space:]]*}"}"
SYSLOG_API_TOKEN="${SYSLOG_API_TOKEN%"${SYSLOG_API_TOKEN##*[![:space:]]}"}"
[[ -n "${SYSLOG_API_TOKEN}" ]] || need "SYSLOG_API_TOKEN not set/blank (checked env and $SYSLOG_ENV_FILE)"
export SYSLOG_API_TOKEN

info "binary  : $(command -v "$SYSLOG_BIN")"
info "version : $("$SYSLOG_BIN" --version 2>/dev/null || echo unknown)"
info "server  : $SYSLOG_SERVER"

# ─── Helpers ────────────────────────────────────────────────────────────────

# Run a command and require its stdout to be a parseable JSON value.
# Usage: assert_json "label" syslog --http --json ...
assert_json() {
  local label="$1"; shift
  local out
  if ! out="$("$@" 2>&1)"; then
    printf '  stderr: %s\n' "$out" >&2
    fail "$label: command exited non-zero"
  fi
  if ! printf '%s' "$out" | jq -e . >/dev/null 2>&1; then
    printf '  stdout: %s\n' "${out:0:400}" >&2
    fail "$label: stdout is not valid JSON"
  fi
  pass "$label"
}

# Assert a command fails AND its combined output contains an expected
# substring. Used for the documented `db backup --http` bail.
assert_fails_with() {
  local label="$1"; local needle="$2"; shift 2
  local out
  if out="$("$@" 2>&1)"; then
    printf '  unexpected success, stdout: %s\n' "${out:0:400}" >&2
    fail "$label: expected failure but command exited 0"
  fi
  if ! grep -qF "$needle" <<<"$out"; then
    printf '  output: %s\n' "${out:0:400}" >&2
    fail "$label: expected error containing %s" "$needle"
  fi
  pass "$label"
}

http() {
  "$SYSLOG_BIN" --http --server "$SYSLOG_SERVER" --token "$SYSLOG_API_TOKEN" "$@"
}

# Default mode = no --http, no SYSLOG_USE_HTTP. Local SQL path.
local_mode() {
  env -u SYSLOG_USE_HTTP "$SYSLOG_BIN" "$@"
}

# ─── HTTP-supported query commands (7) ──────────────────────────────────────
#
# CLI globals (--http, --server, --token) are stripped by Mode::parse before
# subcommand parsing; per-command flags (including --json) MUST follow the
# subcommand or they're rejected as an unknown leading argument.

SYSLOG_SMOKE_REFTIME="${SYSLOG_SMOKE_REFTIME:-$(date -u +"%Y-%m-%dT%H:%M:%SZ")}"

assert_json "http: search (limit 1)"     http search --json --limit 1
assert_json "http: tail (limit 1)"       http tail --json --limit 1
assert_json "http: errors"               http errors --json
assert_json "http: hosts"                http hosts --json
assert_json "http: correlate (1m, h=_)"  http correlate --json --reference-time "$SYSLOG_SMOKE_REFTIME" --hostname _smoke_ --window-minutes 1
assert_json "http: stats"                http stats --json
assert_json "http: sessions (limit 1)"   http sessions --json --limit 1

# ─── HTTP-supported AI commands (10) ────────────────────────────────────────

assert_json "http: ai search"            http ai search --json --query smoke --limit 1
assert_json "http: ai abuse"             http ai abuse --json --limit 1
assert_json "http: ai correlate"         http ai correlate --json --window-minutes 1 --limit 1
assert_json "http: ai blocks"            http ai blocks --json
assert_json "http: ai context"           http ai context --json --project / --limit 1
assert_json "http: ai tools"             http ai tools --json
assert_json "http: ai projects"          http ai projects --json
assert_json "http: ai checkpoints"       http ai checkpoints --json --limit 1
assert_json "http: ai errors"            http ai errors --json --limit 1
assert_json "http: ai prune-checkpoints" http ai prune-checkpoints --json --missing --dry-run --limit 1

# ─── HTTP-supported DB commands (4) ─────────────────────────────────────────

assert_json "http: db status"            http db status --json
# `db integrity --quick` keeps runtime predictable. The CLI bails non-zero
# only if integrity actually fails — JSON parses either way.
assert_json "http: db integrity (quick)" http db integrity --json --quick
assert_json "http: db checkpoint"        http db checkpoint --json --mode passive
# `vacuum --pages 1` keeps wall-clock low; full VACUUM may exceed 10-min
# HTTP timeout on large DBs (see docs/rollout.md Notes).
assert_json "http: db vacuum (pages=1)"  http db vacuum --json --pages 1

# ─── LOCAL-only AI commands (6) — must succeed in default mode ──────────────
# These all read host filesystem or run host-side processes. They are
# expected to bail with a descriptive error if invoked with --http (and
# bead .8 wired that bail in). Here we only assert the DEFAULT (no-flag)
# path returns JSON or, for daemon commands, runs briefly without crashing.

assert_json "local: ai doctor"           local_mode ai doctor --json
assert_json "local: ai watch-status"     local_mode ai watch-status --json
# `ai index` writes to the DB; keep it side-effect-light by indexing an
# empty tempdir if SYSLOG_SMOKE_AI_INDEX_PATH is set, else skip the index
# side-effect and just exercise `ai add` against /dev/null which bails
# cleanly. We assert the default `ai index --help` path produces help
# output (still part of the CLI surface) rather than mutating DB state.
SYSLOG_SMOKE_AI_TMP="$(mktemp -d)"
trap 'rm -rf "$SYSLOG_SMOKE_AI_TMP"' EXIT
assert_json "local: ai index (empty dir)" local_mode ai index --path "$SYSLOG_SMOKE_AI_TMP" --json
# `ai add` requires a real file path; create a minimal stub.
printf '{}\n' >"$SYSLOG_SMOKE_AI_TMP/empty.jsonl"
assert_json "local: ai add (stub)"       local_mode ai add --file "$SYSLOG_SMOKE_AI_TMP/empty.jsonl" --json
# `ai smoke-watch` writes a synthetic transcript and runs ai-watch briefly.
# Gate behind a flag — it's slower (~30s) and not appropriate for every
# CI run. Operators opt in via SYSLOG_SMOKE_RUN_AI_WATCH=1.
if [[ "${SYSLOG_SMOKE_RUN_AI_WATCH:-0}" == "1" ]]; then
  assert_json "local: ai smoke-watch"    local_mode ai smoke-watch --json
else
  info "skip: ai smoke-watch (set SYSLOG_SMOKE_RUN_AI_WATCH=1 to include)"
fi
# `ai watch` is a daemon — we do NOT run it from a smoke test.
info "skip: ai watch (long-running daemon; tested by systemd unit healthchecks)"

# ─── LOCAL-only DB: backup ──────────────────────────────────────────────────

# `db backup` must succeed in DEFAULT mode.
SYSLOG_SMOKE_BACKUP="$SYSLOG_SMOKE_AI_TMP/backup.db"
assert_json "local: db backup (default)" local_mode db backup --output "$SYSLOG_SMOKE_BACKUP" --json
[[ -s "$SYSLOG_SMOKE_BACKUP" ]] || fail "local: db backup did not produce a non-empty file at $SYSLOG_SMOKE_BACKUP"

# `db backup --http` must bail with the documented error from
# src/cli/dispatch.rs run_db_backup (substring check tolerates minor
# wording tweaks but anchors on the unique "db backup currently runs locally"
# phrase).
assert_fails_with \
  "http: db backup bails as documented" \
  "db backup currently runs locally" \
  http --json db backup --output "$SYSLOG_SMOKE_AI_TMP/should-not-exist.db"

printf '\nAll %s assertions passed.\n' "$SCRIPT_NAME"
