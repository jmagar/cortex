# Hive Rebrand Contract

This release renames the product from `syslog-mcp` to **Hive** while keeping
`syslog` as the protocol name for RFC 3164/5424 ingest, parsing, facilities,
severities, and listener configuration.

## Identity Matrix

| Surface | Current name/value | New name/value | Classification | Compatibility policy | Precedence rule | Owner | Validation gate | Sunset |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Product display | syslog-mcp | Hive | product | Rename all user-facing product copy | Hive is primary | `.5` | docs grep review | indefinite |
| Repository | `jmagar/syslog-mcp` | `jmagar/hive` | artifact | Docs may mention old repo only in migration/history | Hive is primary | `.5` / `.6` | PR/release metadata review | indefinite |
| Cargo package | `syslog-mcp` | `hive-mcp` | artifact | Breaking package rename | `hive-mcp` only for package metadata | `.2` | `cargo metadata --no-deps` | none |
| Rust import namespace | `syslog_mcp` | `hive_mcp` | artifact | Breaking import rename | `hive_mcp` only | `.2` | integration tests compile | none |
| Primary binary | `syslog` | `hive` | artifact | Keep `syslog` binary alias this release | `hive` documented first | `.2` | `cargo run --bin hive -- --help`; `cargo run --bin syslog -- --help` | transition |
| MCP tool | `syslog` | `hive` | MCP/plugin API | Keep `syslog` tool alias this release | `hive` listed first | `.4` | MCP tools/list and tools/call tests | transition |
| MCP read scope | `syslog:read` | `hive:read` | MCP/plugin API | Accept both scopes this release | `hive:read` documented first | `.4` | OAuth scope tests | transition |
| MCP admin scope | `syslog:admin` | `hive:admin` | MCP/plugin API | Accept both scopes this release | `hive:admin` documented first | `.4` | OAuth scope tests | transition |
| MCP schema resource | `syslog://schema/mcp-tool` | `hive://schema/mcp-tool` | MCP/plugin API | Keep legacy resource readable | `hive://` listed first | `.4` | resources/read tests | transition |
| Plugin manifest | `.claude-plugin` name `syslog` | `hive` | MCP/plugin API | Breaking plugin identity rename with docs | Hive metadata only | `.4` / `.5` | JSON validation and sensitive-field scan | none |
| Docker service | `syslog-mcp` | `hive-mcp` | runtime/deploy | Rename service with data migration notes | Hive default | `.3` | `docker compose config` | none |
| Docker image | `ghcr.io/jmagar/syslog-mcp` | `ghcr.io/jmagar/hive-mcp` | artifact | Old image remains historical; new docs use Hive | Hive image primary | `.5` / `.6` | workflow/server metadata review | none |
| Docker data volume | `syslog-mcp-data` | `hive-mcp-data` | data artifact | Must not silently abandon existing data | Existing explicit user value wins | `.3` | compose upgrade verification | transition |
| Docker network | `syslog-mcp` | `hive-mcp` | runtime/deploy | Rename default only; explicit `DOCKER_NETWORK` wins | explicit env wins | `.3` | `docker compose config` | indefinite |
| MCP env vars | `SYSLOG_MCP_*` | `HIVE_MCP_*` | runtime/deploy | Accept legacy aliases | `HIVE_MCP_*` overrides `SYSLOG_MCP_*` | `.3` | config tests | transition |
| API env vars | `SYSLOG_API_*` | `HIVE_API_*` | runtime/deploy | Accept legacy aliases | `HIVE_API_*` overrides `SYSLOG_API_*` | `.3` | config tests | transition |
| Docker ingest env vars | `SYSLOG_DOCKER_*` | `HIVE_DOCKER_*` | runtime/deploy | Accept legacy aliases | `HIVE_DOCKER_*` overrides `SYSLOG_DOCKER_*` | `.3` | config tests | transition |
| Syslog listener env vars | `SYSLOG_HOST`, `SYSLOG_PORT`, `SYSLOG_*` ingest knobs | unchanged | protocol term | Remain canonical protocol settings | syslog protocol settings remain canonical | `.3` | config tests | indefinite |
| Database filename | `/data/syslog.db` | `/data/hive.db` only with explicit migration | data artifact | Do not silently rename in this release | existing `*_DB_PATH` wins | `.3` | compose upgrade verification | indefinite |

## Compatibility Matrix

| Legacy surface | Compatibility behavior |
| --- | --- |
| `syslog` binary | Runs the same server as `hive` in this release. |
| `syslog` MCP tool | Dispatches to the same action implementation as `hive`. |
| `syslog://schema/mcp-tool` | Returns the same schema resource as `hive://schema/mcp-tool`. |
| `syslog:read` / `syslog:admin` | Accepted alongside `hive:read` / `hive:admin`. |
| `SYSLOG_MCP_*` | Accepted as lower-precedence aliases for `HIVE_MCP_*`. |
| `SYSLOG_API_*` | Accepted as lower-precedence aliases for `HIVE_API_*`. |
| `SYSLOG_DOCKER_*` | Accepted as lower-precedence aliases for `HIVE_DOCKER_*`. |
| `SYSLOG_HOST` / `SYSLOG_PORT` | Remain canonical because they describe the syslog protocol listener. |

## Data Preservation Contract

Docker and config migration must never silently switch an existing operator to
an empty DB. Existing explicit `SYSLOG_MCP_DATA_VOLUME`, `HIVE_MCP_DATA_VOLUME`,
`SYSLOG_MCP_DB_PATH`, or `HIVE_MCP_DB_PATH` values win over defaults. The default
DB path remains `/data/syslog.db` for this release because the filename is a data
artifact and silent renames are riskier than stale branding. Any future move to
`/data/hive.db` must include an explicit copy/rename step and rollback path.

## Auth and Scope Contract

`hive:read` and `hive:admin` are the primary scopes. `syslog:read` and
`syslog:admin` remain valid transition aliases. `hive:admin` and
`syslog:admin` both satisfy read actions. Unknown actions remain denied before
database work runs.

OAuth and static-token error messages must name `HIVE_*` variables first and
legacy `SYSLOG_*` variables second when both forms exist.

## Plugin Contract

`.claude-plugin/plugin.json` is a required release surface. It must use Hive
branding for name, description, repository, and keywords while keeping syslog
wording for protocol listener settings. Every user config field whose name,
title, or description implies a token, secret, password, credential, private
key, or API key must include `"sensitive": true`.

## Rollback Contract

Rollback keeps using the existing data path or explicit data volume. Operators
who move from Hive back to the previous syslog-mcp image must set the old image
and keep the same data volume or DB path. MCP clients can temporarily call the
legacy `syslog` tool and use legacy scopes while rollback is in progress.

## File Inventory

The initial inventory command was:

```bash
rg -n "syslog-mcp|syslog_mcp|SYSLOG_MCP|syslog://|syslog:read|syslog:admin|name = \"syslog\"|\"name\": \"syslog\"" .
```

High-signal owners from that inventory:

| Path group | Owner | Action |
| --- | --- | --- |
| `Cargo.toml`, `Cargo.lock`, `src/lib.rs`, integration tests | `.2` | Rename package/import namespace and binaries. |
| `src/config.rs`, `src/config_tests.rs`, `.env.example` | `.3` | Add Hive env aliases and precedence tests. |
| `docker-compose.yml`, `config/Dockerfile`, Docker docs | `.3` / `.5` | Hive deployment defaults with data-preserving validation. |
| `src/mcp/schemas.rs`, `src/mcp/tools.rs`, `src/mcp/rmcp_server.rs`, MCP tests | `.4` | Primary `hive` tool/resource/scopes plus legacy aliases. |
| `.claude-plugin/plugin.json`, `plugins/`, `scripts/plugin-setup.sh`, `scripts/validate-marketplace.sh` | `.4` / `.5` | Hive plugin identity and marketplace validation. |
| `README.md`, `docs/`, `CLAUDE.md`, `server.json`, `CHANGELOG.md` | `.5` | Public docs, release metadata, migration notes. |
| `.github/workflows/`, `scripts/bump-version.sh`, `scripts/check-version-sync.sh` | `.5` / `.6` | Release and version sync alignment. |

Remaining `syslog` references after implementation must be classified as one of:

- protocol term
- legacy compatibility alias
- migration example
- historical changelog/review reference
- intentional external reference
