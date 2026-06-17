use anyhow::{Result, anyhow, bail};

use super::parse_common::{FlagCursor, parse_u32_flag, value_after_equals};
use super::{
    AiAskHistoryArgs, AiAssessArgs, AiCommand, AiIncidentContextArgs, AiIncidentsArgs,
    AiInvestigateArgs, AiOutputDetail, AiSimilarArgs, CliCommand,
};
pub(crate) fn parse_ai_similar(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiSimilarArgs::default();
    let mut query_parts = Vec::new();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--host" => parsed.host = Some(flags.value("--host")?),
            "--app" => parsed.app = Some(flags.value("--app")?),
            "--severity-min" => parsed.severity_min = Some(flags.value("--severity-min")?),
            "--since" => parsed.since = Some(flags.value("--since")?),
            "--until" => parsed.until = Some(flags.value("--until")?),
            "--window-minutes" => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--host=") => {
                parsed.host = Some(value_after_equals(arg, "--host")?)
            }
            _ if arg.starts_with("--app=") => parsed.app = Some(value_after_equals(arg, "--app")?),
            _ if arg.starts_with("--severity-min=") => {
                parsed.severity_min = Some(value_after_equals(arg, "--severity-min")?)
            }
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(value_after_equals(arg, "--since")?)
            }
            _ if arg.starts_with("--until=") => {
                parsed.until = Some(value_after_equals(arg, "--until")?)
            }
            _ if arg.starts_with("--window-minutes=") => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    value_after_equals(arg, "--window-minutes")?,
                )?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown ai similar option: {arg}"),
            _ => query_parts.push(arg),
        }
    }
    parsed.query = query_parts.join(" ");
    if parsed.query.is_empty() {
        bail!("ai similar requires a query");
    }
    Ok(CliCommand::Ai(AiCommand::SimilarIncidents(parsed)))
}

pub(crate) fn parse_ai_ask_history(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiAskHistoryArgs::default();
    let mut query_parts = Vec::new();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--host" => parsed.host = Some(flags.value("--host")?),
            "--app" => parsed.app = Some(flags.value("--app")?),
            "--since" => parsed.since = Some(flags.value("--since")?),
            "--until" => parsed.until = Some(flags.value("--until")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--host=") => {
                parsed.host = Some(value_after_equals(arg, "--host")?)
            }
            _ if arg.starts_with("--app=") => parsed.app = Some(value_after_equals(arg, "--app")?),
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(value_after_equals(arg, "--since")?)
            }
            _ if arg.starts_with("--until=") => {
                parsed.until = Some(value_after_equals(arg, "--until")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown ai ask-history option: {arg}"),
            _ => query_parts.push(arg),
        }
    }
    parsed.query = query_parts.join(" ");
    if parsed.query.is_empty() {
        bail!("ai ask-history requires a query");
    }
    Ok(CliCommand::Ai(AiCommand::AskHistory(parsed)))
}

pub(crate) fn parse_ai_incident_context(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiIncidentContextArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--since" => parsed.since = flags.value("--since")?,
            "--until" => parsed.until = flags.value("--until")?,
            "--host" => parsed.host = Some(flags.value("--host")?),
            "--app" => parsed.app = Some(flags.value("--app")?),
            "--query" => parsed.query = Some(flags.value("--query")?),
            "--severity-min" => parsed.severity_min = Some(flags.value("--severity-min")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--since=") => parsed.since = value_after_equals(arg, "--since")?,
            _ if arg.starts_with("--until=") => parsed.until = value_after_equals(arg, "--until")?,
            _ if arg.starts_with("--host=") => {
                parsed.host = Some(value_after_equals(arg, "--host")?)
            }
            _ if arg.starts_with("--app=") => parsed.app = Some(value_after_equals(arg, "--app")?),
            _ if arg.starts_with("--query=") => {
                parsed.query = Some(value_after_equals(arg, "--query")?)
            }
            _ if arg.starts_with("--severity-min=") => {
                parsed.severity_min = Some(value_after_equals(arg, "--severity-min")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown ai incident-context option: {arg}"),
            _ => bail!("unexpected positional argument for ai incident-context: {arg}"),
        }
    }
    if parsed.since.is_empty() {
        bail!("ai incident-context requires --since");
    }
    if parsed.until.is_empty() {
        bail!("ai incident-context requires --until");
    }
    Ok(CliCommand::Ai(AiCommand::IncidentContext(parsed)))
}

pub(crate) fn parse_ai_incidents(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiIncidentsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--since" => parsed.since = Some(flags.value("--since")?),
            "--until" => parsed.until = Some(flags.value("--until")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--window-minutes" => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--term" => parsed.terms.push(flags.value("--term")?),
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(value_after_equals(arg, "--since")?)
            }
            _ if arg.starts_with("--until=") => {
                parsed.until = Some(value_after_equals(arg, "--until")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with("--window-minutes=") => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    value_after_equals(arg, "--window-minutes")?,
                )?)
            }
            _ if arg.starts_with("--term=") => {
                parsed.terms.push(value_after_equals(arg, "--term")?)
            }
            _ if arg.starts_with('-') => bail!("unknown ai incidents option: {arg}"),
            _ => bail!("unexpected ai incidents argument: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Incidents(parsed)))
}

pub(crate) fn parse_ai_investigate(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiInvestigateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--since" => parsed.since = Some(flags.value("--since")?),
            "--until" => parsed.until = Some(flags.value("--until")?),
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
            "--term" => parsed.terms.push(flags.value("--term")?),
            "--detail" => {
                parsed.detail = AiOutputDetail::parse(&flags.value("--detail")?, "--detail")?
            }
            "--include-transcript" => parsed.include_transcript = true,
            "--max-bytes" => {
                parsed.max_bytes =
                    Some(parse_u32_flag("--max-bytes", flags.value("--max-bytes")?)? as usize)
            }
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(value_after_equals(arg, "--since")?)
            }
            _ if arg.starts_with("--until=") => {
                parsed.until = Some(value_after_equals(arg, "--until")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with("--window-minutes=") => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    value_after_equals(arg, "--window-minutes")?,
                )?)
            }
            _ if arg.starts_with("--correlation-window-minutes=") => {
                parsed.correlation_window_minutes = Some(parse_u32_flag(
                    "--correlation-window-minutes",
                    value_after_equals(arg, "--correlation-window-minutes")?,
                )?)
            }
            _ if arg.starts_with("--term=") => {
                parsed.terms.push(value_after_equals(arg, "--term")?)
            }
            _ if arg.starts_with("--detail=") => {
                parsed.detail =
                    AiOutputDetail::parse(&value_after_equals(arg, "--detail")?, "--detail")?
            }
            _ if arg.starts_with("--max-bytes=") => {
                parsed.max_bytes = Some(parse_u32_flag(
                    "--max-bytes",
                    value_after_equals(arg, "--max-bytes")?,
                )? as usize)
            }
            _ if arg.starts_with('-') => bail!(
                "{}",
                super::suggest::unknown_option(
                    "ai investigate",
                    &arg,
                    &[
                        "--json",
                        "--project",
                        "--tool",
                        "--since",
                        "--until",
                        "--limit",
                        "--window-minutes",
                        "--correlation-window-minutes",
                        "--term",
                        "--detail",
                        "--include-transcript",
                        "--max-bytes",
                    ],
                )
            ),
            _ => bail!("unexpected ai investigate argument: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Investigate(parsed)))
}

pub(crate) fn parse_ai_assess(args: &[String]) -> Result<CliCommand> {
    let mut incident_id: Option<String> = None;
    let mut model: Option<String> = None;
    let mut json = false;
    let mut project: Option<String> = None;
    let mut tool: Option<String> = None;
    let mut from: Option<String> = None;
    let mut to: Option<String> = None;
    let mut limit: Option<u32> = None;
    let mut window_minutes: Option<u32> = None;
    let mut correlation_window_minutes: Option<u32> = None;
    let mut terms: Vec<String> = Vec::new();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--model" => model = Some(flags.value("--model")?),
            "--project" => project = Some(flags.value("--project")?),
            "--tool" => tool = Some(flags.value("--tool")?),
            "--since" => from = Some(flags.value("--since")?),
            "--until" => to = Some(flags.value("--until")?),
            "--limit" => limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--window-minutes" => {
                window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--correlation-window-minutes" => {
                correlation_window_minutes = Some(parse_u32_flag(
                    "--correlation-window-minutes",
                    flags.value("--correlation-window-minutes")?,
                )?)
            }
            "--term" => terms.push(flags.value("--term")?),
            _ if arg.starts_with("--model=") => model = Some(value_after_equals(arg, "--model")?),
            _ if arg.starts_with("--project=") => {
                project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => tool = Some(value_after_equals(arg, "--tool")?),
            _ if arg.starts_with("--since=") => from = Some(value_after_equals(arg, "--since")?),
            _ if arg.starts_with("--until=") => to = Some(value_after_equals(arg, "--until")?),
            _ if arg.starts_with("--limit=") => {
                limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with("--window-minutes=") => {
                window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    value_after_equals(arg, "--window-minutes")?,
                )?)
            }
            _ if arg.starts_with("--correlation-window-minutes=") => {
                correlation_window_minutes = Some(parse_u32_flag(
                    "--correlation-window-minutes",
                    value_after_equals(arg, "--correlation-window-minutes")?,
                )?)
            }
            _ if arg.starts_with("--term=") => terms.push(value_after_equals(arg, "--term")?),
            _ if arg.starts_with('-') => bail!("unknown ai assess option: {arg}"),
            _ => {
                if incident_id.is_some() {
                    bail!("ai assess: unexpected extra argument: {arg}");
                }
                incident_id = Some(arg);
            }
        }
    }
    let incident_id =
        incident_id.ok_or_else(|| anyhow!("ai assess requires an <incident_id> argument"))?;
    Ok(CliCommand::Ai(AiCommand::Assess(AiAssessArgs {
        incident_id,
        model,
        json,
        project,
        tool,
        since: from,
        until: to,
        window_minutes,
        correlation_window_minutes,
        terms,
        limit,
    })))
}

#[cfg(test)]
#[path = "parse_ai_more_tests.rs"]
mod tests;
