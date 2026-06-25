#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 || "$2" != "--" ]]; then
  echo "Usage: scripts/with_timeout.sh SECONDS -- COMMAND [ARG...]" >&2
  exit 2
fi

seconds="$1"
shift 2

if command -v timeout >/dev/null 2>&1; then
  exec timeout "${seconds}" "$@"
fi

echo "timeout command not found; running without timeout: $*" >&2
exec "$@"
