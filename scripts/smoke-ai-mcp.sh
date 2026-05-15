#!/usr/bin/env bash
# Focused smoke test for AI transcript MCP actions against a running HTTP MCP server.
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SYSLOG_BIN="${SYSLOG_BIN:-syslog}"
PYTHON_BIN="${PYTHON_BIN:-python3}"
MCP_URL="${SYSLOG_MCP_URL:-http://localhost:3100/mcp}"
DB_PATH="${SYSLOG_SMOKE_DB_PATH:-${SYSLOG_MCP_DB_PATH:-${PROJECT_DIR}/data/syslog.db}}"
QUERY="${SYSLOG_AI_SMOKE_QUERY:-aismoke}"
PROJECT="/tmp/syslog-mcp-ai-smoke"

pass() {
  printf 'PASS  %s\n' "$1"
}

fail() {
  printf 'FAIL  %s\n' "$1" >&2
  exit 1
}

load_token() {
  if [[ -n "${SYSLOG_MCP_TOKEN:-}" ]]; then
    printf '%s' "$SYSLOG_MCP_TOKEN"
    return
  fi
  local env_file="${SYSLOG_MCP_ENV_FILE:-${SYSLOG_MCP_HOME:-${HOME}/.syslog-mcp}/.env}"
  if [[ -f "$env_file" ]]; then
    awk -F= '$1=="SYSLOG_MCP_TOKEN" {print $2; exit}' "$env_file"
  fi
}

run_syslog() {
  SYSLOG_MCP_DB_PATH="$DB_PATH" \
    SYSLOG_DOCKER_INGEST_ENABLED="${SYSLOG_DOCKER_INGEST_ENABLED:-false}" \
    RUST_LOG="${RUST_LOG:-error}" \
    "$SYSLOG_BIN" "$@"
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

[[ -x "$(command -v "$SYSLOG_BIN")" ]] || fail "$SYSLOG_BIN is not on PATH"
TOKEN="$(load_token || true)"
export TOKEN

SMOKE_DIR="$(mktemp -d)"
trap 'rm -rf "$SMOKE_DIR"' EXIT
FIXTURE="$SMOKE_DIR/ai-session-smoke.jsonl"
now="$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
from="$(date -u -d '10 minutes ago' +'%Y-%m-%dT%H:%M:%SZ')"
{
  printf '{"sessionId":"ai-smoke-session","timestamp":"%s","cwd":"%s","content":"aismoke authentication smoke transcript seed from Claude"}\n' "$now" "$PROJECT"
  printf '{"sessionId":"ai-smoke-session","timestamp":"%s","cwd":"%s","content":[{"type":"text","text":"aismoke project context MCP smoke content"}]}\n' "$now" "$PROJECT"
} >"$FIXTURE"

printf 'syslog: %s\n' "$("$SYSLOG_BIN" --version)"
printf 'db:     %s\n' "$DB_PATH"
printf 'mcp:    %s\n' "$MCP_URL"

run_syslog ai add --file "$FIXTURE" --force --json >/dev/null
pass "seeded AI transcript fixture"

search_response="$(mcp_call 1 "{\"action\":\"search_sessions\",\"query\":\"${QUERY}\",\"tool\":\"claude\",\"project\":\"${PROJECT}\",\"limit\":5}")"
assert_mcp "search_sessions did not return seeded session" "$search_response" "any(s.get('session_id') == 'ai-smoke-session' for s in data.get('sessions', []))"
pass "mcp search_sessions"

blocks_response="$(mcp_call 2 "{\"action\":\"usage_blocks\",\"tool\":\"claude\",\"project\":\"${PROJECT}\"}")"
assert_mcp "usage_blocks did not return blocks" "$blocks_response" "len(data.get('blocks', [])) >= 1"
pass "mcp usage_blocks"

context_response="$(mcp_call 3 "{\"action\":\"project_context\",\"tool\":\"claude\",\"project\":\"${PROJECT}\",\"limit\":5}")"
assert_mcp "project_context did not include fixture project" "$context_response" "data.get('project') == '/tmp/syslog-mcp-ai-smoke' and data.get('event_count', 0) >= 1"
pass "mcp project_context"

tools_response="$(mcp_call 4 '{"action":"list_ai_tools"}')"
assert_mcp "list_ai_tools did not include claude" "$tools_response" "any(t.get('tool') == 'claude' for t in data.get('tools', []))"
pass "mcp list_ai_tools"

projects_response="$(mcp_call 5 "{\"action\":\"list_ai_projects\",\"tool\":\"claude\",\"from\":\"${from}\"}")"
assert_mcp "list_ai_projects did not include fixture project" "$projects_response" "any(p.get('project') == '/tmp/syslog-mcp-ai-smoke' for p in data.get('projects', []))"
pass "mcp list_ai_projects"

printf 'OK    AI MCP smoke passed\n'
