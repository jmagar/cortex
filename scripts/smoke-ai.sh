#!/usr/bin/env bash
# Focused smoke test for local AI transcript indexing/query CLI surfaces.
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORTEX_BIN="${CORTEX_BIN:-}"
PYTHON_BIN="${PYTHON_BIN:-python3}"
DB_PATH="${CORTEX_SMOKE_DB_PATH:-${CORTEX_DB_PATH:-${PROJECT_DIR}/data/cortex.db}}"
SOURCE_FIXTURE="${CORTEX_AI_SMOKE_FIXTURE:-${PROJECT_DIR}/tests/fixtures/ai-session-smoke.jsonl}"
QUERY="${CORTEX_AI_SMOKE_QUERY:-\"ai-smoke\"}"

pass() {
  printf 'PASS  %s\n' "$1"
}

fail() {
  printf 'FAIL  %s\n' "$1" >&2
  exit 1
}

resolve_cortex_bin() {
  if [[ -n "$CORTEX_BIN" ]]; then
    if [[ -x "$CORTEX_BIN" ]]; then
      printf '%s\n' "$CORTEX_BIN"
    elif command -v "$CORTEX_BIN" >/dev/null 2>&1; then
      command -v "$CORTEX_BIN"
    else
      fail "CORTEX_BIN is not executable or on PATH: $CORTEX_BIN"
    fi
  elif command -v cortex >/dev/null 2>&1; then
    command -v cortex
  elif [[ -x "${PROJECT_DIR}/target/debug/cortex" ]]; then
    printf '%s\n' "${PROJECT_DIR}/target/debug/cortex"
  else
    fail "cortex binary not found; install cortex on PATH, set CORTEX_BIN, or run cargo build"
  fi
}

run_cortex() {
  CORTEX_DB_PATH="$DB_PATH" \
    CORTEX_DOCKER_INGEST_ENABLED="${CORTEX_DOCKER_INGEST_ENABLED:-false}" \
    RUST_LOG="${RUST_LOG:-error}" \
    "$CORTEX_BIN" "$@"
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

CORTEX_BIN="$(resolve_cortex_bin)"
[[ -x "$CORTEX_BIN" ]] || fail "$CORTEX_BIN is not executable"
[[ -f "$SOURCE_FIXTURE" ]] || fail "fixture missing: $SOURCE_FIXTURE"

SMOKE_DIR="$(mktemp -d)"
trap 'rm -rf "$SMOKE_DIR"' EXIT
FIXTURE="$SMOKE_DIR/ai-session-smoke.jsonl"
now="$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
{
  printf '{"sessionId":"ai-smoke-session","timestamp":"%s","cwd":"/tmp/cortex-ai-smoke","content":"ai-smoke-authentication smoke transcript seed from Claude"}\n' "$now"
  printf '{"sessionId":"ai-smoke-session","timestamp":"%s","cwd":"/tmp/cortex-ai-smoke","content":[{"type":"text","text":"ai-smoke-project-context object array content"}]}\n' "$now"
} >"$FIXTURE"

printf 'cortex: %s\n' "$("$CORTEX_BIN" --version)"
printf 'db:     %s\n' "$DB_PATH"
printf 'query:  %s\n' "$QUERY"

add_json="$(run_cortex sessions add --file "$FIXTURE" --force --json)"
require_json_count "sessions add did not report ingested rows" "$add_json" "data.get('ingested', 0) >= 1"
pass "sessions add --file --force"

index_json="$(run_cortex sessions index --path "$FIXTURE" --json)"
require_json_count "sessions index did not discover the fixture" "$index_json" "data.get('discovered_files') == 1"
pass "sessions index --path"

inventory="$(run_cortex sessions tools --json)"
require_json_count "sessions tools did not include any tools" "$inventory" "len(data.get('tools', [])) >= 1"
pass "sessions tools"

sessions="$(run_cortex sessions --tool claude --project /tmp/cortex-ai-smoke --limit 5 --json)"
require_json_count "sessions did not include fixture session" "$sessions" "any(s.get('session_id') == 'ai-smoke-session' for s in data.get('sessions', []))"
pass "sessions --tool claude"

search="$(run_cortex sessions search "$QUERY" --tool claude --project /tmp/cortex-ai-smoke --limit 5 --json)"
require_json_count "sessions search did not return the fixture session" "$search" "any(s.get('session_id') == 'ai-smoke-session' for s in data.get('sessions', []))"
pass "sessions search"

checkpoints="$(run_cortex sessions checkpoints --limit 20 --json)"
require_json_count "sessions checkpoints did not include fixture source" "$checkpoints" "any(item.get('canonical_path', '').endswith('ai-session-smoke.jsonl') for item in data)"
pass "sessions checkpoints"

tail_output="$(run_cortex tail -n 5 --app claude-transcript)"
grep -q 'ai-smoke-session' <<<"$tail_output" || fail "tail output did not include fixture session"
if grep -qE '\blocalhost\b' <<<"$tail_output"; then
  fail "tail output still shows synthetic localhost transcript row"
fi
pass "tail transcript rendering"

if [[ "${CORTEX_AI_SMOKE_CHECK_RUNTIME:-1}" == "1" ]]; then
  if bash scripts/check-runtime-current.sh >/tmp/cortex-ai-runtime-current.out 2>&1; then
    pass "compose runtime current"
  else
    printf 'FAIL  compose runtime current check failed:\n' >&2
    sed 's/^/      /' /tmp/cortex-ai-runtime-current.out >&2
    exit 1
  fi
elif [[ "${CORTEX_AI_SMOKE_CHECK_RUNTIME:-1}" == "warn" ]]; then
  if bash scripts/check-runtime-current.sh >/tmp/cortex-ai-runtime-current.out 2>&1; then
    pass "compose runtime current"
  else
    printf 'WARN  compose runtime current check failed:\n' >&2
    sed 's/^/      /' /tmp/cortex-ai-runtime-current.out >&2
  fi
fi

printf 'OK    AI transcript smoke passed\n'
