#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

NO_BUILD=0
for arg in "$@"; do
  case "${arg}" in
    --no-build) NO_BUILD=1 ;;
    --help|-h)
      echo "Usage: scripts/build-mcpb.sh [--no-build]"
      exit 0
      ;;
    *)
      echo "unknown argument: ${arg}" >&2
      exit 2
      ;;
  esac
done

VERSION="$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
MANIFEST_VERSION="$(python3 -c 'import json; print(json.load(open("mcpb/manifest.json"))["version"])')"
if [ "${VERSION}" != "${MANIFEST_VERSION}" ]; then
  echo "mcpb manifest version ${MANIFEST_VERSION} does not match Cargo.toml ${VERSION}" >&2
  exit 1
fi

if [ "${NO_BUILD}" -eq 0 ]; then
  cargo build --release
fi

TARGET_DIR="${CARGO_TARGET_DIR:-target}"
if [ ! -x "${TARGET_DIR}/release/syslog" ] && [ -x ".cache/cargo/release/syslog" ]; then
  TARGET_DIR=".cache/cargo"
fi
if [ ! -x "${TARGET_DIR}/release/syslog" ]; then
  echo "missing release binary: ${TARGET_DIR}/release/syslog" >&2
  exit 1
fi

STAGE_DIR="dist/mcpb/cortex"
OUT_FILE="dist/cortex-${VERSION}-linux.mcpb"
rm -rf "${STAGE_DIR}"
mkdir -p "${STAGE_DIR}/server"

cp mcpb/manifest.json "${STAGE_DIR}/manifest.json"
install -m 755 "${TARGET_DIR}/release/syslog" "${STAGE_DIR}/server/syslog"

npx --yes @anthropic-ai/mcpb validate "${STAGE_DIR}/manifest.json"
rm -f "${OUT_FILE}"
npx --yes @anthropic-ai/mcpb pack "${STAGE_DIR}" "${OUT_FILE}"
npx --yes @anthropic-ai/mcpb info "${OUT_FILE}" >/dev/null

echo "Built ${OUT_FILE}"
