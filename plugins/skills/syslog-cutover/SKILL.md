---
name: syslog-cutover
description: Switch syslog-mcp between Docker Compose and systemd user-unit deployment modes, update saved userConfig, run the setup hook, and verify health. Use when the user asks to cut over to docker, cut over to systemd, switch deployment mode, or test both deployment modes.
---

# Syslog Cutover

Perform a one-shot deploy-mode switch between Docker and systemd.

## Workflow

1. Parse the target mode from the user request:
   - `docker` means `use_docker=true`.
   - `systemd` means `use_docker=false`.
   - If the target is missing or invalid, stop and ask for `docker` or `systemd`.

2. Read the current deploy mode:

   ```bash
   echo "$CLAUDE_PLUGIN_OPTION_USE_DOCKER"
   ```

   If it already matches the target, ask whether to force a redeploy. Stop if the user does not want a forced redeploy.

3. Before changing anything, summarize:
   - current mode and target mode
   - old deployment will be stopped before the new deployment starts
   - `/health` may be briefly unreachable and syslog packets may be missed during the swap; wait up to 90 seconds for the new mode to become healthy before reporting failure
   - the same `$CLAUDE_PLUGIN_OPTION_DATA_DIR/syslog.db` is reused

4. Update `~/.claude/settings.json`:

   ```bash
   export TARGET_IS_DOCKER=true  # use false for systemd
   python3 -c "
   import json
   import os
   from pathlib import Path
   p = Path.home() / '.claude' / 'settings.json'
   s = json.loads(p.read_text())
   configs = s.setdefault('pluginConfigs', {})
   key = next((k for k in configs if k.startswith('syslog@')), None)
   if key is None:
       print('ERROR: no syslog@ plugin config found in settings.json — verify the plugin is installed')
       exit(1)
   opts = configs.setdefault(key, {}).setdefault('options', {})
   opts['use_docker'] = os.environ['TARGET_IS_DOCKER'].lower() == 'true'
   p.write_text(json.dumps(s, indent=2))
   print(f\"{key}: use_docker -> {opts['use_docker']}\")
   "
   ```

   Set `TARGET_IS_DOCKER=true` for Docker or `false` for systemd. This direct edit saves the setting but does not itself trigger ConfigChange.

5. Run the setup hook:

   ```bash
   ${CLAUDE_PLUGIN_ROOT}/scripts/plugin-setup.sh
   ```

   Capture stdout, stderr, and exit code.

6. Verify:
   - HTTP `/health` returns 200.
   - target mode is active or healthy.
   - old mode is stopped.
   - MCP `stats` succeeds.
   - if needed, verify the same syslog DB path is still in use.

## Report

End with:
- `Cutover complete` and the target mode
- `Cutover partial` with the failed verification and current state
- `Cutover failed` with the hook error and rollback suggestion

If rollback is needed, restore the previous `use_docker` value in `~/.claude/settings.json` and run the setup hook again.
