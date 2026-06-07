#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

status=0
manifest_paths=()

if [ -f ".claude-plugin/plugin.json" ]; then
  manifest_paths+=(".claude-plugin/plugin.json")
fi
if [ -f ".codex-plugin/plugin.json" ]; then
  manifest_paths+=(".codex-plugin/plugin.json")
fi
if [ -f "gemini-extension.json" ]; then
  manifest_paths+=("gemini-extension.json")
fi

while IFS= read -r path; do
  manifest_paths+=("$path")
done < <(find plugins -path '*/plugin.json' -type f 2>/dev/null | sort || true)

for path in "${manifest_paths[@]}"; do
  if python3 - "$path" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as fh:
    payload = json.load(fh)
sys.exit(0 if "version" in payload else 1)
PY
  then
    echo "[plugin-manifest] FAIL — $path must not contain a top-level version key" >&2
    status=1
  fi
done

if [ "$status" -eq 0 ]; then
  echo "[plugin-manifest] OK — plugin manifests are unversioned"
fi

exit "$status"
