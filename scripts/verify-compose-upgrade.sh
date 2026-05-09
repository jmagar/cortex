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

render_config() {
  (cd "${tmpdir}" && docker compose -f docker-compose.yml config)
}

require_rendered_line() {
  local expected="$1"
  printf '%s\n' "${rendered}" | grep -F "${expected}" >/dev/null
}

cat > "${tmpdir}/.env" <<EOF
HIVE_MCP_VERSION=latest
HIVE_MCP_PORT=43100
SYSLOG_HOST_PORT=41514
SYSLOG_PORT=1514
DOCKER_NETWORK=hive-upgrade-check
EOF

rendered="$(render_config)"
require_rendered_line "source: syslog-mcp-data"
require_rendered_line "target: /data"
require_rendered_line "ghcr.io/jmagar/hive-mcp:"

cat >> "${tmpdir}/.env" <<EOF
SYSLOG_MCP_DATA_VOLUME=${sentinel_dir}
EOF
rendered="$(render_config)"
require_rendered_line "source: ${sentinel_dir}"
require_rendered_line "target: /data"

sed -i '/^SYSLOG_MCP_DATA_VOLUME=/d' "${tmpdir}/.env"
cat >> "${tmpdir}/.env" <<EOF
SYSLOG_MCP_DATA_VOLUME=ignored-legacy-volume
HIVE_MCP_DATA_VOLUME=${sentinel_dir}
EOF
rendered="$(render_config)"
require_rendered_line "source: ${sentinel_dir}"
require_rendered_line "target: /data"

if ! command -v docker >/dev/null 2>&1; then
  echo "SKIP: docker not available; rendered compose data path preserved"
  exit 0
fi

if ! docker info >/dev/null 2>&1; then
  echo "SKIP: docker daemon unavailable; rendered compose data path preserved"
  exit 0
fi

docker network inspect hive-upgrade-check >/dev/null 2>&1 || docker network create hive-upgrade-check >/dev/null
echo "OK: compose upgrade preserves default, legacy, and explicit Hive data mounts"
