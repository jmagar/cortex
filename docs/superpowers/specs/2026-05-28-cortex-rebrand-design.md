# cortex v1.0.0 — Rebrand Design

**Date:** 2026-05-28
**Status:** Draft
**Scope:** Full rename of syslog-mcp → cortex

---

## Summary

Rename the `syslog-mcp` project to `cortex` at v1.0.0. The product has grown well beyond an MCP server — it now includes a full CLI, HTTP API with parity to MCP, fleet state, heartbeat agent, command log, AI watch, RAG endpoints, and OTLP ingestion. The name change is a declaration: cortex is a homelab intelligence platform that happens to expose MCP as one of its interfaces.

This is a hard break. No backwards-compatibility shims. Deployers update env vars and agent configs as part of the v1.0.0 upgrade.

---

## Decisions

| Question | Decision |
|---|---|
| Binary name | `cortex` (canonical) + `cx` (short alias) |
| MCP tool name | `cortex` — action strings unchanged |
| Env prefix | `CORTEX_*` (was `SYSLOG_MCP_*`) |
| Version | v1.0.0 (was v0.35.0) |
| Docker image | `ghcr.io/jmagar/cortex` canonical + Docker Hub mirror |
| GitHub repo | Rename `jmagar/syslog-mcp` → `jmagar/cortex` |
| Internal depth | Full: modules, types, config sections |
| Migration | Hard break — no deprecation shims |
| Execution | Script-assisted single PR |

---

## Identity Changes

### External surfaces

| Surface | Before | After |
|---|---|---|
| Crate name | `syslog-mcp` | `cortex` |
| Binary | `syslog` | `cortex` + `cx` alias |
| MCP tool | `syslog` | `cortex` |
| Env prefix | `SYSLOG_MCP_*` | `CORTEX_*` |
| Docker image | `jmagar/syslog-mcp` | `ghcr.io/jmagar/cortex` |
| GitHub repo | `jmagar/syslog-mcp` | `jmagar/cortex` |
| Version | 0.35.0 | 1.0.0 |

### Internal renames

| What | Before | After |
|---|---|---|
| Protocol module | `src/syslog/` | `src/receiver/` |
| Module file | `src/syslog.rs` | `src/receiver.rs` |
| Service type | `SyslogService` | `CortexService` |
| Log entry types | `SyslogEntry`, `SyslogRecord` | `LogEntry`, `LogRecord` |
| Config section | `[syslog.*]` | `[receiver.*]` |
| Config struct | `SyslogConfig` | `ReceiverConfig` |

### What does NOT change

- All 42+ MCP action strings (`search`, `fleet_state`, `heartbeat`, etc.)
- SQLite schema and database format
- HTTP API routes (`/api/v1/*`)
- RFC 3164/5424 wire protocol support
- Docker Compose structure
- Justfile command names
- Beads issue tracker integration

---

## Rationale for key decisions

**`src/syslog/` → `src/receiver/`**: The module receives log streams over multiple protocols (RFC syslog, Docker Engine API, OTLP). "receiver" is protocol-neutral and accurate. "syslog" there referred to the RFC protocol, but with the product renamed the ambiguity becomes confusing.

**`SyslogEntry` → `LogEntry`**: cortex ingests syslog, Docker logs, OTLP, and AI transcripts into a unified record. The struct has always been a generic log entry — the name just hadn't caught up.

**Hard break at v1.0.0**: The deployer (a single homelab) controls all instances. A deprecation window adds complexity with no practical benefit.

**Script-assisted single PR**: The rename is ~95% mechanical string substitution. A script makes every substitution auditable and reproducible. The remaining 5% (semantic renames, file moves) is done by hand after running the script.

**MCP action strings unchanged**: These are the stable API surface that agents depend on. The tool name changes (`syslog` → `cortex`) but all action names stay exactly as-is. Agents need one config update (tool name), not a full action remapping.

---

## Execution Plan

### Phase 1 — Write the rename script

`scripts/rename.sh` performs mechanical substitutions across all non-binary files:

```
SYSLOG_MCP_  → CORTEX_
syslog-mcp   → cortex  (Cargo.toml, docs, Docker)
SyslogService → CortexService
SyslogEntry  → LogEntry
SyslogRecord → LogRecord
SyslogConfig → ReceiverConfig  (scoped to receiver module context)
```

The script excludes:
- `target/` and `.git/`
- The CHANGELOG (updated manually)
- Strings where "syslog" refers to the RFC protocol in comments/docs (manual review)

### Phase 2 — File moves

```
src/syslog/   → src/receiver/
src/syslog.rs → src/receiver.rs
```

Update all `mod syslog` / `use crate::syslog` references to `mod receiver` / `use crate::receiver`.

### Phase 3 — Cargo.toml and binary

```toml
[package]
name = "cortex"

[[bin]]
name = "cortex"
path = "src/main.rs"

[[bin]]
name = "cx"
path = "src/main.rs"   # same entrypoint — Cargo builds two binaries from one source
```

### Phase 4 — MCP tool registration

In `src/mcp/rmcp_server.rs` and `src/mcp/schemas.rs`: rename the tool from `syslog` to `cortex`. Action dispatch table unchanged.

### Phase 5 — Docker and CI

- `docker-compose.yml` / `docker-compose.prod.yml`: image name, container name, service name
- `.github/workflows/`: image tags, release artifact names
- `install.sh`: binary name references
- `server.json`: MCP tool name

### Phase 6 — Docs

- `README.md`: product name, install instructions, binary name examples
- `CLAUDE.md` / `AGENTS.md` / `GEMINI.md`: all references
- `CHANGELOG.md`: add v1.0.0 entry documenting breaking changes
- `docs/`: sweep for `syslog-mcp` and `SYSLOG_MCP_` references

### Phase 7 — Plugin manifests

- `.claude-plugin/`: skill manifests, tool name references
- `plugins/`: any skill files referencing the old binary or tool name
- `mcpb/`: MCP builder config if present

### Phase 8 — Verify

```bash
cargo build --release          # must compile clean
just test                      # all tests pass
just lint                      # no clippy warnings
grep -r "syslog-mcp\|SYSLOG_MCP_\|SyslogService\|SyslogEntry" src/ --include="*.rs"
# expect: zero hits (except RFC protocol comments)
```

### Phase 9 — Release

```bash
# Cargo.toml already has version = "1.0.0"
git tag v1.0.0
git push origin v1.0.0
# GitHub Actions builds ghcr.io/jmagar/cortex:1.0.0
# Then rename repo on GitHub: jmagar/syslog-mcp → jmagar/cortex
```

---

## Deployment migration checklist

For each deployed instance after upgrading to v1.0.0:

1. Update `.env`: rename all `SYSLOG_MCP_*` vars to `CORTEX_*`
2. Update `config.toml`: rename `[syslog.*]` sections to `[receiver.*]`
3. Update agent configs: MCP tool name `syslog` → `cortex`
4. Update `server.json` on MCP clients
5. Update Docker Compose to pull `ghcr.io/jmagar/cortex:1.0.0`
6. Restart service

---

## Out of scope

These are future cortex work, not part of the v1.0.0 rebrand:

- Web frontend / dashboard
- New MCP actions
- Architecture changes to the ingest pipeline
- Extracting cortex into a standalone open-source project
