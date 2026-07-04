//! Deterministic failure-hypothesis and prevention-hint generation over
//! hook-usage incident evidence bundles. Pure rule evaluation — never
//! queries the database and never calls an external LLM. Mirrors
//! `src/app/skill_incident_findings.rs` but targets hook-specific failure
//! categories from GH #105's "Suggested hook finding categories" list.
//!
//! CRITICAL: every finding function takes `has_runtime_evidence` from the
//! incident and must not claim a hook *executed* when the backing evidence
//! is config/trust-state only (`evidence_kind != "runtime_transcript"`). See
//! `evidence_kind_note` below — every findings bundle carries an explicit
//! statement of which evidence class backs it.

use serde::{Deserialize, Serialize};

use super::models::{HookIncident, LogEntry};

// ── Stable failure-mode categories (GH #105) ────────────────────────────────
pub const HOOK_FAILED: &str = "hook_failed";
pub const HOOK_TIMED_OUT: &str = "hook_timed_out";
pub const HOOK_NOT_INVOKED: &str = "hook_not_invoked";
pub const HOOK_INVOKED_TOO_OFTEN: &str = "hook_invoked_too_often";
pub const HOOK_WRONG_SCOPE: &str = "hook_wrong_scope";
pub const HOOK_OUTPUT_PARSE_ERROR: &str = "hook_output_parse_error";
pub const HOOK_POLICY_DRIFT: &str = "hook_policy_drift";
pub const HOOK_BLOCKED_AGENT_FLOW: &str = "hook_blocked_agent_flow";
pub const HOOK_MUTATED_UNEXPECTED_STATE: &str = "hook_mutated_unexpected_state";
pub const HOOK_CAUSED_TOOL_FAILURE: &str = "hook_caused_tool_failure";
pub const UNKNOWN: &str = "unknown";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookFailureMode {
    pub category: String,
    pub confidence: String,
    pub evidence_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookContributingFactor {
    pub factor: String,
    pub evidence_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookPreventionHint {
    pub category: String,
    pub hint: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookIncidentFindings {
    pub likely_failure_modes: Vec<HookFailureMode>,
    pub contributing_factors: Vec<HookContributingFactor>,
    pub prevention_hints: Vec<HookPreventionHint>,
    pub open_questions: Vec<String>,
    /// Explicit provenance statement — GH #105's acceptance criterion that
    /// `cortex assess hooks` "can explain whether it is using runtime hook
    /// execution evidence or only config/trust-state evidence."
    pub evidence_basis: String,
}

const RUNTIME_EVIDENCE_BASIS: &str = "This incident is backed by at least one runtime_transcript hook event \
     (a Claude transcript hook-execution attachment) — findings reflect \
     proven hook execution, not just configuration.";
const CONFIG_ONLY_EVIDENCE_BASIS: &str = "This incident is backed ONLY by config_inventory/trusted_hash_state \
     evidence (hook configuration/trust files, not a transcript-proven \
     execution). Do not treat these findings as proof the hook actually ran \
     — they describe what is configured/trusted, not what executed.";

pub fn evidence_basis_for(has_runtime_evidence: bool) -> String {
    if has_runtime_evidence {
        RUNTIME_EVIDENCE_BASIS.to_string()
    } else {
        CONFIG_ONLY_EVIDENCE_BASIS.to_string()
    }
}

fn confidence_for(count: usize) -> &'static str {
    match count {
        0 | 1 => "low",
        2 => "medium",
        _ => "high",
    }
}

const HOOK_EVENT_FACTOR_THRESHOLD: usize = 5;
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

/// Derive deterministic findings from a hook-incident evidence bundle. Pure
/// and total: identical input always yields identical output; every
/// non-`unknown` failure mode / contributing factor cites at least one
/// evidence id; weak evidence yields `unknown` + `open_questions` rather than
/// an unsupported claim.
#[allow(clippy::too_many_arguments)]
pub fn derive_hook_incident_findings(
    incident: &HookIncident,
    hook_events: &[crate::db::AiHookEventEntry],
    signal_anchors: &[LogEntry],
    transcript_before: &[LogEntry],
    transcript_after: &[LogEntry],
    nearby_tool_calls: &[LogEntry],
    nearby_logs: &[LogEntry],
    nearby_errors: &[LogEntry],
) -> HookIncidentFindings {
    let mut findings = HookIncidentFindings {
        evidence_basis: evidence_basis_for(incident.has_runtime_evidence),
        ..Default::default()
    };

    // ── Direct signal-count-derived failure modes (from the incident's own
    // deterministic signal counts, cited against the hook_events ids that
    // produced them since those rows ARE the evidence for these categories) ──
    let hook_event_ids: Vec<i64> = hook_events.iter().map(|e| e.id).collect();

    if incident.signal_counts.hook_failed > 0 {
        findings.likely_failure_modes.push(HookFailureMode {
            category: HOOK_FAILED.to_owned(),
            confidence: confidence_for(incident.signal_counts.hook_failed).to_owned(),
            evidence_ids: hook_event_ids.clone(),
        });
        findings.prevention_hints.push(HookPreventionHint {
            category: HOOK_FAILED.to_owned(),
            hint: "Review the hook command for the failure condition and add error handling or \
                   a guard clause so it exits 0 on expected inputs."
                .to_owned(),
        });
    }
    if incident.signal_counts.hook_timed_out > 0 {
        findings.likely_failure_modes.push(HookFailureMode {
            category: HOOK_TIMED_OUT.to_owned(),
            confidence: confidence_for(incident.signal_counts.hook_timed_out).to_owned(),
            evidence_ids: hook_event_ids.clone(),
        });
        findings.prevention_hints.push(HookPreventionHint {
            category: HOOK_TIMED_OUT.to_owned(),
            hint: "Add an explicit timeout to the hook command and move slow work to a \
                   background process so it does not block agent flow."
                .to_owned(),
        });
    }
    if incident.signal_counts.hook_output_parse_error > 0 {
        findings.likely_failure_modes.push(HookFailureMode {
            category: HOOK_OUTPUT_PARSE_ERROR.to_owned(),
            confidence: confidence_for(incident.signal_counts.hook_output_parse_error).to_owned(),
            evidence_ids: hook_event_ids.clone(),
        });
        findings.prevention_hints.push(HookPreventionHint {
            category: HOOK_OUTPUT_PARSE_ERROR.to_owned(),
            hint: "Validate the hook's stdout against its expected schema before returning it, \
                   and emit structured JSON only (no mixed log lines) on stdout."
                .to_owned(),
        });
    }
    if incident.signal_counts.hook_invoked_too_often > 0 {
        findings.likely_failure_modes.push(HookFailureMode {
            category: HOOK_INVOKED_TOO_OFTEN.to_owned(),
            confidence: confidence_for(incident.signal_counts.hook_invoked_too_often).to_owned(),
            evidence_ids: hook_event_ids.clone(),
        });
        findings.prevention_hints.push(HookPreventionHint {
            category: HOOK_INVOKED_TOO_OFTEN.to_owned(),
            hint: "Narrow the hook's matcher/trigger event so it fires only for the intended \
                   tool/event pattern instead of every turn."
                .to_owned(),
        });
    }
    if incident.signal_counts.user_correction_after_hook > 0 {
        findings.likely_failure_modes.push(HookFailureMode {
            category: HOOK_BLOCKED_AGENT_FLOW.to_owned(),
            confidence: confidence_for(incident.signal_counts.user_correction_after_hook)
                .to_owned(),
            evidence_ids: signal_anchors.iter().map(|a| a.id).collect(),
        });
        findings.prevention_hints.push(HookPreventionHint {
            category: HOOK_BLOCKED_AGENT_FLOW.to_owned(),
            hint: "Review the hook's injected context/instructions for ambiguity or conflict \
                   with the user's actual request; tighten the hook's output to be unambiguous."
                .to_owned(),
        });
    }

    // ── Phrase-scanned contributing factors over the transcript/log evidence ──
    let mutation_hit_ids: Vec<i64> = scannable(
        signal_anchors,
        transcript_before,
        transcript_after,
        nearby_logs,
        nearby_errors,
    )
    .filter(|entry| {
        let lower = entry.message.to_ascii_lowercase();
        [
            "unexpected config change",
            "unexpected file change",
            "mutated state",
        ]
        .iter()
        .any(|kw| lower.contains(kw))
    })
    .map(|e| e.id)
    .collect();
    if !mutation_hit_ids.is_empty() {
        findings.likely_failure_modes.push(HookFailureMode {
            category: HOOK_MUTATED_UNEXPECTED_STATE.to_owned(),
            confidence: confidence_for(mutation_hit_ids.len()).to_owned(),
            evidence_ids: mutation_hit_ids,
        });
        findings.prevention_hints.push(HookPreventionHint {
            category: HOOK_MUTATED_UNEXPECTED_STATE.to_owned(),
            hint: "Scope the hook's file/config writes narrowly and document exactly what it is \
                   allowed to mutate."
                .to_owned(),
        });
    }

    // ── Additional phrase-scanned failure modes ─────────────────────────────
    let scope_hit_ids: Vec<i64> = scannable(
        signal_anchors,
        transcript_before,
        transcript_after,
        nearby_logs,
        nearby_errors,
    )
    .filter(|entry| {
        let lower = entry.message.to_ascii_lowercase();
        [
            "hook fired on the wrong",
            "wrong tool for this hook",
            "hook matched too broadly",
            "hook should not have run",
        ]
        .iter()
        .any(|kw| lower.contains(kw))
    })
    .map(|e| e.id)
    .collect();
    if !scope_hit_ids.is_empty() {
        findings.likely_failure_modes.push(HookFailureMode {
            category: HOOK_WRONG_SCOPE.to_owned(),
            confidence: confidence_for(scope_hit_ids.len()).to_owned(),
            evidence_ids: scope_hit_ids,
        });
        findings.prevention_hints.push(HookPreventionHint {
            category: HOOK_WRONG_SCOPE.to_owned(),
            hint: "Tighten the hook's matcher so it fires only for the intended tool/event scope."
                .to_owned(),
        });
    }

    let drift_hit_ids: Vec<i64> = scannable(
        signal_anchors,
        transcript_before,
        transcript_after,
        nearby_logs,
        nearby_errors,
    )
    .filter(|entry| {
        let lower = entry.message.to_ascii_lowercase();
        [
            "hook config drifted",
            "hook policy changed",
            "unexpected hook configuration",
            "hook trust changed",
        ]
        .iter()
        .any(|kw| lower.contains(kw))
    })
    .map(|e| e.id)
    .collect();
    if !drift_hit_ids.is_empty() {
        findings.likely_failure_modes.push(HookFailureMode {
            category: HOOK_POLICY_DRIFT.to_owned(),
            confidence: confidence_for(drift_hit_ids.len()).to_owned(),
            evidence_ids: drift_hit_ids,
        });
        findings.prevention_hints.push(HookPreventionHint {
            category: HOOK_POLICY_DRIFT.to_owned(),
            hint: "Pin the hook's config/trust state and review changes to the hook source before \
                   re-trusting it."
                .to_owned(),
        });
    }

    // ── hook_not_invoked: a config/trust-only incident (a hook is configured
    // and/or trusted but no runtime execution evidence exists in this
    // incident) is the ONLY safe basis for a not-invoked signal, and ONLY as a
    // low-confidence hypothesis — per GH #105, config presence is never proof
    // of non-execution across sessions, so this stays scoped to the incident's
    // own evidence and is explicitly low confidence.
    let has_config_evidence = hook_events
        .iter()
        .any(|e| e.evidence_kind == "config_inventory" || e.evidence_kind == "trusted_hash_state");
    if has_config_evidence && !incident.has_runtime_evidence {
        findings.likely_failure_modes.push(HookFailureMode {
            category: HOOK_NOT_INVOKED.to_owned(),
            confidence: "low".to_owned(),
            evidence_ids: hook_event_ids.clone(),
        });
        findings.prevention_hints.push(HookPreventionHint {
            category: HOOK_NOT_INVOKED.to_owned(),
            hint:
                "This hook is configured/trusted but shows no runtime execution evidence in this \
                   window. Confirm it is wired to fire for the expected event, and compare against \
                   runtime evidence for the same session before concluding it never ran."
                    .to_owned(),
        });
    }

    if !nearby_tool_calls.is_empty() {
        findings.contributing_factors.push(HookContributingFactor {
            factor: format!(
                "{} nearby tool-call failure(s) in the correlation window; hook output may have \
                 contributed to the failing tool call.",
                nearby_tool_calls.len()
            ),
            evidence_ids: nearby_tool_calls.iter().map(|e| e.id).collect(),
        });
        findings.likely_failure_modes.push(HookFailureMode {
            category: HOOK_CAUSED_TOOL_FAILURE.to_owned(),
            confidence: "low".to_owned(),
            evidence_ids: nearby_tool_calls.iter().map(|e| e.id).collect(),
        });
    }

    if incident.hook_event_count >= HOOK_EVENT_FACTOR_THRESHOLD {
        findings.contributing_factors.push(HookContributingFactor {
            factor: format!(
                "Repeated hook invocation: {} hook events within the incident window.",
                incident.hook_event_count
            ),
            evidence_ids: hook_event_ids.clone(),
        });
    }
    if nearby_errors.len() >= ERROR_BURST_THRESHOLD {
        findings.contributing_factors.push(HookContributingFactor {
            factor: format!(
                "Error burst: {} error-level logs in the correlation window.",
                nearby_errors.len()
            ),
            evidence_ids: nearby_errors.iter().map(|e| e.id).collect(),
        });
    }

    if findings.likely_failure_modes.is_empty() {
        findings.likely_failure_modes.push(HookFailureMode {
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
    if !incident.has_runtime_evidence {
        findings.open_questions.push(
            "No runtime_transcript evidence was found for this hook — findings are based on \
             configuration/trust-state inventory only and do not prove the hook executed."
                .to_owned(),
        );
    }
    if signal_anchors.is_empty() && hook_events.is_empty() {
        findings
            .open_questions
            .push("No hook events or signal anchors were captured for this incident.".to_owned());
    }

    findings
}

#[cfg(test)]
#[path = "hook_incident_findings_tests.rs"]
mod tests;
