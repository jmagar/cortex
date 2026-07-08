---
name: cortex-session-search
description: Search and explore AI/Claude/Codex conversation history stored in cortex. Use when the user asks to "search sessions", "find conversations", "what did I work on", "session history", "past conversations", "ask history", "when did I last use X", "show recent sessions", "list AI projects", "what tools have I used", "investigate skill failures", or mentions AI transcripts, conversation search, session analysis, or past work context retrieval.
---

# Cortex Session Search

Search and explore AI/Claude/Codex conversation history stored in cortex. The skill provides full-text search over session transcripts, question-based history retrieval with system log context, project summaries, usage patterns, and investigation tools for skill/MCP/hook failures.

## Primary Actions

### `sessions` — Browse recent conversations
List AI transcript sessions grouped by project, tool, session, and host. Use for discovery and overview of recent activity.

### `search_sessions` — Full-text search with relevance ranking
FTS5 search over AI transcript rows. Returns grouped session results ranked by relevance. Best for targeted searches when you know what you're looking for.

### `ask_history` — Question-based history with system context
Search AI transcript history for past work related to a topic. Returns AI session hits plus non-AI system logs from the top session's time window. Use for "what happened when I worked on X" or "how did we solve Y before".

### `project_context` — Full project summary
Complete context for one AI project path: tools, sessions, hosts, counts, and recent representative entries. Use for getting oriented on a project's activity.

### `ai_correlate` — Session events + system logs
Use AI transcript rows as timeline anchors and pull nearby non-AI syslog, Docker, OTLP, and host events. Excludes AI rows from correlated logs so sessions don't self-correlate. Use for "what system events happened around my session work".

### `usage_blocks` — Activity patterns over time
Bucket AI activity into deterministic 5-hour UTC windows. Use for understanding when work happens and identifying patterns.

## Discovery Actions

### `list_ai_projects` — Distinct projects with counts
Enumerate all known AI projects with session and transcript counts. Use for orientation and discovery.

### `list_ai_tools` — Distinct tools with counts
Enumerate all known AI tools (claude, codex, gemini, etc.) with session and transcript counts. Use for understanding tool usage.

## Investigation Actions

### `skill_investigate` — Skill failure deep-dive
Expands skill-usage incidents into deterministic evidence bundles. Accepts a skill name directly. Use for "what went wrong with cortex-troubleshoot" or "show me skill failure incidents".

### `mcp_investigate` — MCP tool failure deep-dive
Expands MCP-usage incidents into evidence bundles, server/tool-first. Use for "what's failing with the cortex MCP server" or "tool timeout incidents".

### `hook_investigate` — Hook failure deep-dive
Expands hook-usage incidents into evidence bundles, hook-first. Use for "hook failures" or "hook timeout issues".

### `abuse_incidents` — Resource abuse detection
Groups AI transcript abuse hits into scored incident candidates. Returns incidents ordered by priority: low, medium, high, critical.

### `abuse_investigate` — Abuse incident deep-dive
Expands abuse incidents into evidence bundles with transcript context and nearby system logs.

## Workflows

### Browse recent sessions
```
mcp__cortex__cortex(action="sessions", limit=30)
```

### Search for specific topics
```
mcp__cortex__cortex(action="search_sessions", query="nginx ssl", limit=10)
```

### Question-based history with system context
```
mcp__cortex__cortex(action="ask_history", query="what causes qbittorrent to keep dying?")
```

### Get full project context
```
mcp__cortex__cortex(action="project_context", project="lab")
```

### Correlate session with system logs
```
mcp__cortex__cortex(action="ai_correlate", session_id="abc123", window_minutes=15)
```

### Investigate skill failures
```
mcp__cortex__cortex(action="skill_investigate", skill="cortex-troubleshoot", limit=5)
```

### Usage patterns over time
```
mcp__cortex__cortex(action="usage_blocks", project="cortex", since="7d")
```

### Discover projects and tools
```
mcp__cortex__cortex(action="list_ai_projects")
mcp__cortex__cortex(action="list_ai_tools")
```

### Investigate abuse incidents
```
mcp__cortex__cortex(action="abuse_incidents", limit=10)
mcp__cortex__cortex(action="abuse_investigate", incident_id=123)
```

## Time Windows

`since`/`until`/`from`/`to` accept multiple formats across all actions:

- **Relative**: `30m`, `1h`, `2d`, `90s`, `7d`
- **Keywords**: `now`, `today`, `yesterday`
- **Bare dates**: `2026-06-01`, `2026-06-01 08:30`
- **Full RFC3339**: `2026-06-19T09:30:00Z`

Omit time windows to use each action's default (often the last hour or full history).

## Common Filters

Many actions support optional filters:
- `project` — scope to one AI project path
- `tool` — scope to one AI tool (claude, codex, gemini, etc.)
- `hostname` — scope to one host
- `limit` — cap results (varies by action, often default 10-50)
- `from`/`to` — time window bounds

## FTS5 Query Syntax

`search_sessions` and `ask_history` use SQLite FTS5 with porter stemming:

- `AND`, `OR`, `NOT` are uppercase boolean operators
- Quote phrases and hyphenated terms: `"smoke-test"`, `"nginx ssl"`
- Invalid FTS5 syntax returns a database error

## Action Cost Tiers

Start with **cheap** bounded calls, narrow scope with **moderate** actions, reserve **expensive** for specific questions:

- **cheap**: `sessions`, `list_ai_projects`, `list_ai_tools`, `usage_blocks`
- **moderate**: `search_sessions`, `ask_history`, `project_context`, `abuse_incidents`
- **expensive**: `ai_correlate`, `skill_investigate`, `mcp_investigate`, `hook_investigate`, `abuse_investigate`

## When to Use Each Action

| User Intent | Best Action | Why |
|-------------|-------------|-----|
| "What did I work on recently?" | `sessions` | Browse overview |
| "Find conversations about X" | `search_sessions` | Targeted FTS5 search |
| "How did we solve X before?" | `ask_history` | Returns system context too |
| "Tell me about project X" | `project_context` | Full project summary |
| "What happened around that session?" | `ai_correlate` | Session + system events |
| "When do I use tool X?" | `usage_blocks` | Activity patterns |
| "What's failing with skill X?" | `skill_investigate` | Skill failure incidents |
| "Show me projects I've used" | `list_ai_projects` | Discovery |
| "What tools have I used?" | `list_ai_tools` | Discovery |
| "Any abuse incidents?" | `abuse_incidents` | Abuse detection |

## CLI Examples

The same actions are available via the CLI:

```bash
# Browse sessions
cortex sessions --limit 30

# Search transcripts
cortex sessions search "nginx ssl" --limit 10

# Ask history with system context
cortex sessions ask-history "what causes qbittorrent to keep dying?"

# Project context
cortex sessions project-context cortex

# Investigate skill failures
cortex sessions investigate skill cortex-troubleshoot --limit 5

# Usage blocks
cortex sessions usage-blocks --project cortex --since 7d
```

## HTTP Fallback Mode

Use only when the MCP tool is unavailable. The plugin exports connection settings as environment variables:

- `CLAUDE_PLUGIN_OPTION_SERVER_URL` — base URL (e.g., `http://localhost:3100`)
- `CLAUDE_PLUGIN_OPTION_API_TOKEN` — bearer token

**Required headers for `POST /mcp`:**
```bash
-H "Accept: application/json, text/event-stream" \
-H "Content-Type: application/json"
```

### Example: Search sessions via HTTP
```bash
curl -s -X POST "$CLAUDE_PLUGIN_OPTION_SERVER_URL/mcp" \
  -H "Authorization: Bearer $CLAUDE_PLUGIN_OPTION_API_TOKEN" \
  -H "Accept: application/json, text/event-stream" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"cortex","arguments":{"action":"search_sessions","query":"nginx","limit":10}}}'
```
