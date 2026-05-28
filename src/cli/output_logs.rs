use anyhow::Result;
use syslog_mcp::app::{
    AbuseSearchResponse, AiCorrelateResponse, CorrelateEventsResponse, DbStats, GetErrorsResponse,
    ListAiProjectsResponse, ListAiToolsResponse, ListHostsResponse, ProjectContextResponse,
    SearchLogsResponse, SearchSessionsResponse, UsageBlocksResponse,
};

use super::color::Palette;
use super::output_common::{local_ts, print_json, print_log, truncate};

pub(crate) fn print_search_response(response: &SearchLogsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!("{} log(s)", p.cyan(&response.count.to_string()));
    for log in &response.logs {
        print_log(log);
    }
    Ok(())
}

pub(crate) fn print_errors_response(response: &GetErrorsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!("{}", p.muted("HOST                 SEVERITY COUNT"));
    for row in &response.summary {
        println!(
            "{:<20} {:<8} {}",
            p.cyan(&row.hostname),
            p.severity(&row.severity),
            p.cyan(&row.count.to_string())
        );
    }
    Ok(())
}

pub(crate) fn print_hosts_response(response: &ListHostsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!("{}", p.muted("HOST                 COUNT LAST SEEN"));
    for host in &response.hosts {
        println!(
            "{:<20} {:<5} {}",
            p.cyan(&host.hostname),
            p.cyan(&host.log_count.to_string()),
            p.muted(&local_ts(&host.last_seen))
        );
    }
    Ok(())
}

pub(crate) fn print_sessions_response(
    response: &syslog_mcp::app::ListSessionsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!("{} session(s)", p.cyan(&response.count.to_string()));
    println!(
        "{}",
        p.muted(&format!(
            "{:<40} {:<10} {:<36} {:<15} COUNT",
            "PROJECT", "TOOL", "SESSION ID", "HOST"
        ))
    );
    for s in &response.sessions {
        println!(
            "{:<40} {:<10} {:<36} {:<15} {}",
            p.primary(&truncate(&s.project, 39)),
            p.violet(&s.tool),
            p.muted(&s.session_id),
            p.cyan(&s.hostname),
            p.cyan(&s.event_count.to_string())
        );
    }
    Ok(())
}

pub(crate) fn print_search_sessions_response(
    response: &SearchSessionsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "{} grouped session(s) from {} newest matching row(s){}",
        p.cyan(&response.sessions.len().to_string()),
        p.cyan(&response.candidate_rows.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    if response.candidate_window_truncated {
        println!(
            "search window capped at {} matching rows; use --project, --tool, --from, or --to to narrow exact grouping",
            p.cyan(&response.candidate_cap.to_string())
        );
    }
    println!(
        "{}",
        p.muted(&format!(
            "{:<10} {:<30} {:<20} {:<6} MATCH",
            "TOOL", "PROJECT", "SESSION ID", "EVENTS"
        ))
    );
    for session in &response.sessions {
        println!(
            "{:<10} {:<30} {:<20} {:<6} {}",
            p.violet(&session.tool),
            p.primary(&truncate(&session.project, 29)),
            p.muted(&truncate(&session.session_id, 19)),
            p.cyan(&session.event_count.to_string()),
            p.cyan(&session.match_count.to_string())
        );
    }
    Ok(())
}

pub(crate) fn print_abuse_search_response(
    response: &AbuseSearchResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "{} abuse match(es) from {} candidate row(s){}",
        p.cyan(&response.matches.len().to_string()),
        p.cyan(&response.candidate_rows.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    if response.candidate_window_truncated {
        println!(
            "abuse scan capped at {} candidate rows; use --project, --tool, --from, or --to to narrow it",
            p.cyan(&response.candidate_cap.to_string())
        );
    }
    println!(
        "{}: {}",
        p.muted("terms"),
        p.primary(&response.terms.join(", "))
    );
    for item in &response.matches {
        println!();
        println!(
            "match term={} id={} {}",
            p.primary(&item.term),
            p.cyan(&item.entry.id.to_string()),
            p.muted(&local_ts(&item.entry.timestamp))
        );
        for before in &item.before {
            println!("  {}:", p.muted("before"));
            print_log(before);
        }
        println!("  {}:", p.muted("hit"));
        print_log(&item.entry);
        for after in &item.after {
            println!("  {}:", p.muted("after"));
            print_log(after);
        }
    }
    Ok(())
}

pub(crate) fn print_ai_correlate_response(
    response: &AiCorrelateResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "{} AI anchor(s), {} related non-AI event(s), +/-{}m, severity >= {}{}",
        p.cyan(&response.total_anchors.to_string()),
        p.cyan(&response.total_related_events.to_string()),
        p.cyan(&response.window_minutes.to_string()),
        p.primary(&response.severity_min),
        if response.anchors_truncated {
            " (anchors truncated)"
        } else {
            ""
        }
    );
    for anchor in &response.anchors {
        println!();
        println!(
            "AI anchor id={} {} window={}..{}{}",
            p.cyan(&anchor.entry.id.to_string()),
            p.muted(&local_ts(&anchor.entry.timestamp)),
            p.muted(&local_ts(&anchor.window_from)),
            p.muted(&local_ts(&anchor.window_to)),
            if anchor.related_truncated {
                " (related truncated)"
            } else {
                ""
            }
        );
        print_log(&anchor.entry);
        for log in &anchor.related {
            print_log(log);
        }
    }
    Ok(())
}

pub(crate) fn print_usage_blocks_response(
    response: &UsageBlocksResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "{} usage block(s) shown of {}{}",
        p.cyan(&response.blocks.len().to_string()),
        p.cyan(&response.total_blocks.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for block in &response.blocks {
        println!(
            "{} {} {} {} events={} sessions={}",
            p.muted(&block.bucket_start),
            p.muted(&block.bucket_end),
            p.violet(&block.tool),
            p.primary(&truncate(&block.project, 30)),
            p.cyan(&block.event_count.to_string()),
            p.cyan(&block.session_count.to_string())
        );
    }
    Ok(())
}

pub(crate) fn print_project_context_response(
    response: &ProjectContextResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!("{}: {}", p.muted("project"), p.primary(&response.project));
    println!(
        "{}: {}",
        p.muted("event_count"),
        p.cyan(&response.event_count.to_string())
    );
    println!(
        "{}: {}",
        p.muted("tools"),
        p.violet(&response.tools.join(", "))
    );
    println!(
        "{}: {}",
        p.muted("sessions"),
        p.cyan(&response.sessions.len().to_string())
    );
    println!(
        "{}: {}",
        p.muted("hosts"),
        p.cyan(&response.hostnames.join(", "))
    );
    println!(
        "{}: {}{}",
        p.muted("recent_entries"),
        p.cyan(&response.recent_entries.len().to_string()),
        if response.recent_entries_truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for entry in &response.recent_entries {
        print_log(entry);
    }
    Ok(())
}

pub(crate) fn print_ai_tools_response(response: &ListAiToolsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "{} tool(s) shown of {}{}",
        p.cyan(&response.tools.len().to_string()),
        p.cyan(&response.total_tools.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    println!("{}", p.muted("TOOL       EVENTS SESSIONS LAST SEEN"));
    for tool in &response.tools {
        println!(
            "{:<10} {:<6} {:<8} {}",
            p.violet(&tool.tool),
            p.cyan(&tool.event_count.to_string()),
            p.cyan(&tool.session_count.to_string()),
            p.muted(&local_ts(&tool.last_seen))
        );
    }
    Ok(())
}

pub(crate) fn print_ai_projects_response(
    response: &ListAiProjectsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "{} project(s) shown of {}{}",
        p.cyan(&response.projects.len().to_string()),
        p.cyan(&response.total_projects.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    println!(
        "{}",
        p.muted("PROJECT                          EVENTS SESSIONS TOOLS")
    );
    for project in &response.projects {
        println!(
            "{:<32} {:<6} {:<8} {}",
            p.primary(&truncate(&project.project, 32)),
            p.cyan(&project.event_count.to_string()),
            p.cyan(&project.session_count.to_string()),
            p.violet(&project.tools.join(","))
        );
    }
    Ok(())
}

pub(crate) fn print_correlate_response(
    response: &CorrelateEventsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "{} event(s) across {} host(s), window {} to {}, severity >= {}{}",
        p.cyan(&response.total_events.to_string()),
        p.cyan(&response.hosts_count.to_string()),
        p.muted(&response.window_from),
        p.muted(&response.window_to),
        p.primary(&response.severity_min),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for host in &response.hosts {
        println!();
        println!(
            "{} ({} event(s))",
            p.cyan(&host.hostname),
            p.cyan(&host.event_count.to_string())
        );
        for log in &host.events {
            print_log(log);
        }
    }
    Ok(())
}

pub(crate) fn print_stats_response(stats: &DbStats, json: bool) -> Result<()> {
    if json {
        return print_json(stats);
    }
    let p = Palette::new();
    println!(
        "{}: {}",
        p.muted("total_logs"),
        p.cyan(&stats.total_logs.to_string())
    );
    println!(
        "{}: {}",
        p.muted("total_hosts"),
        p.cyan(&stats.total_hosts.to_string())
    );
    println!(
        "{}: {}",
        p.muted("oldest_log"),
        p.primary(stats.oldest_log.as_deref().unwrap_or("-"))
    );
    println!(
        "{}: {}",
        p.muted("newest_log"),
        p.primary(stats.newest_log.as_deref().unwrap_or("-"))
    );
    println!(
        "{}: {}",
        p.muted("logical_db_size_mb"),
        p.cyan(&stats.logical_db_size_mb.to_string())
    );
    println!(
        "{}: {}",
        p.muted("physical_db_size_mb"),
        p.cyan(&stats.physical_db_size_mb.to_string())
    );
    println!(
        "{}: {}",
        p.muted("free_disk_mb"),
        p.primary(stats.free_disk_mb.as_deref().unwrap_or("-"))
    );
    println!(
        "{}: {}",
        p.muted("max_db_size_mb"),
        p.cyan(&stats.max_db_size_mb.to_string())
    );
    println!(
        "{}: {}",
        p.muted("min_free_disk_mb"),
        p.cyan(&stats.min_free_disk_mb.to_string())
    );
    println!(
        "{}: {}",
        p.muted("write_blocked"),
        p.primary(&stats.write_blocked.to_string())
    );
    println!(
        "{}: {}",
        p.muted("phantom_fts_rows"),
        p.cyan(&stats.phantom_fts_rows.to_string())
    );
    Ok(())
}

#[cfg(test)]
#[path = "output_logs_tests.rs"]
mod tests;
