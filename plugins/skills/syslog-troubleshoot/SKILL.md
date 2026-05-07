---
name: syslog-troubleshoot
description: This skill should be used when the user reports that syslog-mcp isn't working — connection failures ("/mcp can't reach syslog", "MCP tool returns error", "Failed to reconnect"), missing logs ("no logs from <host>", "host X stopped sending", "tail returns empty"), service problems ("syslog-mcp keeps crashing", "container unhealthy", "restart loop"), or vague reports ("syslog seems broken", "logs aren't working", "something's off with the log server"). Triggers on phrases like "troubleshoot syslog", "syslog isn't working", "no logs from X", "mcp connection failing", "syslog-mcp down", "container unhealthy", "why is syslog broken".
---

# Syslog Troubleshooting Skill

Diagnose syslog-mcp problems systematically. Use the binary's observability counters and existing diagnostic tooling rather than guessing — the codebase exposes most state needed to localize a failure.

## Decision tree — pick the right diagnostic

Match the user's report against one of these branches and follow only that branch. Don't run every check; that's what `/syslog:dr` is for and it overwhelms when the failure is narrow.

### Branch A — "MCP can't connect" / "Failed to reconnect" / "401 / 404 from /mcp"

Most common cause: empty / wrong `${user_config.server_url}`, mismatched `${user_config.api_token}`, or service not running.

1. **Is anything listening on the MCP port?**
   `ss -tlnp | grep -E ':${user_config.mcp_port}'` — if empty, the service is down → branch C
2. **Is the URL Claude Code is using sane?**
   Read `${HOME}/.claude/settings.json → pluginConfigs.syslog@jmagar-lab.options.server_url` — empty string is a known footgun (the `.mcp.json` substitution produces a literal `/mcp`). Check non-empty, has scheme, no trailing `/mcp`.
3. **Does the server require auth and the client send it?**
   `curl -sS -o /dev/null -w '%{http_code}' http://localhost:${user_config.mcp_port}/mcp` should return `401`. If `200`, server isn't enforcing auth (open access — flag this). If `404`, the route is wrong (different server is on that port). If connection refused, branch C.
4. **Token roundtrip**: `curl -sS -X POST -H "Authorization: Bearer <TOKEN>" -H "Content-Type: application/json, text/event-stream" -d '{"jsonrpc":"2.0","id":1,"method":"initialize",...}' http://localhost:${user_config.mcp_port}/mcp`. 401 = wrong token. 200 with valid response = server fine, problem is in Claude Code's MCP client config.

### Branch B — "No logs from <host>" / "host X stopped sending" / "missing entries"

1. **Does the host appear in the hosts list at all?**
   Call MCP tool: `syslog action=hosts`. If host is absent, no logs ever arrived → check forwarding config on `<host>`. If present with old `last_seen`, forwarding stopped → check rsyslog/forwarder on host.
2. **Is the listener actually accepting connections?**
   `ss -tlnp | grep ':${user_config.syslog_port}'` should show our process bound. From `<host>`: `nc -zv <our_host> ${user_config.syslog_port}` should connect.
3. **Recent forwarding errors on the host?**
   `ssh <host> "sudo journalctl -t rsyslogd -n 30 --no-pager"` — look for `omfwd` errors (DNS resolution, peer closed, EOF on TCP). Common patterns we've seen: stale forwarder pointing at a dead host, idle TCP timeout flapping, missing rsyslog drop-in.
4. **Drop-in present and correct?**
   `ssh <host> "cat /etc/rsyslog.d/99-syslog-mcp.conf 2>/dev/null"` should contain `*.* @@<our_host>:${user_config.syslog_port}` (TCP) — if missing or wrong, fix is `/syslog:deploy-dropins`.
5. **For Docker container logs**: if user expected logs from a container in `${user_config.fleet_hosts}` but doesn't see them, check `${user_config.docker_ingest_enabled}`. If false, ingest is off entirely. If true, verify the docker-socket-proxy on that host is reachable: `curl -sS http://<host>:2375/_ping` should return `OK`.

### Branch C — Service down / crashing / unhealthy

1. **Which mode are we in?**
   Read `${user_config.use_docker}` from settings.json.
2. **Get current state**:
   - Docker: `docker ps --filter name=syslog-mcp --format '{{.Status}}'`
   - Systemd: `systemctl --user status syslog-mcp.service`
3. **If recently restarted / crashing — get the actual error**: `/syslog:logs 100` (or do it manually if the command isn't loaded). Look for: panic messages, port-bind errors (`address already in use`), DB lock errors, OOM kills.
4. **Common service-failure causes (ranked by frequency in this plugin's history)**:
   1. Port `${user_config.syslog_port}` or `${user_config.mcp_port}` held by another process. Resolve with `sudo fuser -k <port>/tcp` or kill the offender.
   2. Mode cutover left both running — `docker compose down` and `systemctl --user stop syslog-mcp` both, then `/syslog:redeploy`.
   3. Database lock (another `syslog mcp` stdio process holds it). `pgrep -af "syslog mcp"` and kill stragglers.
   4. Docker image missing/stale: `docker compose pull` to refresh.
5. **If healthcheck failing but `/health` works manually**: Container is unhealthy because the healthcheck command inside the image is wrong/can't run. Compare image version to what you expect — `docker inspect syslog-mcp | jq '.[0].Config.Image'`.

### Branch D — "Something's off" / vague / user doesn't know

Run `/syslog:dr` (the comprehensive preflight + health check). Its PASS / WARN / FAIL output narrows the problem to a specific check. Then re-enter this skill on the failing check's category.

## Use observability counters

The binary exposes runtime counters via `syslog action=stats` and `/health`. Useful signals:

- `total_logs` not increasing → ingest pipeline is broken, not just MCP
- `write_blocked: true` → storage budget tripped, oldest logs being purged but can't keep up; check `${user_config.max_db_size_mb}` vs disk free
- `phantom_fts_rows` growing → retention purges aren't merging FTS5 cleanly; usually self-recovers
- `last_ingest_at` minutes-stale → forwarders aren't reaching us
- Newer counters in `RuntimeObservability` (since v0.13.0): UDP/TCP packets, ingest queue depth, writer flush failures — pull these via the `/health` endpoint or stats action and use to localize "ingest path" vs "writer path" failures

## Don't over-fix

- For a single-host symptom, don't restart the whole stack — just fix that host's forwarder.
- For an MCP-only failure with healthy ingest, don't touch the listener config.
- If the immediate problem is a missing config, prefer `/syslog:redeploy` over manual systemctl/docker commands.
- If you're going to change deploy mode as part of the fix, use `/syslog:cutover` not raw systemctl + docker compose.

## When to escalate to the user

- After a confident diagnosis, propose the fix and ask before applying it for anything destructive: changing settings.json, killing processes, deleting files, switching deploy modes.
- If checks return inconsistent state (e.g. listener says ours, but the binary says it isn't writing), surface the inconsistency rather than guessing.
- If the failure looks like an upstream bug (panic, deadlock, repeated crash on the same input), gather the journalctl/docker-logs output and stop — don't try multiple fix attempts on suspected source-code bugs.
