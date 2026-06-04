#!/usr/bin/env bash
# smoke-test.sh — verify end-to-end log flow through cortex.
#
# Sends a unique test syslog message to the cortex listener, then polls the
# cortex HTTP search API until the message appears (or times out).
#
# Exit codes:
#   0  PASS — message found within TIMEOUT seconds
#   1  FAIL — message not found, or send failed
#
# Usage:
#   smoke-test.sh [--timeout <seconds>] [--host <host>] [--port <port>] [--url <http-url>]
set -euo pipefail

: "${CLAUDE_PLUGIN_ROOT:=$(cd "$(dirname "$0")/.." && pwd)}"

# Source plugin env if available
[ -f "${CLAUDE_PLUGIN_ROOT}/scripts/plugin-setup.sh" ] && source "${CLAUDE_PLUGIN_ROOT}/scripts/plugin-setup.sh" 2>/dev/null || true

CORTEX_HOST="${CORTEX_HOST:-127.0.0.1}"
CORTEX_SYSLOG_PORT="${CORTEX_SYSLOG_PORT:-1514}"
CORTEX_HTTP_URL="${CORTEX_HTTP_URL:-http://127.0.0.1:3100}"
TIMEOUT=10

# Parse optional CLI overrides
while [[ $# -gt 0 ]]; do
  case "$1" in
    --timeout) TIMEOUT="$2"; shift 2 ;;
    --host)    CORTEX_HOST="$2"; shift 2 ;;
    --port)    CORTEX_SYSLOG_PORT="$2"; shift 2 ;;
    --url)     CORTEX_HTTP_URL="$2"; shift 2 ;;
    *) shift ;;
  esac
done

TEST_TAG="cortex-smoke-$(date +%s)"

echo "Sending test log message (tag: $TEST_TAG)..."
echo "  target: ${CORTEX_HOST}:${CORTEX_SYSLOG_PORT} (UDP syslog)"

# Try logger first, fall back to nc, fail clearly if neither works.
if command -v logger >/dev/null 2>&1; then
  logger -n "$CORTEX_HOST" -P "$CORTEX_SYSLOG_PORT" -d "$TEST_TAG cortex smoke test" 2>/dev/null \
    && echo "  sent via logger"
elif command -v nc >/dev/null 2>&1; then
  # Manually craft a minimal RFC-3164 syslog message (PRI + content)
  # PRI 14 = facility:user (1) + severity:info (6)  →  <14>
  printf '<14>%s cortex smoke-test[%d]: %s\n' \
    "$(date '+%b %e %H:%M:%S')" "$$" "$TEST_TAG" \
    | nc -u -w1 "$CORTEX_HOST" "$CORTEX_SYSLOG_PORT" 2>/dev/null \
    && echo "  sent via nc"
else
  echo "FAIL: could not send syslog — neither logger nor nc is available"
  exit 1
fi

echo "Waiting for message to appear in cortex search (up to ${TIMEOUT}s)..."
echo "  polling: ${CORTEX_HTTP_URL}/v1/logs/search"

START_TS=$(date +%s)

for i in $(seq 1 "$TIMEOUT"); do
  sleep 1

  # URL-encode the tag with python3 (available in the plugin env)
  encoded_tag=$(python3 -c \
    "import urllib.parse, sys; print(urllib.parse.quote(sys.argv[1]))" \
    "$TEST_TAG" 2>/dev/null || printf '%s' "$TEST_TAG")

  result=$(curl -sf \
    "${CORTEX_HTTP_URL%/}/v1/logs/search?q=${encoded_tag}&limit=1" \
    2>/dev/null || true)

  if echo "$result" | grep -q "$TEST_TAG" 2>/dev/null; then
    END_TS=$(date +%s)
    ELAPSED=$(( END_TS - START_TS ))
    echo "PASS: smoke test message found after ${ELAPSED}s"
    exit 0
  fi
done

echo "FAIL: smoke test message not found in cortex after ${TIMEOUT}s"
echo "  Check: is cortex running? Is syslog port ${CORTEX_SYSLOG_PORT} reachable on ${CORTEX_HOST}?"
echo "  Check: is the HTTP API reachable at ${CORTEX_HTTP_URL%/}/v1/logs/search?"
exit 1
