---
description: Deploy rsyslog forwarding drop-ins to every fleet host via SSH (one-shot setup)
---

Push an rsyslog drop-in to each host in `${user_config.fleet_hosts}` so they forward all syslog to this syslog-mcp server.

## Prerequisites (verify before running)

1. **SSH access** to every fleet host using the alias from `~/.ssh/config` (e.g. `ssh squirts` works without prompting)
2. **Sudo without password** (or NOPASSWD for the rsyslog config + restart commands)
3. **Server is reachable** from each fleet host on port `${user_config.syslog_port}`

If any fleet host can't be configured this way (UniFi, Mikrotik, ATT routers, etc.), skip it here and configure it manually per `docs/SETUP.md`.

## Determine the forwarding target

The fleet hosts need a hostname or IP they can route to. Parse the host portion from `${user_config.server_url}` — that's the syslog-mcp server's name. If the result is `localhost` or `127.0.0.1`, ask the user for the routable hostname (e.g. their tailscale name like `shart`) — fleet hosts can't forward to localhost.

Call this resolved value `FORWARD_TARGET`. The forwarding endpoint is `FORWARD_TARGET:${user_config.syslog_port}`.

## Drop-in content

Each fleet host gets `/etc/rsyslog.d/99-syslog-mcp.conf` containing:

```
*.* @@FORWARD_TARGET:SYSLOG_PORT
```

`@@` selects TCP for delivery reliability. Use `@` (single) instead if a host only supports UDP.

## Deployment loop

For each host in `${user_config.fleet_hosts}` (comma-separated):

1. Test SSH: `ssh -o BatchMode=yes -o ConnectTimeout=5 <host> true` — skip the host with a clear FAIL if this errors
2. Write the drop-in:
   ```
   ssh <host> "echo '*.* @@FORWARD_TARGET:SYSLOG_PORT' | sudo tee /etc/rsyslog.d/99-syslog-mcp.conf >/dev/null"
   ```
3. Restart rsyslog:
   ```
   ssh <host> "sudo systemctl restart rsyslog"
   ```
4. Verify the service came back up:
   ```
   ssh <host> "systemctl is-active rsyslog"
   ```

## Output

Print a results table:

| Host | Drop-in deployed | rsyslog restarted | Status |
|------|------------------|-------------------|--------|
| <host> | ✓ / ✗ | ✓ / ✗ | active / failed |

After deployment, suggest the user run `/syslog:dr` after a few seconds to confirm logs are flowing in (the `hosts` action should now list the deployed hosts), or `bash scripts/smoke-test.sh` for full validation.

## Idempotency

The drop-in path is fixed (`99-syslog-mcp.conf`), so re-running this command overwrites the existing file with the current target. Safe to re-run after changing `server_url` or `syslog_port`.
