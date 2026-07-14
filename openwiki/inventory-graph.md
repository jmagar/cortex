# Inventory Graph

This page maps exactly what Cortex currently projects into the investigation
graph. It is the operator-facing companion to the stricter compatibility
contract in [`docs/contracts/investigation-graph.md`](../docs/contracts/investigation-graph.md).

Implementation source of truth:

- `src/db/graph.rs`: log, heartbeat, Docker, AI-session, identity, git-command,
  and error-signature graph projection.
- `src/db/graph_inventory.rs`: normalized homelab inventory projection.
- `src/db/graph_inventory/sql.rs`: inventory projection merge, pruning, and
  evidence insertion behavior.
- `src/runtime/graph_refresh.rs`: optional background refresh of log-derived
  graph state.
- `src/runtime/inventory_refresh.rs`: inventory cache refresh and optional
  inventory graph projection.

## What The Graph Is

The graph is a rebuildable SQLite projection. Raw logs, heartbeats, error
signatures, and inventory cache files remain authoritative. Graph rows are
derived facts used for topology lookup, topic correlation, and evidence-backed
explanations.

The public graph tables are:

| Table | What It Stores |
| --- | --- |
| `graph_entities` | Nodes, unique by `(entity_type, canonical_key)`. Includes display label, source kind, source id, trust level, and first/last seen timestamps. |
| `graph_entity_aliases` | Alternate lookup keys for an entity, such as hostname, heartbeat host id, IP address, or service domain. |
| `graph_relationships` | Directed edges, unique by source entity, relationship type, destination entity, and relationship key. Includes reason code, trust, confidence, evidence count, and seen window. |
| `graph_relationship_evidence` | Bounded evidence rows backing each relationship. Includes source references, reason text, confidence delta, trust level, safe excerpt, and metadata path. |
| `graph_projection_meta` | Projection lifecycle and counts: status, watermark, row counts, runtime, chunk count, degraded flag, and last error. |

## Projection Lanes

There are two active projectors that write into the same graph tables.

| Lane | Source | Refresh Path | Writes |
| --- | --- | --- | --- |
| Runtime graph projection | `logs`, `host_heartbeats_latest`, `error_signatures` | `refresh_graph_projection()` for full rebuild, `refresh_graph_projection_incremental()` for deltas | Log-derived entities/edges, heartbeat host entities/aliases, error-signature entities/edges |
| Inventory graph projection | Normalized `HomelabInventory` cache | `project_inventory()` after inventory refresh when `CORTEX_INVENTORY_GRAPH_PROJECTION_ENABLED=true`; also run after explicit graph rebuild if an inventory cache exists | Source/app inventory entities, aliases, topology edges, and config-artifact evidence |

The runtime graph background task is disabled by default:
`CORTEX_GRAPH_REFRESH_INTERVAL_SECS=0`. When enabled, startup does an eager
pass, then incremental passes process only logs newer than the watermark plus
bounded heartbeat/signature snapshots.

The inventory refresh task is enabled on its own cadence, but inventory graph
projection is separately opt-in because the merge can create visible DB pressure
on large databases. Inventory projection prunes previous
`source_inventory`/`app_inventory` evidence and inventory-reason relationships
before applying the latest cache.

## Entity Types

These are all valid entity types in the current graph vocabulary and how they
are populated today.

| Entity Type | Populated Today From | Key Shape / Notes |
| --- | --- | --- |
| `host` | `logs.hostname`, Docker source metadata, heartbeats, inventory nodes, error signatures | Canonical hostname. Log hostname is sender-claimed; Docker host and heartbeat are treated as verified for their projection path. Aliases can include `hostname`, `heartbeat_host_id`, and inventory IPs. |
| `source_ip` | `logs.source_ip` | Raw source string from the log row, normalized as a key. Despite the name, this can be an IP or an ingest URI such as `docker://...`, `docker-event://...`, `agent-command://...`, or `shell-history://...`. |
| `app` | `logs.app_name`, `error_signatures.sample_app_name` | Application/process label observed in logs or error summaries. |
| `container` | Docker log/event rows | Keyed by Docker host plus container id or name. |
| `logical_service` | Resolver decisions over agent Docker metadata and verified inventory | Canonical logical service identity, keyed by the bare canonical name (`plex`). One node per service regardless of how many hosts run it. |
| `service_instance` | Resolver decisions over agent Docker metadata and verified inventory | Host-scoped runtime deployment, keyed `host/service` (`tootie/plex`). Linked to its logical service via `instance_of`. |
| `service` | Legacy (pre entity-resolution) — no longer projected | Stale defect shapes such as `tootie:plex` and `tootie:plex:plex`. Deleted by migration 41; rejected on lookup surfaces with `rejected_legacy_shape`. |
| `compose_project` | Docker compose project labels, inventory compose projects | Log-derived projects are keyed by Docker host plus project. Inventory projects are scoped by the provenance source host. |
| `reverse_proxy` | Inventory reverse proxy routes | Keyed from the route id; display label usually comes from the first server name. |
| `domain` | Inventory reverse proxy server names, AdGuard DNS queries | Reverse proxy domains are verified inventory facts; AdGuard query domains are inferred from DNS logs. Service domains are aliases on `service`, not standalone domain nodes, unless another source creates a domain entity. |
| `network` | Inventory network segments | Keyed by source host plus network name. |
| `storage` | Inventory service mounts, inventory storage summaries | Service mount targets create storage nodes scoped by host and target. Storage summaries create storage nodes from their inventory id. |
| `config_artifact` | Inventory artifact references | Redacted config artifact, usually compose or reverse-proxy config. The graph stores references and safe excerpts, not raw config bodies. |
| `ai_project` | AI transcript log rows, agent-command cwd inference, git-command projection | Regular AI rows use `logs.ai_project`. Agent-command rows infer the repo/project from cwd, preferring the path segment after `workspace`. |
| `ai_session` | AI transcript log rows, agent-command log rows | Keyed as `{project}:{tool}:{session}`. Agent-command rows use the inferred project so they converge with transcript-derived sessions for the same session id. |
| `error_signature` | `error_signatures` table | Keyed by `{signature_hash}:{normalizer_version}`. Display label is the first 120 chars of the template. |
| `git_commit` | Agent-command or shell-history rows whose message contains `git commit` or `git push` | Keyed by `{ai_project}:{timestamp}` for agent-command rows or `{hostname}:{timestamp}` for shell-history rows. The graph asserts a commit/push event happened, not an exact SHA. |
| `user` | Shell-history source URI, Authelia username metadata | Keyed `{host}:{user}`. Represents an operating or authenticated identity principal. |
| `device` | AdGuard DNS client metadata | Keyed by client identifier, usually client IP. Represents an endpoint distinct from a server `host`. |

## Relationship Types

These are all valid relationship types.

| Relationship Type | Active Today? | Meaning |
| --- | --- | --- |
| `observed_as` | Yes | A source identifier claimed or was observed as another entity. |
| `runs_on` | Yes | A container/service is associated with a host or service runtime. |
| `emitted_by` | Yes | An app or git-command event was emitted by a host. |
| `worked_on` | Yes | An AI session worked on a project, host, or git-command event. |
| `matches_signature` | Yes | A host/app matches an error signature. |
| `defines_service` | Yes | A compose project defines a service. |
| `routes_to` | Yes | A reverse proxy route points at a service. |
| `exposes_domain` | Yes | A reverse proxy exposes a domain/server name. |
| `attached_to` | Yes | A service is attached to a network. |
| `mounts` | Yes | A service mounts storage. |
| `backed_by` | Yes | A host has a storage backing/mount. |
| `has_artifact` | Yes | A compose project has a config artifact, or a git-command event is attributed to a project. |
| `authenticated_as` | Yes | A user authenticated to the log row's host through Authelia. |
| `accessed` | Yes | A user/device accessed a host or domain. |
| `communicates_with` | Reserved | Vocabulary for future device-to-peer flow ingestion, not emitted today. |
| `instance_of` | Yes | A `service_instance` is a deployment of a `logical_service` (`tootie/plex instance_of plex`). |

Relationship direction is meaningful. For example, `service_instance runs_on host`
is not equivalent to `host runs_on service_instance`.

## Active Edge Rules

### Generic Log Identity

| Input | Output Edge | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| `logs.source_ip` + `logs.hostname` | `source_ip observed_as host` | `syslog_claimed_hostname` | `claimed` | `0.6` | `log`, `source_log_id`, safe excerpt is hostname |
| `logs.app_name` + `logs.hostname` | `app emitted_by host` | `log_app_name` | `inferred` | `0.5` | `log`, metadata path `logs.app_name` |

`logs.hostname` is sender-claimed, so the host side of generic syslog identity
is not treated as verified.

### AI Session Logs

| Input | Output Edge | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| `logs.ai_project`, `logs.ai_tool`, `logs.ai_session_id` | `ai_session worked_on ai_project` | `ai_session_project` | `verified` | `0.9` | `log`, metadata path `logs.ai_project/logs.ai_session_id` |

Agent-command rows are skipped by this generic AI rule because their
`ai_project` column stores raw cwd. They are handled by the agent-command rules
below.

### Agent Command Rows

Rows whose `logs.source_ip` starts with `agent-command://` produce session and
project topology from the command spool record.

| Input | Output Edge | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| `ai_session_id` + `hostname` | `ai_session worked_on host` | `agent_command_session` | `verified` | `0.95` | `log`, metadata path `logs.ai_session_id/logs.hostname` |
| cwd from `logs.ai_project` or `metadata_json.agent_command.cwd` | `ai_session worked_on ai_project` | `agent_command_cwd_infer` | `inferred` | `0.7` | `log`, metadata path `logs.ai_project (cwd)` |

Project inference prefers the path segment after `workspace`, then falls back
to the final non-empty path segment.

### Git Commit / Push Events

Only agent-command and shell-history rows are considered. The command text must
contain `git commit` or `git push`.

| Input | Output Edge | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| Agent-command row with session and inferred project | `ai_session worked_on git_commit` | `agent_command_git_commit` | `inferred` | `0.8` | `log`, metadata path `logs.message (git commit)` |
| Agent-command row with inferred project | `git_commit has_artifact ai_project` | `agent_command_git_commit` | `inferred` | `0.9` | `log`, metadata path `logs.ai_project (cwd)` |
| Shell-history row with hostname | `git_commit emitted_by host` | `shell_history_git_commit` | `inferred` | `0.7` | `log`, metadata path `logs.message (git commit)` |

The graph does not parse or assert the actual commit SHA.

### User And Device Identity

| Input | Output Edge | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| `shell-history://{host}/{user}/{shell}` in `logs.source_ip` | `user accessed host` | `shell_history_user` | `claimed` | `0.7` | `log`, metadata path `logs.source_ip (shell-history)` |
| AdGuard row with `metadata_json.client` and `metadata_json.query` | `device accessed domain` | `adguard_client_query` | `inferred` | `0.9` | `log`, metadata path `metadata_json.client/query` |
| Authelia row with `metadata_json.username` and `logs.hostname` | `user authenticated_as host` | `authelia_auth` | `claimed` | `0.8` | `log`, metadata path `metadata_json.username` |

AdGuard matching is based on `app_name` starting with `adguard`. Authelia
matching is based on `app_name == "authelia"`.

### Docker Log And Event Rows

Rows whose `logs.source_ip` starts with `docker://` or `docker-event://` are
parsed as Docker identity rows. Docker host and container can come from
`metadata_json` or from the source URI.

| Input | Output Edge | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| Docker host + container id/name | `container runs_on host` | `docker_container_id` | `verified` | `0.9` | `log`, metadata path `logs.source_ip/metadata_json` |

Hard break (entity_resolution_v2): central-pull rows keep verified
host/container edges only. They no longer synthesize legacy `service`
topology (`host:project:service`) or compose edges; canonical service
identity comes exclusively from resolver decisions over structured
agent-docker metadata and verified inventory.

### Agent Docker Identity Rows

Rows carrying `metadata_json.agent_docker` (structured identity attached by
the host-local agent) project through the deterministic resolver:

| Input | Output | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| `agent_docker.host` + `compose_service` (or `container_name`) | `logical_service` and `service_instance` entities | n/a | `verified` | n/a | entity source is `log` |
| Resolver decision pair | `service_instance instance_of logical_service` | `resolver_instance_of` | `verified` | `1.0` | `log`, metadata path `metadata_json.agent_docker` |

Central-pull `docker://` / `docker-event://` rows are ignored by the
resolver (not proof). Nested slash-triplet app labels (`plex/plex/plex`)
are never projected as `app` entities.

### Heartbeats

Heartbeats currently create or update host entities and aliases only.

| Input | Graph Effect | Trust |
| --- | --- | --- |
| `host_heartbeats_latest.hostname` | `host` entity | `verified` |
| `host_heartbeats_latest.host_id` | `heartbeat_host_id` alias on the host | `verified` |

`heartbeat_host_state` is a valid reason code reserved for heartbeat-derived
relationship evidence, but the current projector does not emit heartbeat
relationship evidence rows.

### Error Signatures

| Input | Output Edge | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| `sample_app_name` | `app matches_signature error_signature` | `error_signature_match` | `inferred` | `0.7` | `error_signature`, `source_signature_hash`, metadata path `error_signatures` |
| `sample_hostname` | `host matches_signature error_signature` | `error_signature_match` | `inferred` | `0.5` | `error_signature`, `source_signature_hash`, metadata path `error_signatures` |

The error signature entity key is `{signature_hash}:{normalizer_version}`.

### Inventory Nodes

Inventory nodes create host entities and aliases.

| Input | Graph Effect | Source Kind | Trust Mapping |
| --- | --- | --- | --- |
| `InventoryNode.hostname` | `host` entity plus `hostname` alias | `source_inventory` | `verified`/`observed` -> `verified`, `claimed` -> `claimed`, `inferred` -> `inferred` |
| `InventoryNode.ips` | `ip` aliases on the host | `source_inventory` | Same as node trust |

`inventory_node` is a valid reason code, but current inventory node projection
does not emit host relationship evidence. It creates entities and aliases.

### Inventory Services

| Input | Output Edge / Alias | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| `InventoryService.name` | `logical_service` entity (`plex`) | n/a | from inventory trust | n/a | entity source is `app_inventory` |
| `InventoryService.name`, `host` | `service_instance` entity (`tootie/plex`) | n/a | from inventory trust | n/a | entity source is `app_inventory` |
| Instance + logical pair | `service_instance instance_of logical_service` | `resolver_instance_of` | from inventory trust | `0.95` | `app_inventory` |
| `InventoryService.domains` | `domain` alias on the service instance | n/a | from inventory trust | n/a | alias source is `app_inventory` |
| `InventoryService.host` matching an inventory host | `service_instance runs_on host` | `inventory_service` | `inferred` | `0.85` | `app_inventory` |
| `InventoryService.mounts[].target` | `service_instance mounts storage` | `storage_probe` | `inferred` | `0.65` | `app_inventory` |

Service-instance keying is `host/service` (`tootie/plex`); legacy
`{host}:{service}` keys are never emitted. A service without host context
projects the `logical_service` only — no `unknown/` instance is guessed. A
bare service-name alias is usable only when that service name is unique
across the inventory snapshot.

### Inventory Compose Projects

| Input | Output Edge | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| `ComposeProject.services` matching inventory services | `compose_project defines_service service_instance` | `compose_config` | `verified` | `0.90` | `app_inventory` |
| `ComposeProject.compose_files` matching artifact refs | `compose_project has_artifact config_artifact` | `config_artifact` | `verified` | `0.95` | `app_inventory` |

Compose project keys are scoped by source host, derived from the provenance
source string.

### Inventory Reverse Proxies

| Input | Output Edge | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| `ReverseProxyRoute.server_names` | `reverse_proxy exposes_domain domain` | `reverse_proxy_config` | `verified` | `0.95` | `app_inventory` |
| `ReverseProxyRoute.upstreams` matching inventory services | `reverse_proxy routes_to service_instance` | `reverse_proxy_config` | `verified` | `0.85` | `app_inventory` |

Upstream matching understands service names and service endpoints. Ambiguous
bare names are left unlinked.

### Inventory Networks

| Input | Output Edge | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| `NetworkSegment.members` matching inventory services | `service_instance attached_to network` | `docker_network` | `verified` | `0.80` | `app_inventory` |

Network keys are scoped by source host.

### Inventory Storage

| Input | Output Edge | Reason | Trust | Confidence | Evidence |
| --- | --- | --- | --- | --- | --- |
| `StorageSummary.id` containing a host segment | `host backed_by storage` | `storage_probe` | `verified` | `0.75` | `source_inventory` |

Storage entities are created from both service mounts and storage summaries.

### Inventory Config Artifacts

| Input | Graph Effect | Source Kind | Notes |
| --- | --- | --- | --- |
| `ArtifactRef.id` | `config_artifact` entity | `app_inventory` | Display label is `{kind} artifact {id}`. |
| `ArtifactRef.source_path` | Lookup handle for compose-file matching | `app_inventory` | Raw artifact content is not stored in graph rows. |

The graph only stores safe excerpts in evidence. Raw config bodies stay in the
redacted inventory artifact cache.

## Reason Codes

These are all valid reason codes and their current status.

| Reason Code | Current Status |
| --- | --- |
| `syslog_claimed_hostname` | Active: `source_ip observed_as host`. |
| `log_app_name` | Active: `app emitted_by host`. |
| `docker_container_id` | Active: `container runs_on host`. |
| `docker_service_label` | Active: `container runs_on service`. |
| `ai_session_project` | Active: `ai_session worked_on ai_project` from regular AI log rows. |
| `heartbeat_host_state` | Reserved: valid vocabulary, no relationship evidence emitted today. |
| `error_signature_match` | Active: app/host matches error signature. |
| `inventory_node` | Reserved in vocabulary: inventory nodes currently create host entities and aliases, not relationship evidence. |
| `inventory_service` | Active: `service runs_on host` from inventory. |
| `compose_config` | Active: `compose_project defines_service service` from inventory and Docker labels. |
| `reverse_proxy_config` | Active: reverse proxy exposes/routes edges. |
| `docker_network` | Active: `service attached_to network` from inventory. |
| `storage_probe` | Active: service mounts storage and host backed by storage. |
| `config_artifact` | Active: compose project has config artifact. |
| `agent_command_session` | Active: agent-command session worked on host. |
| `agent_command_cwd_infer` | Active: agent-command session worked on inferred project. |
| `agent_command_git_commit` | Active: agent-command session/project to git-command event. |
| `shell_history_git_commit` | Active: shell-history git-command event emitted by host. |
| `adguard_client_query` | Active: device accessed domain. |
| `shell_history_user` | Active: user accessed host. |
| `authelia_auth` | Active: user authenticated as host. |

## Source Kinds

These source kinds are accepted by the graph evidence vocabulary.

| Source Kind | Current Use |
| --- | --- |
| `log` | Active relationship evidence for syslog/app, AI session, agent-command, git-command, identity, Docker, and log-derived compose edges. |
| `heartbeat` | Active for host entities and aliases; not currently emitted as relationship evidence. |
| `ai_session_rollup` | Reserved for graph evidence. The `ai_session_rollup` table exists for fast `sessions` reads, but the current graph projector reads AI session topology from `logs`, not from the rollup table. |
| `source_inventory` | Active for inventory host/storage entities, aliases, and storage edges. |
| `app_inventory` | Active for inventory service, compose, reverse proxy, network, mount, and artifact topology. |
| `error_signature` | Active relationship evidence for error signature matches. |

## Trust Levels

| Trust Level | Meaning In Graph Projection |
| --- | --- |
| `verified` | Derived from a field Cortex treats as identity evidence for that rule, such as Docker source identity, heartbeat host identity, or verified inventory facts. |
| `claimed` | Derived from sender-claimed identity, such as generic syslog hostname or shell-history user attribution. |
| `inferred` | Derived from labels, metadata, summaries, or paths that imply a relationship but are not identity proof. |
| `correlated` | Reserved for future query-time temporal/co-occurrence edges. |
| `refuted` | Reserved for manual override/refutation; excluded from traversal results. |

## Inventory Fields Not Graphed Today

The normalized inventory cache contains more information than the graph
projection uses. These fields are available to inventory/map responses or other
services, but they do not currently create graph entities or edges directly:

- `InventoryNode.roles`, `os`, `cpu`, `memory`, `listeners`, `extras`.
- `InventoryService.kind`, `image`, `status`, `ports`, `env_keys`, `labels`.
- `ComposeProject.domains` and `ports`.
- `NetworkSegment.kind`.
- `StorageSummary.fs_type`, `total_bytes`, `available_bytes`.
- `HomelabInventory.media_services`.
- `HomelabInventory.projects`.
- `HomelabInventory.collection_errors`.
- Raw artifact bodies.

This is intentional: the graph stores topology and evidence, not every observed
field in the inventory snapshot.

## Other Cortex Data Not Graphed Today

These data sets exist in Cortex but are not currently projected into the
investigation graph:

- Timeline rollups (`timeline_hourly`).
- AI session rollup rows (`ai_session_rollup`) as a graph source.
- LLM invocation audit rows.
- Skill, MCP, and hook event/incident tables.
- Notification firings.
- Raw heartbeat metrics beyond host identity and `heartbeat_host_id` alias.
- Raw log rows as standalone nodes. Logs appear as relationship evidence, not
  graph entities.

## Operational Checks

Use these commands to inspect graph freshness and contents:

```bash
cortex graph status --json
cortex graph rebuild --json
cortex entity host tootie --json
cortex graph around host tootie --limit 25 --json
cortex graph explain host tootie --depth 2 --json
```

The `map`, `graph`, `topic_correlate`, `ai_correlate`, `host_state`, and
`fleet_state` surfaces consume graph and heartbeat/inventory state, but lookup
commands do not implicitly rebuild the graph.

When adding a new graph fact, update all of these in the same change:

- Vocabulary constants in `src/db/graph.rs`.
- SQLite CHECK-constraint migrations in `src/db/pool.rs`.
- Projection code and tests for the new entity/edge/evidence rule.
- [`docs/contracts/investigation-graph.md`](../docs/contracts/investigation-graph.md).
- This page.

## Canonical Resolver Proof: Plex

The canonical graph shape for Plex is:

- `logical_service:plex`
- `service_instance:tootie/plex`
- `service_instance:tootie/plex instance_of logical_service:plex`
- `service_instance:tootie/plex runs_on host:tootie`
- `compose_project:tootie/plex defines_service service_instance:tootie/plex`
- route/domain/storage/container/error/session evidence links to the service
  instance when deterministic evidence exists

`tootie:plex`, `tootie:plex:plex`, and `plex/plex/plex` are not supported
service identity inputs. They are stale defect shapes: migration 41 deletes
them from populated databases and every public lookup surface rejects them
with `rejected_legacy_shape`.

Migration 41 also flips a previously-ready projection to `stale`. Run
`cortex graph rebuild` (or wait for the in-server scheduler when
`CORTEX_GRAPH_REFRESH_INTERVAL_SECS > 0`) before expecting
`topic_correlate` service results to populate: until the rebuild, a topic
that resolves to a logical service with no `instance_of` instances reports
`resolver_status: degraded` with an empty service timeline.

Read-only proof commands (safe against production):

```bash
scripts/validate-canonical-plex-graph.sh
cortex entity logical_service plex
cortex graph around logical_service:plex
cortex graph around service_instance:tootie/plex
cortex topic-correlate plex --limit 20
```

`scripts/validate-canonical-plex-graph.sh` prints `old_key_count` (must be 0
after migration + rebuild), `new_key_count` (must be > 0 once resolver
projection has seen structured evidence), and the canonical lookup query
plan. It refuses any live rebuild: back up first (`cortex db backup`), run
`cortex graph rebuild` off-peak, then re-run the read-only checks.

Central Docker pull rows (`docker://`, `docker-event://`) are not proof for
this milestone; the proof source is `metadata_json.agent_docker` from
host-local agents (plus verified inventory). Raw log app labels such as
`complex` or `plex-backup` never self-upgrade into `logical_service:plex`.
