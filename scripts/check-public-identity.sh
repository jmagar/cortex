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

tracked_current_files=()
while IFS= read -r path; do
  case "$path" in
    scripts/check-public-identity.sh)
      continue
      ;;
    docs/plans/*|docs/runbooks/*|docs/sessions/*|docs/superpowers/*|CHANGELOG.md)
      # Archival/historical docs intentionally preserve old project names.
      continue
      ;;
    CLAUDE.md|README.md|server.json|mcpb/manifest.json|config/*|scripts/*|.github/*|.claude-plugin/*|plugins/*|docs/*)
      tracked_current_files+=("$path")
      ;;
  esac
done < <(git ls-files)

if [ "${#tracked_current_files[@]}" -eq 0 ]; then
  echo "[public-identity] FAIL — no tracked current files selected for scan" >&2
  exit 1
fi

for pattern in "${patterns[@]}"; do
  set +e
  rg -n --fixed-strings -- "$pattern" "${tracked_current_files[@]}"
  rg_status=$?
  set -e
  if [ "$rg_status" -eq 0 ]; then
    echo "[public-identity] FAIL — stale identity token found: $pattern" >&2
    status=1
  elif [ "$rg_status" -eq 2 ]; then
    echo "[public-identity] FAIL — rg failed while scanning for: $pattern" >&2
    status=1
  elif [ "$rg_status" -ne 1 ]; then
    echo "[public-identity] FAIL — unexpected rg exit $rg_status while scanning for: $pattern" >&2
    status=1
  fi
done

if [ "$status" -eq 0 ]; then
  echo "[public-identity] OK — public docs/config use cortex identity"
fi

exit "$status"
