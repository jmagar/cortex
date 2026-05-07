#!/usr/bin/env bash
# SessionStart hook — deploys or connects syslog-mcp based on userConfig
set -euo pipefail

# When invoked directly (e.g. /syslog:redeploy), the plugin runtime vars are
# absent. Derive CLAUDE_PLUGIN_ROOT from the script's own location and default
# CLAUDE_PLUGIN_DATA to the well-known plugin data directory so the hook works
# both from the plugin system and from manual invocation.
: "${CLAUDE_PLUGIN_ROOT:=$(cd "$(dirname "$0")/.." && pwd)}"
: "${CLAUDE_PLUGIN_DATA:=${HOME}/.claude/plugins/data/syslog-jmagar-lab}"

# Seed the token from the existing env file when the plugin option isn't set,
# so /syslog:redeploy doesn't fail just because the env var wasn't injected.
if [[ -z "${CLAUDE_PLUGIN_OPTION_API_TOKEN:-}" && -f "${CLAUDE_PLUGIN_DATA}/syslog-mcp.env" ]]; then
  _tok=$(grep -m1 '^SYSLOG_MCP_API_TOKEN=' "${CLAUDE_PLUGIN_DATA}/syslog-mcp.env" | cut -d= -f2- || true)
  [[ -n "${_tok}" ]] && CLAUDE_PLUGIN_OPTION_API_TOKEN="${_tok}"
  unset _tok
fi

# ── Config from userConfig ────────────────────────────────────────────────────
IS_SERVER="${CLAUDE_PLUGIN_OPTION_IS_SERVER:-true}"
USE_DOCKER="${CLAUDE_PLUGIN_OPTION_USE_DOCKER:-false}"
API_TOKEN="${CLAUDE_PLUGIN_OPTION_API_TOKEN:?API token is required}"
SERVER_URL="${CLAUDE_PLUGIN_OPTION_SERVER_URL:-http://localhost:3100}"
SYSLOG_HOST="${CLAUDE_PLUGIN_OPTION_SYSLOG_HOST:-0.0.0.0}"
SYSLOG_PORT="${CLAUDE_PLUGIN_OPTION_SYSLOG_PORT:-1514}"
MCP_HOST="${CLAUDE_PLUGIN_OPTION_MCP_HOST:-0.0.0.0}"
MCP_PORT="${CLAUDE_PLUGIN_OPTION_MCP_PORT:-3100}"
DATA_DIR="${CLAUDE_PLUGIN_OPTION_DATA_DIR:-${CLAUDE_PLUGIN_DATA}}"
MAX_DB_SIZE_MB="${CLAUDE_PLUGIN_OPTION_MAX_DB_SIZE_MB:-8192}"
RETENTION_DAYS="${CLAUDE_PLUGIN_OPTION_RETENTION_DAYS:-90}"
DOCKER_INGEST="${CLAUDE_PLUGIN_OPTION_DOCKER_INGEST_ENABLED:-false}"
FLEET_HOSTS="${CLAUDE_PLUGIN_OPTION_FLEET_HOSTS:-}"

# ── Paths ─────────────────────────────────────────────────────────────────────
ENV_FILE="${CLAUDE_PLUGIN_DATA}/syslog-mcp.env"
UNIT_FILE="${HOME}/.config/systemd/user/syslog-mcp.service"
COMPOSE_DIR="${CLAUDE_PLUGIN_DATA}"
COMPOSE_FILE="${COMPOSE_DIR}/docker-compose.yml"

# ── Helpers ───────────────────────────────────────────────────────────────────

# Returns 0 if env file was written/changed, 1 if unchanged
write_env() {
  mkdir -p "${CLAUDE_PLUGIN_DATA}"

  local batch_size="${CLAUDE_PLUGIN_OPTION_BATCH_SIZE:-}"
  local write_channel_capacity="${CLAUDE_PLUGIN_OPTION_WRITE_CHANNEL_CAPACITY:-}"
  if [[ -f "${ENV_FILE}" ]]; then
    [[ -n "${batch_size}" ]] || batch_size="$(awk -F= '$1 == "SYSLOG_BATCH_SIZE" {print substr($0, index($0, "=") + 1); exit}' "${ENV_FILE}")"
    [[ -n "${write_channel_capacity}" ]] || write_channel_capacity="$(awk -F= '$1 == "SYSLOG_WRITE_CHANNEL_CAPACITY" {print substr($0, index($0, "=") + 1); exit}' "${ENV_FILE}")"
  fi
  batch_size="${batch_size:-100}"
  write_channel_capacity="${write_channel_capacity:-10000}"

  if [[ "${USE_DOCKER}" == "true" ]]; then
    # Docker compose reads these directly from .env. Pin UID/GID so the
    # container writes syslog.db with the host user's ownership — keeps the
    # same file readable by the systemd binary if you switch modes back.
    local db_line="SYSLOG_MCP_DATA_VOLUME=${DATA_DIR}"
    local config_line=""
    local uid_line="SYSLOG_UID=$(id -u)"
    local gid_line="SYSLOG_GID=$(id -g)"
  else
    # Systemd binary reads these as direct env vars
    local db_line="SYSLOG_MCP_DB_PATH=${DATA_DIR}/syslog.db"
    local config_line=""
    local uid_line=""
    local gid_line=""
  fi

  local new_env
  new_env=$(cat << EOF
SYSLOG_HOST=${SYSLOG_HOST}
SYSLOG_PORT=${SYSLOG_PORT}
SYSLOG_MCP_HOST=${MCP_HOST}
SYSLOG_MCP_PORT=${MCP_PORT}
SYSLOG_MCP_API_TOKEN=${API_TOKEN}
${db_line}
SYSLOG_MCP_MAX_DB_SIZE_MB=${MAX_DB_SIZE_MB}
SYSLOG_MCP_RETENTION_DAYS=${RETENTION_DAYS}
SYSLOG_BATCH_SIZE=${batch_size}
SYSLOG_WRITE_CHANNEL_CAPACITY=${write_channel_capacity}
SYSLOG_DOCKER_INGEST_ENABLED=${DOCKER_INGEST}
EOF
)
  [[ -n "${uid_line}" ]] && new_env="${new_env}
${uid_line}
${gid_line}"

  [[ -n "${config_line}" ]] && new_env="${new_env}
${config_line}"

  # Fleet hosts feed Docker ingest only when ingest is enabled
  if [[ "${DOCKER_INGEST}" == "true" && -n "${FLEET_HOSTS}" ]]; then
    new_env="${new_env}
SYSLOG_DOCKER_HOSTS=${FLEET_HOSTS}"
  fi

  if [[ -f "${ENV_FILE}" ]] && diff -q <(echo "${new_env}") "${ENV_FILE}" >/dev/null 2>&1; then
    return 1  # unchanged
  fi

  echo "${new_env}" > "${ENV_FILE}"
  return 0  # changed
}


setup_systemd() {
  mkdir -p "${HOME}/.config/systemd/user"

  # If a previous run deployed via docker, stop the container first so the
  # systemd binary can bind the same ports. Idempotent.
  if [[ -f "${COMPOSE_FILE}" ]] && command -v docker >/dev/null 2>&1; then
    if (cd "${COMPOSE_DIR}" && docker compose ps --quiet syslog-mcp 2>/dev/null | grep -q .); then
      echo "syslog-mcp: stopping existing docker container before systemd cutover"
      (cd "${COMPOSE_DIR}" && docker compose down)
    fi
  fi

  local new_unit
  new_unit=$(cat << EOF
[Unit]
Description=syslog-mcp server
After=network.target

[Service]
ExecStart=${CLAUDE_PLUGIN_ROOT}/bin/syslog serve mcp
EnvironmentFile=${ENV_FILE}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF
)

  local unit_changed=false
  if ! diff -q <(echo "${new_unit}") "${UNIT_FILE}" >/dev/null 2>&1; then
    echo "${new_unit}" > "${UNIT_FILE}"
    unit_changed=true
  fi

  local env_changed=false
  write_env && env_changed=true || true

  if [[ "${unit_changed}" == "true" ]]; then
    systemctl --user daemon-reload
    systemctl --user enable --now syslog-mcp
  elif [[ "${env_changed}" == "true" ]]; then
    systemctl --user restart syslog-mcp
  elif ! systemctl --user is-active --quiet syslog-mcp; then
    systemctl --user start syslog-mcp
  fi

  echo "syslog-mcp: systemd service running on ${MCP_HOST}:${MCP_PORT}"
}

setup_docker() {
  mkdir -p "${COMPOSE_DIR}"

  # If a previous run deployed via systemd, stop it first so the docker
  # container can bind the same ports. Idempotent — silent if absent/inactive.
  if systemctl --user list-unit-files syslog-mcp.service >/dev/null 2>&1 \
     && systemctl --user is-active --quiet syslog-mcp.service; then
    echo "syslog-mcp: stopping existing systemd unit before docker cutover"
    systemctl --user stop syslog-mcp.service
    systemctl --user disable syslog-mcp.service >/dev/null 2>&1 || true
  fi

  # Refresh compose file if plugin updated
  if ! diff -q "${CLAUDE_PLUGIN_ROOT}/docker-compose.yml" "${COMPOSE_FILE}" >/dev/null 2>&1; then
    cp "${CLAUDE_PLUGIN_ROOT}/docker-compose.yml" "${COMPOSE_FILE}"
  fi

  write_env || true
  # Docker compose reads .env from its working directory
  cp "${ENV_FILE}" "${COMPOSE_DIR}/.env"

  cd "${COMPOSE_DIR}"

  # Pull the published image. If the registry is unreachable or the tag
  # doesn't exist, fall through to `up` which will use a cached image or
  # (if --build is added) build from source — neither is expected in the
  # plugin path, but this keeps the hook resilient in dev workflows.
  docker compose pull --quiet syslog-mcp 2>&1 || \
    echo "syslog-mcp: pull failed; will try cached image" >&2

  if docker compose ps --quiet syslog-mcp 2>/dev/null | grep -q .; then
    docker compose up -d --force-recreate --no-build
  else
    docker compose up -d --no-build
  fi

  echo "syslog-mcp: docker container running on ${MCP_HOST}:${MCP_PORT}"
}

validate_client() {
  if curl -sf "${SERVER_URL}/health" >/dev/null 2>&1; then
    echo "syslog-mcp: connected to ${SERVER_URL}"
  else
    echo "WARNING: syslog-mcp server at ${SERVER_URL} is not reachable" >&2
  fi
}

link_binary() {
  # Symlink the bundled binary into the user's PATH. ${CLAUDE_PLUGIN_ROOT}
  # changes on plugin update, so we re-link every SessionStart.
  mkdir -p "${HOME}/.local/bin"
  ln -sf "${CLAUDE_PLUGIN_ROOT}/bin/syslog" "${HOME}/.local/bin/syslog"
}

# ── Main ──────────────────────────────────────────────────────────────────────
link_binary

if [[ "${IS_SERVER}" == "true" ]]; then
  if [[ "${USE_DOCKER}" == "true" ]]; then
    setup_docker
  else
    setup_systemd
  fi
else
  validate_client
fi
