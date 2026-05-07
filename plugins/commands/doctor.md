---
description: Check syslog-mcp health — config, MCP, service, ports, Docker ingest, and recent service logs on failure
---

Run a full health check of the syslog-mcp deployment and report a clear PASS/FAIL summary.

## Step 1 — Display resolved config

Print the user's plugin configuration so they can verify it's what they expect. The plugin substitutes these values into this command at invocation time (the API token is sensitive and intentionally omitted):

| Setting | Value |
|---------|-------|
| Mode | server=`${user_config.is_server}`, docker=`${user_config.use_docker}` |
| Server URL | `${user_config.server_url}` |
| Syslog bind | `${user_config.syslog_host}:${user_config.syslog_port}` |
| MCP bind | `${user_config.mcp_host}:${user_config.mcp_port}` |
| Data dir | `${user_config.data_dir}` |
| Retention | `${user_config.retention_days}` days |
| Max DB size | `${user_config.max_db_size_mb}` MB |
| Docker ingest | enabled=`${user_config.docker_ingest_enabled}`, hosts=`${user_config.docker_hosts}` |

## Step 2 — Run health checks

### 2.1 MCP connectivity (broad)
Call the `syslog` MCP tool with multiple read-only actions to verify the MCP layer is healthy end-to-end:
- `action: stats` → DB stats (verifies DB access)
- `action: hosts` → host list (verifies queries work)
- `action: tail` with `n: 1` → most recent log (verifies log retrieval)

Report PASS only if all three succeed without `isError`. Surface stats summary (total logs, total hosts, write_blocked, logical DB size) inline.

### 2.2 HTTP health endpoint
GET `${user_config.server_url}/health`. PASS if 200 with `{"status":"ok"}`.

### 2.3 Service status (server mode only)
If `${user_config.is_server}` is true:

**Docker mode** (`${user_config.use_docker}` is true):
- Run `docker compose ps --format json` from `${CLAUDE_PLUGIN_DATA}` and check the syslog-mcp container is `running`
- **If the container is NOT running**, also tail the last 30 lines: `docker compose logs syslog-mcp --tail 30 --no-color`

**Systemd mode** (`${user_config.use_docker}` is false):
- Run `systemctl --user is-active syslog-mcp` and report active/inactive
- **If inactive or failed**, also tail the journal: `journalctl --user -u syslog-mcp -n 30 --no-pager`

### 2.4 Syslog port
Test TCP connectivity to the syslog port: `nc -z -w2 <host-from-server-url> ${user_config.syslog_port}`. The host comes from parsing `${user_config.server_url}`.

### 2.5 Docker ingest hosts (when enabled)
If `${user_config.docker_ingest_enabled}` is true, for each host in `${user_config.fleet_hosts}` (comma-separated):
- `curl -sf -m 3 http://<host>:2375/version`
- Report each as reachable or unreachable, with the response status if reachable

### 2.6 Fleet rsyslog drop-ins (server mode only)
If `${user_config.is_server}` is true and `${user_config.fleet_hosts}` is non-empty, verify each fleet host is forwarding correctly. Parse the host portion from `${user_config.server_url}` — call it `FORWARD_TARGET`.

For each host in `${user_config.fleet_hosts}`:
1. **SSH reachability**: `ssh -o BatchMode=yes -o ConnectTimeout=5 <host> true`. Skip the host with a single FAIL row if SSH fails.
2. **Drop-in present**: `ssh <host> "cat /etc/rsyslog.d/99-syslog-mcp.conf 2>/dev/null"`. Report PASS only if it contains `FORWARD_TARGET:${user_config.syslog_port}`. Report FAIL with the actual file content (or "missing") if not.
3. **rsyslog active**: `ssh <host> "systemctl is-active rsyslog"`. PASS on `active`.
4. **Log flow**: cross-check against the `hosts` action output from check 2.1. If `<host>` appears in the hosts list with a recent `last_seen`, logs are actually flowing → PASS. If the drop-in is present but the host never appears in `hosts`, that's a network/firewall problem worth surfacing.

A host that fails check 2 with "missing" → the fix is `/syslog:deploy-dropins`. Mention this explicitly in the failure detail.

## Step 3 — Output format

Print a single results table:

| Check | Status | Detail |
|-------|--------|--------|
| MCP — stats | ✓ PASS / ✗ FAIL | totals or error |
| MCP — hosts | ✓ PASS / ✗ FAIL | host count or error |
| MCP — tail | ✓ PASS / ✗ FAIL | most recent timestamp or error |
| HTTP /health | ✓ PASS / ✗ FAIL | response body |
| Service running | ✓ PASS / ✗ FAIL | active/inactive or container state |
| Syslog port | ✓ PASS / ✗ FAIL | host:port reachable |
| Docker host: <name> | ✓ PASS / ✗ FAIL | reachable or error |
| Fleet <name>: SSH | ✓ PASS / ✗ FAIL | reachable or error |
| Fleet <name>: drop-in | ✓ PASS / ✗ FAIL | matches expected / missing → run `/syslog:deploy-dropins` |
| Fleet <name>: rsyslog | ✓ PASS / ✗ FAIL | active or inactive |
| Fleet <name>: log flow | ✓ PASS / ✗ FAIL | last_seen recent or no logs received |

Then any failure-mode log output (journalctl / docker compose logs) in a `## Service logs` section.

End with:
- **All checks passed** or **N check(s) failed** with concrete next steps for failures
- A note: *For deep functional validation (seeds test data and verifies all actions work correctly), run `bash scripts/smoke-test.sh`.*
