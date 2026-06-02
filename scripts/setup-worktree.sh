#!/usr/bin/env bash
# setup-worktree.sh — Make a fresh git worktree a working cortex environment.
#
# A worktree only receives git-TRACKED files. This script copies in the
# gitignored bits cortex needs to actually run, and fixes the one tracked
# file that breaks in a worktree (the relative .beads symlink).
#
# What it does (idempotent, never clobbers an existing .env):
#   1. .env          — copy from the main checkout if absent, then ISOLATE the
#                      worktree's runtime: distinct CORTEX_DATA_VOLUME + bumped
#                      CORTEX_PORT / CORTEX_RECEIVER_HOST_PORT, so `just up` here
#                      doesn't collide with the shared install (which the repo
#                      .env points at, ~/.cortex/data, ports 3100/1514).
#   2. .beads        — repoint the symlink to an ABSOLUTE target (the tracked
#                      value is `../../.beads`, which dangles from a worktree)
#   3. target/       — with --share-target, write .cargo/config.toml so cargo
#                      reuses the main checkout's build cache (~281 MB cold build)
#
# Usage:
#   bash scripts/setup-worktree.sh                 # run from inside the worktree
#   bash scripts/setup-worktree.sh --share-target  # also share the Cargo target dir
#   bash scripts/setup-worktree.sh --settings      # also copy .claude/settings.local.json
#   bash scripts/setup-worktree.sh --no-isolate    # keep .env pointed at the shared install
#
# Safe to re-run. Refuses to run in the main checkout.

set -euo pipefail

share_target=0
copy_settings=0
no_isolate=0
for arg in "$@"; do
  case "$arg" in
    --share-target) share_target=1 ;;
    --settings)     copy_settings=1 ;;
    --no-isolate)   no_isolate=1 ;;
    -h|--help)      sed -n '2,26p' "$0"; exit 0 ;;
    *) echo "unknown flag: $arg (try --help)" >&2; exit 2 ;;
  esac
done

# Set KEY=value in an env file: replace the existing line or append.
set_env_key() {
  local file="$1" key="$2" val="$3" tmp
  tmp="$(mktemp)"
  if grep -qE "^${key}=" "$file" 2>/dev/null; then
    awk -v k="$key" -v v="$val" 'index($0, k"=")==1 {print k"="v; next} {print}' "$file" >"$tmp"
  else
    cat "$file" >"$tmp"
    printf '%s=%s\n' "$key" "$val" >>"$tmp"
  fi
  mv "$tmp" "$file"
}

# Resolve the worktree we're in and the primary working tree.
wt_root="$(git rev-parse --show-toplevel)"
common_dir="$(git rev-parse --path-format=absolute --git-common-dir)"
main_root="$(dirname "$common_dir")"

if [ "$wt_root" = "$main_root" ]; then
  echo "✗ This is the main checkout ($main_root), not a worktree. Nothing to do." >&2
  exit 1
fi

cd "$wt_root"
echo "▸ Provisioning worktree: $wt_root"
echo "  (primary checkout:     $main_root)"

# 1. .env — copy if absent, never overwrite.
if [ -f .env ]; then
  echo "  • .env       already present — left untouched"
elif [ -f "$main_root/.env" ]; then
  cp "$main_root/.env" .env
  echo "  • .env       copied from primary checkout"
else
  echo "  ! .env       MISSING in primary checkout too — run 'just setup' to create one" >&2
fi

# 1b. Isolate the worktree's runtime so `just up` here can't collide with the
#     shared install the repo .env targets. Deterministic per worktree name.
if [ "$no_isolate" = "0" ] && [ -f .env ]; then
  wt_name="$(basename "$wt_root")"
  off=$(( $(printf '%s' "$wt_name" | cksum | cut -d' ' -f1) % 100 ))
  set_env_key .env CORTEX_DATA_VOLUME "cortex-wt-${wt_name}"
  set_env_key .env CORTEX_PORT "$((3200 + off))"
  set_env_key .env CORTEX_RECEIVER_HOST_PORT "$((11514 + off))"
  echo "  • runtime    isolated → volume cortex-wt-${wt_name}, MCP $((3200 + off)), syslog $((11514 + off))"
else
  [ "$no_isolate" = "1" ] && echo "  • runtime    --no-isolate: .env left pointing at the shared install"
fi

# 2. .beads — repoint to an absolute target so it survives the depth change.
beads_target="$(readlink -f "$main_root/.beads" 2>/dev/null || true)"
if [ -n "$beads_target" ] && [ -d "$beads_target" ]; then
  ln -sfn "$beads_target" .beads
  echo "  • .beads     → $beads_target"
else
  echo "  ! .beads     could not resolve target from $main_root/.beads — skipped" >&2
fi

# 3. target/ — share the build cache via .cargo/config.toml (cargo reads it
#    automatically; no shell env var needed). Opt-in: concurrent builds across
#    worktrees serialize on the shared target lock.
if [ "$share_target" = "1" ]; then
  mkdir -p .cargo
  cat > .cargo/config.toml <<EOF
# Written by scripts/setup-worktree.sh — share the primary checkout's build cache.
[build]
target-dir = "$main_root/target"
EOF
  echo "  • target/    shared via .cargo/config.toml → $main_root/target"
fi

# Optional: carry over local Claude Code permission allowlist.
if [ "$copy_settings" = "1" ]; then
  if [ -f "$main_root/.claude/settings.local.json" ]; then
    mkdir -p .claude
    cp "$main_root/.claude/settings.local.json" .claude/settings.local.json
    echo "  • .claude/settings.local.json copied"
  else
    echo "  ! .claude/settings.local.json not found in primary checkout — skipped"
  fi
fi

echo "✓ Worktree ready. Try: just build && just test"
