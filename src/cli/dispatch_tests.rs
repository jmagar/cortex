//! Tests for the query-command dispatch layer (bead 0p8r.7).
//!
//! Covered:
//!
//! - **Drift snapshots**: per command, `Cli*Args::into_request()` produces a
//!   stable `Debug` rendering. If anyone adds a field to either side without
//!   plumbing it through, the snapshot diff catches it (eng-review #A37).
//! - **HTTP success path**: each `run_X` against an [`HttpClient`] pointed at
//!   a [`MockServer`] succeeds and triggers EXACTLY ONE request — no
//!   `/api/version` probe on the success path (that only fires on 404 per
//!   bead .5).
//! - **Ctrl-C cancellation**: `http_or_cancel_with` is the testable form of
//!   `http_or_cancel`; pinning behaviour here proves the production wrapper
//!   bails with `"interrupted"` when SIGINT fires mid-flight (eng-review
//!   #A29).

use super::{
    http_or_cancel_with, run_ai_abuse, run_ai_add, run_ai_blocks, run_ai_checkpoints,
    run_ai_context, run_ai_correlate, run_ai_doctor, run_ai_errors, run_ai_index, run_ai_projects,
    run_ai_prune_checkpoints, run_ai_search, run_ai_smoke_watch, run_ai_tools, run_ai_watch,
    run_ai_watch_status, run_correlate, run_db_backup, run_db_checkpoint, run_db_integrity,
    run_db_status, run_db_vacuum, run_errors, run_hosts, run_search, run_sessions, run_stats,
    run_tail,
};
use crate::cli::http_client::HttpClient;
use crate::cli::{
    AiAbuseArgs, AiAddArgs, AiBlocksArgs, AiCheckpointsArgs, AiContextArgs, AiCorrelateArgs,
    AiDoctorArgs, AiErrorsArgs, AiIndexArgs, AiListArgs, AiPruneCheckpointsArgs, AiSearchArgs,
    AiWatchArgs, CliMode, CorrelateArgs, DbBackupArgs, DbCheckpointArgs, DbIntegrityArgs,
    DbStatusArgs, DbVacuumArgs, FilterArgs, IngestRateArgs, OutputArgs, PatternsArgs, SearchArgs,
    SessionsArgs, SigAckArgs, SigListArgs, SigUnackArgs, SourceIpsArgs, TailArgs, TimeRangeArgs,
    TimelineArgs,
};
use anyhow::{bail, Result};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ─── helpers ────────────────────────────────────────────────────────────────

async fn http_mode() -> (MockServer, CliMode) {
    let server = MockServer::start().await;
    let client =
        HttpClient::discover(Some(server.uri()), Some("test-token".into())).expect("discover ok");
    // Catch-all guard: any request that doesn't match a per-test
    // mock returns 404 and counts against `expect(0)`. Combined with the
    // per-test `expect(1)` on the actual endpoint, this asserts EXACTLY
    // one total request per command — surfaces stray /api/version probes
    // or any other extra call that would otherwise slip through silently.
    // Lowest priority (255) so per-test mocks always match first; only
    // unmatched requests fall through to the catch-all.
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(404))
        .with_priority(255)
        .expect(0)
        .mount(&server)
        .await;
    (server, CliMode::Http(client))
}

fn empty_search_logs_body() -> serde_json::Value {
    serde_json::json!({"count": 0, "logs": []})
}

// ─── Drift snapshots ────────────────────────────────────────────────────────
//
// We snapshot the `Debug` of the constructed Request. The Request struct is
// the SAME type that flows through both the Local and HTTP arms — so if the
// Debug output here matches our literal expectation, both arms by
// construction send the same shape. (We don't have to round-trip via
// `serde_qs` to a wire string; the Request IS the contract.)

#[test]
fn search_args_into_request_snapshot() {
    let args = SearchArgs {
        query: Some("foo".into()),
        hostname: Some("h1".into()),
        source_ip: Some("10.0.0.1".into()),
        severity: Some("error".into()),
        app_name: Some("nginx".into()),
        facility: Some("auth".into()),
        exclude_facility: Some("transcript".into()),
        from: Some("2026-01-01T00:00:00Z".into()),
        to: Some("2026-01-02T00:00:00Z".into()),
        received_from: Some("2026-01-01T00:00:30Z".into()),
        received_to: Some("2026-01-02T00:00:30Z".into()),
        limit: Some(50),
        json: true, // not propagated to Request — verified by snapshot below
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "SearchLogsRequest { query: Some(\"foo\"), hostname: Some(\"h1\"), source_ip: Some(\"10.0.0.1\"), severity: Some(\"error\"), app_name: Some(\"nginx\"), facility: Some(\"auth\"), exclude_facility: Some(\"transcript\"), process_id: None, from: Some(\"2026-01-01T00:00:00Z\"), to: Some(\"2026-01-02T00:00:00Z\"), received_from: Some(\"2026-01-01T00:00:30Z\"), received_to: Some(\"2026-01-02T00:00:30Z\"), limit: Some(50), source_kind: None, tool: None, project: None, session_id: None, container: None, docker_host: None, stream: None, event_action: None }"
    );
}

#[test]
fn filter_args_into_request_snapshot() {
    let args = FilterArgs {
        source_kind: Some("docker-stream".into()),
        docker_host: Some("dookie".into()),
        container: Some("cortex".into()),
        stream: Some("stdout".into()),
        event_action: Some("die".into()),
        tool: Some("claude".into()),
        project: Some("/tmp/project".into()),
        session_id: Some("abc123".into()),
        limit: Some(25),
        json: true,
        ..Default::default()
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "FilterLogsRequest { hostname: None, source_ip: None, severity: None, app_name: None, facility: None, exclude_facility: None, process_id: None, from: None, to: None, received_from: None, received_to: None, limit: Some(25), source_kind: Some(\"docker-stream\"), tool: Some(\"claude\"), project: Some(\"/tmp/project\"), session_id: Some(\"abc123\"), container: Some(\"cortex\"), docker_host: Some(\"dookie\"), stream: Some(\"stdout\"), event_action: Some(\"die\") }"
    );
}

#[test]
fn tail_args_into_request_snapshot() {
    let args = TailArgs {
        hostname: Some("h1".into()),
        source_ip: None,
        app_name: Some("docker".into()),
        n: Some(100),
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "TailLogsRequest { hostname: Some(\"h1\"), source_ip: None, app_name: Some(\"docker\"), severity_min: None, n: Some(100) }"
    );
}

#[test]
fn errors_args_into_request_snapshot() {
    let args = TimeRangeArgs {
        from: Some("2026-01-01T00:00:00Z".into()),
        to: None,
        limit: Some(10),
        json: false,
    };
    let req = args.into_errors_request();
    assert_eq!(
        format!("{req:?}"),
        "GetErrorsRequest { from: Some(\"2026-01-01T00:00:00Z\"), to: None, group_by: None, limit: Some(10) }"
    );
}

#[test]
fn sessions_args_into_request_snapshot() {
    let args = SessionsArgs {
        project: Some("/home/me/proj".into()),
        tool: Some("claude".into()),
        hostname: None,
        from: None,
        to: None,
        limit: Some(20),
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "ListSessionsRequest { project: Some(\"/home/me/proj\"), tool: Some(\"claude\"), hostname: None, from: None, to: None, limit: Some(20) }"
    );
}

#[test]
fn correlate_args_into_request_snapshot() {
    let args = CorrelateArgs {
        reference_time: "2026-01-01T12:00:00Z".into(),
        window_minutes: Some(15),
        severity_min: Some("warning".into()),
        hostname: Some("h1".into()),
        source_ip: None,
        query: Some("oom".into()),
        limit: Some(50),
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "CorrelateEventsRequest { reference_time: \"2026-01-01T12:00:00Z\", window_minutes: Some(15), severity_min: Some(\"warning\"), hostname: Some(\"h1\"), source_ip: None, query: Some(\"oom\"), limit: Some(50) }"
    );
}

// ─── HTTP success path: exactly one request per command ─────────────────────
//
// The `.expect(1)` on each mock asserts EXACTLY ONE call. If anyone wires
// in a /api/version probe on the success path (which would only be correct
// on 404 — see bead .5), wiremock panics on drop.

#[tokio::test]
async fn run_search_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_search_logs_body()))
        .expect(1)
        .mount(&server)
        .await;
    let args = SearchArgs {
        query: Some("foo".into()),
        json: true, // suppress non-JSON noise on stdout
        ..Default::default()
    };
    run_search(&mode, args).await.expect("search ok");
}

#[tokio::test]
async fn run_tail_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/tail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_search_logs_body()))
        .expect(1)
        .mount(&server)
        .await;
    let args = TailArgs {
        n: Some(10),
        json: true,
        ..Default::default()
    };
    run_tail(&mode, args).await.expect("tail ok");
}

#[tokio::test]
async fn run_errors_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/errors"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "summary": [],
        })))
        .expect(1)
        .mount(&server)
        .await;
    let args = TimeRangeArgs {
        json: true,
        ..Default::default()
    };
    run_errors(&mode, args).await.expect("errors ok");
}

#[tokio::test]
async fn run_hosts_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/hosts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"hosts": []})))
        .expect(1)
        .mount(&server)
        .await;
    run_hosts(&mode, OutputArgs { json: true })
        .await
        .expect("hosts ok");
}

#[tokio::test]
async fn run_stats_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    // DbStats has a number of fields; we only need a 200 body that
    // deserialises into DbStats. The shape comes from `cortex::app::DbStats`.
    Mock::given(method("GET"))
        .and(path("/api/stats"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total_logs": 0,
            "total_hosts": 0,
            "oldest_log": null,
            "newest_log": null,
            "logical_db_size_mb": "0.00",
            "physical_db_size_mb": "0.00",
            "free_disk_mb": null,
            "max_db_size_mb": 0,
            "min_free_disk_mb": 0,
            "write_blocked": false,
            "phantom_fts_rows": 0,
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_stats(&mode, OutputArgs { json: true })
        .await
        .expect("stats ok");
}

#[tokio::test]
async fn run_sessions_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/sessions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 0,
            "sessions": [],
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_sessions(
        &mode,
        SessionsArgs {
            json: true,
            ..Default::default()
        },
    )
    .await
    .expect("sessions ok");
}

#[tokio::test]
async fn run_correlate_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/correlate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "reference_time": "2026-01-01T12:00:00Z",
            "window_minutes": 15,
            "window_from": "2026-01-01T11:45:00Z",
            "window_to": "2026-01-01T12:15:00Z",
            "severity_min": "info",
            "total_events": 0,
            "truncated": false,
            "hosts_count": 0,
            "hosts": [],
        })))
        .expect(1)
        .mount(&server)
        .await;
    let args = CorrelateArgs {
        reference_time: "2026-01-01T12:00:00Z".into(),
        json: true,
        ..Default::default()
    };
    run_correlate(&mode, args).await.expect("correlate ok");
}

// ─── HTTP request shape verification (drift between Local + HTTP) ───────────
//
// We can't intercept the Local arm without spinning a SQLite database, so
// the drift snapshots above pin the Request struct. To prove the HTTP arm
// sends the SAME shape, we assert on the actual query string wiremock
// receives.

#[tokio::test]
async fn run_search_http_sends_expected_query_params() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_search_logs_body()))
        .expect(1)
        .mount(&server)
        .await;
    let args = SearchArgs {
        query: Some("foo".into()),
        hostname: Some("h1".into()),
        severity: Some("error".into()),
        limit: Some(50),
        json: true,
        ..Default::default()
    };
    run_search(&mode, args).await.expect("ok");

    // Verify the request that landed has the expected query params.
    let received = server.received_requests().await.expect("requests");
    let req = received
        .iter()
        .find(|r| r.url.path() == "/api/search")
        .expect("search request");
    let qs = req.url.query().unwrap_or("");
    assert!(qs.contains("query=foo"), "missing query=foo in {qs}");
    assert!(qs.contains("hostname=h1"), "missing hostname=h1 in {qs}");
    assert!(
        qs.contains("severity=error"),
        "missing severity=error in {qs}"
    );
    assert!(qs.contains("limit=50"), "missing limit=50 in {qs}");
}

// ─── Cancellation: http_or_cancel_with bails with "interrupted" ─────────────

#[tokio::test]
async fn http_or_cancel_returns_inner_result_when_fut_finishes_first() {
    let res: Result<u32> = http_or_cancel_with(
        async { Ok(42u32) },
        // Cancel future never resolves within the test window.
        async {
            tokio::time::sleep(Duration::from_secs(10)).await;
        },
    )
    .await;
    assert_eq!(res.unwrap(), 42);
}

#[tokio::test]
async fn http_or_cancel_bails_interrupted_when_cancel_fires_first() {
    let res: Result<u32> = http_or_cancel_with(
        async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok(42)
        },
        async {
            tokio::time::sleep(Duration::from_millis(10)).await;
        },
    )
    .await;
    let err = res.expect_err("cancel should win");
    assert_eq!(err.to_string(), "interrupted");
}

#[tokio::test]
async fn http_or_cancel_propagates_inner_error() {
    let res: Result<u32> = http_or_cancel_with(async { bail!("inner kaboom") }, async {
        tokio::time::sleep(Duration::from_secs(10)).await;
    })
    .await;
    let err = res.expect_err("inner err");
    assert!(err.to_string().contains("inner kaboom"));
}

/// End-to-end: a `run_search` against a slow mock server, cancelled by a
/// short timer playing the role of SIGINT. Proves the cancellation wraps
/// the real HTTP path (not just the helper in isolation).
#[tokio::test]
async fn run_search_via_dispatch_can_be_cancelled() {
    let (server, _mode_unused) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(empty_search_logs_body())
                .set_delay(Duration::from_secs(10)),
        )
        .mount(&server)
        .await;
    let client = HttpClient::discover(Some(server.uri()), Some("test-token".into())).unwrap();
    let req = SearchArgs {
        query: Some("foo".into()),
        json: true,
        ..Default::default()
    }
    .into_request();
    let res: Result<()> = http_or_cancel_with(
        async {
            let _ = client.search(&req).await?;
            Ok(())
        },
        async {
            tokio::time::sleep(Duration::from_millis(50)).await;
        },
    )
    .await;
    let err = res.expect_err("cancel should win against 10s mock delay");
    assert_eq!(err.to_string(), "interrupted");
}

// ─── bead 0p8r.8: AI dispatch ───────────────────────────────────────────────

// Drift snapshots — one per HTTP-capable AI command (10). Pins the wire
// Request struct constructed from CLI args. The same struct flows through
// both Local and HTTP arms, so if the Debug rendering matches, the two
// paths send identical shapes (cf. bead .7 #A37).

#[test]
fn ai_search_args_into_request_snapshot() {
    let args = AiSearchArgs {
        query: "needle".into(),
        project: Some("/p".into()),
        tool: Some("claude".into()),
        from: Some("2026-01-01T00:00:00Z".into()),
        to: Some("2026-01-02T00:00:00Z".into()),
        limit: Some(25),
        json: true,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "SearchSessionsRequest { query: \"needle\", project: Some(\"/p\"), tool: Some(\"claude\"), from: Some(\"2026-01-01T00:00:00Z\"), to: Some(\"2026-01-02T00:00:00Z\"), limit: Some(25) }"
    );
}

#[test]
fn ai_abuse_args_into_request_snapshot() {
    let args = AiAbuseArgs {
        project: Some("/p".into()),
        tool: Some("claude".into()),
        from: None,
        to: None,
        limit: Some(10),
        before: Some(3),
        after: Some(2),
        terms: vec!["bad".into(), "worse".into()],
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "AbuseSearchRequest { project: Some(\"/p\"), tool: Some(\"claude\"), from: None, to: None, limit: Some(10), before: Some(3), after: Some(2), terms: [\"bad\", \"worse\"] }"
    );
}

#[test]
fn ai_correlate_args_into_request_snapshot() {
    let args = AiCorrelateArgs {
        project: Some("/p".into()),
        tool: Some("claude".into()),
        session_id: Some("s1".into()),
        ai_query: Some("ai".into()),
        log_query: Some("log".into()),
        hostname: Some("h1".into()),
        source_ip: Some("10.0.0.1".into()),
        app_name: Some("nginx".into()),
        from: Some("2026-01-01T00:00:00Z".into()),
        to: Some("2026-01-02T00:00:00Z".into()),
        window_minutes: Some(15),
        severity_min: Some("warning".into()),
        limit: Some(50),
        events_per_anchor: Some(20),
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "AiCorrelateRequest { project: Some(\"/p\"), tool: Some(\"claude\"), session_id: Some(\"s1\"), ai_query: Some(\"ai\"), log_query: Some(\"log\"), hostname: Some(\"h1\"), source_ip: Some(\"10.0.0.1\"), app_name: Some(\"nginx\"), from: Some(\"2026-01-01T00:00:00Z\"), to: Some(\"2026-01-02T00:00:00Z\"), window_minutes: Some(15), severity_min: Some(\"warning\"), limit: Some(50), events_per_anchor: Some(20) }"
    );
}

#[test]
fn ai_blocks_args_into_request_snapshot() {
    let args = AiBlocksArgs {
        project: Some("/p".into()),
        tool: None,
        from: None,
        to: None,
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "UsageBlocksRequest { project: Some(\"/p\"), tool: None, from: None, to: None }"
    );
}

#[test]
fn ai_context_args_into_request_snapshot() {
    let args = AiContextArgs {
        project: "/p".into(),
        tool: Some("claude".into()),
        limit: Some(10),
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "ProjectContextRequest { project: \"/p\", tool: Some(\"claude\"), limit: Some(10) }"
    );
}

#[test]
fn ai_tools_args_into_request_snapshot() {
    let args = AiListArgs {
        project: Some("/p".into()),
        tool: None,
        from: Some("2026-01-01T00:00:00Z".into()),
        to: None,
        json: false,
    };
    let req = args.into_tools_request();
    assert_eq!(
        format!("{req:?}"),
        "ListAiToolsRequest { project: Some(\"/p\"), from: Some(\"2026-01-01T00:00:00Z\"), to: None }"
    );
}

#[test]
fn ai_projects_args_into_request_snapshot() {
    let args = AiListArgs {
        project: None,
        tool: Some("claude".into()),
        from: None,
        to: Some("2026-01-02T00:00:00Z".into()),
        json: false,
    };
    let req = args.into_projects_request();
    assert_eq!(
        format!("{req:?}"),
        "ListAiProjectsRequest { tool: Some(\"claude\"), from: None, to: Some(\"2026-01-02T00:00:00Z\") }"
    );
}

#[test]
fn ai_checkpoints_args_into_request_snapshot() {
    let args = AiCheckpointsArgs {
        errors_only: true,
        missing_only: false,
        limit: Some(20),
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "AiCheckpointsRequest { errors_only: true, missing_only: false, limit: Some(20) }"
    );
}

#[test]
fn ai_errors_args_into_request_snapshot() {
    let args = AiErrorsArgs {
        limit: Some(5),
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "AiParseErrorsRequest { limit: Some(5) }"
    );
}

#[test]
fn ai_prune_checkpoints_args_into_request_snapshot() {
    let args = AiPruneCheckpointsArgs {
        missing_only: true,
        dry_run: true,
        limit: Some(100),
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "AiPruneCheckpointsRequest { dry_run: true, missing_only: true, limit: Some(100) }"
    );
}

// ─── HTTP success path: one request per HTTP-capable AI command (10) ────────

fn empty_search_sessions_body() -> serde_json::Value {
    serde_json::json!({
        "total_candidates": 0,
        "candidate_rows": 0,
        "candidate_cap": 0,
        "candidate_window_truncated": false,
        "truncated": false,
        "sessions": [],
    })
}

#[tokio::test]
async fn run_ai_search_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/ai/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_search_sessions_body()))
        .expect(1)
        .mount(&server)
        .await;
    run_ai_search(
        &mode,
        AiSearchArgs {
            query: "q".into(),
            json: true,
            ..Default::default()
        },
    )
    .await
    .expect("ai search ok");
}

#[tokio::test]
async fn run_ai_abuse_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/ai/abuse"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "terms": [],
            "candidate_rows": 0,
            "candidate_cap": 0,
            "candidate_window_truncated": false,
            "truncated": false,
            "matches": [],
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_ai_abuse(
        &mode,
        AiAbuseArgs {
            terms: vec!["bad".into()],
            json: true,
            ..Default::default()
        },
    )
    .await
    .expect("ai abuse ok");
}

#[tokio::test]
async fn run_ai_correlate_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/ai/correlate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "window_minutes": 15,
            "severity_min": "info",
            "total_anchors": 0,
            "anchor_rows": 0,
            "anchor_limit": 0,
            "anchors_truncated": false,
            "related_limit_per_anchor": 0,
            "total_related_events": 0,
            "anchors": [],
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_ai_correlate(
        &mode,
        AiCorrelateArgs {
            json: true,
            ..Default::default()
        },
    )
    .await
    .expect("ai correlate ok");
}

#[tokio::test]
async fn run_ai_blocks_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/ai/blocks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total_blocks": 0,
            "truncated": false,
            "blocks": [],
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_ai_blocks(
        &mode,
        AiBlocksArgs {
            json: true,
            ..Default::default()
        },
    )
    .await
    .expect("ai blocks ok");
}

#[tokio::test]
async fn run_ai_context_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/ai/context"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "project": "/p",
            "tools": [],
            "sessions": [],
            "hostnames": [],
            "first_seen": null,
            "last_seen": null,
            "event_count": 0,
            "recent_entries_truncated": false,
            "recent_entries": [],
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_ai_context(
        &mode,
        AiContextArgs {
            project: "/p".into(),
            json: true,
            ..Default::default()
        },
    )
    .await
    .expect("ai context ok");
}

#[tokio::test]
async fn run_ai_tools_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/ai/tools"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total_tools": 0,
            "truncated": false,
            "tools": [],
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_ai_tools(
        &mode,
        AiListArgs {
            json: true,
            ..Default::default()
        },
    )
    .await
    .expect("ai tools ok");
}

#[tokio::test]
async fn run_ai_projects_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/ai/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total_projects": 0,
            "truncated": false,
            "projects": [],
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_ai_projects(
        &mode,
        AiListArgs {
            json: true,
            ..Default::default()
        },
    )
    .await
    .expect("ai projects ok");
}

#[tokio::test]
async fn run_ai_checkpoints_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/ai/checkpoints"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .expect(1)
        .mount(&server)
        .await;
    run_ai_checkpoints(
        &mode,
        AiCheckpointsArgs {
            json: true,
            ..Default::default()
        },
    )
    .await
    .expect("ai checkpoints ok");
}

#[tokio::test]
async fn run_ai_errors_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/ai/errors"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .expect(1)
        .mount(&server)
        .await;
    run_ai_errors(
        &mode,
        AiErrorsArgs {
            json: true,
            ..Default::default()
        },
    )
    .await
    .expect("ai errors ok");
}

#[tokio::test]
async fn run_ai_prune_checkpoints_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("POST"))
        .and(path("/api/ai/prune-checkpoints"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "matched": 0,
            "pruned": 0,
            "dry_run": true,
            "paths": [],
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_ai_prune_checkpoints(
        &mode,
        AiPruneCheckpointsArgs {
            dry_run: true,
            json: true,
            ..Default::default()
        },
    )
    .await
    .expect("ai prune-checkpoints ok");
}

// ─── LOCAL-only HTTP-mode error tests (6) ───────────────────────────────────
//
// Each LOCAL-only command in HTTP mode must exit non-zero with its exact
// inline message. `assert_eq!` on the err string catches drift.

async fn http_only_mode() -> CliMode {
    // No mock server needed — these tests never make an HTTP call.
    let client =
        HttpClient::discover(Some("http://127.0.0.1:1".into()), Some("t".into())).expect("ok");
    CliMode::Http(client)
}

#[tokio::test]
async fn run_ai_index_http_bails_with_inline_message() {
    let mode = http_only_mode().await;
    let err = run_ai_index(&mode, AiIndexArgs::default())
        .await
        .expect_err("must bail in http mode");
    assert_eq!(
        err.to_string(),
        "ai index reads host ~/.claude/projects; omit --http"
    );
}

#[tokio::test]
async fn run_ai_add_http_bails_with_inline_message() {
    let mode = http_only_mode().await;
    let err = run_ai_add(
        &mode,
        AiAddArgs {
            file: "/tmp/x".into(),
            ..Default::default()
        },
    )
    .await
    .expect_err("must bail in http mode");
    assert_eq!(
        err.to_string(),
        "ai add reads a host file path; omit --http"
    );
}

#[tokio::test]
async fn run_ai_doctor_http_bails_with_inline_message() {
    let mode = http_only_mode().await;
    let err = run_ai_doctor(&mode, AiDoctorArgs::default())
        .await
        .expect_err("must bail in http mode");
    assert_eq!(
        err.to_string(),
        "ai doctor checks host filesystem permissions; omit --http"
    );
}

#[tokio::test]
async fn run_ai_smoke_watch_http_bails_with_inline_message() {
    let mode = http_only_mode().await;
    let err = run_ai_smoke_watch(&mode, OutputArgs { json: true })
        .await
        .expect_err("must bail in http mode");
    assert_eq!(
        err.to_string(),
        "ai smoke-watch writes synthetic transcript to host fs; omit --http"
    );
}

#[tokio::test]
async fn run_ai_watch_status_http_bails_with_inline_message() {
    let mode = http_only_mode().await;
    let err = run_ai_watch_status(&mode, OutputArgs { json: true })
        .await
        .expect_err("must bail in http mode");
    assert_eq!(
        err.to_string(),
        "ai watch-status shells out to systemctl on host; omit --http"
    );
}

#[tokio::test]
async fn run_ai_watch_http_bails_with_inline_message() {
    let mode = http_only_mode().await;
    let err = run_ai_watch(&mode, AiWatchArgs::default())
        .await
        .expect_err("must bail in http mode");
    assert_eq!(
        err.to_string(),
        "ai watch is a long-running daemon; omit --http"
    );
}

// ─── DB drift snapshots (bead 0p8r.9) ───────────────────────────────────────

// Bead 0p8r.29: the DbIntegrityArgs / DbCheckpointArgs identity-map
// `into_request` impls were inlined at the call sites. The remaining drift
// risk lives in `run_db_integrity` / `run_db_checkpoint`, which still
// construct `*Request` from `args`. The snapshot tests previously checked
// the trivial map; they're dropped here since there's no longer a discrete
// transform to snapshot. `DbVacuumArgs::into_request` retains its snapshot
// because the bool→Option<bool> rewrite is non-trivial.

#[test]
fn db_vacuum_args_into_request_snapshot_force_absent_maps_to_none() {
    let req = DbVacuumArgs {
        full: true,
        pages: 1000,
        force: false,
        json: false,
    }
    .into_request();
    // Force is `None` when CLI bool is false — server treats `None` and
    // `Some(false)` identically (pre-flight stays in force).
    assert_eq!(
        format!("{req:?}"),
        "DbVacuumRequest { full: true, incremental_pages: 1000, force: None }"
    );
}

#[test]
fn db_vacuum_args_into_request_snapshot_force_present_maps_to_some_true() {
    let req = DbVacuumArgs {
        full: true,
        pages: 500,
        force: true,
        json: true,
    }
    .into_request();
    assert_eq!(
        format!("{req:?}"),
        "DbVacuumRequest { full: true, incremental_pages: 500, force: Some(true) }"
    );
}

// ─── DB HTTP success path: exactly one request per command ──────────────────

fn db_status_body() -> serde_json::Value {
    serde_json::json!({
        "db_path": "/data/cortex.db",
        "page_count": 1,
        "freelist_count": 0,
        "page_size": 4096,
        "logical_size_bytes": 4096,
        "physical_size_bytes": 4096,
        "wal_size_bytes": null,
        "shm_size_bytes": null,
        "auto_vacuum": 0,
        "journal_mode": "wal",
        "integrity_ok": null,
        "integrity_messages": [],
    })
}

#[tokio::test]
async fn run_db_status_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/db/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(db_status_body()))
        .expect(1)
        .mount(&server)
        .await;
    run_db_status(
        &mode,
        DbStatusArgs {
            json: true,
            check_coord: false,
        },
    )
    .await
    .expect("db status ok");
}

#[tokio::test]
async fn run_db_integrity_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/db/integrity"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "messages": [],
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_db_integrity(
        &mode,
        DbIntegrityArgs {
            quick: true,
            json: true,
        },
    )
    .await
    .expect("db integrity ok");
}

#[tokio::test]
async fn run_db_checkpoint_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("POST"))
        .and(path("/api/db/checkpoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "mode": "passive",
            "busy": 0,
            "log_frames": 0,
            "checkpointed_frames": 0,
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_db_checkpoint(
        &mode,
        DbCheckpointArgs {
            mode: "passive".into(),
            json: true,
        },
    )
    .await
    .expect("db checkpoint ok");
}

#[tokio::test]
async fn run_db_vacuum_http_sends_exactly_one_request() {
    let (server, mode) = http_mode().await;
    Mock::given(method("POST"))
        .and(path("/api/db/vacuum"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "full": false,
            "incremental_pages": 1000,
            "before_physical_size_bytes": 4096,
            "after_physical_size_bytes": 4096,
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_db_vacuum(
        &mode,
        DbVacuumArgs {
            full: false,
            pages: 1000,
            force: false,
            json: true,
        },
    )
    .await
    .expect("db vacuum ok");
}

// ─── DB integrity failure surfaces as bail ──────────────────────────────────

#[tokio::test]
async fn run_db_integrity_bails_when_response_not_ok() {
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/db/integrity"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": false,
            "messages": ["row 1 missing"],
        })))
        .mount(&server)
        .await;
    let err = run_db_integrity(
        &mode,
        DbIntegrityArgs {
            quick: false,
            json: true,
        },
    )
    .await
    .expect_err("must bail when integrity fails");
    assert_eq!(err.to_string(), "database integrity check failed");
}

#[tokio::test]
async fn run_db_checkpoint_bails_when_busy_nonzero() {
    let (server, mode) = http_mode().await;
    Mock::given(method("POST"))
        .and(path("/api/db/checkpoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "mode": "passive",
            "busy": 1,
            "log_frames": 0,
            "checkpointed_frames": 0,
        })))
        .mount(&server)
        .await;
    let err = run_db_checkpoint(
        &mode,
        DbCheckpointArgs {
            mode: "passive".into(),
            json: true,
        },
    )
    .await
    .expect_err("must bail when busy");
    assert_eq!(err.to_string(), "database WAL checkpoint was busy");
}

// ─── DB vacuum --force serializes correctly in the request body ─────────────

#[tokio::test]
async fn run_db_vacuum_force_present_sends_force_true_body() {
    let (server, mode) = http_mode().await;
    Mock::given(method("POST"))
        .and(path("/api/db/vacuum"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "full": true,
            "incremental_pages": 1000,
            "before_physical_size_bytes": 0,
            "after_physical_size_bytes": 0,
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_db_vacuum(
        &mode,
        DbVacuumArgs {
            full: true,
            pages: 1000,
            force: true,
            json: true,
        },
    )
    .await
    .expect("vacuum ok");

    let received = server.received_requests().await.expect("requests");
    let req = received
        .iter()
        .find(|r| r.url.path() == "/api/db/vacuum")
        .expect("vacuum request");
    let body: serde_json::Value = serde_json::from_slice(&req.body).expect("body parses as JSON");
    assert_eq!(body["full"], serde_json::Value::Bool(true));
    assert_eq!(body["force"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn run_db_vacuum_force_absent_does_not_send_force_true() {
    let (server, mode) = http_mode().await;
    Mock::given(method("POST"))
        .and(path("/api/db/vacuum"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "full": false,
            "incremental_pages": 1000,
            "before_physical_size_bytes": 0,
            "after_physical_size_bytes": 0,
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_db_vacuum(
        &mode,
        DbVacuumArgs {
            full: false,
            pages: 1000,
            force: false,
            json: true,
        },
    )
    .await
    .expect("vacuum ok");

    let received = server.received_requests().await.expect("requests");
    let req = received
        .iter()
        .find(|r| r.url.path() == "/api/db/vacuum")
        .expect("vacuum request");
    let body: serde_json::Value = serde_json::from_slice(&req.body).expect("body parses as JSON");
    // `force: None` serializes as JSON `null`, not as missing — but either
    // way, the value must NOT be `true`. Server semantics: only `Some(true)`
    // bypasses the size pre-flight.
    assert_ne!(body["force"], serde_json::Value::Bool(true));
}

// ─── DB backup: HTTP mode routes to POST /api/db/backup ─────────────────────
//
// HTTP mode now forwards to the server (xknb fix): the server runs the backup
// via the rusqlite online backup API on its own pool connection, avoiding
// SQLITE_BUSY when the container is actively ingesting logs.

#[tokio::test]
async fn run_db_backup_http_posts_to_api_endpoint() {
    let (server, mode) = http_mode().await;
    Mock::given(method("POST"))
        .and(path("/api/db/backup"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "db_path": "/data/cortex.db",
            "backup_path": "/data/backup.db",
            "size_bytes": 1024
        })))
        .expect(1)
        .mount(&server)
        .await;
    run_db_backup(
        &mode,
        DbBackupArgs {
            output: Some("/data/backup.db".into()),
            json: true,
        },
    )
    .await
    .expect("http backup must succeed");
}

// ─── HTTP client timeout (bead 0p8r.5 / bead cortex-qekb) ──────────────
//
// bead 0p8r.5 originally specified no per-method timeout on `db integrity`.
// bead cortex-qekb revised that: `run_db_integrity` now wraps the HTTP
// arm in a 120s `tokio::time::timeout` (via `INTEGRITY_HTTP_TIMEOUT`) so a
// 31 GB+ DB does not silently hit the global 600s reqwest timeout. Fast
// requests (well under 120s) continue to complete normally — that is all this
// test exercises. The timeout-fires path is covered by
// `dispatch_db_tests::run_db_integrity_http_timeout_emits_actionable_message`.

#[tokio::test]
async fn db_integrity_http_request_completes_within_integrity_timeout_budget() {
    // A 50ms mock response is well under the 120s INTEGRITY_HTTP_TIMEOUT, so
    // the call should succeed and the timeout wrapper should be a no-op.
    let (server, mode) = http_mode().await;
    Mock::given(method("GET"))
        .and(path("/api/db/integrity"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"ok": true, "messages": []}))
                .set_delay(Duration::from_millis(50)),
        )
        .expect(1)
        .mount(&server)
        .await;
    run_db_integrity(
        &mode,
        DbIntegrityArgs {
            quick: false,
            json: true,
        },
    )
    .await
    .expect("db integrity ok under 120s timeout budget");
}

// ─── Surface parity snapshot tests (Task 5/6) ───────────────────────────────

#[test]
fn source_ips_args_into_request_default() {
    let args = SourceIpsArgs {
        limit: None,
        offset: None,
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "ListSourceIpsRequest { limit: None, offset: None }"
    );
}

#[test]
fn timeline_args_into_request_passes_time_range_through() {
    // The default-lookback injection now lives in `CortexService::timeline`
    // (bead dyqw) so the service is the single source of truth. `into_request`
    // must therefore pass `from`/`to` through verbatim and NOT inject a default
    // — verified end-to-end by the service-layer test
    // `timeline_applies_default_lookback_only_when_from_and_to_both_absent`.
    let args = TimelineArgs {
        bucket: Some("hour".to_string()),
        group_by: None,
        from: None,
        to: None,
        hostname: None,
        app_name: None,
        severity_min: None,
        json: false,
    };
    let req = args.into_request();
    assert_eq!(req.bucket.as_deref(), Some("hour"));
    assert!(
        req.from.is_none(),
        "into_request must not inject a default `from`; the service applies it"
    );
    assert!(
        req.to.is_none(),
        "into_request must not inject a default `to`"
    );
}

#[test]
fn timeline_args_into_request_explicit_from_preserved() {
    // Explicit from must override the default.
    let args = TimelineArgs {
        bucket: None,
        group_by: None,
        from: Some("2025-01-01T00:00:00Z".to_string()),
        to: None,
        hostname: None,
        app_name: None,
        severity_min: None,
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        req.from.as_deref(),
        Some("2025-01-01T00:00:00Z"),
        "explicit from must not be overridden by the default"
    );
}

#[test]
fn patterns_args_into_request_default() {
    let args = PatternsArgs::default();
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "PatternsRequest { from: None, to: None, hostname: None, app_name: None, severity_min: None, scan_limit: None, top_n: None }"
    );
}

#[test]
fn ingest_rate_args_into_request_by_host() {
    let args = IngestRateArgs {
        by_host: true,
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "IngestRateRequest { by_host: Some(true) }"
    );
}

#[test]
fn ingest_rate_args_into_request_default_unset() {
    let args = IngestRateArgs::default();
    let req = args.into_request();
    assert_eq!(format!("{req:?}"), "IngestRateRequest { by_host: None }");
}

#[test]
fn sig_list_args_default() {
    let args = SigListArgs {
        limit: None,
        include_acknowledged: false,
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "UnaddressedErrorsRequest { limit: None, include_acknowledged: Some(false) }"
    );
}

#[test]
fn sig_ack_args_with_notes() {
    let args = SigAckArgs {
        signature_hash: "abc123".to_string(),
        notes: Some("arcane auto-heal bug".to_string()),
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "AckErrorRequest { signature_hash: \"abc123\", notes: Some(\"arcane auto-heal bug\") }"
    );
}

#[test]
fn sig_unack_args_with_reason() {
    let args = SigUnackArgs {
        signature_hash: "def456".to_string(),
        reason: Some("regression fixed in v0.27.3".to_string()),
        json: false,
    };
    let req = args.into_request();
    assert_eq!(
        format!("{req:?}"),
        "UnackErrorRequest { signature_hash: \"def456\", reason: Some(\"regression fixed in v0.27.3\") }"
    );
}
