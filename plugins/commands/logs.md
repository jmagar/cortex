---
description: Tail or follow syslog-mcp service logs. Mode-aware (docker compose logs vs systemd journalctl). Default 50 lines; pass --follow to stream.
argument-hint: "[N|--follow]"
---

Show recent syslog-mcp service logs (the binary's own stdout/stderr, NOT the syslog database — use `syslog action=tail` for log entries received from clients).

## Step 1 — Determine deploy mode

Read `${user_config.use_docker}`:

- `true` → docker mode
- `false` → systemd mode

Also note `${user_config.is_server}`. If false, the user is in client mode and there's no local service to tail — politely explain and stop.

## Step 2 — Parse arguments

The user may pass: `$ARGUMENTS`

- Empty → tail last 50 lines, no follow
- A bare integer (e.g. `100`, `200`) → tail that many lines, no follow
- `--follow` or `-f` → tail last 50 lines and stream new ones live (the user must Ctrl-C to exit)
- Both (e.g. `200 --follow`) → tail that many lines, then stream

## Step 3 — Run the right command

**Docker mode** (`use_docker=true`):

- No follow: `docker compose -f ~/.claude/plugins/data/syslog-jmagar-lab/docker-compose.yml --project-directory ~/.claude/plugins/data/syslog-jmagar-lab logs syslog-mcp --tail <N> --no-color`
- Follow: same but with `-f` and no `--no-color`

If the plugin compose project has no container, check the source-repo deployment instead: `cd /home/jmagar/workspace/syslog-mcp && docker compose logs syslog-mcp --tail <N>` (note this in the output).

**Systemd mode** (`use_docker=false`):

- No follow: `journalctl --user -u syslog-mcp.service -n <N> --no-pager`
- Follow: `journalctl --user -u syslog-mcp.service -n <N> -f`

## Step 4 — Output

Show the captured log lines. If follow mode, the streaming terminal output is the deliverable — no further commentary needed unless the user Ctrl-Cs and asks for a summary.

If the service appears unhealthy from the logs (frequent errors, restart loops, panics), surface the pattern at the end and suggest `/syslog:dr` for full diagnostics.
