# AI Incidents

The AI incident detection system extracts events from AI transcripts, detects negative signals, groups them into scored incidents, and provides deterministic investigation bundles with optional LLM assessment.

## Overview

cortex tracks three types of AI usage incidents:

| Type | Tracked Events | Signal Detection | Investigation |
|------|----------------|------------------|---------------|
| **Skill Usage** | Skill invocations (Claude skills, Codex functions) | Negative transcript hits after skill loaded | `skill_investigate` |
| **MCP Tool Calls** | MCP tool invocations (server/tool pairs) | Negative transcript hits after tool call | `mcp_investigate` |
| **Hook Execution** | Hook runtime events + config inventory | Hook failures, timeouts, negative signals | `hook_investigate` |

All three follow the same pattern:
1. **Event Extraction**: Parse AI transcripts → normalized event rows
2. **Signal Detection**: Scan transcripts for negative patterns after events
3. **Incident Grouping**: Group signals by event key → score and prioritize
4. **Investigation**: Deterministic evidence bundles (transcripts, logs, findings)
5. **Assessment**: Optional LLM analysis (CLI-only, guarded)

## Architecture

```
┌────────────────────────────────────────────────────────────┐
│                  AI Transcript Ingest                       │
│  (cortex sessions watch / scanner)                          │
└───────────────────────┬────────────────────────────────────┘
                        │
                        ▼
┌────────────────────────────────────────────────────────────┐
│                   Event Extraction                          │
├────────────────────────────────────────────────────────────┤
│  skill_events │ mcp_events │ hook_events                   │
│  (parser)     │ (parser)   │ (collector)                    │
└───────────────────────┬────────────────────────────────────┘
                        │
                        ▼
┌────────────────────────────────────────────────────────────┐
│                   Signal Detection                          │
├────────────────────────────────────────────────────────────┤
│  skill_signal_detectors │ mcp_signal_detectors │ hook_signal_detectors │
└───────────────────────┬────────────────────────────────────┘
                        │
                        ▼
┌────────────────────────────────────────────────────────────┐
│                   Incident Grouping                          │
├────────────────────────────────────────────────────────────┤
│  skill_incidents │ mcp_incidents │ hook_incidents           │
│  (score/priority) │ (score/priority) │ (score/priority)     │
└───────────────────────┬────────────────────────────────────┘
                        │
                        ▼
┌────────────────────────────────────────────────────────────┐
│                   Investigation                              │
├────────────────────────────────────────────────────────────┤
│  skill_investigate │ mcp_investigate │ hook_investigate      │
│  (evidence bundle) │ (evidence bundle) │ (evidence bundle)  │
└────────────────────────────────────────────────────────────┘
                        │
                        ▼
┌────────────────────────────────────────────────────────────┐
│                   LLM Assessment (Optional)                  │
├────────────────────────────────────────────────────────────┤
│  cortex assess skill │ mcp │ hooks                         │
│  (CLI-only, guarded LLM analysis)                           │
└────────────────────────────────────────────────────────────┘
```

## Event Extraction

### Skill Events
- **Parser**: `src/scanner/skill_events.rs`
- **Table**: `ai_skill_events`
- **Tracked**: Skill name, AI tool, project, session ID, hostname, timestamp
- **Source**: Claude skill-loaded markers, Codex function calls

**Key files**:
- `src/scanner/skill_events.rs`: Skill-invocation parser
- `src/db/skill_events.rs`: Insert/list queries

### MCP Events
- **Parser**: `src/scanner/mcp_events.rs`
- **Table**: `ai_mcp_events`
- **Tracked**: MCP server, MCP tool, AI tool, project, session ID, hostname, timestamp
- **Source**: Claude `tool_use`/`tool_result`, Codex `function_call`/`function_call_output`

**Key files**:
- `src/scanner/mcp_events.rs`: MCP tool-call parser
- `src/db/mcp_events.rs`: Insert/list queries

### Hook Events
- **Collector**: `src/scanner/hook_events.rs`
- **Table**: `ai_hook_events`
- **Tracked**: Hook name, event type (runtime/config), source, session ID, hostname, timestamp
- **Source**: Hook execution logs + config inventory from AI transcripts

**Key files**:
- `src/scanner/hook_events.rs`: Hook event collector
- `src/db/hook_events.rs`: Insert/list queries

## Signal Detection

### Skill Signal Detectors
- **File**: `src/app/skill_signal_detectors.rs`
- **Signals**: `skill_loaded_no_output`, `skill_loaded_then_abuse`, `skill_loaded_then_error`, `skill_loaded_then_negative`
- **Logic**: Scan AI transcripts for negative patterns after skill invocation

### MCP Signal Detectors
- **File**: `src/app/mcp_signal_detectors.rs`
- **Signals**: `repeated_call_failure`, `timeout_or_rate_limit`, `auth_or_permission_failure`, `schema_or_validation_error`, `unknown_tool_or_server`, `user_correction_after_tool_call`
- **Logic**: Scan AI transcripts for negative patterns after MCP tool call

### Hook Signal Detectors
- **File**: `src/app/hook_signal_detectors.rs`
- **Signals**: `hook_invoked_and_failed`, `hook_invoked_and_timeout`, `hook_invoked_too_often`, `hook_config_mismatch`
- **Logic**: Detect hook failures, timeouts, and config inconsistencies

## Incident Grouping

### Grouping Key
All three incident types group signals by a composite key:
- **Skills**: `(skill_name, ai_tool, ai_project, ai_session_id, hostname, window_bucket)`
- **MCP**: `(mcp_server, mcp_tool, ai_tool, ai_project, ai_session_id, hostname, window_bucket)`
- **Hooks**: `(hook_name, hook_event, hook_source, ai_project, ai_session_id, hostname, window_bucket)`

**Window bucket**: 5-minute UTC window for temporal grouping

### Scoring & Prioritization
- **Score**: Sum of signal weights (higher = more severe)
- **Priority**: `f64::total_cmp` comparison (score, recency, signal count)
- **Top 100**: Only top 100 incidents by priority are returned (configurable)

**Key files**:
- `src/db/skill_incidents.rs::group_skill_incidents()`: Skill incident grouping
- `src/db/mcp_incidents.rs::group_mcp_incidents()`: MCP incident grouping
- `src/db/hook_incidents.rs::group_hook_incidents()`: Hook incident grouping

## Investigation

### Evidence Bundle Structure
Each investigation returns a deterministic evidence bundle:

**For skill incidents** (`SkillIncidentEvidence`):
- `skill_events`: Skill-invocation events
- `ai_sessions`: Full AI transcript sessions
- `skill_signal_anchors`: Negative transcript hits
- `nearby_logs`: Non-AI logs in correlation window (hostname-scoped)
- `findings`: Deterministic findings (rule-based, no DB/LLM calls)

**For MCP incidents** (`McpIncidentEvidence`):
- `mcp_events`: MCP tool-call events
- `ai_sessions`: Full AI transcript sessions
- `mcp_signal_anchors`: Negative transcript hits
- `nearby_logs`: Non-AI logs in correlation window (hostname-scoped)
- `findings`: Deterministic findings

**For hook incidents** (`HookIncidentEvidence`):
- `hook_events`: Hook runtime/config events
- `ai_sessions`: Full AI transcript sessions (when available)
- `hook_signal_anchors`: Negative transcript hits
- `nearby_logs`: Non-AI logs in correlation window (hostname-scoped)
- `findings`: Deterministic findings

### Deterministic Findings
Findings are rule-based classifications (no LLM calls):

**Skill findings** (`src/app/skill_incident_findings.rs`):
- `skill_invoked_error_recovery`, `skill_loaded_no_output`, `skill_invoked_then_abuse`, `skill_invoked_then_negative`, `unknown`

**MCP findings** (`src/app/mcp_incident_findings.rs`):
- `wrong_mcp_tool_selected`, `mcp_server_unavailable`, `mcp_auth_or_permission_failure`, `mcp_schema_mismatch`, `mcp_timeout_or_rate_limit`, `mcp_result_misinterpreted`, `missing_mcp_discovery_step`, `tool_surface_confusion`, `unknown`

**Hook findings** (`src/app/hook_incident_findings.rs`):
- `hook_execution_failed`, `hook_timeout`, `hook_invoked_too_often`, `config_mismatch`, `unknown`

### Resolution Rules
- **skill_investigate**: Accepts `skill_name` directly (skill-first)
- **mcp_investigate**: Accepts `server` or `tool` (server/tool-first)
- **hook_investigate**: Accepts `hook_name` (hook-first)

**Key files**:
- `src/db/skill_incident_evidence.rs::build_skill_incident_evidence()`: Skill evidence bundle
- `src/db/mcp_incident_evidence.rs::build_mcp_incident_evidence()`: MCP evidence bundle
- `src/db/hook_incident_evidence.rs::build_hook_incident_evidence()`: Hook evidence bundle

## LLM Assessment

### Overview
CLI-only LLM analysis that generates detailed incident write-ups:

- **Commands**:
  - `cortex assess skill <skill> [--since 7d] [--tool codex] [--all|--limit N] [--no-llm]`
  - `cortex assess mcp <server-or-tool> [--since 7d] [--all|--limit N] [--no-llm]`
  - `cortex assess hooks [--since 7d] [--all|--limit N] [--no-llm]`

- **Behavior**:
  - Resolves highest-priority (or all with `--all`) matching incident
  - Runs guarded Gemini assessment via `LlmRunner`
  - Generates markdown write-up with findings and recommendations

- **Guardrails**:
  - Concurrency limit (`CORTEX_LLM_CONCURRENCY`)
  - Rate limit (`CORTEX_LLM_RATE_LIMIT_PER_MIN`)
  - Circuit breaker (`CORTEX_LLM_CIRCUIT_BREAKER_THRESHOLD`)
  - `--http` rejected unless `--no-llm` is also passed (prevents HTTP routing for LLM calls)

**Key files**:
- `src/skill_assessment.rs`: Skill assessment CLI
- `src/mcp_assessment.rs`: MCP assessment CLI
- `src/hook_assessment.rs`: Hook assessment CLI
- `src/app/llm_runner.rs`: LLM runner with guardrails
- `src/app/services/skill_assessment.rs`: Skill assessment service
- `src/app/services/mcp_assessment.rs`: MCP assessment service
- `src/app/services/hook_assessment.rs`: Hook assessment service

### Assessment Skills
Embedded skills that generate the assessment write-up:
- `skill-improvement-assessment`: Skill assessment
- `mcp-friction-assessment`: MCP assessment
- `hook-friction-assessment`: Hook assessment (planned)

## Backfill

Bounded, idempotent, single-flight backfill catches up on events ingested before event tracking shipped:

**Key files**:
- `src/app/services/skill_backfill.rs`: Skill event backfill
- `src/app/services/mcp_backfill.rs`: MCP event backfill
- `src/app/services/hook_backfill.rs`: Hook event backfill (planned)

**Operation**:
1. Scan `ai_sessions.raw` column (original transcript JSON)
2. Extract events using the same parser as live ingest
3. Insert with `INSERT OR IGNORE` for idempotence
4. Track progress to skip completed sessions

## MCP & CLI Actions

### MCP Actions
| Action | Purpose |
|--------|---------|
| `skill_events` | List extracted skill-invocation events |
| `skill_incidents` | List grouped skill-usage incident candidates |
| `skill_investigate` | Evidence bundle for a skill-usage incident |
| `mcp_events` | List extracted MCP tool-call events |
| `mcp_incidents` | List grouped MCP-usage incident candidates |
| `mcp_investigate` | Evidence bundle for an MCP-usage incident |
| `hook_events` | List extracted/collected hook events |
| `hook_incidents` | List grouped hook-usage incident candidates |
| `hook_investigate` | Evidence bundle for a hook-usage incident |

### CLI Commands
| Command | Purpose |
|---------|---------|
| `cortex skill-events` | List skill events |
| `cortex skill-incidents` | List skill incidents |
| `cortex skill-investigate <skill>` | Investigate a skill incident |
| `cortex assess skill <skill>` | LLM assessment of skill incidents |
| `cortex sessions skills` | Parser for skill events output |
| `cortex sessions skills-backfill` | Run skill backfill |
| `cortex mcp-events` | List MCP events |
| `cortex mcp-incidents` | List MCP incidents |
| `cortex mcp-investigate <server-or-tool>` | Investigate an MCP incident |
| `cortex assess mcp <server-or-tool>` | LLM assessment of MCP incidents |
| `cortex sessions mcp-events` | Parser for MCP events output |
| `cortex sessions mcp-backfill` | Run MCP backfill |
| `cortex hook-events` | List hook events |
| `cortex hook-incidents` | List hook incidents |
| `cortex hook-investigate <hook>` | Investigate a hook incident |
| `cortex assess hooks` | LLM assessment of hook incidents |

## Data Models

### Event Tables
- `ai_skill_events`: `(skill_name, ai_tool, ai_project, ai_session_id, hostname, timestamp)`
- `ai_mcp_events`: `(mcp_server, mcp_tool, ai_tool, ai_project, ai_session_id, hostname, timestamp)`
- `ai_hook_events`: `(hook_name, hook_event, hook_source, ai_project, ai_session_id, hostname, timestamp)`

### Incident Tables
- `ai_skill_incidents`: Grouped incidents with score/priority
- `ai_mcp_incidents`: Grouped incidents with score/priority
- `ai_hook_incidents`: Grouped incidents with score/priority

### Evidence Tables (View/Computed)
- Evidence bundles are computed on-demand from events, sessions, signals, and logs
- No separate evidence tables (materialized view pattern)

## References

- **[docs/mcp/SCHEMA.md](../docs/mcp/SCHEMA.md)** – MCP tool and action reference
- **[docs/CLI.md](../docs/CLI.md)** – Complete CLI reference
- **[docs/contracts/incident-card.md](../docs/contracts/incident-card.md)** – Incident card schema
- **[docs/contracts/investigation-graph.md](../docs/contracts/investigation-graph.md)** – Investigation graph design
