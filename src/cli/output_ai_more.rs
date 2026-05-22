use super::*;
pub(crate) fn print_similar_incidents_response(
    response: &SimilarIncidentsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{} incident cluster(s){}",
        response.total_clusters,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for cluster in &response.clusters {
        println!(
            "\n[{} / {}] {} → {} | {} log(s) | peak: {}",
            cluster.hostname,
            cluster.app_name.as_deref().unwrap_or("-"),
            cluster.window_start,
            cluster.window_end,
            cluster.log_count,
            cluster.severity_peak
        );
        for msg in &cluster.representative_messages {
            println!("  {}", truncate(msg, 120));
        }
        if !cluster.correlated_sessions.is_empty() {
            println!("  AI sessions:");
            for sess in &cluster.correlated_sessions {
                println!(
                    "    [{}/{}] {} ({} hits)",
                    sess.tool, sess.project, sess.session_id, sess.match_count
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
        response.sessions.len(),
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
            session.tool,
            truncate(&session.project, 29),
            truncate(&session.session_id, 19),
            session.match_count
        );
        if let Some(snippet) = &session.best_snippet {
            println!("  snippet: {}", truncate(snippet, 100));
        }
    }
    if !response.context_logs.is_empty() {
        println!(
            "\nSystem log context ({} entries):",
            response.context_logs.len()
        );
        for log in &response.context_logs {
            println!(
                "  [{}] {} {} {}",
                log.severity,
                local_ts(&log.timestamp),
                log.hostname,
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
    println!("Window: {} → {}", response.window_from, response.window_to);
    println!("Total logs: {}", response.total_logs);
    println!("By severity:");
    for sv in &response.by_severity {
        println!("  {:<10} {}", sv.severity, sv.count);
    }
    let truncated_note = if response.error_logs_truncated {
        " (truncated)".to_string()
    } else {
        String::new()
    };
    println!(
        "Error logs ({}{}):",
        response.error_logs.len(),
        truncated_note
    );
    for log in &response.error_logs {
        println!(
            "  [{}] {} {} {}",
            log.severity,
            local_ts(&log.timestamp),
            log.hostname,
            truncate(&log.message, 80)
        );
    }
    if !response.ai_sessions.is_empty() {
        println!("AI sessions ({}):", response.ai_sessions.len());
        for sess in &response.ai_sessions {
            println!(
                "  {}/{} {} {} → {}",
                sess.tool,
                truncate(&sess.project, 20),
                truncate(&sess.session_id, 16),
                sess.first_seen,
                sess.last_seen
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
        response.incidents.len(),
        response.total_incidents,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        },
        if response.candidate_window_truncated {
            format!(
                "\nwarning: candidate scan capped at {} rows; narrow with --project/--tool/--from/--to",
                response.candidate_cap
            )
        } else {
            String::new()
        }
    );
    for inc in &response.incidents {
        println!();
        println!(
            "incident {} score={:.1} [{}] project={} tool={} session={}",
            inc.incident_id,
            inc.priority_score,
            inc.priority_label,
            inc.project,
            inc.tool,
            inc.session_id,
        );
        println!(
            "  host={} first={} last={} duration={}s anchors={}",
            inc.hostname,
            local_ts(&inc.first_seen),
            local_ts(&inc.last_seen),
            inc.duration_secs,
            inc.abuse_count,
        );
        println!("  terms: {}", inc.terms.join(", "));
        println!("  anchor ids: {:?}", inc.anchor_ids);
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
        response.evidence.len(),
        response.total_incidents,
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
            inc.incident_id, inc.priority_label, inc.project, inc.tool, inc.session_id
        );
        println!(
            "  {} anchor(s), {} transcript-before{}, {} transcript-after{}, {} nearby log(s), {} nearby error(s)",
            ev.anchors.len(),
            ev.transcript_before.len(),
            if ev.transcript_before_truncated { " (trunc)" } else { "" },
            ev.transcript_after.len(),
            if ev.transcript_after_truncated { " (trunc)" } else { "" },
            ev.nearby_logs.len(),
            ev.nearby_errors.len(),
        );
        println!("  anchor messages:");
        for a in &ev.anchors {
            println!("    [{}] {}", local_ts(&a.timestamp), a.message);
        }
        if !ev.nearby_errors.is_empty() {
            println!("  nearby errors:");
            for e in &ev.nearby_errors {
                println!(
                    "    [{}] ({}) {}",
                    local_ts(&e.timestamp),
                    e.severity,
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
