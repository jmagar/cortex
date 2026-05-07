//! Pre-insert enrichment for syslog batch entries.
//!
//! Two responsibilities:
//! 1. **Source-aware reclassification** — parse Authelia `level=` into a real
//!    syslog severity, classify AdGuard query results into
//!    `adguard-blocked` / `adguard-allowed` / `adguard-rewrite`.
//! 2. **Best-effort secret scrubbing** for AI-source records (claude/codex
//!    transcripts and OTLP records carrying their service.name) so accidental
//!    credential pastes don't end up FTS5-indexed.
//!
//! The scrubber is **defense-in-depth, not a compliance control** — regex has
//! structural bypass classes (multi-line wrapping, encoding obfuscation,
//! token formats not in the pattern list). Set `SYSLOG_MCP_SCRUB_PROMPTS=false`
//! to disable.
//!
//! All regex patterns are compiled exactly once via `LazyLock` because
//! enrichment runs on the batch-writer hot path: per-record `Regex::new`
//! costs 10–50 ms each and would saturate the writer at any non-trivial
//! volume.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use crate::db::LogBatchEntry;

/// Configuration for the enrichment pipeline. Built from environment variables
/// at runtime startup; cloned into the batch writer.
#[derive(Debug, Clone, Default)]
pub struct EnrichmentConfig {
    /// If `Some`, only apply Authelia enrichment when the entry's `source_ip`
    /// starts with this prefix. If `None`, apply to any entry whose
    /// `app_name` matches.
    pub authelia_source_ip: Option<String>,
    /// Same gating, for AdGuard.
    pub adguard_source_ip: Option<String>,
    /// When `true`, redact known secret patterns from AI-source messages.
    pub scrub_prompts: bool,
    /// Optional API token value to add to the redaction set so a leaked token
    /// in tool output is scrubbed before FTS5 indexes it.
    pub api_token: Option<String>,
}

// --- compiled once at module load ----------------------------------------

/// Captures `level=...` from Authelia structured log lines.
static AUTHELIA_LEVEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\blevel=([A-Za-z]+)").expect("static regex"));

/// Patterns scrubbed from AI-source message bodies. Each matches the entire
/// secret token; the matched text is replaced with `[REDACTED]`.
static SECRET_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    let raw = [
        // AWS access keys: AKIA, ASIA (STS), AGPA (group), AROA (role), AIDA (user)
        r"\b(?:AKIA|ASIA|AGPA|AROA|AIDA)[0-9A-Z]{16}\b",
        // GitHub tokens: gh[pousr]_<base62>
        r"\bgh[pousr]_[A-Za-z0-9]{36,}\b",
        // GitHub fine-grained PATs
        r"\bgithub_pat_[A-Za-z0-9_]{20,}\b",
        // Anthropic
        r"\bsk-ant-api03-[A-Za-z0-9_\-]{20,}\b",
        // OpenAI project keys
        r"\bsk-proj-[A-Za-z0-9_\-]{20,}\b",
        // Standalone JWTs (3 base64url segments separated by `.`)
        r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\b",
        // Bearer tokens in Authorization headers (broad token charset incl. base64 +/=)
        r"(?i)Authorization:\s*Bearer\s+[A-Za-z0-9._+/=\-]+",
        // password=value / api_key=value / secret=value (handles quoted values)
        r#"(?i)\b(?:password|api[_-]?key|secret)\s*[:=]\s*"?[^\s,;\}\]"]+"?"#,
        // PEM private key block — match the WHOLE block including key body. (?s)
        // makes `.` cross newlines; lazy `.+?` stops at the first END marker.
        r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.+?-----END [A-Z ]*PRIVATE KEY-----",
    ];
    raw.into_iter()
        .map(|p| Regex::new(p).expect("static secret pattern"))
        .collect()
});

/// AI-source `app_name` values whose message bodies are eligible for scrubbing.
const AI_SOURCES: &[&str] = &[
    "claude-transcript",
    "codex-transcript",
    "claude-code",
    "codex",
];

/// Minimal AdGuard query log row. We deliberately use named fields with
/// `default` so unknown JSON keys are dropped without allocation.
#[derive(Debug, Default, Deserialize)]
struct AdGuardQuery {
    #[serde(rename = "Result", default)]
    result: AdGuardResult,
    #[serde(rename = "Upstream", default)]
    upstream: String,
}

#[derive(Debug, Default, Deserialize)]
struct AdGuardResult {
    #[serde(rename = "IsFiltered", default)]
    is_filtered: bool,
    #[serde(rename = "Reason", default)]
    reason: String,
}

/// Apply enrichment to one entry. Pure function — never panics, never logs at
/// `error`. Parse failures fall through, leaving the entry unchanged.
pub(crate) fn enrich_entry(mut entry: LogBatchEntry, config: &EnrichmentConfig) -> LogBatchEntry {
    if matches_app(&entry, "authelia")
        && source_ip_matches(&entry, config.authelia_source_ip.as_deref())
    {
        if let Some(level_severity) = extract_authelia_level(&entry.message) {
            entry.severity = level_severity.to_string();
        }
    }

    if matches_app(&entry, "adguard-query")
        && source_ip_matches(&entry, config.adguard_source_ip.as_deref())
    {
        if let Some(new_app) = classify_adguard(&entry.message) {
            entry.app_name = Some(new_app.to_string());
        }
    }

    if config.scrub_prompts && entry.app_name.as_deref().is_some_and(is_ai_source) {
        entry.message = scrub_secrets(&entry.message, config.api_token.as_deref());
    }

    entry
}

fn matches_app(entry: &LogBatchEntry, expected: &str) -> bool {
    entry.app_name.as_deref() == Some(expected)
}

fn source_ip_matches(entry: &LogBatchEntry, configured_prefix: Option<&str>) -> bool {
    match configured_prefix {
        Some(prefix) if !prefix.is_empty() => entry.source_ip.starts_with(prefix),
        // Unset: apply to all matching app_name. Less safe but simpler default.
        _ => true,
    }
}

fn extract_authelia_level(message: &str) -> Option<&'static str> {
    let cap = AUTHELIA_LEVEL.captures(message)?;
    let level = cap.get(1)?.as_str();
    Some(match level.to_ascii_lowercase().as_str() {
        "trace" | "debug" => "debug",
        "info" => "info",
        "warn" | "warning" => "warning",
        "error" => "err",
        "fatal" | "panic" => "crit",
        _ => return None,
    })
}

fn classify_adguard(message: &str) -> Option<&'static str> {
    let parsed: AdGuardQuery = serde_json::from_str(message).ok()?;
    if parsed.result.reason.contains("Rewrite") {
        return Some("adguard-rewrite");
    }
    if parsed.result.is_filtered {
        return Some("adguard-blocked");
    }
    if !parsed.upstream.is_empty() {
        return Some("adguard-allowed");
    }
    None
}

fn is_ai_source(app_name: &str) -> bool {
    AI_SOURCES.contains(&app_name)
}

/// Replace any token matching the secret pattern set with `[REDACTED]`. The
/// raw API token (if configured) is appended as a literal pattern so a leaked
/// copy in tool output is scrubbed before storage.
///
/// Common case (no match) returns `Cow::Borrowed` and allocates nothing —
/// hot-path optimization for AI bursts where most messages have no secrets.
fn scrub_secrets(message: &str, api_token: Option<&str>) -> String {
    let mut out: Cow<str> = Cow::Borrowed(message);
    for re in SECRET_PATTERNS.iter() {
        if let Cow::Owned(replaced) = re.replace_all(&out, "[REDACTED]") {
            out = Cow::Owned(replaced);
        }
    }
    if let Some(token) = api_token {
        if !token.is_empty() && out.contains(token) {
            out = Cow::Owned(out.replace(token, "[REDACTED]"));
        }
    }
    out.into_owned()
}

#[cfg(test)]
#[path = "enrichment_tests.rs"]
mod tests;
