---
name: syslog-redeploy
description: Re-run the syslog-mcp plugin setup hook with the current userConfig and verify the deployment. Use when the user asks to redeploy syslog-mcp, apply plugin config changes immediately, rerun the setup hook, refresh Docker/systemd deployment, or recover after an automated SessionStart/ConfigChange hook did not run.
---

# Syslog Redeploy

Run the plugin setup hook directly, then verify the selected deployment mode is healthy.

## Before Running

Explain that the hook will re-render the plugin env file from current userConfig. In Docker mode it may pull the configured image and recreate the container. In systemd mode it may update and restart the user unit. If the saved deploy mode differs from the running mode, the hook stops the old mode before starting the new one, causing a brief `/health` gap.

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
   - Docker mode: `docker ps --filter name=syslog-mcp --format '{{.Status}}'`; expect `Up ... (healthy)` or briefly `(starting)` just after recreate. Wait up to 60 seconds before treating `(starting)` as a failure.
   - Systemd mode: `systemctl --user is-active syslog-mcp.service`; expect `active`.

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
- Container or unit state line
- Runtime freshness verdict (`CURRENT`, `STALE`, or `FAIL`)

If anything failed, recommend `syslog-dr` for the comprehensive diagnostic.
