use anyhow::Result;
use cortex::app::{ListHookEventsResponse, ListMcpEventsResponse, ListSkillEventsResponse};

use super::super::super::color::{cyan, muted, primary, violet};
use super::super::common::{local_ts, print_json};

/// `event.skill_name`/`event.skill_plugin` are printed directly here with no
/// additional sanitization. This is safe because
/// `ExtractedSkillEvent::normalized()` already rejects any skill name/plugin
/// containing a control character before it ever reaches the database, so by
/// the time a row gets here it cannot contain an ANSI escape or embedded
/// newline. Do not re-add sanitization here; the fix belongs at the
/// extraction boundary, not the printer.
pub(crate) fn print_skill_events_response(
    response: &ListSkillEventsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    if response.events.is_empty() {
        println!("No skill events found.");
        return Ok(());
    }
    println!(
        "{} event(s) shown{}",
        cyan(&response.events.len().to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for event in &response.events {
        let plugin = event
            .skill_plugin
            .as_deref()
            .map(|p| format!(" plugin={p}"))
            .unwrap_or_default();
        println!(
            "{}  {}{}  {}  tool={} project={}",
            muted(&local_ts(&event.timestamp)),
            violet(&event.skill_name),
            plugin,
            primary(&event.event_kind),
            cyan(&event.ai_tool),
            event.ai_project.as_deref().unwrap_or("-")
        );
    }
    if response.truncated {
        println!("{}", muted("(truncated — refine filters or raise --limit)"));
    }
    Ok(())
}

/// `event.tool_name`/`event.mcp_server`/`event.mcp_tool` are printed
/// directly here with no additional sanitization, matching
/// `print_skill_events_response`'s eng-review note: these values are
/// clamped/redacted at the extraction boundary
/// (`ExtractedMcpEvent`/`classify_tool_name` in
/// `crate::scanner::mcp_events`) before ever reaching the database, so by
/// the time a row gets here it cannot contain an ANSI escape or embedded
/// newline.
pub(crate) fn print_mcp_events_response(
    response: &ListMcpEventsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    if response.events.is_empty() {
        println!("No MCP events found.");
        return Ok(());
    }
    println!(
        "{} event(s) shown{}",
        cyan(&response.events.len().to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for event in &response.events {
        let server = event
            .mcp_server
            .as_deref()
            .map(|s| format!(" server={s}"))
            .unwrap_or_default();
        let tool = event
            .mcp_tool
            .as_deref()
            .map(|t| format!(" mcp_tool={t}"))
            .unwrap_or_default();
        let error_marker = if event.is_error == Some(true) {
            " [ERROR]"
        } else {
            ""
        };
        println!(
            "{}  {}{}{}  {}{}  tool={} project={}",
            muted(&local_ts(&event.timestamp)),
            violet(&event.tool_name),
            server,
            tool,
            primary(&event.event_kind),
            error_marker,
            cyan(&event.ai_tool),
            event.ai_project.as_deref().unwrap_or("-")
        );
    }
    if response.truncated {
        println!("{}", muted("(truncated — refine filters or raise --limit)"));
    }
    Ok(())
}

/// `event.hook_name`/`event.hook_source`/`event.hook_command` are printed
/// directly here with no additional sanitization, matching
/// `print_skill_events_response`'s eng-review note: `hook_command` is
/// redacted+control-char-checked at the extraction boundary
/// (`ExtractedHookEvent::normalized()` in `crate::scanner::hook_events`)
/// before ever reaching the database, so by the time a row gets here it
/// cannot contain an ANSI escape or embedded newline.
pub(crate) fn print_hook_events_response(
    response: &ListHookEventsResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    if response.events.is_empty() {
        println!("No hook events found.");
        return Ok(());
    }
    println!(
        "{} event(s) shown{}",
        cyan(&response.events.len().to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for event in &response.events {
        let name = event
            .hook_name
            .as_deref()
            .map(|n| format!(" hook={n}"))
            .unwrap_or_default();
        println!(
            "{}  {}{}  {}  evidence={}  tool={} project={}",
            muted(&local_ts(&event.timestamp)),
            violet(&event.hook_event),
            name,
            primary(&event.status),
            event.evidence_kind,
            cyan(&event.ai_tool),
            event.ai_project.as_deref().unwrap_or("-")
        );
    }
    if response.truncated {
        println!("{}", muted("(truncated — refine filters or raise --limit)"));
    }
    Ok(())
}
