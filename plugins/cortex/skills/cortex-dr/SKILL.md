---
name: cortex-dr
description: Run a comprehensive cortex health check covering environment, config quality, storage, ports, service status, HTTP health, MCP actions, listener reachability, Docker ingest, and fleet rsyslog forwarding. Use when the user asks for syslog doctor, deployment diagnostics, first-run preflight, health check, sanity check, or broad deployment verification.
---

# Syslog Doctor

Run a full PASS / WARN / FAIL diagnostic for cortex. Use this when the user needs broad deployment confidence rather than a narrow log query.

## Workflow

1. Display resolved plugin config, excluding sensitive token values:
   - server/client mode
   - server URL
   - syslog bind and MCP bind
   - data dir
   - retention and storage limits
   - Docker ingest status and fleet hosts

2. Gather host context:
   - `uname -srm`
   - `systemd-detect-virt`

3. Check prerequisites for server mode:
   - `command -v docker`, `docker version --format {{.Server.Version}}`, `docker compose version`

4. Check storage and permissions for `$CLAUDE_PLUGIN_OPTION_DATA_DIR`:
   - directory exists
   - writable by the service user
   - free space is at least `max_db_size_mb * 1.2`, or at least 2048 MB when max DB size is disabled

5. Check auth mode and token quality without printing token values:
   - If `$CLAUDE_PLUGIN_OPTION_NO_AUTH` is true, record `WARN (auth disabled by plugin config)` and do not fail an empty token.
   - If OAuth mode is configured, verify OAuth config presence and skip bearer-token strength checks unless a bearer fallback token is also configured.
   - In bearer mode with auth enabled, empty token is FAIL.
   - In bearer mode with auth enabled, length under 24 characters is WARN.
   - Known weak placeholders are FAIL: `your-secret-token`, `changeme`, `test`, `placeholder`, `secret`, `token`, `password`, `abc123`, `default`.
   - Otherwise PASS.

6. Check ports:
   - syslog TCP and UDP port
   - MCP TCP port
   - PASS only when the port is free or held by the expected cortex process/container

7. Check service state:
   - Inspect Compose state and container health, and include `docker compose logs cortex --tail 30 --no-color` when not running.

8. Check HTTP and MCP:
   - `curl -sS -m 3 "$CLAUDE_PLUGIN_OPTION_SERVER_URL/health"`; expect status ok.
   - Use the cortex MCP tool for read-only `stats`, `hosts`, and `tail n=1`; report totals, host count, write_blocked, DB size, free disk, and most recent timestamp. If the MCP tool fails, record the error and continue with HTTP-only evidence.

9. Check runtime freshness in server mode:
    - Run `${CLAUDE_PLUGIN_ROOT}/scripts/check-runtime-current.sh`.
    - PASS only when the final verdict starts with `CURRENT:`.
    - FAIL on `STALE:` or `FAIL:` and include the printed `fix:` line when present.
    - The checker compares the running container image ID to the local Compose image ID. This is a local-cache check; use `cortex-version-check --pull` when the user specifically wants to pull first and detect a stale registry tag.

10. Check listener reachability:
    - Parse host from `$CLAUDE_PLUGIN_OPTION_SERVER_URL`.
    - TCP syslog: `nc -z -w2 <host> "${CLAUDE_PLUGIN_OPTION_SYSLOG_HOST_PORT:-$CLAUDE_PLUGIN_OPTION_SYSLOG_PORT}"`
    - MCP auth: `curl -sS -o /dev/null -w "%{http_code}" -m 3 "$CLAUDE_PLUGIN_OPTION_SERVER_URL/mcp"`.
      - If auth is disabled (`NO_AUTH=true` or no bearer/OAuth auth configured), `200` or MCP protocol-level `400/405` is acceptable as route evidence; do not flag open access as a failure because it matches configuration.
      - If bearer or OAuth auth is enabled, expect `401` for an unauthenticated request.
      - `404` means the route is wrong or another service owns the port.

11. Check optional Docker ingest hosts:
    - For each host in `$CLAUDE_PLUGIN_OPTION_FLEET_HOSTS`, `curl -sf -m 3 http://<host>:2375/_ping`; expect `OK`.

12. Check fleet rsyslog forwarding in server mode when `$CLAUDE_PLUGIN_OPTION_FLEET_HOSTS` is non-empty:
    - SSH reachability
    - drop-in contains the expected target and externally reachable port
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
