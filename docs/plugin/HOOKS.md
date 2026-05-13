<!--
Author: jmagar
License: MIT
Description: Hook configuration and lifecycle documentation for the syslog-mcp plugin.
-->

# Hook Configuration -- syslog-mcp

Lifecycle hooks that run automatically during Claude Code sessions.

## File location

```
plugins/
  hooks/
    hooks.json              # Hook definitions
scripts/
  plugin-setup.sh           # SessionStart hook: shared setup repair
```

## Hook definitions

Hooks are registered in `plugins/hooks/hooks.json` and executed by Claude Code at the appropriate lifecycle point.

### SessionStart — plugin-setup.sh

Runs `${CLAUDE_PLUGIN_ROOT}/scripts/plugin-setup.sh` at the start of every Claude Code session.

Responsibilities:
- Server mode: exports current Claude Code `userConfig` values as
  `SYSLOG_*` / `SYSLOG_MCP_*` environment variables.
- Ensures a `syslog` binary exists on `PATH`; if it is missing, runs the
  one-line installer.
- Delegates host setup to `syslog setup repair`, which owns
  `~/.syslog-mcp/.env`, `~/.syslog-mcp/compose/`, and the Docker Compose
  container.
- Client mode: skips local server setup and only checks the configured
  server's `/health` endpoint.

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/scripts/plugin-setup.sh"
          }
        ]
      }
    ]
  }
}
```

## Manual execution

Run the setup script outside of Claude Code:

```bash
bash scripts/plugin-setup.sh
```

The hook deliberately contains no separate Compose rendering logic. The Claude
plugin and the one-line installer both converge on the same `syslog setup`
implementation and the same `~/.syslog-mcp` host layout.

## See also

- [../GUARDRAILS.md](../GUARDRAILS.md) -- security patterns enforced by hooks
- [../mcp/PRE-COMMIT.md](../mcp/PRE-COMMIT.md) -- git hook checks
