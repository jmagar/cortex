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
image="${SYSLOG_TEST_IMAGE:-syslog-mcp:local-debug}"
case "$*" in
  "ps --filter name=^/syslog-mcp$ --format {{.ID}}"*) echo cid ;;
  "inspect cid --format {{ index .Config.Labels \"com.docker.compose.project.working_dir\" }}"*) echo /tmp/legacy-compose ;;
  "inspect cid --format {{.Config.Image}}"*) echo "$image" ;;
  "inspect cid --format {{.Image}}"*) echo sha256:debug ;;
  "image inspect syslog-mcp:local-debug --format {{.Id}}"*) echo sha256:debug ;;
  "image inspect syslog-mcp:local-debug --format {{join .RepoDigests \", \"}}"*) echo "" ;;
  "exec cid syslog --version"*) awk -F'"' '/^version = / {print "syslog-mcp " $2; exit}' "${PWD}/Cargo.toml" ;;
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
[[ "${status}" -eq 0 ]] || fail "local-debug image exit=${status}, want 0; output=${out}"
[[ "${out}" == *"CURRENT:"* ]] || fail "local-debug image current message missing"

set +e
out="$(SYSLOG_TEST_IMAGE=custom/syslog:dev PATH="${tmpdir}:${PATH}" "${CHECKER}" --mode docker --allow-legacy 2>&1)"
status=$?
set -e
[[ "${status}" -eq 1 ]] || fail "custom local image exit=${status}, want 1"
[[ "${out}" == *"unsupported image"* ]] || fail "custom local image message missing"

echo "check-runtime-current.sh argument tests passed"
