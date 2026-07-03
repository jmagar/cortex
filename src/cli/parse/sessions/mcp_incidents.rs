use anyhow::{Result, bail};

use super::super::super::parse_common::{FlagCursor, norm_time, parse_u32_flag};
use super::super::super::{
    CliCommand, SessionsCommand, SessionsMcpIncidentsArgs, SessionsMcpInvestigateArgs,
};

pub(crate) fn parse_sessions_mcp_incidents(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsMcpIncidentsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--mcp-server" => parsed.mcp_server = Some(flags.value("--mcp-server")?),
            "--mcp-tool" => parsed.mcp_tool = Some(flags.value("--mcp-tool")?),
            "--tool-name" => parsed.tool_name = Some(flags.value("--tool-name")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--session-id" => parsed.session_id = Some(flags.value("--session-id")?),
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--window-minutes" => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--signal" => parsed.signals.push(flags.value("--signal")?),
            "--min-score" => parsed.min_score = Some(flags.value("--min-score")?),
            _ if arg.starts_with('-') => bail!("unknown sessions mcp-incidents option: {arg}"),
            _ if parsed.mcp_server.is_none() => parsed.mcp_server = Some(arg),
            _ => bail!("unexpected sessions mcp-incidents argument: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::McpIncidents(parsed)))
}

pub(crate) fn parse_sessions_mcp_investigate(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsMcpInvestigateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--all" => parsed.all = true,
            "--incident-id" => parsed.incident_id = Some(flags.value("--incident-id")?),
            "--mcp-server" => parsed.mcp_server = Some(flags.value("--mcp-server")?),
            "--mcp-tool" => parsed.mcp_tool = Some(flags.value("--mcp-tool")?),
            "--tool-name" => parsed.tool_name = Some(flags.value("--tool-name")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--window-minutes" => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--correlation-window-minutes" => {
                parsed.correlation_window_minutes = Some(parse_u32_flag(
                    "--correlation-window-minutes",
                    flags.value("--correlation-window-minutes")?,
                )?)
            }
            _ if arg.starts_with('-') => {
                bail!("unknown sessions mcp-investigate option: {arg}")
            }
            // Bare positional binds to the target (server/tool/name).
            _ if parsed.target.is_none() => parsed.target = Some(arg),
            _ => bail!("unexpected sessions mcp-investigate argument: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::McpInvestigate(
        parsed,
    )))
}
