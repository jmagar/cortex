#!/usr/bin/env bash
# Lightweight checks for check-runtime-current.sh argument handling. Runtime
# Docker behavior is verified live by the command itself.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="${SCRIPT_DIR}/check-runtime-current.sh"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

out="$("${CHECKER}" --help)"
[[ "${out}" == *"--mode auto|docker"* ]] || fail "help omits mode usage"
[[ "${out}" == *"local cache"* ]] || fail "help omits Docker cache semantics"
[[ "${out}" == *"--allow-legacy"* ]] || fail "help omits legacy escape hatch"
[[ "${out}" == *"--allow-local-image"* ]] || fail "help omits local image escape hatch"

set +e
out="$("${CHECKER}" --bogus 2>&1)"
status=$?
set -e
[[ "${status}" -eq 2 ]] || fail "unknown argument exit=${status}, want 2"
[[ "${out}" == *"unknown argument: --bogus"* ]] || fail "unknown argument message missing"

set +e
out="$("${CHECKER}" --mode nope 2>&1)"
status=$?
set -e
[[ "${status}" -eq 2 ]] || fail "invalid mode exit=${status}, want 2"
[[ "${out}" == *"invalid mode: nope"* ]] || fail "invalid mode message missing"

tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir}"' EXIT
cat >"${tmpdir}/docker" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case "$*" in
  "ps --filter name=^/syslog-mcp$ --format {{.ID}}"*) echo cid ;;
  "inspect cid --format {{ index .Config.Labels \"com.docker.compose.project.working_dir\" }}"*) echo /tmp/legacy-compose ;;
  "inspect cid --format {{.Config.Image}}"*) echo syslog-mcp:local-debug ;;
  *) echo "unexpected docker args: $*" >&2; exit 9 ;;
esac
SH
chmod +x "${tmpdir}/docker"

set +e
out="$(PATH="${tmpdir}:${PATH}" "${CHECKER}" --mode docker 2>&1)"
status=$?
set -e
[[ "${status}" -eq 1 ]] || fail "legacy compose dir exit=${status}, want 1"
[[ "${out}" == *"non-canonical Compose dir"* ]] || fail "legacy compose dir message missing"

set +e
out="$(PATH="${tmpdir}:${PATH}" "${CHECKER}" --mode docker --allow-legacy 2>&1)"
status=$?
set -e
[[ "${status}" -eq 1 ]] || fail "local image exit=${status}, want 1"
[[ "${out}" == *"unsupported image"* ]] || fail "local image message missing"

echo "check-runtime-current.sh argument tests passed"
