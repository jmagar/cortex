#!/usr/bin/env bash
# SessionStart hook — deploys or connects hive-mcp based on userConfig
set -euo pipefail

# When invoked directly (e.g. /syslog:redeploy), the plugin runtime vars are
# absent. Derive CLAUDE_PLUGIN_ROOT from the script's own location and default
# CLAUDE_PLUGIN_DATA to the well-known plugin data directory so the hook works
# both from the plugin system and from manual invocation.
: "${CLAUDE_PLUGIN_ROOT:=$(cd "$(dirname "$0")/.." && pwd)}"
: "${CLAUDE_PLUGIN_DATA:=${HOME}/.claude/plugins/data/syslog-jmagar-lab}"

existing_env_value() {
  local key="$1"
  local file
  local value
  for file in \
    "${CLAUDE_PLUGIN_DATA}/.env" \
    "${CLAUDE_PLUGIN_DATA}/hive-mcp.env" \
    "${CLAUDE_PLUGIN_DATA}/syslog-mcp.env"
  do
    [[ -f "${file}" ]] || continue
    value="$(awk -F= -v key="${key}" '$1 == key {print substr($0, index($0, "=") + 1); exit}' "${file}")"
    if [[ -n "${value}" ]]; then
      printf '%s\n' "${value}"
      return 0
    fi
  done
  return 0
}

existing_env_value_any() {
  local key
  local value
  for key in "$@"; do
    value="$(existing_env_value "${key}")"
    if [[ -n "${value}" ]]; then
      printf '%s\n' "${value}"
      return 0
    fi
  done
  return 0
}

validate_port_value() {
  local name="$1" value="$2"
  if ! [[ "${value}" =~ ^[0-9]+$ ]] || (( value < 1 || value > 65535 )); then
    echo "ERROR: ${name} must be a TCP/UDP port number (1-65535), got: ${value}" >&2
    exit 1
  fi
}

mcp_host_is_loopback() {
  case "$1" in
    127.*|::1) return 0 ;;
    *) return 1 ;;
  esac
}

systemd_unit_exists() {
  systemctl --user list-unit-files "$1" >/dev/null 2>&1 || [[ -f "${HOME}/.config/systemd/user/$1" ]]
}

stop_disable_remove_systemd_unit() {
  local unit="$1"
  local unit_path="${HOME}/.config/systemd/user/${unit}"

  if ! systemd_unit_exists "${unit}"; then
    return 0
  fi

  if systemctl --user is-active --quiet "${unit}" 2>/dev/null; then
    echo "hive-mcp: stopping existing ${unit} before cutover"
    systemctl --user stop "${unit}" || {
      echo "ERROR: failed to stop ${unit}; refusing to continue cutover" >&2
      return 1
    }
    if systemctl --user is-active --quiet "${unit}" 2>/dev/null; then
      echo "ERROR: ${unit} is still active after stop; refusing to continue cutover" >&2
      return 1
    fi
  fi
  if systemctl --user is-enabled --quiet "${unit}" 2>/dev/null; then
    systemctl --user disable "${unit}" >/dev/null 2>&1 || {
      echo "ERROR: failed to disable ${unit}; refusing to continue cutover" >&2
      return 1
    }
    if systemctl --user is-enabled --quiet "${unit}" 2>/dev/null; then
      echo "ERROR: ${unit} is still enabled after disable; refusing to continue cutover" >&2
      return 1
    fi
  fi
  if [[ -f "${unit_path}" ]]; then
    rm -f "${unit_path}"
    systemctl --user daemon-reload
  fi
}

legacy_compose_file() {
  local path="${COMPOSE_FILE}"
  [[ -f "${path}" ]] && printf '%s\n' "${path}"
}

stop_compose_service_if_running() {
  local service="$1"
  local file
  file="$(legacy_compose_file)"
  [[ -n "${file}" ]] || return 0
  if docker compose -f "${file}" ps --quiet "${service}" 2>/dev/null | grep -q .; then
    echo "hive-mcp: stopping existing docker compose service ${service} before cutover"
    (cd "${COMPOSE_DIR}" && docker compose down)
  fi
}

docker_container_running() {
  local name="$1"
  docker ps --filter "name=^/${name}$" --quiet 2>/dev/null | grep -q .
}

# Seed the token from the existing env file when the plugin option isn't set,
# so /syslog:redeploy doesn't fail just because the env var wasn't injected.
NO_AUTH="${CLAUDE_PLUGIN_OPTION_NO_AUTH:-$(existing_env_value NO_AUTH)}"
NO_AUTH="${NO_AUTH:-false}"
NO_AUTH="$(printf '%s' "${NO_AUTH}" | tr '[:upper:]' '[:lower:]')"
AUTH_MODE="${CLAUDE_PLUGIN_OPTION_AUTH_MODE:-$(existing_env_value_any HIVE_MCP_AUTH_MODE SYSLOG_MCP_AUTH_MODE)}"
AUTH_MODE="${AUTH_MODE:-bearer}"
AUTH_MODE="$(printf '%s' "${AUTH_MODE}" | tr '[:upper:]' '[:lower:]')"

if [[ "${NO_AUTH}" != "true" && -z "${CLAUDE_PLUGIN_OPTION_API_TOKEN:-}" ]]; then
  _tok="$(existing_env_value_any HIVE_MCP_TOKEN SYSLOG_MCP_TOKEN)"
  [[ -n "${_tok}" ]] || _tok="$(existing_env_value SYSLOG_MCP_API_TOKEN)"
  [[ -n "${_tok}" ]] && CLAUDE_PLUGIN_OPTION_API_TOKEN="${_tok}"
  unset _tok
fi

# ── Config from userConfig ────────────────────────────────────────────────────
IS_SERVER="${CLAUDE_PLUGIN_OPTION_IS_SERVER:-true}"
USE_DOCKER="${CLAUDE_PLUGIN_OPTION_USE_DOCKER:-false}"
API_TOKEN="${CLAUDE_PLUGIN_OPTION_API_TOKEN:-}"
SERVER_URL="${CLAUDE_PLUGIN_OPTION_SERVER_URL:-http://localhost:3100}"
SYSLOG_HOST="${CLAUDE_PLUGIN_OPTION_SYSLOG_HOST:-0.0.0.0}"
SYSLOG_PORT="${CLAUDE_PLUGIN_OPTION_SYSLOG_PORT:-1514}"
SYSLOG_HOST_PORT="${CLAUDE_PLUGIN_OPTION_SYSLOG_HOST_PORT:-$(existing_env_value SYSLOG_HOST_PORT)}"
SYSLOG_HOST_PORT="${SYSLOG_HOST_PORT:-1514}"
MCP_HOST="${CLAUDE_PLUGIN_OPTION_MCP_HOST:-0.0.0.0}"
MCP_PORT="${CLAUDE_PLUGIN_OPTION_MCP_PORT:-3100}"
validate_port_value SYSLOG_PORT "${SYSLOG_PORT}"
validate_port_value SYSLOG_HOST_PORT "${SYSLOG_HOST_PORT}"
validate_port_value HIVE_MCP_PORT "${MCP_PORT}"
DATA_DIR="${CLAUDE_PLUGIN_OPTION_DATA_DIR:-${CLAUDE_PLUGIN_DATA}}"
MAX_DB_SIZE_MB="${CLAUDE_PLUGIN_OPTION_MAX_DB_SIZE_MB:-8192}"
RETENTION_DAYS="${CLAUDE_PLUGIN_OPTION_RETENTION_DAYS:-90}"
DOCKER_INGEST="${CLAUDE_PLUGIN_OPTION_DOCKER_INGEST_ENABLED:-false}"
FLEET_HOSTS="${CLAUDE_PLUGIN_OPTION_FLEET_HOSTS:-}"
PUBLIC_URL="${CLAUDE_PLUGIN_OPTION_PUBLIC_URL:-$(existing_env_value_any HIVE_MCP_PUBLIC_URL SYSLOG_MCP_PUBLIC_URL)}"
GOOGLE_CLIENT_ID="${CLAUDE_PLUGIN_OPTION_GOOGLE_CLIENT_ID:-$(existing_env_value_any HIVE_MCP_GOOGLE_CLIENT_ID SYSLOG_MCP_GOOGLE_CLIENT_ID)}"
GOOGLE_CLIENT_SECRET="${CLAUDE_PLUGIN_OPTION_GOOGLE_CLIENT_SECRET:-$(existing_env_value_any HIVE_MCP_GOOGLE_CLIENT_SECRET SYSLOG_MCP_GOOGLE_CLIENT_SECRET)}"
AUTH_ADMIN_EMAIL="${CLAUDE_PLUGIN_OPTION_AUTH_ADMIN_EMAIL:-$(existing_env_value_any HIVE_MCP_AUTH_ADMIN_EMAIL SYSLOG_MCP_AUTH_ADMIN_EMAIL)}"
AUTH_ALLOWED_REDIRECT_URIS="${CLAUDE_PLUGIN_OPTION_AUTH_ALLOWED_REDIRECT_URIS:-$(existing_env_value_any HIVE_MCP_AUTH_ALLOWED_REDIRECT_URIS SYSLOG_MCP_AUTH_ALLOWED_REDIRECT_URIS)}"

if [[ "${NO_AUTH}" != "true" && -z "${API_TOKEN}" ]]; then
  if ! [[ "${AUTH_MODE}" == "oauth" && "${IS_SERVER}" == "true" ]] || ! mcp_host_is_loopback "${MCP_HOST}"; then
    echo "ERROR: API token is required unless no_auth is true or OAuth server mode binds MCP to loopback" >&2
    echo "       OAuth mode still needs HIVE_MCP_TOKEN, or legacy SYSLOG_MCP_TOKEN, when OTLP /v1/logs is exposed on a non-loopback listener." >&2
    exit 1
  fi
fi

# ── Paths ─────────────────────────────────────────────────────────────────────
ENV_FILE="${CLAUDE_PLUGIN_DATA}/.env"
LEGACY_ENV_FILE="${CLAUDE_PLUGIN_DATA}/syslog-mcp.env"
UNIT_FILE="${HOME}/.config/systemd/user/hive-mcp.service"
COMPOSE_DIR="${CLAUDE_PLUGIN_DATA}"
COMPOSE_FILE="${COMPOSE_DIR}/docker-compose.yml"

# ── Helpers ───────────────────────────────────────────────────────────────────

strip_trailing_mcp_path() {
  local url="${1%/}"
  if [[ "${url}" == */mcp ]]; then
    url="${url%/mcp}"
  fi
  printf '%s\n' "${url}"
}

derive_public_url() {
  if [[ -n "${PUBLIC_URL}" ]]; then
    strip_trailing_mcp_path "${PUBLIC_URL}"
    return
  fi
  if [[ "${SERVER_URL}" == https://* ]]; then
    strip_trailing_mcp_path "${SERVER_URL}"
  fi
}

codex_oauth_callback_url() {
  local config="${HOME}/.codex/config.toml"
  [[ -f "${config}" ]] || return 0
  awk -F= '
    $1 ~ /^[[:space:]]*mcp_oauth_callback_url[[:space:]]*$/ {
      value = $2
      sub(/^[[:space:]]*"/, "", value)
      sub(/"[[:space:]]*$/, "", value)
      print value
      exit
    }
  ' "${config}"
}

append_csv_unique() {
  local csv="$1"
  local value="$2"
  [[ -n "${value}" ]] || { printf '%s\n' "${csv}"; return; }

  local existing
  IFS=',' read -r -a existing <<< "${csv}"
  for item in "${existing[@]}"; do
    item="${item#"${item%%[![:space:]]*}"}"
    item="${item%"${item##*[![:space:]]}"}"
    if [[ "${item}" == "${value}" ]]; then
      printf '%s\n' "${csv}"
      return
    fi
  done

  if [[ -n "${csv}" ]]; then
    printf '%s,%s\n' "${csv}" "${value}"
  else
    printf '%s\n' "${value}"
  fi
}

oauth_env_block() {
  if [[ "${NO_AUTH}" == "true" ]]; then
    return 0
  fi
  if [[ "${AUTH_MODE}" != "bearer" && "${AUTH_MODE}" != "oauth" ]]; then
    echo "ERROR: auth_mode must be bearer or oauth" >&2
    return 1
  fi
  if [[ "${AUTH_MODE}" != "oauth" ]]; then
    return 0
  fi

  local public_url
  public_url="$(derive_public_url)"
  if [[ -z "${public_url}" ]]; then
    echo "ERROR: OAuth mode requires public_url or an https server_url" >&2
    return 1
  fi
  if [[ -z "${GOOGLE_CLIENT_ID}" || -z "${GOOGLE_CLIENT_SECRET}" || -z "${AUTH_ADMIN_EMAIL}" ]]; then
    echo "ERROR: OAuth mode requires google_client_id, google_client_secret, and auth_admin_email" >&2
    return 1
  fi

  local redirects="${AUTH_ALLOWED_REDIRECT_URIS}"
  redirects="$(append_csv_unique "${redirects}" "https://claude.ai/api/mcp/auth_callback")"
  redirects="$(append_csv_unique "${redirects}" "https://claudeai.ai/api/mcp/auth_callback")"

  local codex_callback
  codex_callback="$(codex_oauth_callback_url)"
  if [[ -n "${codex_callback}" ]]; then
    redirects="$(append_csv_unique "${redirects}" "${codex_callback}")"
  fi

  cat << EOF
HIVE_MCP_AUTH_MODE=oauth
HIVE_MCP_PUBLIC_URL=${public_url}
HIVE_MCP_GOOGLE_CLIENT_ID=${GOOGLE_CLIENT_ID}
HIVE_MCP_GOOGLE_CLIENT_SECRET=${GOOGLE_CLIENT_SECRET}
HIVE_MCP_AUTH_ADMIN_EMAIL=${AUTH_ADMIN_EMAIL}
HIVE_MCP_AUTH_ALLOWED_REDIRECT_URIS=${redirects}
HIVE_MCP_AUTH_DISABLE_STATIC_TOKEN_WITH_OAUTH=false
EOF
}

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

  local db_line config_line uid_line gid_line
  if [[ "${USE_DOCKER}" == "true" ]]; then
    # Docker compose reads these directly from .env. Pin UID/GID so the
    # container writes syslog.db with the host user's ownership — keeps the
    # same file readable by the systemd binary if you switch modes back.
    db_line="HIVE_MCP_DATA_VOLUME=${DATA_DIR}"
    config_line=""
    uid_line="SYSLOG_UID=$(id -u)"
    gid_line="SYSLOG_GID=$(id -g)"
  else
    # Systemd binary reads these as direct env vars
    db_line="HIVE_MCP_DB_PATH=${DATA_DIR}/syslog.db"
    config_line=""
    uid_line=""
    gid_line=""
  fi

  local new_env
  new_env=$(cat << EOF
SYSLOG_HOST=${SYSLOG_HOST}
SYSLOG_PORT=${SYSLOG_PORT}
SYSLOG_HOST_PORT=${SYSLOG_HOST_PORT}
HIVE_MCP_HOST=${MCP_HOST}
HIVE_MCP_PORT=${MCP_PORT}
NO_AUTH=${NO_AUTH}
${db_line}
HIVE_MCP_MAX_DB_SIZE_MB=${MAX_DB_SIZE_MB}
HIVE_MCP_RETENTION_DAYS=${RETENTION_DAYS}
SYSLOG_BATCH_SIZE=${batch_size}
SYSLOG_WRITE_CHANNEL_CAPACITY=${write_channel_capacity}
HIVE_DOCKER_INGEST_ENABLED=${DOCKER_INGEST}
EOF
)

  if [[ "${NO_AUTH}" != "true" && -n "${API_TOKEN}" ]]; then
    new_env="${new_env}
HIVE_MCP_TOKEN=${API_TOKEN}"
  fi

  local auth_block
  if ! auth_block="$(oauth_env_block)"; then
    return 2
  fi
  [[ -n "${auth_block}" ]] && new_env="${new_env}
${auth_block}"

  [[ -n "${uid_line}" ]] && new_env="${new_env}
${uid_line}
${gid_line}"

  [[ -n "${config_line}" ]] && new_env="${new_env}
${config_line}"

  # Fleet hosts feed Docker ingest only when ingest is enabled
  if [[ "${DOCKER_INGEST}" == "true" && -n "${FLEET_HOSTS}" ]]; then
    new_env="${new_env}
HIVE_DOCKER_HOSTS=${FLEET_HOSTS}"
  fi

  if [[ -f "${ENV_FILE}" ]] && diff -q <(echo "${new_env}") "${ENV_FILE}" >/dev/null 2>&1; then
    rm -f "${LEGACY_ENV_FILE}" "${CLAUDE_PLUGIN_DATA}/hive-mcp.env"
    return 1  # unchanged
  fi

  echo "${new_env}" > "${ENV_FILE}"
  chmod 600 "${ENV_FILE}"
  rm -f "${LEGACY_ENV_FILE}" "${CLAUDE_PLUGIN_DATA}/hive-mcp.env"
  return 0  # changed
}

ensure_env_written() {
  local rc
  if write_env; then
    return 0
  fi
  rc=$?
  if [[ "${rc}" -eq 0 || "${rc}" -eq 1 ]]; then
    return 0
  fi
  return "${rc}"
}


setup_systemd() {
  mkdir -p "${HOME}/.config/systemd/user"

  # ── Pre-flight checks ─────────────────────────────────────────────────────

  # 1. Binary must exist — stale symlink after a plugin cache purge is a
  #    common failure mode that would produce a cryptic systemd start error.
  if [[ ! -x "${CLAUDE_PLUGIN_ROOT}/bin/hive" ]]; then
    echo "ERROR: hive binary not found at ${CLAUDE_PLUGIN_ROOT}/bin/hive" >&2
    return 1
  fi

  # 2. Port conflict check — skip when the service is already running (it owns
  #    the ports; systemctl restart will handle the swap atomically).
  local service_running=false
  if systemctl --user is-active --quiet hive-mcp.service 2>/dev/null; then
    service_running=true
  fi

  stop_disable_remove_systemd_unit "syslog-mcp.service"
  if command -v docker >/dev/null 2>&1; then
    stop_compose_service_if_running "syslog-mcp"
    if docker_container_running "syslog-mcp"; then
      echo "hive-mcp: removing existing syslog-mcp container before systemd cutover"
      docker rm -f syslog-mcp >/dev/null
    fi
  fi
  if [[ "${service_running}" == "false" ]]; then
    for port_proto in "${SYSLOG_PORT}/udp" "${SYSLOG_PORT}/tcp" "${MCP_PORT}/tcp"; do
      local port="${port_proto%%/*}" proto="${port_proto##*/}"
      if ss -"${proto:0:1}"lnp "sport = :${port}" 2>/dev/null | awk 'NR>1 && NF>0' | grep -q .; then
        echo "ERROR: port ${port}/${proto} is already in use — cannot start hive-mcp" >&2
        return 1
      fi
    done
  fi

  # 3. Data dir must be writable by the service user.
  mkdir -p "${DATA_DIR}"
  if ! touch "${DATA_DIR}/.write_test" 2>/dev/null; then
    echo "ERROR: data dir ${DATA_DIR} is not writable by UID $(id -u)" >&2
    return 1
  fi
  rm -f "${DATA_DIR}/.write_test"

  # 4. Warn if the data volume is low on disk.
  local free_mb
  free_mb="$(df -k "${DATA_DIR}" | awk 'NR==2{printf "%d", $4/1024}')"
  if (( free_mb < 512 )); then
    echo "WARNING: only ${free_mb}MB free on ${DATA_DIR} — server may fail to start or write logs" >&2
  fi

  # ── Docker cleanup ────────────────────────────────────────────────────────

  # If a previous run deployed via docker, stop the container first so the
  # systemd binary can bind the same ports. Idempotent.
  if [[ -f "${COMPOSE_FILE}" ]] && command -v docker >/dev/null 2>&1; then
    stop_compose_service_if_running "hive-mcp"
    stop_compose_service_if_running "syslog-mcp"
    if docker_container_running "hive-mcp"; then
      echo "hive-mcp: removing existing hive-mcp container before systemd cutover"
      docker rm -f hive-mcp >/dev/null
    fi
    if docker_container_running "syslog-mcp"; then
      echo "hive-mcp: removing existing syslog-mcp container before systemd cutover"
      docker rm -f syslog-mcp >/dev/null
    fi
  fi

  local new_unit
  new_unit=$(cat << EOF
[Unit]
Description=hive-mcp server
After=network.target

[Service]
ExecStart=${CLAUDE_PLUGIN_ROOT}/bin/hive serve mcp
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

  ensure_env_written

  if [[ "${unit_changed}" == "true" ]]; then
    systemctl --user daemon-reload
    systemctl --user enable --now hive-mcp
  else
    systemctl --user restart hive-mcp
  fi

  echo "hive-mcp: systemd service running on ${MCP_HOST}:${MCP_PORT}"
}

setup_docker() {
  mkdir -p "${COMPOSE_DIR}"

  # ── Pre-flight checks ─────────────────────────────────────────────────────

  # 1. Docker daemon must be reachable before we attempt anything else.
  if ! docker info >/dev/null 2>&1; then
    echo "ERROR: docker daemon is not reachable — is dockerd running?" >&2
    return 1
  fi

  stop_disable_remove_systemd_unit "hive-mcp.service"
  stop_disable_remove_systemd_unit "syslog-mcp.service"
  stop_compose_service_if_running "syslog-mcp"
  if docker_container_running "syslog-mcp"; then
    echo "hive-mcp: removing existing syslog-mcp container before docker cutover"
    docker rm -f syslog-mcp >/dev/null
  fi

  # 2. Port conflict check — only when the container is not already running
  #    (if it is running, it owns the ports and force-recreate will handle it).
  local container_running=false
  local external_named_container=false
  if [[ -f "${COMPOSE_FILE}" ]] && \
     docker compose -f "${COMPOSE_FILE}" ps --quiet hive-mcp 2>/dev/null | grep -q .; then
    container_running=true
  elif [[ -f "${COMPOSE_FILE}" ]] && \
     docker compose -f "${COMPOSE_FILE}" ps --quiet syslog-mcp 2>/dev/null | grep -q .; then
    container_running=true
  elif docker_container_running "hive-mcp"; then
    container_running=true
    external_named_container=true
  elif docker_container_running "syslog-mcp"; then
    container_running=true
    external_named_container=true
  fi
  if [[ "${container_running}" == "false" ]]; then
    for port_proto in "${SYSLOG_HOST_PORT}/udp" "${SYSLOG_HOST_PORT}/tcp" "${MCP_PORT}/tcp"; do
      local port="${port_proto%%/*}" proto="${port_proto##*/}"
      if ss -"${proto:0:1}"lnp "sport = :${port}" 2>/dev/null | awk 'NR>1 && NF>0' | grep -q .; then
        echo "ERROR: port ${port}/${proto} is already in use — cannot start hive-mcp" >&2
        return 1
      fi
    done
  fi

  # 3. Data dir must be writable by the container UID.
  mkdir -p "${DATA_DIR}"
  if ! touch "${DATA_DIR}/.write_test" 2>/dev/null; then
    echo "ERROR: data dir ${DATA_DIR} is not writable by UID $(id -u)" >&2
    return 1
  fi
  rm -f "${DATA_DIR}/.write_test"

  # 4. Warn if the data volume is low on disk (server has its own guardrail but
  #    can't help if it can't open the DB at startup).
  local free_mb
  free_mb="$(df -k "${DATA_DIR}" | awk 'NR==2{printf "%d", $4/1024}')"
  if (( free_mb < 512 )); then
    echo "WARNING: only ${free_mb}MB free on ${DATA_DIR} — server may fail to start or write logs" >&2
  fi

  # ── Systemd cleanup ───────────────────────────────────────────────────────

  # Fully remove the systemd unit so it can't start on boot — docker compose
  # handles restarts via restart: unless-stopped; systemd is not involved.
  stop_disable_remove_systemd_unit "hive-mcp.service"

  # ── Compose setup ─────────────────────────────────────────────────────────

  # Refresh compose file if plugin updated.
  if ! diff -q "${CLAUDE_PLUGIN_ROOT}/docker-compose.yml" "${COMPOSE_FILE}" >/dev/null 2>&1; then
    cp "${CLAUDE_PLUGIN_ROOT}/docker-compose.yml" "${COMPOSE_FILE}"
  fi

  ensure_env_written

  cd "${COMPOSE_DIR}"

  # 5. Validate compose config before touching the running container.
  if ! docker compose config --quiet 2>/dev/null; then
    echo "ERROR: docker compose config validation failed; refusing to continue cutover" >&2
    return 1
  fi

  # Ensure the external docker network exists — compose will fail without it.
  # Honour the DOCKER_NETWORK env var (same default the compose file uses).
  local network_name="${DOCKER_NETWORK:-hive-mcp}"
  if ! docker network inspect "${network_name}" >/dev/null 2>&1; then
    echo "hive-mcp: creating docker network ${network_name}"
    docker network create "${network_name}"
  fi

  # Source checkouts can build the image directly. Installed plugins normally
  # do not include the Rust source tree, so they pull the published image.
  if [[ "${CLAUDE_PLUGIN_OPTION_BUILD_LOCAL:-false}" == "true" && -f "${CLAUDE_PLUGIN_ROOT}/Cargo.toml" && -f "${CLAUDE_PLUGIN_ROOT}/config/Dockerfile" ]]; then
    (cd "${CLAUDE_PLUGIN_ROOT}" && docker compose build --no-cache hive-mcp)
  else
    if ! docker compose pull --quiet hive-mcp; then
      if [[ "${HIVE_MCP_ALLOW_CACHED_IMAGE:-false}" == "true" ]]; then
        echo "hive-mcp: pull failed; using cached image because HIVE_MCP_ALLOW_CACHED_IMAGE=true" >&2
      else
        echo "ERROR: failed to pull hive-mcp image; set HIVE_MCP_ALLOW_CACHED_IMAGE=true to use a cached image intentionally" >&2
        return 1
      fi
    fi
  fi

  if [[ "${external_named_container}" == "true" ]]; then
    if docker_container_running "hive-mcp"; then
      echo "hive-mcp: removing existing hive-mcp container before docker compose cutover"
      docker rm -f hive-mcp >/dev/null
    fi
    if docker_container_running "syslog-mcp"; then
      echo "hive-mcp: removing existing syslog-mcp container before docker compose cutover"
      docker rm -f syslog-mcp >/dev/null
    fi
  fi

  if docker compose ps --quiet hive-mcp 2>/dev/null | grep -q .; then
    docker compose up -d --force-recreate --no-build
  else
    docker compose up -d --no-build
  fi

  echo "hive-mcp: docker container running on ${MCP_HOST}:${MCP_PORT}"
}

validate_client() {
  if curl -sf "${SERVER_URL}/health" >/dev/null 2>&1; then
    echo "hive-mcp: connected to ${SERVER_URL}"
  else
    if [[ "${HIVE_MCP_ALLOW_UNREACHABLE_CLIENT:-false}" == "true" ]]; then
      echo "WARNING: hive-mcp server at ${SERVER_URL} is not reachable" >&2
    else
      echo "ERROR: hive-mcp server at ${SERVER_URL} is not reachable" >&2
      echo "       Set HIVE_MCP_ALLOW_UNREACHABLE_CLIENT=true only when intentionally configuring an offline client." >&2
      return 1
    fi
  fi
}

link_binary() {
  # Symlink the bundled binary into the user's PATH. ${CLAUDE_PLUGIN_ROOT}
  # changes on plugin update, so we re-link every SessionStart.
  mkdir -p "${HOME}/.local/bin"
  ln -sf "${CLAUDE_PLUGIN_ROOT}/bin/hive" "${HOME}/.local/bin/hive"
  if [[ -x "${CLAUDE_PLUGIN_ROOT}/bin/syslog" ]]; then
    ln -sf "${CLAUDE_PLUGIN_ROOT}/bin/syslog" "${HOME}/.local/bin/syslog"
  fi
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
