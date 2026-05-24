#!/usr/bin/env bash
# Render a syslog MCP prompt and run it through a headless agent.
#
# This is intentionally live-first: the prompt comes from the running MCP server
# and the agent can use the caller's normal MCP configuration to query real
# ingested syslog/session data. The only deterministic fixture is the output
# schema check.

set -euo pipefail

MCP_URL="${SYSLOG_MCP_URL:-http://localhost:3100/mcp}"
AGENT="${SYSLOG_PROMPT_EVAL_AGENT:-codex}"
PROMPT_NAME="infra.incident-triage"
DRY_RUN=0
MCP_CONFIG="${SYSLOG_PROMPT_EVAL_MCP_CONFIG:-}"
WORKDIR="${SYSLOG_PROMPT_EVAL_WORKDIR:-$(pwd)}"
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
  --dry-run                   Render prompt and schema but do not run an agent
  -h, --help                  Show this help

Auth:
  If SYSLOG_MCP_TOKEN is set, it is sent as a Bearer token to syslog-mcp.

Examples:
  scripts/prompt-headless-eval.sh --dry-run --prompt infra.service-outage --arg service=plex
  SYSLOG_MCP_TOKEN=... scripts/prompt-headless-eval.sh --agent codex --prompt infra.after-deploy-check --arg service=syslog-mcp
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

TMPDIR="$(mktemp -d /tmp/syslog-prompt-eval-XXXXXX)"
trap 'rm -rf "$TMPDIR"' EXIT

AUTH_HEADER=()
if [[ -n "${SYSLOG_MCP_TOKEN:-}" ]]; then
    AUTH_HEADER=(-H "Authorization: Bearer ${SYSLOG_MCP_TOKEN}")
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

curl_mcp "$REQUEST" > "$PROMPT_RESPONSE"
python3 - "$PROMPT_RESPONSE" > "$PROMPT_TEXT" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1]))
if "error" in data:
    raise SystemExit(data["error"])
print(data["result"]["messages"][0]["content"]["text"])
PY

curl_mcp '{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"syslog://schema/prompt-output"}}' > "$SCHEMA_RESPONSE"
python3 - "$SCHEMA_RESPONSE" > "$SCHEMA_FILE" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1]))
if "error" in data:
    raise SystemExit(data["error"])
print(data["result"]["contents"][0]["text"])
PY

if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "Rendered prompt: $PROMPT_NAME"
    echo "---"
    cat "$PROMPT_TEXT"
    echo "---"
    echo "Schema: syslog://schema/prompt-output"
    exit 0
fi

EVAL_PROMPT="$TMPDIR/eval-prompt.txt"
cat > "$EVAL_PROMPT" <<EOF
Use the syslog MCP server and follow this rendered prompt.

Return only JSON that validates against the provided prompt output schema.

$(cat "$PROMPT_TEXT")
EOF

case "$AGENT" in
    codex)
        codex exec \
            --cd "$WORKDIR" \
            --sandbox read-only \
            --ask-for-approval never \
            --output-schema "$SCHEMA_FILE" \
            --output-last-message "$AGENT_OUTPUT" \
            "$(cat "$EVAL_PROMPT")"
        ;;
    claude)
        CLAUDE_ARGS=(-p --permission-mode dontAsk --output-format json --json-schema "$(cat "$SCHEMA_FILE")")
        if [[ -n "$MCP_CONFIG" ]]; then
            CLAUDE_ARGS+=(--mcp-config "$MCP_CONFIG")
        fi
        claude "${CLAUDE_ARGS[@]}" "$(cat "$EVAL_PROMPT")" > "$AGENT_OUTPUT"
        ;;
esac

python3 - "$AGENT_OUTPUT" <<'PY'
import json
import sys

path = sys.argv[1]
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

print("PASS: final output has required prompt investigation shape")
PY
