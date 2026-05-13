---
name: syslog-redeploy
description: Re-run the syslog-mcp plugin setup hook with the current userConfig and verify the Docker Compose deployment. Use when the user asks to redeploy syslog-mcp, apply plugin config changes immediately, rerun the setup hook, refresh the Docker deployment, or recover after an automated SessionStart/ConfigChange hook did not run.
---

# Syslog Redeploy

Run the plugin setup hook directly, then verify the selected deployment mode is healthy.

## Before Running

Explain that the hook exports current userConfig into environment variables, then delegates to `syslog setup repair`. The shared setup path repairs `~/.syslog-mcp/.env`, writes Compose assets under `~/.syslog-mcp/compose`, recreates the Docker Compose container when needed, and removes stale user-level `syslog-mcp.service` units/drop-ins left by older plugin versions. Recreating the container causes a brief `/health` gap.

If the user asked for a dry run only, stop after describing what would happen.

## Workflow

1. Run:

   ```bash
   ${CLAUDE_PLUGIN_ROOT}/scripts/plugin-setup.sh
   ```

   Capture stdout, stderr, and exit code.

2. Verify HTTP health:

   ```bash
   curl -sS -m 3 -o /dev/null -w '%{http_code}' "$CLAUDE_PLUGIN_OPTION_SERVER_URL/health"
   ```

   Expect `200`.

3. Verify the active runtime:
   - `docker ps --filter name=syslog-mcp --format '{{.Status}}'`; expect `Up ... (healthy)` or briefly `(starting)` just after recreate. Wait up to 60 seconds before treating `(starting)` as a failure.

4. Verify runtime freshness:

   ```bash
   ${CLAUDE_PLUGIN_ROOT}/scripts/check-runtime-current.sh
   ```

   Expect `CURRENT`. Treat `STALE` or `FAIL` as a failed redeploy verification and include the printed `fix:` line when present.

## Report

Include:
- Hook exit code
- The `syslog-mcp: ...` summary line printed by the hook
- Health check result
- Container state line
- Runtime freshness verdict (`CURRENT`, `STALE`, or `FAIL`)

If anything failed, recommend `syslog-dr` for the comprehensive diagnostic.
