---
description: One-shot deploy-mode switch between docker and systemd. Stops the running mode, starts the chosen mode, verifies health. Updates userConfig if the requested mode differs from saved.
argument-hint: "docker|systemd"
---

Switch syslog-mcp between docker compose and systemd user unit deployment. Useful for testing both modes without manually editing userConfig + waiting for ConfigChange.

## Step 1 — Parse target

The argument: `$ARGUMENTS`

- `docker` → set `use_docker=true`
- `systemd` → set `use_docker=false`
- Any other value → error and stop, suggest one of the two

## Step 2 — Show current vs target

Read current `${user_config.use_docker}`:

- If it already matches the target, ask the user whether they want to force a redeploy anyway. If no, stop.
- If it differs, summarize the cutover before proceeding:
  - "Cutting over from <current_mode> to <target_mode>."
  - "<current> deployment will be stopped, then <target> will start."
  - "Brief gap in `/health` reachability and possibly missed syslog packets during the swap."
  - "Same `${user_config.data_dir}/syslog.db` is reused — no migration."

## Step 3 — Update userConfig

The plugin substitution at command invocation reflects the saved `use_docker` value. To actually flip the deployment, edit `~/.claude/settings.json`:

```bash
python3 -c "
import json, sys
from pathlib import Path
p = Path.home() / '.claude' / 'settings.json'
s = json.loads(p.read_text())
opts = s.setdefault('pluginConfigs', {}).setdefault('syslog@jmagar-lab', {}).setdefault('options', {})
opts['use_docker'] = ('docker' == '$ARGUMENTS')
p.write_text(json.dumps(s, indent=2))
print(f\"use_docker → {opts['use_docker']}\")
"
```

Note: setting via this path saves the choice but does NOT trigger ConfigChange (the file write happens via direct edit, not via the TUI). The next step runs the hook explicitly.

## Step 4 — Trigger the cutover

Run the setup hook directly. The hook reads userConfig env and runs `setup_docker()` or `setup_systemd()`, each of which now stops the other mode first:

```bash
${CLAUDE_PLUGIN_ROOT}/scripts/plugin-setup.sh
```

Capture stdout + stderr + exit code.

## Step 5 — Verify with /syslog:dr

Once the hook returns success, run a health check:

- HTTP `/health` returns 200
- The chosen deploy mode reports active/healthy
- The other mode is stopped (not just inactive — for systemd, it should also be disabled)
- MCP tool `stats` action succeeds
- Same syslog.db file is being written (verify by inode if you want to be thorough)

If `/syslog:dr` is available in this plugin, suggest the user run it. Otherwise emit a compact pass/fail of the four checks above.

## Step 6 — Report

End with:

- ✅ Cutover complete — running on <target_mode>
- ⚠️ Cutover partial — <what failed>, deployment may be in a half-state; check `/syslog:logs` and `/syslog:dr`
- ❌ Cutover failed — <error>, both modes may be down

If the cutover fails midway (target failed to start), suggest:

```bash
# rollback to previous mode
python3 -c "import json, sys; p='/home/jmagar/.claude/settings.json'; s=json.load(open(p)); s['pluginConfigs']['syslog@jmagar-lab']['options']['use_docker'] = not ('docker' == '$ARGUMENTS'); json.dump(s, open(p,'w'), indent=2)"
${CLAUDE_PLUGIN_ROOT}/scripts/plugin-setup.sh
```
