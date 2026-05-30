# `set dotenv-load` is GLOBAL (bead i5lx): every recipe below runs with the
# variables from `.env` injected into its environment — including
# CORTEX_API_TOKEN, NO_AUTH, and any OAuth secrets. Two consequences to keep in
# mind:
#   1. A local `.env` override silently changes what a recipe tests vs. CI.
#   2. Test recipes that must exercise the no-auth path explicitly strip the
#      auth vars (`env -u CORTEX_API_TOKEN -u NO_AUTH ...`); any new test or
#      release recipe that runs the suite MUST do the same, or it will test a
#      different environment than `just test`.
set dotenv-load

dev:
    cargo run -- serve mcp

build:
    cargo build

release:
    cargo build --release

check:
    cargo check
    bash scripts/check-rust-module-size.sh --limit 500 src/cli.rs src/cli

lint:
    cargo clippy -- -D warnings

fmt:
    cargo fmt

test:
    env -u CORTEX_API_TOKEN -u NO_AUTH cargo nextest run

# Doc tests (nextest does not run these; no executable doc tests currently exist)
test-doc:
    cargo test --doc

docker-build:
    docker build -t cortex .

up:
    docker compose up -d

down:
    docker compose down

restart:
    docker compose restart

logs:
    docker compose logs -f

health:
    curl -sf http://localhost:3100/health | jq .

test-live:
    #!/usr/bin/env bash
    set -euo pipefail
    # Load the deployed env file so tokens are current
    deployed_env="${HOME}/.cortex/.env"
    if [[ -f "${deployed_env}" ]]; then
        set -a; source "${deployed_env}"; set +a
    fi
    # CORTEX_USE_HTTP must be unset so the local seed (ai add) uses SQLite directly
    CORTEX_SMOKE_DB_PATH="${HOME}/.cortex/data/cortex.db" \
        env -u CORTEX_USE_HTTP \
        bash tests/test_live.sh --mode http --url http://localhost:3100 \
            ${CORTEX_TOKEN:+--token "${CORTEX_TOKEN}"}

setup:
    cp -n .env.example .env || true

gen-token:
    openssl rand -hex 32

# Validate plugin manifests, MCP config, hooks, and skill frontmatter
validate-plugin:
    #!/usr/bin/env bash
    set -euo pipefail
    python3 - <<'PY'
    import json
    from pathlib import Path

    plugin = json.loads(Path(".claude-plugin/plugin.json").read_text())
    if "version" in plugin:
        raise SystemExit("FORBIDDEN: .claude-plugin/plugin.json version")
    for key in ["mcpServers", "hooks", "skills"]:
        value = plugin.get(key)
        if not value:
            raise SystemExit(f"MISSING: .claude-plugin/plugin.json {key}")
        path = Path(value)
        if not path.exists():
            raise SystemExit(f"MISSING: {path}")

    mcp_path = Path(plugin["mcpServers"])
    mcp = json.loads(mcp_path.read_text())
    if "syslog" not in mcp.get("mcpServers", {}):
        raise SystemExit(f"MISSING: syslog server in {mcp_path}")

    hooks_path = Path(plugin["hooks"])
    hooks = json.loads(hooks_path.read_text()).get("hooks", {})
    for event in ["SessionStart", "ConfigChange"]:
        entries = hooks.get(event)
        if not entries:
            raise SystemExit(f"MISSING: {event} hook in {hooks_path}")
        for entry in entries:
            for hook in entry.get("hooks", []):
                command = hook.get("command", "")
                if command.startswith("${CLAUDE_PLUGIN_ROOT}/"):
                    command_path = Path(command.removeprefix("${CLAUDE_PLUGIN_ROOT}/"))
                    if not command_path.exists():
                        raise SystemExit(f"MISSING: hook command {command_path}")
    PY
    found=0
    for dir in plugins/cortex/skills/*; do
      [[ -d "$dir" ]] || continue
      found=1
      test -f "$dir/SKILL.md" || { echo "MISSING: $dir/SKILL.md"; exit 1; }
      grep -q '^name:' "$dir/SKILL.md" || { echo "MISSING name: $dir/SKILL.md"; exit 1; }
      grep -q '^description:' "$dir/SKILL.md" || { echo "MISSING description: $dir/SKILL.md"; exit 1; }
    done
    [[ "$found" -eq 1 ]] || { echo "MISSING: plugins/cortex/skills/*"; exit 1; }
    echo "OK"

validate-skills: validate-plugin

# Generate a standalone CLI for this server (requires running server; HTTP-only transport)
generate-cli:
    #!/usr/bin/env bash
    set -euo pipefail
    TOKEN="${CORTEX_TOKEN:-}"
    if [[ -z "${TOKEN}" ]]; then
      echo "Set CORTEX_TOKEN before running generate-cli"
      exit 1
    fi
    echo "⚠  Server must be running on port 3100 (run 'just dev' first)"
    echo "⚠  Generated CLI embeds your OAuth token — do not commit or share"
    mkdir -p dist dist/.cache
    current_hash=$(timeout 10 curl -sf \
      -H "Authorization: Bearer ${TOKEN}" \
      -H "Accept: application/json, text/event-stream" \
      http://localhost:3100/mcp/tools/list 2>/dev/null | sha256sum | cut -d' ' -f1 || echo "nohash")
    cache_file="dist/.cache/cortex-cli.schema_hash"
    if [[ -f "$cache_file" ]] && [[ "$(cat "$cache_file")" == "$current_hash" ]] && [[ -f "dist/cortex-cli" ]]; then
      echo "SKIP: cortex tool schema unchanged — use existing dist/cortex-cli"
      exit 0
    fi
    timeout 30 mcporter generate-cli \
      --command http://localhost:3100/mcp \
      --header "Authorization: Bearer ${TOKEN}" \
      --name cortex-cli \
      --output dist/cortex-cli
    printf '%s' "$current_hash" > "$cache_file"
    echo "✓ Generated dist/cortex-cli (requires bun at runtime)"

clean:
    cargo clean
    rm -rf .cache/

# Linux only — Windows would need .exe binaries; requires git lfs install
build-plugin: release
    #!/bin/sh
    set -eu
    target_dir="${CARGO_TARGET_DIR:-target}"
    if [ ! -x "$target_dir/release/syslog" ] && [ -x ".cache/cargo/release/syslog" ]; then
      target_dir=".cache/cargo"
    fi
    install -m 755 "$target_dir/release/syslog" bin/syslog

build-mcpb:
    bash scripts/build-mcpb.sh

runtime-current:
    bash scripts/check-runtime-current.sh

# Publish: bump version, tag, push (triggers crates.io + Docker publish)
publish bump="patch":
    #!/usr/bin/env bash
    set -euo pipefail
    [ "$(git branch --show-current)" = "main" ] || { echo "Switch to main first"; exit 1; }
    [ -z "$(git status --porcelain)" ] || { echo "Commit or stash changes first"; exit 1; }
    git pull origin main
    case "{{bump}}" in
      major|minor|patch) ;;
      *) echo "Usage: just publish [major|minor|patch]"; exit 1 ;;
    esac
    scripts/bump-version.sh "{{bump}}"
    NEW=$(grep -m1 "^version" Cargo.toml | sed "s/.*\"\(.*\)\".*/\1/")
    scripts/check-version-sync.sh --require-changelog
    # Reuse the canonical gates (bead ok8c) so the release gate can't drift from
    # them: `test` strips the auth vars `set dotenv-load` injects and runs
    # nextest, `test-doc` covers doc tests (nextest skips those), `lint` is
    # clippy -D warnings.
    just test
    just test-doc
    just lint
    git add -A
    git commit -m "release: v${NEW}"
    git tag "v${NEW}"
    git push origin main --tags
    echo "Tagged v${NEW} — publish workflow will run automatically"
