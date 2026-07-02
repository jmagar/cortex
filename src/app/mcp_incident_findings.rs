//! Deterministic failure-hypothesis and prevention-hint generation over
//! MCP-incident evidence bundles. Pure rule evaluation — never queries the
//! database and never calls an external LLM. Mirrors
//! `src/app/skill_incident_findings.rs` but targets the MCP finding
//! categories from GH #94's "Suggested MCP finding categories" section.

use serde::{Deserialize, Serialize};

use super::models::{LogEntry, McpIncident};

// ── Stable failure-mode categories (GH #94 "Suggested MCP finding
// categories") ──────────────────────────────────────────────────────────────
pub const WRONG_MCP_TOOL_SELECTED: &str = "wrong_mcp_tool_selected";
pub const MCP_SERVER_UNAVAILABLE: &str = "mcp_server_unavailable";
pub const MCP_AUTH_OR_PERMISSION_FAILURE: &str = "mcp_auth_or_permission_failure";
pub const MCP_SCHEMA_MISMATCH: &str = "mcp_schema_mismatch";
pub const MCP_TIMEOUT_OR_RATE_LIMIT: &str = "mcp_timeout_or_rate_limit";
pub const MCP_RESULT_MISINTERPRETED: &str = "mcp_result_misinterpreted";
pub const MISSING_MCP_DISCOVERY_STEP: &str = "missing_mcp_discovery_step";
pub const TOOL_SURFACE_CONFUSION: &str = "tool_surface_confusion";
pub const UNKNOWN: &str = "unknown";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpFailureMode {
    pub category: String,
    pub confidence: String,
    pub evidence_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpContributingFactor {
    pub factor: String,
    pub evidence_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpPreventionHint {
    pub category: String,
    pub hint: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpIncidentFindings {
    pub likely_failure_modes: Vec<McpFailureMode>,
    pub contributing_factors: Vec<McpContributingFactor>,
    pub prevention_hints: Vec<McpPreventionHint>,
    pub open_questions: Vec<String>,
}

/// `(category, keyword substrings, prevention hint)`. Kept specific — no
/// broad single tokens.
type Rule = (&'static str, &'static [&'static str], &'static str);

const RULES: &[Rule] = &[
    (
        MCP_SERVER_UNAVAILABLE,
        &[
            "server unavailable",
            "server not found",
            "not connected",
            "disconnected",
            "mcp server error",
        ],
        "Check the MCP server's connection/health before retrying, and surface a clear \
         reconnect step in the calling skill/doc instead of retrying blind.",
    ),
    (
        MCP_AUTH_OR_PERMISSION_FAILURE,
        &[
            "permission denied",
            "unauthorized",
            "forbidden",
            "authentication failed",
            "auth failed",
            "invalid token",
            "invalid credentials",
        ],
        "Add an auth-check step before the tool call (verify token/credential presence) and \
         document the expected auth setup for this MCP server.",
    ),
    (
        MCP_SCHEMA_MISMATCH,
        &[
            "schema validation",
            "invalid parameters",
            "invalid arguments",
            "missing required",
            "does not match schema",
            "validation error",
            "invalidparams",
        ],
        "Review the tool's parameter schema against how the agent is calling it; update the \
         skill/tool doc with a concrete example matching the current schema.",
    ),
    (
        MCP_TIMEOUT_OR_RATE_LIMIT,
        &[
            "timed out",
            "timeout",
            "rate limit",
            "rate-limited",
            "too many requests",
        ],
        "Add a backoff/retry policy note for this tool, and document a lower-cost alternative \
         call pattern if repeated calls are triggering rate limits.",
    ),
    (
        WRONG_MCP_TOOL_SELECTED,
        &["wrong tool", "not the right tool", "you used the wrong"],
        "Narrow the tool's description/trigger phrases so it's less likely to be selected for \
         out-of-scope requests, and cross-reference the correct tool in its docstring.",
    ),
    (
        TOOL_SURFACE_CONFUSION,
        &[
            "unknown tool",
            "tool not found",
            "no such tool",
            "wrong server",
        ],
        "Add a discovery step (list available tools/servers) before assuming a specific tool \
         name, and document the exact tool surface this skill depends on.",
    ),
    (
        MCP_RESULT_MISINTERPRETED,
        &[
            "that's not what i asked",
            "that is not what i asked",
            "misread the result",
            "misinterpreted",
        ],
        "Add a verification step requiring the agent to restate what the tool result actually \
         showed before acting on it.",
    ),
    (
        MISSING_MCP_DISCOVERY_STEP,
        &[
            "should have checked",
            "should have searched",
            "didn't check available tools",
            "did not check available tools",
        ],
        "Add an explicit discovery/search step to the skill doc before assuming a tool is \
         unavailable or using a guessed tool name.",
    ),
];

fn confidence_for(count: usize) -> &'static str {
    match count {
        0 | 1 => "low",
        2 => "medium",
        _ => "high",
    }
}

const EVENT_COUNT_FACTOR_THRESHOLD: usize = 3;
const ERROR_BURST_THRESHOLD: usize = 2;

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

/// Derive deterministic findings from an MCP-incident evidence bundle. Pure
/// and total: identical input always yields identical output; every
/// non-`unknown` failure mode / contributing factor cites at least one
/// evidence id; weak evidence yields `unknown` + `open_questions` rather
/// than an unsupported claim.
#[allow(clippy::too_many_arguments)]
pub fn derive_mcp_incident_findings(
    incident: &McpIncident,
    _mcp_events: &[crate::db::AiMcpEventEntry],
    signal_anchors: &[LogEntry],
    transcript_before: &[LogEntry],
    transcript_after: &[LogEntry],
    nearby_logs: &[LogEntry],
    nearby_errors: &[LogEntry],
) -> McpIncidentFindings {
    let mut findings = McpIncidentFindings::default();

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
            findings.likely_failure_modes.push(McpFailureMode {
                category: (*category).to_owned(),
                confidence,
                evidence_ids: ids,
            });
            findings.prevention_hints.push(McpPreventionHint {
                category: (*category).to_owned(),
                hint: (*hint).to_owned(),
            });
        }
    }

    if incident.event_count >= EVENT_COUNT_FACTOR_THRESHOLD && incident.error_count > 0 {
        findings.contributing_factors.push(McpContributingFactor {
            factor: format!(
                "Repeated tool calls with errors: {} events ({} errors) within the incident window.",
                incident.event_count, incident.error_count
            ),
            evidence_ids: signal_anchors.iter().map(|a| a.id).collect(),
        });
    }
    if incident.error_count >= ERROR_BURST_THRESHOLD {
        findings.contributing_factors.push(McpContributingFactor {
            factor: format!(
                "Error burst: {} error-flagged MCP events for {}/{}.",
                incident.error_count,
                incident.mcp_server,
                incident.mcp_tool.as_deref().unwrap_or("*")
            ),
            evidence_ids: nearby_errors.iter().map(|e| e.id).collect(),
        });
    }

    if findings.likely_failure_modes.is_empty() {
        findings.likely_failure_modes.push(McpFailureMode {
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
#[path = "mcp_incident_findings_tests.rs"]
mod tests;
