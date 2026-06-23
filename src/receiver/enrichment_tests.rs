//! Tests for the enrichment pipeline.

use std::fs;

use super::*;

fn entry(app: &str, msg: &str, source_ip: &str, severity: &str) -> LogBatchEntry {
    LogBatchEntry {
        timestamp: "2026-05-07T00:00:00.000Z".to_string(),
        hostname: "test".to_string(),
        facility: None,
        severity: severity.to_string(),
        app_name: Some(app.to_string()),
        process_id: None,
        message: msg.to_string(),
        raw: String::new(),
        source_ip: source_ip.to_string(),
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

// ---- authelia severity parsing ----

#[test]
fn authelia_level_warn_promotes_severity() {
    let cfg = EnrichmentConfig::default();
    let e = entry(
        "authelia",
        "time=2026-05-07 level=warn msg=\"failed login\"",
        "10.0.0.1:1234",
        "info",
    );
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.severity, "warning");
}

#[test]
fn authelia_level_error_promotes_severity() {
    let cfg = EnrichmentConfig::default();
    let e = entry("authelia", "level=error msg=test", "10.0.0.1:1", "info");
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.severity, "err");
}

#[test]
fn authelia_no_level_keeps_severity() {
    let cfg = EnrichmentConfig::default();
    let e = entry("authelia", "no level field here", "10.0.0.1:1", "info");
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.severity, "info");
}

#[test]
fn authelia_unknown_level_keeps_severity() {
    let cfg = EnrichmentConfig::default();
    let e = entry("authelia", "level=galaxy", "10.0.0.1:1", "info");
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.severity, "info");
}

#[test]
fn authelia_source_ip_gating_blocks_non_matching() {
    let cfg = EnrichmentConfig {
        authelia_source_ip: Some("192.168.1.10".into()),
        ..Default::default()
    };
    let e = entry("authelia", "level=error msg=foo", "10.0.0.1:1", "info");
    let out = enrich_entry(e, &cfg);
    // Severity must NOT be promoted because source IP doesn't match.
    assert_eq!(out.severity, "info");
}

#[test]
fn authelia_source_ip_gating_allows_matching() {
    let cfg = EnrichmentConfig {
        authelia_source_ip: Some("192.168.1.10".into()),
        ..Default::default()
    };
    let e = entry(
        "authelia",
        "level=error msg=foo",
        "192.168.1.10:5000",
        "info",
    );
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.severity, "err");
}

/// Regression: prefix `10.0.0.1` must NOT match a spoofer at `10.0.0.10`.
/// Plain `starts_with` would have, allowing an attacker one IP off from the
/// configured authelia host to inject `level=` reclassifications.
#[test]
fn authelia_source_ip_gating_blocks_prefix_collision() {
    let cfg = EnrichmentConfig {
        authelia_source_ip: Some("10.0.0.1".into()),
        ..Default::default()
    };
    // Attacker on 10.0.0.10 — would pass naive starts_with(10.0.0.1)
    let e = entry("authelia", "level=err msg=spoof", "10.0.0.10:1234", "info");
    let out = enrich_entry(e, &cfg);
    assert_eq!(
        out.severity, "info",
        "prefix-collision must not bypass gate"
    );
}

/// Subnet match: prefix ending in `.` (e.g. `10.0.0.`) matches any host in
/// that octet boundary.
#[test]
fn authelia_source_ip_gating_subnet_match_with_trailing_dot() {
    let cfg = EnrichmentConfig {
        authelia_source_ip: Some("10.0.0.".into()),
        ..Default::default()
    };
    let e = entry("authelia", "level=warn msg=ok", "10.0.0.42:9000", "info");
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.severity, "warning");
}

// ---- adguard tag classification ----

#[test]
fn adguard_filtered_becomes_blocked() {
    let cfg = EnrichmentConfig::default();
    let body = r#"{"QH":"ads.example.com","Result":{"IsFiltered":true,"Reason":"FilteredBlackList"},"Upstream":""}"#;
    let e = entry("adguard-query", body, "10.0.0.1:1", "info");
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.app_name.as_deref(), Some("adguard-blocked"));
}

#[test]
fn adguard_unfiltered_with_upstream_becomes_allowed() {
    let cfg = EnrichmentConfig::default();
    let body =
        r#"{"QH":"github.com","Result":{"IsFiltered":false,"Reason":""},"Upstream":"9.9.9.9:53"}"#;
    let e = entry("adguard-query", body, "10.0.0.1:1", "info");
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.app_name.as_deref(), Some("adguard-allowed"));
}

#[test]
fn adguard_rewrite_classified() {
    let cfg = EnrichmentConfig::default();
    let body =
        r#"{"QH":"local.lan","Result":{"IsFiltered":false,"Reason":"Rewrite"},"Upstream":""}"#;
    let e = entry("adguard-query", body, "10.0.0.1:1", "info");
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.app_name.as_deref(), Some("adguard-rewrite"));
}

#[test]
fn adguard_malformed_json_passes_through() {
    let cfg = EnrichmentConfig::default();
    let e = entry("adguard-query", "{not json", "10.0.0.1:1", "info");
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.app_name.as_deref(), Some("adguard-query"));
}

#[test]
fn adguard_source_ip_gating_blocks_spoof() {
    let cfg = EnrichmentConfig {
        adguard_source_ip: Some("192.168.1.20".into()),
        ..Default::default()
    };
    let body = r#"{"Result":{"IsFiltered":true},"Upstream":""}"#;
    let e = entry("adguard-query", body, "10.0.0.1:1", "info");
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.app_name.as_deref(), Some("adguard-query"));
}

// ---- non-target apps unchanged ----

#[test]
fn non_authelia_non_adguard_passes_unchanged() {
    let cfg = EnrichmentConfig::default();
    let e = entry("nginx", "level=error this is nginx", "10.0.0.1:1", "info");
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.severity, "info");
    assert_eq!(out.app_name.as_deref(), Some("nginx"));
}

// ---- secret scrubbing ----

#[test]
fn scrub_aws_access_key() {
    let cfg = EnrichmentConfig {
        scrub_prompts: true,
        ..Default::default()
    };
    let e = entry(
        "claude-code",
        "Found AKIAIOSFODNN7EXAMPLE in the env file",
        "10.0.0.1:1",
        "info",
    );
    let out = enrich_entry(e, &cfg);
    assert!(out.message.contains("[REDACTED]"));
    assert!(!out.message.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn scrub_github_token() {
    let cfg = EnrichmentConfig {
        scrub_prompts: true,
        ..Default::default()
    };
    let e = entry(
        "claude-transcript",
        "use token ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa here",
        "10.0.0.1:1",
        "info",
    );
    let out = enrich_entry(e, &cfg);
    assert!(out.message.contains("[REDACTED]"));
    assert!(!out.message.contains("ghp_aaaa"));
}

#[test]
fn scrub_api_token_value() {
    let cfg = EnrichmentConfig {
        scrub_prompts: true,
        api_token: Some("super-secret-token-value-123".into()),
        ..Default::default()
    };
    let e = entry(
        "claude-code",
        "the token is super-secret-token-value-123 do not share",
        "10.0.0.1:1",
        "info",
    );
    let out = enrich_entry(e, &cfg);
    assert!(!out.message.contains("super-secret-token-value-123"));
    assert!(out.message.contains("[REDACTED]"));
}

#[test]
fn scrub_disabled_leaves_message_untouched() {
    let cfg = EnrichmentConfig {
        scrub_prompts: false,
        ..Default::default()
    };
    let e = entry(
        "claude-code",
        "AKIAIOSFODNN7EXAMPLE in plain text",
        "10.0.0.1:1",
        "info",
    );
    let out = enrich_entry(e, &cfg);
    assert!(out.message.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn scrub_skips_non_ai_source() {
    let cfg = EnrichmentConfig {
        scrub_prompts: true,
        ..Default::default()
    };
    let e = entry(
        "nginx",
        "AKIAIOSFODNN7EXAMPLE in nginx log",
        "10.0.0.1:1",
        "info",
    );
    let out = enrich_entry(e, &cfg);
    // nginx is not in AI_SOURCES, scrubber doesn't touch it
    assert!(out.message.contains("AKIAIOSFODNN7EXAMPLE"));
}

// ---- AI transcript enrichment ----

#[test]
fn enriches_codex_session_meta_with_project_and_session() {
    let cfg = EnrichmentConfig::default();
    let e = entry(
        "codex-transcript",
        r#"{"timestamp":"2026-05-11T03:16:18.603Z","type":"session_meta","payload":{"id":"019e1506-dc81-7881-9926-4d6d4efda1ac","cwd":"/home/jmagar/workspace/mem0"}}"#,
        "10.0.0.1:1",
        "info",
    );
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.ai_tool.as_deref(), Some("codex"));
    assert_eq!(
        out.ai_project.as_deref(),
        Some("/home/jmagar/workspace/mem0")
    );
    assert_eq!(
        out.ai_session_id.as_deref(),
        Some("019e1506-dc81-7881-9926-4d6d4efda1ac")
    );
}

#[test]
fn enriches_codex_function_call_workdir_when_session_meta_absent() {
    let cfg = EnrichmentConfig::default();
    let e = entry(
        "codex-transcript",
        r#"{"type":"response_item","payload":{"type":"function_call","arguments":"{\"cmd\":\"cargo test\",\"workdir\":\"/home/jmagar/code/swag-mcp\"}"}}"#,
        "10.0.0.1:1",
        "info",
    );
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.ai_tool.as_deref(), Some("codex"));
    assert_eq!(
        out.ai_project.as_deref(),
        Some("/home/jmagar/code/swag-mcp")
    );
}

#[test]
fn enriches_codex_workdir_normalizes_project_local_worktree() {
    let cfg = EnrichmentConfig::default();
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("workspace/cortex");
    let worktree = project.join(".worktrees/session-indexing");
    let e = entry(
        "codex-transcript",
        &format!(
            "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"function_call\",\"arguments\":\"{{\\\"cmd\\\":\\\"cargo test\\\",\\\"workdir\\\":\\\"{}\\\"}}\"}}}}",
            worktree.display()
        ),
        "10.0.0.1:1",
        "info",
    );
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.ai_tool.as_deref(), Some("codex"));
    assert_eq!(out.ai_project.as_deref(), Some(project.to_str().unwrap()));
}

#[test]
fn enriches_claude_project_from_transcript_path_in_raw() {
    let cfg = EnrichmentConfig::default();
    let mut e = entry(
        "claude-transcript",
        r#"{"sessionId":"3a8bdaf9-721c-4e0b-8a6b-cffe2740c8d5","cwd":"/home/jmagar/workspace/cortex"}"#,
        "10.0.0.1:1",
        "info",
    );
    e.raw = r#"<165>1 2026-05-11T00:00:00Z dookie claude-transcript - - [origin file="/home/jmagar/.claude/projects/-home-jmagar-workspace-cortex/3a8bdaf9-721c-4e0b-8a6b-cffe2740c8d5.jsonl"] {"sessionId":"3a8bdaf9-721c-4e0b-8a6b-cffe2740c8d5"}"#.to_string();
    let out = enrich_entry(e, &cfg);
    assert_eq!(out.ai_tool.as_deref(), Some("claude"));
    assert_eq!(
        out.ai_project.as_deref(),
        Some("/home/jmagar/workspace/cortex")
    );
    assert_eq!(
        out.ai_session_id.as_deref(),
        Some("3a8bdaf9-721c-4e0b-8a6b-cffe2740c8d5")
    );
    assert_eq!(
        out.ai_transcript_path.as_deref(),
        Some(
            "/home/jmagar/.claude/projects/-home-jmagar-workspace-cortex/3a8bdaf9-721c-4e0b-8a6b-cffe2740c8d5.jsonl"
        )
    );
}

#[test]
fn project_from_transcript_path_prefers_sessions_index_original_path() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("cortex");
    fs::create_dir_all(&project).unwrap();

    let transcript_dir = tmp.path().join(".claude/projects/-tmp-cortex");
    fs::create_dir_all(&transcript_dir).unwrap();
    fs::write(
        transcript_dir.join("sessions-index.json"),
        format!(
            "{{\"version\":1,\"entries\":[],\"originalPath\":\"{}\"}}",
            project.display()
        ),
    )
    .unwrap();

    let transcript = transcript_dir.join("session-123.jsonl");
    assert_eq!(
        project_from_transcript_path(transcript.to_str().unwrap()).as_deref(),
        Some(project.to_str().unwrap())
    );
}
