use anyhow::Result;
use cortex::app::{AiHookIncidentResponse, AiHookInvestigateResponse};

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
    if response.no_data {
        println!(
            "No hook events found for the given filter.\nTry: {}",
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
            "{}: no negative signal detected; showing a summary",
            warn("note")
        );
    }
    for evidence in &response.evidence {
        let incident = &evidence.incident;
        println!();
        println!(
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
        );
        println!(
            "  {} hook event(s), {} signal anchor(s), {} nearby log(s), {} nearby error(s)",
            cyan(&evidence.hook_events.len().to_string()),
            cyan(&evidence.signal_anchors.len().to_string()),
            cyan(&evidence.nearby_logs.len().to_string()),
            cyan(&evidence.nearby_errors.len().to_string()),
        );
    }
    Ok(())
}
