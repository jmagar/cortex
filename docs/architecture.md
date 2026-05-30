# cortex architecture (post v0.26)

This document captures how callers reach the SQLite database after the
v0.26 CLI-over-HTTP cutover (epic `cortex-0p8r`). It complements
the runtime overview in `CLAUDE.md` and the endpoint matrix in
[`docs/api.md`](api.md).

## Caller → database paths

```text
AI clients ──▶ /mcp (rmcp streamable HTTP)        ─┐
                                                   │
CLI default ──▶ /api/* (REST)                      ├──▶ container SyslogService ──▶ SQLite (/data)
   [CORTEX_USE_HTTP=true since v0.26]              │       (db_permits pool + MAINTENANCE_PERMIT)
                                                   │
CLI explicit "unset CORTEX_USE_HTTP"  ─────────────┘
   ──▶ direct SQLite (RuntimeCore::load_query_only, read-only)

syslog ai watch (systemd) ────────────────────────────▶ direct SQLite
   (service.add_ai_file; long-running daemon on the host)

syslog mcp stdio (spawned by AI clients) ─────────────▶ direct SQLite
   (one-shot stdio session bound to the host's DB path)
```

## Ownership

The container is the **canonical query-path owner**: every `/api/*`
caller — REST CLI, AI client over `/mcp`, anything routed through
SWAG — funnels through one `SyslogService` instance with shared
`db_permits` and `MAINTENANCE_PERMIT` gates. Direct-SQLite access
remains for two consumers that cannot reasonably go through HTTP — one
write-path and one read-path:

- `syslog ai watch` (write-path) — a host-side systemd daemon that
  streams local AI transcript files into SQLite. Going through HTTP
  would mean uploading every JSONL chunk over loopback for no value, so
  this writer keeps direct `service.add_ai_file` access against the
  same DB file the container reads.
- `syslog mcp` stdio (read/query-path only) — spawned by AI clients
  (Claude Desktop, Codex) that don't speak HTTP-MCP. The stdio process
  opens the same DB path read-only via `RuntimeCore::load_query_only`,
  so it never participates in the write path.

Both direct-write consumers are detected by `syslog compose doctor`
(always-on) and optionally surfaced through `syslog db status --check-coord`.
See [`docs/api.md`](api.md) "Local-only commands" for the per-command
breakdown and the operational `systemd` timer recipe.
