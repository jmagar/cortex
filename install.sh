#!/usr/bin/env sh
set -eu

CRATE="${SYSLOG_INSTALL_CRATE:-syslog-mcp}"
VERSION="${SYSLOG_VERSION:-latest}"
PREFIX="${SYSLOG_INSTALL_PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
BIN="$BIN_DIR/syslog"
DRY_RUN="${SYSLOG_INSTALL_DRY_RUN:-0}"
SKIP_SETUP="${SYSLOG_INSTALL_SKIP_SETUP:-0}"

say() {
  printf '%s\n' "$*" >&2
}

fail() {
  say "syslog install: $*"
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail "$1 is required"
}

check_prereqs() {
  need cargo
}

check_setup_prereqs() {
  need docker
  docker compose version >/dev/null 2>&1 || fail "docker compose is required"
}

install_binary() {
  mkdir -p "$BIN_DIR"
  if [ "${SYSLOG_INSTALL_FROM_PATH:-}" ]; then
    cargo install --locked --path "$SYSLOG_INSTALL_FROM_PATH" --root "$PREFIX" --force
  elif [ "$VERSION" = "latest" ]; then
    cargo install --locked "$CRATE" --root "$PREFIX" --force
  else
    cargo install --locked "$CRATE" --version "$VERSION" --root "$PREFIX" --force
  fi
  [ -x "$BIN" ] || fail "cargo install completed but $BIN is missing"
  say "Installed $BIN"
}

main() {
  check_prereqs
  if [ "$DRY_RUN" = "1" ]; then
    say "Dry run OK: prefix=$PREFIX crate=$CRATE version=$VERSION"
    exit 0
  fi
  if [ "$SKIP_SETUP" != "1" ]; then
    check_setup_prereqs
  fi
  install_binary
  if [ "$SKIP_SETUP" != "1" ]; then
    "$BIN" setup
  fi
}

main "$@"
