//! Deterministic failure-hypothesis and prevention-hint generation over abuse
//! incident evidence bundles (bead syslog-mcp-kmib.4).
//!
//! This is **pure rule evaluation** over an already-built evidence bundle — it
//! never queries the database and never calls an external LLM. Every emitted
//! finding cites the log row ids that support it, confidence is conservative
//! (high only when multiple evidence items agree), and the result always
//! surfaces an `unknown` mode plus `open_questions` when the signal is weak,
//! so a downstream summariser is never tempted to overclaim a root cause.

use serde::{Deserialize, Serialize};

use super::models::{AbuseIncident, LogEntry};

// ── Stable failure-mode categories ──────────────────────────────────────────
pub const COMMAND_FAILURE: &str = "command_failure";
pub const TOOL_TIMEOUT: &str = "tool_timeout";
pub const AUTH_OR_PERMISSION_FAILURE: &str = "auth_or_permission_failure";
pub const STALE_BINARY_OR_VERSION_DRIFT: &str = "stale_binary_or_version_drift";
pub const TEST_FAILURE: &str = "test_failure";
pub const DOCKER_OR_SERVICE_RUNTIME_FAILURE: &str = "docker_or_service_runtime_failure";
pub const DB_BUSY_OR_PERFORMANCE_BOTTLENECK: &str = "db_busy_or_performance_bottleneck";
pub const UNCLEAR_INSTRUCTION_OR_SCOPE_DRIFT: &str = "unclear_instruction_or_scope_drift";
pub const UNKNOWN: &str = "unknown";

/// One detected failure category with conservative confidence and the evidence
/// row ids that triggered it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailureMode {
    pub category: String,
    /// `"low"`, `"medium"`, or `"high"`. `high` requires ≥3 supporting rows;
    /// `medium` requires ≥2; a single hit is always `low`.
    pub confidence: String,
    pub evidence_ids: Vec<i64>,
}

/// A contributing factor inferred from the evidence window. Always cites
/// evidence row ids.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContributingFactor {
    pub factor: String,
    pub evidence_ids: Vec<i64>,
}

/// A templated, category-tied prevention suggestion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreventionHint {
    pub category: String,
    pub hint: String,
}

/// Deterministic findings for one incident evidence bundle.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncidentFindings {
    pub likely_failure_modes: Vec<FailureMode>,
    pub contributing_factors: Vec<ContributingFactor>,
    pub prevention_hints: Vec<PreventionHint>,
    pub open_questions: Vec<String>,
}

/// `(category, keyword substrings, prevention hint)`. Keywords are matched
/// case-insensitively against log message text. Kept deliberately specific so
/// generic noise does not trip a category — broad tokens like `error:` are
/// intentionally excluded.
type Rule = (&'static str, &'static [&'static str], &'static str);

const RULES: &[Rule] = &[
    (
        TOOL_TIMEOUT,
        &[
            "timeout",
            "timed out",
            "deadline exceeded",
            "context deadline",
        ],
        "Add or raise an explicit timeout and retry the operation with backoff before escalating.",
    ),
    (
        AUTH_OR_PERMISSION_FAILURE,
        &[
            "401",
            "403",
            "unauthorized",
            "permission denied",
            "forbidden",
            "access denied",
            "authentication failed",
        ],
        "Verify credentials/scopes and file or socket permissions before retrying the action.",
    ),
    (
        STALE_BINARY_OR_VERSION_DRIFT,
        &[
            "version mismatch",
            "version drift",
            "stale binary",
            "out of date",
            "rebuild required",
            "binary mismatch",
        ],
        "Rebuild and redeploy the affected binary/image so host and container versions match.",
    ),
    (
        TEST_FAILURE,
        &[
            "test failed",
            "tests failed",
            "assertion failed",
            "test result: failed",
            "panicked at",
        ],
        "Reproduce the failing test in isolation and fix it before retrying the broader task.",
    ),
    (
        DOCKER_OR_SERVICE_RUNTIME_FAILURE,
        &[
            "oomkilled",
            "out of memory",
            "container exited",
            "crashloop",
            "unhealthy",
            "restarting",
            "segmentation fault",
        ],
        "Inspect container/service logs and resource limits; raise limits or fix the crash loop.",
    ),
    (
        DB_BUSY_OR_PERFORMANCE_BOTTLENECK,
        &[
            "database is locked",
            "database busy",
            "sqlite_busy",
            "worker limit",
            "too many connections",
            "deadlock",
        ],
        "Reduce write concurrency, add retry-on-busy, or widen the DB worker/connection budget.",
    ),
    (
        COMMAND_FAILURE,
        &[
            "command not found",
            "no such file or directory",
            "non-zero exit",
            "exit code",
            "exit status",
        ],
        "Confirm the command, its arguments, and working directory exist before re-running it.",
    ),
    (
        UNCLEAR_INSTRUCTION_OR_SCOPE_DRIFT,
        &[
            "not what i asked",
            "going in circles",
            "wrong file",
            "misunderstood",
            "that is not what",
        ],
        "Restate the goal and acceptance criteria explicitly and confirm scope before continuing.",
    ),
];

/// Threshold above which the raw abuse-anchor count is treated as a
/// frustration contributing factor.
const ABUSE_FACTOR_THRESHOLD: usize = 3;
/// Number of nearby error rows that constitutes an "error burst" factor.
const ERROR_BURST_THRESHOLD: usize = 3;

fn confidence_for(count: usize) -> &'static str {
    match count {
        0 | 1 => "low",
        2 => "medium",
        _ => "high",
    }
}

/// Evidence rows scanned for category keywords: transcript context on both
/// sides, the abuse anchors themselves, and nearby non-AI logs/errors.
fn scannable<'a>(
    anchors: &'a [LogEntry],
    transcript_before: &'a [LogEntry],
    transcript_after: &'a [LogEntry],
    nearby_logs: &'a [LogEntry],
    nearby_errors: &'a [LogEntry],
) -> impl Iterator<Item = &'a LogEntry> {
    anchors
        .iter()
        .chain(transcript_before)
        .chain(transcript_after)
        .chain(nearby_logs)
        .chain(nearby_errors)
}

/// Derive deterministic findings from an incident evidence bundle.
///
/// The function is total and side-effect free: identical input always yields
/// identical output, every failure mode / contributing factor cites at least
/// one evidence id, and weak evidence yields an `unknown` mode plus
/// `open_questions` rather than an unsupported claim.
pub fn derive_incident_findings(
    incident: &AbuseIncident,
    anchors: &[LogEntry],
    transcript_before: &[LogEntry],
    transcript_after: &[LogEntry],
    nearby_logs: &[LogEntry],
    nearby_errors: &[LogEntry],
) -> IncidentFindings {
    let mut findings = IncidentFindings::default();

    // ── Rule evaluation: collect supporting evidence ids per category ───────
    for (category, keywords, hint) in RULES {
        let mut ids: Vec<i64> = Vec::new();
        for entry in scannable(
            anchors,
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
            findings.likely_failure_modes.push(FailureMode {
                category: (*category).to_owned(),
                confidence,
                evidence_ids: ids,
            });
            findings.prevention_hints.push(PreventionHint {
                category: (*category).to_owned(),
                hint: (*hint).to_owned(),
            });
        }
    }

    // ── Contributing factors (each cites evidence) ──────────────────────────
    if incident.abuse_count >= ABUSE_FACTOR_THRESHOLD && !anchors.is_empty() {
        findings.contributing_factors.push(ContributingFactor {
            factor: format!(
                "Elevated frustration signal: {} abuse anchors within the incident window.",
                incident.abuse_count
            ),
            evidence_ids: anchors.iter().map(|a| a.id).collect(),
        });
    }
    if nearby_errors.len() >= ERROR_BURST_THRESHOLD {
        findings.contributing_factors.push(ContributingFactor {
            factor: format!(
                "Error burst: {} error-level logs in the correlation window.",
                nearby_errors.len()
            ),
            evidence_ids: nearby_errors.iter().map(|e| e.id).collect(),
        });
    }

    // ── Open questions / unknown handling ───────────────────────────────────
    if findings.likely_failure_modes.is_empty() {
        // No deterministic signature matched — never overclaim.
        findings.likely_failure_modes.push(FailureMode {
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
    if anchors.is_empty() {
        findings
            .open_questions
            .push("No abuse anchors were captured for this incident.".to_owned());
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
#[path = "incident_findings_tests.rs"]
mod tests;
