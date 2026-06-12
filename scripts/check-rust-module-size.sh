#!/usr/bin/env bash
set -euo pipefail

limit=500
self_test=0
allowlist_file="scripts/rust-module-size.allow"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --limit)
      limit="${2:?--limit requires a value}"
      shift 2
      ;;
    --self-test)
      self_test=1
      shift
      ;;
    -h|--help)
      cat <<'USAGE'
Usage: scripts/check-rust-module-size.sh [--limit N] [--self-test] [PATH ...]

Reports non-test Rust production files with more than N non-comment/doc lines.
Blank lines, line comments, doc comments, and block comments are ignored.
When PATH values are provided, only those files/directories are checked.
USAGE
      exit 0
      ;;
    --)
      shift
      break
      ;;
    -*)
      echo "unknown option: $1" >&2
      exit 2
      ;;
    *)
      break
      ;;
  esac
done

count_file() {
  perl -0ne '
    s{/\*.*?\*/}{}gs;
    my $count = 0;
    for my $line (split /\n/) {
      $line =~ s/^\s+//;
      $line =~ s/\s+$//;
      next if $line eq "";
      next if $line =~ m{^//};
      $count++;
    }
    print "$count\n";
  ' "$1"
}

is_prod_rust_file() {
  local file="$1"
  [[ "$file" == *.rs ]] || return 1
  [[ "$file" != *_tests.rs ]] || return 1
  [[ "$file" != *test.rs ]] || return 1
  [[ "$file" != tests/* ]] || return 1
  [[ "$file" != */tests/* ]] || return 1
  return 0
}

run_self_test() {
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN

  cat >"$tmp/sample.rs" <<'RUST'
// ignored
/// ignored
//! ignored

fn one() {}
/*
fn ignored_block() {}
*/
fn two() {
    let _x = 1; // counted
}
RUST

  local count
  count="$(count_file "$tmp/sample.rs")"
  if [[ "$count" != "4" ]]; then
    echo "self-test failed: expected 4 counted lines, got $count" >&2
    return 1
  fi

  if is_prod_rust_file "src/foo_tests.rs"; then
    echo "self-test failed: *_tests.rs should be excluded" >&2
    return 1
  fi
  if is_prod_rust_file "tests/integration.rs"; then
    echo "self-test failed: tests/ files should be excluded" >&2
    return 1
  fi
  if ! is_prod_rust_file "src/foo.rs"; then
    echo "self-test failed: src/foo.rs should be included" >&2
    return 1
  fi
  echo "self-test ok"
}

if [[ "$self_test" -eq 1 ]]; then
  run_self_test
  exit
fi

tracked_files() {
  if [[ $# -eq 0 ]]; then
    git ls-files --cached --others --exclude-standard '*.rs'
    return
  fi

  local path
  for path in "$@"; do
    if [[ -f "$path" ]]; then
      git ls-files --cached --others --exclude-standard --error-unmatch "$path" 2>/dev/null || true
    elif [[ -d "$path" ]]; then
      git ls-files --cached --others --exclude-standard "$path/**/*.rs" "$path/*.rs" 2>/dev/null || true
    else
      echo "path not found: $path" >&2
      return 2
    fi
  done | sort -u
}

status=0
is_allowlisted() {
  local file="$1"
  [[ -f "$allowlist_file" ]] || return 1
  grep -Fxq -- "$file" "$allowlist_file"
}

while IFS= read -r file; do
  is_prod_rust_file "$file" || continue
  is_allowlisted "$file" && continue
  count="$(count_file "$file")"
  if (( count > limit )); then
    printf '%s\t%s\n' "$count" "$file"
    status=1
  fi
done < <(tracked_files "$@")

exit "$status"
