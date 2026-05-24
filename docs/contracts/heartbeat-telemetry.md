# Heartbeat Telemetry Contract (V1 Pre-Implementation)

## 1. Purpose And Status

This document is the contract for the first-class host heartbeat telemetry
surface. It is derived from
[`docs/superpowers/specs/2026-05-24-heartbeat-telemetry-design.md`](../superpowers/specs/2026-05-24-heartbeat-telemetry-design.md).

Status: **pre-implementation contract**.

The implementation plan MUST treat this file as the source of truth. Any code
PR that changes endpoint names, payload fields, database tables, MCP actions,
auth behavior, retention behavior, or error codes MUST update this contract in
the same PR.

Heartbeat telemetry is not synthetic syslog text. It is sampled host state with
its own ingest path, storage schema, query actions, and retention policy.

## 2. Stability

The v1 heartbeat surface uses these tiers:

| Surface | Tier | Compatibility Rule |
| --- | --- | --- |
| `POST /v1/heartbeats` | experimental until the first working release, then stable | Path, method, auth semantics, and accepted payload fields are additive-only after stabilization. |
| MCP actions in this file | experimental until all three v1 actions ship | Parameters and result fields are additive-only after stabilization. |
| SQLite tables in this file | migration contract | New nullable columns and new tables are additive. Renaming or deleting fields requires a migration note and changelog entry. |
| Agent probe internals | internal | Can change when the emitted heartbeat payload remains compatible. |

## 3. Transport

Heartbeat agents push snapshots to the existing `syslog-mcp` HTTP listener.

### 3.1 Endpoint

```http
POST /v1/heartbeats
Content-Type: application/json
Authorization: Bearer <SYSLOG_MCP_TOKEN>
```

The route is mounted beside OTLP on the same server process.

### 3.2 Auth

The route follows the OTLP trust model:

- When `SYSLOG_MCP_TOKEN` is configured, requests MUST include a valid bearer
  token.
- When no token is configured, loopback-only development MAY be unauthenticated.
- Non-loopback unauthenticated exposure MUST be rejected at startup unless the
  operator explicitly enables no-auth mode.
- OAuth JWT support is not required for v1 heartbeat ingest.

The route MUST NOT accept tokens in query parameters.

### 3.3 Body Limit

The maximum request body size is **256 KiB**.

Oversize requests return:

```json
{
  "error": "payload_too_large"
}
```

with HTTP `413`.

### 3.4 Response

Successful ingest returns HTTP `202 Accepted`:

```json
{
  "ok": true,
  "accepted": 1,
  "heartbeat_id": 12345,
  "received_at": "2026-05-24T14:30:00.000Z"
}
```

The server accepts exactly one heartbeat snapshot per request in v1.

## 4. Agent Operation

The v1 collector is an always-on agent, invoked as:

```bash
syslog heartbeat agent
```

Default agent behavior:

| Setting | Default |
| --- | --- |
| Sample interval | 30 seconds |
| Soft collection budget | 2 seconds |
| Hard collection deadline | 5 seconds |
| Retry buffer | bounded memory buffer |
| Host identity | generated stable host id stored in syslog config |
| Primary target OS | Linux |

The agent MAY emit partial snapshots. It MUST prefer sending partial data over
skipping a heartbeat entirely when cheap probes completed.

The agent MUST bound:

- probe execution time,
- number of per-interface rows,
- number of per-disk rows,
- number of top processes,
- number of container details,
- retry buffer size.

## 5. Heartbeat Request Schema

The request body is a JSON object.

Required top-level fields:

```json
{
  "schema_version": 1,
  "host": {},
  "sample": {},
  "agent": {},
  "cpu": {},
  "memory": {},
  "disks": [],
  "networks": [],
  "processes": {},
  "containers": {}
}
```

Unknown top-level fields are rejected.

### 5.1 Host

```json
{
  "host_id": "01jyn5j7n3h9wc0x0mx2x2fb9c",
  "hostname": "tootie",
  "os": "linux",
  "kernel": "6.8.0-60-generic",
  "architecture": "x86_64",
  "boot_id": "9f7f0b8e-f9b7-44bd-92c4-9ab66f43d80a",
  "machine_id_hash": "sha256:3c0e...",
  "timezone": "America/New_York"
}
```

Required:

- `host_id`
- `hostname`
- `os`
- `architecture`
- `boot_id`

Rules:

- `host_id` is the primary identity for heartbeat rows.
- `hostname` is display/filter metadata and is self-reported.
- Raw `/etc/machine-id` MUST NOT be sent. If machine id is used, it is sent as
  a one-way hash.

### 5.2 Sample

```json
{
  "sequence": 42,
  "sampled_at": "2026-05-24T14:29:59.250Z",
  "uptime_secs": 86400,
  "monotonic_ms": 86400000,
  "collection_ms": 37,
  "partial": false,
  "probe_errors": [],
  "skipped_probes": []
}
```

Required:

- `sequence`
- `sampled_at`
- `uptime_secs`
- `collection_ms`
- `partial`

Rules:

- `sequence` increases monotonically per host agent process.
- A lower `sequence` with a new `boot_id` indicates reboot or agent restart.
- `probe_errors` entries are bounded strings and MUST NOT contain secrets.

### 5.3 Agent

```json
{
  "version": "0.31.0",
  "mode": "always_on",
  "interval_secs": 30,
  "push_latency_ms": 12,
  "retry_backlog": 0
}
```

Required:

- `version`
- `mode`
- `interval_secs`

`mode` is currently fixed to `always_on`.

### 5.4 CPU

```json
{
  "load1": 0.42,
  "load5": 0.51,
  "load15": 0.49,
  "usage_pct": 11.4,
  "user_pct": 6.1,
  "system_pct": 3.2,
  "iowait_pct": 1.7,
  "steal_pct": 0.0,
  "core_count": 8
}
```

Required:

- `load1`
- `load5`
- `load15`
- `core_count`

Percentage fields are nullable on the first sample because they require a
previous counter snapshot.

### 5.5 Memory

```json
{
  "mem_total_bytes": 33554432000,
  "mem_available_bytes": 21474836480,
  "mem_used_bytes": 12079595520,
  "swap_total_bytes": 8589934592,
  "swap_used_bytes": 0
}
```

Required:

- `mem_total_bytes`
- `mem_available_bytes`
- `swap_total_bytes`
- `swap_used_bytes`

### 5.6 Disks

Each disk entry is either a mount capacity record or a block-device IO record.

Capacity entry:

```json
{
  "kind": "mount",
  "name": "/mnt/cache",
  "fs_type": "xfs",
  "bytes_total": 1000204886016,
  "bytes_free": 500102443008,
  "bytes_used": 500102443008,
  "inodes_total": 488378368,
  "inodes_free": 488000000,
  "readonly": false
}
```

IO entry:

```json
{
  "kind": "block",
  "name": "nvme0n1",
  "read_bytes_per_sec": 1048576,
  "write_bytes_per_sec": 524288,
  "read_ops_per_sec": 20.5,
  "write_ops_per_sec": 8.0,
  "busy_pct": 3.4
}
```

Rules:

- Pseudo filesystems are excluded by default.
- Device and mount arrays MUST be bounded.
- Per-second rates are nullable on the first sample.

### 5.7 Networks

```json
{
  "interface": "eth0",
  "rx_bytes_per_sec": 12000,
  "tx_bytes_per_sec": 8200,
  "rx_packets_per_sec": 90.5,
  "tx_packets_per_sec": 70.2,
  "rx_errors_per_sec": 0,
  "tx_errors_per_sec": 0,
  "rx_drops_per_sec": 0,
  "tx_drops_per_sec": 0,
  "link_state": "up",
  "speed_mbps": 1000
}
```

Rules:

- Loopback is excluded by default.
- Interface arrays MUST be bounded.
- Per-second rates are nullable on the first sample.

### 5.8 Processes

```json
{
  "total": 312,
  "running": 2,
  "sleeping": 302,
  "zombies": 0,
  "top": [
    {
      "pid": 1234,
      "name": "postgres",
      "cpu_pct": 7.5,
      "rss_bytes": 2147483648,
      "state": "S"
    }
  ]
}
```

Required:

- `total`
- `zombies`

Rules:

- `top` is optional and collected on a slower schedule.
- Full command lines, environment variables, usernames, open files, and network
  connections are excluded by default.

### 5.9 Containers

```json
{
  "runtime": "docker",
  "reachable": true,
  "running": 42,
  "exited": 3,
  "restarting": 0,
  "unhealthy": 1,
  "details": [
    {
      "id": "6d2f...",
      "name": "freshrss",
      "state": "running",
      "health": "unhealthy"
    }
  ]
}
```

Required:

- `reachable`
- `running`
- `exited`
- `restarting`
- `unhealthy`

Rules:

- `details` is optional and bounded.
- Container environment variables are never collected.

### 5.10 Optional GPU

```json
{
  "present": true,
  "devices": [
    {
      "name": "NVIDIA GeForce RTX 3060",
      "utilization_pct": 6,
      "memory_used_bytes": 1073741824,
      "memory_total_bytes": 12884901888,
      "temperature_c": 48,
      "power_watts": 52.5
    }
  ]
}
```

GPU is optional. If the probe command is unavailable or slow, the agent omits
the `gpu` field and records the skipped probe.

## 6. Storage Contract

The implementation creates dedicated heartbeat tables. Table and column names
below are normative for v1 unless this contract is updated before code lands.

### 6.1 `host_heartbeats`

```sql
CREATE TABLE IF NOT EXISTS host_heartbeats (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id         TEXT NOT NULL,
    hostname        TEXT NOT NULL,
    source_ip       TEXT NOT NULL DEFAULT '',
    sampled_at      TEXT NOT NULL,
    received_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    boot_id         TEXT NOT NULL,
    uptime_secs     INTEGER NOT NULL,
    sequence        INTEGER NOT NULL,
    collection_ms   INTEGER NOT NULL,
    push_latency_ms INTEGER,
    partial         INTEGER NOT NULL DEFAULT 0,
    agent_version   TEXT NOT NULL,
    os              TEXT NOT NULL,
    kernel          TEXT,
    architecture    TEXT NOT NULL,
    metadata_json   TEXT,
    UNIQUE(host_id, boot_id, sequence)
);
```

Required indexes:

```sql
CREATE INDEX IF NOT EXISTS idx_host_heartbeats_host_sampled
    ON host_heartbeats(host_id, sampled_at);

CREATE INDEX IF NOT EXISTS idx_host_heartbeats_received
    ON host_heartbeats(received_at);

CREATE INDEX IF NOT EXISTS idx_host_heartbeats_hostname_sampled
    ON host_heartbeats(hostname, sampled_at);
```

### 6.2 Metric Tables

Metric tables reference `host_heartbeats(id)` by `heartbeat_id`.

Foreign key enforcement is not required in v1 because the existing SQLite
contract does not globally enable `PRAGMA foreign_keys`.

Required tables:

- `heartbeat_cpu`
- `heartbeat_memory`
- `heartbeat_disks`
- `heartbeat_network`
- `heartbeat_processes`
- `heartbeat_containers`

Optional table:

- `heartbeat_gpu`

Each table MUST include `heartbeat_id INTEGER NOT NULL` and enough typed fields
to satisfy the request/result schemas in this contract. Extra probe-specific
metadata belongs in bounded `metadata_json`.

### 6.3 Retention

Heartbeat retention is independent from log retention.

Default high-resolution retention: **14 days**.

Retention deletes heartbeat rows by `received_at`, oldest first. The delete
path MUST be chunked so it does not hold SQLite's write lock long enough to
starve log ingest.

## 7. Derived Server Signals

The server computes these normalized flags:

- `cpu_pressure`
- `memory_pressure`
- `swap_pressure`
- `disk_capacity_pressure`
- `disk_io_pressure`
- `network_error_pressure`
- `container_unhealthy`
- `collector_partial`
- `heartbeat_late`
- `host_rebooted`
- `clock_skew`

Agent-supplied local flags MAY be stored, but server-computed flags are the
source of truth for fleet views and correlation.

Default late heartbeat threshold: no accepted heartbeat for **2.5x the agent's
declared interval**.

## 8. MCP Actions

Heartbeat MCP actions dispatch through the existing `syslog` tool using the
`action` argument.

All actions return the existing MCP envelope style:

```json
{
  "ok": true,
  "result": {}
}
```

On error:

```json
{
  "ok": false,
  "error": {
    "code": "invalid_params",
    "message": "..."
  }
}
```

### 8.1 `host_state`

Purpose: return latest heartbeat state or a bounded summary for one host.

Params:

```json
{
  "action": "host_state",
  "host": "tootie",
  "since": "2026-05-24T14:00:00Z",
  "limit": 20
}
```

Rules:

- `host` may match `host_id` or `hostname`.
- When `since` is omitted, return the latest heartbeat.
- `limit` defaults to 1 and is capped at 100.

Result:

```json
{
  "host_id": "01jyn5j7n3h9wc0x0mx2x2fb9c",
  "hostname": "tootie",
  "latest": {},
  "samples": [],
  "derived": {
    "heartbeat_late": false,
    "host_rebooted": false,
    "cpu_pressure": false,
    "memory_pressure": false,
    "disk_capacity_pressure": false
  }
}
```

### 8.2 `fleet_state`

Purpose: return latest heartbeat status for all known heartbeat hosts.

Params:

```json
{
  "action": "fleet_state",
  "include_ok": true,
  "sort": "pressure"
}
```

Rules:

- `include_ok` defaults to true.
- `sort` values: `pressure`, `freshness`, `hostname`.

Result:

```json
{
  "hosts": [
    {
      "host_id": "01jyn5j7n3h9wc0x0mx2x2fb9c",
      "hostname": "tootie",
      "last_heartbeat_at": "2026-05-24T14:30:00Z",
      "status": "ok",
      "pressure": ["container_unhealthy"],
      "partial": false
    }
  ],
  "summary": {
    "total": 1,
    "ok": 1,
    "late": 0,
    "partial": 0,
    "pressure": 0
  }
}
```

### 8.3 `correlate_state`

Purpose: return logs plus heartbeat summaries around a time window.

Params:

```json
{
  "action": "correlate_state",
  "reference_time": "2026-05-24T14:30:00Z",
  "window_minutes": 10,
  "host": "tootie",
  "severity_min": "warning",
  "limit": 100
}
```

Rules:

- `reference_time` is required.
- `window_minutes` defaults to 10 and is capped at 120.
- `host` is optional; omitted means correlate across all hosts.
- `limit` caps returned log rows and defaults to 100.

Result:

```json
{
  "window": {
    "from": "2026-05-24T14:20:00Z",
    "to": "2026-05-24T14:40:00Z"
  },
  "hosts": [
    {
      "host_id": "01jyn5j7n3h9wc0x0mx2x2fb9c",
      "hostname": "tootie",
      "heartbeat_summary": {
        "samples": 20,
        "partial_samples": 0,
        "max_cpu_pct": 55.2,
        "min_mem_available_bytes": 2147483648,
        "pressure": []
      },
      "logs": []
    }
  ],
  "truncated": false
}
```

## 9. CLI/API Parity

Every MCP action in section 8 needs CLI and REST API parity unless the
implementation plan explicitly documents why parity is deferred.

Expected CLI shape:

```bash
syslog host-state --host tootie --json
syslog fleet-state --json
syslog correlate-state --reference-time 2026-05-24T14:30:00Z --window-minutes 10 --json
```

Expected REST shape:

- `GET /api/host-state`
- `GET /api/fleet-state`
- `GET /api/correlate-state`

REST endpoints are mounted only when `SYSLOG_API_ENABLED=true`, matching the
existing API convention.

## 10. Error Codes

HTTP ingest errors:

| HTTP | Error | Meaning |
| --- | --- | --- |
| 400 | `invalid_payload` | JSON is malformed, unknown fields are present, or required fields are missing. |
| 401 | `unauthorized` | Missing or invalid bearer token. |
| 413 | `payload_too_large` | Body exceeds 256 KiB. |
| 409 | `duplicate_heartbeat` | The same `(host_id, boot_id, sequence)` was already accepted. |
| 503 | `storage_unavailable` | DB write path unavailable or backpressured. |

MCP/action errors:

| Code | Meaning |
| --- | --- |
| `invalid_params` | Bad host, timestamp, window, limit, or enum value. |
| `not_found` | Requested host has no heartbeat data. |
| `heartbeat_unavailable` | Heartbeat tables are not initialized or feature is disabled. |
| `internal` | Unexpected server-side failure. |

## 11. Privacy Contract

V1 heartbeat collection MUST NOT collect these by default:

- environment variables,
- process command lines,
- usernames,
- open files,
- network connection tuples,
- container environment variables,
- raw machine id,
- secrets or auth material.

If any future mode collects those fields, it must be explicitly opt-in and
called out in this contract.

## 12. Future Extension Points

Reserved but not implemented in v1:

- mesh reachability fields,
- relay/spool mode,
- disk-backed heartbeat retry queue,
- OTLP metrics export/import mapping,
- vector embeddings over heartbeat/log incidents,
- non-Linux collectors.

These may be added without changing the v1 endpoint if the existing payload
remains accepted.
