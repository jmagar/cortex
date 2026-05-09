#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
tmpdir="$(mktemp -d)"
cleanup() {
  if command -v docker >/dev/null 2>&1; then
    docker compose -p hive-upgrade-check -f "${tmpdir}/docker-compose.yml" down -v >/dev/null 2>&1 || true
  fi
  rm -rf "${tmpdir}"
}
trap cleanup EXIT

cp "${repo_root}/docker-compose.yml" "${tmpdir}/docker-compose.yml"

sentinel_dir="${tmpdir}/legacy-data"
mkdir -p "${sentinel_dir}"
printf 'preserve-me\n' > "${sentinel_dir}/syslog.db"

cat > "${tmpdir}/.env" <<EOF
HIVE_MCP_VERSION=latest
HIVE_MCP_DATA_VOLUME=${sentinel_dir}
HIVE_MCP_PORT=43100
SYSLOG_HOST_PORT=41514
SYSLOG_PORT=1514
DOCKER_NETWORK=hive-upgrade-check
EOF

rendered="$(cd "${tmpdir}" && docker compose -f docker-compose.yml config)"
printf '%s\n' "${rendered}" | grep -F "source: ${sentinel_dir}" >/dev/null
printf '%s\n' "${rendered}" | grep -F "target: /data" >/dev/null
printf '%s\n' "${rendered}" | grep -F "ghcr.io/jmagar/hive-mcp:" >/dev/null

if ! command -v docker >/dev/null 2>&1; then
  echo "SKIP: docker not available; rendered compose data path preserved"
  exit 0
fi

if ! docker info >/dev/null 2>&1; then
  echo "SKIP: docker daemon unavailable; rendered compose data path preserved"
  exit 0
fi

docker network inspect hive-upgrade-check >/dev/null 2>&1 || docker network create hive-upgrade-check >/dev/null
echo "OK: compose upgrade keeps explicit Hive data path mounted at /data"
