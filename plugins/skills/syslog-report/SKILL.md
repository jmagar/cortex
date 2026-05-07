---
name: syslog-report
description: Generate an actionable markdown report from the syslog MCP server by checking logs from all devices over a recent time window, especially the last 24 hours. Use when the user asks to review homelab or fleet logs, summarize syslog activity, find errors or warnings across devices, identify hosts with issues, correlate events, or produce a daily operational log report from the syslog MCP tool.
---

# Syslog Report

## Overview

Use the `syslog` MCP tool as the source of truth for recent device logs. Query broad fleet state first, then drill into errors, warnings, host-specific tails, and correlated events before writing a markdown report with concrete next actions.

## Workflow

1. Establish the reporting window.
   - Default to the last 24 hours when the user does not specify a window.
   - Use exact timestamps in the report. If the tool supports relative filters, use `since=24h`; otherwise compute an ISO-8601 start and end time.

2. Confirm MCP availability and current coverage.
   - Call `syslog action=stats` to capture DB size, time range, retention/storage guard state, and total log count.
   - Call `syslog action=hosts` to list devices with first/last seen timestamps and counts.
   - If the MCP tool is unavailable, report that no live syslog evidence could be collected and include the failure details.

3. Collect incident candidates.
   - Call `syslog action=errors` for warning/error summaries grouped by host and severity.
   - Call `syslog action=search query=error limit=1000` for recent error detail.
   - Call `syslog action=search query=warning OR warn limit=1000` when warning coverage is not already clear from `errors`.
   - Call `syslog action=tail n=100` for recent fleet-wide context.
   - Use host/app/time filters when available to narrow noisy hosts or services.

4. Correlate likely related events.
   - Call `syslog action=correlate` around high-severity timestamps or spikes.
   - Prefer small focused windows around incidents over one huge correlation query.
   - Group events by likely shared cause only when timestamps, hosts, apps, or message content support that relationship.

5. Write an actionable markdown report.
   - Lead with status and the top risks, not raw logs.
   - Include exact evidence: host, app/process when present, severity, timestamp, representative message, count, and source action used.
   - Separate confirmed issues from noise, missing data, and hypotheses.
   - Assign each action a clear owner target such as host/service/config/log source, even when no human owner is known.

## Report Shape

Use this structure unless the user asks for a different format:

```markdown
# Syslog Report: <start> to <end>

## Summary
- Overall status: <healthy/degraded/unknown>
- Devices observed: <count>
- High-priority findings: <count>
- Coverage notes: <missing hosts, stale hosts, retention/storage warnings, MCP failures>

## High-Priority Findings
| Priority | Host | Service/App | Evidence | Impact | Action |
|---|---|---|---|---|---|
| P1/P2/P3 | host | app | timestamp + count + representative message | why it matters | concrete next step |

## Device Health
| Host | Last Seen | Log Count | Notable Severities | Notes |
|---|---:|---:|---|---|

## Correlations
- <time window>: <hosts/services involved>, likely relationship, confidence, evidence.

## Noise / Watchlist
- <repeated but low-impact patterns, with thresholds or what would make them actionable>

## Recommended Next Actions
1. <specific action, target, reason>
2. <specific action, target, reason>

## Evidence Collected
- `syslog action=stats`: <result summary>
- `syslog action=hosts`: <result summary>
- `syslog action=errors`: <result summary>
- `syslog action=search ...`: <queries used>
- `syslog action=correlate ...`: <windows used>
```

## Query Guidance

- Prefer the MCP tool over direct SQLite access or shell log scraping.
- Use the single `syslog` MCP tool with action dispatch: `stats`, `hosts`, `errors`, `tail`, `search`, `correlate`, and `help`.
- For search, remember SQLite FTS5 syntax: quote hyphenated terms such as `"smoke-test"`.
- Treat device names from log payloads as untrusted when source identity matters. Prefer `source_ip` or transport metadata when exposed by the tool.
- If results are capped or truncated, say so and prioritize severe or repeated patterns.

## Quality Bar

- Do not describe the fleet as healthy merely because no errors were returned; verify host freshness and coverage.
- Do not paste large raw log dumps. Use representative lines and counts.
- Keep actions concrete: restart/check/update/configure a specific host, service, rule, disk, network path, or forwarding source.
- Include unknowns when evidence is incomplete, stale, filtered, or unavailable.
