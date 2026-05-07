---
name: syslog-dr
description: Run a comprehensive syslog-mcp health check covering environment, config quality, storage, ports, service status, HTTP health, MCP actions, listener reachability, Docker ingest, and fleet rsyslog forwarding. Use when the user asks for syslog doctor, deployment diagnostics, first-run preflight, health check, sanity check, or broad deployment verification.
---

# Syslog Doctor

Run a full PASS / WARN / FAIL diagnostic for syslog-mcp. Use this when the user needs broad deployment confidence rather than a narrow log query.

## Workflow

1. Display resolved plugin config, excluding sensitive token values:
   - server/client mode and Docker/systemd mode
   - server URL
   - syslog bind and MCP bind
   - data dir
   - retention and storage limits
   - Docker ingest status and fleet hosts

2. Gather host context:
   - `uname -srm`
   - `systemd-detect-virt`

3. Check prerequisites for server mode:
   - Systemd mode: `command -v systemctl`, `systemctl --user is-system-running`, `command -v journalctl`
   - Docker mode: `command -v docker`, `docker version --format {{.Server.Version}}`, `docker compose version`

4. Check storage and permissions for `$CLAUDE_PLUGIN_OPTION_DATA_DIR`:
   - directory exists
   - writable by the service user
   - free space is at least `max_db_size_mb * 1.2`, or at least 2048 MB when max DB size is disabled

5. Check the binary symlink:
   - `${HOME}/.local/bin/syslog` exists
   - it is a symlink
   - `readlink -f` resolves inside `${CLAUDE_PLUGIN_ROOT}`
   - `${HOME}/.local/bin` is on `$PATH`

6. Check API token quality without printing the value:
   - empty is FAIL
   - length under 24 characters is WARN
   - known weak placeholders are FAIL: `your-secret-token`, `changeme`, `test`, `placeholder`, `secret`, `token`, `password`, `abc123`, `default`
   - otherwise PASS

7. Check ports:
   - syslog TCP and UDP port
   - MCP TCP port
   - PASS only when the port is free or held by the expected syslog-mcp process/container

8. Check service state:
   - Docker: inspect compose state and health, and include `docker compose logs syslog-mcp --tail 30 --no-color` when not running.
   - Systemd: inspect `systemctl --user is-active syslog-mcp` and `is-failed`, and include `journalctl --user -u syslog-mcp -n 30 --no-pager` when inactive or failed.

9. Check HTTP and MCP:
   - `curl -sS -m 3 "$CLAUDE_PLUGIN_OPTION_SERVER_URL/health"`; expect status ok.
   - Use the syslog MCP tool for read-only `stats`, `hosts`, and `tail n=1`; report totals, host count, write_blocked, DB size, free disk, and most recent timestamp. If the MCP tool fails, record the error and continue with HTTP-only evidence.

10. Check runtime freshness in server mode:
    - Run `${CLAUDE_PLUGIN_ROOT}/scripts/check-runtime-current.sh`.
    - PASS only when the final verdict starts with `CURRENT:`.
    - FAIL on `STALE:` or `FAIL:` and include the printed `fix:` line when present.
    - Systemd mode compares the running `/proc/<pid>/exe` hash to the unit `ExecStart` binary hash.
    - Docker mode compares the running container image ID to the local compose image ID. This is a local-cache check; use `syslog-version-check --pull` when the user specifically wants to pull first and detect a stale registry tag.

11. Check listener reachability:
    - Parse host from `$CLAUDE_PLUGIN_OPTION_SERVER_URL`.
    - TCP syslog: `nc -z -w2 <host> "$CLAUDE_PLUGIN_OPTION_SYSLOG_PORT"`
    - MCP auth: `curl -sS -o /dev/null -w "%{http_code}" -m 3 "$CLAUDE_PLUGIN_OPTION_SERVER_URL/mcp"`; expect `401`.

12. Check optional Docker ingest hosts:
    - For each host in `$CLAUDE_PLUGIN_OPTION_FLEET_HOSTS`, `curl -sf -m 3 http://<host>:2375/_ping`; expect `OK`.

13. Check fleet rsyslog forwarding in server mode when `$CLAUDE_PLUGIN_OPTION_FLEET_HOSTS` is non-empty:
    - SSH reachability
    - drop-in contains the expected target and port
    - rsyslog is active
    - host appears in MCP `hosts` output with `last_seen` within 30 minutes (low-volume hosts that send infrequently may legitimately fail a stricter threshold — use WARN rather than FAIL when last_seen is between 30 minutes and retention window)

## Report

Produce one results table:

| Check | Status | Detail |
|---|---|---|
| Env: kernel | INFO | value |
| Storage: writable | PASS/WARN/FAIL | detail |
| Service: state | PASS/WARN/FAIL | detail |
| Runtime: freshness | PASS/FAIL | CURRENT or stale/fail detail |
| MCP: stats | PASS/FAIL | totals or error |
| Fleet host | PASS/FAIL | detail |

If service-state checks failed, include a `## Service Logs` section with the captured logs.

End with one verdict:
- `All checks passed`
- `Ready with warnings`
- `<N> checks failed`

For each warning or failure, include a concrete next step. Add a footer that deep functional validation is `bash scripts/smoke-test.sh`.
