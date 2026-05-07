---
description: Re-run the syslog-mcp deployment hook with the current userConfig. Apply config changes immediately without waiting for SessionStart or a ConfigChange event.
---

Run the plugin's setup hook directly and report the result. This is what `SessionStart` and `ConfigChange` run automatically — useful when iterating on the plugin or when an automated trigger didn't fire.

## Step 1 — Confirm intent

This will:

- Re-render `~/.claude/plugins/data/syslog-jmagar-lab/syslog-mcp.env` from the current userConfig
- In `use_docker=true` mode: pull `ghcr.io/jmagar/syslog-mcp:${SYSLOG_MCP_VERSION:-latest}` and `docker compose up -d --force-recreate --no-build`
- In `use_docker=false` mode: ensure the `syslog-mcp.service` user unit is up to date and restart it on env or unit change

If a deploy-mode cutover is implied by the current config (running deployment differs from `use_docker`), the hook will stop the old mode before starting the new one. Brief gap in `/health` reachability expected during cutover.

If the user asked for a dry-run only, stop here and report what would happen.

## Step 2 — Run the hook

Execute:

```bash
${CLAUDE_PLUGIN_ROOT}/scripts/plugin-setup.sh
```

Capture stdout + stderr + exit code.

## Step 3 — Verify

After the hook returns:

- `curl -sS -m 3 -o /dev/null -w '%{http_code}' http://localhost:${user_config.mcp_port:-3100}/health` — expect `200`
- For docker mode: `docker ps --filter name=syslog-mcp --format '{{.Status}}'` — expect `Up ... (healthy)` (may say `(starting)` for ~10s after a fresh start)
- For systemd mode: `systemctl --user is-active syslog-mcp.service` — expect `active`

## Step 4 — Output

Report:

- Hook exit code (0 / non-zero)
- The `syslog-mcp: ...` summary line the hook prints
- Health check result
- Container or unit state line

If anything failed, suggest `/syslog:dr` for the comprehensive diagnostic.
