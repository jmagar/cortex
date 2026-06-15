# MCP UI Patterns -- cortex

Protocol-level UI hints for MCP servers to improve client-side rendering of tools and results.

## Current state

cortex ships one MCP Apps surface: an interactive query widget.

- **Resource:** `ui://cortex/query-widget`, served through `resources/list` and `resources/read` with MIME `text/html;profile=mcp-app` (see `src/mcp/rmcp_server.rs`).
- **Tool link:** the `cortex` tool carries `_meta.ui.resourceUri` pointing at that resource with `visibility: ["model", "app"]`.
- **Structured results:** `tools/call` returns both readable JSON text content and `structuredContent` so UI hosts can render rows without re-parsing text.
- **Widget UI:** `src/mcp/ui/query_widget.html` is a self-contained, dependency-free page (FTS5 query + hostname/severity/limit filters) that calls the `cortex` tool with `action=search` over an MCP Apps host bridge — `window.openai.callTool` when the host injects it, otherwise an mcp-ui `postMessage` adapter (`type:"tool"` → `ui-message-response`). It degrades to a visible "bridge unavailable" state when no host bridge responds, and renders all log fields via `textContent` to avoid HTML injection.

Tool inputs otherwise use standard JSON Schema properties without `x-ui-*` extensions.

## Schema annotations available

Tool schemas support optional UI hints that clients can interpret:

### Parameter widgets

```json
{
  "severity": {
    "type": "string",
    "enum": ["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"],
    "description": "Filter by syslog severity level"
  }
}
```

The `enum` constraint already enables clients to render a dropdown/select widget without explicit UI annotations.

### Time range inputs

```json
{
  "from": {
    "type": "string",
    "description": "Start of time range (ISO 8601, e.g., '2025-01-15T00:00:00Z')"
  }
}
```

Clients that support datetime widgets can detect ISO 8601 format from the description.

## Response formatting

Tool responses return JSON as text content. Clients render this according to their capabilities:
- CLI clients: raw JSON or formatted with jq
- Web clients: parsed and rendered as tables
- LLM clients: interpreted directly

## Future enhancements

If MCP UI annotations are adopted:

| Action | Possible UI hint |
| --- | --- |
| `cortex search` | Multi-line text input for query, datetime pickers for from/to |
| `cortex tail` | Slider for n parameter |
| `cortex correlate` | Datetime picker for reference_time, slider for window_minutes |
| `cortex errors` | Datetime range picker |

## See also

- [TOOLS.md](TOOLS.md) -- tool reference with current schemas
- [SCHEMA.md](SCHEMA.md) -- schema documentation
- [CORRELATION.md](CORRELATION.md) -- correlation action behavior
