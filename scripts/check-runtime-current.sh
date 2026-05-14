#!/usr/bin/env bash
# Check whether the running syslog-mcp Docker Compose container is using the
# current local compose image and binary version.
set -euo pipefail

MODE="auto"
PULL="false"
SERVICE="syslog-mcp"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
DEFAULT_COMPOSE_DIR="${SYSLOG_MCP_HOME:-${HOME}/.syslog-mcp}/compose"
COMPOSE_DIR="${SYSLOG_MCP_COMPOSE_DIR:-$DEFAULT_COMPOSE_DIR}"
ALLOW_LEGACY="false"
ALLOW_LOCAL_IMAGE="false"

usage() {
  cat <<'EOF'
Usage: scripts/check-runtime-current.sh [--mode auto|docker] [--pull] [--compose-dir DIR] [--allow-legacy] [--allow-local-image]

Checks:
  docker:  running container image ID == local compose image ID and
           container `syslog --version` == repo Cargo.toml version

Options:
  --pull                  Docker only: pull compose image before comparing.
                          Without this, Docker mode only proves the container
                          matches the image already present in the local cache.
  --compose-dir DIR       Docker compose project dir (default: ~/.syslog-mcp/compose)
  --allow-legacy          Permit a running container from a non-canonical
                          Compose working directory.
  --allow-local-image     Permit non-ghcr.io/jmagar/syslog-mcp images such as
                          syslog-mcp:local-debug.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      MODE="${2:?--mode requires a value}"
      case "$MODE" in
        auto|docker) ;;
        *)
          echo "invalid mode: $MODE" >&2
          exit 2
          ;;
      esac
      shift 2
      ;;
    --pull)
      PULL="true"
      shift
      ;;
    --compose-dir)
      COMPOSE_DIR="${2:?--compose-dir requires a value}"
      shift 2
      ;;
    --allow-legacy)
      ALLOW_LEGACY="true"
      shift
      ;;
    --allow-local-image)
      ALLOW_LOCAL_IMAGE="true"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

status_line() {
  printf '%-10s %s\n' "$1" "$2"
}

detect_mode() {
  if command -v docker >/dev/null 2>&1; then
    if [[ -d "$COMPOSE_DIR" ]] && (cd "$COMPOSE_DIR" && docker compose ps -q "$SERVICE" 2>/dev/null | grep -q .); then
      echo docker
      return
    fi
    if docker ps --filter "name=^/${SERVICE}$" --format '{{.ID}}' 2>/dev/null | grep -q .; then
      echo docker
      return
    fi
  fi
  echo none
}

compose_image() {
  if [[ -d "$COMPOSE_DIR" ]]; then
    (cd "$COMPOSE_DIR" && docker compose config --images 2>/dev/null | head -1) || true
  fi
}

realpath_or_echo() {
  if command -v realpath >/dev/null 2>&1; then
    realpath -m "$1"
  else
    printf '%s\n' "$1"
  fi
}

check_docker() {
  local cid running_image image local_image repo_digests repo_version container_version label_compose_dir
  local canonical_compose_dir canonical_default_dir
  status_line mode docker

  if [[ -d "$COMPOSE_DIR" ]]; then
    cid="$(cd "$COMPOSE_DIR" && docker compose ps -q "$SERVICE" 2>/dev/null || true)"
  else
    cid=""
  fi
  if [[ -z "$cid" ]]; then
    cid="$(docker ps --filter "name=^/${SERVICE}$" --format '{{.ID}}' 2>/dev/null | head -1)"
  fi
  if [[ -z "$cid" ]]; then
    echo "FAIL: syslog-mcp container is not running"
    return 1
  fi
  label_compose_dir="$(docker inspect "$cid" --format '{{ index .Config.Labels "com.docker.compose.project.working_dir" }}' 2>/dev/null || true)"
  if [[ -n "$label_compose_dir" && "$label_compose_dir" != "<no value>" ]]; then
    COMPOSE_DIR="$label_compose_dir"
  fi
  status_line compose_dir "$COMPOSE_DIR"
  canonical_compose_dir="$(realpath_or_echo "$COMPOSE_DIR")"
  canonical_default_dir="$(realpath_or_echo "$DEFAULT_COMPOSE_DIR")"
  if [[ "$ALLOW_LEGACY" != "true" && "$canonical_compose_dir" != "$canonical_default_dir" ]]; then
    echo "FAIL: running container belongs to non-canonical Compose dir: $COMPOSE_DIR"
    echo "fix: migrate to $DEFAULT_COMPOSE_DIR or rerun with --allow-legacy for an intentional local/debug deployment"
    return 1
  fi

  image="$(compose_image)"
  [[ -n "$image" ]] || image="$(docker inspect "$cid" --format '{{.Config.Image}}')"
  if [[ "$ALLOW_LOCAL_IMAGE" != "true" && "$image" != ghcr.io/jmagar/syslog-mcp:* ]]; then
    echo "FAIL: running container uses unsupported image: $image"
    echo "fix: use ghcr.io/jmagar/syslog-mcp:<version> or rerun with --allow-local-image for an intentional local/debug deployment"
    return 1
  fi

  if [[ "$PULL" == "true" && -d "$COMPOSE_DIR" ]]; then
    (cd "$COMPOSE_DIR" && docker compose pull --quiet "$SERVICE")
  fi

  running_image="$(docker inspect "$cid" --format '{{.Image}}')"
  local_image="$(docker image inspect "$image" --format '{{.Id}}' 2>/dev/null || true)"
  repo_digests="$(docker image inspect "$image" --format '{{join .RepoDigests ", "}}' 2>/dev/null || true)"
  repo_version="$(awk -F'"' '/^version = / {print $2; exit}' "${REPO_DIR}/Cargo.toml" 2>/dev/null || true)"
  if [[ -z "$repo_version" ]]; then
    echo "FAIL: could not determine repo version from ${REPO_DIR}/Cargo.toml"
    return 1
  fi
  container_version="$(docker exec "$cid" syslog --version 2>/dev/null | awk '{print $2}' || true)"

  status_line container "$cid"
  status_line image "$image"
  status_line running_image_id "$running_image"
  status_line local_image_id "${local_image:-missing}"
  [[ -n "$repo_digests" ]] && status_line repo_digests "$repo_digests"
  [[ -n "$repo_version" ]] && status_line repo_version "$repo_version"
  [[ -n "$container_version" ]] && status_line container_version "$container_version"

  if [[ -z "$local_image" ]]; then
    echo "FAIL: compose image is not present locally"
    echo "fix: cd $COMPOSE_DIR && docker compose pull $SERVICE"
    return 1
  fi
  if [[ "$running_image" != "$local_image" ]]; then
    echo "STALE: running container image differs from local compose image"
    echo "fix: cd $COMPOSE_DIR && docker compose up -d --force-recreate --no-build $SERVICE"
    return 1
  fi
  if [[ -n "$repo_version" && "$container_version" != "$repo_version" ]]; then
    echo "STALE: container syslog version does not match repo version"
    echo "fix: rebuild and restart the Compose service from this repo"
    return 1
  fi

  echo "CURRENT: running container matches local compose image and repo version"
}

if [[ "$MODE" == "auto" ]]; then
  MODE="$(detect_mode)"
fi

case "$MODE" in
  docker) check_docker ;;
  none)
    echo "FAIL: no running syslog-mcp Docker container detected"
    exit 1
    ;;
  *)
    echo "invalid mode: $MODE" >&2
    exit 2
    ;;
esac
