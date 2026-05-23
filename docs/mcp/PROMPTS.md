# MCP Prompts Reference -- syslog-mcp

syslog-mcp exposes reusable MCP prompts for common infrastructure debugging
workflows. Prompts do not add new data access paths; they guide MCP clients to
call the existing `syslog` tool actions in a useful order and to report evidence
before conclusions.

| Prompt | Purpose | Useful arguments |
| --- | --- | --- |
| `infra.incident-triage` | Build a timeline, scope, likely cause, and next actions for an incident | `window`, `host`, `service` |
| `infra.host-health` | Check one host for silence, error spikes, clock skew, noisy apps, and source identity drift | `host`, `window` |
| `infra.service-outage` | Debug a service, application, or container outage from service logs and nearby host events | `service`, `host`, `window` |
| `infra.security-auth-review` | Review auth failures, bans, suspicious IPs, and correlated infrastructure context | `window`, `actor`, `host` |
| `infra.noise-reduction` | Identify repeated patterns and recommend safe alert tuning or source fixes | `window`, `host`, `service` |
| `infra.agent-change-correlation` | Correlate AI agent work with infrastructure errors and regressions | `project`, `session_id`, `window`, `host`, `service` |

## Example

```json
{
  "name": "infra.service-outage",
  "arguments": {
    "service": "plex",
    "host": "tootie",
    "window": "last 45 minutes"
  }
}
```

The rendered prompt asks the client to use actions such as `search`, `errors`,
`timeline`, `anomalies`, `correlate`, and `context`, then return the likely
failure mode, earliest visible symptom, blast radius, supporting log ids, and a
ranked remediation checklist.
