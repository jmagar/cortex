---
description: Full health check — environment, config quality, storage, ports, service, MCP, listener, fleet hosts. Run this after configuring the plugin to verify the deployment is sane.
---

Run a comprehensive health check of the syslog-mcp deployment and report a clear PASS / WARN / FAIL summary. This command doubles as a **first-run preflight** (immediately after setting userConfig) and an **ongoing health check** — failing checks surface concrete next-step fixes either way.

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
| Docker ingest | enabled=`${user_config.docker_ingest_enabled}`, hosts=`${user_config.fleet_hosts}` |

## Step 2 — Run health checks

Run these checks in order. Capture results into the final report table. WARN ≠ FAIL — warnings let the user know about non-blocking issues (e.g. weak token) without claiming the deployment is broken.

### 2.1 Environment prerequisites

Gather host context (informational, always reported):
- `uname -srm` — kernel name + release + arch
- `systemd-detect-virt` — bare metal / wsl / docker / kvm / etc.
- For client mode (`${user_config.is_server}` is false): skip the rest of this section.

**If `${user_config.is_server}` is true:**

**Systemd mode** (`${user_config.use_docker}` is false):
- `command -v systemctl` exists → PASS, else FAIL ("systemd not found — pick `use_docker=true` or deploy on a systemd host")
- `systemctl --user is-system-running` returns anything other than `offline` → PASS, else FAIL ("user systemd manager not running — `loginctl enable-linger ${USER}` may be needed")
- `command -v journalctl` exists → PASS (informational; needed for service-log tail on failure)

**Docker mode** (`${user_config.use_docker}` is true):
- `command -v docker` exists → PASS, else FAIL ("docker not installed")
- `docker version --format {{.Server.Version}}` succeeds → PASS, else FAIL ("docker daemon unreachable — is dockerd running and is `${USER}` in the `docker` group?")
- `docker compose version` succeeds → PASS, else FAIL ("compose v2 plugin not installed")

### 2.2 Storage & permissions (server mode only)

Skip if `${user_config.is_server}` is false.

For `${user_config.data_dir}`:
- Directory exists → PASS, else FAIL ("data_dir does not exist — the plugin setup hook should create it; rerun the SessionStart hook or check perms on its parent")
- Writable by the user that owns the service process: `test -w "${user_config.data_dir}"` → PASS, else FAIL with `ls -ld` output
- Free space ≥ `${user_config.max_db_size_mb}` × 1.2 (parse `df -BM --output=avail` for the directory's filesystem) → PASS, else WARN ("free space N MB is below 120% of max_db_size_mb; storage guard may block writes after a burst")
- If `max_db_size_mb` is 0, instead check free space ≥ 2048 MB → WARN below that threshold

### 2.3 Binary symlink

The setup hook symlinks the bundled binary into `~/.local/bin/syslog`. Verify:
- `${HOME}/.local/bin/syslog` exists → PASS, else FAIL ("symlink missing — rerun the SessionStart hook")
- It's a symlink (`test -L`) → PASS, else WARN ("file is a regular file, not a symlink; will not auto-update on plugin upgrade")
- The symlink target resolves: `readlink -f` succeeds and points inside `${CLAUDE_PLUGIN_ROOT}` → PASS, else FAIL with the broken target path
- `~/.local/bin` is on `$PATH`: `case ":$PATH:" in *":${HOME}/.local/bin:"*) yes ;; *) no ;; esac` → PASS or WARN ("symlink exists but $PATH does not include ~/.local/bin — `syslog` CLI won't work without an absolute path")

### 2.4 API token quality

`${user_config.api_token}` is sensitive — DO NOT print the value. Run all checks against the substituted value internally, then report only the verdict.

- **Empty / unset** → FAIL ("API token is empty; clients will receive 401")
- **Length < 24 characters** → WARN ("token is only N chars; recommend ≥ 32. Generate with `just gen-token` or `openssl rand -hex 32`")
- **Matches a known-weak value** (case-insensitive, exact match): `password`, `changeme`, `test`, `admin`, `secret`, `123456`, `default`, `letmein`, `token` → FAIL ("token is a known-weak placeholder; replace with a randomly generated value")
- Otherwise → PASS

### 2.5 Port availability (server mode only)

Skip if `${user_config.is_server}` is false.

For each of the two ports — `${user_config.syslog_port}` (UDP+TCP) and `${user_config.mcp_port}` (TCP):
1. `ss -tlnp` (and `ss -ulnp` for syslog UDP) — find any listener on the port
2. **No listener** → PASS ("port available; service can bind on next start")
3. **Listener exists** — extract the PID from the `users:(("name",pid=N,fd=K))` field and verify ownership:
   - **Docker mode**: PASS if the listener is inside the syslog-mcp container (PID belongs to a `containerd-shim` child or matches `docker compose ps -q syslog-mcp`'s container PID)
   - **Systemd mode**: PASS if `systemctl --user show -p MainPID --value syslog-mcp` matches the listener's PID
   - **Otherwise** → FAIL ("port held by an unrelated process — name=X pid=Y; free it before deploying, e.g. `sudo fuser -k ${user_config.syslog_port}/tcp`")

### 2.6 Service status (server mode only)

If `${user_config.is_server}` is true:

**Docker mode** (`${user_config.use_docker}` is true):
- Run `docker compose ps --format json` from `${CLAUDE_PLUGIN_DATA}` and parse the syslog-mcp entry
- **State** must be `running` → otherwise FAIL with the reported state
- **Health.Status** (if a healthcheck is defined) — PASS if `healthy`, WARN if `starting`, FAIL if `unhealthy` or absent when expected
- **If the container is NOT running**, also tail the last 30 lines: `docker compose logs syslog-mcp --tail 30 --no-color` and include in the `## Service logs` section

**Systemd mode** (`${user_config.use_docker}` is false):
- `systemctl --user is-active syslog-mcp` returns `active` → PASS, else FAIL with the reported state
- `systemctl --user is-failed syslog-mcp` returns non-zero (i.e. NOT failed) → PASS
- **If inactive or failed**, also tail the journal: `journalctl --user -u syslog-mcp -n 30 --no-pager` and include in `## Service logs`

### 2.7 HTTP health endpoint

`curl -sS -m 3 ${user_config.server_url}/health`. PASS if 200 with `{"status":"ok"}`. FAIL with the response (or connection error) otherwise.

### 2.8 MCP connectivity (broad)

Call the `syslog` MCP tool with three read-only actions to verify the MCP layer is healthy end-to-end:
- `action: stats` → DB stats (verifies DB access)
- `action: hosts` → host list (verifies queries work)
- `action: tail` with `n: 1` → most recent log (verifies log retrieval)

Report PASS only if all three succeed without `isError`. Surface stats summary inline (total logs, total hosts, write_blocked, logical DB size, free disk).

### 2.9 Listener reachability

Parse the host portion from `${user_config.server_url}` (call it `MCP_HOST`).

- **TCP syslog port**: `nc -z -w2 MCP_HOST ${user_config.syslog_port}` → PASS if connectable, FAIL otherwise (firewall? bind interface?)
- **MCP auth wired**: `curl -sS -o /dev/null -w "%{http_code}" -m 3 ${user_config.server_url}/mcp` should return `401` (auth required) — anything else (esp. `404` or `200`) is a misconfiguration. PASS on 401, FAIL with the actual code otherwise.

### 2.10 Docker ingest hosts (when enabled)

If `${user_config.docker_ingest_enabled}` is true, for each host in `${user_config.fleet_hosts}`:
- `curl -sf -m 3 http://<host>:2375/_ping` returns 200 with `OK` body → PASS
- Else FAIL ("docker-socket-proxy unreachable on <host>:2375 — is it running and exposed?")

### 2.11 Fleet rsyslog drop-ins (server mode only)

If `${user_config.is_server}` is true and `${user_config.fleet_hosts}` is non-empty, verify each fleet host is forwarding correctly. Use `MCP_HOST` from check 2.9 as `FORWARD_TARGET`.

For each host in `${user_config.fleet_hosts}`:
1. **SSH reachability**: `ssh -o BatchMode=yes -o ConnectTimeout=5 <host> true`. Skip the host's remaining checks with a single FAIL row if SSH fails.
2. **Drop-in present**: `ssh <host> "cat /etc/rsyslog.d/99-syslog-mcp.conf 2>/dev/null"`. PASS only if it contains `FORWARD_TARGET:${user_config.syslog_port}`. FAIL with the actual content (or "missing") otherwise.
3. **rsyslog active**: `ssh <host> "systemctl is-active rsyslog"`. PASS on `active`.
4. **Log flow**: cross-check against the `hosts` MCP action output from 2.8. If `<host>` appears with a `last_seen` ≤ 5 minutes old → PASS. Drop-in present but no logs in DB → FAIL ("network/firewall is blocking ${user_config.syslog_port} from <host> to FORWARD_TARGET").

A host that fails check 2 with "missing" → the fix is `/syslog:deploy-dropins`. Mention this explicitly in the failure detail.

## Step 3 — Output format

Print a single results table:

| Check | Status | Detail |
|-------|--------|--------|
| Env: kernel | ℹ INFO | `<uname output>` |
| Env: virt | ℹ INFO | `<systemd-detect-virt output>` |
| Env: systemctl | ✓ / ✗ | path or "not found" |
| Env: docker | ✓ / ✗ | version or "not found" (docker mode only) |
| Env: docker compose | ✓ / ✗ | version or "not found" (docker mode only) |
| Storage: data_dir exists | ✓ / ✗ | path |
| Storage: writable | ✓ / ✗ | mode + owner |
| Storage: free space | ✓ / ⚠ | `N MB free / required M MB` |
| Binary: symlink | ✓ / ⚠ / ✗ | target path |
| Binary: PATH | ✓ / ⚠ | "in PATH" or "$PATH lacks ~/.local/bin" |
| Token: quality | ✓ / ⚠ / ✗ | length tier or "weak placeholder" (never echo the value) |
| Port: syslog (`${user_config.syslog_port}`) | ✓ / ✗ | "free" or "ours pid=N" or "held by `name` pid=N" |
| Port: mcp (`${user_config.mcp_port}`) | ✓ / ✗ | same as above |
| Service: state | ✓ / ✗ | active / running / inactive / failed |
| Service: health (docker) | ✓ / ⚠ / ✗ | healthy / starting / unhealthy (docker mode only) |
| HTTP /health | ✓ / ✗ | response body |
| MCP — stats | ✓ / ✗ | totals or error |
| MCP — hosts | ✓ / ✗ | host count or error |
| MCP — tail | ✓ / ✗ | most recent timestamp or error |
| Listener: syslog port | ✓ / ✗ | `host:port` reachable |
| Listener: MCP auth | ✓ / ✗ | "401 (auth wired)" or unexpected code |
| Docker host: `<name>` | ✓ / ✗ | reachable or error (docker_ingest only) |
| Fleet `<name>`: SSH | ✓ / ✗ | reachable or error |
| Fleet `<name>`: drop-in | ✓ / ✗ | matches expected or "missing → `/syslog:deploy-dropins`" |
| Fleet `<name>`: rsyslog | ✓ / ✗ | active / inactive |
| Fleet `<name>`: log flow | ✓ / ✗ | "last_seen <timestamp>" or "no logs received" |

Then, if any service-state check failed, include a `## Service logs` section with the `journalctl` or `docker compose logs` output.

## Step 4 — Final verdict

End with a one-line verdict and a count breakdown:

- ✅ **All checks passed** (`N` checks, `0` warnings)
- ⚠️  **Ready with warnings** (`N-W` passed, `W` warnings, `0` failures) — list the warnings and recommended fixes
- ❌ **`F` check(s) failed** — list each failure with its concrete next step, e.g.:
  - "Token is weak placeholder → run `just gen-token` and update userConfig"
  - "Port 1514 held by rsyslogd pid=400 → `sudo systemctl stop rsyslog` or change `syslog_port`"
  - "Fleet host `tootie`: drop-in missing → run `/syslog:deploy-dropins`"
  - "data_dir not writable → `chmod 700 ${user_config.data_dir}`"

Footer note: *For deep functional validation (seeds test data and verifies all actions work correctly), run `bash scripts/smoke-test.sh`.*
