#!/usr/bin/env bash
# check-version-sync.sh — verify release version-bearing files match.
# Exits non-zero if versions are out of sync. With --require-changelog, also
# exits non-zero if CHANGELOG.md is missing an entry for the canonical version.
set -euo pipefail

REQUIRE_CHANGELOG=0
PROJECT_DIR="."

for arg in "$@"; do
  case "$arg" in
    --require-changelog)
      REQUIRE_CHANGELOG=1
      ;;
    --help|-h)
      echo "Usage: $0 [--require-changelog] [PROJECT_DIR]"
      exit 0
      ;;
    -*)
      echo "[version-sync] Unknown option: $arg" >&2
      echo "Usage: $0 [--require-changelog] [PROJECT_DIR]" >&2
      exit 2
      ;;
    *)
      PROJECT_DIR="$arg"
      ;;
  esac
done

cd "$PROJECT_DIR"

versions=()
files_checked=()

changelog_has_release_heading() {
  local version="$1"
  local line

  while IFS= read -r line; do
    case "$line" in
      "## [$version]" | "## [$version] "*) return 0 ;;
    esac
  done < CHANGELOG.md

  return 1
}

# Extract version from each file type
if [ -f "Cargo.toml" ]; then
  v=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
  [ -n "$v" ] && versions+=("Cargo.toml=$v") && files_checked+=("Cargo.toml")
fi

if [ -f "package.json" ]; then
  v=$(python3 -c "import json; print(json.load(open('package.json')).get('version',''))" 2>/dev/null)
  [ -n "$v" ] && versions+=("package.json=$v") && files_checked+=("package.json")
fi

if [ -f "pyproject.toml" ]; then
  v=$(grep -m1 '^version' pyproject.toml | sed 's/.*"\(.*\)".*/\1/')
  [ -n "$v" ] && versions+=("pyproject.toml=$v") && files_checked+=("pyproject.toml")
fi

if [ -f "gemini-extension.json" ]; then
  v=$(python3 -c "import json; print(json.load(open('gemini-extension.json')).get('version',''))" 2>/dev/null)
  [ -n "$v" ] && versions+=("gemini-extension.json=$v") && files_checked+=("gemini-extension.json")
fi

if [ -f "server.json" ]; then
  v=$(python3 -c "import json; print(json.load(open('server.json')).get('version',''))" 2>/dev/null)
  [ -n "$v" ] && versions+=("server.json=$v") && files_checked+=("server.json")
fi

if [ -f "mcpb/manifest.json" ]; then
  v=$(python3 -c "import json; print(json.load(open('mcpb/manifest.json')).get('version',''))" 2>/dev/null)
  [ -n "$v" ] && versions+=("mcpb/manifest.json=$v") && files_checked+=("mcpb/manifest.json")
fi

if [ -f "docker-compose.prod.yml" ]; then
  # Default image tag: ghcr.io/jmagar/cortex:${CORTEX_VERSION:-X.Y.Z}
  v=$(grep -m1 -o 'CORTEX_VERSION:-[0-9][0-9.]*' docker-compose.prod.yml | cut -d- -f2)
  [ -n "$v" ] && versions+=("docker-compose.prod.yml=$v") && files_checked+=("docker-compose.prod.yml")
fi

# Need at least one version source
if [ ${#versions[@]} -eq 0 ]; then
  echo "[version-sync] No version-bearing files found — skipping"
  exit 0
fi

# Check all versions match
canonical=""
mismatch=0
for entry in "${versions[@]}"; do
  file="${entry%%=*}"
  ver="${entry##*=}"
  if [ -z "$canonical" ]; then
    canonical="$ver"
  elif [ "$ver" != "$canonical" ]; then
    mismatch=1
  fi
done

if [ "$mismatch" -eq 1 ]; then
  echo "[version-sync] FAIL — versions are out of sync:"
  for entry in "${versions[@]}"; do
    file="${entry%%=*}"
    ver="${entry##*=}"
    marker=" "
    [ "$ver" != "$canonical" ] && marker="!"
    echo "  $marker $file: $ver"
  done
  echo ""
  echo "All version-bearing files must have the same version."
  echo "Files checked: ${files_checked[*]}"
  exit 1
fi

# Check CHANGELOG.md has an entry for the current version
if [ -f "CHANGELOG.md" ]; then
  if ! changelog_has_release_heading "$canonical"; then
    if [ "$REQUIRE_CHANGELOG" -eq 1 ]; then
      echo "[version-sync] FAIL — CHANGELOG.md has no release heading for version $canonical"
      echo "  Add a changelog entry before releasing."
      exit 1
    else
      echo "[version-sync] WARN — CHANGELOG.md has no release heading for version $canonical"
      echo "  Add a changelog entry before releasing."
    fi
  fi
elif [ "$REQUIRE_CHANGELOG" -eq 1 ]; then
  echo "[version-sync] FAIL — CHANGELOG.md is required for release checks"
  exit 1
fi

echo "[version-sync] OK — all ${#versions[@]} files at v${canonical}"
exit 0
