<!--
SPDX-License-Identifier: MIT
Author: jmagar
License: MIT
Description: Hook configuration and lifecycle documentation for the cortex plugin.
-->

# Hook Configuration -- cortex

Lifecycle hooks that run automatically during Claude Code sessions.

## File location

```
plugins/
  cortex/
    hooks/
      hooks.json            # Hook definitions
scripts/
  plugin-setup.sh           # Manual/legacy thin adapter
```

## Hook definitions

Hooks are registered in `plugins/cortex/hooks/hooks.json` and executed by Claude Code at the appropriate lifecycle point.

### SessionStart — cortex setup pluginhook

Runs `${CLAUDE_PLUGIN_ROOT}/bin/cortex setup pluginhook` at the start of
every Claude Code session.

Responsibilities:
- Server mode: exports current Claude Code `userConfig` values as
  `CORTEX_*` / `CORTEX_*` environment variables.
- Ensures a `cortex` binary exists on `PATH`; if it is missing, runs the
  one-line installer.
- Delegates host setup to `cortex setup repair`, which owns
  `~/.cortex/.env`, `~/.cortex/compose/`, and the Docker Compose
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
            "command": "${CLAUDE_PLUGIN_ROOT}/bin/cortex setup pluginhook"
          }
        ]
      }
    ]
  }
}
```

## Manual execution

Run the binary-owned hook outside of Claude Code:

```bash
CLAUDE_PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-$PWD/plugins/cortex}" \
  "$CLAUDE_PLUGIN_ROOT/bin/cortex" setup pluginhook
```

The legacy script remains a thin manual adapter for environments that still
need to map `CLAUDE_PLUGIN_OPTION_*` values before delegating to the binary:

```bash
bash scripts/plugin-setup.sh
```

The hook deliberately contains no separate Compose rendering logic. The Claude
plugin and the one-line installer both converge on the same `cortex setup`
implementation and the same `~/.cortex` host layout.

## See also

- [../GUARDRAILS.md](../GUARDRAILS.md) -- security patterns enforced by hooks
- [../mcp/PRE-COMMIT.md](../mcp/PRE-COMMIT.md) -- git hook checks
