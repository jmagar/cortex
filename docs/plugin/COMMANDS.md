# Slash Commands -- syslog-mcp

Two slash commands are defined in `plugins/commands/`. They are installed as `/syslog:doctor` and `/syslog:deploy-dropins` in Claude Code.

## Commands

| Command | File | Description |
| --- | --- | --- |
| `/syslog:doctor` | `plugins/commands/doctor.md` | Full health check: MCP connectivity, HTTP /health, service status, syslog port, Docker ingest hosts, fleet rsyslog drop-ins |
| `/syslog:deploy-dropins` | `plugins/commands/deploy-dropins.md` | Push rsyslog forwarding drop-ins to fleet hosts via SSH (one-shot setup) |

## `/syslog:doctor`

Runs a multi-step health check and outputs a PASS/FAIL table:

- MCP stats / hosts / tail actions
- HTTP `/health` endpoint
- Service status (Docker container or systemd unit)
- Syslog port TCP reachability
- Docker ingest proxy connectivity (when enabled)
- Fleet host SSH reachability, drop-in presence, rsyslog active, log flow verification

On failure, tails container or journal logs automatically.

## `/syslog:deploy-dropins`

For each host in `fleet_hosts` (plugin config):

1. Tests SSH reachability
2. Writes `/etc/rsyslog.d/99-syslog-mcp.conf` with TCP forwarding to the syslog port
3. Restarts rsyslog and verifies it came back active
4. Outputs a per-host results table

Idempotent — re-running overwrites the existing drop-in with the current `server_url` and `syslog_port`.

## See also

- [CONFIG.md](CONFIG.md) -- `fleet_hosts`, `server_url`, `syslog_port` userConfig fields
- [SKILLS.md](SKILLS.md) -- skill documentation that guides tool usage
- [../mcp/TOOLS.md](../mcp/TOOLS.md) -- MCP tool reference
