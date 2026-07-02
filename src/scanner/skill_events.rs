//! Skill-event extraction from AI transcript records.
//!
//! Two independent extractors feed the same [`ExtractedSkillEvent`] shape:
//! - Claude: structured `attributionSkill` / `attributionPlugin` JSON fields
//!   (top-level or `message.*` nesting — a third `payload.*` candidate was
//!   deliberately NOT added: no observed transcript sample confirms that
//!   shape, so it would be speculative).
//! - Codex: `<skill><name>...</name></skill>` tags embedded in transcript
//!   message text (see `CODEX_SKILL_TAG` in this module).
//!
//! Both extractors short-circuit on a cheap substring check before doing any
//! real parsing/regex work (eng review Fix 1), so the common no-skill-event
//! case costs a single `str::contains` call.
//!
//! Callers normalize with [`ExtractedSkillEvent::normalized`] before
//! inserting, which trims/clamps/derives the `plugin:skill` combined form
//! and rejects control characters (eng review Fix 8 — an adversarial
//! transcript could otherwise embed ANSI escapes that the CLI printer
//! would echo verbatim via `println!`).

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
    pub event_kind: SkillEventKind,
    pub evidence_kind: SkillEvidenceKind,
}

impl ExtractedSkillEvent {
    /// Trim, reject-if-empty, reject-if-contains-control-characters, and
    /// clamp `skill_name`/`skill_plugin` to `MAX_SKILL_FIELD_CHARS`. Returns
    /// `None` when the resulting skill_name would be empty OR contains any
    /// `char::is_control()` character (eng review Fix 8 — closes a terminal
    /// output spoofing vector: ANSI escapes or embedded newlines/CRs in a
    /// skill name would otherwise be echoed verbatim by the CLI's
    /// `println!`-based printer). Never panics or bubbles an error —
    /// callers skip the event and keep parsing the rest of the transcript.
    fn normalized(mut self) -> Option<Self> {
        let trimmed_name = self.skill_name.trim();
        if trimmed_name.is_empty() || trimmed_name.chars().any(char::is_control) {
            return None;
        }
        if self
            .skill_plugin
            .as_deref()
            .is_some_and(|plugin| plugin.chars().any(char::is_control))
        {
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
/// Checks top-level and `message.*` nesting for `attributionSkill` /
/// `attributionPlugin` string fields (Claude transcripts use flat top-level
/// fields on user-facing records and nested `message.*` fields on some
/// tool-result records). Returns one event per candidate location that has a
/// non-empty `attributionSkill`; at most one event in practice since a single
/// transcript line only has one of the two shapes.
///
/// Eng review Fix 1: callers should already have skipped calling this
/// function at all when the source text doesn't contain `"attributionSkill"`
/// as a substring — this function itself has nothing further to
/// short-circuit on since it operates on an already-parsed `Value`, not raw
/// text.
pub fn extract_claude_skill_events(value: &serde_json::Value) -> Vec<ExtractedSkillEvent> {
    let candidates = [
        value,
        value.get("message").unwrap_or(&serde_json::Value::Null),
    ];
    for candidate in candidates {
        let Some(skill) = candidate
            .get("attributionSkill")
            .and_then(serde_json::Value::as_str)
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
            event_kind: SkillEventKind::ClaudeAttribution,
            evidence_kind: SkillEvidenceKind::StructuredJsonField,
        };
        if let Some(normalized) = event.normalized() {
            return vec![normalized];
        }
        return Vec::new();
    }
    Vec::new()
}

#[cfg(test)]
#[path = "skill_events_tests.rs"]
mod tests;
