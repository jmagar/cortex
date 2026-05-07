//! Tests for the enrichment pipeline.

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
