#!/usr/bin/env bash
# SessionStart hook — deploys or connects syslog-mcp based on userConfig
set -euo pipefail

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
MAX_DB_SIZE_MB="${CLAUDE_PLUGIN_OPTION_MAX_DB_SIZE_MB:-1024}"
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

  if [[ "${USE_DOCKER}" == "true" ]]; then
    # Docker compose reads these directly from .env
    local db_line="SYSLOG_MCP_DATA_VOLUME=${DATA_DIR}"
    local config_line="SYSLOG_MCP_CONFIG_VOLUME=${CLAUDE_PLUGIN_DATA}/config"
  else
    # Systemd binary reads these as direct env vars
    local db_line="SYSLOG_MCP_DB_PATH=${DATA_DIR}/syslog.db"
    local config_line=""
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
SYSLOG_DOCKER_INGEST_ENABLED=${DOCKER_INGEST}
EOF
)

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

  # Refresh compose file if plugin updated
  if ! diff -q "${CLAUDE_PLUGIN_ROOT}/docker-compose.yml" "${COMPOSE_FILE}" >/dev/null 2>&1; then
    cp "${CLAUDE_PLUGIN_ROOT}/docker-compose.yml" "${COMPOSE_FILE}"
  fi

  write_env || true
  # Docker compose reads .env from its working directory
  cp "${ENV_FILE}" "${COMPOSE_DIR}/.env"

  cd "${COMPOSE_DIR}"
  if docker compose ps --quiet syslog-mcp 2>/dev/null | grep -q .; then
    docker compose up -d --force-recreate
  else
    docker compose up -d
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
