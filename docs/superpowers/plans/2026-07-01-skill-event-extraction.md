# Skill Event Extraction, Ingest, and Backfill (GH #94 PR 2/4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract normalized skill-invocation events from Claude/Codex AI transcripts at ingest time and via backfill, and expose them for query across the CLI, MCP, and REST surfaces.

**Architecture:** A new `ai_skill_events` table (migration 38) stores one row per detected skill invocation, keyed by `(log_id, skill_name, event_kind, evidence_kind)` for idempotent `INSERT OR IGNORE` writes. Two independent extractors in `src/scanner/skill_events.rs` recognize skill invocations — Claude's structured `attributionSkill`/`attributionPlugin` JSON fields, and Codex's `<skill><name>` transcript tags — and both feed the same `ExtractedSkillEvent` shape. Extraction runs inline inside the existing transcript-ingest transaction (`flush_chunk` in `src/scanner.rs`, using log ids returned from a refactored `insert_logs_batch_in_tx`) for new data, and via a chunked, bounded, dry-run-capable backfill service (`CortexService::backfill_skill_events`) for historical data, with a shared read surface (`skill_events` MCP action, `GET /api/ai/skills`, `cortex sessions skills`) on top.

**Tech Stack:** Rust 2024 edition, `rusqlite` (bundled SQLite, WAL mode), `regex` for the Codex `<skill><name>` tag scanner, `serde`/`serde_json` for wire types.

## Global Constraints

- Never hold the SQLite write lock across a large corpus scan — chunk and release, mirroring `purge_old_logs` in `src/db/maintenance.rs`. This applies above all to the skill-event backfill task.
- Use `INSERT OR IGNORE` for all `ai_skill_events` inserts — transcript re-imports and watch retries are normal and must be idempotent.
- Do not add a second full-table scan during normal ingest — skill-event insertion happens from the log ids already available from the batch insert call in the same chunk transaction.
- `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` must pass before any task is considered done.
- Every new MCP action needs a row in `src/mcp/actions.rs` (`ACTION_SPECS`) + a dispatch arm in `src/mcp/tools.rs` + docs updates (`docs/mcp/TOOLS.md`, `docs/mcp/SCHEMA.md`, `docs/contracts/mcp-actions-current.md`, `CLAUDE.md` action table + count).
- **PR sequencing note:** This is PR 2 of 4 for GH #94 Plan A. This PR is independent of PR 1 (LLM Invocation Guard) except for the migration-number ordering documented in this plan's migration task — re-verify the live `KNOWN_SCHEMA_VERSION` in `src/db/pool.rs` at implementation time rather than trusting a hardcoded assumption, since either PR 1 or PR 2 may merge first. PR 3 (skill incident detection) depends on THIS PR (needs the `ai_skill_events` table and its query layer). PR 4 (skill assessment + unified CLI) depends on PR 1 and PR 3, not directly on this PR.

---

## Locked interfaces for other phases

These names/shapes are final. The later `skill-incidents` (grouping/scoring)
and `skill-assess` phases build directly on top of them — do not rename.

### Table: `ai_skill_events` (migration 38)

```sql
CREATE TABLE IF NOT EXISTS ai_skill_events (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  log_id             INTEGER NOT NULL REFERENCES logs(id) ON DELETE CASCADE,
  ai_tool            TEXT NOT NULL,
  ai_project         TEXT,
  ai_session_id      TEXT,
  hostname           TEXT NOT NULL,
  timestamp          TEXT NOT NULL,
  skill_name         TEXT NOT NULL,
  skill_plugin       TEXT,
  skill_path         TEXT,
  event_kind         TEXT NOT NULL,
  evidence_kind      TEXT NOT NULL,
  metadata_json      TEXT,
  created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE(log_id, skill_name, event_kind, evidence_kind)
);

CREATE INDEX IF NOT EXISTS idx_ai_skill_events_skill_time ON ai_skill_events(skill_name, timestamp);
CREATE INDEX IF NOT EXISTS idx_ai_skill_events_session_time ON ai_skill_events(ai_tool, ai_project, ai_session_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_ai_skill_events_project_skill_time ON ai_skill_events(ai_project, skill_name, timestamp) WHERE ai_project IS NOT NULL;
```

`KNOWN_SCHEMA_VERSION` bumps from `37` to `38` in `src/db/pool.rs` (PR 1, "LLM Invocation Guard", claims migration 37; this PR must land its migration immediately after whatever PR 1 adds — re-verify the live `KNOWN_SCHEMA_VERSION` at implementation time rather than trusting this hardcoded assumption, since either PR may merge first).

### Parser struct (`src/scanner/skill_events.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillEventKind {
    ClaudeAttribution,
    CodexSkillBlock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillEvidenceKind {
    StructuredJsonField,
    TranscriptContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSkillEvent {
    pub skill_name: String,
    pub skill_plugin: Option<String>,
    pub skill_path: Option<String>,
    pub event_kind: SkillEventKind,
    pub evidence_kind: SkillEvidenceKind,
    pub metadata_json: Option<String>,
}

pub fn extract_claude_skill_events(value: &serde_json::Value) -> Vec<ExtractedSkillEvent>;
pub fn extract_codex_skill_events(text: &str) -> Vec<ExtractedSkillEvent>;
```

`SkillEventKind::as_str()` -> `"claude_attribution"` / `"codex_skill_block"`.
`SkillEvidenceKind::as_str()` -> `"structured_json_field"` / `"transcript_content"`.
These `as_str()` values are exactly what gets written into the `event_kind` /
`evidence_kind` TEXT columns.

### DB layer (`src/db/skill_events.rs`, new module)

```rust
#[derive(Debug, Clone)]
pub struct SkillEventInsert {
    pub log_id: i64,
    pub ai_tool: String,
    pub ai_project: Option<String>,
    pub ai_session_id: Option<String>,
    pub hostname: String,
    pub timestamp: String,
    pub event: crate::scanner::skill_events::ExtractedSkillEvent,
}

/// INSERT OR IGNORE all rows in `events`. Returns count of rows actually inserted
/// (SQLite `changes()` summed per statement) so callers can report duplicates.
pub fn insert_skill_events_in_tx(
    tx: &rusqlite::Transaction<'_>,
    events: &[SkillEventInsert],
) -> anyhow::Result<usize>;

/// Pool-acquiring wrapper (single INSERT OR IGNORE transaction) for callers
/// outside an existing transaction (e.g. backfill chunks).
pub fn insert_skill_events(
    pool: &crate::db::DbPool,
    events: &[SkillEventInsert],
) -> anyhow::Result<usize>;

#[derive(Debug, Clone, Default)]
pub struct AiSkillEventParams {
    pub skill: Option<String>,
    pub plugin: Option<String>,
    pub tool: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub hostname: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiSkillEventEntry {
    pub id: i64,
    pub log_id: i64,
    pub ai_tool: String,
    pub ai_project: Option<String>,
    pub ai_session_id: Option<String>,
    pub hostname: String,
    pub timestamp: String,
    pub skill_name: String,
    pub skill_plugin: Option<String>,
    pub skill_path: Option<String>,
    pub event_kind: String,
    pub evidence_kind: String,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListSkillEventsResult {
    pub total: usize,
    pub truncated: bool,
    pub events: Vec<AiSkillEventEntry>,
}

pub fn list_skill_events(
    pool: &crate::db::DbPool,
    params: &AiSkillEventParams,
) -> anyhow::Result<ListSkillEventsResult>;
```

`insert_skill_events_in_tx` / `insert_skill_events` / `list_skill_events` /
`AiSkillEventParams` / `AiSkillEventEntry` / `ListSkillEventsResult` /
`SkillEventInsert` are all re-exported from `src/db.rs` (`pub use skill_events::{...}`)
exactly like every other query module, so `use crate::db::{...}` works from
`src/app/services/*` and `src/mcp/tools.rs`.

### Batch-insert-returns-ids refactor (`src/db/ingest.rs`)

Before:
```rust
pub(crate) fn insert_logs_batch_in_tx(tx: &Transaction<'_>, entries: &[LogBatchEntry]) -> Result<()>
```

After:
```rust
pub(crate) fn insert_logs_batch_in_tx(tx: &Transaction<'_>, entries: &[LogBatchEntry]) -> Result<Vec<i64>>
```
Returns one `id` per input `entries[i]`, same order, via `tx.last_insert_rowid()`
read immediately after each `stmt.execute(...)` in the existing per-row loop
(SQLite guarantees `last_insert_rowid()` reflects the most recent successful
insert on that connection — safe because these executes happen sequentially on
the same `Transaction`, never interleaved with other writers).

`pub fn insert_logs_batch(pool: &DbPool, entries: &[LogBatchEntry]) -> Result<usize>`
keeps its existing signature/return type (row count) — callers outside scanner.rs
are untouched. Internally it now discards the `Vec<i64>` from `insert_logs_batch_in_tx`
and returns `entries.len()` as before.

### Service-level backfill (`src/app/services/skill_backfill.rs`, new module under `src/app/services.rs`)

```rust
#[derive(Debug, Clone, Default)]
pub struct SkillBackfillRequest {
    pub since: Option<String>,
    pub limit: Option<u64>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SkillBackfillResult {
    pub scanned: u64,
    pub inserted: u64,
    pub skipped_duplicates: u64,
    pub parse_errors: u64,
    pub truncated: bool,
    pub dry_run: bool,
}

impl CortexService {
    pub async fn backfill_skill_events(
        &self,
        req: SkillBackfillRequest,
    ) -> ServiceResult<SkillBackfillResult>;
}
```

## Context notes for implementers (read before starting)

- **Repo migration state**: `KNOWN_SCHEMA_VERSION` is currently `36` in
  `src/db/pool.rs:42` at the time this plan was drafted. A sibling PR (PR 1,
  "LLM Invocation Guard") independently claims migration `37`. This phase
  (PR 2, skill events) claims migration `38` — the next number after PR 1's.
  **Re-verify `KNOWN_SCHEMA_VERSION` live in `src/db/pool.rs` before writing
  the migration block** — if PR 1 has not yet merged when this phase is
  implemented, insert immediately after the migration 36 block for now, but
  confirm no other migration has claimed 37 or 38 in the interim; if PR 1 has
  already merged, insert immediately after PR 1's migration 37 block instead.
  Follow the exact `if !migration_applied(&conn, 38)? { conn.execute_batch("...
  INSERT OR IGNORE INTO schema_migrations (version) VALUES (38);");
  tracing::info!(...) }` shape used by migrations 31-36 (see
  `src/db/pool.rs:1322-1975`).
- **`regex` is already a dependency** (`Cargo.toml:105`, `regex = "1"`) — no
  `Cargo.toml` edit needed for the Codex `<skill><name>...</name>` scanner.
- **CLI grammar**: `docs/CLI.md` explicitly documents `cortex ai ...` as a
  REMOVED command family, replaced by `cortex sessions ...` (v3.0 breaking
  change, no aliases). The task description's `cortex ai skills backfill`
  syntax does NOT match this repo's actual grammar. This plan uses
  `cortex sessions skills` / `cortex sessions skills backfill` instead, wired
  into the existing `SessionsCommand` enum in `src/cli/args.rs` and dispatched
  through `src/cli/parse/sessions.rs` + `src/cli/dispatch_sessions.rs`, exactly
  like `cortex sessions tools` / `cortex sessions index`. The REST route stays
  `GET /api/ai/skills` as specified (the existing `/api/sessions/*` prefix is
  the internal convention for other AI-transcript reads, but the task fixes
  `/api/ai/skills` explicitly, and `/api/ai/*` is currently unused — no
  collision).
- **Ingest hot path**: `index_file_with_options` in `src/scanner.rs` builds a
  `Vec<LogBatchEntry>` per chunk and calls `flush_chunk`, which calls
  `insert_logs_batch_in_tx(&tx, &claimed_batch)` inside the SAME transaction as
  the checkpoint update (`src/scanner.rs:774-832`). Skill-event insertion must
  happen inside `flush_chunk`, in the same `tx`, immediately after
  `insert_logs_batch_in_tx` returns the new `Vec<i64>` log ids — zipped against
  `claimed_batch` (same order, same length) to know which `log_id` each
  transcript row landed at, then re-parsed from the ORIGINAL raw JSON value
  (Claude) or the extracted message text (Codex) to find skill events. This
  means `flush_chunk` needs access to the raw parsed value/text per batch
  entry, not just the already-scrubbed `LogBatchEntry.message` — see Task 6 for
  exact plumbing (`ChunkSkillSource` side-channel vector built alongside
  `batch`/`imports` in `index_file_with_options`).
- **Scrubbing**: skill names/plugins are short identifiers, not free text, so
  they do NOT go through `scrub_ai_message` — only `LogBatchEntry.message` is
  scrubbed. Skill event fields are extracted from the ORIGINAL value before
  scrubbing (scrubbing only redacts secret-shaped substrings and would not
  usually touch a skill name, but extraction happens pre-scrub regardless per
  the parser design in Task 2/3, which read `parsed.raw_value` / the codex
  message text captured before `scrub_ai_message` runs).
- **Batch-and-release lock pattern**: mirror `purge_old_logs` in
  `src/db/maintenance.rs:578-634` — each backfill chunk: `pool.get()`, acquire
  `crate::db::write_lock()`, run one bounded chunk in one transaction, commit,
  `drop(conn)`, then loop. Do NOT hold the lock across the whole historical
  corpus.

---

### Task 1: Migration 38 — `ai_skill_events` table

**Files:**
- Modify: `src/db/pool.rs:42` (bump `KNOWN_SCHEMA_VERSION`)
- Modify: `src/db/pool.rs` (insert migration 38 block immediately after the
  migration block that PR 1 "LLM Invocation Guard" adds as migration 37 — or,
  if PR 1 has not yet merged, immediately after the migration 36 block at the
  time of writing, before the orphaned-maintenance-job cleanup comment;
  re-check line numbers live since PR 1 shifts them)
- Test: `src/db/pool_tests.rs` (sidecar convention: `src/db/pool.rs` already has
  `#[cfg(test)] #[path = "pool_tests.rs"] mod tests;` — confirm this hook exists
  near the bottom of `pool.rs`; if it does not yet exist for this file, this task
  must add it)

**Interfaces:**
- Consumes: nothing (first task in the phase)
- Produces: `ai_skill_events` table with columns/indexes exactly as specified
  in "Locked interfaces" above. `KNOWN_SCHEMA_VERSION = 38`.

- [ ] **Step 1: Write the failing test**

  In `src/db/pool_tests.rs`, add:
  ```rust
  #[test]
  fn migration_37_creates_ai_skill_events_table() {
      let dir = tempfile::tempdir().unwrap();
      let db_path = dir.path().join("test.db");
      let pool = init_pool(&crate::config::StorageConfig::for_test(db_path)).unwrap();
      let conn = pool.get().unwrap();

      let table_exists: i64 = conn
          .query_row(
              "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'ai_skill_events'",
              [],
              |row| row.get(0),
          )
          .unwrap();
      assert_eq!(table_exists, 1);

      let indexes: Vec<String> = {
          let mut stmt = conn
              .prepare(
                  "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'ai_skill_events' ORDER BY name",
              )
              .unwrap();
          stmt.query_map([], |row| row.get::<_, String>(0))
              .unwrap()
              .collect::<rusqlite::Result<Vec<_>>>()
              .unwrap()
      };
      assert!(indexes.contains(&"idx_ai_skill_events_skill_time".to_string()));
      assert!(indexes.contains(&"idx_ai_skill_events_session_time".to_string()));
      assert!(indexes.contains(&"idx_ai_skill_events_project_skill_time".to_string()));

      // UNIQUE constraint + idempotent re-run of the whole insert on identical
      // (log_id, skill_name, event_kind, evidence_kind) is exercised in Task 6;
      // here we only assert the migration ran and version advanced.
      let version = crate::db::read_schema_version_info_conn(&conn)
          .unwrap()
          .version;
      assert_eq!(version, 38);
  }
  ```

- [ ] **Step 2: Run test to verify it fails**
  ```bash
  cargo test --lib db::pool::tests::migration_37_creates_ai_skill_events_table
  ```
  Expected: compile error or `assert_eq!(table_exists, 1)` failure (table does
  not exist yet) — confirms the test currently fails for the right reason.

- [ ] **Step 3: Write minimal implementation**

  In `src/db/pool.rs`, change line 42:
  ```rust
  pub const KNOWN_SCHEMA_VERSION: i64 = 38;
  ```

  Insert immediately after the migration block PR 1 ("LLM Invocation Guard")
  adds as migration 37 (or, if PR 1 has not yet merged, immediately after the
  migration 36 block at the time of writing), before the
  orphaned-maintenance-job cleanup comment:
  ```rust
      // Migration 38: ai_skill_events — one row per detected skill invocation
      // extracted from an AI transcript log row (Claude `attributionSkill` /
      // `attributionPlugin` structured fields, Codex `<skill><name>` transcript
      // tags). UNIQUE(log_id, skill_name, event_kind, evidence_kind) makes
      // INSERT OR IGNORE idempotent across re-ingest and backfill re-runs.
      if !migration_applied(&conn, 38)? {
          conn.execute_batch(
              "BEGIN IMMEDIATE;

               CREATE TABLE IF NOT EXISTS ai_skill_events (
                 id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                 log_id             INTEGER NOT NULL REFERENCES logs(id) ON DELETE CASCADE,
                 ai_tool            TEXT NOT NULL,
                 ai_project         TEXT,
                 ai_session_id      TEXT,
                 hostname           TEXT NOT NULL,
                 timestamp          TEXT NOT NULL,
                 skill_name         TEXT NOT NULL,
                 skill_plugin       TEXT,
                 skill_path         TEXT,
                 event_kind         TEXT NOT NULL,
                 evidence_kind      TEXT NOT NULL,
                 metadata_json      TEXT,
                 created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 UNIQUE(log_id, skill_name, event_kind, evidence_kind)
               );

               CREATE INDEX IF NOT EXISTS idx_ai_skill_events_skill_time
                   ON ai_skill_events(skill_name, timestamp);
               CREATE INDEX IF NOT EXISTS idx_ai_skill_events_session_time
                   ON ai_skill_events(ai_tool, ai_project, ai_session_id, timestamp);
               CREATE INDEX IF NOT EXISTS idx_ai_skill_events_project_skill_time
                   ON ai_skill_events(ai_project, skill_name, timestamp)
                   WHERE ai_project IS NOT NULL;

               INSERT OR IGNORE INTO schema_migrations (version) VALUES (38);
               COMMIT;",
          )?;
          tracing::info!("Migration 38: created ai_skill_events table");
      }
  ```

  If `src/db/pool.rs` does not already end with a `#[cfg(test)] #[path =
  "pool_tests.rs"] mod tests;` hook, add one at the bottom of the file (it does
  — confirmed present; `src/db/pool_tests.rs` already exists in the repo).

- [ ] **Step 4: Run test to verify it passes**
  ```bash
  cargo test --lib db::pool::tests::migration_37_creates_ai_skill_events_table
  ```
  Expected: `test db::pool::tests::migration_37_creates_ai_skill_events_table ... ok`

- [ ] **Step 5: Commit**
  ```bash
  git add src/db/pool.rs src/db/pool_tests.rs
  git commit -m "feat(db): add migration 38 for ai_skill_events table"
  ```

---

### Task 2: Claude skill-event parser

**Files:**
- Create: `src/scanner/skill_events.rs`
- Test: `src/scanner/skill_events_tests.rs` (new sidecar file, hooked via
  `#[cfg(test)] #[path = "skill_events_tests.rs"] mod tests;` at the bottom of
  `src/scanner/skill_events.rs`, matching `src/scanner/claude.rs` /
  `src/scanner/codex.rs` convention)

**Interfaces:**
- Consumes: nothing new (operates on `serde_json::Value` and `&str`, no other
  Task-1+ types)
- Produces: `ExtractedSkillEvent`, `SkillEventKind`, `SkillEvidenceKind` (all
  locked above), `pub fn extract_claude_skill_events(value: &serde_json::Value)
  -> Vec<ExtractedSkillEvent>`

- [ ] **Step 1: Write the failing test**

  Create `src/scanner/skill_events_tests.rs`:
  ```rust
  use super::*;
  use serde_json::json;

  #[test]
  fn extracts_top_level_attribution_skill_and_plugin() {
      let value = json!({
          "sessionId": "sess-1",
          "attributionSkill": "cortex-troubleshoot",
          "attributionPlugin": "cortex",
          "content": "ran the troubleshoot skill"
      });
      let events = extract_claude_skill_events(&value);
      assert_eq!(events.len(), 1);
      let event = &events[0];
      assert_eq!(event.skill_name, "cortex-troubleshoot");
      assert_eq!(event.skill_plugin.as_deref(), Some("cortex"));
      assert_eq!(event.event_kind, SkillEventKind::ClaudeAttribution);
      assert_eq!(event.evidence_kind, SkillEvidenceKind::StructuredJsonField);
  }

  #[test]
  fn extracts_nested_message_attribution_fields() {
      let value = json!({
          "message": {
              "attributionSkill": "web-app-testing",
              "attributionPlugin": "testing",
              "content": "tested the app"
          }
      });
      let events = extract_claude_skill_events(&value);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].skill_name, "web-app-testing");
      assert_eq!(events[0].skill_plugin.as_deref(), Some("testing"));
  }

  #[test]
  fn extracts_nested_payload_attribution_fields() {
      let value = json!({
          "payload": {
              "attributionSkill": "code-review",
              "attributionPlugin": null
          }
      });
      let events = extract_claude_skill_events(&value);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].skill_name, "code-review");
      assert_eq!(events[0].skill_plugin, None);
  }

  #[test]
  fn emits_nothing_when_attribution_fields_absent() {
      let value = json!({"sessionId": "sess-1", "content": "just chatting"});
      assert!(extract_claude_skill_events(&value).is_empty());
  }

  #[test]
  fn emits_nothing_for_empty_or_whitespace_skill_name() {
      let value = json!({"attributionSkill": "   "});
      assert!(extract_claude_skill_events(&value).is_empty());
  }

  #[test]
  fn does_not_fabricate_plugin_skill_combined_string() {
      // Claude gives plugin and skill as SEPARATE fields — the combined
      // "plugin:skill" form must only appear when the source field itself
      // already used that format (see codex tests for that case).
      let value = json!({
          "attributionSkill": "sonnar",
          "attributionPlugin": "arrs"
      });
      let events = extract_claude_skill_events(&value);
      assert_eq!(events[0].skill_name, "sonnar");
      assert_eq!(events[0].skill_plugin.as_deref(), Some("arrs"));
  }
  ```

- [ ] **Step 2: Run test to verify it fails**
  ```bash
  cargo test --lib scanner::skill_events
  ```
  Expected: compile error — `src/scanner/skill_events.rs` and
  `extract_claude_skill_events` do not exist yet.

- [ ] **Step 3: Write minimal implementation**

  Create `src/scanner/skill_events.rs`:
  ```rust
  //! Skill-event extraction from AI transcript records.
  //!
  //! Two independent extractors feed the same [`ExtractedSkillEvent`] shape:
  //! - Claude: structured `attributionSkill` / `attributionPlugin` JSON fields
  //!   (top-level, `message.*`, or `payload.*` nesting).
  //! - Codex: `<skill><name>...</name></skill>` tags embedded in transcript
  //!   message text (see `codex_skill_regex` in this module).
  //!
  //! Callers normalize with [`ExtractedSkillEvent::normalized`] before
  //! inserting, which trims/clamps/derives the `plugin:skill` combined form.

  const MAX_SKILL_FIELD_CHARS: usize = 256;

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum SkillEventKind {
      ClaudeAttribution,
      CodexSkillBlock,
  }

  impl SkillEventKind {
      pub fn as_str(self) -> &'static str {
          match self {
              Self::ClaudeAttribution => "claude_attribution",
              Self::CodexSkillBlock => "codex_skill_block",
          }
      }
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum SkillEvidenceKind {
      StructuredJsonField,
      TranscriptContent,
  }

  impl SkillEvidenceKind {
      pub fn as_str(self) -> &'static str {
          match self {
              Self::StructuredJsonField => "structured_json_field",
              Self::TranscriptContent => "transcript_content",
          }
      }
  }

  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct ExtractedSkillEvent {
      pub skill_name: String,
      pub skill_plugin: Option<String>,
      pub skill_path: Option<String>,
      pub event_kind: SkillEventKind,
      pub evidence_kind: SkillEvidenceKind,
      pub metadata_json: Option<String>,
  }

  impl ExtractedSkillEvent {
      /// Trim, reject-if-empty, and clamp `skill_name`/`skill_plugin` to
      /// `MAX_SKILL_FIELD_CHARS`. Returns `None` when the resulting skill_name
      /// would be empty (never panics or bubbles an error — callers skip the
      /// event and keep parsing the rest of the transcript).
      fn normalized(mut self) -> Option<Self> {
          let trimmed_name = self.skill_name.trim();
          if trimmed_name.is_empty() {
              return None;
          }
          self.skill_name = clamp_chars(trimmed_name, MAX_SKILL_FIELD_CHARS);
          self.skill_plugin = self.skill_plugin.and_then(|plugin| {
              let trimmed = plugin.trim();
              (!trimmed.is_empty()).then(|| clamp_chars(trimmed, MAX_SKILL_FIELD_CHARS))
          });
          Some(self)
      }
  }

  fn clamp_chars(value: &str, max_chars: usize) -> String {
      if value.chars().count() <= max_chars {
          value.to_string()
      } else {
          value.chars().take(max_chars).collect()
      }
  }

  /// Extract Claude skill-attribution events from a raw transcript JSON value.
  /// Checks top-level, `message.*`, and `payload.*` nesting for
  /// `attributionSkill` / `attributionPlugin` string fields (Claude transcripts
  /// use flat top-level fields on user-facing records and nested `message.*`
  /// fields on some tool-result records; `payload.*` mirrors the Codex
  /// event-envelope shape defensively in case a future transcript format nests
  /// the same way). Returns one event per candidate location that has a
  /// non-empty `attributionSkill`; at most one event in practice since a single
  /// transcript line only has one of the three shapes.
  pub fn extract_claude_skill_events(value: &serde_json::Value) -> Vec<serde_json::Value>
  {
      unreachable!()
  }

  #[cfg(test)]
  #[path = "skill_events_tests.rs"]
  mod tests;
  ```

  That stub is wrong on purpose as a checkpoint — replace the final function
  with the real implementation before running tests:

  ```rust
  pub fn extract_claude_skill_events(value: &serde_json::Value) -> Vec<ExtractedSkillEvent> {
      let candidates = [
          value,
          value.get("message").unwrap_or(&serde_json::Value::Null),
          value.get("payload").unwrap_or(&serde_json::Value::Null),
      ];
      for candidate in candidates {
          let Some(skill) = candidate.get("attributionSkill").and_then(serde_json::Value::as_str)
          else {
              continue;
          };
          let plugin = candidate
              .get("attributionPlugin")
              .and_then(serde_json::Value::as_str)
              .map(ToString::to_string);
          let event = ExtractedSkillEvent {
              skill_name: skill.to_string(),
              skill_plugin: plugin,
              skill_path: None,
              event_kind: SkillEventKind::ClaudeAttribution,
              evidence_kind: SkillEvidenceKind::StructuredJsonField,
              metadata_json: None,
          };
          if let Some(normalized) = event.normalized() {
              return vec![normalized];
          }
          return Vec::new();
      }
      Vec::new()
  }
  ```

  Remove the placeholder stub entirely — the file should contain exactly one
  definition of `extract_claude_skill_events` (the real one above). Leave the
  `#[cfg(test)] #[path = "skill_events_tests.rs"] mod tests;` at the bottom.

- [ ] **Step 4: Run test to verify it passes**
  ```bash
  cargo test --lib scanner::skill_events
  ```
  Expected: all 6 tests in `skill_events_tests.rs` pass —
  `test scanner::skill_events::tests::... ok` x6.

- [ ] **Step 5: Commit**
  ```bash
  git add src/scanner/skill_events.rs
  git commit -m "feat(scanner): add Claude attributionSkill/attributionPlugin extraction"
  ```

---

### Task 3: Codex `<skill><name>` parser + normalization edge cases

**Files:**
- Modify: `src/scanner/skill_events.rs` (add `extract_codex_skill_events`)
- Test: `src/scanner/skill_events_tests.rs` (append)

**Interfaces:**
- Consumes: `ExtractedSkillEvent`, `SkillEventKind`, `SkillEvidenceKind` from
  Task 2 (same file)
- Produces: `pub fn extract_codex_skill_events(text: &str) -> Vec<ExtractedSkillEvent>`

- [ ] **Step 1: Write the failing test**

  Append to `src/scanner/skill_events_tests.rs`:
  ```rust
  #[test]
  fn extracts_single_codex_skill_tag() {
      let text = "Running <skill><name>rustarr</name></skill> now.";
      let events = extract_codex_skill_events(text);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].skill_name, "rustarr");
      assert_eq!(events[0].event_kind, SkillEventKind::CodexSkillBlock);
      assert_eq!(events[0].evidence_kind, SkillEvidenceKind::TranscriptContent);
  }

  #[test]
  fn extracts_multiple_distinct_codex_skill_tags() {
      let text = "<skill><name>sonarr</name></skill> then <skill><name>radarr</name></skill>";
      let mut events = extract_codex_skill_events(text);
      events.sort_by(|a, b| a.skill_name.cmp(&b.skill_name));
      assert_eq!(events.len(), 2);
      assert_eq!(events[0].skill_name, "radarr");
      assert_eq!(events[1].skill_name, "sonarr");
  }

  #[test]
  fn dedupes_identical_skill_names_within_one_row() {
      let text = "<skill><name>cortex</name></skill> ... <skill><name>cortex</name></skill>";
      let events = extract_codex_skill_events(text);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].skill_name, "cortex");
  }

  #[test]
  fn accepts_optional_whitespace_around_tags() {
      let text = "<skill> <name> tailscale </name> </skill>";
      let events = extract_codex_skill_events(text);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].skill_name, "tailscale");
  }

  #[test]
  fn does_not_match_prose_mentioning_a_skill() {
      let text = "You should use the rust skill for this task, not a literal tag.";
      assert!(extract_codex_skill_events(text).is_empty());
  }

  #[test]
  fn skips_empty_skill_name_tag_without_erroring() {
      let text = "<skill><name></name></skill> <skill><name>real-skill</name></skill>";
      let events = extract_codex_skill_events(text);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].skill_name, "real-skill");
  }

  #[test]
  fn derives_plugin_skill_split_from_combined_form() {
      let text = "<skill><name>cortex:cortex-troubleshoot</name></skill>";
      let events = extract_codex_skill_events(text);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].skill_name, "cortex:cortex-troubleshoot");
      assert_eq!(events[0].skill_plugin.as_deref(), Some("cortex"));
  }

  #[test]
  fn clamps_oversized_skill_name_to_256_chars() {
      let long_name = "a".repeat(300);
      let text = format!("<skill><name>{long_name}</name></skill>");
      let events = extract_codex_skill_events(&text);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].skill_name.chars().count(), 256);
  }
  ```

- [ ] **Step 2: Run test to verify it fails**
  ```bash
  cargo test --lib scanner::skill_events
  ```
  Expected: compile error — `extract_codex_skill_events` undefined.

- [ ] **Step 3: Write minimal implementation**

  Add to `src/scanner/skill_events.rs` (below `extract_claude_skill_events`,
  above the test module hook):
  ```rust
  use std::sync::LazyLock;

  use regex::Regex;

  /// Matches `<skill> <name> ... </name> </skill>` with optional whitespace
  /// around every tag boundary. `(?s)` lets `.` cross newlines (skill names are
  /// short but transcripts can wrap). Non-greedy `.*?` keeps each match scoped
  /// to one tag pair even when multiple `<skill>` blocks appear in one message.
  static CODEX_SKILL_TAG: LazyLock<Regex> = LazyLock::new(|| {
      Regex::new(
          r"(?s)<skill>\s*<name>\s*(.*?)\s*</name>\s*</skill>",
      )
      .expect("static regex")
  });

  /// Extract Codex skill-invocation events from transcript message text. Scans
  /// for ALL `<skill><name>...</name></skill>` occurrences (a single row can
  /// invoke multiple skills), de-duplicating identical skill names within the
  /// row. Deliberately narrow — matches only the literal tag pair, never prose
  /// like "use the rust skill".
  pub fn extract_codex_skill_events(text: &str) -> Vec<ExtractedSkillEvent> {
      let mut seen = std::collections::HashSet::new();
      let mut events = Vec::new();
      for capture in CODEX_SKILL_TAG.captures_iter(text) {
          let raw_name = capture.get(1).map_or("", |m| m.as_str());
          let event = ExtractedSkillEvent {
              skill_name: raw_name.to_string(),
              skill_plugin: None,
              skill_path: None,
              event_kind: SkillEventKind::CodexSkillBlock,
              evidence_kind: SkillEvidenceKind::TranscriptContent,
              metadata_json: None,
          };
          let Some(normalized) = event.normalized() else {
              continue;
          };
          if seen.insert(normalized.skill_name.clone()) {
              events.push(normalized);
          }
      }
      events
  }
  ```

  Now update `ExtractedSkillEvent::normalized` (from Task 2) to derive the
  `plugin:skill` split when the raw name already contains a single `:`:
  ```rust
  fn normalized(mut self) -> Option<Self> {
      let trimmed_name = self.skill_name.trim();
      if trimmed_name.is_empty() {
          return None;
      }
      // If the source already used "plugin:skill" combined form, split it out
      // for skill_plugin while keeping skill_name as the full combined string
      // (locked behavior — do not fabricate this split when plugin/skill came
      // from separate source fields, e.g. Claude's attributionSkill/attributionPlugin).
      if self.skill_plugin.is_none() {
          if let Some((plugin, _rest)) = trimmed_name.split_once(':') {
              if !plugin.is_empty() {
                  self.skill_plugin = Some(plugin.to_string());
              }
          }
      }
      self.skill_name = clamp_chars(trimmed_name, MAX_SKILL_FIELD_CHARS);
      self.skill_plugin = self.skill_plugin.and_then(|plugin| {
          let trimmed = plugin.trim();
          (!trimmed.is_empty()).then(|| clamp_chars(trimmed, MAX_SKILL_FIELD_CHARS))
      });
      Some(self)
  }
  ```

  Add `use std::sync::LazyLock;` and `use regex::Regex;` at the top of the file
  if not already present from the edit above.

- [ ] **Step 4: Run test to verify it passes**
  ```bash
  cargo test --lib scanner::skill_events
  ```
  Expected: all tests from Task 2 + Task 3 pass (14 total).

- [ ] **Step 5: Commit**
  ```bash
  git add src/scanner/skill_events.rs
  git commit -m "feat(scanner): add Codex <skill><name> tag extraction with dedup and normalization"
  ```

---

### Task 4: `insert_skill_events` / `list_skill_events` DB layer

**Files:**
- Create: `src/db/skill_events.rs`
- Modify: `src/db.rs` (add `mod skill_events;` and `pub use skill_events::{...}`)
- Test: `src/db/skill_events_tests.rs` (sidecar, hooked from `src/db/skill_events.rs`)

**Interfaces:**
- Consumes: `crate::scanner::skill_events::{ExtractedSkillEvent, SkillEventKind,
  SkillEvidenceKind}` from Tasks 2-3; `crate::db::DbPool` from `src/db/pool.rs`
  (existing)
- Produces: `SkillEventInsert`, `insert_skill_events_in_tx`,
  `insert_skill_events`, `AiSkillEventParams`, `AiSkillEventEntry`,
  `ListSkillEventsResult`, `list_skill_events` — exact signatures in "Locked
  interfaces" above.

- [ ] **Step 1: Write the failing test**

  Create `src/db/skill_events_tests.rs`:
  ```rust
  use super::*;
  use crate::config::StorageConfig;
  use crate::db::pool::init_pool;
  use crate::scanner::skill_events::{ExtractedSkillEvent, SkillEventKind, SkillEvidenceKind};

  fn test_pool() -> (crate::db::DbPool, tempfile::TempDir) {
      let dir = tempfile::tempdir().unwrap();
      let db_path = dir.path().join("test.db");
      let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
      (pool, dir)
  }

  fn insert_log_row(pool: &crate::db::DbPool, hostname: &str, timestamp: &str) -> i64 {
      let conn = pool.get().unwrap();
      conn.execute(
          "INSERT INTO logs (timestamp, hostname, severity, message, raw, source_ip)
           VALUES (?1, ?2, 'info', 'msg', 'raw', 'transcript://claude_project')",
          rusqlite::params![timestamp, hostname],
      )
      .unwrap();
      conn.last_insert_rowid()
  }

  fn sample_event(skill_name: &str) -> ExtractedSkillEvent {
      ExtractedSkillEvent {
          skill_name: skill_name.to_string(),
          skill_plugin: Some("cortex".to_string()),
          skill_path: None,
          event_kind: SkillEventKind::ClaudeAttribution,
          evidence_kind: SkillEvidenceKind::StructuredJsonField,
          metadata_json: None,
      }
  }

  #[test]
  fn insert_and_list_round_trips() {
      let (pool, _dir) = test_pool();
      let log_id = insert_log_row(&pool, "dookie", "2026-06-01T00:00:00.000Z");
      let insert = SkillEventInsert {
          log_id,
          ai_tool: "claude".to_string(),
          ai_project: Some("cortex".to_string()),
          ai_session_id: Some("sess-1".to_string()),
          hostname: "dookie".to_string(),
          timestamp: "2026-06-01T00:00:00.000Z".to_string(),
          event: sample_event("cortex-troubleshoot"),
      };
      let inserted = insert_skill_events(&pool, &[insert]).unwrap();
      assert_eq!(inserted, 1);

      let result = list_skill_events(&pool, &AiSkillEventParams::default()).unwrap();
      assert_eq!(result.total, 1);
      assert_eq!(result.events[0].skill_name, "cortex-troubleshoot");
      assert_eq!(result.events[0].skill_plugin.as_deref(), Some("cortex"));
      assert_eq!(result.events[0].event_kind, "claude_attribution");
      assert_eq!(result.events[0].evidence_kind, "structured_json_field");
      assert_eq!(result.events[0].log_id, log_id);
  }

  #[test]
  fn insert_or_ignore_is_idempotent_on_duplicate() {
      let (pool, _dir) = test_pool();
      let log_id = insert_log_row(&pool, "dookie", "2026-06-01T00:00:00.000Z");
      let insert = SkillEventInsert {
          log_id,
          ai_tool: "claude".to_string(),
          ai_project: None,
          ai_session_id: None,
          hostname: "dookie".to_string(),
          timestamp: "2026-06-01T00:00:00.000Z".to_string(),
          event: sample_event("cortex-troubleshoot"),
      };
      assert_eq!(insert_skill_events(&pool, &[insert.clone()]).unwrap(), 1);
      assert_eq!(insert_skill_events(&pool, &[insert]).unwrap(), 0);

      let result = list_skill_events(&pool, &AiSkillEventParams::default()).unwrap();
      assert_eq!(result.total, 1);
  }

  #[test]
  fn insert_succeeds_without_project_or_session_id() {
      let (pool, _dir) = test_pool();
      let log_id = insert_log_row(&pool, "dookie", "2026-06-01T00:00:00.000Z");
      let insert = SkillEventInsert {
          log_id,
          ai_tool: "codex".to_string(),
          ai_project: None,
          ai_session_id: None,
          hostname: "dookie".to_string(),
          timestamp: "2026-06-01T00:00:00.000Z".to_string(),
          event: ExtractedSkillEvent {
              skill_name: "rustarr".to_string(),
              skill_plugin: None,
              skill_path: None,
              event_kind: SkillEventKind::CodexSkillBlock,
              evidence_kind: SkillEvidenceKind::TranscriptContent,
              metadata_json: None,
          },
      };
      assert_eq!(insert_skill_events(&pool, &[insert]).unwrap(), 1);
      let result = list_skill_events(&pool, &AiSkillEventParams::default()).unwrap();
      assert_eq!(result.events[0].ai_project, None);
      assert_eq!(result.events[0].ai_session_id, None);
  }

  #[test]
  fn list_filters_by_skill_project_and_tool() {
      let (pool, _dir) = test_pool();
      let log_id_a = insert_log_row(&pool, "dookie", "2026-06-01T00:00:00.000Z");
      let log_id_b = insert_log_row(&pool, "tootie", "2026-06-01T01:00:00.000Z");
      insert_skill_events(
          &pool,
          &[
              SkillEventInsert {
                  log_id: log_id_a,
                  ai_tool: "claude".to_string(),
                  ai_project: Some("cortex".to_string()),
                  ai_session_id: Some("sess-a".to_string()),
                  hostname: "dookie".to_string(),
                  timestamp: "2026-06-01T00:00:00.000Z".to_string(),
                  event: sample_event("cortex-troubleshoot"),
              },
              SkillEventInsert {
                  log_id: log_id_b,
                  ai_tool: "codex".to_string(),
                  ai_project: Some("axon".to_string()),
                  ai_session_id: Some("sess-b".to_string()),
                  hostname: "tootie".to_string(),
                  timestamp: "2026-06-01T01:00:00.000Z".to_string(),
                  event: sample_event("axon-deploy"),
              },
          ],
      )
      .unwrap();

      let result = list_skill_events(
          &pool,
          &AiSkillEventParams {
              project: Some("cortex".to_string()),
              ..Default::default()
          },
      )
      .unwrap();
      assert_eq!(result.total, 1);
      assert_eq!(result.events[0].skill_name, "cortex-troubleshoot");

      let result = list_skill_events(
          &pool,
          &AiSkillEventParams {
              tool: Some("codex".to_string()),
              ..Default::default()
          },
      )
      .unwrap();
      assert_eq!(result.total, 1);
      assert_eq!(result.events[0].ai_tool, "codex");
  }
  ```

- [ ] **Step 2: Run test to verify it fails**
  ```bash
  cargo test --lib db::skill_events
  ```
  Expected: compile error — `src/db/skill_events.rs` does not exist.

- [ ] **Step 3: Write minimal implementation**

  Create `src/db/skill_events.rs`:
  ```rust
  //! `ai_skill_events` insert + list query layer. Table/columns are defined in
  //! migration 38 (`src/db/pool.rs`). Extraction happens in
  //! `crate::scanner::skill_events`; this module only persists and reads back
  //! already-extracted events.

  use anyhow::Result;
  use rusqlite::{Transaction, params};
  use serde::{Deserialize, Serialize};

  use crate::scanner::skill_events::ExtractedSkillEvent;

  use super::pool::DbPool;

  #[derive(Debug, Clone)]
  pub struct SkillEventInsert {
      pub log_id: i64,
      pub ai_tool: String,
      pub ai_project: Option<String>,
      pub ai_session_id: Option<String>,
      pub hostname: String,
      pub timestamp: String,
      pub event: ExtractedSkillEvent,
  }

  /// Insert `events` inside an existing transaction with `INSERT OR IGNORE`
  /// (idempotent on the `UNIQUE(log_id, skill_name, event_kind, evidence_kind)`
  /// constraint). Returns the number of rows actually inserted (excludes
  /// ignored duplicates) via SQLite `changes()` summed per statement.
  pub(crate) fn insert_skill_events_in_tx(
      tx: &Transaction<'_>,
      events: &[SkillEventInsert],
  ) -> Result<usize> {
      if events.is_empty() {
          return Ok(0);
      }
      let mut stmt = tx.prepare_cached(
          "INSERT OR IGNORE INTO ai_skill_events (
              log_id, ai_tool, ai_project, ai_session_id, hostname, timestamp,
              skill_name, skill_plugin, skill_path, event_kind, evidence_kind, metadata_json
          ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
      )?;
      let mut inserted = 0usize;
      for item in events {
          let changed = stmt.execute(params![
              item.log_id,
              item.ai_tool,
              item.ai_project,
              item.ai_session_id,
              item.hostname,
              item.timestamp,
              item.event.skill_name,
              item.event.skill_plugin,
              item.event.skill_path,
              item.event.event_kind.as_str(),
              item.event.evidence_kind.as_str(),
              item.event.metadata_json,
          ])?;
          inserted += changed;
      }
      Ok(inserted)
  }

  /// Pool-acquiring wrapper for callers outside an existing transaction (e.g.
  /// the backfill service, which owns its own chunked transaction boundary).
  pub fn insert_skill_events(pool: &DbPool, events: &[SkillEventInsert]) -> Result<usize> {
      let mut conn = pool.get()?;
      let _write_guard = crate::db::write_lock();
      let tx = conn.transaction()?;
      let inserted = insert_skill_events_in_tx(&tx, events)?;
      tx.commit()?;
      Ok(inserted)
  }

  #[derive(Debug, Clone, Default)]
  pub struct AiSkillEventParams {
      pub skill: Option<String>,
      pub plugin: Option<String>,
      pub tool: Option<String>,
      pub project: Option<String>,
      pub session_id: Option<String>,
      pub hostname: Option<String>,
      pub from: Option<String>,
      pub to: Option<String>,
      pub limit: Option<u32>,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct AiSkillEventEntry {
      pub id: i64,
      pub log_id: i64,
      pub ai_tool: String,
      pub ai_project: Option<String>,
      pub ai_session_id: Option<String>,
      pub hostname: String,
      pub timestamp: String,
      pub skill_name: String,
      pub skill_plugin: Option<String>,
      pub skill_path: Option<String>,
      pub event_kind: String,
      pub evidence_kind: String,
      pub metadata_json: Option<String>,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ListSkillEventsResult {
      pub total: usize,
      pub truncated: bool,
      pub events: Vec<AiSkillEventEntry>,
  }

  const DEFAULT_LIMIT: u32 = 50;
  const MAX_LIMIT: u32 = 500;

  /// List `ai_skill_events` rows newest-first, applying every non-`None`
  /// filter in `params` as an `AND`-ed equality/range clause. `limit` is
  /// clamped to `[1, 500]`; `truncated` is `true` when more rows matched than
  /// were returned (probed via `LIMIT + 1`, mirroring `list_ai_tools`'s
  /// truncation-detection pattern in `src/db/queries.rs`).
  pub fn list_skill_events(pool: &DbPool, params: &AiSkillEventParams) -> Result<ListSkillEventsResult> {
      let conn = pool.get()?;
      let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize;

      let mut sql = String::from(
          "SELECT id, log_id, ai_tool, ai_project, ai_session_id, hostname, timestamp,
                  skill_name, skill_plugin, skill_path, event_kind, evidence_kind, metadata_json
           FROM ai_skill_events WHERE 1 = 1",
      );
      let mut bindings: Vec<rusqlite::types::Value> = Vec::new();
      let mut idx = 1usize;

      macro_rules! bind_eq {
          ($column:literal, $value:expr) => {
              if let Some(value) = $value {
                  sql.push_str(&format!(" AND {} = ?{idx}", $column));
                  bindings.push(rusqlite::types::Value::Text(value.clone()));
                  idx += 1;
              }
          };
      }
      bind_eq!("skill_name", &params.skill);
      bind_eq!("skill_plugin", &params.plugin);
      bind_eq!("ai_tool", &params.tool);
      bind_eq!("ai_project", &params.project);
      bind_eq!("ai_session_id", &params.session_id);
      bind_eq!("hostname", &params.hostname);
      if let Some(from) = &params.from {
          sql.push_str(&format!(" AND timestamp >= ?{idx}"));
          bindings.push(rusqlite::types::Value::Text(from.clone()));
          idx += 1;
      }
      if let Some(to) = &params.to {
          sql.push_str(&format!(" AND timestamp <= ?{idx}"));
          bindings.push(rusqlite::types::Value::Text(to.clone()));
          idx += 1;
      }
      let _ = idx;
      sql.push_str(&format!(" ORDER BY timestamp DESC, id DESC LIMIT {}", limit + 1));

      let mut stmt = conn.prepare(&sql)?;
      let mut rows = stmt
          .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
              Ok(AiSkillEventEntry {
                  id: row.get(0)?,
                  log_id: row.get(1)?,
                  ai_tool: row.get(2)?,
                  ai_project: row.get(3)?,
                  ai_session_id: row.get(4)?,
                  hostname: row.get(5)?,
                  timestamp: row.get(6)?,
                  skill_name: row.get(7)?,
                  skill_plugin: row.get(8)?,
                  skill_path: row.get(9)?,
                  event_kind: row.get(10)?,
                  evidence_kind: row.get(11)?,
                  metadata_json: row.get(12)?,
              })
          })?
          .collect::<rusqlite::Result<Vec<_>>>()?;

      let truncated = rows.len() > limit;
      rows.truncate(limit);
      Ok(ListSkillEventsResult {
          total: rows.len(),
          truncated,
          events: rows,
      })
  }

  #[cfg(test)]
  #[path = "skill_events_tests.rs"]
  mod tests;
  ```

  Modify `src/db.rs`: add `mod skill_events;` alongside the other `mod`
  declarations (alphabetical position after `pool`, before `queries` — matches
  existing ordering), and re-export:
  ```rust
  mod skill_events;
  ```
  ```rust
  pub use skill_events::{
      AiSkillEventEntry, AiSkillEventParams, ListSkillEventsResult, SkillEventInsert,
      insert_skill_events, list_skill_events,
  };
  pub(crate) use skill_events::insert_skill_events_in_tx;
  ```

- [ ] **Step 4: Run test to verify it passes**
  ```bash
  cargo test --lib db::skill_events
  ```
  Expected: all 4 tests pass — `test db::skill_events::tests::... ok` x4.

- [ ] **Step 5: Commit**
  ```bash
  git add src/db/skill_events.rs src/db.rs
  git commit -m "feat(db): add ai_skill_events insert and list query layer"
  ```

---

### Task 5: `insert_logs_batch_in_tx` returns inserted log ids

**Files:**
- Modify: `src/db/ingest.rs:34-45,47-126`
- Test: `src/db/ingest_tests.rs`

**Interfaces:**
- Consumes: nothing new
- Produces (signature change, LOCKED for Task 6):
  ```rust
  // before:
  pub(crate) fn insert_logs_batch_in_tx(tx: &Transaction<'_>, entries: &[LogBatchEntry]) -> Result<()>
  // after:
  pub(crate) fn insert_logs_batch_in_tx(tx: &Transaction<'_>, entries: &[LogBatchEntry]) -> Result<Vec<i64>>
  ```
  `pub fn insert_logs_batch(pool: &DbPool, entries: &[LogBatchEntry]) -> Result<usize>`
  signature is UNCHANGED (still returns row count) — every existing caller
  outside `scanner.rs` compiles without modification.

- [ ] **Step 1: Write the failing test**

  Append to `src/db/ingest_tests.rs` (check existing `use` statements at top of
  that file first; add `LogBatchEntry` fixture builder matching whatever helper
  already exists there, e.g. `sample_entry()` — if no such helper exists,
  define one inline):
  ```rust
  #[test]
  fn insert_logs_batch_in_tx_returns_ids_in_input_order() {
      let (pool, _dir) = test_pool();
      let mut conn = pool.get().unwrap();
      let tx = conn.transaction().unwrap();

      let entries = vec![
          LogBatchEntry {
              timestamp: "2026-06-01T00:00:00.000Z".to_string(),
              hostname: "dookie".to_string(),
              facility: None,
              severity: "info".to_string(),
              app_name: None,
              process_id: None,
              message: "first".to_string(),
              raw: "first".to_string(),
              source_ip: "transcript://claude_project".to_string(),
              docker_checkpoint: None,
              ai_tool: Some("claude".to_string()),
              ai_project: None,
              ai_session_id: None,
              ai_transcript_path: None,
              metadata_json: None,
              http_status: None,
              auth_outcome: None,
              dns_blocked: None,
              event_action: None,
              parse_error: None,
          },
          LogBatchEntry {
              timestamp: "2026-06-01T00:00:01.000Z".to_string(),
              hostname: "dookie".to_string(),
              facility: None,
              severity: "info".to_string(),
              app_name: None,
              process_id: None,
              message: "second".to_string(),
              raw: "second".to_string(),
              source_ip: "transcript://claude_project".to_string(),
              docker_checkpoint: None,
              ai_tool: Some("claude".to_string()),
              ai_project: None,
              ai_session_id: None,
              ai_transcript_path: None,
              metadata_json: None,
              http_status: None,
              auth_outcome: None,
              dns_blocked: None,
              event_action: None,
              parse_error: None,
          },
      ];

      let ids = insert_logs_batch_in_tx(&tx, &entries).unwrap();
      tx.commit().unwrap();

      assert_eq!(ids.len(), 2);
      assert!(ids[1] > ids[0], "second row's id must be greater than first");

      let conn = pool.get().unwrap();
      let stored_message: String = conn
          .query_row("SELECT message FROM logs WHERE id = ?1", [ids[0]], |row| row.get(0))
          .unwrap();
      assert_eq!(stored_message, "first");
      let stored_message: String = conn
          .query_row("SELECT message FROM logs WHERE id = ?1", [ids[1]], |row| row.get(0))
          .unwrap();
      assert_eq!(stored_message, "second");
  }
  ```
  If `src/db/ingest_tests.rs` does not already define `test_pool()`, add it
  matching the convention in `src/scanner_tests.rs` / `src/db/queries_tests.rs`
  (`tempfile::tempdir()` + `init_pool(&StorageConfig::for_test(db_path))`).

- [ ] **Step 2: Run test to verify it fails**
  ```bash
  cargo test --lib db::ingest::tests::insert_logs_batch_in_tx_returns_ids_in_input_order
  ```
  Expected: compile error — `insert_logs_batch_in_tx` currently returns
  `Result<()>`, so `let ids = ...unwrap();` followed by `ids.len()` fails to
  typecheck (`()` has no `.len()`).

- [ ] **Step 3: Write minimal implementation**

  In `src/db/ingest.rs`, change the signature and body of
  `insert_logs_batch_in_tx` (currently lines 47-126):
  ```rust
  pub(crate) fn insert_logs_batch_in_tx(
      tx: &Transaction<'_>,
      entries: &[LogBatchEntry],
  ) -> Result<Vec<i64>> {
      let mut ids = Vec::with_capacity(entries.len());
      {
          let mut stmt = tx.prepare_cached(
              "INSERT INTO logs (
                  timestamp, hostname, facility, severity, app_name, process_id,
                  message, raw, source_ip, ai_tool, ai_project, ai_session_id, ai_transcript_path,
                  metadata_json, http_status, auth_outcome, dns_blocked, event_action, parse_error
              ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
          )?;

          for entry in entries {
              stmt.execute(params![
                  entry.timestamp,
                  entry.hostname,
                  entry.facility,
                  entry.severity,
                  entry.app_name,
                  entry.process_id,
                  entry.message,
                  entry.raw,
                  entry.source_ip,
                  entry.ai_tool,
                  entry.ai_project,
                  entry.ai_session_id,
                  entry.ai_transcript_path,
                  entry.metadata_json,
                  entry.http_status,
                  entry.auth_outcome,
                  entry.dns_blocked.map(|b| b as i64),
                  entry.event_action,
                  entry.parse_error,
              ])?;
              ids.push(tx.last_insert_rowid());
          }

          // Batch upsert hosts — group by hostname to avoid one upsert per log entry
          let mut host_counts: HashMap<&str, i64> = HashMap::new();
          for entry in entries {
              *host_counts.entry(entry.hostname.as_str()).or_insert(0) += 1;
          }
          let mut host_stmt = tx.prepare_cached(
              "INSERT INTO hosts (hostname, log_count)
               VALUES (?1, ?2)
               ON CONFLICT(hostname) DO UPDATE SET
                   last_seen = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                   log_count = log_count + excluded.log_count",
          )?;
          for (hostname, count) in &host_counts {
              host_stmt.execute(params![hostname, count])?;
          }
          let mut checkpoint_stmt = tx.prepare_cached(
              "INSERT INTO docker_ingest_checkpoints (host_name, container_id, last_timestamp)
               VALUES (?1, ?2, ?3)
               ON CONFLICT(host_name, container_id) DO UPDATE SET
                   last_timestamp = excluded.last_timestamp,
                   updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
          )?;
          let mut checkpoint_count = 0usize;
          for entry in entries {
              if let Some(checkpoint) = &entry.docker_checkpoint {
                  checkpoint_stmt.execute(params![
                      checkpoint.host_name,
                      checkpoint.container_id,
                      checkpoint.timestamp
                  ])?;
                  checkpoint_count += 1;
              }
          }

          tracing::debug!(
              entry_count = entries.len(),
              unique_hosts = host_counts.len(),
              checkpoint_count,
              "Prepared batch insert transaction"
          );
      }
      Ok(ids)
  }
  ```

  Update `insert_logs_batch_once` (the only in-crate caller) to discard the
  `Vec<i64>` and keep its existing `Result<usize>` contract:
  ```rust
  fn insert_logs_batch_once(pool: &DbPool, entries: &[LogBatchEntry]) -> Result<usize> {
      let mut conn = pool.get()?;
      let _write_guard = crate::db::write_lock();
      let tx = conn.transaction()?;
      let _ids = insert_logs_batch_in_tx(&tx, entries)?;
      tx.commit()?;
      tracing::debug!(
          entry_count = entries.len(),
          "Committed batch insert transaction"
      );
      Ok(entries.len())
  }
  ```

- [ ] **Step 4: Run test to verify it passes**
  ```bash
  cargo test --lib db::ingest
  ```
  Expected: `insert_logs_batch_in_tx_returns_ids_in_input_order` and all
  pre-existing `db::ingest::tests::*` tests pass.

  Also run the full workspace build to confirm no other caller broke (the only
  caller of `insert_logs_batch_in_tx` outside this file is
  `src/scanner.rs::flush_chunk`, which Task 6 updates in lockstep — build will
  fail here until Task 6 lands; that is expected and acceptable within this
  same phase since Task 6 is next):
  ```bash
  cargo build --lib 2>&1 | tail -40
  ```
  Expected: the only remaining error is in `src/scanner.rs` at the
  `insert_logs_batch_in_tx(&tx, &claimed_batch)?;` call site (type mismatch,
  ignored `Vec<i64>` return) — confirms Task 6 is the correct next step.

- [ ] **Step 5: Commit**
  ```bash
  git add src/db/ingest.rs src/db/ingest_tests.rs
  git commit -m "refactor(db): insert_logs_batch_in_tx returns inserted log ids"
  ```

---

### Task 6: Wire skill-event extraction into transcript ingest (`flush_chunk`)

**Files:**
- Modify: `src/scanner.rs:1-13` (imports), `:483-631` (per-line loop in
  `index_file_with_options`), `:774-832` (`flush_chunk`), `:1063-1161` (Gemini
  path — Gemini is explicitly out of scope per the phase spec, which only
  requires Claude + Codex extraction; the Gemini branch passes an empty
  skill-source vector so `flush_chunk`'s new parameter is satisfied uniformly)
- Test: `src/scanner_tests.rs`

**Interfaces:**
- Consumes: `insert_logs_batch_in_tx` returning `Vec<i64>` (Task 5);
  `crate::db::{SkillEventInsert, insert_skill_events_in_tx}` (Task 4);
  `crate::scanner::skill_events::{extract_claude_skill_events,
  extract_codex_skill_events}` (Tasks 2-3)
- Produces: no new public API — this task is pure wiring. Behavior contract:
  every transcript row ingested via `index_file_with_options` /
  `index_roots_with_options` now also inserts zero or more `ai_skill_events`
  rows in the SAME transaction as its `logs` row.

- [ ] **Step 1: Write the failing test**

  Append to `src/scanner_tests.rs`:
  ```rust
  #[test]
  fn indexing_claude_transcript_extracts_skill_events() {
      let (pool, dir) = test_pool();
      let file = dir.path().join("claude-skill.jsonl");
      std::fs::write(
          &file,
          concat!(
              r#"{"sessionId":"sess-1","attributionSkill":"cortex-troubleshoot","attributionPlugin":"cortex","content":"ran troubleshoot"}"#,
              "\n"
          ),
      )
      .unwrap();

      let result = index_file(&pool, &file, "explicit_file").unwrap();
      assert_eq!(result.ingested, 1);

      let conn = pool.get().unwrap();
      let (skill_name, plugin, event_kind): (String, Option<String>, String) = conn
          .query_row(
              "SELECT skill_name, skill_plugin, event_kind FROM ai_skill_events",
              [],
              |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
          )
          .unwrap();
      assert_eq!(skill_name, "cortex-troubleshoot");
      assert_eq!(plugin.as_deref(), Some("cortex"));
      assert_eq!(event_kind, "claude_attribution");
  }

  #[test]
  fn indexing_codex_transcript_extracts_skill_events() {
      let (pool, dir) = test_pool();
      let file = dir.path().join("codex-skill.jsonl");
      std::fs::write(
          &file,
          concat!(
              r#"{"type":"response_item","payload":{"type":"message","content":"<skill><name>rustarr</name></skill> deploying now"},"timestamp":"2026-06-01T00:00:00Z"}"#,
              "\n"
          ),
      )
      .unwrap();

      let result = index_file(&pool, &file, "codex_session").unwrap();
      assert_eq!(result.ingested, 1);

      let conn = pool.get().unwrap();
      let (skill_name, event_kind): (String, String) = conn
          .query_row(
              "SELECT skill_name, event_kind FROM ai_skill_events",
              [],
              |row| Ok((row.get(0)?, row.get(1)?)),
          )
          .unwrap();
      assert_eq!(skill_name, "rustarr");
      assert_eq!(event_kind, "codex_skill_block");
  }

  #[test]
  fn reindexing_same_transcript_does_not_duplicate_skill_events() {
      let (pool, dir) = test_pool();
      let file = dir.path().join("claude-skill-idem.jsonl");
      std::fs::write(
          &file,
          concat!(
              r#"{"sessionId":"sess-1","attributionSkill":"cortex","content":"hi"}"#,
              "\n"
          ),
      )
      .unwrap();

      index_file(&pool, &file, "explicit_file").unwrap();
      let forced = index_file_with_options(
          &pool,
          &file,
          "explicit_file",
          IndexFileOptions { force: true },
          None,
      )
      .unwrap();
      assert_eq!(forced.ingested, 1);

      let conn = pool.get().unwrap();
      let count: i64 = conn
          .query_row("SELECT COUNT(*) FROM ai_skill_events", [], |row| row.get(0))
          .unwrap();
      // force=true re-inserts the logs row (new log_id), so the skill event
      // row is NOT a duplicate by the UNIQUE(log_id, ...) constraint — it is
      // correctly re-created against the new log_id. This asserts the count
      // tracks 1-per-logs-row rather than silently growing unbounded on
      // ordinary (non-forced) re-scans, which is covered by the next test.
      assert_eq!(count, 1);
  }

  #[test]
  fn transcript_row_with_no_skill_reference_creates_no_skill_event() {
      let (pool, dir) = test_pool();
      let file = dir.path().join("no-skill.jsonl");
      std::fs::write(&file, "{\"sessionId\":\"sess-1\",\"content\":\"just chatting\"}\n").unwrap();

      index_file(&pool, &file, "explicit_file").unwrap();

      let conn = pool.get().unwrap();
      let count: i64 = conn
          .query_row("SELECT COUNT(*) FROM ai_skill_events", [], |row| row.get(0))
          .unwrap();
      assert_eq!(count, 0);
  }
  ```

- [ ] **Step 2: Run test to verify it fails**
  ```bash
  cargo test --lib scanner::tests::indexing_claude_transcript_extracts_skill_events
  ```
  Expected: fails at the `conn.query_row("SELECT ... FROM ai_skill_events", ...)`
  step with "Query returned no rows" (or a compile error if `scanner.rs` still
  fails to build from Task 5's signature change) — confirms no skill events are
  written yet.

- [ ] **Step 3: Write minimal implementation**

  In `src/scanner.rs`, update the import block (currently lines 9-13):
  ```rust
  use crate::ai_project::normalize_local_ai_project_path;
  use crate::config::StorageConfig;
  use crate::db::{
      DbPool, LogBatchEntry, SkillEventInsert, enforce_storage_budget, insert_logs_batch_in_tx,
      insert_skill_events_in_tx,
  };
  use crate::ingest_metadata::bounded_metadata_json;
  use crate::receiver::enrichment::{project_from_transcript_path, scrub_ai_message};
  use crate::scanner::skill_events::{extract_claude_skill_events, extract_codex_skill_events};
  ```

  Add a new struct near the top of the file (after `IndexFileOptions`, before
  `CheckpointListOptions` — around line 62):
  ```rust
  /// Raw skill-extraction source paired 1:1 with each `LogBatchEntry` pushed
  /// into a chunk's `batch` vector. Carried alongside `batch`/`imports` because
  /// skill extraction needs the PRE-SCRUB parsed value (Claude JSON) or raw
  /// extracted text (Codex), not the already-scrubbed `LogBatchEntry.message`.
  #[derive(Debug, Clone)]
  enum ChunkSkillSource {
      Claude(serde_json::Value),
      Codex(String),
      None,
  }
  ```

  In `index_file_with_options`, inside the per-line loop (around the existing
  `Ok(Some(parsed)) => { ... }` arm at line 537), the parser currently only
  returns `ParsedTranscriptRecord` (message already extracted, no access to the
  raw `serde_json::Value`). Add a raw-value/text side channel:

  First, change `parse_line_for_source` callers to also retain the raw
  `serde_json::Value` for Claude/ExplicitFile rows. Simplest approach: after
  `parse_line_for_source` succeeds, re-parse `line_text` as JSON once more for
  the skill-extraction side channel (cheap — same JSON already parsed once
  inside `claude::parse_line`/`codex::parse_line`; a second `serde_json::from_str`
  here avoids threading a new return field through `ParsedTranscriptRecord`,
  which is `pub(crate)` and read by both parser modules and would otherwise
  require touching `codex.rs`/`claude.rs` return types — this local re-parse is
  the minimal-diff choice):
  ```rust
              Ok(Some(parsed)) => {
                  let record_key = parsed.record_key;
                  let message = scrub_ai_message(&parsed.message, None);
                  let skill_source = match source_kind {
                      SourceKind::CodexSession => ChunkSkillSource::Codex(parsed.message.clone()),
                      SourceKind::ClaudeProject | SourceKind::ExplicitFile => {
                          match serde_json::from_str::<serde_json::Value>(line_text) {
                              Ok(value) => ChunkSkillSource::Claude(value),
                              Err(_) => ChunkSkillSource::None,
                          }
                      }
                      SourceKind::GeminiSession => ChunkSkillSource::None,
                  };
                  let project_candidate = parsed
  ```
  (the rest of that arm is unchanged up through `imports.push(record_key);`).

  Add a new `skill_sources: Vec<ChunkSkillSource>` vector alongside `batch` and
  `imports` (declared near line 484, `let mut batch = Vec::new();`):
  ```rust
      let mut imports = Vec::new();
      let mut batch = Vec::new();
      let mut skill_sources = Vec::new();
      let mut chunk_bytes = 0usize;
  ```
  Push into it in lockstep with `batch.push(entry);` / `imports.push(record_key);`:
  ```rust
                  chunk_bytes = chunk_bytes.saturating_add(log_entry_string_bytes(&entry));
                  batch.push(entry);
                  imports.push(record_key);
                  skill_sources.push(skill_source);
                  if batch.len() >= MAX_INDEX_CHUNK_RECORDS || chunk_bytes >= MAX_INDEX_CHUNK_BYTES {
                      if !flush_chunk(
                          pool,
                          storage,
                          source_id,
                          &mut batch,
                          &mut imports,
                          &mut skill_sources,
                          None,
                          &mut result,
                      )? {
                          return Ok(result);
                      }
                      chunk_bytes = 0;
                  }
  ```

  Update both remaining `flush_chunk(...)` call sites inside
  `index_file_with_options` (the `result.parse_errors > 0` early-return branch
  and the final flush) to pass `&mut skill_sources` in the same position, and
  clear it exactly where `imports.clear()` already happens (inside
  `flush_chunk` itself, per below) — do NOT clear it at the call sites.

  Now update `flush_chunk`'s signature and body (currently lines 774-832):
  ```rust
  fn flush_chunk(
      pool: &DbPool,
      storage: Option<&StorageConfig>,
      source_id: i64,
      batch: &mut Vec<LogBatchEntry>,
      imports: &mut Vec<String>,
      skill_sources: &mut Vec<ChunkSkillSource>,
      completion_metadata: Option<&FileMetadata>,
      result: &mut IndexResult,
  ) -> Result<bool> {
      if batch.is_empty() {
          skill_sources.clear();
          if let Some(file_metadata) = completion_metadata {
              let mut conn = pool.get()?;
              let _write_guard = crate::db::write_lock();
              let tx = conn.transaction()?;
              checkpoint::update_source_metadata_in_tx(&tx, source_id, file_metadata)?;
              tx.commit()?;
              result.checkpoint_updates += 1;
          }
          return Ok(true);
      }

      if let Some(storage) = storage {
          let outcome = enforce_storage_budget(pool, storage)?;
          if outcome.write_blocked {
              result.storage_blocked_chunks += 1;
              batch.clear();
              imports.clear();
              skill_sources.clear();
              return Ok(false);
          }
      }

      let mut conn = pool.get()?;
      let _write_guard = crate::db::write_lock();
      let tx = conn.transaction()?;
      let claimed = checkpoint::claim_imports_in_tx(&tx, source_id, imports)?;
      let mut claimed_batch = Vec::with_capacity(batch.len());
      let mut claimed_skill_sources = Vec::with_capacity(skill_sources.len());
      let mut skipped_dupes = 0usize;
      for ((entry, claimed), skill_source) in
          batch.drain(..).zip(claimed).zip(skill_sources.drain(..))
      {
          if claimed {
              claimed_batch.push(entry);
              claimed_skill_sources.push(skill_source);
          } else {
              skipped_dupes += 1;
          }
      }
      if !claimed_batch.is_empty() {
          let log_ids = insert_logs_batch_in_tx(&tx, &claimed_batch)?;
          let mut skill_inserts = Vec::new();
          for ((entry, log_id), skill_source) in claimed_batch
              .iter()
              .zip(log_ids.iter().copied())
              .zip(claimed_skill_sources.iter())
          {
              let extracted = match skill_source {
                  ChunkSkillSource::Claude(value) => extract_claude_skill_events(value),
                  ChunkSkillSource::Codex(text) => extract_codex_skill_events(text),
                  ChunkSkillSource::None => Vec::new(),
              };
              for event in extracted {
                  skill_inserts.push(SkillEventInsert {
                      log_id,
                      ai_tool: entry.ai_tool.clone().unwrap_or_default(),
                      ai_project: entry.ai_project.clone(),
                      ai_session_id: entry.ai_session_id.clone(),
                      hostname: entry.hostname.clone(),
                      timestamp: entry.timestamp.clone(),
                      event,
                  });
              }
          }
          if !skill_inserts.is_empty() {
              insert_skill_events_in_tx(&tx, &skill_inserts)?;
          }
      }
      if let Some(file_metadata) = completion_metadata {
          checkpoint::update_source_metadata_in_tx(&tx, source_id, file_metadata)?;
      }
      tx.commit()?;
      result.ingested += claimed_batch.len();
      result.skipped_dupes += skipped_dupes;
      if completion_metadata.is_some() {
          result.checkpoint_updates += 1;
      }
      imports.clear();
      Ok(true)
  }
  ```

  Finally, update the Gemini path (`index_gemini_file`, around lines
  1026-1197): it calls `flush_chunk` three times with the same signature. Add
  a local `let mut skill_sources: Vec<ChunkSkillSource> = Vec::new();` right
  next to its existing `let mut imports = Vec::new();` (Gemini rows never
  produce skill events — Gemini extraction is explicitly out of scope for this
  phase), push `ChunkSkillSource::None` in lockstep with each
  `imports.push(record_key);` in that function, and pass `&mut skill_sources`
  to each of its three `flush_chunk(...)` calls.

- [ ] **Step 4: Run test to verify it passes**
  ```bash
  cargo test --lib scanner
  ```
  Expected: all `scanner::tests::*` pass including the 4 new tests from Step 1,
  and no regressions in the pre-existing scanner test suite (idempotency,
  force-reindex, checkpoint, gemini tests).

  Also run the full lib build + clippy to confirm the refactor is clean:
  ```bash
  cargo build --lib && cargo clippy --lib -- -D warnings
  ```

- [ ] **Step 5: Commit**
  ```bash
  git add src/scanner.rs
  git commit -m "feat(scanner): extract and persist ai_skill_events during transcript ingest"
  ```

---

### Task 7: Backfill service method (`CortexService::backfill_skill_events`)

**Files:**
- Create: `src/app/services/skill_backfill.rs`
- Modify: `src/app/services.rs` (add `mod skill_backfill;` near the other
  `mod ai_indexing;` line, and re-export the new request/result types from
  `src/app/models.rs` — see below)
- Create: `src/app/models/skill_backfill.rs` (request/result structs, following
  the pattern of `src/app/models/ai_inventory.rs`)
- Modify: `src/app/models.rs` (add `mod skill_backfill;` and re-export)
- Test: `src/app/services/skill_backfill_tests.rs` (new sidecar; note
  `src/app/services.rs` itself has no direct sidecar — service submodules like
  `ai_indexing.rs` typically colocate `#[cfg(test)] mod tests { ... }` inline
  OR use a sidecar; check `src/app/services/ai_indexing.rs`'s tail to confirm
  which convention it uses and mirror exactly that one for `skill_backfill.rs`)

**Interfaces:**
- Consumes: `crate::db::{list_hosts is NOT used here — instead, this task
  queries `logs` directly via a new lightweight `SELECT id, ...FROM logs WHERE
  ai_tool IN ('claude','codex') ...` query added inside
  `skill_backfill.rs` itself (not `queries.rs`), since it is backfill-specific
  and chunked}; `crate::db::{SkillEventInsert, insert_skill_events}` (Task 4);
  `crate::scanner::skill_events::{extract_claude_skill_events,
  extract_codex_skill_events}` (Tasks 2-3); `crate::db::write_lock` (existing)
- Produces: `SkillBackfillRequest`, `SkillBackfillResult` (locked above),
  `CortexService::backfill_skill_events`

- [ ] **Step 1: Write the failing test**

  First inspect the sidecar convention:
  ```bash
  tail -20 src/app/services/ai_indexing.rs
  ```
  Use whichever pattern that shows (inline `#[cfg(test)] mod tests { use
  super::*; ... }` vs `#[path = "..._tests.rs"]`). The steps below assume the
  sidecar-file convention (matching every other module in this phase); if
  `ai_indexing.rs` instead uses an inline test module, use that shape instead
  for `skill_backfill.rs` and skip creating a separate `_tests.rs` file.

  Create `src/app/services/skill_backfill_tests.rs`:
  ```rust
  use super::*;
  use crate::app::CortexService;
  use crate::config::StorageConfig;
  use crate::db::{DbPool, init_pool};
  use std::sync::Arc;

  fn test_service() -> (CortexService, tempfile::TempDir) {
      let dir = tempfile::tempdir().unwrap();
      let db_path = dir.path().join("test.db");
      let storage = StorageConfig::for_test(db_path);
      let pool: Arc<DbPool> = Arc::new(init_pool(&storage).unwrap());
      (CortexService::new(pool, storage), dir)
  }

  fn insert_claude_log_row(pool: &DbPool, message: &str) -> i64 {
      let conn = pool.get().unwrap();
      conn.execute(
          "INSERT INTO logs (timestamp, hostname, severity, message, raw, source_ip, ai_tool, ai_project, ai_session_id)
           VALUES ('2026-06-01T00:00:00.000Z', 'dookie', 'info', ?1, ?1, 'transcript://claude_project', 'claude', 'cortex', 'sess-1')",
          rusqlite::params![message],
      )
      .unwrap();
      conn.last_insert_rowid()
  }

  #[tokio::test]
  async fn dry_run_reports_counts_without_inserting() {
      let (service, _dir) = test_service();
      let pool = service.pool_for_test();
      insert_claude_log_row(&pool, r#"{"attributionSkill":"cortex-troubleshoot"}"#);

      let result = service
          .backfill_skill_events(SkillBackfillRequest {
              since: None,
              limit: Some(100),
              dry_run: true,
          })
          .await
          .unwrap();

      assert_eq!(result.scanned, 1);
      assert_eq!(result.inserted, 0);
      assert!(result.dry_run);

      let conn = pool.get().unwrap();
      let count: i64 = conn
          .query_row("SELECT COUNT(*) FROM ai_skill_events", [], |row| row.get(0))
          .unwrap();
      assert_eq!(count, 0);
  }

  #[tokio::test]
  async fn real_run_inserts_events_and_is_idempotent() {
      let (service, _dir) = test_service();
      let pool = service.pool_for_test();
      insert_claude_log_row(&pool, r#"{"attributionSkill":"cortex-troubleshoot"}"#);

      let first = service
          .backfill_skill_events(SkillBackfillRequest {
              since: None,
              limit: Some(100),
              dry_run: false,
          })
          .await
          .unwrap();
      assert_eq!(first.scanned, 1);
      assert_eq!(first.inserted, 1);
      assert_eq!(first.skipped_duplicates, 0);

      let second = service
          .backfill_skill_events(SkillBackfillRequest {
              since: None,
              limit: Some(100),
              dry_run: false,
          })
          .await
          .unwrap();
      assert_eq!(second.scanned, 1);
      assert_eq!(second.inserted, 0);
      assert_eq!(second.skipped_duplicates, 1);
  }
  ```

  This test assumes a `pool_for_test()` accessor exists on `CortexService` for
  tests to reach the underlying pool directly. Check
  `src/app/services.rs`/`src/app/service_tests.rs` for whether such an accessor
  already exists (search `grep -n "fn pool_for_test\|pub(crate) fn pool\b"
  src/app/services.rs`). If it does not exist, add:
  ```rust
  #[cfg(test)]
  pub(crate) fn pool_for_test(&self) -> Arc<DbPool> {
      Arc::clone(&self.pool)
  }
  ```
  to the `impl CortexService` block in `src/app/services.rs`.

- [ ] **Step 2: Run test to verify it fails**
  ```bash
  cargo test --lib app::services::skill_backfill
  ```
  Expected: compile error — `SkillBackfillRequest`/`backfill_skill_events` do
  not exist yet.

- [ ] **Step 3: Write minimal implementation**

  Create `src/app/models/skill_backfill.rs`:
  ```rust
  use serde::{Deserialize, Serialize};

  #[derive(Debug, Clone, Default, Deserialize)]
  pub struct SkillBackfillRequest {
      pub since: Option<String>,
      pub limit: Option<u64>,
      #[serde(default)]
      pub dry_run: bool,
  }

  #[derive(Debug, Clone, Default, Serialize, Deserialize)]
  pub struct SkillBackfillResult {
      pub scanned: u64,
      pub inserted: u64,
      pub skipped_duplicates: u64,
      pub parse_errors: u64,
      pub truncated: bool,
      pub dry_run: bool,
  }
  ```

  In `src/app/models.rs`, add `mod skill_backfill;` and
  `pub use skill_backfill::{SkillBackfillRequest, SkillBackfillResult};`
  next to the other `mod`/`pub use` lines (match existing alphabetical-ish
  grouping in that file).

  Create `src/app/services/skill_backfill.rs`:
  ```rust
  //! `ai skills backfill` — scans existing `logs` rows with `ai_tool IN
  //! ('claude','codex')` and extracts/persists `ai_skill_events` for rows that
  //! predate this phase's ingest-time wiring (`src/scanner.rs::flush_chunk`).
  //!
  //! Chunked scan-and-release: each chunk acquires a fresh pool connection,
  //! processes up to `CHUNK_SIZE` rows in one transaction, commits, and drops
  //! the connection before continuing — mirrors `purge_old_logs` in
  //! `src/db/maintenance.rs` so a large historical backfill never starves the
  //! ingest writer of a pool connection for more than one chunk's duration.

  use anyhow::Result;
  use rusqlite::params;

  use crate::db::{DbPool, SkillEventInsert, insert_skill_events};
  use crate::scanner::skill_events::{extract_claude_skill_events, extract_codex_skill_events};

  use super::super::models::{SkillBackfillRequest, SkillBackfillResult};
  use super::super::time::parse_optional_timestamp;
  use super::{CortexService, ServiceResult};

  const CHUNK_SIZE: i64 = 2_000;

  struct CandidateRow {
      id: i64,
      ai_tool: String,
      ai_project: Option<String>,
      ai_session_id: Option<String>,
      hostname: String,
      timestamp: String,
      message: String,
  }

  impl CortexService {
      pub async fn backfill_skill_events(
          &self,
          req: SkillBackfillRequest,
      ) -> ServiceResult<SkillBackfillResult> {
          let since = parse_optional_timestamp(req.since.as_deref(), "since")?
              .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string());
          let limit = req.limit.unwrap_or(10_000).max(1);
          let dry_run = req.dry_run;

          self.run_db("backfill_skill_events", move |pool| {
              run_backfill(pool, since.as_deref(), limit, dry_run)
          })
          .await
      }
  }

  fn run_backfill(
      pool: &DbPool,
      since: Option<&str>,
      limit: u64,
      dry_run: bool,
  ) -> Result<SkillBackfillResult> {
      let mut result = SkillBackfillResult {
          dry_run,
          ..Default::default()
      };
      let mut last_id = 0i64;
      let mut remaining = limit;

      loop {
          if remaining == 0 {
              result.truncated = true;
              break;
          }
          let chunk_limit = CHUNK_SIZE.min(remaining as i64).max(1);
          let conn = pool.get()?;
          let _write_guard = crate::db::write_lock();
          let rows = fetch_candidate_chunk(&conn, since, last_id, chunk_limit)?;
          drop(conn); // release back to pool before per-row parse work + next chunk

          if rows.is_empty() {
              break;
          }
          last_id = rows.last().map(|r| r.id).unwrap_or(last_id);
          result.scanned += rows.len() as u64;
          remaining = remaining.saturating_sub(rows.len() as u64);

          let mut inserts = Vec::new();
          for row in &rows {
              let extracted = match row.ai_tool.as_str() {
                  "claude" => match serde_json::from_str::<serde_json::Value>(&row.message) {
                      Ok(value) => extract_claude_skill_events(&value),
                      Err(_) => {
                          result.parse_errors += 1;
                          continue;
                      }
                  },
                  "codex" => extract_codex_skill_events(&row.message),
                  _ => continue,
              };
              for event in extracted {
                  inserts.push(SkillEventInsert {
                      log_id: row.id,
                      ai_tool: row.ai_tool.clone(),
                      ai_project: row.ai_project.clone(),
                      ai_session_id: row.ai_session_id.clone(),
                      hostname: row.hostname.clone(),
                      timestamp: row.timestamp.clone(),
                      event,
                  });
              }
          }

          if !dry_run && !inserts.is_empty() {
              let attempted = inserts.len() as u64;
              let inserted = insert_skill_events(pool, &inserts)? as u64;
              result.inserted += inserted;
              result.skipped_duplicates += attempted - inserted;
          } else if dry_run {
              // Dry-run still reports how many events WOULD be inserted as
              // "inserted" would be misleading; instead every extracted event
              // in a dry run counts toward neither inserted nor duplicates —
              // scanned/parse_errors are the only meaningful dry-run signal
              // per the CLI contract (`--dry-run` reports scanned rows and
              // parse errors without touching the table). Callers that need a
              // precise "would insert N" count should drop --dry-run.
          }

          if (rows.len() as i64) < chunk_limit {
              break;
          }
          std::thread::sleep(std::time::Duration::from_millis(10));
      }

      Ok(result)
  }

  fn fetch_candidate_chunk(
      conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
      since: Option<&str>,
      last_id: i64,
      chunk_limit: i64,
  ) -> Result<Vec<CandidateRow>> {
      let (sql, bindings): (&str, Vec<rusqlite::types::Value>) = match since {
          Some(since) => (
              "SELECT id, ai_tool, ai_project, ai_session_id, hostname, timestamp, message
               FROM logs
               WHERE ai_tool IN ('claude', 'codex')
                 AND id > ?1
                 AND timestamp >= ?2
               ORDER BY id ASC
               LIMIT ?3",
              vec![
                  rusqlite::types::Value::Integer(last_id),
                  rusqlite::types::Value::Text(since.to_string()),
                  rusqlite::types::Value::Integer(chunk_limit),
              ],
          ),
          None => (
              "SELECT id, ai_tool, ai_project, ai_session_id, hostname, timestamp, message
               FROM logs
               WHERE ai_tool IN ('claude', 'codex')
                 AND id > ?1
               ORDER BY id ASC
               LIMIT ?2",
              vec![
                  rusqlite::types::Value::Integer(last_id),
                  rusqlite::types::Value::Integer(chunk_limit),
              ],
          ),
      };
      let mut stmt = conn.prepare(sql)?;
      let rows = stmt
          .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
              Ok(CandidateRow {
                  id: row.get(0)?,
                  ai_tool: row.get(1)?,
                  ai_project: row.get(2)?,
                  ai_session_id: row.get(3)?,
                  hostname: row.get(4)?,
                  timestamp: row.get(5)?,
                  message: row.get(6)?,
              })
          })?
          .collect::<rusqlite::Result<Vec<_>>>()?;
      Ok(rows)
  }

  #[cfg(test)]
  #[path = "skill_backfill_tests.rs"]
  mod tests;
  ```

  NOTE for implementer: `r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>`
  is the concrete type `pool.get()` returns in this codebase — confirm the
  exact type alias by checking `type DbPool = r2d2::Pool<...>` in
  `src/db/pool.rs` and use whatever type alias (e.g. `crate::db::PooledConn` if
  one is already exported) is idiomatic here instead of spelling out the full
  generic, to match house style. If no such alias is exported, add
  `pub(crate) type PooledConn<'a> = ...` or just use `&rusqlite::Connection`
  by deref — check how `src/db/maintenance.rs`'s chunked functions type their
  `conn` parameter and mirror that exactly (grep
  `fn delete_heartbeat_chunk_where` signature for the pattern).

  Modify `src/app/services.rs`: add `mod skill_backfill;` next to
  `mod ai_indexing;` (around line 64).

- [ ] **Step 4: Run test to verify it passes**
  ```bash
  cargo test --lib app::services::skill_backfill
  ```
  Expected: both tests pass —
  `dry_run_reports_counts_without_inserting` and
  `real_run_inserts_events_and_is_idempotent`.

- [ ] **Step 5: Commit**
  ```bash
  git add src/app/services/skill_backfill.rs src/app/services/skill_backfill_tests.rs src/app/models/skill_backfill.rs src/app/models.rs src/app/services.rs
  git commit -m "feat(app): add backfill_skill_events service method with dry-run and idempotent chunked scan"
  ```

---

### Task 8: `cortex sessions skills backfill` CLI command

**Files:**
- Modify: `src/cli/args.rs` (add `SessionsCommand::Skills(SessionsSkillsCommand)`
  variant, `SessionsSkillsCommand` enum, `SessionsSkillsBackfillArgs` struct)
- Modify: `src/cli/parse/sessions.rs` (add `"skills"` to
  `SESSIONS_SUBCOMMANDS`, dispatch to a new `parse_sessions_skills` function)
- Create: `src/cli/parse/sessions/skills.rs` (parse functions, following
  `src/cli/parse/sessions/ops.rs` shape)
- Modify: `src/cli/run.rs` (dispatch `SessionsCommand::Skills(...)` arm)
- Modify: `src/cli/dispatch_sessions.rs` (add `run_ai_skills_backfill`,
  `run_ai_skills_list` — the latter is the read surface built out fully in
  Task 9; this task only needs the backfill arm, but declare both stubs so
  Task 9 slots in cleanly)
- Test: `src/cli/parse/sessions_tests.rs` or wherever `parse_sessions_*` tests
  live (`grep -rn "parse_sessions_tools" src/cli --include="*_tests.rs"` to
  find the exact sidecar file; append there)

**Interfaces:**
- Consumes: `CortexService::backfill_skill_events` (Task 7)
- Produces: `cortex sessions skills backfill --since 30d --limit 10000
  --dry-run` and `cortex sessions skills backfill --since 30d --limit 10000`
  as working CLI invocations (Local mode only — HTTP mode explicitly rejected
  with a clear error, matching `run_ai_index`'s pattern for local-filesystem
  operations, since this is a DB-heavy batch job that should run against the
  local `CortexService`, not proxied over HTTP per this repo's HTTP-CLI-parity
  rollout status — confirm current HTTP support scope by checking whether
  `HttpClient` has an analogous batch endpoint; if unsure, follow
  `run_ai_index`'s `CliMode::Http(_) => bail!(...)` precedent exactly)

- [ ] **Step 1: Write the failing test**

  First locate the exact sidecar test file for `src/cli/parse/sessions.rs`:
  ```bash
  find src/cli -iname "*sessions*test*"
  ```
  Append to whichever file that search returns (most likely
  `src/cli/parse/sessions_tests.rs` or `src/cli/parse_tests.rs` if sessions
  parsing tests live centrally — inspect and match existing test style there,
  e.g. `parse_command(vec!["sessions".into(), "tools".into()])`):
  ```rust
  #[test]
  fn parses_sessions_skills_backfill_with_flags() {
      let command = super::parse_command(vec![
          "sessions".to_string(),
          "skills".to_string(),
          "backfill".to_string(),
          "--since".to_string(),
          "30d".to_string(),
          "--limit".to_string(),
          "10000".to_string(),
          "--dry-run".to_string(),
      ])
      .unwrap();

      match command {
          CliCommand::Sessions(SessionsCommand::SkillsBackfill(args)) => {
              assert_eq!(args.since.as_deref(), Some("30d"));
              assert_eq!(args.limit, Some(10000));
              assert!(args.dry_run);
          }
          other => panic!("expected SessionsCommand::SkillsBackfill, got {other:?}"),
      }
  }

  #[test]
  fn parses_sessions_skills_list_with_project_filter() {
      let command = super::parse_command(vec![
          "sessions".to_string(),
          "skills".to_string(),
          "--project".to_string(),
          "cortex".to_string(),
          "--limit".to_string(),
          "20".to_string(),
      ])
      .unwrap();

      match command {
          CliCommand::Sessions(SessionsCommand::Skills(args)) => {
              assert_eq!(args.project.as_deref(), Some("cortex"));
              assert_eq!(args.limit, Some(20));
          }
          other => panic!("expected SessionsCommand::Skills, got {other:?}"),
      }
  }
  ```
  Adjust the `super::parse_command` path/import to match whatever the
  discovered test file already uses (some files call `parse_command` directly
  via `use super::super::parse_command;` or similar — mirror the neighboring
  `parses_sessions_tools_*`-style test in the same file exactly).

- [ ] **Step 2: Run test to verify it fails**
  ```bash
  cargo test --lib cli::parse::sessions::tests::parses_sessions_skills_backfill_with_flags
  ```
  Expected: compile error — `SessionsCommand::SkillsBackfill` /
  `SessionsCommand::Skills` do not exist.

- [ ] **Step 3: Write minimal implementation**

  In `src/cli/args.rs`, find `enum SessionsCommand` and add two variants
  (alongside `Tools(SessionsListArgs)` / `Index(SessionsIndexArgs)`):
  ```rust
  Skills(SessionsSkillsListArgs),
  SkillsBackfill(SessionsSkillsBackfillArgs),
  ```
  Add the two new arg structs near `SessionsListArgs`:
  ```rust
  #[derive(Debug, Clone, Default, PartialEq, Eq)]
  pub(crate) struct SessionsSkillsListArgs {
      pub json: bool,
      pub skill: Option<String>,
      pub plugin: Option<String>,
      pub tool: Option<String>,
      pub project: Option<String>,
      pub session_id: Option<String>,
      pub host: Option<String>,
      pub since: Option<String>,
      pub until: Option<String>,
      pub limit: Option<u32>,
  }

  #[derive(Debug, Clone, Default, PartialEq, Eq)]
  pub(crate) struct SessionsSkillsBackfillArgs {
      pub json: bool,
      pub since: Option<String>,
      pub limit: Option<u64>,
      pub dry_run: bool,
  }
  ```

  In `src/cli/parse/sessions.rs`:
  - Add `"skills"` to `SESSIONS_SUBCOMMANDS`.
  - Add a dispatch arm in `parse_sessions_command`:
    ```rust
    "skills" => super::skills::parse_sessions_skills(rest),
    ```
  - Add `mod skills;` near the top (alongside `mod more; mod ops;`).

  Create `src/cli/parse/sessions/skills.rs`:
  ```rust
  use anyhow::{Result, bail};

  use super::super::super::parse_common::{FlagCursor, norm_time, value_after_equals};
  use super::super::super::{
      CliCommand, SessionsCommand, SessionsSkillsBackfillArgs, SessionsSkillsListArgs,
  };

  pub(crate) fn parse_sessions_skills(args: &[String]) -> Result<CliCommand> {
      match args.first().map(String::as_str) {
          Some("backfill") => parse_skills_backfill(&args[1..]),
          _ => parse_skills_list(args),
      }
  }

  fn parse_skills_list(args: &[String]) -> Result<CliCommand> {
      let mut parsed = SessionsSkillsListArgs::default();
      let mut flags = FlagCursor::new(args);
      while let Some(arg) = flags.next() {
          match arg.as_str() {
              "--json" => parsed.json = true,
              "--skill" => parsed.skill = Some(flags.value("--skill")?),
              "--plugin" => parsed.plugin = Some(flags.value("--plugin")?),
              "--tool" => parsed.tool = Some(flags.value("--tool")?),
              "--project" => parsed.project = Some(flags.value("--project")?),
              "--session-id" => parsed.session_id = Some(flags.value("--session-id")?),
              "--host" => parsed.host = Some(flags.value("--host")?),
              "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
              "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
              "--limit" => parsed.limit = Some(flags.value("--limit")?.parse()?),
              _ if arg.starts_with("--skill=") => {
                  parsed.skill = Some(value_after_equals(arg, "--skill")?)
              }
              _ if arg.starts_with("--plugin=") => {
                  parsed.plugin = Some(value_after_equals(arg, "--plugin")?)
              }
              _ if arg.starts_with("--tool=") => {
                  parsed.tool = Some(value_after_equals(arg, "--tool")?)
              }
              _ if arg.starts_with("--project=") => {
                  parsed.project = Some(value_after_equals(arg, "--project")?)
              }
              _ if arg.starts_with("--session-id=") => {
                  parsed.session_id = Some(value_after_equals(arg, "--session-id")?)
              }
              _ if arg.starts_with("--host=") => {
                  parsed.host = Some(value_after_equals(arg, "--host")?)
              }
              _ if arg.starts_with("--since=") => {
                  parsed.since = Some(norm_time(value_after_equals(arg, "--since")?)?)
              }
              _ if arg.starts_with("--until=") => {
                  parsed.until = Some(norm_time(value_after_equals(arg, "--until")?)?)
              }
              _ if arg.starts_with("--limit=") => {
                  parsed.limit = Some(value_after_equals(arg, "--limit")?.parse()?)
              }
              _ => bail!("unknown sessions skills option: {arg}"),
          }
      }
      Ok(CliCommand::Sessions(SessionsCommand::Skills(parsed)))
  }

  fn parse_skills_backfill(args: &[String]) -> Result<CliCommand> {
      let mut parsed = SessionsSkillsBackfillArgs::default();
      let mut flags = FlagCursor::new(args);
      while let Some(arg) = flags.next() {
          match arg.as_str() {
              "--json" => parsed.json = true,
              "--dry-run" => parsed.dry_run = true,
              "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
              "--limit" => parsed.limit = Some(flags.value("--limit")?.parse()?),
              _ if arg.starts_with("--since=") => {
                  parsed.since = Some(norm_time(value_after_equals(arg, "--since")?)?)
              }
              _ if arg.starts_with("--limit=") => {
                  parsed.limit = Some(value_after_equals(arg, "--limit")?.parse()?)
              }
              _ => bail!("unknown sessions skills backfill option: {arg}"),
          }
      }
      Ok(CliCommand::Sessions(SessionsCommand::SkillsBackfill(parsed)))
  }
  ```
  Adjust the exact `FlagCursor`/`norm_time`/`value_after_equals` import path if
  the real module path differs slightly from this guess — mirror
  `src/cli/parse/sessions.rs`'s own `use super::super::parse_common::{...}`
  line exactly (this new file is one directory level deeper, so it needs
  `super::super::super::parse_common`; verify against
  `src/cli/parse/sessions/ops.rs`'s actual import line and copy it verbatim
  with the module path adjusted).

  In `src/cli/run.rs`, add to the `CliCommand::Sessions(command) => match
  command { ... }` block:
  ```rust
  super::SessionsCommand::Skills(args) => dispatch::run_ai_skills(&mode, args).await,
  super::SessionsCommand::SkillsBackfill(args) => {
      dispatch::run_ai_skills_backfill(&mode, args).await
  }
  ```

  In `src/cli/dispatch_sessions.rs`, add (near `run_ai_index`):
  ```rust
  pub(crate) async fn run_ai_skills_backfill(
      mode: &CliMode,
      args: SessionsSkillsBackfillArgs,
  ) -> Result<()> {
      let service = match mode {
          CliMode::Http(_) => bail!("sessions skills backfill runs local DB scans; omit --http"),
          CliMode::Local(service) => service,
      };
      let response = service
          .backfill_skill_events(cortex::app::SkillBackfillRequest {
              since: args.since,
              limit: args.limit,
              dry_run: args.dry_run,
          })
          .await?;
      if args.json {
          println!("{}", serde_json::to_string_pretty(&response)?);
      } else {
          println!(
              "scanned={} inserted={} skipped_duplicates={} parse_errors={} truncated={} dry_run={}",
              response.scanned,
              response.inserted,
              response.skipped_duplicates,
              response.parse_errors,
              response.truncated,
              response.dry_run
          );
      }
      Ok(())
  }
  ```
  Leave `run_ai_skills` (the list/read variant) as a stub returning
  `bail!("not yet implemented")` for now — Task 9 replaces it with the real
  implementation. Confirm `cortex::app::SkillBackfillRequest` is actually
  re-exported from `src/app.rs`'s top-level `pub use` (check
  `grep -n "pub use services" src/app.rs` and add `SkillBackfillRequest,
  SkillBackfillResult` to that re-export list if `src/app/services.rs`'s
  `models` re-export doesn't already surface them transitively).

- [ ] **Step 4: Run test to verify it passes**
  ```bash
  cargo test --lib cli::parse::sessions::tests::parses_sessions_skills_backfill_with_flags
  cargo test --lib cli::parse::sessions::tests::parses_sessions_skills_list_with_project_filter
  cargo build --bin cortex
  ```
  Expected: both parse tests pass; binary builds (dispatch arms compile even
  though `run_ai_skills` is a stub, since Task 9 fills it in next).

- [ ] **Step 5: Commit**
  ```bash
  git add src/cli/args.rs src/cli/parse/sessions.rs src/cli/parse/sessions/skills.rs src/cli/run.rs src/cli/dispatch_sessions.rs
  git commit -m "feat(cli): add cortex sessions skills backfill command"
  ```

---

### Task 9: `skill_events` MCP action + CLI list + REST route

**Files:**
- Modify: `src/mcp/actions.rs` (add `ActionHandler::SkillEvents` variant, add
  `action_spec!("skill_events", Read, ..., SkillEvents, ...)` row)
- Modify: `src/mcp/tools.rs` (add `H::SkillEvents => tool_skill_events(state,
  args).await` dispatch arm + `tool_skill_events` function)
- Modify: `src/app/services/skill_backfill.rs` OR create
  `src/app/services/skill_events.rs` (add `CortexService::list_skill_events`
  wrapping `db::list_skill_events`)
- Modify: `src/app/models/skill_backfill.rs` -> rename/extend, or add
  `src/app/models/skill_events.rs` with `ListSkillEventsRequest`,
  `ListSkillEventsResponse` (API-facing types distinct from the DB-layer
  `AiSkillEventParams`/`AiSkillEventEntry`, mirroring how `ListAiToolsRequest`
  (app layer) differs from `ListAiToolsParams` (db layer))
- Modify: `src/api.rs` (add `.route("/api/ai/skills", get(ai_skills))` +
  handler function)
- Modify: `src/cli/dispatch_sessions.rs` (replace the `run_ai_skills` stub from
  Task 8 with the real implementation)
- Modify: `src/cli/output/logs.rs` (add `print_skill_events_response`,
  mirroring `print_ai_tools_response` at line 322)
- Test: `src/mcp/tools_tests.rs`, `src/api_tests.rs`, plus dispatch-level CLI
  test alongside Task 8's tests

**Interfaces:**
- Consumes: `db::{AiSkillEventParams, AiSkillEventEntry, ListSkillEventsResult,
  list_skill_events}` (Task 4)
- Produces: MCP action `skill_events` (scope `cortex:read`); REST `GET
  /api/ai/skills`; CLI `cortex sessions skills [--skill NAME] [--plugin
  PLUGIN] [--tool TOOL] [--project PROJECT] [--session-id ID] [--host HOST]
  [--since ...] [--until ...] [--limit N] [--json]`

- [ ] **Step 1: Write the failing test**

  In `src/mcp/tools_tests.rs` (or wherever action-dispatch tests for a similar
  simple read action live — check the existing `list_ai_tools`/`hosts` test
  for the exact harness pattern, e.g. an in-memory `AppState` builder), add:
  ```rust
  #[tokio::test]
  async fn skill_events_action_returns_inserted_rows() {
      let (state, pool) = test_app_state(); // reuse whatever helper existing
                                             // MCP action tests use to build
                                             // an AppState over a temp DB —
                                             // match the exact helper name
                                             // used by a neighboring test such
                                             // as one exercising ListHosts or
                                             // ListApps in this same file.
      // seed one logs row + one ai_skill_events row directly via pool, same
      // shape as src/db/skill_events_tests.rs's insert_log_row helper.
      let conn = pool.get().unwrap();
      conn.execute(
          "INSERT INTO logs (timestamp, hostname, severity, message, raw, source_ip)
           VALUES ('2026-06-01T00:00:00.000Z', 'dookie', 'info', 'm', 'm', 'transcript://claude_project')",
          [],
      )
      .unwrap();
      let log_id = conn.last_insert_rowid();
      conn.execute(
          "INSERT INTO ai_skill_events (log_id, ai_tool, ai_project, ai_session_id, hostname, timestamp, skill_name, event_kind, evidence_kind)
           VALUES (?1, 'claude', 'cortex', 'sess-1', 'dookie', '2026-06-01T00:00:00.000Z', 'cortex-troubleshoot', 'claude_attribution', 'structured_json_field')",
          rusqlite::params![log_id],
      )
      .unwrap();
      drop(conn);

      let args = serde_json::json!({"action": "skill_events", "project": "cortex"});
      let response = crate::mcp::tools::execute_tool(&state, "cortex", args, None)
          .await
          .unwrap();
      assert_eq!(response["total"], 1);
      assert_eq!(response["events"][0]["skill_name"], "cortex-troubleshoot");
  }
  ```
  Before writing this, run `grep -n "async fn test_app_state\|fn build_app_state"
  src/mcp/tools_tests.rs src/mcp_tests.rs` to find the actual test-harness
  helper name/signature in this repo and adapt the test to call it correctly
  (do not invent a helper that doesn't exist — reuse the established one).

- [ ] **Step 2: Run test to verify it fails**
  ```bash
  cargo test --lib mcp::tools::tests::skill_events_action_returns_inserted_rows
  ```
  Expected: compile error or "unknown cortex action: skill_events" runtime
  error — action not registered yet.

- [ ] **Step 3: Write minimal implementation**

  In `src/mcp/actions.rs`:
  - Add `SkillEvents,` to the `ActionHandler` enum (alongside `Graph,` near the
    end, before `Help,`).
  - Add a new flags const near `TOPIC_CORRELATE_FLAGS` import area, or reuse
    `&[]` for a first cut (flags metadata is optional per the `action_spec!`
    short form). Add the action row right before the `help` entry (after
    `"graph"`):
    ```rust
    action_spec!(
        "skill_events",
        Read,
        "List extracted AI skill-invocation events",
        Cheap,
        SkillEvents,
        flags: &[],
        examples: &[
            "cortex sessions skills --project cortex --limit 20",
            "cortex sessions skills --skill cortex-troubleshoot --since 1h",
        ]
    ),
    ```

  In `src/mcp/tools.rs`:
  - Add dispatch arm: `H::SkillEvents => tool_skill_events(state, args).await,`
    (near `H::Graph => ...`).
  - Add handler function (near `tool_list_hosts`):
    ```rust
    async fn tool_skill_events(state: &AppState, args: Value) -> anyhow::Result<Value> {
        let req: cortex::app::ListSkillEventsRequest = action_payload(args, "skill_events")?;
        let response = state.service.list_skill_events(req).await?;
        Ok(serde_json::to_value(response)?)
    }
    ```
    Check `action_payload`'s exact signature/import in this file (it's already
    used by every other handler, e.g. `tool_list_apps`) and match it.

  Create `src/app/models/skill_events.rs`:
  ```rust
  use serde::{Deserialize, Serialize};

  use crate::db;

  #[derive(Debug, Clone, Default, Deserialize)]
  pub struct ListSkillEventsRequest {
      pub skill: Option<String>,
      pub plugin: Option<String>,
      pub tool: Option<String>,
      pub project: Option<String>,
      pub session_id: Option<String>,
      pub hostname: Option<String>,
      pub from: Option<String>,
      pub to: Option<String>,
      pub limit: Option<u32>,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct SkillEventEntry {
      pub id: i64,
      pub log_id: i64,
      pub ai_tool: String,
      pub ai_project: Option<String>,
      pub ai_session_id: Option<String>,
      pub hostname: String,
      pub timestamp: String,
      pub skill_name: String,
      pub skill_plugin: Option<String>,
      pub skill_path: Option<String>,
      pub event_kind: String,
      pub evidence_kind: String,
      pub metadata_json: Option<String>,
  }

  impl From<db::AiSkillEventEntry> for SkillEventEntry {
      fn from(value: db::AiSkillEventEntry) -> Self {
          Self {
              id: value.id,
              log_id: value.log_id,
              ai_tool: value.ai_tool,
              ai_project: value.ai_project,
              ai_session_id: value.ai_session_id,
              hostname: value.hostname,
              timestamp: value.timestamp,
              skill_name: value.skill_name,
              skill_plugin: value.skill_plugin,
              skill_path: value.skill_path,
              event_kind: value.event_kind,
              evidence_kind: value.evidence_kind,
              metadata_json: value.metadata_json,
          }
      }
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ListSkillEventsResponse {
      pub total: usize,
      pub truncated: bool,
      pub events: Vec<SkillEventEntry>,
  }

  impl From<db::ListSkillEventsResult> for ListSkillEventsResponse {
      fn from(value: db::ListSkillEventsResult) -> Self {
          Self {
              total: value.total,
              truncated: value.truncated,
              events: value.events.into_iter().map(Into::into).collect(),
          }
      }
  }
  ```
  Register it in `src/app/models.rs` (`mod skill_events;` +
  `pub use skill_events::{ListSkillEventsRequest, ListSkillEventsResponse,
  SkillEventEntry};`).

  Add the service method — put it in `src/app/services/skill_backfill.rs`
  right after `backfill_skill_events`, or a new `src/app/services/skill_events.rs`
  if the module is getting large; either is fine, pick whichever keeps
  `skill_backfill.rs` under roughly 200 lines. If splitting, remember to add
  `mod skill_events;` to `src/app/services.rs`.
  ```rust
  impl CortexService {
      pub async fn list_skill_events(
          &self,
          req: ListSkillEventsRequest,
      ) -> ServiceResult<ListSkillEventsResponse> {
          let from = parse_optional_timestamp(req.from.as_deref(), "from")?
              .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string());
          let to = parse_optional_timestamp(req.to.as_deref(), "to")?
              .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string());
          let params = db::AiSkillEventParams {
              skill: req.skill,
              plugin: req.plugin,
              tool: req.tool,
              project: req.project,
              session_id: req.session_id,
              hostname: req.hostname,
              from,
              to,
              limit: req.limit,
          };
          let result = self
              .run_db("list_skill_events", move |pool| db::list_skill_events(pool, &params))
              .await?;
          Ok(result.into())
      }
  }
  ```

  In `src/api.rs`:
  - Add route: `.route("/api/ai/skills", get(ai_skills))` (near the other
    `/api/sessions/*` routes for locality).
  - Add handler:
    ```rust
    async fn ai_skills(
        State(state): State<ApiState>,
        Query(req): Query<ListSkillEventsRequest>,
    ) -> impl IntoResponse {
        respond(state.service.list_skill_events(req).await)
    }
    ```
    Add `ListSkillEventsRequest` to the big `use super::models::{...}` import
    block at the top of `src/api.rs`.

  In `src/cli/dispatch_sessions.rs`, replace the Task-8 stub `run_ai_skills`
  with:
  ```rust
  pub(crate) async fn run_ai_skills(mode: &CliMode, args: SessionsSkillsListArgs) -> Result<()> {
      let json = args.json;
      let req = cortex::app::ListSkillEventsRequest {
          skill: args.skill,
          plugin: args.plugin,
          tool: args.tool,
          project: args.project,
          session_id: args.session_id,
          hostname: args.host,
          from: args.since,
          to: args.until,
          limit: args.limit,
      };
      let response = match mode {
          CliMode::Local(service) => service.list_skill_events(req).await?,
          CliMode::Http(client) => http_or_cancel(client.ai_skills(&req)).await?,
      };
      super::output::logs::print_skill_events_response(&response, json)
  }
  ```
  If `HttpClient` has no `ai_skills` method yet, either add a minimal one
  mirroring `client.ai_tools(&req)` (a GET to `/api/ai/skills` with query-string
  serialization via the same `serde_qs` mechanism already used for other
  list endpoints — check `src/cli/http_client.rs`'s `ai_tools` implementation
  and copy its shape), or fall back to `CliMode::Http(_) => bail!("sessions
  skills over --http is not yet supported")` if adding full HTTP parity is out
  of scope for this task — prefer adding the real HTTP client method since the
  pattern is mechanical and this repo's convention is full CLI/MCP/REST parity
  per `CLAUDE.md`.

  In `src/cli/output/logs.rs`, add near `print_ai_tools_response`:
  ```rust
  pub(crate) fn print_skill_events_response(
      response: &cortex::app::ListSkillEventsResponse,
      json: bool,
  ) -> Result<()> {
      if json {
          println!("{}", serde_json::to_string_pretty(response)?);
          return Ok(());
      }
      if response.events.is_empty() {
          println!("No skill events found.");
          return Ok(());
      }
      for event in &response.events {
          println!(
              "{}  {}  {}{}  tool={} project={}",
              event.timestamp,
              event.skill_name,
              event.skill_plugin.as_deref().map(|p| format!("plugin={p} ")).unwrap_or_default(),
              event.event_kind,
              event.ai_tool,
              event.ai_project.as_deref().unwrap_or("-"),
          );
      }
      if response.truncated {
          println!("(truncated — refine filters or raise --limit)");
      }
      Ok(())
  }
  ```
  Match this function's exact formatting style to whatever
  `print_ai_tools_response` actually does (read it first) rather than
  inventing new conventions — the snippet above is a reasonable placeholder
  shape to adapt.

- [ ] **Step 4: Run test to verify it passes**
  ```bash
  cargo test --lib mcp::tools::tests::skill_events_action_returns_inserted_rows
  cargo test --lib api
  cargo build --bin cortex
  cargo clippy --all-targets -- -D warnings
  ```
  Expected: MCP action test passes; `cargo build`/`clippy` clean across the
  whole workspace (this is the task where every surface — MCP, REST, CLI —
  must compile together).

- [ ] **Step 5: Commit**
  ```bash
  git add src/mcp/actions.rs src/mcp/tools.rs src/app/models/skill_events.rs src/app/models.rs src/app/services.rs src/app/services/skill_backfill.rs src/api.rs src/cli/dispatch_sessions.rs src/cli/output/logs.rs
  git commit -m "feat(mcp,api,cli): add skill_events read action across MCP, REST, and CLI surfaces"
  ```

---

### Task 10: Full-surface integration test (ingest -> backfill -> list, end-to-end idempotency)

**Files:**
- Test: `src/scanner_tests.rs` (or a new `tests/skill_events_integration.rs`
  black-box test if the repo has a `tests/` integration-test convention beyond
  fixtures — check `find tests -maxdepth 1 -name "*.rs"` first; if
  `tests/*.rs` integration tests already exist for similar cross-module flows,
  add there instead of `scanner_tests.rs` to avoid growing that file further)

**Interfaces:**
- Consumes: everything from Tasks 1-9
- Produces: no new public API; proves the full pipeline is coherent end-to-end.

- [ ] **Step 1: Write the failing test**

  ```bash
  find tests -maxdepth 1 -name "*.rs"
  ```
  If an integration-test binary convention exists (e.g. `tests/smoke.rs`),
  add there with `use cortex::...` crate-external imports. Otherwise append to
  `src/scanner_tests.rs`:
  ```rust
  #[test]
  fn end_to_end_ingest_then_backfill_is_idempotent_across_both_paths() {
      let (pool, dir) = test_pool();

      // Row 1: indexed normally (picks up the skill event via flush_chunk).
      let claude_file = dir.path().join("claude.jsonl");
      std::fs::write(
          &claude_file,
          concat!(
              r#"{"sessionId":"sess-1","attributionSkill":"cortex-troubleshoot","attributionPlugin":"cortex","content":"hi"}"#,
              "\n"
          ),
      )
      .unwrap();
      index_file(&pool, &claude_file, "explicit_file").unwrap();

      // Row 2: inserted directly into `logs` bypassing the scanner entirely,
      // simulating data ingested BEFORE this phase shipped — this is exactly
      // what `sessions skills backfill` exists to catch up.
      {
          let conn = pool.get().unwrap();
          conn.execute(
              "INSERT INTO logs (timestamp, hostname, severity, message, raw, source_ip, ai_tool, ai_project, ai_session_id)
               VALUES ('2026-06-01T00:00:00.000Z', 'dookie', 'info', ?1, ?1, 'transcript://claude_project', 'claude', 'cortex', 'sess-2')",
              rusqlite::params![r#"{"attributionSkill":"web-app-testing"}"#],
          )
          .unwrap();
      }

      let conn = pool.get().unwrap();
      let pre_backfill_count: i64 = conn
          .query_row("SELECT COUNT(*) FROM ai_skill_events", [], |row| row.get(0))
          .unwrap();
      assert_eq!(pre_backfill_count, 1, "only the scanner-ingested row should have a skill event pre-backfill");
      drop(conn);

      // Backfill catches the pre-existing row 2 without duplicating row 1's event.
      let result = crate::app::services::skill_backfill::run_backfill(&pool, None, 100, false).unwrap();
      assert_eq!(result.scanned, 2, "both claude rows should be scanned");
      assert_eq!(result.inserted, 1, "only row 2's event is new");
      assert_eq!(result.skipped_duplicates, 1, "row 1's event was already present from ingest-time extraction");

      let conn = pool.get().unwrap();
      let total: i64 = conn
          .query_row("SELECT COUNT(*) FROM ai_skill_events", [], |row| row.get(0))
          .unwrap();
      assert_eq!(total, 2);

      // Running backfill again is fully idempotent.
      let second = crate::app::services::skill_backfill::run_backfill(&pool, None, 100, false).unwrap();
      assert_eq!(second.inserted, 0);
      assert_eq!(second.skipped_duplicates, 2);
  }
  ```
  This calls `run_backfill` directly (crate-internal `pub(crate)` or private
  function) rather than through `CortexService`, so it requires
  `run_backfill` in `src/app/services/skill_backfill.rs` to be at least
  `pub(crate)` — adjust its visibility from Task 7's `fn run_backfill(...)`
  (private) to `pub(crate) fn run_backfill(...)` so this cross-module test can
  reach it. If the repo's existing convention instead drives everything
  through `CortexService` in integration tests (check whether
  `src/scanner_tests.rs` has ever called into `src/app/` before — it currently
  has not, based on its `use` block), prefer building a
  `CortexService::new(Arc::new(pool.clone()), StorageConfig::for_test(...))`
  and calling `.backfill_skill_events(...).await` inside a
  `#[tokio::test]` instead, matching Task 7's own test style, and drop the
  `pub(crate)` visibility change.

- [ ] **Step 2: Run test to verify it fails**
  ```bash
  cargo test --lib scanner::tests::end_to_end_ingest_then_backfill_is_idempotent_across_both_paths
  ```
  Expected: compile error (visibility or missing import) until the adjustments
  above are made, then a logical failure if wiring from earlier tasks has any
  gap — this test is the final correctness gate for the whole phase.

- [ ] **Step 3: Write minimal implementation**

  No new production code should be required if Tasks 1-9 were implemented
  correctly — this step is limited to:
  - Adjusting `run_backfill`'s visibility in
    `src/app/services/skill_backfill.rs` to `pub(crate)` (only if the direct
    call style above is used instead of the `CortexService` style).
  - Fixing any integration gap this test surfaces (e.g. a missed `mod`
    declaration, an import path typo from an earlier task's write-up).

- [ ] **Step 4: Run test to verify it passes**
  ```bash
  cargo test --lib scanner::tests::end_to_end_ingest_then_backfill_is_idempotent_across_both_paths
  cargo test --lib
  cargo clippy --all-targets -- -D warnings
  cargo fmt -- --check
  ```
  Expected: the new test passes, the FULL `cargo test --lib` suite passes (no
  regressions across the whole phase), clippy is clean, and `cargo fmt --check`
  reports no diffs.

- [ ] **Step 5: Commit**
  ```bash
  git add src/scanner_tests.rs src/app/services/skill_backfill.rs
  git commit -m "test: add end-to-end ingest+backfill idempotency coverage for ai_skill_events"
  ```

---

### Task 11: Documentation — action count, action table, CLI reference, schema docs

**Files:**
- Modify: `CLAUDE.md` (repo-root, at
  `/home/jmagar/workspace/cortex/.claude/worktrees/happy-kepler-2d8fa5/CLAUDE.md`)
  — MCP Tools section: `47 actions` -> `48 actions`, add `skill_events` row to
  the action table (alphabetically/logically placed after `graph`, before the
  admin actions section, matching `src/mcp/actions.rs` ordering)
- Modify: `README.md` — search for any "47 action" or MCP action count/table
  mentions (`grep -n "47 action\|action.*count" README.md`) and update
  in lockstep
- Modify: `docs/mcp/TOOLS.md` — add `skill_events` to the action reference
- Modify: `docs/mcp/SCHEMA.md` — document the `ai_skill_events` table schema
  (columns, indexes, `UNIQUE` constraint, and the two closed enums
  `event_kind`/`evidence_kind` values)
- Modify: `docs/contracts/mcp-actions-current.md` — add `skill_events` to
  whatever generated/maintained contract listing exists there (check its
  format first — it may be a machine-generated snapshot; if so, check whether
  it has a regeneration script rather than hand-editing)
- Modify: `docs/CLI.md` — add `cortex sessions skills` and `cortex sessions
  skills backfill` to the command reference, in the `sessions` section
- Modify: `docs/api.md` (if it exists — `find docs -iname "api.md"`) — add
  `GET /api/ai/skills` to the REST route table

**Interfaces:**
- Consumes: nothing (pure documentation)
- Produces: nothing (no code)

- [ ] **Step 1: Write the failing test**

  Documentation changes are covered by this repo's existing doc-consistency
  test if one exists — check first:
  ```bash
  grep -rn "47" src/docs_tests.rs 2>&1
  cat src/docs_tests.rs | head -60
  ```
  If `src/docs_tests.rs` asserts an action count or cross-checks
  `ACTION_SPECS` against `CLAUDE.md`/`docs/contracts/mcp-actions-current.md`
  programmatically, that test is the "failing test" for this task — it should
  already be red after Task 9 added the `skill_events` action (48 actions vs.
  a docs file still claiming 47). Confirm:
  ```bash
  cargo test --lib docs_tests
  ```
  Expected: if such a test exists, it now fails (action count mismatch,
  missing doc row, etc.) — that failure IS this task's Step 1/Step 2. If no
  such automated doc-consistency test exists in this repo, skip Steps 1-2 and
  proceed directly to Step 3 (manual doc edits), since there is nothing to
  automatically assert; note this explicitly in the commit message.

- [ ] **Step 2: Run test to verify it fails**
  ```bash
  cargo test --lib docs_tests
  ```
  Expected (if the test exists): failure citing action count 47 vs actual 48,
  or a missing `skill_events` entry. If no such test exists, this step is N/A.

- [ ] **Step 3: Write minimal implementation**

  In `CLAUDE.md` (repo root), find:
  ```
  One MCP tool: **`cortex`** — dispatches by `action` argument. 47 actions, generated from `ACTION_SPECS` in `src/mcp/actions.rs` (the single authoritative registry — regenerate this table from there).
  ```
  Change `47 actions` to `48 actions`. In the action table, add a new row
  after the `graph` row and before the `file_tails` (admin) row:
  ```
  | `skill_events` | List extracted AI skill-invocation events |
  ```

  In `README.md`, run `grep -n "47" README.md` and update any matching action
  count reference the same way.

  In `docs/mcp/TOOLS.md`, add a `skill_events` entry following whatever format
  the file already uses for each action (read the file first to match its
  exact per-action documentation block shape — likely name, description,
  scope, example args, example response).

  In `docs/mcp/SCHEMA.md`, add a new section documenting `ai_skill_events`:
  ```markdown
  ### `ai_skill_events`

  One row per detected skill invocation extracted from an AI transcript log
  row. Added in migration 38.

  | Column | Type | Notes |
  |---|---|---|
  | `id` | INTEGER PK | autoincrement |
  | `log_id` | INTEGER NOT NULL | FK -> `logs(id)` ON DELETE CASCADE |
  | `ai_tool` | TEXT NOT NULL | `claude` \| `codex` |
  | `ai_project` | TEXT | nullable — copied from the source `logs` row |
  | `ai_session_id` | TEXT | nullable — copied from the source `logs` row |
  | `hostname` | TEXT NOT NULL | copied from the source `logs` row |
  | `timestamp` | TEXT NOT NULL | copied from the source `logs` row |
  | `skill_name` | TEXT NOT NULL | trimmed, max 256 chars; `plugin:skill` combined form when the source used that shape |
  | `skill_plugin` | TEXT | nullable, trimmed, max 256 chars |
  | `skill_path` | TEXT | nullable; reserved, currently always NULL from both extractors |
  | `event_kind` | TEXT NOT NULL | `claude_attribution` \| `codex_skill_block` |
  | `evidence_kind` | TEXT NOT NULL | `structured_json_field` \| `transcript_content` |
  | `metadata_json` | TEXT | nullable; reserved for future evidence detail |
  | `created_at` | TEXT NOT NULL | insert time |

  `UNIQUE(log_id, skill_name, event_kind, evidence_kind)` makes ingest-time and
  backfill insertion idempotent via `INSERT OR IGNORE`.

  Indexes: `idx_ai_skill_events_skill_time (skill_name, timestamp)`,
  `idx_ai_skill_events_session_time (ai_tool, ai_project, ai_session_id,
  timestamp)`, `idx_ai_skill_events_project_skill_time (ai_project, skill_name,
  timestamp) WHERE ai_project IS NOT NULL`.
  ```

  In `docs/contracts/mcp-actions-current.md`, check for a generation script
  reference at the top of the file (`grep -n "generated\|regenerate" docs/contracts/mcp-actions-current.md`).
  If it says "generated — do not hand-edit", find and run the generator
  script instead of hand-editing:
  ```bash
  grep -rn "mcp-actions-current" --include="*.rs" --include="*.sh" src/ scripts/ xtask/ 2>&1
  ```
  Run whatever script/binary that turns up (likely `cargo run --bin
  <something>` or a `scripts/*.sh`) to regenerate the file. If no generator
  exists, hand-edit it to add the `skill_events` row matching its existing
  format exactly.

  In `docs/CLI.md`, find the `sessions` command reference section and add:
  ```markdown
  | `cortex sessions skills [--skill NAME] [--plugin PLUGIN] [--tool TOOL] [--project PROJECT] [--session-id ID] [--host HOST] [--since ...] [--until ...] [--limit N] [--json]` | List extracted AI skill-invocation events |
  | `cortex sessions skills backfill [--since ...] [--limit N] [--dry-run] [--json]` | Backfill `ai_skill_events` from existing `logs` rows |
  ```
  Match the exact table/list format already used in that section (read it
  first).

  If `docs/api.md` exists, add `GET /api/ai/skills` with query params matching
  `ListSkillEventsRequest` fields, in whatever format the file's existing
  route entries use.

- [ ] **Step 4: Run test to verify it passes**
  ```bash
  cargo test --lib docs_tests
  cargo test --lib
  ```
  Expected: doc-consistency test (if it exists) passes; full test suite still
  green.

- [ ] **Step 5: Commit**
  ```bash
  git add CLAUDE.md README.md docs/mcp/TOOLS.md docs/mcp/SCHEMA.md docs/contracts/mcp-actions-current.md docs/CLI.md docs/api.md
  git commit -m "docs: document skill_events action, ai_skill_events schema, and sessions skills CLI"
  ```

---

## Post-phase verification checklist (run once, after Task 11)

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo xtask check-version-sync
```

Per this repo's `CLAUDE.md` versioning rule, this phase's commits should be
squashed/tagged with a `feat` (minor) or `feat!`/`fix` prefix at PR time so
`cargo xtask bump-version minor` (new action, new table, new CLI surface —
qualifies as a feature addition, not a breaking change) runs before merge, and
`CHANGELOG.md` gets an entry. That version bump is intentionally NOT included
as a task above since it is normally done once at the end of the whole
feature branch, per repo convention, not per phase-task.

## Self-Review

### Spec coverage

| Spec item | Covered by |
|---|---|
| Claude `attributionSkill`/`attributionPlugin` extraction (top-level, `message.*`, `payload.*` nesting) | Task 2 |
| Codex `<skill><name>` extraction, multi-tag dedup, prose-not-matched | Task 3 |
| Shared normalization (trim/clamp/`plugin:skill` split, never fabricated across separate Claude fields) | Task 2 (base), Task 3 (extends) |
| Migration for `ai_skill_events` table + indexes + `KNOWN_SCHEMA_VERSION` bump | Task 1 |
| DB insert/list query layer (`insert_skill_events[_in_tx]`, `list_skill_events`) | Task 4 |
| `insert_logs_batch_in_tx` returns log ids (prerequisite plumbing for ingest-time insertion) | Task 5 |
| Ingest-time insertion, same transaction as the `logs` row, no second full-table scan | Task 6 |
| Backfill: bounded/chunked, lock released between chunks, idempotent, dry-run | Task 7 |
| `cortex sessions skills backfill` CLI command | Task 8 |
| `skill_events` read surface — MCP action, CLI list (`cortex sessions skills`), REST (`GET /api/ai/skills`) | Task 9 |
| End-to-end ingest-then-backfill idempotency across both insertion paths | Task 10 |
| Docs: action count/table, CLI reference, schema docs, contracts snapshot | Task 11 |

No gaps were found requiring a new task — every locked interface declared at the top of this plan (`ExtractedSkillEvent`, `SkillEventKind`, `SkillEvidenceKind`, `SkillEventInsert`, `AiSkillEventParams`, `AiSkillEventEntry`, `ListSkillEventsResult`, `SkillBackfillRequest`, `SkillBackfillResult`) is produced by exactly one task and consumed by name in every later task that needs it.

### Placeholder scan

Task 2's Step 3 includes a deliberate `unreachable!()` stub shown as a "checkpoint" before the real implementation — this is intentional scaffolding called out explicitly in the plan text ("That stub is wrong on purpose as a checkpoint — replace the final function with the real implementation before running tests") and is immediately superseded by the real `extract_claude_skill_events` body in the same step. No other `unreachable!()`, `todo!()`, `unimplemented!()`, or silent no-op stub remains outside that single documented exception. Task 8 leaves `run_ai_skills` (the read/list dispatch arm) as an explicit `bail!("not yet implemented")` stub, but this is closed out within the same plan at Task 9 Step 3, not left dangling at the end of the phase.

### Type consistency

Verified by direct text search across the whole document:
- `ExtractedSkillEvent` — same five fields (`skill_name`, `skill_plugin`, `skill_path`, `event_kind`, `evidence_kind`, `metadata_json`) in the Locked Interfaces block, Task 2's struct definition, Task 3's usage, and every constructor call site in Tasks 4, 6, and 7.
- `AiSkillEventParams` — identical nine-field shape (`skill`, `plugin`, `tool`, `project`, `session_id`, `hostname`, `from`, `to`, `limit`) in the Locked Interfaces block and Task 4's concrete implementation; Task 9's `ListSkillEventsRequest` (app-layer) maps 1:1 onto it field-for-field in `CortexService::list_skill_events`.
- `AiSkillEventEntry` — identical thirteen-field shape in the Locked Interfaces block, Task 4's struct, and Task 9's `From<db::AiSkillEventEntry> for SkillEventEntry` conversion (every field is threaded through, none dropped or renamed).
- `SkillEventInsert` — same six fields (`log_id`, `ai_tool`, `ai_project`, `ai_session_id`, `hostname`, `timestamp`, `event`) used consistently by Task 4's insert function, Task 6's ingest wiring, and Task 7's backfill service.
- `SkillBackfillRequest`/`SkillBackfillResult` — same shapes in the Locked Interfaces block, Task 7's model definitions, and Task 8's CLI arg mapping into the request.

No divergent field names or types were found between the interface declarations and their downstream usages.
