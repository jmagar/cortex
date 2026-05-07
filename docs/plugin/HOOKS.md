# Hook Configuration -- syslog-mcp

Lifecycle hooks that run automatically during Claude Code sessions.

## File location

```
plugins/
  hooks/
    hooks.json              # Hook definitions
scripts/
  plugin-setup.sh           # SessionStart hook: env sync + permission fixes
```

## Hook definitions

Hooks are registered in `plugins/hooks/hooks.json` and executed by Claude Code at the appropriate lifecycle point.

### SessionStart — plugin-setup.sh

Runs `${CLAUDE_PLUGIN_ROOT}/scripts/plugin-setup.sh` at the start of every Claude Code session.

Responsibilities:
- Syncs `.env.example` with environment variables read from `src/config.rs` (detects new vars not yet documented)
- Sets `.env` to `chmod 600` (owner read/write only) if the file exists

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

## See also

- [../GUARDRAILS.md](../GUARDRAILS.md) -- security patterns enforced by hooks
- [../mcp/PRE-COMMIT.md](../mcp/PRE-COMMIT.md) -- git hook checks
