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
//! token formats not in the pattern list). Set `CORTEX_SCRUB_PROMPTS=false`
//! to disable.
//!
//! All regex patterns are compiled exactly once via `LazyLock` because
//! enrichment runs on the batch-writer hot path: per-record `Regex::new`
//! costs 10–50 ms each and would saturate the writer at any non-trivial
//! volume.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::{
    fs,
    path::{Path, PathBuf},
};

use regex::Regex;
use serde::Deserialize;

use crate::ai_project::normalize_ai_project_path;
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

/// Capture the file="..." metadata from rsyslog imfile records.
static IMFILE_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("file=\"([^\"]+\\.(?:jsonl|json))\"").expect("static regex"));

static CLAUDE_PROJECT_INDEX_CACHE: LazyLock<Mutex<HashMap<PathBuf, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
    "gemini-transcript",
    "claude-code",
    "codex",
    "ai-transcript",
];

/// Minimal AdGuard query log row. AdGuard emits PascalCase JSON keys; the
/// container-level `rename_all` keeps Rust field names idiomatic snake_case.
/// `default` per field tolerates partial / future-incompatible payloads.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AdGuardQuery {
    #[serde(default)]
    result: AdGuardResult,
    #[serde(default)]
    upstream: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AdGuardResult {
    #[serde(default)]
    is_filtered: bool,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeSessionsIndex {
    #[serde(default)]
    original_path: Option<String>,
    #[serde(default)]
    entries: Vec<ClaudeSessionsIndexEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeSessionsIndexEntry {
    #[serde(default)]
    project_path: Option<String>,
}

/// Apply enrichment to one entry. Pure function — never panics, never logs at
/// `error`. Parse failures fall through, leaving the entry unchanged.
pub(crate) fn enrich_entry(mut entry: LogBatchEntry, config: &EnrichmentConfig) -> LogBatchEntry {
    enrich_ai_metadata(&mut entry);

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

/// Match `entry.source_ip` against an operator-configured prefix at the
/// IP-octet boundary. Plain `starts_with` would let an attacker on
/// `10.0.0.10` (or `10.0.0.123`) pass a gate configured for `10.0.0.1`
/// because `"10.0.0.10:1234".starts_with("10.0.0.1")` is true. Two cases:
///
/// * **Subnet prefix** ending with `.` (e.g. `"10.0.0."`): match any IP
///   in the subnet — the byte after the prefix must be a digit (next octet).
/// * **Exact host** without trailing dot (e.g. `"10.0.0.5"`): match only
///   that IP — extract the IP portion of `"<ip>:<port>"` and require equality.
///
/// `None` or empty prefix preserves the legacy "apply to all matching
/// app_name" default.
fn source_ip_matches(entry: &LogBatchEntry, configured_prefix: Option<&str>) -> bool {
    let Some(prefix) = configured_prefix.filter(|p| !p.is_empty()) else {
        return true;
    };
    let ip_only = entry.source_ip.split(':').next().unwrap_or("");
    if prefix.ends_with('.') {
        // Subnet match: prefix is a partial dotted-quad like "10.0.0."
        ip_only.starts_with(prefix)
    } else {
        // Exact-host match: prefix is a full IP literal
        ip_only == prefix
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

fn enrich_ai_metadata(entry: &mut LogBatchEntry) {
    let Some(app_name) = entry.app_name.as_deref() else {
        return;
    };
    let Some(tool) = ai_tool_from_app(app_name) else {
        return;
    };
    entry.ai_tool = Some(tool.to_string());

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&entry.message) {
        fill_ai_metadata_from_json(entry, &value);
    }

    if let Some(path) = extract_imfile_path(&entry.raw) {
        entry.ai_transcript_path = Some(path.clone());
        if entry.ai_project.is_none() {
            entry.ai_project = project_from_transcript_path(&path);
        }
        if entry.ai_session_id.is_none() {
            entry.ai_session_id = session_id_from_path(&path);
        }
    }
}

fn ai_tool_from_app(app_name: &str) -> Option<&'static str> {
    match app_name {
        "claude-transcript" | "claude-code" => Some("claude"),
        "codex-transcript" | "codex" => Some("codex"),
        "gemini-transcript" => Some("gemini"),
        _ => None,
    }
}

fn extract_imfile_path(raw: &str) -> Option<String> {
    IMFILE_PATH
        .captures(raw)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

fn session_id_from_path(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string)
}

pub(crate) fn project_from_transcript_path(path: &str) -> Option<String> {
    if let Some(project) = project_from_sessions_index(path) {
        return Some(project);
    }
    if let Some(project_part) = path.split("/.claude/projects/").nth(1) {
        let encoded = project_part.split('/').next()?;
        return decode_claude_project(encoded).map(|project| normalize_ai_project_path(&project));
    }
    None
}

/// Decode a Claude project directory name back to a path.
///
/// Claude encodes project paths by replacing `/` with `-` and prefixing with `-`.
/// Example: `/home/user/code` -> `-home-user-code`.
///
/// This decoder is best-effort and lossy: it cannot distinguish between an
/// encoded `/` and a literal `-` in a directory name (e.g. `cortex` vs
/// `syslog/mcp`).
fn decode_claude_project(encoded: &str) -> Option<String> {
    let stripped = encoded.strip_prefix('-').unwrap_or(encoded);
    if stripped.is_empty() {
        return None;
    }
    Some(format!("/{}", stripped.replace('-', "/")))
}

fn project_from_sessions_index(path: &str) -> Option<String> {
    let index_path = Path::new(path).parent()?.join("sessions-index.json");
    let mut cache = CLAUDE_PROJECT_INDEX_CACHE.lock().ok()?;
    if let Some(project) = cache.get(&index_path) {
        return project.clone();
    }

    let project = fs::read_to_string(&index_path)
        .ok()
        .and_then(|body| serde_json::from_str::<ClaudeSessionsIndex>(&body).ok())
        .and_then(|index| {
            index.original_path.or_else(|| {
                index
                    .entries
                    .into_iter()
                    .find_map(|entry| entry.project_path)
            })
        })
        .map(|project| normalize_ai_project_path(&project));

    cache.insert(index_path, project.clone());
    project
}

fn fill_ai_metadata_from_json(entry: &mut LogBatchEntry, value: &serde_json::Value) {
    let payload = value.get("payload").unwrap_or(value);
    if entry.ai_session_id.is_none() {
        entry.ai_session_id = payload
            .get("id")
            .or_else(|| value.get("sessionId"))
            .or_else(|| value.get("session_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
    }
    if entry.ai_project.is_none() {
        entry.ai_project = payload
            .get("cwd")
            .or_else(|| value.get("cwd"))
            .and_then(serde_json::Value::as_str)
            .map(normalize_ai_project_path);
    }
    if entry.ai_project.is_none() {
        entry.ai_project = payload
            .get("arguments")
            .and_then(serde_json::Value::as_str)
            .and_then(|args| serde_json::from_str::<serde_json::Value>(args).ok())
            .and_then(|args| {
                args.get("workdir")
                    .and_then(serde_json::Value::as_str)
                    .map(normalize_ai_project_path)
            });
    }
}

/// Replace any token matching the secret pattern set with `[REDACTED]`. The
/// raw API token (if configured) is appended as a literal pattern so a leaked
/// copy in tool output is scrubbed before storage.
///
/// Common case (no match) returns `Cow::Borrowed` and allocates nothing —
/// hot-path optimization for AI bursts where most messages have no secrets.
pub(crate) fn scrub_ai_message(message: &str, api_token: Option<&str>) -> String {
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

fn scrub_secrets(message: &str, api_token: Option<&str>) -> String {
    scrub_ai_message(message, api_token)
}

#[cfg(test)]
#[path = "enrichment_tests.rs"]
mod tests;
