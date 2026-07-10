# Cortex Quickstart

**cortex** is a Rust-based homelab intelligence platform that ingests logs and AI transcripts, stores them in SQLite with full-text search, and exposes powerful investigation tools via MCP, REST, and CLI.

## What Cortex Does

```
┌─────────────────────────────────────────────────────────────┐
│                    Log & AI Ingestion                        │
├─────────────────────────────────────────────────────────────┤
│  Syslog (UDP/TCP)    │  OTLP /v1/logs  │  Docker logs       │
│  RFC 3164/5424       │  HTTP protobuf  │  Host-local agent  │
│  + CEF parsing       │  4 MiB cap      │  + central pull    │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│              SQLite + FTS5 Full-Text Search                  │
├─────────────────────────────────────────────────────────────┤
│  • 31 sequential migrations                                  │
│  • Batch writes with WAL mode                                │
│  • Retention + storage budget enforcement                    │
│  • Background rollups and maintenance tasks                  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│           Unified Service Layer → Three Surfaces              │
├─────────────────────────────────────────────────────────────┤
│  MCP Server          │  REST API        │  Direct CLI        │
│  56 actions          │  62 routes       │  Full parity        │
│  /mcp (RMCP HTTP)    │  /api/*          │  HTTP-routed        │
│  OAuth/JWT or bearer │  bearer/JWT      │  or direct SQLite   │
└─────────────────────────────────────────────────────────────┘
```

## Key Features

### Log Intelligence
- **Ingestion**: Syslog (UDP/TCP :1514), OTLP HTTP (`/v1/logs`), Docker container logs
- **Storage**: SQLite with FTS5 full-text search, 31 migrations, WAL mode
- **Search**: Full-text search with filters, structured queries, timeline aggregation
- **Maintenance**: Automatic retention, storage budgets, error signature detection

### AI Incident Detection (Differentiating Feature)
- **Event Extraction**: Tracks skill invocations, MCP tool calls, and hook executions from AI transcripts
- **Signal Detection**: Identifies negative transcript patterns after AI actions
- **Incident Grouping**: Scores and prioritizes incident candidates
- **Investigation Bundles**: Deterministic evidence bundles with transcripts, nearby logs, and findings
- **LLM Assessment**: `cortex assess skill|mcp|hooks` runs guarded LLM analysis for deep investigations

### Fleet Awareness
- **Inventory Collection**: SSH/API-based discovery (Docker, UniFi, Unraid, media servers)
- **Heartbeat Telemetry**: Host state and pressure flags
- **Investigation Graph**: Rebuildable projection connecting hosts → containers → apps → AI sessions

### Deployment & Operations
- **Docker Compose**: Native deployment with owner resolution and diagnostics
- **Notifications**: Apprise dispatcher with rule evaluators and daily digests
- **Health Checks**: `/health` and `/health/full` endpoints

## Architecture Overview

cortex is one binary with three operational sub-products sharing a SQLite database:

1. **Log Intelligence Core** – Syslog/OTLP/Docker ingest, AI transcript indexing, FTS5 search, MCP/REST/CLI surfaces
2. **Fleet Inventory / Investigation Graph** – SSH/API inventory, heartbeat telemetry, graph projection
3. **Deployment Tooling** – Compose lifecycle, diagnostics, repair

All three share a common `CortexService` layer that enforces consistent limits, validation, and business logic across MCP, REST, and CLI.

```
┌──────────────────────────────────────────────────────────────┐
│                        RuntimeCore                            │
│  Config → Pool → AuthPolicy → IngestTx → MaintenanceTasks   │
└──────────────────────────────────────────────────────────────┘
                            │
          ┌─────────────────┼─────────────────┐
          ▼                 ▼                 ▼
    ┌──────────┐      ┌──────────┐      ┌──────────┐
    │   MCP    │      │   REST   │      │   CLI    │
    │  Server  │      │   API    │      │ (HTTP)   │
    │  :3100   │      │  :3100   │      │          │
    └──────────┘      └──────────┘      └──────────┘
          │                 │                 │
          └─────────────────┼─────────────────┘
                            ▼
                    ┌───────────────────┐
                    │  CortexService   │
                    │  (app/mod.rs)    │
                    └───────────────────┘
                            │
          ┌─────────────────┼─────────────────┐
          ▼                 ▼                 ▼
    ┌──────────┐      ┌──────────┐      ┌──────────┐
    │   DB     │      │ Scanner  │      │Inventory │
    │  Pool    │      │ Sessions │      │ Heartbeat│
    └──────────┘      └──────────┘      └──────────┘
```

## Major Domains

| Domain | Purpose | Key Sources |
|--------|---------|-------------|
| **[Log Intelligence](log-intelligence.md)** | Syslog/OTLP/Docker ingest, SQLite storage, FTS5 search, maintenance | `src/receiver/`, `src/ingest.rs`, `src/otlp.rs`, `src/db/` |
| **[AI Incidents](ai-incidents.md)** | Skill/MCP/hook event tracking, signal detection, incident grouping, LLM assessment | `src/app/*/incident_findings.rs`, `src/db/*_incidents.rs` |
| **[Exposure Surfaces](exposure-surfaces.md)** | MCP tool dispatch, REST API routes, CLI parity, scope gates | `src/mcp/`, `src/api.rs`, `src/cli/`, `src/app/` |
| **[Inventory Graph](inventory-graph.md)** | Fleet inventory collection, heartbeat telemetry, graph projection | `src/inventory/`, `src/heartbeat.rs`, `src/db/graph.rs` |
| **[Operations](operations.md)** | Deployment lifecycle, config management, notifications, diagnostics | `src/compose/`, `src/setup/`, `src/config.rs` |
| **[Development](development.md)** | Build/test commands, module organization, adding features, test coverage | `src/lib.rs`, `Justfile`, `.github/workflows/` |

## Quick Start

### Local Development

```bash
# Clone and build
git clone <repo>
cd cortex
cargo build --release

# Run locally (reads config.toml)
cargo run

# Run tests
cargo test
cargo clippy
cargo fmt
```

### Production Deployment

```bash
# Start Docker Compose stack
docker compose up -d

# Check health
curl http://localhost:3100/health | jq

# View logs
docker compose logs -f cortex
```

### Key Commands

```bash
# Log search (CLI routes to HTTP by default)
cortex search "error" --since 1h

# AI incident investigation
cortex skill-incidents --limit 5
cortex skill-investigate <skill_name>

# LLM assessment (CLI-only, guarded)
cortex assess skill <skill_name> --since 7d

# Fleet diagnostics
cortex compose status
cortex doctor
```

## Important Concepts

### Background Tasks
The runtime spawns supervised maintenance tasks (see `src/runtime.rs`):
- **Retention purge**: Hourly cleanup of old logs
- **Storage budget**: 60s checks for DB size and disk space
- **Error signatures**: Hourly scan for repeating error patterns
- **Inventory refresh**: 5min inventory collection and graph projection
- **Session rollup**: 300s refresh of AI session aggregates
- **Timeline rollup**: 60s incremental hourly bucketing
- **Notification dispatcher**: 30s outbound notification queue
- **Docker streams**: Continuous container log ingestion

### Service Layer Boundaries
All MCP actions, REST routes, and CLI commands route through `CortexService` in `src/app/`. This shared layer:
- Enforces query limits and timeouts
- Validates request parameters
- Implements business logic (correlation, incident grouping, evidence bundling)
- Returns consistent response models

### Authentication & Scopes
- **MCP**: OAuth/JWT (Google provider) or static bearer tokens
- **REST**: Bearer token (`CORTEX_API_TOKEN`) or OAuth/JWT
- **Scopes**: `cortex:read` for most actions; `cortex:admin` for mutation (`ack_error`, `unack_error`, `file_tails`, `notifications_test`, `llm_invocations`)

### HTTP CLI Cutover (v0.26+)
The CLI now routes all queries (except `setup` and `compose`) via `/api/*` by default:
- Set `CORTEX_USE_HTTP=true` explicitly to force HTTP mode
- Unset `CORTEX_USE_HTTP` to use direct SQLite (read-only, for `cortex mcp stdio`)

## Navigation

- **[Architecture Details](architecture.md)** – System architecture, data flow, module map
- **[Log Intelligence](log-intelligence.md)** – Ingestion, storage, search, maintenance
- **[AI Incidents](ai-incidents.md)** – Event tracking, signal detection, investigation workflows
- **[Exposure Surfaces](exposure-surfaces.md)** – MCP, REST, CLI design and routing
- **[Inventory Graph](inventory-graph.md)** – Fleet inventory, heartbeat telemetry, graph projection
- **[Operations](operations.md)** – Deployment, configuration, notifications
- **[Development Guide](development.md)** – Build/test, module organization, adding features

## External Documentation

The `docs/` directory has comprehensive references:
- **[SETUP.md](../docs/SETUP.md)** – Step-by-step setup guide
- **[CONFIG.md](../docs/CONFIG.md)** – Complete configuration reference
- **[CLI.md](../docs/CLI.md)** – Full CLI command reference
- **[api.md](../docs/api.md)** – REST API endpoint matrix
- **[mcp/SCHEMA.md](../docs/mcp/SCHEMA.md)** – MCP tool and action reference

## Repository Context

- **Language**: Rust (edition 2024, MSRV 1.86)
- **Database**: SQLite with FTS5 full-text search
- **Transport**: RMCP (MCP over Streamable HTTP), Axum (REST)
- **Deployment**: Docker Compose
- **Current version**: 3.6.5 (see `Cargo.toml`)

## Next Steps

1. **For operators**: Read [Operations](operations.md) and the external [SETUP.md](../docs/SETUP.md)
2. **For developers**: Read [Development](development.md) and [Architecture](architecture.md)
3. **For incident investigators**: Read [AI Incidents](ai-incidents.md)
4. **For API integrators**: Read [Exposure Surfaces](exposure-surfaces.md) and [api.md](../docs/api.md)
