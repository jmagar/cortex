use anyhow::{Result, bail};

use super::super::super::parse_common::{
    FlagCursor, norm_time, parse_positive_u64_flag, parse_u32_flag,
};
use super::super::super::{
    CliCommand, SessionsCommand, SessionsMcpEventsBackfillArgs, SessionsMcpEventsListArgs,
};

pub(crate) fn parse_sessions_mcp_events(args: &[String]) -> Result<CliCommand> {
    match args.first().map(String::as_str) {
        Some("backfill") => parse_mcp_events_backfill(&args[1..]),
        _ => parse_mcp_events_list(args),
    }
}

fn parse_mcp_events_list(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsMcpEventsListArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--tool-name" => parsed.tool_name = Some(flags.value("--tool-name")?),
            "--mcp-server" => parsed.mcp_server = Some(flags.value("--mcp-server")?),
            "--mcp-tool" => parsed.mcp_tool = Some(flags.value("--mcp-tool")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--session-id" => parsed.session_id = Some(flags.value("--session-id")?),
            "--host" => parsed.host = Some(flags.value("--host")?),
            "--error-only" => parsed.is_error = Some(true),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ => bail!("unknown sessions mcpevents option: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::McpEvents(parsed)))
}

fn parse_mcp_events_backfill(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsMcpEventsBackfillArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--dry-run" => parsed.dry_run = true,
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--limit" => {
                parsed.limit = Some(parse_positive_u64_flag("--limit", flags.value("--limit")?)?)
            }
            _ => bail!("unknown sessions mcpevents backfill option: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::McpEventsBackfill(
        parsed,
    )))
}
