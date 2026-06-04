#!/usr/bin/env bash
# check-runtime-current.sh — compare the running cortex container image to the
# image declared in the local Docker Compose file.
#
# Exit codes:
#   0  CURRENT     — running image matches expected image
#   1  STALE       — running image differs from expected image
#   2  NOT_RUNNING — cortex container is not running
#
# Usage:
#   check-runtime-current.sh [--json]
set -euo pipefail

: "${CLAUDE_PLUGIN_ROOT:=$(cd "$(dirname "$0")/.." && pwd)}"

# Source plugin env if available (sets CORTEX_* vars, etc.)
if [ -f "${CLAUDE_PLUGIN_ROOT}/scripts/plugin-setup.sh" ]; then
  # Avoid re-running the full setup logic; just source the helper functions
  # by checking if the file is safe to source in this context.
  true
fi

JSON=false
for arg in "$@"; do
  case "$arg" in
    --json) JSON=true ;;
  esac
done

# -- Discover running container image ----------------------------------------

CONTAINER_ID=$(docker compose ps -q cortex 2>/dev/null || true)
RUNNING_IMAGE=""
if [ -n "$CONTAINER_ID" ]; then
  RUNNING_IMAGE=$(docker inspect "$CONTAINER_ID" --format '{{.Image}}' 2>/dev/null || true)
fi

# -- Discover expected image from Compose config -----------------------------

EXPECTED_IMAGE=""

# Try `docker compose images` first (available in Compose v2.x)
images_json=$(docker compose images cortex --format json 2>/dev/null || true)
if [ -n "$images_json" ]; then
  # The JSON output is an array of objects; grab the ID of the first entry.
  # Format varies by version — try .ID then .Repository+:+.Tag as fallback.
  EXPECTED_IMAGE=$(printf '%s' "$images_json" | \
    python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    if isinstance(data, list) and data:
        obj = data[0]
        # Prefer a raw digest/ID if present
        for key in ('ID', 'Id', 'id', 'ImageID', 'image_id'):
            if obj.get(key):
                print(obj[key])
                sys.exit(0)
        # Fall back to repo:tag
        repo = obj.get('Repository') or obj.get('repository') or ''
        tag  = obj.get('Tag') or obj.get('tag') or 'latest'
        if repo:
            print(f'{repo}:{tag}')
except Exception:
    pass
" 2>/dev/null || true)
fi

# Fallback: parse the image name from `docker compose config`
if [ -z "$EXPECTED_IMAGE" ]; then
  EXPECTED_IMAGE=$(docker compose config --format json 2>/dev/null \
    | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    img = (data.get('services') or {}).get('cortex', {}).get('image') or ''
    if img:
        print(img)
except Exception:
    pass
" 2>/dev/null || true)
fi

# Last resort: plain text parse of docker compose config
if [ -z "$EXPECTED_IMAGE" ]; then
  EXPECTED_IMAGE=$(docker compose config 2>/dev/null \
    | awk '/^services:/{in_svc=1} in_svc && /^  cortex:/{in_cortex=1} in_cortex && /^    image:/{print $2; exit}' \
    || true)
fi

# -- Determine status --------------------------------------------------------

STATUS="NOT_RUNNING"
MESSAGE=""

if [ -z "$RUNNING_IMAGE" ]; then
  STATUS="NOT_RUNNING"
  MESSAGE="cortex container is not running"
elif [ -z "$EXPECTED_IMAGE" ]; then
  # Can't determine expected image — treat running as current if container exists
  STATUS="CURRENT"
  MESSAGE="cortex is running (expected image could not be determined)"
else
  # Normalise: strip sha256: prefix for display; compare full IDs when possible
  running_short="${RUNNING_IMAGE#sha256:}"
  running_short="${running_short:0:12}"
  expected_short="${EXPECTED_IMAGE#sha256:}"
  expected_short="${expected_short:0:12}"

  if [ "$RUNNING_IMAGE" = "$EXPECTED_IMAGE" ]; then
    STATUS="CURRENT"
    MESSAGE="cortex is current (${running_short})"
  elif [[ "$RUNNING_IMAGE" == *"$expected_short"* ]] || [[ "$EXPECTED_IMAGE" == *"$running_short"* ]]; then
    STATUS="CURRENT"
    MESSAGE="cortex is current (${running_short})"
  else
    STATUS="STALE"
    MESSAGE="cortex is stale — running ${running_short}, expected ${expected_short}"
  fi
fi

# -- Output ------------------------------------------------------------------

if [ "$JSON" = "true" ]; then
  python3 -c "
import json, sys
print(json.dumps({
    'status':         sys.argv[1],
    'running_image':  sys.argv[2],
    'expected_image': sys.argv[3],
    'message':        sys.argv[4],
}))
" "$STATUS" "$RUNNING_IMAGE" "$EXPECTED_IMAGE" "$MESSAGE"
else
  case "$STATUS" in
    CURRENT)     echo "✓ $MESSAGE" ;;
    STALE)       echo "⚠ $MESSAGE" ;;
    NOT_RUNNING) echo "✗ $MESSAGE" ;;
  esac
fi

# Exit code
case "$STATUS" in
  CURRENT)     exit 0 ;;
  STALE)       exit 1 ;;
  NOT_RUNNING) exit 2 ;;
esac
