use anyhow::Result;
use cortex::app::{
    AiIncidentResponse, AiInvestigateResponse, AskHistoryResponse, IncidentContextResponse,
    SimilarIncidentsResponse,
};

use super::color::{cyan, muted, primary, severity, violet, warn};
use super::output_common::{local_ts, print_json, truncate};

pub(crate) fn print_similar_incidents_response(
    response: &SimilarIncidentsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} incident cluster(s){}",
        cyan(&response.total_clusters.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for cluster in &response.clusters {
        println!(
            "\n[{} / {}] {} → {} | {} log(s) | peak: {}",
            cyan(cluster.hostname.as_str()),
            primary(cluster.app_name.as_deref().unwrap_or("-")),
            muted(&cluster.window_start),
            muted(&cluster.window_end),
            cyan(&cluster.log_count.to_string()),
            severity(&cluster.severity_peak)
        );
        for msg in &cluster.representative_messages {
            println!("  {}", truncate(msg, 120));
        }
        if !cluster.correlated_sessions.is_empty() {
            println!("  {}:", muted("AI sessions"));
            for sess in &cluster.correlated_sessions {
                println!(
                    "    [{}/{}] {} ({} hits)",
                    violet(&sess.tool),
                    primary(&sess.project),
                    muted(&sess.session_id),
                    cyan(&sess.match_count.to_string())
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn print_ask_history_response(response: &AskHistoryResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} session(s) for query {:?}{}",
        cyan(&response.sessions.len().to_string()),
        response.query,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for session in &response.sessions {
        println!(
            "{:<10} {:<30} {:<20} {} hit(s)",
            violet(&session.tool),
            primary(&truncate(&session.project, 29)),
            muted(&truncate(&session.session_id, 19)),
            cyan(&session.match_count.to_string())
        );
        if let Some(snippet) = &session.best_snippet {
            println!("  {}: {}", muted("snippet"), truncate(snippet, 100));
        }
    }
    if !response.context_logs.is_empty() {
        println!(
            "\n{} ({} entries):",
            muted("System log context"),
            cyan(&response.context_logs.len().to_string())
        );
        for log in &response.context_logs {
            println!(
                "  [{}] {} {} {}",
                severity(&log.severity),
                muted(&local_ts(&log.timestamp)),
                cyan(&log.hostname),
                truncate(&log.message, 80)
            );
        }
    }
    Ok(())
}

pub(crate) fn print_incident_context_response(
    response: &IncidentContextResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{}: {} → {}",
        muted("Window"),
        muted(&response.window_from),
        muted(&response.window_to)
    );
    println!(
        "{}: {}",
        muted("Total logs"),
        cyan(&response.total_logs.to_string())
    );
    println!("{}:", muted("By severity"));
    for sv in &response.by_severity {
        println!(
            "  {:<10} {}",
            severity(&sv.severity),
            cyan(&sv.count.to_string())
        );
    }
    let truncated_note = if response.error_logs_truncated {
        " (truncated)".to_string()
    } else {
        String::new()
    };
    println!(
        "{} ({}{}):",
        muted("Error logs"),
        cyan(&response.error_logs.len().to_string()),
        truncated_note
    );
    for log in &response.error_logs {
        println!(
            "  [{}] {} {} {}",
            severity(&log.severity),
            muted(&local_ts(&log.timestamp)),
            cyan(&log.hostname),
            truncate(&log.message, 80)
        );
    }
    if !response.ai_sessions.is_empty() {
        println!(
            "{} ({}):",
            muted("AI sessions"),
            cyan(&response.ai_sessions.len().to_string())
        );
        for sess in &response.ai_sessions {
            println!(
                "  {}/{} {} {} → {}",
                violet(&sess.tool),
                primary(&truncate(&sess.project, 20)),
                muted(&truncate(&sess.session_id, 16)),
                muted(&sess.first_seen),
                muted(&sess.last_seen)
            );
        }
    }
    Ok(())
}

pub(crate) fn print_ai_incidents_response(response: &AiIncidentResponse, json: bool) -> Result<()> {
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
                "\n{}: candidate scan capped at {} rows; narrow with --project/--tool/--from/--to",
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
            "incident {} score={:.1} [{}] project={} tool={} session={}",
            primary(&inc.incident_id),
            inc.priority_score,
            severity(&inc.priority_label),
            primary(&inc.project),
            violet(&inc.tool),
            muted(&inc.session_id),
        );
        println!(
            "  host={} first={} last={} duration={}s anchors={}",
            cyan(&inc.hostname),
            muted(&local_ts(&inc.first_seen)),
            muted(&local_ts(&inc.last_seen)),
            cyan(&inc.duration_secs.to_string()),
            cyan(&inc.abuse_count.to_string()),
        );
        println!("  {}: {}", muted("terms"), primary(&inc.terms.join(", ")));
        println!("  {}: {:?}", muted("anchor ids"), inc.anchor_ids);
    }
    Ok(())
}

pub(crate) fn print_ai_investigate_response(
    response: &AiInvestigateResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
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
    for ev in &response.evidence {
        let inc = &ev.incident;
        println!();
        println!(
            "incident {} [{}] project={} tool={} session={}",
            primary(&inc.incident_id),
            severity(&inc.priority_label),
            primary(&inc.project),
            violet(&inc.tool),
            muted(&inc.session_id)
        );
        println!(
            "  {} anchor(s), {} transcript-before{}, {} transcript-after{}, {} nearby log(s), {} nearby error(s)",
            cyan(&ev.anchors.len().to_string()),
            cyan(&ev.transcript_before.len().to_string()),
            if ev.transcript_before_truncated { " (trunc)" } else { "" },
            cyan(&ev.transcript_after.len().to_string()),
            if ev.transcript_after_truncated { " (trunc)" } else { "" },
            cyan(&ev.nearby_logs.len().to_string()),
            cyan(&ev.nearby_errors.len().to_string()),
        );
        println!("  {}:", muted("anchor messages"));
        for a in &ev.anchors {
            println!("    [{}] {}", muted(&local_ts(&a.timestamp)), a.message);
        }
        if !ev.nearby_errors.is_empty() {
            println!("  {}:", muted("nearby errors"));
            for e in &ev.nearby_errors {
                println!(
                    "    [{}] ({}) {}",
                    muted(&local_ts(&e.timestamp)),
                    severity(&e.severity),
                    e.message
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "output_ai_more_tests.rs"]
mod tests;
