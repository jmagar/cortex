#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

status=0

while IFS= read -r claude_file; do
  dir="$(dirname "$claude_file")"
  for sibling in AGENTS.md GEMINI.md; do
    path="$dir/$sibling"
    if [ ! -L "$path" ]; then
      echo "[agent-memory] FAIL — $path must be a symlink to CLAUDE.md" >&2
      status=1
      continue
    fi
    target="$(readlink "$path")"
    if [ "$target" != "CLAUDE.md" ]; then
      echo "[agent-memory] FAIL — $path points to $target, expected CLAUDE.md" >&2
      status=1
    fi
  done
done < <(find . -path './target' -prune -o -name CLAUDE.md -print | sort)

if [ "$status" -eq 0 ]; then
  echo "[agent-memory] OK — CLAUDE.md siblings are source-of-truth symlinks"
fi

exit "$status"
