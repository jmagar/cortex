#!/usr/bin/env bash
# Claude Code hook. Server-mode setup delegates to the shared host installer
# path: ~/.local/bin/syslog + `syslog setup repair` + ~/.syslog-mcp.
set -euo pipefail

: "${CLAUDE_PLUGIN_ROOT:=$(cd "$(dirname "$0")/.." && pwd)}"
INSTALL_URL="${SYSLOG_INSTALL_URL:-https://raw.githubusercontent.com/jmagar/syslog-mcp/main/install.sh}"
INSTALL_SHA256="${SYSLOG_INSTALL_SHA256:-}"

reject_unsafe_value() {
  local name="$1" value="${2:-}"
  if [[ "${value}" == *$'\n'* || "${value}" == *$'\r'* ]]; then
    printf 'syslog plugin setup: %s must not contain newlines\n' "${name}" >&2
    exit 2
  fi
}

export_if_set() {
  local env_name="$1" option_name="$2" value
  value="$(printenv "${option_name}" || true)"
  reject_unsafe_value "${option_name}" "${value}"
  [[ -n "${value}" ]] || return 0
  export "${env_name}=${value}"
}

ensure_syslog_binary() {
  if command -v syslog >/dev/null 2>&1; then
    return 0
  fi
  local tmp
  tmp="$(mktemp)"
  trap 'rm -f "${tmp}"' RETURN
  printf 'syslog plugin setup: syslog not found; running installer %s\n' "${INSTALL_URL}" >&2
  curl -fsSL --connect-timeout 5 --max-time 120 -o "${tmp}" "${INSTALL_URL}"
  if [[ -n "${INSTALL_SHA256}" ]]; then
    printf '%s  %s\n' "${INSTALL_SHA256}" "${tmp}" | sha256sum -c -
  elif [[ "${SYSLOG_INSTALL_ALLOW_UNVERIFIED:-false}" != "true" ]]; then
    printf 'syslog plugin setup: refusing to run unverified installer; set SYSLOG_INSTALL_SHA256 or SYSLOG_INSTALL_ALLOW_UNVERIFIED=true\n' >&2
    exit 1
  fi
  sh "${tmp}"
  rm -f "${tmp}"
  trap - RETURN
  export PATH="${HOME}/.local/bin:${PATH}"
  command -v syslog >/dev/null 2>&1 || {
    printf 'syslog plugin setup: installer completed but syslog is still not on PATH\n' >&2
    exit 1
  }
}

validate_client() {
  local server_url
  server_url="$(strip_trailing_mcp_path "${CLAUDE_PLUGIN_OPTION_SERVER_URL:-http://localhost:3100}")"
  if curl -fsS --connect-timeout 2 --max-time 5 "${server_url%/}/health" >/dev/null 2>&1; then
    echo "syslog-mcp: connected to ${server_url%/}"
  else
    echo "WARNING: syslog-mcp server at ${server_url%/} is not reachable" >&2
  fi
}

strip_trailing_mcp_path() {
  local url="${1%/}"
  if [[ "${url}" == */mcp ]]; then
    url="${url%/mcp}"
  fi
  printf '%s\n' "${url}"
}

append_csv_unique() {
  local csv="$1" value="$2" item
  local -a items
  [[ -n "${value}" ]] || { printf '%s\n' "${csv}"; return; }
  local IFS=','
  read -r -a items <<< "${csv}"
  for item in "${items[@]}"; do
    item="${item#"${item%%[![:space:]]*}"}"
    item="${item%"${item##*[![:space:]]}"}"
    [[ "${item}" != "${value}" ]] || { printf '%s\n' "${csv}"; return; }
  done
  if [[ -n "${csv}" ]]; then
    printf '%s,%s\n' "${csv}" "${value}"
  else
    printf '%s\n' "${value}"
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

prepare_oauth_env() {
  local auth_mode="${SYSLOG_MCP_AUTH_MODE:-bearer}"
  local server_url="${CLAUDE_PLUGIN_OPTION_SERVER_URL:-http://localhost:3100}"
  local redirects="${SYSLOG_MCP_AUTH_ALLOWED_REDIRECT_URIS:-}"
  local codex_callback

  [[ "${auth_mode}" == "oauth" ]] || return 0
  if [[ -z "${SYSLOG_MCP_PUBLIC_URL:-}" && "${server_url}" == https://* ]]; then
    export SYSLOG_MCP_PUBLIC_URL
    SYSLOG_MCP_PUBLIC_URL="$(strip_trailing_mcp_path "${server_url}")"
  fi

  redirects="$(append_csv_unique "${redirects}" "https://claude.ai/api/mcp/auth_callback")"
  redirects="$(append_csv_unique "${redirects}" "https://claudeai.ai/api/mcp/auth_callback")"
  codex_callback="$(codex_oauth_callback_url)"
  if [[ -n "${codex_callback}" ]]; then
    redirects="$(append_csv_unique "${redirects}" "${codex_callback}")"
  fi
  export SYSLOG_MCP_AUTH_ALLOWED_REDIRECT_URIS="${redirects}"
  export SYSLOG_MCP_AUTH_DISABLE_STATIC_TOKEN_WITH_OAUTH="${SYSLOG_MCP_AUTH_DISABLE_STATIC_TOKEN_WITH_OAUTH:-false}"
}

main() {
  local is_server="${CLAUDE_PLUGIN_OPTION_IS_SERVER:-true}"

  reject_unsafe_value "CLAUDE_PLUGIN_OPTION_API_TOKEN" "${CLAUDE_PLUGIN_OPTION_API_TOKEN:-}"
  export_if_set SYSLOG_MCP_TOKEN CLAUDE_PLUGIN_OPTION_API_TOKEN
  export_if_set SYSLOG_HOST CLAUDE_PLUGIN_OPTION_SYSLOG_HOST
  export_if_set SYSLOG_PORT CLAUDE_PLUGIN_OPTION_SYSLOG_PORT
  export_if_set SYSLOG_HOST_PORT CLAUDE_PLUGIN_OPTION_SYSLOG_HOST_PORT
  export_if_set SYSLOG_MCP_HOST CLAUDE_PLUGIN_OPTION_MCP_HOST
  export_if_set SYSLOG_MCP_PORT CLAUDE_PLUGIN_OPTION_MCP_PORT
  export_if_set SYSLOG_MCP_MAX_DB_SIZE_MB CLAUDE_PLUGIN_OPTION_MAX_DB_SIZE_MB
  export_if_set SYSLOG_MCP_DATA_VOLUME CLAUDE_PLUGIN_OPTION_DATA_DIR
  export_if_set SYSLOG_MCP_RETENTION_DAYS CLAUDE_PLUGIN_OPTION_RETENTION_DAYS
  export_if_set SYSLOG_BATCH_SIZE CLAUDE_PLUGIN_OPTION_BATCH_SIZE
  export_if_set SYSLOG_WRITE_CHANNEL_CAPACITY CLAUDE_PLUGIN_OPTION_WRITE_CHANNEL_CAPACITY
  export_if_set SYSLOG_DOCKER_INGEST_ENABLED CLAUDE_PLUGIN_OPTION_DOCKER_INGEST_ENABLED
  export_if_set SYSLOG_DOCKER_HOSTS CLAUDE_PLUGIN_OPTION_FLEET_HOSTS
  export_if_set NO_AUTH CLAUDE_PLUGIN_OPTION_NO_AUTH
  export_if_set SYSLOG_MCP_AUTH_MODE CLAUDE_PLUGIN_OPTION_AUTH_MODE
  export_if_set SYSLOG_MCP_PUBLIC_URL CLAUDE_PLUGIN_OPTION_PUBLIC_URL
  export_if_set SYSLOG_MCP_GOOGLE_CLIENT_ID CLAUDE_PLUGIN_OPTION_GOOGLE_CLIENT_ID
  export_if_set SYSLOG_MCP_GOOGLE_CLIENT_SECRET CLAUDE_PLUGIN_OPTION_GOOGLE_CLIENT_SECRET
  export_if_set SYSLOG_MCP_AUTH_ADMIN_EMAIL CLAUDE_PLUGIN_OPTION_AUTH_ADMIN_EMAIL
  export_if_set SYSLOG_MCP_AUTH_ALLOWED_REDIRECT_URIS CLAUDE_PLUGIN_OPTION_AUTH_ALLOWED_REDIRECT_URIS
  prepare_oauth_env

  if [[ "${is_server}" != "true" ]]; then
    validate_client
    return 0
  fi

  ensure_syslog_binary
  syslog setup repair
}

main "$@"
