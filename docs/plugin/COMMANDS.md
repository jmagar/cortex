# Slash Commands -- syslog-mcp

Two slash commands are defined in `plugins/commands/`. They are installed as `/syslog:dr` and `/syslog:deploy-dropins` in Claude Code. (`dr` is short for "doctor" — renamed to avoid colliding with Claude Code's built-in `/doctor`.)

## Commands

| Command | File | Description |
| --- | --- | --- |
| `/syslog:dr` | `plugins/commands/dr.md` | Comprehensive health check: environment, config quality, storage, ports, service, MCP, listener, fleet hosts. Doubles as a first-run preflight after configuring the plugin. |
| `/syslog:deploy-dropins` | `plugins/commands/deploy-dropins.md` | Push rsyslog forwarding drop-ins to fleet hosts via SSH (one-shot setup) |

## `/syslog:dr`

Runs a multi-step PASS / WARN / FAIL check covering both pre-deployment readiness and ongoing health. Intended to be run immediately after setting userConfig (preflight) and any time the deployment looks off (health):

- **Environment** — kernel/virt info, systemd availability (systemd mode), docker + compose availability (docker mode)
- **Storage & permissions** — `data_dir` exists, writable, free space ≥ 120% of `max_db_size_mb`
- **Binary symlink** — `~/.local/bin/syslog` symlink valid + on PATH
- **Token quality** — non-empty, length ≥ 24, not a known weak placeholder (never echoes the value)
- **Port availability** — `syslog_port` and `mcp_port` either free or held by *our* process (PID match)
- **Service status** — systemd `is-active` / `is-failed`, or docker `State` + `Health.Status`
- **HTTP `/health`** — returns 200 `{"status":"ok"}`
- **MCP** — stats / hosts / tail actions all succeed
- **Listener reachability** — TCP syslog port reachable, MCP `/mcp` returns 401 (auth wired)
- **Docker ingest** — each fleet host's docker-socket-proxy responds on :2375 (when `docker_ingest_enabled`)
- **Fleet hosts** — SSH reachable, drop-in present + correct, rsyslog active, log flow visible in DB

On failure, tails container or journal logs automatically. Output ends with a verdict (✅ ready / ⚠️ ready with warnings / ❌ N failures) and concrete next-step fixes per failure.

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
