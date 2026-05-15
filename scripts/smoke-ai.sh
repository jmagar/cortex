#!/usr/bin/env bash
# Focused smoke test for local AI transcript indexing/query CLI surfaces.
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SYSLOG_BIN="${SYSLOG_BIN:-}"
PYTHON_BIN="${PYTHON_BIN:-python3}"
DB_PATH="${SYSLOG_SMOKE_DB_PATH:-${SYSLOG_MCP_DB_PATH:-${PROJECT_DIR}/data/syslog.db}}"
SOURCE_FIXTURE="${SYSLOG_AI_SMOKE_FIXTURE:-${PROJECT_DIR}/tests/fixtures/ai-session-smoke.jsonl}"
QUERY="${SYSLOG_AI_SMOKE_QUERY:-\"ai-smoke\"}"

pass() {
  printf 'PASS  %s\n' "$1"
}

fail() {
  printf 'FAIL  %s\n' "$1" >&2
  exit 1
}

resolve_syslog_bin() {
  if [[ -n "$SYSLOG_BIN" ]]; then
    if [[ -x "$SYSLOG_BIN" ]]; then
      printf '%s\n' "$SYSLOG_BIN"
    elif command -v "$SYSLOG_BIN" >/dev/null 2>&1; then
      command -v "$SYSLOG_BIN"
    else
      fail "SYSLOG_BIN is not executable or on PATH: $SYSLOG_BIN"
    fi
  elif command -v syslog >/dev/null 2>&1; then
    command -v syslog
  elif [[ -x "${PROJECT_DIR}/target/debug/syslog" ]]; then
    printf '%s\n' "${PROJECT_DIR}/target/debug/syslog"
  else
    fail "syslog binary not found; install syslog on PATH, set SYSLOG_BIN, or run cargo build"
  fi
}

run_syslog() {
  SYSLOG_MCP_DB_PATH="$DB_PATH" \
    SYSLOG_DOCKER_INGEST_ENABLED="${SYSLOG_DOCKER_INGEST_ENABLED:-false}" \
    RUST_LOG="${RUST_LOG:-error}" \
    "$SYSLOG_BIN" "$@"
}

require_json_count() {
  local label="$1"
  local json="$2"
  local expr="$3"
  JSON_INPUT="$json" "$PYTHON_BIN" - "$expr" <<'PY' >/dev/null || fail "$label"
import json
import os
import sys

data = json.loads(os.environ["JSON_INPUT"])
scope = {"__builtins__": {}, "data": data, "any": any, "len": len}
if not eval(sys.argv[1], scope, {}):
    sys.exit(1)
PY
}

cd "$PROJECT_DIR"

SYSLOG_BIN="$(resolve_syslog_bin)"
[[ -x "$SYSLOG_BIN" ]] || fail "$SYSLOG_BIN is not executable"
[[ -f "$SOURCE_FIXTURE" ]] || fail "fixture missing: $SOURCE_FIXTURE"

SMOKE_DIR="$(mktemp -d)"
trap 'rm -rf "$SMOKE_DIR"' EXIT
FIXTURE="$SMOKE_DIR/ai-session-smoke.jsonl"
now="$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
{
  printf '{"sessionId":"ai-smoke-session","timestamp":"%s","cwd":"/tmp/syslog-mcp-ai-smoke","content":"ai-smoke-authentication smoke transcript seed from Claude"}\n' "$now"
  printf '{"sessionId":"ai-smoke-session","timestamp":"%s","cwd":"/tmp/syslog-mcp-ai-smoke","content":[{"type":"text","text":"ai-smoke-project-context object array content"}]}\n' "$now"
} >"$FIXTURE"

printf 'syslog: %s\n' "$("$SYSLOG_BIN" --version)"
printf 'db:     %s\n' "$DB_PATH"
printf 'query:  %s\n' "$QUERY"

add_json="$(run_syslog ai add --file "$FIXTURE" --force --json)"
require_json_count "ai add did not report ingested rows" "$add_json" "data.get('ingested', 0) >= 1"
pass "ai add --file --force"

index_json="$(run_syslog ai index --path "$FIXTURE" --json)"
require_json_count "ai index did not discover the fixture" "$index_json" "data.get('discovered_files') == 1"
pass "ai index --path"

inventory="$(run_syslog ai tools --json)"
require_json_count "ai tools did not include any tools" "$inventory" "len(data.get('tools', [])) >= 1"
pass "ai tools"

sessions="$(run_syslog sessions --tool claude --project /tmp/syslog-mcp-ai-smoke --limit 5 --json)"
require_json_count "sessions did not include fixture session" "$sessions" "any(s.get('session_id') == 'ai-smoke-session' for s in data.get('sessions', []))"
pass "sessions --tool claude"

search="$(run_syslog ai search "$QUERY" --tool claude --project /tmp/syslog-mcp-ai-smoke --limit 5 --json)"
require_json_count "ai search did not return the fixture session" "$search" "any(s.get('session_id') == 'ai-smoke-session' for s in data.get('sessions', []))"
pass "ai search"

checkpoints="$(run_syslog ai checkpoints --limit 20 --json)"
require_json_count "ai checkpoints did not include fixture source" "$checkpoints" "any(item.get('canonical_path', '').endswith('ai-session-smoke.jsonl') for item in data)"
pass "ai checkpoints"

tail_output="$(run_syslog tail -n 5 --app-name claude-transcript)"
grep -q 'ai-smoke-session' <<<"$tail_output" || fail "tail output did not include fixture session"
if grep -qE '\blocalhost\b' <<<"$tail_output"; then
  fail "tail output still shows synthetic localhost transcript row"
fi
pass "tail transcript rendering"

if [[ "${SYSLOG_AI_SMOKE_CHECK_RUNTIME:-1}" == "1" ]]; then
  if bash scripts/check-runtime-current.sh >/tmp/syslog-ai-runtime-current.out 2>&1; then
    pass "compose runtime current"
  else
    printf 'FAIL  compose runtime current check failed:\n' >&2
    sed 's/^/      /' /tmp/syslog-ai-runtime-current.out >&2
    exit 1
  fi
elif [[ "${SYSLOG_AI_SMOKE_CHECK_RUNTIME:-1}" == "warn" ]]; then
  if bash scripts/check-runtime-current.sh >/tmp/syslog-ai-runtime-current.out 2>&1; then
    pass "compose runtime current"
  else
    printf 'WARN  compose runtime current check failed:\n' >&2
    sed 's/^/      /' /tmp/syslog-ai-runtime-current.out >&2
  fi
fi

printf 'OK    AI transcript smoke passed\n'
