#!/usr/bin/env bash
# SessionStart / ConfigChange hook for syslog-mcp.
# Keep setup policy in the syslog binary; this script adapts plugin settings to env.
set -euo pipefail

: "${CLAUDE_PLUGIN_ROOT:=$(cd "$(dirname "$0")/.." && pwd)}"
: "${CLAUDE_PLUGIN_DATA:=${HOME}/.claude/plugins/data/syslog-jmagar-lab}"
: "${SYSLOG_DATA_DIR:=${CLAUDE_PLUGIN_DATA}}"

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

  local bundled="${CLAUDE_PLUGIN_ROOT}/bin/syslog"
  if [[ -x "${bundled}" ]]; then
    mkdir -p "${HOME}/.local/bin"
    ln -sf "${bundled}" "${HOME}/.local/bin/syslog"
    export PATH="${HOME}/.local/bin:${PATH}"
  fi

  command -v syslog >/dev/null 2>&1 || {
    printf 'syslog plugin setup: syslog binary not found on PATH or at %s\n' "${bundled}" >&2
    exit 1
  }
}

main() {
  reject_unsafe_value "CLAUDE_PLUGIN_OPTION_API_TOKEN" "${CLAUDE_PLUGIN_OPTION_API_TOKEN:-}"
  export_if_set SYSLOG_MCP_TOKEN CLAUDE_PLUGIN_OPTION_API_TOKEN
  export_if_set SYSLOG_SERVER_URL CLAUDE_PLUGIN_OPTION_SERVER_URL
  export_if_set SYSLOG_MCP_AUTH_MODE CLAUDE_PLUGIN_OPTION_AUTH_MODE
  export_if_set SYSLOG_MCP_PUBLIC_URL CLAUDE_PLUGIN_OPTION_PUBLIC_URL
  export_if_set SYSLOG_MCP_GOOGLE_CLIENT_ID CLAUDE_PLUGIN_OPTION_GOOGLE_CLIENT_ID
  export_if_set SYSLOG_MCP_GOOGLE_CLIENT_SECRET CLAUDE_PLUGIN_OPTION_GOOGLE_CLIENT_SECRET
  export_if_set SYSLOG_MCP_AUTH_ADMIN_EMAIL CLAUDE_PLUGIN_OPTION_AUTH_ADMIN_EMAIL
  export_if_set SYSLOG_HOST CLAUDE_PLUGIN_OPTION_SYSLOG_HOST
  export_if_set SYSLOG_PORT CLAUDE_PLUGIN_OPTION_SYSLOG_PORT
  export_if_set SYSLOG_HOST_PORT CLAUDE_PLUGIN_OPTION_SYSLOG_HOST_PORT
  export_if_set SYSLOG_MCP_HOST CLAUDE_PLUGIN_OPTION_MCP_HOST
  export_if_set SYSLOG_MCP_PORT CLAUDE_PLUGIN_OPTION_MCP_PORT
  export_if_set SYSLOG_DOCKER_INGEST_ENABLED CLAUDE_PLUGIN_OPTION_DOCKER_INGEST_ENABLED

  mkdir -p "${SYSLOG_DATA_DIR}"
  chmod 700 "${SYSLOG_DATA_DIR}" 2>/dev/null || true
  export SYSLOG_DATA_DIR

  ensure_syslog_binary
  syslog setup plugin-hook "$@"
}

main "$@"
