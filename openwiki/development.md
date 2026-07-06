# Development Guide

Building, testing, and contributing to cortex.

## Quick Start

```bash
# Clone repository
git clone <repo>
cd cortex

# Install git LFS (for large files in bin/)
git lfs install

# Build debug
cargo build

# Build release
cargo build --release

# Run locally
cargo run

# Run tests
cargo test

# Lint
cargo clippy

# Format
cargo fmt
```

## Commands

### Justfile Commands
The `Justfile` provides convenient aliases:

```bash
just dev              # cargo run
just test             # cargo test
just health           # curl /health | jq
just gen-token        # openssl rand -hex 32
just build-plugin     # release build → bin/
just publish <type>   # bump version, tag, push
```

### CLI Commands
```bash
cortex search "error" --since 1h
cortex tail -n 50
cortex stats
cortex compose status
cortex doctor
```

## Module Organization

### Sidecar Tests
Most modules have sidecar test files:

```
src/db/pool.rs              ← Implementation
src/db/pool_tests.rs        ← Unit tests
```

Benefits:
- Private item access via `use super::*`
- Clear test-to-source proximity
- Parallel test compilation

**Pattern**:
```rust
// src/db/pool.rs
#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;
```

### Module Boundaries

| Module | Purpose | Public API |
|--------|---------|------------|
| `config.rs` | Config loading | `Config::load()` |
| `runtime.rs` | Composition root | `RuntimeCore::load()`, `load_query_only()` |
| `app/` | Service layer | `CortexService` (all business logic) |
| `db/` | Database queries | `DbPool`, insert/list helpers |
| `mcp/` | MCP server | `mcp::AppState`, tool handlers |
| `api.rs` | REST API | Axum router |
| `cli/` | CLI commands | Command parsers, HTTP client |

### Testing Support
`src/lib.rs::testing` module provides factory helpers:

```rust
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod testing {
    pub fn loopback_state(data_dir: &Path) -> AppState;
    pub fn bearer_state(data_dir: &Path, token: &str) -> AppState;
    pub fn oauth_state(data_dir: &Path) -> AppState;
    // Re-export db internals for integration tests
    pub use crate::db::{DbPool, LogBatchEntry, init_pool, insert_logs_batch};
}
```

Use in integration tests:
```rust
use cortex::testing::loopback_state;
let temp = tempfile::TempDir::new().unwrap();
let state = loopback_state(temp.path());
```

## Adding Features

### Adding a New MCP Action

1. **Define action spec** in `src/mcp/actions.rs`:
```rust
ACTION_SPECS.push(ActionSpec {
    name: "my_action",
    scope: Scope::Read,
    cost: Cost::Cheap,
    description: "My new action",
    params: vec![...],
});
```

2. **Add request/response models** in `src/app/models/`:
```rust
#[derive(Serialize, Deserialize)]
pub struct MyActionRequest {
    pub param: String,
}

#[derive(Serialize, Deserialize)]
pub struct MyActionResponse {
    pub result: String,
}
```

3. **Implement service method** in `src/app/services.rs`:
```rust
impl CortexService {
    pub async fn my_action(&self, req: MyActionRequest) -> Result<MyActionResponse> {
        // Business logic here
    }
}
```

4. **Add MCP handler** in `src/mcp/tools.rs`:
```rust
async fn tool_my_action(state: &AppState, args: Value) -> Result<Value> {
    let req: MyActionRequest = serde_json::from_value(args)?;
    let service = state.service.read().await;
    let response = service.my_action(req).await?;
    serde_json::to_value(response)
}
```

5. **Add REST route** in `src/api.rs`:
```rust
router = router.route(
    "/api/my-action",
    post(api_my_action).with_state(app_state.clone()),
);

async fn api_my_action(
    State(app_state): State<Arc<AppState>>,
    Json(req): Json<MyActionRequest>,
) -> Result<Json<MyActionResponse>, ApiError> {
    let service = app_state.service.read().await;
    let response = service.my_action(req).await?;
    Ok(Json(response))
}
```

6. **Add CLI command** in `src/cli/commands/` and `src/cli/parse/`:
```rust
// src/cli/commands/analytics.rs
#[derive(Parser, Debug)]
pub struct MyActionCmd {
    pub param: String,
}

// src/cli/parse/my_action.rs
pub fn parse_my_action(output: &str) -> MyActionResponse {
    serde_json::from_str(output).unwrap()
}
```

7. **Add tests** in sidecar files:
```rust
// src/app/service_tests.rs
#[tokio::test]
async fn test_my_action() {
    let service = test_service().await;
    let req = MyActionRequest { param: "test".into() };
    let response = service.my_action(req).await.unwrap();
    assert_eq!(response.result, "expected");
}
```

### Adding a New Incident Type

1. **Create event table** (migration in `src/db/pool.rs`):
```sql
CREATE TABLE ai_my_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_name TEXT NOT NULL,
    ai_tool TEXT NOT NULL,
    ai_project TEXT,
    ai_session_id TEXT,
    hostname TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    raw_value TEXT,
    UNIQUE(event_name, ai_tool, ai_project, COALESCE(ai_session_id, ''), hostname, timestamp)
);
```

2. **Add event extraction** in `src/scanner/my_events.rs`:
```rust
pub fn extract_my_events(session: &AiSession) -> Vec<ExtractedMyEvent> {
    // Parse transcripts for my events
}
```

3. **Add signal detectors** in `src/app/my_signal_detectors.rs`:
```rust
pub fn detect_my_signals(
    session: &AiSession,
    my_events: &[MyEventEntry],
) -> Vec<SignalAnchor> {
    // Detect negative patterns after my events
}
```

4. **Add incident grouping** in `src/db/my_incidents.rs`:
```rust
pub fn group_my_incidents(
    pool: &DbPool,
    params: &MyIncidentParams,
) -> Result<Vec<MyIncident>> {
    // Group signals by key, score, prioritize
}
```

5. **Add evidence bundles** in `src/db/my_incident_evidence.rs`:
```rust
pub fn build_my_incident_evidence(
    pool: &DbPool,
    incident_id: i64,
) -> Result<MyIncidentEvidence> {
    // Collect events, sessions, signals, nearby logs
}
```

6. **Add deterministic findings** in `src/app/my_incident_findings.rs`:
```rust
pub fn derive_my_findings(
    evidence: &MyIncidentEvidence,
) -> Vec<MyFinding> {
    // Rule-based classification
}
```

7. **Wire through service layer** in `src/app/services.rs`:
```rust
impl CortexService {
    pub async fn my_events(&self, req: MyEventsRequest) -> Result<MyEventsResponse> { }
    pub async fn my_incidents(&self, req: MyIncidentsRequest) -> Result<MyIncidentsResponse> { }
    pub async fn my_investigate(&self, req: MyInvestigateRequest) -> Result<MyInvestigateResponse> { }
}
```

## Test Coverage

### Target: ~80%
See `scripts/coverage.sh` for coverage report generation.

### Running Tests
```bash
# Unit tests (fast)
cargo test --lib

# Integration tests (slower, requires --features test-support)
cargo test --test '*' --features test-support

# Specific test
cargo test test_search_logs
```

### Test Patterns
- **Unit tests**: Private item access via sidecar modules
- **Integration tests**: Use `testing` factory helpers
- **Property tests**: Use `proptest` for randomized testing (where applicable)

## CI/CD

### GitHub Actions
- `.github/workflows/ci.yml`: Main CI pipeline
- Checks: `cargo test`, `cargo clippy`, `cargo fmt --check`
- Release: Triggers on version tags

### Release Gates
See **[docs/RELEASE.md](../docs/RELEASE.md)** and **[docs/CHECKLIST.md](../docs/CHECKLIST.md)**.

## Code Style

### Rust Patterns
- **Error handling**: `anyhow` for CLI/runtime, `thiserror` for typed errors
- **Async**: `tokio` runtime, `await` everywhere
- **Logging**: `tracing` with structured fields
- **Testing**: Sidecar `*_tests.rs` modules

### Conventions
- Module order: `pub mod` first, then `pub(crate) mod`, then private
- Test hooks: `#[cfg(test)] #[path = "..."] mod tests;`
- Factory helpers: In `testing` module (feature-gated)
- Documentation: Public items get `///` doc comments

## Performance

### SQLite Optimization
- WAL mode for concurrent reads/writes
- FTS5 full-text search with BM25 ranking
- Connection pooling with `r2d2`
- Prepared statements for repeated queries
- `PRAGMA optimize` runs every 6h

### Batch Processing
- Ingest channel buffers logs in memory
- Batch writes reduce lock contention
- One connection reserved for writer

### Query Limits
- Default row limits via `CORTEX_MAX_RESULTS`
- Timeout enforcement (30s default)
- Response caps prevent memory exhaustion

## Troubleshooting

### Build Issues
- **Rust version**: Ensure MSRV 1.86+
- **Git LFS**: Run `git lfs install && git lfs pull`
- **Dependencies**: Run `cargo update` if lockfile is stale

### Test Failures
- **Flaky tests**: Check for time-dependent logic (add tolerances)
- **Path issues**: Use `&Path` from `tempfile::TempDir::path()`
- **Concurrency**: Ensure proper `await` and locking

### Performance Issues
- **Slow queries**: Run `PRAGMA optimize`, check FTS5 index
- **Memory leaks**: Check for unbounded Vec growth
- **Lock contention**: Reduce batch size or increase pool size

## References

- **[docs/RUST.md](../docs/RUST.md)** – Rust toolchain and dependencies
- **[docs/repo/RULES.md](../docs/repo/RULES.md)** – Repository conventions
- **[docs/repo/SCRIPTS.md](../docs/repo/SCRIPTS.md)** – Build and test scripts
- **[.github/workflows/ci.yml](../.github/workflows/ci.yml)** – CI pipeline
