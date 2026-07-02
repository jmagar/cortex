//! Hook-event extraction from AI transcript records.
//!
//! Claude transcripts carry first-class hook *runtime execution* evidence:
//! attachment rows whose `attachment.type` begins with `hook_` (e.g.
//! `hook_success`), carrying `hookName`, `hookEvent`, `command`, `exitCode`,
//! `durationMs`, `stdout`, `stderr`, `content`, and an optional persisted
//! output path pointer when the captured output is too large to inline.
//!
//! Codex transcripts (per the original research for GH #105) did NOT show an
//! equivalent structured runtime hook attachment shape in sampled logs, so
//! this module intentionally ships **no Codex runtime-hook parser**. Codex
//! hook evidence is config/trust-state only and is produced by the separate
//! config-inventory collectors (see `crate::scanner::hook_config`), never here.
//!
//! Every extracted event carries an `evidence_kind` so downstream callers can
//! distinguish runtime-proven facts (`runtime_transcript`) from
//! configuration-inventory facts (`config_inventory`, `trusted_hash_state`).
//! This module only ever produces `runtime_transcript` evidence.
//!
//! The extractor short-circuits on a cheap substring check before doing any
//! real JSON walking, mirroring the skill-event extractor: the common
//! no-hook-attachment case costs a single `str::contains` call.
//!
//! Bounded preview fields (`stdout_preview`/`stderr_preview`) are redacted via
//! [`crate::assessment::redact_secrets`] and clamped to
//! [`MAX_HOOK_PREVIEW_CHARS`] before they leave this module — an adversarial
//! transcript could otherwise embed ANSI escapes or secrets that the CLI
//! printer would echo verbatim.

use crate::assessment::redact_secrets;

/// Char cap for every bounded free-text field on an extracted hook event.
/// Mirrors the `MAX_SKILL_FIELD_CHARS` convention from `skill_events.rs`; the
/// preview fields get a larger budget because stdout/stderr are inherently
/// longer than a skill name while still needing a hard bound before storage.
const MAX_HOOK_FIELD_CHARS: usize = 256;
const MAX_HOOK_PREVIEW_CHARS: usize = 2048;
const MAX_HOOK_COMMAND_CHARS: usize = 1024;

/// Runtime status of a hook execution, derived from the attachment `type`
/// suffix. Unknown/unrecognized `hook_*` variants map to [`HookStatus::Unknown`]
/// rather than panicking — we have no live sample of every failure variant, so
/// the parser is deliberately defensive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookStatus {
    Success,
    Failed,
    Blocked,
    Error,
    Unknown,
    /// Not a runtime status: marks a config-inventory / trusted-hash-state row
    /// produced by `crate::hook_config`. A `Configured` row is proof only that
    /// a hook is *configured*/*trusted*, never that it executed. Kept in this
    /// enum (rather than a bare string) so the DB `status` column has a single
    /// authoritative source and config rows are excluded from runtime failure
    /// anchors (`hook_signal_detectors::is_hook_failure_status`).
    Configured,
}

impl HookStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Error => "error",
            Self::Unknown => "unknown",
            Self::Configured => "configured",
        }
    }

    /// Map a Claude attachment `type` (e.g. `hook_success`, `hook_failure`,
    /// `hook_blocked`, `hook_error`) to a status. Best-effort: matches on the
    /// suffix after the `hook_` prefix, tolerating unseen variants.
    fn from_attachment_type(attachment_type: &str) -> Self {
        let suffix = attachment_type
            .strip_prefix("hook_")
            .unwrap_or(attachment_type);
        match suffix {
            "success" | "ok" | "completed" | "complete" => Self::Success,
            "failure" | "failed" | "fail" | "nonzero" => Self::Failed,
            "blocked" | "block" | "denied" | "deny" => Self::Blocked,
            "error" | "errored" | "timeout" | "timed_out" | "parse_error" => Self::Error,
            _ => Self::Unknown,
        }
    }
}

/// Provenance of a hook event. Only `RuntimeTranscript` is produced by this
/// module; the config/trust-state variants are produced by the config
/// inventory collectors. The `LogCorrelation`/`SideEffectInference` variants
/// are reserved conceptually per GH #105 — no extractor emits them yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvidenceKind {
    RuntimeTranscript,
    ConfigInventory,
    TrustedHashState,
    LogCorrelation,
    SideEffectInference,
}

impl HookEvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeTranscript => "runtime_transcript",
            Self::ConfigInventory => "config_inventory",
            Self::TrustedHashState => "trusted_hash_state",
            Self::LogCorrelation => "log_correlation",
            Self::SideEffectInference => "side_effect_inference",
        }
    }
}

/// A single extracted hook event, in the shape shared by the runtime-transcript
/// extractor and the config-inventory collectors. Callers thread the
/// already-known session/host/timestamp context around this struct (mirroring
/// `ExtractedSkillEvent`); this struct holds only the hook-specific fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedHookEvent {
    pub hook_event: String,
    pub hook_name: Option<String>,
    pub hook_source: Option<String>,
    pub hook_command: Option<String>,
    pub status: HookStatus,
    pub exit_code: Option<i64>,
    pub duration_ms: Option<i64>,
    pub stdout_preview: Option<String>,
    pub stderr_preview: Option<String>,
    pub persisted_output_path: Option<String>,
    pub trusted_hash: Option<String>,
    pub evidence_kind: HookEvidenceKind,
    pub metadata_json: Option<String>,
}

impl ExtractedHookEvent {
    /// Trim, reject-if-control-chars, and clamp every free-text field. Returns
    /// `None` when the resulting `hook_event` would be empty OR any retained
    /// text field contains a `char::is_control()` character (closes the same
    /// terminal-spoofing vector the skill extractor's `normalized()` closes).
    /// Preview fields are redacted here as a final guard even though the
    /// runtime extractor already redacts them — the config collectors reuse
    /// this path and must also be safe. Never panics.
    pub fn normalized(mut self) -> Option<Self> {
        let trimmed_event = self.hook_event.trim();
        if trimmed_event.is_empty() || trimmed_event.chars().any(char::is_control) {
            return None;
        }
        self.hook_event = clamp_chars(trimmed_event, MAX_HOOK_FIELD_CHARS);

        self.hook_name = normalize_field(self.hook_name, MAX_HOOK_FIELD_CHARS)?;
        self.hook_source = normalize_field(self.hook_source, MAX_HOOK_FIELD_CHARS)?;
        self.hook_command = normalize_field(self.hook_command, MAX_HOOK_COMMAND_CHARS)?;
        self.persisted_output_path =
            normalize_field(self.persisted_output_path, MAX_HOOK_FIELD_CHARS)?;
        self.trusted_hash = normalize_field(self.trusted_hash, MAX_HOOK_FIELD_CHARS)?;

        self.stdout_preview = normalize_preview(self.stdout_preview);
        self.stderr_preview = normalize_preview(self.stderr_preview);

        Some(self)
    }
}

/// Trim + clamp an optional short field. Rejects the whole event (returns the
/// outer `None` via `?`) when the field carries control characters; drops the
/// field to `None` when it trims to empty.
fn normalize_field(value: Option<String>, max_chars: usize) -> Option<Option<String>> {
    match value {
        None => Some(None),
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.chars().any(char::is_control) {
                return None;
            }
            if trimmed.is_empty() {
                Some(None)
            } else {
                Some(Some(clamp_chars(trimmed, max_chars)))
            }
        }
    }
}

/// Redact secrets, strip control characters, and clamp a preview blob. Unlike
/// [`normalize_field`], control characters here are stripped (not rejected) —
/// stdout/stderr legitimately contain newlines, so we sanitize them into
/// spaces rather than dropping the whole event.
fn normalize_preview(value: Option<String>) -> Option<String> {
    let raw = value?;
    let redacted = redact_secrets(&raw);
    let sanitized: String = redacted
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(clamp_chars(trimmed, MAX_HOOK_PREVIEW_CHARS))
    }
}

fn clamp_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        value.chars().take(max_chars).collect()
    }
}

/// Extract Claude hook *runtime* events from a raw transcript JSON value.
///
/// Claude transcript lines carry hook execution evidence in an `attachment`
/// object (or an `attachments` array of them) whose `type` begins with
/// `hook_`. This walks both the top-level record and the `message.*` nesting
/// (the same two candidate locations the skill extractor checks), collecting
/// one event per hook attachment found.
///
/// Callers should already have skipped this function when the source text
/// doesn't contain the `"hook_"` substring — this function operates on an
/// already-parsed `Value`, so it can only short-circuit on structural absence.
pub fn extract_claude_hook_events(value: &serde_json::Value) -> Vec<ExtractedHookEvent> {
    let mut events = Vec::new();
    let candidates = [
        value,
        value.get("message").unwrap_or(&serde_json::Value::Null),
    ];
    for candidate in candidates {
        // A single record may carry a single `attachment` object or an
        // `attachments` array; support both.
        if let Some(attachment) = candidate.get("attachment") {
            collect_hook_attachment(attachment, &mut events);
        }
        if let Some(array) = candidate.get("attachments").and_then(|v| v.as_array()) {
            for attachment in array {
                collect_hook_attachment(attachment, &mut events);
            }
        }
    }
    events
}

/// Parse a single attachment object into a hook event if its `type` marks it as
/// a hook attachment. Non-hook attachments are ignored.
fn collect_hook_attachment(attachment: &serde_json::Value, out: &mut Vec<ExtractedHookEvent>) {
    let Some(attachment_type) = attachment.get("type").and_then(serde_json::Value::as_str) else {
        return;
    };
    if !attachment_type.starts_with("hook_") {
        return;
    }

    let status = HookStatus::from_attachment_type(attachment_type);
    let hook_event = str_field(attachment, "hookEvent")
        .or_else(|| str_field(attachment, "hook_event"))
        // Fall back to the attachment type suffix so an event with a missing
        // hookEvent still lands somewhere meaningful rather than being dropped.
        .unwrap_or_else(|| {
            attachment_type
                .strip_prefix("hook_")
                .unwrap_or(attachment_type)
                .to_string()
        });

    let hook_name =
        str_field(attachment, "hookName").or_else(|| str_field(attachment, "hook_name"));
    let hook_command =
        str_field(attachment, "command").or_else(|| str_field(attachment, "hookCommand"));
    let exit_code =
        int_field(attachment, "exitCode").or_else(|| int_field(attachment, "exit_code"));
    let duration_ms =
        int_field(attachment, "durationMs").or_else(|| int_field(attachment, "duration_ms"));
    // `content` is a secondary source for stdout when `stdout` is absent.
    let stdout_preview = str_field(attachment, "stdout")
        .or_else(|| str_field(attachment, "content"))
        .map(|s| redact_secrets(&s));
    let stderr_preview = str_field(attachment, "stderr").map(|s| redact_secrets(&s));
    let persisted_output_path = str_field(attachment, "persistedOutputPath")
        .or_else(|| str_field(attachment, "persisted_output_path"))
        .or_else(|| str_field(attachment, "outputPath"));

    let event = ExtractedHookEvent {
        hook_event,
        hook_name,
        hook_source: None,
        hook_command,
        status,
        exit_code,
        duration_ms,
        stdout_preview,
        stderr_preview,
        persisted_output_path,
        trusted_hash: None,
        evidence_kind: HookEvidenceKind::RuntimeTranscript,
        metadata_json: None,
    };
    if let Some(normalized) = event.normalized() {
        out.push(normalized);
    }
}

fn str_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

fn int_field(value: &serde_json::Value, key: &str) -> Option<i64> {
    value.get(key).and_then(serde_json::Value::as_i64)
}

#[cfg(test)]
#[path = "hook_events_tests.rs"]
mod tests;
