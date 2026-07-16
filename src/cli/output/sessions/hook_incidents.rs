use anyhow::Result;
use cortex::app::{AiHookIncidentResponse, AiHookInvestigateResponse};
use std::fmt::Write as _;

use super::super::super::color::{cyan, muted, primary, severity, violet, warn};
use super::super::common::{local_ts, print_json};

pub(crate) fn print_ai_hook_incidents_response(
    response: &AiHookIncidentResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} incident(s) of {} total{}",
        cyan(&response.incidents.len().to_string()),
        cyan(&response.total_incidents.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    if response.candidate_window_truncated {
        println!(
            "{}: candidate scan capped at {} rows; narrow with a hook name or --since",
            warn("warning"),
            cyan(&response.candidate_cap.to_string())
        );
    }
    for incident in &response.incidents {
        println!();
        println!(
            "incident {} score={:.1} [{}] hook={} tool={} project={} session={}",
            primary(&incident.incident_id),
            incident.priority_score,
            severity(&incident.priority_label),
            primary(
                incident
                    .hook_name
                    .as_deref()
                    .unwrap_or(&incident.hook_event)
            ),
            violet(&incident.tool),
            primary(&incident.project),
            muted(&incident.session_id),
        );
        println!(
            "  host={} first={} last={} duration={}s events={}",
            cyan(&incident.hostname),
            muted(&local_ts(&incident.first_seen)),
            muted(&local_ts(&incident.last_seen)),
            cyan(&incident.duration_secs.to_string()),
            cyan(&incident.hook_event_count.to_string()),
        );
        println!(
            "  {}: {}",
            muted("signals"),
            incident.signals_present.join(", ")
        );
    }
    Ok(())
}

pub(crate) fn print_ai_hook_investigate_response(
    response: &AiHookInvestigateResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    print!("{}", render_ai_hook_investigate_response(response));
    Ok(())
}

fn evidence_count(count: usize, truncated: bool) -> String {
    if truncated {
        format!("{count} (truncated)")
    } else {
        count.to_string()
    }
}

fn render_ai_hook_investigate_response(response: &AiHookInvestigateResponse) -> String {
    let mut output = String::new();
    if response.no_data {
        writeln!(
            output,
            "No hook events found for the given filter.\nTry: {}",
            response.suggested_filters.join("; ")
        )
        .expect("writing to a String cannot fail");
        return output;
    }
    writeln!(
        output,
        "{} evidence bundle(s) of {} total incident(s){}",
        cyan(&response.evidence.len().to_string()),
        cyan(&response.total_incidents.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    )
    .expect("writing to a String cannot fail");
    if response.no_incident_low_severity_summary {
        writeln!(
            output,
            "{}: no negative signal detected; showing a summary",
            warn("note")
        )
        .expect("writing to a String cannot fail");
    }
    for evidence in &response.evidence {
        let incident = &evidence.incident;
        writeln!(output).expect("writing to a String cannot fail");
        writeln!(
            output,
            "incident {} [{}] hook={} tool={} project={} session={}",
            primary(&incident.incident_id),
            severity(&incident.priority_label),
            primary(
                incident
                    .hook_name
                    .as_deref()
                    .unwrap_or(&incident.hook_event)
            ),
            violet(&incident.tool),
            primary(&incident.project),
            muted(&incident.session_id),
        )
        .expect("writing to a String cannot fail");
        writeln!(
            output,
            "  context: hook_events={}, signal_anchors={}, transcript_before={}, \
             transcript_after={}, nearby_tool_calls={}, nearby_logs={}, nearby_errors={}",
            evidence_count(evidence.hook_events.len(), evidence.hook_events_truncated),
            evidence_count(
                evidence.signal_anchors.len(),
                evidence.signal_anchors_truncated
            ),
            evidence_count(
                evidence.transcript_before.len(),
                evidence.transcript_before_truncated
            ),
            evidence_count(
                evidence.transcript_after.len(),
                evidence.transcript_after_truncated
            ),
            evidence_count(
                evidence.nearby_tool_calls.len(),
                evidence.nearby_tool_calls_truncated
            ),
            evidence_count(evidence.nearby_logs.len(), evidence.nearby_logs_truncated),
            evidence_count(
                evidence.nearby_errors.len(),
                evidence.nearby_errors_truncated
            ),
        )
        .expect("writing to a String cannot fail");
        writeln!(
            output,
            "  {}: {}",
            muted("evidence basis"),
            evidence.findings.evidence_basis
        )
        .expect("writing to a String cannot fail");
        if !evidence.findings.likely_failure_modes.is_empty() {
            writeln!(output, "  {}", muted("likely failure modes:"))
                .expect("writing to a String cannot fail");
            for mode in &evidence.findings.likely_failure_modes {
                writeln!(
                    output,
                    "    - {} (confidence: {}; evidence: {:?})",
                    primary(&mode.category),
                    muted(&mode.confidence),
                    mode.evidence_ids
                )
                .expect("writing to a String cannot fail");
            }
        }
        if !evidence.findings.contributing_factors.is_empty() {
            writeln!(output, "  {}", muted("contributing factors:"))
                .expect("writing to a String cannot fail");
            for factor in &evidence.findings.contributing_factors {
                writeln!(output, "    - {}", factor.factor)
                    .expect("writing to a String cannot fail");
            }
        }
        if !evidence.findings.prevention_hints.is_empty() {
            writeln!(output, "  {}", muted("prevention hints:"))
                .expect("writing to a String cannot fail");
            for hint in &evidence.findings.prevention_hints {
                writeln!(output, "    - {}", hint.hint).expect("writing to a String cannot fail");
            }
        }
        if !evidence.findings.open_questions.is_empty() {
            writeln!(output, "  {}", muted("open questions:"))
                .expect("writing to a String cannot fail");
            for question in &evidence.findings.open_questions {
                writeln!(output, "    - {question}").expect("writing to a String cannot fail");
            }
        }
    }
    if !response.other_matching_incidents.is_empty() {
        writeln!(output).expect("writing to a String cannot fail");
        writeln!(
            output,
            "{} related incident(s):",
            cyan(&response.other_matching_incidents.len().to_string())
        )
        .expect("writing to a String cannot fail");
        for summary in &response.other_matching_incidents {
            writeln!(
                output,
                "  {} score={:.1} [{}] runtime={} first={} last={}",
                primary(&summary.incident_id),
                summary.priority_score,
                severity(&summary.priority_label),
                summary.has_runtime_evidence,
                muted(&local_ts(&summary.first_seen)),
                muted(&local_ts(&summary.last_seen)),
            )
            .expect("writing to a String cannot fail");
        }
    }
    output
}

#[cfg(test)]
#[path = "hook_incidents_tests.rs"]
mod tests;
