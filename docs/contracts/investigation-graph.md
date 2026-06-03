# Investigation Graph Contract

## 1. Purpose And Status

This document is the compatibility contract for the Cortex investigation graph.
It is derived from [`docs/specs/investigation-graph.md`](../specs/investigation-graph.md).

Status: **implemented v1 contract**.

Any change to graph table semantics, entity types, relationship types, reason
codes, trust levels, evidence fields, rebuild lifecycle, CLI command names, MCP
action shape, REST graph route shape, or projection status semantics MUST update
this contract in the same change.

## 2. Stability Tiers

| Surface | Tier | Compatibility Rule |
| --- | --- | --- |
| Entity type strings | stable | Additive only unless a migration and changelog explain a rename. |
| Relationship type strings | stable | Additive only unless a migration and changelog explain a rename. |
| Reason code strings | stable | Additive only unless a migration and changelog explain a rename. |
| Trust level strings | stable | Additive only. Meaning must not be weakened silently. |
| Graph table names | migration contract | Schema changes require migration tests and changelog entry. |
| Public response fields | additive stable | Existing fields remain unless versioned or explicitly deprecated. |
| CLI command names | stable | Existing commands remain; aliases may be added. |
| Human formatting | ergonomic | May change when JSON contract is preserved. |
| Projection algorithm | internal with observable outputs | May change if entity/relationship/evidence semantics remain compatible. |

## 3. Projection Lifecycle Contract

### 3.1 Rebuild

```bash
cortex graph rebuild [--json]
```

Rebuild MUST:

- be explicit; public lookup commands MUST NOT implicitly trigger it,
- be single-flight,
- publish `projection_status = "building"` at start,
- reset stale count/progress fields at start,
- publish in-flight `source_row_count` and `last_chunk_count`,
- use staging tables before replacing public graph tables,
- preserve public graph tables until the final swap,
- set `projection_status = "ready"` after a successful swap,
- set `projection_status = "failed"` and a redacted `last_error` on failure,
- update `last_started_at`, `last_completed_at`, `source_watermark`,
  `entity_count`, `relationship_count`, `evidence_count`, `last_runtime_ms`,
  and `last_chunk_count`.

If another rebuild is already running, the command returns an `already_running`
outcome and MUST NOT start a second rebuild.

### 3.2 Status

```bash
cortex graph status [--json]
```

Status MUST return:

```json
{
  "projection_status": "ready",
  "last_started_at": "2026-06-02T22:51:14.513Z",
  "last_completed_at": "2026-06-02T23:08:50.712Z",
  "source_watermark": "logs:3750550;heartbeats:5738;signatures:3028",
  "source_row_count": 3713029,
  "entity_count": 28402,
  "relationship_count": 23369,
  "evidence_count": 54365,
  "is_degraded": false,
  "last_error": null,
  "last_runtime_ms": 1055784,
  "last_chunk_count": 371
}
```

Example values are illustrative. Field names and types are contractual.

Valid `projection_status` values:

| Value | Meaning |
| --- | --- |
| `never_built` | Projection tables exist but have not been populated. |
| `building` | A rebuild is in progress; counts may be progress counters. |
| `ready` | Last rebuild completed successfully. |
| `stale` | Projection is known stale. Reserved for future use. |
| `failed` | Last rebuild failed; `last_error` carries a redacted reason. |

## 4. Vocabulary Contract

### 4.1 Entity Types

These entity type strings are valid:

```text
host
container
service
app
source_ip
ai_project
ai_session
error_signature
```

Unknown entity types MUST be rejected on public lookup surfaces.

### 4.2 Relationship Types

These relationship type strings are valid:

```text
observed_as
runs_on
emitted_by
worked_on
matches_signature
```

### 4.3 Trust Levels

These trust level strings are valid:

```text
verified
claimed
inferred
correlated
```

Meanings:

| Trust | Meaning |
| --- | --- |
| `verified` | Derived from a field Cortex treats as an identity source for this relationship. |
| `claimed` | Derived from a sender-claimed field such as syslog hostname. |
| `inferred` | Derived from metadata or summaries that imply a relationship but are not identity proof. |
| `correlated` | Reserved for future query-time correlation. |

Trust levels MUST NOT be upgraded without stronger source evidence.

### 4.4 Source Kinds

These source kind strings are valid:

```text
log
heartbeat
ai_session_rollup
source_inventory
app_inventory
error_signature
```

The current v1 projection emits evidence rows for:

```text
log
error_signature
```

Heartbeat currently contributes host entities and aliases, not relationship
evidence rows.

### 4.5 Reason Codes

These reason code strings are valid:

```text
syslog_claimed_hostname
log_app_name
docker_container_id
docker_service_label
ai_session_project
heartbeat_host_state
error_signature_match
```

`heartbeat_host_state` is reserved for heartbeat-derived relationship evidence.
The current v1 implementation uses heartbeat rows for host identity/aliases.

## 5. Entity Construction Contract

Entity keys are normalized before storage. Normalization trims whitespace,
rejects empty values, and canonicalizes keys for matching. Display labels retain
the source-facing value where practical.

Entities are unique by:

```text
(entity_type, canonical_key)
```

Repeated observations update first/last seen timestamps and keep the existing
entity id stable within a rebuild.

## 6. Relationship Construction Contract

Relationships are unique by:

```text
src_entity_id:relationship_type:dst_entity_id
```

Repeated observations of the same relationship update:

- first seen timestamp,
- last seen timestamp,
- max confidence,
- evidence count,
- representative evidence rows.

Relationship direction is meaningful and MUST be preserved.

## 7. Current Edge Rules

### 7.1 Source Observed As Claimed Host

Input:

- `logs.source_ip`
- `logs.hostname`

Output:

```text
source_ip observed_as host
```

Fields:

| Field | Value |
| --- | --- |
| `relationship_type` | `observed_as` |
| `reason_code` | `syslog_claimed_hostname` |
| `trust_level` | `claimed` |
| `confidence` | `0.6` |
| `evidence.source_kind` | `log` |
| `evidence.source_log_id` | source `logs.id` |
| `evidence.reason_text` | `syslog header hostname claimed by sender` |

### 7.2 App Emitted By Host

Input:

- `logs.app_name`
- `logs.hostname`

Output:

```text
app emitted_by host
```

Fields:

| Field | Value |
| --- | --- |
| `relationship_type` | `emitted_by` |
| `reason_code` | `log_app_name` |
| `trust_level` | `inferred` |
| `confidence` | `0.5` |
| `evidence.metadata_path` | `logs.app_name` |
| `evidence.source_log_id` | source `logs.id` |

### 7.3 AI Session Worked On Project

Input:

- `logs.ai_project`
- `logs.ai_tool`
- `logs.ai_session_id`

Output:

```text
ai_session worked_on ai_project
```

Fields:

| Field | Value |
| --- | --- |
| `relationship_type` | `worked_on` |
| `reason_code` | `ai_session_project` |
| `trust_level` | `verified` |
| `confidence` | `0.9` |
| `evidence.metadata_path` | `logs.ai_project/logs.ai_session_id` |
| `evidence.source_log_id` | source `logs.id` |

### 7.4 Container Runs On Host

Input:

- `logs.source_ip` with `docker://` or `docker-event://`
- `metadata_json.container_id` or `metadata_json.container_name`
- Docker host from metadata or source string

Output:

```text
container runs_on host
```

Fields:

| Field | Value |
| --- | --- |
| `relationship_type` | `runs_on` |
| `reason_code` | `docker_container_id` |
| `trust_level` | `verified` |
| `confidence` | `0.9` |
| `evidence.metadata_path` | `logs.source_ip/metadata_json` |
| `evidence.source_log_id` | source `logs.id` |

### 7.5 Container Runs On Service

Input:

- Docker host,
- `metadata_json.compose_project`,
- `metadata_json.compose_service`.

If `compose_project` is missing, the Docker host is used as the project key.
If `compose_service` is missing, `container_name` may be used as the service
value.

Output:

```text
container runs_on service
```

Fields:

| Field | Value |
| --- | --- |
| `relationship_type` | `runs_on` |
| `reason_code` | `docker_service_label` |
| `trust_level` | `inferred` |
| `confidence` | `0.7` |
| `evidence.metadata_path` | `metadata_json.compose_service` |
| `evidence.source_log_id` | source `logs.id` |

### 7.6 Error Signature Links

Input:

- `error_signatures.signature_hash`
- `error_signatures.normalizer_version`
- `error_signatures.template`
- `error_signatures.sample_app_name`
- `error_signatures.sample_hostname`

Outputs:

```text
app matches_signature error_signature
host matches_signature error_signature
```

Fields:

| Field | App Link | Host Link |
| --- | --- | --- |
| `relationship_type` | `matches_signature` | `matches_signature` |
| `reason_code` | `error_signature_match` | `error_signature_match` |
| `trust_level` | `inferred` | `inferred` |
| `confidence` | `0.7` | `0.5` |
| `evidence.source_kind` | `error_signature` | `error_signature` |
| `evidence.source_signature_hash` | signature hash | signature hash |
| `evidence.metadata_path` | `error_signatures` | `error_signatures` |

## 8. Evidence Contract

Every persisted relationship MUST have evidence.

Evidence fields:

| Field | Type | Rule |
| --- | --- | --- |
| `id` | integer | Stable row id within current projection. |
| `relationship_id` | integer | References `graph_relationships.id`. |
| `evidence_key` | string | Dedupe key for repeated observations. |
| `source_kind` | string | One of the valid source kinds. |
| `source_id` | string | Source-local id string. |
| `source_log_id` | integer/null | Set for evidence derived from `logs`. |
| `source_heartbeat_id` | integer/null | Reserved for heartbeat relationship evidence. |
| `source_signature_hash` | string/null | Set for error signature evidence. |
| `observed_at` | string | RFC3339-ish timestamp from source observation. |
| `reason_code` | string | One of the valid reason codes. |
| `reason_text` | string/null | Short human explanation. |
| `confidence_delta` | number | Rule contribution. |
| `trust_level` | string | Trust level for this evidence. |
| `safe_excerpt` | string/null | Bounded redacted source excerpt. |
| `metadata_path` | string/null | Source field path used for the rule. |
| `evidence_count` | integer | Number of observations represented by this evidence row. |

Public graph responses MUST expose safe evidence only. Raw frames,
unbounded metadata JSON, secrets, and terminal control characters MUST NOT be
emitted through graph evidence fields.

Relationship response fields:

| Field | Type | Rule |
| --- | --- | --- |
| `src_entity_id` | integer | Preserved compatibility id for the source endpoint. |
| `dst_entity_id` | integer | Preserved compatibility id for the destination endpoint. |
| `src_entity` | object/null | Optional compact endpoint summary when available. |
| `dst_entity` | object/null | Optional compact endpoint summary when available. |

Endpoint summaries are additive and compact. They include `id`, `entity_type`,
`canonical_key`, `display_label`, and `trust_level`; they do not embed full
entity source ids or aliases.

Evidence lookup response fields:

| Field | Type | Rule |
| --- | --- | --- |
| `evidence` | object | Safe evidence row from `graph_relationship_evidence`. |
| `relationship` | object | Owning relationship, including compatibility ids and endpoint summaries. |
| `src_entity` | object | Source endpoint summary. |
| `dst_entity` | object | Destination endpoint summary. |
| `source_log_summary` | object/null | Bounded scalar log summary for log-derived evidence when the source row still exists. |
| `missing_source_reason` | string/null | `evidence_source_is_not_a_log` or `source_log_missing_or_retained_out` when no summary is returned. |
| `metadata` | object | Projection status, caps, and truncation metadata. |

`source_log_summary` MUST be structurally incapable of containing raw frames or
full `metadata_json`. It contains only `id`, `timestamp`, `received_at`,
`hostname`, `severity`, `app_name`, `process_id`, `source_ip`, `message`, and
`message_truncated`. Message and text-like fields MUST be bounded, redacted for
auth/header/credential/private-key/home-path/URL-userinfo markers, and stripped
of terminal control characters.

## 9. Public Query Contract

### 9.1 Entity Lookup

```bash
cortex entity <entity-type> <key> [--limit N] [--json]
cortex entity <entity-type:key> [--json]
cortex entity --alias-type TYPE --alias-key KEY [--limit N] [--json]
```

Entity lookup MUST:

- reject unknown entity types,
- return one exact entity when unambiguous,
- return candidates for ambiguous alias matches,
- include projection metadata,
- respect bounded limits.

### 9.2 Neighborhood Lookup

```bash
cortex graph around <entity-type> <key> [--limit N] [--json]
cortex graph around <entity-type:key> [--limit N] [--json]
```

Neighborhood lookup MUST:

- be bounded,
- include relationships and safe evidence samples,
- include source/destination endpoint summaries on relationships when available,
- include projection metadata,
- include truncation metadata when caps are hit,
- avoid implicit rebuilds.

V1 neighborhood lookup is one-hop.

### 9.3 Explanation

```bash
cortex graph explain <entity-type> <key> [--depth N] [--beam-width N] [--max-chains N] [--json]
cortex graph explain <entity-type:key> [--json]
```

Explanation MUST:

- be deterministic,
- cite relationship/evidence ids,
- include readable endpoint summaries on returned relationships when available,
- use conservative language,
- report missing evidence,
- avoid claiming root cause from correlation alone,
- include projection and truncation metadata.

### 9.4 Evidence Lookup

```bash
cortex graph evidence <evidence-id> [--payload-budget BYTES] [--json]
```

MCP uses `action=graph mode=evidence evidence_id=<id>`. REST uses
`GET /api/graph/evidence?evidence_id=<id>`.

Evidence lookup MUST:

- anchor on `graph_relationship_evidence.id`,
- be read-only and never trigger rebuild,
- return the safe evidence row, owning relationship, endpoint summaries, and
  projection metadata,
- return bounded `source_log_summary` for existing log-derived evidence,
- return `source_log_summary: null` with `missing_source_reason` when the source
  is non-log evidence or the referenced log row is missing,
- preserve source identifiers such as `source_log_id`, `source_heartbeat_id`,
  and `source_signature_hash`,
- reject unknown REST query fields,
- avoid exposing raw frames, full `metadata_json`, secrets, or terminal controls.

## 10. Compatibility Requirements

Changes MUST preserve:

- existing entity type strings,
- existing relationship type strings,
- existing reason code strings,
- existing trust level meanings,
- JSON field names for public responses,
- explicit rebuild semantics,
- no implicit rebuild from lookup,
- safe evidence output rules.

Breaking changes require:

1. migration note,
2. changelog entry,
3. update to this contract,
4. tests covering old/new behavior or explicit rejection,
5. operator-facing migration guidance when persisted data shape changes.
