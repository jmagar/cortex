use super::*;
use crate::config::LlmConfig;
use std::sync::Arc;

fn test_pool() -> (Arc<crate::db::DbPool>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = crate::config::StorageConfig::for_test(dir.path().join("test.db"));
    let pool = crate::db::init_pool(&storage).unwrap();
    (Arc::new(pool), dir)
}

fn base_spec(action: &str) -> LlmInvocationSpec {
    LlmInvocationSpec {
        caller_surface: LlmCallerSurface::Test,
        action: action.to_string(),
        incident_id: Some("inc-1".to_string()),
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        evidence_counts: LlmEvidenceCounts::default(),
        prompt: "hello".to_string(),
        provider: "test-provider".to_string(),
        model: "test-model".to_string(),
        program: "test-program".to_string(),
        extra_metadata: serde_json::json!({}),
    }
}

// Eng review fix (Fix 1 — architecture + performance reviewers): the
// `timeout_secs()` accessor is the single source of truth Task 6 threads
// into `GeminiAssessConfig` instead of that struct independently
// re-reading `CORTEX_LLM_COMPLETION_TIMEOUT_SECS`. See "Eng Review Fixes
// Applied" at the top of the plan.
#[test]
fn timeout_secs_accessor_exposes_resolved_config_value() {
    let cfg = LlmConfig {
        timeout_secs: 45,
        ..LlmConfig::default()
    };
    let (pool, _dir) = test_pool();
    let runner = LlmRunner::new(pool, cfg);
    assert_eq!(runner.timeout_secs(), 45);
}

#[tokio::test]
async fn disabled_runner_denies_and_audits() {
    let (pool, _dir) = test_pool();
    let cfg = LlmConfig {
        enabled: false,
        ..LlmConfig::default()
    };
    let runner = LlmRunner::new(pool.clone(), cfg);

    let result = runner
        .run(base_spec("ai_assess"), |_prompt| async {
            Ok("unused".to_string())
        })
        .await;

    assert!(matches!(result, Err(LlmRunnerError::Disabled)));

    let conn = pool.get().unwrap();
    let status: String = conn
        .query_row(
            "SELECT status FROM llm_invocations WHERE action = 'ai_assess' ORDER BY started_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "disabled");
}

#[tokio::test]
async fn background_enrich_action_denied_when_background_enrichment_disabled() {
    let (pool, _dir) = test_pool();
    let cfg = LlmConfig {
        background_enrichment_enabled: false,
        ..LlmConfig::default()
    };
    let runner = LlmRunner::new(pool.clone(), cfg);

    let result = runner
        .run(base_spec("background_enrich"), |_prompt| async {
            panic!("run_fn must not be called when background enrichment is disabled")
        })
        .await;

    assert!(matches!(result, Err(LlmRunnerError::ActionDisabled(_))));
}

/// Regression test for the enforcement gap: the documented contract states
/// `background_enrichment_enabled` is "checked whenever `caller_surface ==
/// Background`" — not only when the action name happens to literally be
/// `"background_enrich"`. Construct a spec with `caller_surface: Background`
/// and a DIFFERENT action name, and confirm the call is denied even though
/// no per-action config entry exists for it.
#[tokio::test]
async fn background_caller_surface_denied_regardless_of_action_name() {
    let (pool, _dir) = test_pool();
    let cfg = LlmConfig {
        background_enrichment_enabled: false,
        ..LlmConfig::default()
    };
    let runner = LlmRunner::new(pool.clone(), cfg);

    let mut spec = base_spec("skill_assess");
    spec.caller_surface = LlmCallerSurface::Background;

    let result = runner
        .run(spec, |_prompt| async {
            panic!(
                "run_fn must not be called: background_enrichment_enabled=false must gate \
                 ANY action run with caller_surface=Background, not just \"background_enrich\""
            )
        })
        .await;

    assert!(
        matches!(result, Err(LlmRunnerError::ActionDisabled(_))),
        "expected ActionDisabled for a Background-surface call with enrichment disabled, got {result:?}"
    );

    let conn = pool.get().unwrap();
    let status: String = conn
        .query_row(
            "SELECT status FROM llm_invocations WHERE action = 'skill_assess' ORDER BY started_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "disabled");
}

/// Sanity check on the flip side: once `background_enrichment_enabled` is
/// true, a Background-surface call with a non-"background_enrich" action
/// name is allowed through (assuming no other gate denies it).
#[tokio::test]
async fn background_caller_surface_allowed_when_enrichment_enabled() {
    let (pool, _dir) = test_pool();
    let cfg = LlmConfig {
        background_enrichment_enabled: true,
        ..LlmConfig::default()
    };
    let runner = LlmRunner::new(pool.clone(), cfg);

    let mut spec = base_spec("skill_assess");
    spec.caller_surface = LlmCallerSurface::Background;

    let result = runner
        .run(spec, |_prompt| async { Ok("ok".to_string()) })
        .await;

    assert!(result.is_ok(), "expected success, got {result:?}");
}

#[tokio::test]
async fn prompt_over_limit_is_rejected_before_spawn() {
    let (pool, _dir) = test_pool();
    let cfg = LlmConfig {
        max_prompt_bytes: 4,
        ..LlmConfig::default()
    };
    let runner = LlmRunner::new(pool.clone(), cfg);

    let mut spec = base_spec("ai_assess");
    spec.prompt = "way too long".to_string();

    let result = runner
        .run(spec, |_prompt| async {
            panic!("run_fn must not be called when prompt exceeds limit")
        })
        .await;

    assert!(matches!(
        result,
        Err(LlmRunnerError::PromptTooLarge {
            actual: 12,
            limit: 4
        })
    ));
}

#[tokio::test]
async fn global_concurrency_limit_denies_second_concurrent_call() {
    let (pool, _dir) = test_pool();
    let cfg = LlmConfig {
        max_concurrent: 1,
        max_per_action_concurrent: 5, // isolate global limit from per-action limit
        ..LlmConfig::default()
    };
    let runner = Arc::new(LlmRunner::new(pool.clone(), cfg));

    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let runner_a = runner.clone();
    let handle = tokio::spawn(async move {
        runner_a
            .run(base_spec("ai_assess"), move |_prompt| async move {
                started_tx.send(()).ok();
                release_rx.await.ok();
                Ok("first".to_string())
            })
            .await
    });
    started_rx.await.unwrap();

    let second = runner
        .run(base_spec("skill_assess"), |_prompt| async {
            Ok("second".to_string())
        })
        .await;
    assert!(matches!(second, Err(LlmRunnerError::ConcurrencyLimited(1))));

    release_tx.send(()).ok();
    let first = handle.await.unwrap();
    assert!(first.is_ok());
}

#[tokio::test]
async fn rate_limit_denies_fourth_call_within_a_minute() {
    let (pool, _dir) = test_pool();
    let cfg = LlmConfig {
        max_invocations_per_minute: 3,
        max_invocations_per_hour: 1000,
        ..LlmConfig::default()
    };
    let runner = LlmRunner::new(pool.clone(), cfg);

    for _ in 0..3 {
        let result = runner
            .run(base_spec("ai_assess"), |_prompt| async {
                Ok("ok".to_string())
            })
            .await;
        assert!(result.is_ok());
    }

    let fourth = runner
        .run(base_spec("ai_assess"), |_prompt| async {
            Ok("ok".to_string())
        })
        .await;
    assert!(matches!(fourth, Err(LlmRunnerError::RateLimited { .. })));
}

#[tokio::test]
async fn circuit_opens_after_failure_threshold_and_audits_denial() {
    let (pool, _dir) = test_pool();
    let cfg = LlmConfig {
        failure_threshold: 2,
        max_invocations_per_minute: 100,
        max_invocations_per_hour: 100,
        ..LlmConfig::default()
    };
    let runner = LlmRunner::new(pool.clone(), cfg);

    for _ in 0..2 {
        let result = runner
            .run(base_spec("ai_assess"), |_prompt| async {
                Err(anyhow::anyhow!("simulated LLM failure"))
            })
            .await;
        assert!(result.is_err());
    }

    let third = runner
        .run(base_spec("ai_assess"), |_prompt| async {
            panic!("run_fn must not be called while circuit is open")
        })
        .await;
    match third {
        Err(LlmRunnerError::CircuitOpen { retry_after, .. }) => {
            // `retry_after` must be an actionable value (seconds remaining
            // until the circuit closes), not `Instant`'s opaque Debug
            // output (e.g. `Instant { tv_sec: ..., tv_nsec: ... }`).
            assert!(!retry_after.is_empty(), "retry_after must not be empty");
            assert!(
                !retry_after.contains("Instant"),
                "retry_after must not leak Instant Debug output: {retry_after}"
            );
            let seconds_str = retry_after.strip_suffix('s').unwrap_or_else(|| {
                panic!("retry_after must be formatted as '<n>s': {retry_after}")
            });
            let seconds: u64 = seconds_str
                .parse()
                .unwrap_or_else(|_| panic!("retry_after seconds must parse as u64: {retry_after}"));
            assert!(
                seconds > 0,
                "retry_after must report a positive number of remaining seconds, got {seconds}"
            );
        }
        other => panic!("expected CircuitOpen, got {other:?}"),
    }

    let conn = pool.get().unwrap();
    let denied_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM llm_invocations WHERE action = 'ai_assess' AND status = 'circuit_open'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        denied_count, 1,
        "circuit_open denial must itself be audited"
    );
}

/// `retry_after` must round UP a sub-second remainder, never truncate it to
/// "0s" — a caller reading "0s" would reasonably assume the circuit is
/// already closed and retry immediately, when it's actually still open.
/// `circuit_opens_after_failure_threshold_and_audits_denial` only asserts
/// `seconds > 0`, which happens to hold with the default 300s cooldown but
/// does not exercise the truncation-vs-rounding boundary; this test uses a
/// 1s cooldown so the remaining time is checked well under a second after
/// the circuit opens, which `.as_secs()` (truncate) would misreport as
/// `"0s"` but `.as_secs_f64().ceil()` (round up) correctly reports as `"1s"`.
#[tokio::test]
async fn circuit_open_retry_after_rounds_up_sub_second_remainder() {
    let (pool, _dir) = test_pool();
    let cfg = LlmConfig {
        failure_threshold: 1,
        cooldown_secs: 1,
        max_invocations_per_minute: 100,
        max_invocations_per_hour: 100,
        ..LlmConfig::default()
    };
    let runner = LlmRunner::new(pool.clone(), cfg);

    let first = runner
        .run(base_spec("ai_assess"), |_prompt| async {
            Err(anyhow::anyhow!("simulated LLM failure"))
        })
        .await;
    assert!(first.is_err());

    let second = runner
        .run(base_spec("ai_assess"), |_prompt| async {
            panic!("run_fn must not be called while circuit is open")
        })
        .await;
    match second {
        Err(LlmRunnerError::CircuitOpen { retry_after, .. }) => {
            assert_ne!(
                retry_after, "0s",
                "retry_after must round up a sub-second remainder, not truncate to 0s \
                 while the circuit is still open"
            );
        }
        other => panic!("expected CircuitOpen, got {other:?}"),
    }
}

#[tokio::test]
async fn dry_run_never_invokes_llm_and_reports_sizes() {
    let (pool, _dir) = test_pool();
    let runner = LlmRunner::new(pool.clone(), LlmConfig::default());
    let spec = base_spec("ai_assess");

    let outcome = runner.dry_run(&spec).await.unwrap();
    assert_eq!(outcome.prompt_bytes, "hello".len());
    assert!(!outcome.would_exceed_prompt_limit);

    let conn = pool.get().unwrap();
    let status: String = conn
        .query_row(
            "SELECT status FROM llm_invocations WHERE id = ?1",
            [outcome.invocation_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "dry_run");
}

#[tokio::test]
async fn successful_run_writes_success_audit_row_with_timing() {
    let (pool, _dir) = test_pool();
    let runner = LlmRunner::new(pool.clone(), LlmConfig::default());

    let outcome = runner
        .run(base_spec("ai_assess"), |prompt| async move {
            assert_eq!(prompt, "hello");
            Ok("assessment markdown".to_string())
        })
        .await
        .unwrap();

    assert_eq!(outcome.output, "assessment markdown");
    assert!(outcome.duration_ms >= 0);

    let conn = pool.get().unwrap();
    let (status, finished_at, incident_id): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT status, finished_at, incident_id FROM llm_invocations WHERE id = ?1",
            [outcome.invocation_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, "success");
    assert!(finished_at.is_some());
    assert_eq!(incident_id.as_deref(), Some("inc-1"));
}

// --- Eng review fix: MP1 (security reviewer) — sanitize_error must catch
// the same secret shapes `looks_secretish` catches elsewhere in this repo,
// and must redact BEFORE truncating so a secret straddling the truncation
// boundary can't leak its surviving half. See "Eng Review Fixes Applied"
// (Fix 3) at the top of the plan.
#[tokio::test]
async fn error_with_secretish_tokens_is_redacted_before_persisting() {
    let (pool, _dir) = test_pool();
    let runner = LlmRunner::new(pool.clone(), LlmConfig::default());

    let result = runner
        .run(base_spec("ai_assess"), |_prompt| async {
            Err(anyhow::anyhow!(
                "gemini auth failed: API_KEY=super-secret-value sk-abc123secretvalue ghp_deadbeef1234 TOKEN=another-one"
            ))
        })
        .await;
    assert!(result.is_err());

    let conn = pool.get().unwrap();
    let error: String = conn
        .query_row(
            "SELECT error FROM llm_invocations WHERE action = 'ai_assess' AND status = 'error' ORDER BY started_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !error.contains("super-secret-value"),
        "API_KEY value must be redacted, got: {error}"
    );
    assert!(
        !error.contains("sk-abc123secretvalue"),
        "sk- prefixed token must be redacted, got: {error}"
    );
    assert!(
        !error.contains("ghp_deadbeef1234"),
        "ghp_ prefixed token must be redacted, got: {error}"
    );
    assert!(
        !error.contains("another-one"),
        "TOKEN value must be redacted, got: {error}"
    );
    assert!(
        error.contains("[REDACTED]"),
        "expected [REDACTED] marker in sanitized error, got: {error}"
    );
}

#[test]
fn sanitize_error_redacts_before_truncating_so_boundary_split_secrets_do_not_leak() {
    // Build an error whose secret token straddles the old 2048-char
    // truncation boundary: 2040 bytes of padding, then a secret that
    // would previously be split mid-token by `.take(2048)` BEFORE
    // redaction ran, leaking the surviving half. Redact-first must catch
    // the whole token regardless of where it falls.
    let padding = "x".repeat(2040);
    let err = anyhow::anyhow!(format!(
        "{padding} API_KEY=leaked-secret-value-should-not-appear"
    ));
    let sanitized = sanitize_error(&err);
    assert!(
        !sanitized.contains("leaked-secret-value-should-not-appear"),
        "secret must not survive truncation in either half, got: {sanitized}"
    );
}

// --- Eng review fix: MP3 (security reviewer) — extra_metadata must be
// redacted and size-capped before it lands in metadata_json, since it is
// caller-supplied and PR 2-4 must not be able to smuggle prompt/evidence
// content into an (admin-scoped, per Fix 4) but still-persisted table.
// See "Eng Review Fixes Applied" (Fix 5).
#[tokio::test]
async fn extra_metadata_with_secretish_value_is_redacted_before_persisting() {
    let (pool, _dir) = test_pool();
    let runner = LlmRunner::new(pool.clone(), LlmConfig::default());

    let mut spec = base_spec("ai_assess");
    spec.extra_metadata = serde_json::json!({
        "note": "leaked TOKEN=do-not-persist-this-value here"
    });

    let outcome = runner
        .run(spec, |_prompt| async { Ok("ok".to_string()) })
        .await
        .unwrap();

    let conn = pool.get().unwrap();
    let metadata_json: String = conn
        .query_row(
            "SELECT metadata_json FROM llm_invocations WHERE id = ?1",
            [outcome.invocation_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !metadata_json.contains("do-not-persist-this-value"),
        "secret-shaped extra_metadata value must be redacted, got: {metadata_json}"
    );
    assert!(metadata_json.contains("[REDACTED]"));
}

#[tokio::test]
async fn extra_metadata_over_byte_cap_is_truncated_not_silently_dropped() {
    let (pool, _dir) = test_pool();
    let runner = LlmRunner::new(pool.clone(), LlmConfig::default());

    let mut spec = base_spec("ai_assess");
    // Comfortably over LLM_METADATA_MAX_BYTES (4096).
    spec.extra_metadata = serde_json::json!({ "blob": "y".repeat(8192) });

    let outcome = runner
        .run(spec, |_prompt| async { Ok("ok".to_string()) })
        .await
        .unwrap();

    let conn = pool.get().unwrap();
    let metadata_json: String = conn
        .query_row(
            "SELECT metadata_json FROM llm_invocations WHERE id = ?1",
            [outcome.invocation_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        metadata_json.len() <= LLM_METADATA_MAX_BYTES + 256,
        "metadata_json must be capped near LLM_METADATA_MAX_BYTES, got {} bytes",
        metadata_json.len()
    );
    assert!(
        metadata_json.contains("truncated"),
        "oversized metadata must carry an explicit truncation marker, not be silently stored in full; got: {metadata_json}"
    );
}

// --- Eng review fix: CRITICAL (architecture + performance reviewers) —
// every audit write must go through `crate::db::write_lock()`, matching
// the standing invariant documented in `src/db/pool.rs`. This test
// doesn't (and can't, from a black-box unit test) prove serialization by
// itself, but it exercises a write concurrently with another
// `write_lock()`-guarded writer (`crate::db::insert_logs_batch`) and
// asserts both complete without a `database is locked` error — the
// observable symptom the invariant exists to prevent. See "Eng Review
// Fixes Applied" (Fix 2).
// --- Eng review fix (architecture-strategist): the error returned to the
// CALLER must be redacted, not just the copy persisted to the DB. Before
// this fix, `run`'s error path persisted `sanitize_error(&err)` to
// `llm_invocations.error` but returned the ORIGINAL `err` wrapped in
// `LlmRunnerError::Internal`, so a secret-shaped string in run_fn's error
// (e.g. leaked auth token in subprocess stderr) reached the CLI/MCP/REST
// caller directly even though the audit trail was clean.
#[tokio::test]
async fn caller_facing_error_is_redacted_not_just_the_db_row() {
    let (pool, _dir) = test_pool();
    let runner = LlmRunner::new(pool.clone(), LlmConfig::default());

    let result = runner
        .run(base_spec("ai_assess"), |_prompt| async {
            Err(anyhow::anyhow!(
                "gemini auth failed: API_KEY=super-secret-value sk-abc123secretvalue"
            ))
        })
        .await;

    let err = result.expect_err("run_fn error must propagate as an error");
    let caller_message = err.to_string();
    assert!(
        !caller_message.contains("super-secret-value"),
        "API_KEY value must be redacted in the error returned to the caller, got: {caller_message}"
    );
    assert!(
        !caller_message.contains("sk-abc123secretvalue"),
        "sk- prefixed token must be redacted in the error returned to the caller, got: {caller_message}"
    );
    assert!(
        caller_message.contains("[REDACTED]"),
        "expected [REDACTED] marker in the caller-facing error, got: {caller_message}"
    );
}

// --- Eng review fix (security-sentinel): a secret supplied as a JSON
// *value* without a `TOKEN=`/`API_KEY=`/`SECRET=` literal wrapper (just a
// bare `sk-`/`ghp_`/`atk_` prefix) must still be redacted. The old
// whole-blob redaction tokenized on whitespace, so
// `{"key":"sk-bareprefixsecretnodelimiter"}` became a single
// whitespace-free token starting with `{`, which `looks_secretish` never
// matched. Walking the JSON value tree and redacting each string leaf
// fixes this because the leaf value itself (not the surrounding braces)
// is what gets checked against `starts_with("sk-")`.
#[tokio::test]
async fn extra_metadata_bare_prefix_secret_without_key_wrapper_is_redacted() {
    let (pool, _dir) = test_pool();
    let runner = LlmRunner::new(pool.clone(), LlmConfig::default());

    let mut spec = base_spec("ai_assess");
    spec.extra_metadata = serde_json::json!({
        "key": "sk-bareprefixsecretnodelimiter"
    });

    let outcome = runner
        .run(spec, |_prompt| async { Ok("ok".to_string()) })
        .await
        .unwrap();

    let conn = pool.get().unwrap();
    let metadata_json: String = conn
        .query_row(
            "SELECT metadata_json FROM llm_invocations WHERE id = ?1",
            [outcome.invocation_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !metadata_json.contains("sk-bareprefixsecretnodelimiter"),
        "bare sk- prefixed value with no TOKEN=/API_KEY= wrapper must still be redacted, got: {metadata_json}"
    );
    assert!(
        metadata_json.contains("[REDACTED]"),
        "expected [REDACTED] marker, got: {metadata_json}"
    );
    // Must still be valid JSON after value-leaf redaction.
    let parsed: serde_json::Value = serde_json::from_str(&metadata_json)
        .expect("metadata_json must remain valid JSON after redaction");
    assert!(parsed.is_object());
}

// --- Eng review fix (architecture-strategist): `max_output_bytes` is
// documented and named as a BYTE limit, but the old implementation used
// `.chars().take(n)`, which limits to `n` characters, not bytes. For
// multi-byte UTF-8 output this let the actual persisted/returned byte
// length exceed the configured cap by up to ~4x.
#[tokio::test]
async fn output_truncation_respects_byte_limit_not_char_count_for_multibyte_utf8() {
    let (pool, _dir) = test_pool();
    // Each 'e' with combining acute (é as 2 codepoints) or, simpler, use a
    // 4-byte emoji so char count vs byte count diverges sharply.
    let cfg = LlmConfig {
        max_output_bytes: 10,
        ..LlmConfig::default()
    };
    let runner = LlmRunner::new(pool.clone(), cfg);

    // 5 emoji, each 4 bytes UTF-8 = 20 bytes, 5 chars. A `.chars().take(10)`
    // approach would keep all 5 emoji (only 5 chars) = 20 bytes, blowing
    // past max_output_bytes=10 by 2x.
    let multibyte_output = "\u{1F600}".repeat(5);
    assert_eq!(multibyte_output.chars().count(), 5);
    assert_eq!(multibyte_output.len(), 20);

    let outcome = runner
        .run(base_spec("ai_assess"), move |_prompt| {
            let out = multibyte_output.clone();
            async move { Ok(out) }
        })
        .await
        .unwrap();

    assert!(
        outcome.output.len() <= 10,
        "truncated output byte length must respect max_output_bytes=10, got {} bytes: {:?}",
        outcome.output.len(),
        outcome.output
    );
    assert_eq!(
        outcome.output_bytes,
        outcome.output.len(),
        "output_bytes must reflect actual byte length"
    );
    // Truncation must land on a valid UTF-8 boundary — the fact that
    // `outcome.output` is a valid `String` at all proves this (a
    // mid-codepoint slice would have panicked when building it).
}

#[tokio::test]
async fn audit_write_does_not_race_a_concurrent_write_locked_writer() {
    // Use a larger pool (default `for_test()` pool_size is 1, which starves
    // under 20 concurrent-ish blocking writers from two independent
    // subsystems) and rate limits generous enough for 20 LlmRunner::run
    // calls in a tight loop — this test is about write_lock() contention,
    // not the rate limiter, so isolate the two concerns.
    let dir = tempfile::tempdir().unwrap();
    let storage = crate::config::StorageConfig {
        pool_size: 4,
        ..crate::config::StorageConfig::for_test(dir.path().join("test.db"))
    };
    let pool = Arc::new(crate::db::init_pool(&storage).unwrap());
    let cfg = LlmConfig {
        max_invocations_per_minute: 1000,
        max_invocations_per_hour: 1000,
        ..LlmConfig::default()
    };
    let runner = LlmRunner::new(pool.clone(), cfg);

    fn test_log_entry(n: usize) -> crate::db::LogBatchEntry {
        crate::db::LogBatchEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            hostname: "llm-runner-lock-test".to_string(),
            facility: None,
            severity: "info".to_string(),
            app_name: None,
            process_id: None,
            message: format!("write_lock contention probe {n}"),
            raw: format!("write_lock contention probe {n}"),
            source_ip: "127.0.0.1:514".to_string(),
            docker_checkpoint: None,
            ai_tool: None,
            ai_project: None,
            ai_session_id: None,
            ai_transcript_path: None,
            metadata_json: None,
            http_status: None,
            auth_outcome: None,
            dns_blocked: None,
            event_action: None,
            parse_error: None,
        }
    }

    let pool_for_ingest = pool.clone();
    let ingest_handle = tokio::task::spawn_blocking(move || {
        for n in 0..20 {
            crate::db::insert_logs_batch(&pool_for_ingest, &[test_log_entry(n)])
                .expect("concurrent syslog batch insert must not fail under write_lock contention");
        }
    });

    for _ in 0..20 {
        runner
            .run(base_spec("ai_assess"), |_p| async { Ok("ok".to_string()) })
            .await
            .expect("concurrent audited LLM run must not fail under write_lock contention");
    }

    ingest_handle.await.unwrap();
}

// PR #106 reconciliation fix (type-design reviewer): `redact_json_value_strings`
// walks the whole `serde_json::Value` tree recursively (see its doc comment),
// but every existing `extra_metadata` test only ever exercises one level of
// nesting (a flat `{"key": "..."}` object). Prove the recursion actually
// reaches secrets buried inside nested arrays/objects, not just top-level
// string values.
#[tokio::test]
async fn extra_metadata_nested_secret_is_redacted_at_every_depth() {
    let (pool, _dir) = test_pool();
    let runner = LlmRunner::new(pool.clone(), LlmConfig::default());

    let mut spec = base_spec("ai_assess");
    // Three levels deep: object -> array -> object -> string leaf.
    spec.extra_metadata = serde_json::json!({
        "tags": [
            {"note": "TOKEN=leak-me-please"},
            {"nested": {"deeper": "API_KEY=also-leak-me"}}
        ]
    });

    let outcome = runner
        .run(spec, |_prompt| async { Ok("ok".to_string()) })
        .await
        .unwrap();

    let conn = pool.get().unwrap();
    let metadata_json: String = conn
        .query_row(
            "SELECT metadata_json FROM llm_invocations WHERE id = ?1",
            [outcome.invocation_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !metadata_json.contains("leak-me-please"),
        "secret nested inside an array element must be redacted, got: {metadata_json}"
    );
    assert!(
        !metadata_json.contains("also-leak-me"),
        "secret nested three levels deep (array -> object -> object) must be redacted, got: {metadata_json}"
    );
    assert!(
        metadata_json.matches("[REDACTED]").count() >= 2,
        "expected a [REDACTED] marker for each nested secret, got: {metadata_json}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&metadata_json)
        .expect("metadata_json must remain valid JSON after nested redaction");
    assert!(parsed.is_object());
}

// PR #106 reconciliation fix (code-reviewer): `global_concurrency_limit_denies_second_concurrent_call`
// isolates the GLOBAL limiter from the per-action one (max_per_action_concurrent
// set high). This test does the inverse — isolate the PER-ACTION limiter from
// the global one (max_concurrent set high) — so a second concurrent call to
// the SAME action is denied with `ActionConcurrencyLimited`, not swallowed by
// (or confused with) the global limiter. Uses the same oneshot-channel
// start/release synchronization pattern; no sleeps.
#[tokio::test]
async fn action_concurrency_limit_denies_second_concurrent_call_for_same_action() {
    let (pool, _dir) = test_pool();
    let cfg = LlmConfig {
        max_concurrent: 100, // isolate per-action limit from the global limit
        max_per_action_concurrent: 1,
        ..LlmConfig::default()
    };
    let runner = Arc::new(LlmRunner::new(pool.clone(), cfg));

    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let runner_a = runner.clone();
    let handle = tokio::spawn(async move {
        runner_a
            .run(base_spec("ai_assess"), move |_prompt| async move {
                started_tx.send(()).ok();
                release_rx.await.ok();
                Ok("first".to_string())
            })
            .await
    });
    started_rx.await.unwrap();

    // Same action name as the in-flight call above — must be denied by the
    // per-action limiter even though the global limiter has ample headroom.
    let second = runner
        .run(base_spec("ai_assess"), |_prompt| async {
            Ok("second".to_string())
        })
        .await;
    assert!(matches!(
        second,
        Err(LlmRunnerError::ActionConcurrencyLimited { ref action, limit: 1 })
            if action == "ai_assess"
    ));

    release_tx.send(()).ok();
    let first = handle.await.unwrap();
    assert!(first.is_ok());
}
