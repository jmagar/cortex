---
date: 2026-06-11 21:37:45 EDT
repo: git@github.com:jmagar/cortex.git
branch: codex/file-tail-ingest
head: 4b17a3e
pr: https://github.com/jmagar/cortex/pull/73
working directory: /home/jmagar/workspace/cortex/.worktrees/file-tail-ingest
worktree: /home/jmagar/workspace/cortex/.worktrees/file-tail-ingest
beads: syslog-mcp-6y96m
---

# Managed file-tail ingest

## User Request
Create and work the plan `2026-06-11-file-tail-ingest.md`: keep the special log-file ingestion functionality that previously came from syslog forwarders, but expose it as a managed Cortex action available through CLI, REST API, and MCP.

## Session Overview
Implemented managed file-tail ingestion on branch `codex/file-tail-ingest` and opened PR #73. Cortex can now persist file-tail source definitions, supervise enabled sources, ingest appended lines through the same log path as other sources, and manage those sources through `cortex file-tail`, `POST /api/file-tails`, and the admin MCP action `file_tails`.

The final review pass hardened the feature around admin authorization, path policy, checkpoint correctness, partial-line handling, rotation, MCP schema typing, smoke/live-test behavior, and docs parity.

## Sequence of Events
1. Created isolated worktree `/home/jmagar/workspace/cortex/.worktrees/file-tail-ingest` on branch `codex/file-tail-ingest`.
2. Claimed bead `syslog-mcp-6y96m` for managed file-tail ingestion.
3. Implemented the initial registry, supervisor, runtime wiring, CLI/API/MCP action, docs, config, and version bump to 1.20.0.
4. Opened PR #73: `feat: add managed file-tail ingest`.
5. Ran multiple review waves focused on data integrity, security, performance, API contracts, test simplification, and PR comments.
6. Landed review hardening commit `4b17a3e` after local full-gate verification and pre-push verification.

## Key Changes
| area | change |
|---|---|
| Registry | `file-tails.json` persisted via `FileTailRegistry`, with add/list/status/remove/enable/disable operations and missing-id errors. |
| Supervisor | `FileTailSupervisor` tails enabled sources, checkpoints progress, reloads registry state between retries, handles copytruncate and rename-create rotation, and buffers partial EOF records until newline. |
| Security | REST management requires normal API bearer auth plus `X-Cortex-Admin-Token`; file paths must be absolute, existing, regular files, non-symlinks, and inside configured allow roots. |
| Metadata | File-tail rows use `source_kind=file-tail`, `source=file-tail:<id>`, and metadata limited to `file_tail_id`, `tag`, and `path_basename`. |
| Interfaces | Added `cortex file-tail` CLI, `/api/file-tails`, and admin MCP action `file_tails`. |
| Contracts | Updated MCP schemas, action counts, source-kind docs, filter aliases, API docs, CLI docs, config docs, README, CLAUDE.md, and smoke/live test scripts. |

## Review Findings Addressed
- Checkpoint updates are registry-authoritative, so retries do not replay from stale in-memory source copies.
- Partial lines at EOF are held until newline instead of being prematurely ingested.
- Rotation and truncation reopen paths through the same path-policy checks used on initial open.
- Missing files during reopen now surface an error instead of silently marking the source healthy.
- MCP schema now constrains `get.id` to integer and `file_tails.id` to string through action-specific JSON Schema conditionals.
- REST tests cover missing and wrong admin token cases.
- Smoke/live MCP scripts skip admin-only `file_tails` only when the token cannot have admin scope.
- The stale `[Unreleased]` changelog compare target now starts from `v1.20.0`.

## Verification Evidence
| command | result |
|---|---|
| `cargo test supervisor_ingests_appended_line_and_updates_checkpoint --lib` | pass |
| `cargo test file_tail --all-targets` | pass |
| `bash -n scripts/smoke-test.sh tests/test_live.sh tests/mcporter/test-tools.sh` | pass |
| `cargo fmt --check` | pass |
| `cargo clippy --all-targets -- -D warnings` | pass |
| `cargo test` | pass: 331 main filtered unit tests plus integration/doc tests reported clean in final local run |
| `bash scripts/check-version-sync.sh` | pass |
| `bash scripts/check-rust-module-size.sh --limit 500 src/cli.rs src/cli` | pass |
| `cargo deny check` | pass, with existing wildcard git-source warning for `lab-auth` |
| `git push` pre-push hook | pass: full `cargo test` reran before pushing `4b17a3e` |

## PR State
- PR: https://github.com/jmagar/cortex/pull/73
- Head after implementation/review fixes: `4b17a3e`
- Local verification: green.
- GitHub checks restarted after the final code push; at the time of this note, the new head had queued/in-progress CI jobs and GitGuardian had already passed.

## Remaining Notes
- CodeRabbit's latest visible summary still listed comments from the previous head when this note was written; the final hardening commit addresses the substantive items except the low-value sidecar-module naming suggestion, which conflicts with the current multi-sidecar file-tail test layout.
- The `FileTailStatus.running` boolean was left as-is to avoid a broader API shape churn; `last_error` now carries retry detail for operators.
- `CORTEX_FILE_TAIL_ALLOWED_ROOTS` should be set explicitly in production when operators want to constrain tails to a narrow mount set.

## Beads Activity
- `syslog-mcp-6y96m` was claimed and implemented.
- No unrelated beads were modified.

## Next Steps
- Wait for PR #73 CI and CodeRabbit to finish on the final branch head.
- Merge PR #73 once checks are green.
- Deploy with mounted log roots for any host that should manage file-tail sources through Cortex rather than rsyslog imfile.
