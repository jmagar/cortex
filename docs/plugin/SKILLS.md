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
    syslog-dr/
      SKILL.md           # Full deployment diagnostic workflow
    syslog-deploy-dropins/
      SKILL.md           # Fleet rsyslog drop-in deployment workflow
    ...
```

## Skills

| Skill | Purpose |
| --- | --- |
| `syslog` | Client-facing documentation for the syslog MCP tool and its action dispatch. |
| `syslog-troubleshoot` | Narrow troubleshooting decision tree for MCP, ingest, service, and missing-host issues. |
| `syslog-report` | Generate actionable 24-hour markdown reports from syslog MCP evidence. |
| `syslog-dr` | Comprehensive deployment health check, including runtime freshness. |
| `syslog-deploy-dropins` | Deploy rsyslog forwarding drop-ins to fleet hosts over SSH. |
| `syslog-redeploy` | Re-run the plugin setup hook and verify health plus runtime freshness. |
| `syslog-logs` | Tail or follow cortex service logs from Docker Compose. |
| `syslog-version-check` | Check whether the running container matches the local Compose image. `--pull` checks after refreshing the local image; without it, Docker checks only the local cache. |

### Contents

`plugins/syslog/skills/syslog/SKILL.md` includes:
- Tool inventory (1 tool: `syslog`, with the current MCP action set described)
- Parameter reference for each tool
- FTS5 query syntax guide
- Common workflow patterns (health check, incident investigation, host onboarding)
- Severity level reference (emerg through debug)

### Validation

```bash
just validate-skills
# Checks skill definitions under plugins/syslog/skills/
```

## Adding a skill

If additional skills are needed:

1. Create `plugins/syslog/skills/<name>/SKILL.md`
2. Add frontmatter with `name` and `description`
3. Document tools, workflows, and examples
4. Run `just validate-skills`

## See also

- [PLUGINS.md](PLUGINS.md) -- plugin manifest references the skill
- [../mcp/TOOLS.md](../mcp/TOOLS.md) -- MCP tool definitions
