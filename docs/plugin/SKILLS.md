<!--
SPDX-License-Identifier: MIT
Author: jmagar
License: MIT
Description: Skill definitions and validation guidance for the cortex plugin.
-->

# Skill Definitions -- cortex

Patterns for defining skills (domain knowledge modules) within the cortex plugin.

## Directory structure

```
plugins/
  skills/
    syslog/
      SKILL.md           # Skill definition with tool reference and workflows
    cortex-dr/
      SKILL.md           # Full deployment diagnostic workflow
    cortex-deploy-dropins/
      SKILL.md           # Fleet rsyslog drop-in deployment workflow
    ...
```

## Skills

| Skill | Purpose |
| --- | --- |
| `cortex` | Client-facing documentation for the cortex MCP tool and its action dispatch. |
| `cortex-troubleshoot` | Narrow troubleshooting decision tree for MCP, ingest, service, and missing-host issues. |
| `cortex-report` | Generate actionable 24-hour markdown reports from cortex MCP evidence. |
| `cortex-dr` | Comprehensive deployment health check, including runtime freshness. |
| `cortex-deploy-dropins` | Deploy rsyslog forwarding drop-ins to fleet hosts over SSH. |
| `cortex-redeploy` | Re-run the plugin setup hook and verify health plus runtime freshness. |
| `cortex-logs` | Tail or follow cortex service logs from Docker Compose. |
| `cortex-version-check` | Check whether the running container matches the local Compose image. `--pull` checks after refreshing the local image; without it, Docker checks only the local cache. |

### Contents

`plugins/cortex/skills/cortex/SKILL.md` includes:
- Tool inventory (1 tool: `cortex`, with the current MCP action set described)
- Parameter reference for each tool
- FTS5 query syntax guide
- Common workflow patterns (health check, incident investigation, host onboarding)
- Severity level reference (emerg through debug)

### Validation

```bash
just validate-skills
# Checks skill definitions under plugins/cortex/skills/
```

## Adding a skill

If additional skills are needed:

1. Create `plugins/cortex/skills/<name>/SKILL.md`
2. Add frontmatter with `name` and `description`
3. Document tools, workflows, and examples
4. Run `just validate-skills`

## See also

- [PLUGINS.md](PLUGINS.md) -- plugin manifest references the skill
- [../mcp/TOOLS.md](../mcp/TOOLS.md) -- MCP tool definitions
