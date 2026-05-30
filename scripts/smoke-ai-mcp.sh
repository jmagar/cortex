#!/usr/bin/env bash
# Focused smoke test for AI transcript MCP actions against a running HTTP MCP server.
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORTEX_BIN="${CORTEX_BIN:-}"
PYTHON_BIN="${PYTHON_BIN:-python3}"
MCP_URL="${CORTEX_URL:-http://localhost:3100/mcp}"
DB_PATH="${CORTEX_SMOKE_DB_PATH:-${CORTEX_DB_PATH:-${PROJECT_DIR}/data/cortex.db}}"
RUN_ID="${CORTEX_AI_SMOKE_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
if [[ ! "$RUN_ID" =~ ^[A-Za-z0-9_.:-]+$ ]]; then
  echo "FAIL  CORTEX_AI_SMOKE_RUN_ID contains unsafe characters" >&2
  exit 1
fi
QUERY="${CORTEX_AI_SMOKE_QUERY:-aismoke${RUN_ID//[^A-Za-z0-9]/}}"
SESSION_ID="ai-smoke-session-${RUN_ID}"
PROJECT="/tmp/cortex-ai-smoke-${RUN_ID}"

pass() {
  printf 'PASS  %s\n' "$1"
}

fail() {
  printf 'FAIL  %s\n' "$1" >&2
  exit 1
}

resolve_syslog_bin() {
  if [[ -n "$CORTEX_BIN" ]]; then
    if [[ -x "$CORTEX_BIN" ]]; then
      printf '%s\n' "$CORTEX_BIN"
    elif command -v "$CORTEX_BIN" >/dev/null 2>&1; then
      command -v "$CORTEX_BIN"
    else
      fail "CORTEX_BIN is not executable or on PATH: $CORTEX_BIN"
    fi
  elif command -v syslog >/dev/null 2>&1; then
    command -v syslog
  elif [[ -x "${PROJECT_DIR}/target/debug/syslog" ]]; then
    printf '%s\n' "${PROJECT_DIR}/target/debug/syslog"
  else
    fail "syslog binary not found; install syslog on PATH, set CORTEX_BIN, or run cargo build"
  fi
}

load_token() {
  if [[ -n "${CORTEX_TOKEN:-}" ]]; then
    printf '%s' "$CORTEX_TOKEN"
    return
  fi
  local env_file="${CORTEX_ENV_FILE:-${CORTEX_HOME:-${HOME}/.cortex}/.env}"
  if [[ -f "$env_file" ]]; then
    awk '$0 ~ /^CORTEX_TOKEN=/ {sub(/^CORTEX_TOKEN=/, ""); print; exit}' "$env_file"
  fi
}

run_syslog() {
  CORTEX_DB_PATH="$DB_PATH" \
    CORTEX_DOCKER_INGEST_ENABLED="${CORTEX_DOCKER_INGEST_ENABLED:-false}" \
    RUST_LOG="${RUST_LOG:-error}" \
    "$CORTEX_BIN" "$@"
}

mcp_call() {
  local id="$1"
  local args_json="$2"
  local token="${TOKEN:-}"
  local -a headers=(-H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream')
  if [[ -n "$token" ]]; then
    headers+=(-H "Authorization: Bearer $token")
  fi
  curl -fsS -X POST "$MCP_URL" "${headers[@]}" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":${id},\"method\":\"tools/call\",\"params\":{\"name\":\"syslog\",\"arguments\":${args_json}}}"
}

tool_args() {
  jq -nc "$@"
}

assert_mcp() {
  local label="$1"
  local response="$2"
  local expr="$3"
  JSON_INPUT="$response" "$PYTHON_BIN" - "$expr" <<'PY' >/dev/null || fail "$label"
import json
import os
import sys

response = json.loads(os.environ["JSON_INPUT"])
if "error" in response:
    raise SystemExit(1)
content = response.get("result", {}).get("content", [])
payload = {}
for item in content:
    text = item.get("text")
    if text:
        payload = json.loads(text)
        break
scope = {"__builtins__": {}, "data": payload, "any": any, "len": len}
if not eval(sys.argv[1], scope, {}):
    raise SystemExit(1)
PY
}

cd "$PROJECT_DIR"

CORTEX_BIN="$(resolve_syslog_bin)"
[[ -x "$CORTEX_BIN" ]] || fail "$CORTEX_BIN is not executable"
TOKEN="$(load_token || true)"
export TOKEN

SMOKE_DIR="$(mktemp -d)"
trap 'rm -rf "$SMOKE_DIR"' EXIT
FIXTURE="$SMOKE_DIR/ai-session-smoke.jsonl"
now="$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
from="$(date -u -d '10 minutes ago' +'%Y-%m-%dT%H:%M:%SZ')"
{
  printf '{"sessionId":"%s","timestamp":"%s","cwd":"%s","content":"%s authentication smoke transcript seed from Claude"}\n' "$SESSION_ID" "$now" "$PROJECT" "$QUERY"
  printf '{"sessionId":"%s","timestamp":"%s","cwd":"%s","content":[{"type":"text","text":"%s project context MCP smoke content"}]}\n' "$SESSION_ID" "$now" "$PROJECT" "$QUERY"
} >"$FIXTURE"

printf 'syslog: %s\n' "$("$CORTEX_BIN" --version)"
printf 'db:     %s\n' "$DB_PATH"
printf 'mcp:    %s\n' "$MCP_URL"

run_syslog ai add --file "$FIXTURE" --force --json >/dev/null
pass "seeded AI transcript fixture"

search_response="$(mcp_call 1 "$(tool_args --arg q "$QUERY" --arg project "$PROJECT" '{"action":"search_sessions","query":$q,"tool":"claude","project":$project,"limit":5}')")"
assert_mcp "search_sessions did not return seeded session" "$search_response" "any(s.get('session_id') == '${SESSION_ID}' for s in data.get('sessions', []))"
pass "mcp search_sessions"

blocks_response="$(mcp_call 2 "$(tool_args --arg project "$PROJECT" '{"action":"usage_blocks","tool":"claude","project":$project}')")"
assert_mcp "usage_blocks did not return blocks" "$blocks_response" "len(data.get('blocks', [])) >= 1"
pass "mcp usage_blocks"

context_response="$(mcp_call 3 "$(tool_args --arg project "$PROJECT" '{"action":"project_context","tool":"claude","project":$project,"limit":5}')")"
assert_mcp "project_context did not include fixture project" "$context_response" "data.get('project') == '${PROJECT}' and data.get('event_count', 0) >= 1"
pass "mcp project_context"

tools_response="$(mcp_call 4 '{"action":"list_ai_tools"}')"
assert_mcp "list_ai_tools did not include claude" "$tools_response" "any(t.get('tool') == 'claude' for t in data.get('tools', []))"
pass "mcp list_ai_tools"

projects_response="$(mcp_call 5 "$(tool_args --arg from "$from" '{"action":"list_ai_projects","tool":"claude","from":$from}')")"
assert_mcp "list_ai_projects did not include fixture project" "$projects_response" "any(p.get('project') == '${PROJECT}' for p in data.get('projects', []))"
pass "mcp list_ai_projects"

printf 'OK    AI MCP smoke passed\n'
