---
name: syslog-logs
description: Tail or follow syslog-mcp service logs from Docker Compose. Use when the user asks for syslog-mcp service logs, startup logs, crash logs, plugin deployment logs, Docker logs, or follow mode. This is for the service's stdout/stderr, not client syslog entries.
---

# Syslog Service Logs

Show recent syslog-mcp service logs. These are the binary's stdout/stderr, not syslog entries received from clients. Use the `syslog` MCP tool with `action=tail` for received log entries.

## Workflow

1. Check server mode:

   ```bash
   echo "$CLAUDE_PLUGIN_OPTION_IS_SERVER"
   ```

   If `false`, explain that the local plugin is in client mode and has no local service logs to tail.

2. Parse user arguments:
   - empty: last 50 lines, no follow
   - bare integer such as `100`: that many lines, no follow
   - `--follow` or `-f`: last 50 lines, then stream
   - integer plus follow flag: that many lines, then stream

3. Run Docker Compose logs:

   ```bash
   docker compose -f "${CLAUDE_PLUGIN_DATA}/docker-compose.yml" --project-directory "${CLAUDE_PLUGIN_DATA}" logs syslog-mcp --tail <N> --no-color
   ```

   For follow mode, add `-f` and omit `--no-color`.

   If the plugin compose project has no container, report that Docker mode is
   configured but no plugin-managed container is running. Do not guess a source
   checkout path.

## Output

Show the captured logs directly. If follow mode is active, the streaming output is the deliverable until the user interrupts.

If the logs show obvious unhealthy patterns such as repeated restarts, panics, bind errors, or DB lock errors, summarize the pattern after the log excerpt and suggest `syslog-dr` for full diagnostics.
