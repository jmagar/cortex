//! Deterministic failure-hypothesis and prevention-hint generation over
//! skill-usage incident evidence bundles. Pure rule evaluation — never
//! queries the database and never calls an external LLM. Mirrors
//! `src/app/incident_findings.rs` (the abuse-incident findings module) but
//! targets skill-specific failure categories.

use serde::{Deserialize, Serialize};

use super::models::{LogEntry, SkillIncident};

// ── Stable failure-mode categories ──────────────────────────────────────────
pub const SKILL_SCOPE_MISMATCH: &str = "skill_scope_mismatch";
pub const MISSING_PREREQUISITE_CHECK: &str = "missing_prerequisite_check";
pub const WRONG_SOURCE_OF_TRUTH: &str = "wrong_source_of_truth";
pub const OVERLY_BROAD_RESEARCH_LOOP: &str = "overly_broad_research_loop";
pub const TOOL_POLICY_MISMATCH: &str = "tool_policy_mismatch";
pub const MISSING_VERIFICATION_STEP: &str = "missing_verification_step";
pub const AMBIGUOUS_SKILL_TRIGGER: &str = "ambiguous_skill_trigger";
pub const STALE_OR_CONFLICTING_SKILL_INSTRUCTION: &str = "stale_or_conflicting_skill_instruction";
pub const ASSISTANT_OVEREXPLAINED_SIMPLE_ANSWER: &str = "assistant_overexplained_simple_answer";
pub const UNKNOWN: &str = "unknown";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillFailureMode {
    pub category: String,
    pub confidence: String,
    pub evidence_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillContributingFactor {
    pub factor: String,
    pub evidence_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillPreventionHint {
    pub category: String,
    pub hint: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillIncidentFindings {
    pub likely_failure_modes: Vec<SkillFailureMode>,
    pub contributing_factors: Vec<SkillContributingFactor>,
    pub prevention_hints: Vec<SkillPreventionHint>,
    pub open_questions: Vec<String>,
}

/// `(category, keyword substrings, prevention hint)`. Kept specific — no
/// broad single tokens.
type Rule = (&'static str, &'static [&'static str], &'static str);

const RULES: &[Rule] = &[
    (
        WRONG_SOURCE_OF_TRUTH,
        &[
            "wrong source of truth",
            "wrong source",
            "stale data",
            "memory-vs-live",
            "memory vs live",
            "not the live",
        ],
        "Add a note to the skill doc naming the canonical source of truth for this data \
         (live system vs. memory/cache) and require the agent to confirm which one it used.",
    ),
    (
        WRONG_SOURCE_OF_TRUTH,
        &["wrong repo"],
        "Add a trigger-boundary note clarifying which repo this skill applies to, and require \
         the agent to confirm the working directory before acting.",
    ),
    (
        MISSING_VERIFICATION_STEP,
        &[
            "without any verification",
            "without verification",
            "claimed success without",
        ],
        "Add a verification checklist item requiring live repo/runtime evidence before claiming \
         success.",
    ),
    (
        MISSING_PREREQUISITE_CHECK,
        &["should have created a bead", "should have created an issue"],
        "Add a prerequisite-check step to the skill doc: confirm an issue/bead exists (or create \
         one) before starting non-trivial work.",
    ),
    (
        TOOL_POLICY_MISMATCH,
        &[
            "wrong transport",
            "wrong source for this call",
            "raw web instead of using axon",
            "instead of using axon",
        ],
        "Add an explicit tool-policy line to the skill doc naming the required transport/source \
         (e.g. Axon before raw web search) and why.",
    ),
    (
        OVERLY_BROAD_RESEARCH_LOOP,
        &["going in circles", "we wasted", "all you had to say"],
        "Add an anti-loop rule: after two failed searches, summarize current evidence and switch \
         strategy instead of repeating the same approach.",
    ),
    (
        AMBIGUOUS_SKILL_TRIGGER,
        &[
            "wrong skill",
            "not the right skill",
            "shouldn't have triggered",
            "should not have triggered",
        ],
        "Add a trigger-boundary note that this skill is for implementation planning only (or \
         narrow its stated trigger phrases) so it stops firing on out-of-scope requests.",
    ),
    (
        STALE_OR_CONFLICTING_SKILL_INSTRUCTION,
        &[
            "stale instruction",
            "conflicting instruction",
            "outdated skill",
            "skill doc is wrong",
            "skill doc is out of date",
        ],
        "Review and update the skill doc section that conflicts with current project conventions.",
    ),
    (
        ASSISTANT_OVEREXPLAINED_SIMPLE_ANSWER,
        &[
            "all you had to say was",
            "didn't need to touch",
            "did not need to touch",
            "you didn't need to",
        ],
        "Add a conciseness note to the skill doc: for simple factual questions, answer directly \
         before taking any action.",
    ),
    (
        SKILL_SCOPE_MISMATCH,
        &["out of scope", "not what this skill is for"],
        "Narrow the skill's stated scope in its description/trigger phrases to exclude this case.",
    ),
];

fn confidence_for(count: usize) -> &'static str {
    match count {
        0 | 1 => "low",
        2 => "medium",
        _ => "high",
    }
}

const SKILL_EVENT_FACTOR_THRESHOLD: usize = 3;
const ERROR_BURST_THRESHOLD: usize = 3;

fn scannable<'a>(
    signal_anchors: &'a [LogEntry],
    transcript_before: &'a [LogEntry],
    transcript_after: &'a [LogEntry],
    nearby_logs: &'a [LogEntry],
    nearby_errors: &'a [LogEntry],
) -> impl Iterator<Item = &'a LogEntry> {
    signal_anchors
        .iter()
        .chain(transcript_before)
        .chain(transcript_after)
        .chain(nearby_logs)
        .chain(nearby_errors)
}

/// Derive deterministic findings from a skill-incident evidence bundle. Pure
/// and total: identical input always yields identical output; every
/// non-`unknown` failure mode / contributing factor cites at least one
/// evidence id; weak evidence yields `unknown` + `open_questions` rather than
/// an unsupported claim.
pub fn derive_skill_incident_findings(
    incident: &SkillIncident,
    _skill_events: &[crate::db::AiSkillEventEntry],
    signal_anchors: &[LogEntry],
    transcript_before: &[LogEntry],
    transcript_after: &[LogEntry],
    nearby_logs: &[LogEntry],
    nearby_errors: &[LogEntry],
) -> SkillIncidentFindings {
    let mut findings = SkillIncidentFindings::default();

    for (category, keywords, hint) in RULES {
        let mut ids: Vec<i64> = Vec::new();
        for entry in scannable(
            signal_anchors,
            transcript_before,
            transcript_after,
            nearby_logs,
            nearby_errors,
        ) {
            let haystack = entry.message.to_ascii_lowercase();
            if keywords.iter().any(|kw| haystack.contains(kw)) {
                ids.push(entry.id);
            }
        }
        ids.sort_unstable();
        ids.dedup();
        if !ids.is_empty() {
            let confidence = confidence_for(ids.len()).to_owned();
            findings.likely_failure_modes.push(SkillFailureMode {
                category: (*category).to_owned(),
                confidence,
                evidence_ids: ids,
            });
            findings.prevention_hints.push(SkillPreventionHint {
                category: (*category).to_owned(),
                hint: (*hint).to_owned(),
            });
        }
    }

    if incident.skill_event_count >= SKILL_EVENT_FACTOR_THRESHOLD && !signal_anchors.is_empty() {
        findings.contributing_factors.push(SkillContributingFactor {
            factor: format!(
                "Repeated skill invocation: {} skill events within the incident window.",
                incident.skill_event_count
            ),
            evidence_ids: signal_anchors.iter().map(|a| a.id).collect(),
        });
    }
    if nearby_errors.len() >= ERROR_BURST_THRESHOLD {
        findings.contributing_factors.push(SkillContributingFactor {
            factor: format!(
                "Error burst: {} error-level logs in the correlation window.",
                nearby_errors.len()
            ),
            evidence_ids: nearby_errors.iter().map(|e| e.id).collect(),
        });
    }

    if findings.likely_failure_modes.is_empty() {
        findings.likely_failure_modes.push(SkillFailureMode {
            category: UNKNOWN.to_owned(),
            confidence: "low".to_owned(),
            evidence_ids: Vec::new(),
        });
        findings.open_questions.push(
            "No deterministic failure signature matched the evidence window; manual transcript \
             review is recommended."
                .to_owned(),
        );
    }
    if signal_anchors.is_empty() {
        findings
            .open_questions
            .push("No negative signal anchors were captured for this incident.".to_owned());
    }
    if nearby_logs.is_empty() && nearby_errors.is_empty() {
        findings.open_questions.push(
            "No surrounding non-AI logs were available to corroborate the transcript signal."
                .to_owned(),
        );
    }

    findings
}

#[cfg(test)]
#[path = "skill_incident_findings_tests.rs"]
mod tests;
