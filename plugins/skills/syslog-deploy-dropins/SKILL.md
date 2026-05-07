---
name: syslog-deploy-dropins
description: Deploy rsyslog forwarding drop-ins to configured fleet hosts over SSH so each host forwards logs to syslog-mcp. Use when the user asks to configure fleet forwarding, deploy syslog drop-ins, repair missing rsyslog forwarding, or update forwarding after server_url or syslog_port changes.
---

# Syslog Deploy Drop-ins

Install or update `/etc/rsyslog.d/99-syslog-mcp.conf` on each configured fleet host.

## Preconditions

Verify before changing hosts:
- SSH aliases from `fleet_hosts` work without prompting.
- Sudo can write rsyslog config and restart rsyslog.
- Each fleet host can route to the syslog-mcp server on the configured syslog port.

Skip devices that cannot be configured through SSH and rsyslog, such as UniFi, Mikrotik, ISP routers, or hosts running syslog-ng or other non-rsyslog forwarders. Point the user to `docs/SETUP.md` for those.

## Resolve Target

Parse the host portion from `$CLAUDE_PLUGIN_OPTION_SERVER_URL`. If it is `localhost` or `127.0.0.1`, stop and ask for a routable hostname or IP because fleet hosts cannot forward to localhost.

Call the resolved value `FORWARD_TARGET`. The endpoint is `FORWARD_TARGET:$CLAUDE_PLUGIN_OPTION_SYSLOG_PORT`.

## Drop-in

Write this file on each host, replacing the target and port:

```text
# Avoid feeding syslog-mcp/rsyslog internal logs back into syslog-mcp.
if ($programname == "syslog" or $programname == "rsyslogd") then stop
*.* @@FORWARD_TARGET:SYSLOG_PORT
```

Use `@@` for TCP. Use single `@` only when a host cannot send TCP.

## Deployment Loop

For each host in `$CLAUDE_PLUGIN_OPTION_FLEET_HOSTS` (split comma-separated or newline-rendered values and ignore blanks):

1. Test SSH:

   ```bash
   ssh -o BatchMode=yes -o ConnectTimeout=5 <host> true
   ```

   On SSH failure: skip this host, mark it as `FAILED (SSH unreachable)` in the report, and continue to the next host.

2. Write the drop-in:

   ```bash
   ssh <host> "printf '%s\n' '# Avoid feeding syslog-mcp/rsyslog internal logs back into syslog-mcp.' 'if (\$programname == \"syslog\" or \$programname == \"rsyslogd\") then stop' '*.* @@FORWARD_TARGET:SYSLOG_PORT' | sudo tee /etc/rsyslog.d/99-syslog-mcp.conf >/dev/null"
   ```

3. Restart rsyslog:

   ```bash
   ssh <host> "sudo systemctl restart rsyslog"
   ```

4. Verify rsyslog:

   ```bash
   ssh <host> "systemctl is-active rsyslog"
   ```

## Report

Print a table:

| Host | Drop-in Deployed | rsyslog Restarted | Status |
|---|---|---|---|
| host | yes/no | yes/no | active/failed |

Tell the user to run `syslog-dr` after a few seconds to confirm log flow, or `bash scripts/smoke-test.sh` for full validation.
