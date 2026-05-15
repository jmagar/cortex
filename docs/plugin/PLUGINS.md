<!--
plugin: syslog-mcp
surface: plugin-manifests
version: 0.21.7
author: Jacob Magar
license: MIT
description: Current syslog-mcp Claude Code plugin manifest reference.
-->

# Plugin Manifest Reference -- syslog-mcp

This repo currently ships the Claude Code plugin manifest at
`.claude-plugin/plugin.json`. The manifest version is kept in sync with
`Cargo.toml`; future plugin manifests must use the same version.

## File locations

| File | Platform | Status |
| --- | --- | --- |
| `.claude-plugin/plugin.json` | Claude Code | Current plugin manifest |
| `plugins/.mcp.json` | Claude Code | MCP server template referenced by the manifest |
| `plugins/hooks/hooks.json` | Claude Code | SessionStart and ConfigChange hook registration |
| `plugins/skills/` | Claude Code | Plugin skill surfaces |

This repo does not currently ship tracked Codex or Gemini manifest files. It
does ship `server.json` for MCP Registry metadata. Do not copy older Codex or
Gemini examples from release history into new docs without adding the actual
manifest files.

## Claude Code plugin.json

The current manifest declares:

| Field | Purpose |
| --- | --- |
| `mcpServers` | Points Claude Code at `./plugins/.mcp.json` |
| `hooks` | Runs `plugins/hooks/hooks.json` |
| `skills` | Exposes repo-local plugin skills |
| `userConfig.server_url` | Base HTTP URL for the running syslog-mcp server |
| `userConfig.api_token` | Required Bearer token used by the plugin MCP client; enforced by the server unless `no_auth=true` |
| `userConfig.no_auth` | Explicitly disables static-token enforcement for loopback or upstream-authenticated deployments |
| `userConfig.is_server` | Whether this machine owns the local Docker Compose deployment |
| `userConfig.syslog_port` / `syslog_host_port` / `mcp_port` | Container port mapping controls |
| `userConfig.data_dir` | Host data directory for the Compose deployment |
| `userConfig.auth_mode` and OAuth fields | Optional OAuth/JWT configuration |
| `userConfig.docker_ingest_*` | Optional docker-socket-proxy log ingestion |

`plugins/.mcp.json` interpolates these values with `${user_config.*}`
placeholders. Keep docs and validation scripts aligned with that syntax.

## Version synchronization

Use `just publish [major|minor|patch]` for releases. That flow bumps
`Cargo.toml`, `.claude-plugin/plugin.json`, and any future version-bearing
files together, then updates `CHANGELOG.md`.

## See also

- [CONFIG.md](CONFIG.md) -- plugin settings and userConfig fields
- [HOOKS.md](HOOKS.md) -- plugin hook behavior
- [SKILLS.md](SKILLS.md) -- plugin skills
- [../mcp/PUBLISH.md](../mcp/PUBLISH.md) -- publishing and transport notes
