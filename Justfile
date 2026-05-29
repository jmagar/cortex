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
    cargo test

docker-build:
    docker build -t syslog-mcp .

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
    deployed_env="${HOME}/.syslog-mcp/.env"
    if [[ -f "${deployed_env}" ]]; then
        set -a; source "${deployed_env}"; set +a
    fi
    # SYSLOG_USE_HTTP must be unset so the local seed (ai add) uses SQLite directly
    SYSLOG_SMOKE_DB_PATH="${HOME}/.syslog-mcp/data/syslog.db" \
        env -u SYSLOG_USE_HTTP \
        bash tests/test_live.sh --mode http --url http://localhost:3100 --token "${SYSLOG_MCP_TOKEN:-${SYSLOG_API_TOKEN}}"

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
    for dir in plugins/syslog/skills/*; do
      [[ -d "$dir" ]] || continue
      found=1
      test -f "$dir/SKILL.md" || { echo "MISSING: $dir/SKILL.md"; exit 1; }
      grep -q '^name:' "$dir/SKILL.md" || { echo "MISSING name: $dir/SKILL.md"; exit 1; }
      grep -q '^description:' "$dir/SKILL.md" || { echo "MISSING description: $dir/SKILL.md"; exit 1; }
    done
    [[ "$found" -eq 1 ]] || { echo "MISSING: plugins/syslog/skills/*"; exit 1; }
    echo "OK"

validate-skills: validate-plugin

# Generate a standalone CLI for this server (requires running server; HTTP-only transport)
generate-cli:
    #!/usr/bin/env bash
    set -euo pipefail
    TOKEN="${SYSLOG_MCP_TOKEN:-}"
    if [[ -z "${TOKEN}" ]]; then
      echo "Set SYSLOG_MCP_TOKEN before running generate-cli"
      exit 1
    fi
    echo "⚠  Server must be running on port 3100 (run 'just dev' first)"
    echo "⚠  Generated CLI embeds your OAuth token — do not commit or share"
    mkdir -p dist dist/.cache
    current_hash=$(timeout 10 curl -sf \
      -H "Authorization: Bearer ${TOKEN}" \
      -H "Accept: application/json, text/event-stream" \
      http://localhost:3100/mcp/tools/list 2>/dev/null | sha256sum | cut -d' ' -f1 || echo "nohash")
    cache_file="dist/.cache/syslog-mcp-cli.schema_hash"
    if [[ -f "$cache_file" ]] && [[ "$(cat "$cache_file")" == "$current_hash" ]] && [[ -f "dist/syslog-mcp-cli" ]]; then
      echo "SKIP: syslog-mcp tool schema unchanged — use existing dist/syslog-mcp-cli"
      exit 0
    fi
    timeout 30 mcporter generate-cli \
      --command http://localhost:3100/mcp \
      --header "Authorization: Bearer ${TOKEN}" \
      --name syslog-mcp-cli \
      --output dist/syslog-mcp-cli
    printf '%s' "$current_hash" > "$cache_file"
    echo "✓ Generated dist/syslog-mcp-cli (requires bun at runtime)"

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
    cargo test
    cargo clippy -- -D warnings
    git add -A
    git commit -m "release: v${NEW}"
    git tag "v${NEW}"
    git push origin main --tags
    echo "Tagged v${NEW} — publish workflow will run automatically"
