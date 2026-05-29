use anyhow::Result;
use syslog_mcp::app::{
    AbuseSearchResponse, AiCorrelateResponse, CorrelateEventsResponse, DbStats, GetErrorsResponse,
    ListAiProjectsResponse, ListAiToolsResponse, ListHostsResponse, ProjectContextResponse,
    SearchLogsResponse, SearchSessionsResponse, UsageBlocksResponse,
};

use super::color::{cyan, muted, primary, severity, violet};
use super::output_common::{local_ts, print_json, print_log, truncate};

pub(crate) fn print_search_response(response: &SearchLogsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("{} log(s)", cyan(&response.count.to_string()));
    for log in &response.logs {
        print_log(log);
    }
    Ok(())
}

pub(crate) fn print_errors_response(response: &GetErrorsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("{}", muted("HOST                 SEVERITY COUNT"));
    for row in &response.summary {
        println!(
            "{:<20} {:<8} {}",
            cyan(&row.hostname),
            severity(&row.severity),
            cyan(&row.count.to_string())
        );
    }
    Ok(())
}

pub(crate) fn print_hosts_response(response: &ListHostsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("{}", muted("HOST                 COUNT LAST SEEN"));
    for host in &response.hosts {
        println!(
            "{:<20} {:<5} {}",
            cyan(&host.hostname),
            cyan(&host.log_count.to_string()),
            muted(&local_ts(&host.last_seen))
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
    println!("{} session(s)", cyan(&response.count.to_string()));
    println!(
        "{}",
        muted(&format!(
            "{:<40} {:<10} {:<36} {:<15} COUNT",
            "PROJECT", "TOOL", "SESSION ID", "HOST"
        ))
    );
    for s in &response.sessions {
        println!(
            "{:<40} {:<10} {:<36} {:<15} {}",
            primary(&truncate(&s.project, 39)),
            violet(&s.tool),
            muted(&s.session_id),
            cyan(&s.hostname),
            cyan(&s.event_count.to_string())
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
    println!(
        "{} grouped session(s) from {} newest matching row(s){}",
        cyan(&response.sessions.len().to_string()),
        cyan(&response.candidate_rows.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    if response.candidate_window_truncated {
        println!(
            "search window capped at {} matching rows; use --project, --tool, --from, or --to to narrow exact grouping",
            cyan(&response.candidate_cap.to_string())
        );
    }
    println!(
        "{}",
        muted(&format!(
            "{:<10} {:<30} {:<20} {:<6} MATCH",
            "TOOL", "PROJECT", "SESSION ID", "EVENTS"
        ))
    );
    for session in &response.sessions {
        println!(
            "{:<10} {:<30} {:<20} {:<6} {}",
            violet(&session.tool),
            primary(&truncate(&session.project, 29)),
            muted(&truncate(&session.session_id, 19)),
            cyan(&session.event_count.to_string()),
            cyan(&session.match_count.to_string())
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
    println!(
        "{} abuse match(es) from {} candidate row(s){}",
        cyan(&response.matches.len().to_string()),
        cyan(&response.candidate_rows.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    if response.candidate_window_truncated {
        println!(
            "abuse scan capped at {} candidate rows; use --project, --tool, --from, or --to to narrow it",
            cyan(&response.candidate_cap.to_string())
        );
    }
    println!(
        "{}: {}",
        muted("terms"),
        primary(&response.terms.join(", "))
    );
    for item in &response.matches {
        println!();
        println!(
            "match term={} id={} {}",
            primary(&item.term),
            cyan(&item.entry.id.to_string()),
            muted(&local_ts(&item.entry.timestamp))
        );
        for before in &item.before {
            println!("  {}:", muted("before"));
            print_log(before);
        }
        println!("  {}:", muted("hit"));
        print_log(&item.entry);
        for after in &item.after {
            println!("  {}:", muted("after"));
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
    println!(
        "{} AI anchor(s), {} related non-AI event(s), +/-{}m, severity >= {}{}",
        cyan(&response.total_anchors.to_string()),
        cyan(&response.total_related_events.to_string()),
        cyan(&response.window_minutes.to_string()),
        primary(&response.severity_min),
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
            cyan(&anchor.entry.id.to_string()),
            muted(&local_ts(&anchor.entry.timestamp)),
            muted(&local_ts(&anchor.window_from)),
            muted(&local_ts(&anchor.window_to)),
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
    println!(
        "{} usage block(s) shown of {}{}",
        cyan(&response.blocks.len().to_string()),
        cyan(&response.total_blocks.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for block in &response.blocks {
        println!(
            "{} {} {} {} events={} sessions={}",
            muted(&block.bucket_start),
            muted(&block.bucket_end),
            violet(&block.tool),
            primary(&truncate(&block.project, 30)),
            cyan(&block.event_count.to_string()),
            cyan(&block.session_count.to_string())
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
    println!("{}: {}", muted("project"), primary(&response.project));
    println!(
        "{}: {}",
        muted("event_count"),
        cyan(&response.event_count.to_string())
    );
    println!("{}: {}", muted("tools"), violet(&response.tools.join(", ")));
    println!(
        "{}: {}",
        muted("sessions"),
        cyan(&response.sessions.len().to_string())
    );
    println!(
        "{}: {}",
        muted("hosts"),
        cyan(&response.hostnames.join(", "))
    );
    println!(
        "{}: {}{}",
        muted("recent_entries"),
        cyan(&response.recent_entries.len().to_string()),
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
    println!(
        "{} tool(s) shown of {}{}",
        cyan(&response.tools.len().to_string()),
        cyan(&response.total_tools.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    println!("{}", muted("TOOL       EVENTS SESSIONS LAST SEEN"));
    for tool in &response.tools {
        println!(
            "{:<10} {:<6} {:<8} {}",
            violet(&tool.tool),
            cyan(&tool.event_count.to_string()),
            cyan(&tool.session_count.to_string()),
            muted(&local_ts(&tool.last_seen))
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
    println!(
        "{} project(s) shown of {}{}",
        cyan(&response.projects.len().to_string()),
        cyan(&response.total_projects.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    println!(
        "{}",
        muted("PROJECT                          EVENTS SESSIONS TOOLS")
    );
    for project in &response.projects {
        println!(
            "{:<32} {:<6} {:<8} {}",
            primary(&truncate(&project.project, 32)),
            cyan(&project.event_count.to_string()),
            cyan(&project.session_count.to_string()),
            violet(&project.tools.join(","))
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
    println!(
        "{} event(s) across {} host(s), window {} to {}, severity >= {}{}",
        cyan(&response.total_events.to_string()),
        cyan(&response.hosts_count.to_string()),
        muted(&response.window_from),
        muted(&response.window_to),
        primary(&response.severity_min),
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
            cyan(&host.hostname),
            cyan(&host.event_count.to_string())
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
    println!(
        "{}: {}",
        muted("total_logs"),
        cyan(&stats.total_logs.to_string())
    );
    println!(
        "{}: {}",
        muted("total_hosts"),
        cyan(&stats.total_hosts.to_string())
    );
    println!(
        "{}: {}",
        muted("oldest_log"),
        primary(stats.oldest_log.as_deref().unwrap_or("-"))
    );
    println!(
        "{}: {}",
        muted("newest_log"),
        primary(stats.newest_log.as_deref().unwrap_or("-"))
    );
    println!(
        "{}: {}",
        muted("logical_db_size_mb"),
        cyan(&stats.logical_db_size_mb.to_string())
    );
    println!(
        "{}: {}",
        muted("physical_db_size_mb"),
        cyan(&stats.physical_db_size_mb.to_string())
    );
    println!(
        "{}: {}",
        muted("free_disk_mb"),
        primary(stats.free_disk_mb.as_deref().unwrap_or("-"))
    );
    println!(
        "{}: {}",
        muted("max_db_size_mb"),
        cyan(&stats.max_db_size_mb.to_string())
    );
    println!(
        "{}: {}",
        muted("min_free_disk_mb"),
        cyan(&stats.min_free_disk_mb.to_string())
    );
    println!(
        "{}: {}",
        muted("write_blocked"),
        primary(&stats.write_blocked.to_string())
    );
    println!(
        "{}: {}",
        muted("phantom_fts_rows"),
        cyan(&stats.phantom_fts_rows.to_string())
    );
    Ok(())
}

#[cfg(test)]
#[path = "output_logs_tests.rs"]
mod tests;
