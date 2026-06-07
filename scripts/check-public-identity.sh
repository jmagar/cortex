#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

status=0
patterns=(
  'syslog:read'
  'syslog:admin'
  '"name":"syslog"'
  '"name": "syslog"'
  '/path/to/syslog'
  'plugins/syslog'
  'syslog.cortex'
  'mcporter list syslog'
  'x-syslog-action-metadata'
  'x-syslog-agent-guidance'
)

paths=(
  CLAUDE.md
  README.md
  config/mcporter.json
  docs/README.md
  docs/CONFIG.md
  docs/CLI.md
  docs/INVENTORY.md
  docs/OAUTH.md
  docs/RELEASE.md
  docs/SECURITY.md
  docs/RUST.md
  docs/contracts/mcp-actions-current.md
  docs/mcp
  docs/plugin
)

for pattern in "${patterns[@]}"; do
  if rg -n --fixed-strings --glob '!docs/mcp/LOGS.md' -- "$pattern" "${paths[@]}"; then
    echo "[public-identity] FAIL — stale identity token found: $pattern" >&2
    status=1
  fi
done

if [ "$status" -eq 0 ]; then
  echo "[public-identity] OK — public docs/config use cortex identity"
fi

exit "$status"
