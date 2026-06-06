#!/usr/bin/env bash
# bump-version.sh — update release version-bearing files atomically.
#
# Usage:
#   ./scripts/bump-version.sh 1.3.5
#   ./scripts/bump-version.sh patch   # auto-increment patch
#   ./scripts/bump-version.sh minor   # auto-increment minor
#   ./scripts/bump-version.sh major   # auto-increment major

set -euo pipefail

REPO_ROOT="${CLAUDE_PLUGIN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

VERSION_FILES=(
    "${REPO_ROOT}/Cargo.toml"
    "${REPO_ROOT}/server.json"
    "${REPO_ROOT}/package.json"
    "${REPO_ROOT}/pyproject.toml"
    "${REPO_ROOT}/gemini-extension.json"
    "${REPO_ROOT}/mcpb/manifest.json"
)

current_version() {
    grep -m1 '^version' "${REPO_ROOT}/Cargo.toml" \
        | sed 's/.*"\(.*\)".*/\1/'
}

bump() {
    local version="$1" part="$2"
    local major minor patch
    IFS='.' read -r major minor patch <<< "$version"
    case "$part" in
        major) echo "$((major + 1)).0.0" ;;
        minor) echo "${major}.$((minor + 1)).0" ;;
        patch) echo "${major}.${minor}.$((patch + 1))" ;;
    esac
}

# Resolve new version
ARG="${1:-}"
CURRENT="$(current_version)"

case "$ARG" in
    major|minor|patch) NEW="$(bump "$CURRENT" "$ARG")" ;;
    "") echo "Usage: $0 <version|major|minor|patch>"; exit 1 ;;
    *) NEW="$ARG" ;;
esac

echo "Bumping $CURRENT → $NEW"

for file in "${VERSION_FILES[@]}"; do
    [ -f "$file" ] || { echo "  skip (not found): $file"; continue; }
    sed -i "s/\"version\": \"${CURRENT}\"/\"version\": \"${NEW}\"/" "$file"
    sed -i "s/^version = \"${CURRENT}\"/version = \"${NEW}\"/" "$file"
    sed -i "s/cortex:v${CURRENT}/cortex:v${NEW}/g" "$file"
    echo "  updated: ${file#"${REPO_ROOT}/"}"
done

if [ -f "${REPO_ROOT}/CHANGELOG.md" ]; then
    if grep -qF "## [${NEW}]" "${REPO_ROOT}/CHANGELOG.md"; then
        echo "  unchanged: CHANGELOG.md already has ${NEW}"
    elif grep -q '^## \[Unreleased\]' "${REPO_ROOT}/CHANGELOG.md"; then
        today="$(date +%F)"
        sed -i "0,/^## \\[Unreleased\\]/s//## [Unreleased]\\n\\n## [${NEW}] - ${today}/" "${REPO_ROOT}/CHANGELOG.md"
        echo "  updated: CHANGELOG.md"
    else
        echo "  WARN: CHANGELOG.md has no [Unreleased] heading; add an entry for ${NEW}" >&2
    fi
fi

echo "Done. Review CHANGELOG.md before committing ${NEW}."
