#!/usr/bin/env bash
# Render a cortex MCP prompt and run it through a headless agent.
#
# This is intentionally live-first: the prompt comes from the running MCP server
# and the agent can use the caller's normal MCP configuration to query real
# ingested syslog/session data. The only deterministic fixture is the output
# schema check.

set -euo pipefail

MCP_URL="${CORTEX_URL:-http://localhost:3100/mcp}"
AGENT="${CORTEX_PROMPT_EVAL_AGENT:-codex}"
PROMPT_NAME="infra.incident-triage"
DRY_RUN=0
MCP_CONFIG="${CORTEX_PROMPT_EVAL_MCP_CONFIG:-}"
WORKDIR="${CORTEX_PROMPT_EVAL_WORKDIR:-$(pwd)}"
REPORT_PATH="${CORTEX_PROMPT_EVAL_REPORT:-}"
RUN_TIMEOUT_SECS="${CORTEX_PROMPT_EVAL_TIMEOUT_SECS:-300}"
PREFLIGHT_TIMEOUT_SECS="${CORTEX_PROMPT_EVAL_PREFLIGHT_TIMEOUT_SECS:-90}"
MAX_TOKENS="${CORTEX_PROMPT_EVAL_MAX_TOKENS:-25000}"
MAX_BUDGET_USD="${CORTEX_PROMPT_EVAL_MAX_BUDGET_USD:-}"
SKIP_AGENT_PREFLIGHT="${CORTEX_PROMPT_EVAL_SKIP_AGENT_PREFLIGHT:-0}"
declare -a PROMPT_ARGS=()

usage() {
    cat <<'USAGE'
Usage: scripts/prompt-headless-eval.sh [options]

Options:
  --agent codex|claude        Headless agent to run (default: codex)
  --prompt NAME               MCP prompt name (default: infra.incident-triage)
  --arg KEY=VALUE             Prompt argument, repeatable
  --mcp-config PATH           MCP config passed to Claude; Codex uses normal config unless overridden externally
  --url URL                   MCP URL (default: http://localhost:3100/mcp)
  --workdir DIR               Agent working directory (default: current directory)
  --report PATH               Write compact JSON run report
  --timeout SECS              Max seconds for full agent run (default: 300)
  --preflight-timeout SECS    Max seconds for agent MCP preflight (default: 90)
  --max-tokens N              Fail if parsed token usage exceeds N (default: 25000, 0 disables)
  --max-budget-usd AMOUNT     Passed to Claude headless mode
  --skip-agent-preflight      Skip the headless-agent MCP visibility preflight
  --dry-run                   Render prompt and schema but do not run an agent
  -h, --help                  Show this help

Auth:
  If CORTEX_TOKEN is set, it is sent as a Bearer token to cortex.

Examples:
  scripts/prompt-headless-eval.sh --dry-run --prompt infra.service-outage --arg service=plex
  CORTEX_TOKEN=... scripts/prompt-headless-eval.sh --agent codex --prompt infra.after-deploy-check --arg service=cortex
  CORTEX_TOKEN=... scripts/prompt-headless-eval.sh --report /tmp/eval.json --max-tokens 20000 --prompt infra.storage-pressure
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --agent)
            AGENT="${2:?--agent requires a value}"
            shift 2
            ;;
        --prompt)
            PROMPT_NAME="${2:?--prompt requires a value}"
            shift 2
            ;;
        --arg)
            PROMPT_ARGS+=("${2:?--arg requires KEY=VALUE}")
            shift 2
            ;;
        --mcp-config)
            MCP_CONFIG="${2:?--mcp-config requires a path}"
            shift 2
            ;;
        --url)
            MCP_URL="${2:?--url requires a value}"
            shift 2
            ;;
        --workdir)
            WORKDIR="${2:?--workdir requires a directory}"
            shift 2
            ;;
        --report)
            REPORT_PATH="${2:?--report requires a path}"
            shift 2
            ;;
        --timeout)
            RUN_TIMEOUT_SECS="${2:?--timeout requires seconds}"
            shift 2
            ;;
        --preflight-timeout)
            PREFLIGHT_TIMEOUT_SECS="${2:?--preflight-timeout requires seconds}"
            shift 2
            ;;
        --max-tokens)
            MAX_TOKENS="${2:?--max-tokens requires a number}"
            shift 2
            ;;
        --max-budget-usd)
            MAX_BUDGET_USD="${2:?--max-budget-usd requires an amount}"
            shift 2
            ;;
        --skip-agent-preflight)
            SKIP_AGENT_PREFLIGHT=1
            shift
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ "$AGENT" != "codex" && "$AGENT" != "claude" ]]; then
    echo "--agent must be codex or claude" >&2
    exit 2
fi
case "$RUN_TIMEOUT_SECS" in (*[!0-9]*|"") echo "--timeout must be an integer" >&2; exit 2 ;; esac
case "$PREFLIGHT_TIMEOUT_SECS" in (*[!0-9]*|"") echo "--preflight-timeout must be an integer" >&2; exit 2 ;; esac
case "$MAX_TOKENS" in (*[!0-9]*|"") echo "--max-tokens must be an integer" >&2; exit 2 ;; esac

if [[ "$AGENT" == "codex" && -n "$MCP_CONFIG" ]]; then
    echo "warning: --mcp-config is not passed to codex exec by this script; Codex uses its normal config" >&2
fi

TMPDIR="$(mktemp -d /tmp/cortex-prompt-eval-XXXXXX)"
trap 'rm -rf "$TMPDIR"' EXIT

AUTH_HEADER=()
if [[ -n "${CORTEX_TOKEN:-}" ]]; then
    AUTH_HEADER=(-H "Authorization: Bearer ${CORTEX_TOKEN}")
fi

ARGS_JSON="$(python3 - "${PROMPT_ARGS[@]}" <<'PY'
import json
import sys

args = {}
for item in sys.argv[1:]:
    if "=" not in item:
        raise SystemExit(f"prompt arg must be KEY=VALUE: {item}")
    key, value = item.split("=", 1)
    if not key:
        raise SystemExit(f"prompt arg key is empty: {item}")
    args[key] = value
print(json.dumps(args))
PY
)"

REQUEST="$(python3 - "$PROMPT_NAME" "$ARGS_JSON" <<'PY'
import json
import sys

print(json.dumps({
    "jsonrpc": "2.0",
    "id": 1,
    "method": "prompts/get",
    "params": {
        "name": sys.argv[1],
        "arguments": json.loads(sys.argv[2]),
    },
}))
PY
)"

curl_mcp() {
    curl -fsS -X POST "$MCP_URL" \
        -H 'Content-Type: application/json' \
        -H 'Accept: application/json, text/event-stream' \
        "${AUTH_HEADER[@]}" \
        -d "$1"
}

PROMPT_RESPONSE="$TMPDIR/prompt.json"
SCHEMA_RESPONSE="$TMPDIR/schema-response.json"
PROMPT_TEXT="$TMPDIR/prompt.txt"
SCHEMA_FILE="$TMPDIR/prompt-output.schema.json"
AGENT_OUTPUT="$TMPDIR/agent-output.json"
AGENT_LOG="$TMPDIR/agent.log"
AGENT_PREFLIGHT_OUTPUT="$TMPDIR/agent-preflight-output.json"
AGENT_PREFLIGHT_LOG="$TMPDIR/agent-preflight.log"
MCP_TOOLS_RESPONSE="$TMPDIR/tools.json"
MCP_HELP_RESPONSE="$TMPDIR/help.json"
VALIDATED_OUTPUT="$TMPDIR/validated-output.json"
SUMMARY_FILE="$TMPDIR/summary.json"
AGENT_PREFLIGHT_STATUS="skipped"
AGENT_PREFLIGHT_DETAIL="skipped"
AGENT_EXIT_CODE=0

curl_mcp "$REQUEST" > "$PROMPT_RESPONSE"
python3 - "$PROMPT_RESPONSE" > "$PROMPT_TEXT" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1]))
if "error" in data:
    raise SystemExit(data["error"])
print(data["result"]["messages"][0]["content"]["text"])
PY

curl_mcp '{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"cortex://schema/prompt-output"}}' > "$SCHEMA_RESPONSE"
python3 - "$SCHEMA_RESPONSE" > "$SCHEMA_FILE" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1]))
if "error" in data:
    raise SystemExit(data["error"])
print(data["result"]["contents"][0]["text"])
PY

curl_mcp '{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}}' > "$MCP_TOOLS_RESPONSE"
python3 - "$MCP_TOOLS_RESPONSE" "$SCHEMA_FILE" <<'PY'
import json
import sys

tools_response = json.load(open(sys.argv[1]))
if "error" in tools_response:
    raise SystemExit(f"FAIL: tools/list failed: {tools_response['error']}")
names = {tool.get("name") for tool in tools_response["result"].get("tools", [])}
if "cortex" not in names:
    raise SystemExit(f"FAIL: tools/list does not expose cortex tool: {sorted(names)}")

schema = json.load(open(sys.argv[2]))
required = set(schema.get("required", []))
expected = {"verdict", "confidence", "evidence", "likely_cause", "not_supported", "next_actions", "telemetry_gaps"}
missing = expected - required
if missing:
    raise SystemExit(f"FAIL: prompt-output schema missing required fields: {sorted(missing)}")
evidence_required = set(schema["properties"]["evidence"]["items"].get("required", []))
for key in ["source", "summary", "timestamp", "host", "app", "severity", "log_id"]:
    if key not in evidence_required:
        raise SystemExit(f"FAIL: evidence schema missing required key: {key}")
print("PASS: MCP preflight found cortex tool and prompt output schema")
PY

curl_mcp '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"cortex","arguments":{"action":"help"}}}' > "$MCP_HELP_RESPONSE"
python3 - "$MCP_HELP_RESPONSE" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1]))
if "error" in data:
    raise SystemExit(f"FAIL: cortex help call failed: {data['error']}")
text = data["result"]["content"][0].get("text", "")
try:
    text = json.loads(text).get("help", text)
except json.JSONDecodeError:
    pass
if "Agent Planning Cost Metadata" not in text or "## cortex help" not in text:
    raise SystemExit("FAIL: cortex help response lacks action/cost metadata")
print("PASS: cortex help exposes action and cost metadata")
PY

if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "Rendered prompt: $PROMPT_NAME"
    echo "---"
    cat "$PROMPT_TEXT"
    echo "---"
    echo "Schema: cortex://schema/prompt-output"
    if [[ -n "$REPORT_PATH" ]]; then
        python3 - "$REPORT_PATH" "$PROMPT_NAME" "$AGENT" "$MCP_URL" <<'PY'
import json
import sys
report = {
    "status": "dry_run",
    "prompt": sys.argv[2],
    "agent": sys.argv[3],
    "mcp_url": sys.argv[4],
    "mcp_preflight": "passed",
}
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    json.dump(report, fh, indent=2, sort_keys=True)
    fh.write("\n")
PY
    fi
    exit 0
fi

run_codex() {
    local prompt_file="$1" output_file="$2"
    codex exec \
        --cd "$WORKDIR" \
        --sandbox read-only \
        --output-schema "$SCHEMA_FILE" \
        --output-last-message "$output_file" \
        "$(cat "$prompt_file")"
}

run_claude() {
    local prompt_file="$1" output_file="$2"
    local -a args=(-p --permission-mode dontAsk --output-format json --json-schema "$(cat "$SCHEMA_FILE")")
    if [[ -n "$MCP_CONFIG" ]]; then
        args+=(--mcp-config "$MCP_CONFIG" --strict-mcp-config)
    fi
    if [[ -n "$MAX_BUDGET_USD" ]]; then
        args+=(--max-budget-usd "$MAX_BUDGET_USD")
    fi
    claude "${args[@]}" "$(cat "$prompt_file")" > "$output_file"
}

run_timed() {
    local timeout_secs="$1" log_file="$2"
    shift 2

    "$@" > "$log_file" 2>&1 &
    local pid=$!
    local deadline=$((SECONDS + timeout_secs))
    while kill -0 "$pid" 2>/dev/null; do
        if (( SECONDS >= deadline )); then
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
            echo "timeout after ${timeout_secs}s" >> "$log_file"
            return 124
        fi
        sleep 1
    done
    wait "$pid"
}

if [[ "$SKIP_AGENT_PREFLIGHT" -eq 0 ]]; then
    PREFLIGHT_SCHEMA="$TMPDIR/agent-preflight.schema.json"
    PREFLIGHT_PROMPT="$TMPDIR/agent-preflight.txt"
    cat > "$PREFLIGHT_SCHEMA" <<'JSON'
{
  "type": "object",
  "additionalProperties": false,
  "required": ["can_access_cortex", "evidence", "failure_reason"],
  "properties": {
    "can_access_cortex": { "type": "boolean" },
    "evidence": { "type": "string" },
    "failure_reason": { "type": ["string", "null"] }
  }
}
JSON
    cat > "$PREFLIGHT_PROMPT" <<'EOF'
Use only the configured cortex MCP server. Do not use shell commands, curl, local cortex binaries, repository files, or other fallbacks.

Call the cortex MCP tool with action=help. Return JSON:
- can_access_cortex: true only if the MCP tool call succeeded.
- evidence: a short phrase from the tool result proving it was cortex help.
- failure_reason: null on success, otherwise the concise reason.
EOF
    cp "$SCHEMA_FILE" "$TMPDIR/full-schema.keep"
    cp "$PREFLIGHT_SCHEMA" "$SCHEMA_FILE"
    set +e
    case "$AGENT" in
        codex)
            run_timed "$PREFLIGHT_TIMEOUT_SECS" "$AGENT_PREFLIGHT_LOG" run_codex "$PREFLIGHT_PROMPT" "$AGENT_PREFLIGHT_OUTPUT"
            ;;
        claude)
            run_timed "$PREFLIGHT_TIMEOUT_SECS" "$AGENT_PREFLIGHT_LOG" run_claude "$PREFLIGHT_PROMPT" "$AGENT_PREFLIGHT_OUTPUT"
            ;;
    esac
    preflight_rc=$?
    set -e
    mv "$TMPDIR/full-schema.keep" "$SCHEMA_FILE"
    if [[ "$preflight_rc" -ne 0 ]]; then
        AGENT_PREFLIGHT_STATUS="failed"
        AGENT_PREFLIGHT_DETAIL="agent preflight exited $preflight_rc"
    else
        if python3 - "$AGENT_PREFLIGHT_OUTPUT" <<'PY'
import json
import sys
raw = open(sys.argv[1]).read().strip()
data = json.loads(raw)
if "result" in data and isinstance(data["result"], str):
    data = json.loads(data["result"])
elif "content" in data and isinstance(data["content"], list):
    data = json.loads("".join(part.get("text", "") for part in data["content"] if isinstance(part, dict)))
if data.get("can_access_cortex") is not True:
    raise SystemExit(data.get("failure_reason") or "agent reported no cortex MCP access")
print(data.get("evidence", "ok"))
PY
        then
            AGENT_PREFLIGHT_STATUS="passed"
            AGENT_PREFLIGHT_DETAIL="cortex MCP visible to headless agent"
        else
            AGENT_PREFLIGHT_STATUS="failed"
            AGENT_PREFLIGHT_DETAIL="headless agent did not prove cortex MCP access"
        fi
    fi
    if [[ "$AGENT_PREFLIGHT_STATUS" != "passed" ]]; then
        if [[ -n "$REPORT_PATH" ]]; then
            python3 - "$REPORT_PATH" "$PROMPT_NAME" "$AGENT" "$MCP_URL" "$AGENT_PREFLIGHT_STATUS" "$AGENT_PREFLIGHT_DETAIL" <<'PY'
import json
import sys
report = {
    "status": "failed",
    "failure_stage": "agent_mcp_preflight",
    "prompt": sys.argv[2],
    "agent": sys.argv[3],
    "mcp_url": sys.argv[4],
    "mcp_preflight": "passed",
    "agent_mcp_preflight": {"status": sys.argv[5], "detail": sys.argv[6]},
}
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    json.dump(report, fh, indent=2, sort_keys=True)
    fh.write("\n")
PY
        fi
        echo "FAIL: headless agent cannot prove access to the configured cortex MCP server" >&2
        echo "Hint: pass --mcp-config for Claude, or configure Codex with the cortex MCP server. Use --skip-agent-preflight only when intentionally testing fallback behavior." >&2
        exit 1
    fi
fi

EVAL_PROMPT="$TMPDIR/eval-prompt.txt"
cat > "$EVAL_PROMPT" <<EOF
Use the cortex MCP server and follow this rendered prompt.

Use only the configured cortex MCP tools for syslog data. If the cortex MCP server is unavailable in the headless agent runtime, stop and return a low-confidence answer with that telemetry gap; do not use shell commands, curl, local cortex binaries, repository files, or other fallback surfaces as substitutes for syslog evidence.

Return only JSON that validates against the provided prompt output schema.

$(cat "$PROMPT_TEXT")
EOF

set +e
case "$AGENT" in
    codex)
        run_timed "$RUN_TIMEOUT_SECS" "$AGENT_LOG" run_codex "$EVAL_PROMPT" "$AGENT_OUTPUT"
        ;;
    claude)
        run_timed "$RUN_TIMEOUT_SECS" "$AGENT_LOG" run_claude "$EVAL_PROMPT" "$AGENT_OUTPUT"
        ;;
esac
AGENT_EXIT_CODE=$?
set -e
if [[ "$AGENT_EXIT_CODE" -ne 0 ]]; then
    tail -n 80 "$AGENT_LOG" >&2 || true
    echo "FAIL: agent run exited $AGENT_EXIT_CODE" >&2
    exit "$AGENT_EXIT_CODE"
fi

set +e
python3 - "$AGENT_OUTPUT" "$VALIDATED_OUTPUT" "$SUMMARY_FILE" "$AGENT_LOG" "$MAX_TOKENS" <<'PY'
import json
import re
import sys

path = sys.argv[1]
validated_path = sys.argv[2]
summary_path = sys.argv[3]
log_path = sys.argv[4]
max_tokens = int(sys.argv[5])
raw = open(path).read().strip()
try:
    data = json.loads(raw)
except json.JSONDecodeError as exc:
    raise SystemExit(f"FAIL: agent output is not JSON: {exc}")

if "result" in data and isinstance(data["result"], str):
    data = json.loads(data["result"])
elif "content" in data and isinstance(data["content"], list):
    text = "".join(part.get("text", "") for part in data["content"] if isinstance(part, dict))
    data = json.loads(text)

required = ["verdict", "confidence", "evidence", "likely_cause", "not_supported", "next_actions", "telemetry_gaps"]
missing = [key for key in required if key not in data]
if missing:
    raise SystemExit(f"FAIL: missing required fields: {', '.join(missing)}")
if data["confidence"] not in {"low", "medium", "high"}:
    raise SystemExit("FAIL: confidence must be low, medium, or high")
if not isinstance(data["evidence"], list) or not data["evidence"]:
    raise SystemExit("FAIL: evidence must be a non-empty array")

log = open(log_path, errors="replace").read()
token_count = None
matches = re.findall(r"tokens used\s+([\d,]+)", log, flags=re.IGNORECASE)
if matches:
    token_count = int(matches[-1].replace(",", ""))
over_token_budget = bool(max_tokens and token_count is not None and token_count > max_tokens)

with open(validated_path, "w", encoding="utf-8") as fh:
    json.dump(data, fh, indent=2, sort_keys=True)
    fh.write("\n")
summary = {
    "output_valid": True,
    "verdict": data.get("verdict"),
    "confidence": data.get("confidence"),
    "evidence_count": len(data.get("evidence", [])),
    "next_action_count": len(data.get("next_actions", [])) if isinstance(data.get("next_actions"), list) else None,
    "telemetry_gap_count": len(data.get("telemetry_gaps", [])) if isinstance(data.get("telemetry_gaps"), list) else None,
    "token_count": token_count,
    "max_tokens": max_tokens,
    "over_token_budget": over_token_budget,
}
with open(summary_path, "w", encoding="utf-8") as fh:
    json.dump(summary, fh, indent=2, sort_keys=True)
    fh.write("\n")
if over_token_budget:
    raise SystemExit(f"FAIL: token usage {token_count} exceeded max {max_tokens}")
print("PASS: final output has required prompt investigation shape")
PY
VALIDATION_EXIT_CODE=$?
set -e

if [[ -n "$REPORT_PATH" ]]; then
    REPORT_ARTIFACT_DIR="${REPORT_PATH}.artifacts"
    mkdir -p "$REPORT_ARTIFACT_DIR"
    cp "$AGENT_LOG" "$REPORT_ARTIFACT_DIR/agent.log"
    [[ -f "$AGENT_OUTPUT" ]] && cp "$AGENT_OUTPUT" "$REPORT_ARTIFACT_DIR/agent-output.raw.json"
    [[ -f "$VALIDATED_OUTPUT" ]] && cp "$VALIDATED_OUTPUT" "$REPORT_ARTIFACT_DIR/agent-output.validated.json"
    python3 - "$REPORT_PATH" "$PROMPT_NAME" "$AGENT" "$MCP_URL" "$AGENT_PREFLIGHT_STATUS" "$AGENT_PREFLIGHT_DETAIL" "$SUMMARY_FILE" "$VALIDATED_OUTPUT" "$REPORT_ARTIFACT_DIR" "$RUN_TIMEOUT_SECS" "$VALIDATION_EXIT_CODE" <<'PY'
import json
import sys

report_path, prompt, agent, mcp_url, preflight_status, preflight_detail, summary_path, output_path, artifact_dir, timeout_secs, validation_exit_code = sys.argv[1:12]
validation_exit_code = int(validation_exit_code)
summary = json.load(open(summary_path)) if validation_exit_code == 0 or __import__("os").path.exists(summary_path) else {}
output = json.load(open(output_path)) if __import__("os").path.exists(output_path) else None
report = {
    "status": "passed" if validation_exit_code == 0 else "failed",
    "failure_stage": None if validation_exit_code == 0 else "output_validation_or_budget",
    "prompt": prompt,
    "agent": agent,
    "mcp_url": mcp_url,
    "mcp_preflight": "passed",
    "agent_mcp_preflight": {"status": preflight_status, "detail": preflight_detail},
    "timeout_secs": int(timeout_secs),
    "token_count": summary.get("token_count"),
    "max_tokens": summary.get("max_tokens"),
    "verdict": summary.get("verdict"),
    "confidence": summary.get("confidence"),
    "evidence_count": summary.get("evidence_count"),
    "next_action_count": summary.get("next_action_count"),
    "telemetry_gap_count": summary.get("telemetry_gap_count"),
    "over_token_budget": summary.get("over_token_budget"),
    "output": output,
    "artifacts": {
        "dir": artifact_dir,
        "agent_log": f"{artifact_dir}/agent.log",
        "agent_output_raw": f"{artifact_dir}/agent-output.raw.json",
        "agent_output_validated": f"{artifact_dir}/agent-output.validated.json",
    },
}
with open(report_path, "w", encoding="utf-8") as fh:
    json.dump(report, fh, indent=2, sort_keys=True)
    fh.write("\n")
print(f"Report: {report_path}")
PY
fi

if [[ "$VALIDATION_EXIT_CODE" -ne 0 ]]; then
    exit "$VALIDATION_EXIT_CODE"
fi
