use anyhow::Result;
use cortex::app::{AiMcpIncidentResponse, AiMcpInvestigateResponse};

use super::super::super::color::{cyan, muted, primary, severity, violet, warn};
use super::super::common::{local_ts, print_json};

pub(crate) fn print_ai_mcp_incidents_response(
    response: &AiMcpIncidentResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} incident(s) of {} total{}{}",
        cyan(&response.incidents.len().to_string()),
        cyan(&response.total_incidents.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        },
        if response.candidate_window_truncated {
            format!(
                "\n{}: candidate scan capped at {} rows; narrow with --mcp-server/--mcp-tool/--since/--until",
                warn("warning"),
                cyan(&response.candidate_cap.to_string())
            )
        } else {
            String::new()
        }
    );
    for inc in &response.incidents {
        println!();
        println!(
            "incident {} score={:.1} [{}] server={} tool={} project={} session={}",
            primary(&inc.incident_id),
            inc.priority_score,
            severity(&inc.priority_label),
            primary(&inc.mcp_server),
            violet(inc.mcp_tool.as_deref().unwrap_or("-")),
            primary(&inc.project),
            muted(&inc.session_id),
        );
        println!(
            "  host={} first={} last={} duration={}s events={} errors={}",
            cyan(&inc.hostname),
            muted(&local_ts(&inc.first_seen)),
            muted(&local_ts(&inc.last_seen)),
            cyan(&inc.duration_secs.to_string()),
            cyan(&inc.event_count.to_string()),
            cyan(&inc.error_count.to_string()),
        );
        println!(
            "  {}: {}",
            muted("signals"),
            primary(&inc.signals_present.join(", "))
        );
    }
    Ok(())
}

pub(crate) fn print_ai_mcp_investigate_response(
    response: &AiMcpInvestigateResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    if response.no_data {
        println!(
            "No MCP events found for the given filter.\nTry: {}",
            response.suggested_filters.join("; ")
        );
        return Ok(());
    }
    println!(
        "{} evidence bundle(s) of {} total incident(s){}",
        cyan(&response.evidence.len().to_string()),
        cyan(&response.total_incidents.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    if response.no_incident_low_severity_summary {
        println!(
            "{}: no negative signal detected — showing the low-severity summary bundle",
            warn("note")
        );
    }
    for ev in &response.evidence {
        let inc = &ev.incident;
        println!();
        println!(
            "incident {} [{}] server={} tool={} project={} session={}",
            primary(&inc.incident_id),
            severity(&inc.priority_label),
            primary(&inc.mcp_server),
            violet(inc.mcp_tool.as_deref().unwrap_or("-")),
            primary(&inc.project),
            muted(&inc.session_id)
        );
        println!(
            "  {} mcp event(s), {} signal anchor(s), {} transcript-before, {} transcript-after, \
             {} nearby log(s), {} nearby error(s)",
            cyan(&ev.mcp_events.len().to_string()),
            cyan(&ev.signal_anchors.len().to_string()),
            cyan(&ev.transcript_before.len().to_string()),
            cyan(&ev.transcript_after.len().to_string()),
            cyan(&ev.nearby_logs.len().to_string()),
            cyan(&ev.nearby_errors.len().to_string()),
        );
        if !ev.findings.likely_failure_modes.is_empty() {
            println!("  {}", muted("likely failure modes:"));
            for mode in &ev.findings.likely_failure_modes {
                println!(
                    "    - {} (confidence: {})",
                    primary(&mode.category),
                    muted(&mode.confidence)
                );
            }
        }
        if !ev.findings.prevention_hints.is_empty() {
            println!("  {}", muted("prevention hints:"));
            for hint in &ev.findings.prevention_hints {
                println!("    - {}", hint.hint);
            }
        }
    }
    if !response.other_matching_incidents.is_empty() {
        println!();
        println!(
            "{} other matching incident(s):",
            cyan(&response.other_matching_incidents.len().to_string())
        );
        for summary in &response.other_matching_incidents {
            println!(
                "  {} score={:.1} [{}] first={} last={}",
                primary(&summary.incident_id),
                summary.priority_score,
                severity(&summary.priority_label),
                muted(&local_ts(&summary.first_seen)),
                muted(&local_ts(&summary.last_seen)),
            );
        }
    }
    Ok(())
}
