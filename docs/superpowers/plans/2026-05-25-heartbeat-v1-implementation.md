# Heartbeat V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Epic:** `syslog-mcp-h2kq` - Unified host agent and heartbeat telemetry.

**Goal:** Ship the first usable heartbeat telemetry slice: structured storage, `POST /v1/heartbeats`, and `host_state`. This plan intentionally does not implement the old WebSocket agent roadmap first. It proves the host-state data plane before adding bidirectional control-plane work.

**Source Of Truth:** [docs/contracts/heartbeat-telemetry.md](../../contracts/heartbeat-telemetry.md)

**Design Input:** [docs/superpowers/specs/2026-05-24-heartbeat-telemetry-design.md](../specs/2026-05-24-heartbeat-telemetry-design.md)

**V1-first beads:**
- `syslog-mcp-h2kq.2` - Implement heartbeat schema, retention, and storage recovery.
- `syslog-mcp-h2kq.3` - Implement `POST /v1/heartbeats` ingest.
- `syslog-mcp-h2kq.4` - Implement `host_state` MCP action.

**Later beads:**
- `syslog-mcp-h2kq.5` - Implement heartbeat agent core with fake probes.
- `syslog-mcp-h2kq.6` - Add cheap Linux heartbeat probes and setup integration.
- `syslog-mcp-h2kq.7` - Defer broader heartbeat and agent-control surfaces.

---

## Non-Negotiable Constraints

- The next schema migration must be based on `src/db/pool.rs` `KNOWN_SCHEMA_VERSION`. At the time of this plan, the code constant is `14`, so heartbeat storage should land as migration `15` unless another migration lands first.
- `POST /v1/heartbeats` uses a **256 KiB route-local ingest limit**. This is an HTTP request limit from agent to server, not permission to return 256 KiB to MCP clients or LLM-facing tools.
- MCP responses must return bounded summaries and bounded sample arrays. Do not expose raw full heartbeat ingest payloads by default.
- Duplicate `(host_id, boot_id, sequence)` retries are idempotent: return `202 Accepted` with `accepted: 0` and the existing `heartbeat_id`; do not insert child metric rows twice.
- `host_id` is authoritative. Hostname lookup is a convenience fallback only when it resolves to exactly one host id. Multiple matches return `ambiguous_host`.
- No raw machine id or plain unsalted machine-id hash is collected by default.
- Retention and storage recovery must explicitly delete heartbeat metric child rows because global SQLite foreign-key enforcement is not assumed.
- `fleet_state`, `correlate_state`, REST parity, CLI parity beyond `host_state`, WebSocket `/ws/agent`, durable log streaming, and server-driven probes are deferred unless a later plan intentionally pulls them forward.

---

## Current Code Anchors

**DB and storage**
- `src/db/pool.rs` owns `KNOWN_SCHEMA_VERSION`, migration sequencing, table/index DDL, FTS rebuilds, and storage-budget enforcement entrypoints.
- `src/db/maintenance.rs` and nearby DB modules own retention and cleanup helpers. Heartbeat cleanup must mirror chunked log deletion patterns without coupling heartbeat retention to log retention.
- Tests live as sidecar module-local tests, e.g. `src/db/*_tests.rs`.

**HTTP routing and auth**
- `src/runtime.rs` builds the shared HTTP runtime state and exposes `otlp_router()` and `mcp_state()`.
- `src/otlp.rs` is the closest route pattern: sibling `/v1/logs` router, `RequestBodyLimitLayer`, `ConnectInfo<SocketAddr>`, bearer parsing via `lab_auth::middleware::{parse_bearer_token, tokens_equal}`, and redacted unauthorized logging.
- `src/main.rs` composes the HTTP listener. Mount heartbeat beside MCP and OTLP, not inside MCP.

**MCP action surface**
- `src/mcp/actions.rs` is the authoritative action registry and scope table.
- `src/mcp/schemas.rs` exposes the single `syslog` tool schema and must include `host_state` parameters.
- `src/mcp/tools.rs` dispatches `action` values and calls `SyslogService`.
- `src/app/models.rs`, `src/app/service.rs`, and `src/db/queries.rs` are the expected request/response/service/query path.

---

## Task 1: Implement Heartbeat Storage

**Bead:** `syslog-mcp-h2kq.2`

**Files:**
- Modify: `src/db/pool.rs`
- Modify/add: `src/db/maintenance.rs` or equivalent DB cleanup module
- Modify/add: `src/db/queries.rs` for heartbeat query helpers if local style fits
- Add sidecar tests beside touched DB modules

- [ ] Increment the schema version from the current `KNOWN_SCHEMA_VERSION` and add a guarded heartbeat migration.
- [ ] Create `host_heartbeats` with the contract columns and `UNIQUE(host_id, boot_id, sequence)`.
- [ ] Create required metric tables: `heartbeat_cpu`, `heartbeat_memory`, `heartbeat_disks`, `heartbeat_network`, `heartbeat_processes`, and `heartbeat_containers`.
- [ ] Add `heartbeat_id` indexes to every metric table.
- [ ] Add latest/window indexes needed by `host_state`: at minimum host/sample time, received time, and hostname/sample time indexes from the contract.
- [ ] Add typed insert helpers that insert parent and children in one transaction.
- [ ] Implement duplicate detection that returns the existing parent id without inserting children again.
- [ ] Implement chunked heartbeat retention, defaulting to the contract retention window.
- [ ] Extend storage-budget recovery so heartbeat rows and child metrics can be reclaimed when whole-database pressure comes from heartbeat tables.
- [ ] Add tests for migration idempotence, indexes, duplicate insert idempotence, child cleanup, and storage recovery.

**Do not do in this bead:**
- Do not add `fleet_state` or `correlate_state` indexes beyond what the implementation actually uses.
- Do not rely on `PRAGMA foreign_keys` for child cleanup.

---

## Task 2: Implement `POST /v1/heartbeats`

**Bead:** `syslog-mcp-h2kq.3`

**Files:**
- Add: `src/heartbeat.rs` or `src/heartbeat/mod.rs`
- Modify: `src/runtime.rs`
- Modify: `src/main.rs`
- Add sidecar tests for route/auth/body/validation behavior

- [ ] Define request structs with `serde(deny_unknown_fields)` for the v1 payload.
- [ ] Validate required fields, bounded arrays, bounded strings, and RFC3339 timestamps before DB writes.
- [ ] Add `HeartbeatState` containing DB pool/service handle, token/auth policy, and any counters needed for health.
- [ ] Mount `POST /v1/heartbeats` beside OTLP with `RequestBodyLimitLayer::new(256 * 1024)` on the heartbeat router only.
- [ ] Reuse OTLP-style bearer parsing and constant-time token comparison.
- [ ] Reject query-parameter tokens.
- [ ] Capture `source_ip` from `ConnectInfo<SocketAddr>`.
- [ ] Return structured errors: `invalid_payload`, `unauthorized`, `payload_too_large`, and `storage_unavailable`.
- [ ] Return `202 Accepted` with `accepted: 1` for new rows and `accepted: 0` for duplicates.
- [ ] Add tests for valid ingest, missing auth, malformed auth, wrong token, query token, malformed JSON, unknown fields, missing required fields, duplicate retry, source IP capture, and body sizes around the route limit.

**Do not do in this bead:**
- Do not mount heartbeat under the MCP router.
- Do not use OTLP's 4 MiB body limit.
- Do not log tokens or raw payloads.

---

## Task 3: Implement `host_state`

**Bead:** `syslog-mcp-h2kq.4`

**Files:**
- Modify: `src/app/models.rs`
- Modify: `src/app/service.rs`
- Modify: `src/db/queries.rs`
- Modify: `src/mcp/actions.rs`
- Modify: `src/mcp/schemas.rs`
- Modify: `src/mcp/tools.rs`
- Add/update sidecar tests beside touched modules

- [ ] Add `HostStateRequest` with `host`, optional `since`, and optional `limit`.
- [ ] Normalize `limit` to default `1`, cap `100`.
- [ ] Resolve host lookup by `host_id` first.
- [ ] If no host id matches, resolve hostname only when exactly one host id exists for that hostname.
- [ ] Return `ambiguous_host` when hostname maps to multiple host ids.
- [ ] Query latest heartbeat when `since` is absent.
- [ ] Query bounded samples when `since` is present.
- [ ] Return bounded summaries with derived flags from server-side calculations.
- [ ] Register `host_state` in `src/mcp/actions.rs` as a read action.
- [ ] Add schema fields for `host`, `since`, and `limit` with descriptions that mention `host_state`.
- [ ] Add tests for latest-by-host-id, unique hostname fallback, ambiguous hostname, not found, since window, limit cap, and no raw full payload exposure.

**Do not do in this bead:**
- Do not implement `fleet_state`.
- Do not implement `correlate_state`.
- Do not expose unbounded child rows.

---

## Task 4: Implement Agent Core With Fake Probes

**Bead:** `syslog-mcp-h2kq.5`

**Files:**
- Modify CLI parsing modules for `syslog heartbeat agent`
- Add heartbeat agent module(s), keeping server ingest modules separate where practical
- Add tests for loop/backoff/payload assembly

- [ ] Add the `syslog heartbeat agent` command.
- [ ] Generate and persist a stable `host_id` in syslog configuration or appdata.
- [ ] Build valid v1 payloads using fake/stub probes.
- [ ] Implement 30-second default interval, configurable endpoint/token, hard sample deadline, partial snapshot handling, and bounded retry/backoff.
- [ ] Treat `accepted: 0` duplicate responses as success.
- [ ] Ensure agent payloads do not include raw machine-id or plain machine-id hashes by default.

---

## Task 5: Add Cheap Linux Probes And Setup

**Bead:** `syslog-mcp-h2kq.6`

**Files:**
- Add Linux probe modules using `/proc` and `/sys` fixtures
- Modify binary-owned setup modules under `src/setup/`
- Keep `scripts/plugin-setup.sh` as a thin adapter

- [ ] Implement cheap Linux probes for CPU, memory, disk capacity, network counters/errors, disk IO, process counts, and container summary.
- [ ] Bound probe time and row counts.
- [ ] Record `skipped_probes` and `probe_errors`.
- [ ] Add setup integration for a separate heartbeat collector service with explicit working directory and config.
- [ ] Add fixture tests for probes and dry-run/setup tests for service generation.

---

## Task 6: Record Deferred Surfaces

**Bead:** `syslog-mcp-h2kq.7`

**Files:**
- Modify docs and tracker comments only unless implementation scope changes

- [ ] Leave explicit tracker notes for `fleet_state`, `correlate_state`, REST parity, CLI parity beyond `host_state`, WebSocket agent mode, durable log streaming, and probe registry.
- [ ] Document which indexes/cache structures are required before `fleet_state` and `correlate_state`.
- [ ] Document that WebSocket `agent.heartbeat` is connection liveness, not sampled host-state telemetry.

---

## Verification And Closeout

- [ ] Run `cargo fmt`.
- [ ] Run targeted tests for touched modules while iterating.
- [ ] Run `cargo clippy -- -D warnings`.
- [ ] Run `cargo test`.
- [ ] Run `git diff --check`.
- [ ] Update and close completed child beads.
- [ ] Push Beads/Dolt state.
- [ ] Commit and push the worktree branch.
- [ ] Open or update the PR when implementation is ready for review.
