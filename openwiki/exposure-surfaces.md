# Exposure Surfaces

cortex exposes its functionality through three surfaces that share a common service layer: **MCP** (Model Context Protocol), **REST API**, and **CLI**. All three enforce consistent limits, validation, and business logic through `CortexService`.

## Overview

```
┌────────────────────────────────────────────────────────────┐
│                    CortexService                            │
│              (Shared business logic layer)                  │
│  • Limits • Validation • Correlation • Investigation       │
└───────────────────────┬────────────────────────────────────┘
                        │
          ┌─────────────┼─────────────┐
          ▼             ▼             ▼
    ┌──────────┐  ┌──────────┐  ┌──────────┐
    │   MCP    │  │   REST   │  │   CLI    │
    │  Server  │  │   API    │  │          │
    │  :3100   │  │  :3100   │  │  (HTTP)  │
    └──────────┘  └──────────┘  └──────────┘
```

## MCP Server

### Transport
- **Protocol**: RMCP (Rust MCP) over Streamable HTTP
- **Endpoint**: `POST /mcp` on port 3100
- **Mode**: Stateless JSON-response (no streaming)
- **Auth**: OAuth/JWT (Google provider) or static bearer token

### Tool Model
- **Single tool**: `cortex`
- **56 actions**: Dispatched via required `action` parameter
- **Source of truth**: `src/mcp/actions.rs::ACTION_SPECS`

**Example usage**:
```json
{
  "name": "cortex",
  "arguments": {
    "action": "search",
    "query": "error",
    "since": "2024-01-01T00:00:00Z"
  }
}
```

### Scope Gates
- **`cortex:read`**: Most actions (search, tail, errors, etc.)
- **`cortex:admin`**: Destructive actions (`ack_error`, `unack_error`, `file_tails`, `notifications_test`, `llm_invocations`)
- **`help`**: No scope required (info-only)

**Key files**:
- `src/mcp/actions.rs`: Action registry (`ACTION_SPECS`)
- `src/mcp/tools.rs`: Action dispatch handlers
- `src/mcp/schemas.rs`: Tool schema generation
- `src/mcp/rmcp_server.rs`: RMCP server setup

## REST API

### Transport
- **Framework**: Axum web framework
- **Endpoint**: `:3100/api/*` (same port as MCP)
- **Auth**: Bearer token (`CORTEX_API_TOKEN`) or OAuth/JWT
- **Versioning**: URL-based (e.g., `/api/v1/...`) for future compatibility

### Routes
62 routes across several categories:
- **Log queries**: `/api/logs/search`, `/api/logs/tail`, `/api/logs/filter`, `/api/logs/context`
- **AI sessions**: `/api/sessions`, `/api/sessions/search`
- **Incidents**: `/api/skill-incidents`, `/api/mcp-incidents`, `/api/hook-incidents`
- **Investigation**: `/api/skill-investigate`, `/api/mcp-investigate`, `/api/hook-investigate`
- **Analytics**: `/api/stats`, `/api/timeline`, `/api/errors`, `/api/patterns`
- **Inventory**: `/api/hosts`, `/api/apps`, `/api/source-ips`
- **Admin**: `/api/errors/ack`, `/api/errors/unack`, `/api/notifications/test`
- **Health**: `/health`, `/health/full`

**Key files**:
- `src/api.rs`: Route handlers and Axum setup
- `src/surfaces/api.rs`: REST-specific surface logic

### Response Caps
- Default row limits apply (configurable via `CORTEX_MAX_RESULTS`)
- Timeout enforcement prevents runaway queries
- JSON responses with consistent error format

## CLI

### Design
- **Full parity**: Every MCP action has a CLI equivalent
- **HTTP-routed**: Routes via `/api/*` by default (v0.26+)
- **Direct SQLite**: Read-only mode when `CORTEX_USE_HTTP` is unset

### Commands
Organized by domain:
- **Log queries**: `cortex search`, `cortex tail`, `cortex filter`, `cortex context`
- **AI sessions**: `cortex sessions`, `cortex search-sessions`
- **Incidents**: `cortex skill-incidents`, `cortex mcp-incidents`, `cortex hook-incidents`
- **Investigation**: `cortex skill-investigate`, `cortex mcp-investigate`, `cortex hook-investigate`
- **Assessment**: `cortex assess skill`, `cortex assess mcp`, `cortex assess hooks`
- **Analytics**: `cortex stats`, `cortex timeline`, `cortex errors`
- **Inventory**: `cortex hosts`, `cortex apps`, `cortex sources`
- **Operations**: `cortex compose status`, `cortex doctor`

**Key files**:
- `src/cli/commands/`: Command definitions and parsers
- `src/cli/dispatch.rs`: HTTP routing logic
- `src/cli/http_client.rs`: REST client
- `src/cli/parse/`: Output parsers for all response types

### HTTP Routing (v0.26+)
The CLI now routes all queries (except `setup` and `compose`) via `/api/*`:

**Behavior**:
- Default: Routes via HTTP to container's `/api/*` endpoints
- Explicit: `--http` or `--server` + `--token` forces HTTP mode
- Direct: Unset `CORTEX_USE_HTTP` for direct SQLite (read-only, for `cortex mcp stdio`)

**Exceptions** (local-only, never HTTP):
- `cortex compose` (manages local Docker Compose)
- `cortex setup` (manages local config)

## Service Layer Boundaries

`CortexService` in `src/app/` is the single owner of business logic:

### Invariants
- All MCP actions, REST routes, and CLI commands route through `CortexService`
- Query limits, timeouts, and validation enforced once
- Response models shared across all surfaces

### Example: Search Flow
```
MCP "search" action     → CortexService::search_logs()
REST /api/logs/search   → CortexService::search_logs()
CLI "search" command    → CortexService::search_logs()
```

All three return the same `LogQueryResponse` model with identical limits and validation.

### Key Methods
- `search_logs()`: FTS5 search with filters
- `filter_logs()`: Structured filter-only retrieval
- `tail_logs()`: Recent log entries
- `errors_summary()`: Error/warning aggregation
- `correlate_events()`: Cross-host/time correlation
- `list_sessions()`: AI transcript inventory
- `skill_events()`, `skill_incidents()`, `skill_investigate()`: Skill incidents
- `mcp_events()`, `mcp_incidents()`, `mcp_investigate()`: MCP incidents
- `hook_events()`, `hook_incidents()`, `hook_investigate()`: Hook incidents
- `graph_resolve()`: Investigation graph resolution
- `stats()`, `status()`: Database and runtime observability

**Key files**:
- `src/app/services.rs`: Service method definitions
- `src/app/models/`: Request/response models

## Authentication & Authorization

### OAuth/JWT Mode
- **Provider**: Google OAuth 2.0
- **Flow**: Authorization code grant with JWT tokens
- **Config**: `CORTEX_OAUTH_ENABLED=true`, `CORTEX_OAUTH_CLIENT_ID`, `CORTEX_OAUTH_CLIENT_SECRET`
- **Store**: SQLite-backed JWT state in `~/.cortex/auth`

**Key files**:
- `src/config.rs::AuthMode`: Auth configuration
- `src/mcp/auth.rs`: MCP auth policy
- External: `lab-auth` crate (shared with lab repo)

### Static Bearer Mode
- **Token**: `CORTEX_API_TOKEN` (REST) or `CORTEX_TOKEN` (MCP)
- **Admin scope**: `CORTEX_STATIC_TOKEN_ADMIN=true` grants `cortex:admin` to static tokens
- **Use case**: Simple deployments without OAuth

### Scope Gates
- **`cortex:read`**: Query-only actions (search, tail, errors, etc.)
- **`cortex:admin`**: Destructive or sensitive actions (ack_error, llm_invocations, etc.)

## Performance

### Connection Pooling
- SQLite pool shared across all surfaces
- One connection reserved for ingest writer
- Maintenance tasks serialize on `maintenance_permit`

### Query Limits
- Default row limits: `CORTEX_MAX_RESULTS` (default 1000)
- Timeout enforcement: 30s default for most queries
- Response caps: Prevent memory exhaustion

### HTTP Caching
- No response caching (all queries are live)
- Timeline rollups materialized for fast time-series queries

## Adding New Actions

To add a new MCP action/REST route/CLI command:

1. **Define action spec**: Add to `src/mcp/actions.rs::ACTION_SPECS`
2. **Implement handler**: Add method to `CortexService` in `src/app/services.rs`
3. **Add REST route**: Add route handler in `src/api.rs`
4. **Add CLI command**: Add command in `src/cli/commands/` and parser in `src/cli/parse/`
5. **Add tests**: Unit tests in sidecar `*_tests.rs` files

**Example** (adding a new analytics query):
1. Add `ACTION_SPECS` entry for `analytics_summary` action
2. Implement `CortexService::analytics_summary()`
3. Add `GET /api/analytics/summary` route in `src/api.rs`
4. Add `cortex analytics summary` command in `src/cli/commands/analytics.rs`
5. Add tests in `src/app/services_tests.rs` and `src/cli/dispatch_tests.rs`

## References

- **[docs/mcp/SCHEMA.md](../docs/mcp/SCHEMA.md)** – Complete MCP action reference
- **[docs/api.md](../docs/api.md)** – REST API endpoint matrix
- **[docs/CLI.md](../docs/CLI.md)** – Complete CLI command reference
- **[docs/mcp/AUTH.md](../docs/mcp/AUTH.md)** – MCP authentication guide
- **[docs/OAUTH.md](../docs/OAUTH.md)** – OAuth/JWT configuration
