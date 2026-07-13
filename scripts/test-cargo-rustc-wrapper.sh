#!/usr/bin/env bash
set -euo pipefail

repo="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

export HOME="$tmp/home"
export CORTEX_RUSTC_WRAPPER_LOCAL_BIN="$HOME/.local/bin/cortex"
export CARGO_BIN_ARTIFACT_WRAPPER_NO_SCCACHE=1
export CORTEX_RUSTC_WRAPPER_NO_SCCACHE=1
mkdir -p "$HOME/.local/bin" "$tmp/target/debug/deps" "$tmp/target/release/deps"

# The plugin bundle path must NEVER be written by the wrapper — it is owned by
# `just build-plugin`. Seed a sentinel and assert it survives every invocation.
plugin_bin="$tmp/plugin/cortex"
mkdir -p "$tmp/plugin"
printf 'plugin sentinel\n' >"$plugin_bin"

fake_rustc="$tmp/fake-rustc"
cat >"$fake_rustc" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
out=""
crate=""
out_dir=""
extra=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --crate-name)
      crate="$2"
      shift 2
      ;;
    -o)
      out="$2"
      shift 2
      ;;
    --out-dir)
      out_dir="$2"
      shift 2
      ;;
    -C)
      case "${2:-}" in
        extra-filename=*) extra="${2#extra-filename=}" ;;
      esac
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
if [ -z "$out" ] && [ -n "$crate" ] && [ -n "$out_dir" ] && [ -n "$extra" ]; then
  out="$out_dir/$crate$extra"
fi
if [ -n "$out" ]; then
  mkdir -p "$(dirname "$out")"
  printf 'fake cortex binary\n' >"$out"
  chmod +x "$out"
fi
SH
chmod +x "$fake_rustc"

assert_plugin_untouched() {
  [ "$(cat "$plugin_bin")" = "plugin sentinel" ] || {
    echo "FAIL: wrapper wrote to the plugin bundle path" >&2
    exit 1
  }
}

# 1. Debug builds must NOT deploy.
"$repo/scripts/cargo-rustc-wrapper" "$fake_rustc" \
  --crate-name cortex \
  --crate-type bin \
  src/main.rs \
  -o "$tmp/target/debug/deps/cortex-123"
test ! -e "$HOME/.local/bin/cortex"
assert_plugin_untouched

# 2. Test builds (--test) must NOT deploy, even from the release dir.
"$repo/scripts/cargo-rustc-wrapper" "$fake_rustc" \
  --crate-name cortex \
  --crate-type bin \
  --test \
  src/main.rs \
  -o "$tmp/target/release/deps/cortex-124"
test ! -e "$HOME/.local/bin/cortex"
assert_plugin_untouched

# 3. Release builds deploy to local bin (relative -o path, cwd-resolved).
(
  cd "$tmp"
  "$repo/scripts/cargo-rustc-wrapper" "$fake_rustc" \
    --crate-name cortex \
    --crate-type bin \
    src/main.rs \
    -o target/release/deps/cortex-456
)
cmp "$tmp/target/release/deps/cortex-456" "$HOME/.local/bin/cortex"
assert_plugin_untouched

# 4. --out-dir + extra-filename reconstruction works; debug still excluded.
rm -f "$HOME/.local/bin/cortex"
"$repo/scripts/cargo-rustc-wrapper" "$fake_rustc" \
  --crate-name cortex \
  --crate-type bin \
  src/main.rs \
  --out-dir "$tmp/target/debug/deps" \
  -C extra-filename=-789
test ! -e "$HOME/.local/bin/cortex"

# 5. --out-dir + extra-filename reconstruction deploys for release.
"$repo/scripts/cargo-rustc-wrapper" "$fake_rustc" \
  --crate-name cortex \
  --crate-type bin \
  src/main.rs \
  --out-dir "$tmp/target/release/deps" \
  -C extra-filename=-790
cmp "$tmp/target/release/deps/cortex-790" "$HOME/.local/bin/cortex"
assert_plugin_untouched

# 6. A `just link-bin` style symlink is REPLACED (mv semantics), never written
#    through — the symlink target must keep its original content.
link_target="$tmp/target/release/cortex-linked"
printf 'cargo release artifact\n' >"$link_target"
ln -sf "$link_target" "$HOME/.local/bin/cortex"
"$repo/scripts/cargo-rustc-wrapper" "$fake_rustc" \
  --crate-name cortex \
  --crate-type bin \
  src/main.rs \
  -o "$tmp/target/release/deps/cortex-791"
test ! -L "$HOME/.local/bin/cortex"
cmp "$tmp/target/release/deps/cortex-791" "$HOME/.local/bin/cortex"
[ "$(cat "$link_target")" = "cargo release artifact" ] || {
  echo "FAIL: wrapper wrote through the symlink into the build artifact" >&2
  exit 1
}

# 7. Non-bin crate types (e.g. clippy/check rmeta) must not deploy.
rm -f "$HOME/.local/bin/cortex"
"$repo/scripts/cargo-rustc-wrapper" "$fake_rustc" \
  --crate-name cortex \
  --crate-type lib \
  src/lib.rs \
  -o "$tmp/target/release/deps/cortex-792"
test ! -e "$HOME/.local/bin/cortex"

# 8. An install failure must not fail the compilation (read-only bin dir).
chmod 555 "$HOME/.local/bin"
"$repo/scripts/cargo-rustc-wrapper" "$fake_rustc" \
  --crate-name cortex \
  --crate-type bin \
  src/main.rs \
  -o "$tmp/target/release/deps/cortex-793" 2>/dev/null
chmod 755 "$HOME/.local/bin"
test -e "$tmp/target/release/deps/cortex-793"

echo "cargo rustc wrapper install behavior ok"
