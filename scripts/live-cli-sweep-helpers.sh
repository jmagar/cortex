#!/usr/bin/env bash
set -euo pipefail

json_value() {
  local json="$1"
  local expression="$2"
  python3 -c "import json,sys; value=${expression}; print('' if value is None else str(value).lower() if isinstance(value, bool) else value)" <<<"$json"
}

preflight() {
  local root="$1"
  local server="$2"
  local cortex_bin="${CORTEX_SWEEP_CORTEX_BIN:-cortex}"
  local runtime_check="${CORTEX_SWEEP_RUNTIME_CHECK:-$root/scripts/check-runtime-current.sh}"
  local host_version api_json api_version runtime_output repo_version container_version

  [[ -n "${CORTEX_API_TOKEN:-}" ]] || {
    echo "FAIL: CORTEX_API_TOKEN is required to verify $server/api/version" >&2
    return 1
  }

  host_version="$($cortex_bin --version | awk '{print $2}')"
  [[ -n "$host_version" ]] || {
    echo "FAIL: could not determine host cortex binary version" >&2
    return 1
  }

  api_json="$(curl -fsS -H "Authorization: Bearer $CORTEX_API_TOKEN" "${server%/}/api/version")"
  api_version="$(json_value "$api_json" 'json.load(sys.stdin).get("version")')"
  [[ -n "$api_version" ]] || {
    echo "FAIL: /api/version did not return a version" >&2
    return 1
  }

  if ! runtime_output="$($runtime_check --mode docker 2>&1)"; then
    printf '%s\n' "$runtime_output" >&2
    return 1
  fi
  printf '%s\n' "$runtime_output"
  repo_version="$(awk '$1 == "repo_version" {print $2; exit}' <<<"$runtime_output")"
  container_version="$(awk '$1 == "container_version" {print $2; exit}' <<<"$runtime_output")"
  [[ -n "$repo_version" && -n "$container_version" ]] || {
    echo "FAIL: runtime image check did not report repo and container versions" >&2
    return 1
  }

  if [[ "$host_version" != "$repo_version" ]]; then
    echo "FAIL: host cortex version $host_version does not match repo version $repo_version; install the built host binary before sweeping" >&2
    return 1
  fi
  if [[ "$api_version" != "$host_version" ]]; then
    echo "FAIL: API version $api_version does not match host version $host_version; refresh the Compose container before sweeping" >&2
    return 1
  fi
  if [[ "$container_version" != "$host_version" ]]; then
    echo "FAIL: container version $container_version does not match host version $host_version; refresh both deployment surfaces" >&2
    return 1
  fi

  echo "runtime parity ok: host=$host_version api=$api_version container=$container_version"
}

wait_integrity() {
  local started_json="$1"
  local timeout_seconds="${2:-90}"
  local interval_seconds="${3:-2}"
  local cortex_bin="${CORTEX_SWEEP_CORTEX_BIN:-cortex}"
  local job_id deadline status_json command_status status ok error

  job_id="$(json_value "$started_json" 'json.load(sys.stdin).get("job_id")')"
  [[ "$job_id" =~ ^[0-9]+$ ]] || {
    echo "FAIL: background integrity start did not return a numeric job_id: $started_json" >&2
    return 1
  }
  deadline=$((SECONDS + timeout_seconds))

  while true; do
    set +e
    status_json="$($cortex_bin db integrity status "$job_id" --json)"
    command_status=$?
    set -e
    status="$(json_value "$status_json" 'json.load(sys.stdin).get("status")')"
    ok="$(json_value "$status_json" '(json.load(sys.stdin).get("integrity") or {}).get("ok")')"
    error="$(json_value "$status_json" 'json.load(sys.stdin).get("error")')"

    case "$status" in
      done)
        if [[ "$command_status" -eq 0 && "$ok" == "true" ]]; then
          printf '%s\n' "$status_json"
          return 0
        fi
        echo "FAIL: integrity job $job_id completed without an ok integrity result: $status_json" >&2
        return 1
        ;;
      failed)
        echo "FAIL: integrity job $job_id failed: ${error:-unknown error}" >&2
        return 1
        ;;
      running)
        if (( SECONDS >= deadline )); then
          echo "FAIL: timed out waiting for integrity job $job_id after ${timeout_seconds}s" >&2
          return 1
        fi
        sleep "$interval_seconds"
        ;;
      *)
        echo "FAIL: integrity job $job_id returned unexpected status '${status:-missing}': $status_json" >&2
        return 1
        ;;
    esac
  done
}

case "${1:-}" in
  preflight)
    shift
    preflight "$@"
    ;;
  wait-integrity)
    shift
    wait_integrity "$@"
    ;;
  *)
    echo "usage: $0 preflight ROOT SERVER | wait-integrity START_JSON [TIMEOUT_SECONDS] [INTERVAL_SECONDS]" >&2
    exit 2
    ;;
esac
