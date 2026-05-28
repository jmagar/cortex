use anyhow::Result;
use syslog_mcp::app::{
    AiIncidentResponse, AiInvestigateResponse, AskHistoryResponse, IncidentContextResponse,
    SimilarIncidentsResponse,
};

use super::color::Palette;
use super::output_common::{local_ts, print_json, truncate};

pub(crate) fn print_similar_incidents_response(
    response: &SimilarIncidentsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "{} incident cluster(s){}",
        p.cyan(&response.total_clusters.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for cluster in &response.clusters {
        println!(
            "\n[{} / {}] {} → {} | {} log(s) | peak: {}",
            p.cyan(cluster.hostname.as_str()),
            p.primary(cluster.app_name.as_deref().unwrap_or("-")),
            p.muted(&cluster.window_start),
            p.muted(&cluster.window_end),
            p.cyan(&cluster.log_count.to_string()),
            p.severity(&cluster.severity_peak)
        );
        for msg in &cluster.representative_messages {
            println!("  {}", truncate(msg, 120));
        }
        if !cluster.correlated_sessions.is_empty() {
            println!("  {}:", p.muted("AI sessions"));
            for sess in &cluster.correlated_sessions {
                println!(
                    "    [{}/{}] {} ({} hits)",
                    p.violet(&sess.tool),
                    p.primary(&sess.project),
                    p.muted(&sess.session_id),
                    p.cyan(&sess.match_count.to_string())
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
    let p = Palette::new();
    println!(
        "{} session(s) for query {:?}{}",
        p.cyan(&response.sessions.len().to_string()),
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
            p.violet(&session.tool),
            p.primary(&truncate(&session.project, 29)),
            p.muted(&truncate(&session.session_id, 19)),
            p.cyan(&session.match_count.to_string())
        );
        if let Some(snippet) = &session.best_snippet {
            println!("  {}: {}", p.muted("snippet"), truncate(snippet, 100));
        }
    }
    if !response.context_logs.is_empty() {
        println!(
            "\n{} ({} entries):",
            p.muted("System log context"),
            p.cyan(&response.context_logs.len().to_string())
        );
        for log in &response.context_logs {
            println!(
                "  [{}] {} {} {}",
                p.severity(&log.severity),
                p.muted(&local_ts(&log.timestamp)),
                p.cyan(&log.hostname),
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
    let p = Palette::new();
    println!(
        "{}: {} → {}",
        p.muted("Window"),
        p.muted(&response.window_from),
        p.muted(&response.window_to)
    );
    println!(
        "{}: {}",
        p.muted("Total logs"),
        p.cyan(&response.total_logs.to_string())
    );
    println!("{}:", p.muted("By severity"));
    for sv in &response.by_severity {
        println!(
            "  {:<10} {}",
            p.severity(&sv.severity),
            p.cyan(&sv.count.to_string())
        );
    }
    let truncated_note = if response.error_logs_truncated {
        " (truncated)".to_string()
    } else {
        String::new()
    };
    println!(
        "{} ({}{}):",
        p.muted("Error logs"),
        p.cyan(&response.error_logs.len().to_string()),
        truncated_note
    );
    for log in &response.error_logs {
        println!(
            "  [{}] {} {} {}",
            p.severity(&log.severity),
            p.muted(&local_ts(&log.timestamp)),
            p.cyan(&log.hostname),
            truncate(&log.message, 80)
        );
    }
    if !response.ai_sessions.is_empty() {
        println!(
            "{} ({}):",
            p.muted("AI sessions"),
            p.cyan(&response.ai_sessions.len().to_string())
        );
        for sess in &response.ai_sessions {
            println!(
                "  {}/{} {} {} → {}",
                p.violet(&sess.tool),
                p.primary(&truncate(&sess.project, 20)),
                p.muted(&truncate(&sess.session_id, 16)),
                p.muted(&sess.first_seen),
                p.muted(&sess.last_seen)
            );
        }
    }
    Ok(())
}

pub(crate) fn print_ai_incidents_response(response: &AiIncidentResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "{} incident(s) of {} total{}{}",
        p.cyan(&response.incidents.len().to_string()),
        p.cyan(&response.total_incidents.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        },
        if response.candidate_window_truncated {
            format!(
                "\n{}: candidate scan capped at {} rows; narrow with --project/--tool/--from/--to",
                p.warn("warning"),
                p.cyan(&response.candidate_cap.to_string())
            )
        } else {
            String::new()
        }
    );
    for inc in &response.incidents {
        println!();
        println!(
            "incident {} score={:.1} [{}] project={} tool={} session={}",
            p.primary(&inc.incident_id),
            inc.priority_score,
            p.severity(&inc.priority_label),
            p.primary(&inc.project),
            p.violet(&inc.tool),
            p.muted(&inc.session_id),
        );
        println!(
            "  host={} first={} last={} duration={}s anchors={}",
            p.cyan(&inc.hostname),
            p.muted(&local_ts(&inc.first_seen)),
            p.muted(&local_ts(&inc.last_seen)),
            p.cyan(&inc.duration_secs.to_string()),
            p.cyan(&inc.abuse_count.to_string()),
        );
        println!(
            "  {}: {}",
            p.muted("terms"),
            p.primary(&inc.terms.join(", "))
        );
        println!("  {}: {:?}", p.muted("anchor ids"), inc.anchor_ids);
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
    let p = Palette::new();
    println!(
        "{} evidence bundle(s) of {} total incident(s){}",
        p.cyan(&response.evidence.len().to_string()),
        p.cyan(&response.total_incidents.to_string()),
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
            p.primary(&inc.incident_id),
            p.severity(&inc.priority_label),
            p.primary(&inc.project),
            p.violet(&inc.tool),
            p.muted(&inc.session_id)
        );
        println!(
            "  {} anchor(s), {} transcript-before{}, {} transcript-after{}, {} nearby log(s), {} nearby error(s)",
            p.cyan(&ev.anchors.len().to_string()),
            p.cyan(&ev.transcript_before.len().to_string()),
            if ev.transcript_before_truncated { " (trunc)" } else { "" },
            p.cyan(&ev.transcript_after.len().to_string()),
            if ev.transcript_after_truncated { " (trunc)" } else { "" },
            p.cyan(&ev.nearby_logs.len().to_string()),
            p.cyan(&ev.nearby_errors.len().to_string()),
        );
        println!("  {}:", p.muted("anchor messages"));
        for a in &ev.anchors {
            println!("    [{}] {}", p.muted(&local_ts(&a.timestamp)), a.message);
        }
        if !ev.nearby_errors.is_empty() {
            println!("  {}:", p.muted("nearby errors"));
            for e in &ev.nearby_errors {
                println!(
                    "    [{}] ({}) {}",
                    p.muted(&local_ts(&e.timestamp)),
                    p.severity(&e.severity),
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
