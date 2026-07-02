use anyhow::{Result, anyhow, bail};

use super::super::super::parse_common::{
    FlagCursor, norm_time, parse_i64_flag, parse_u32_flag, value_after_equals,
};
use super::super::super::{
    CliCommand, SessionsAskHistoryArgs, SessionsAssessArgs, SessionsCommand,
    SessionsIncidentContextArgs, SessionsIncidentsArgs, SessionsInvestigateArgs,
    SessionsLlmInvocationsArgs, SessionsOutputDetail, SessionsSimilarArgs,
};
pub(crate) fn parse_sessions_similar(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsSimilarArgs::default();
    let mut query_parts = Vec::new();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--host" => parsed.host = Some(flags.value("--host")?),
            "--app" => parsed.app = Some(flags.value("--app")?),
            "--severity-min" => parsed.severity_min = Some(flags.value("--severity-min")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
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
                parsed.since = Some(norm_time(value_after_equals(arg, "--since")?)?)
            }
            _ if arg.starts_with("--until=") => {
                parsed.until = Some(norm_time(value_after_equals(arg, "--until")?)?)
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
            _ if arg.starts_with('-') => bail!("unknown sessions similar option: {arg}"),
            _ => query_parts.push(arg),
        }
    }
    parsed.query = query_parts.join(" ");
    if parsed.query.is_empty() {
        bail!("sessions similar requires a query");
    }
    Ok(CliCommand::Sessions(SessionsCommand::SimilarIncidents(
        parsed,
    )))
}

pub(crate) fn parse_sessions_ask_history(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsAskHistoryArgs::default();
    let mut query_parts = Vec::new();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--host" => parsed.host = Some(flags.value("--host")?),
            "--app" => parsed.app = Some(flags.value("--app")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--host=") => {
                parsed.host = Some(value_after_equals(arg, "--host")?)
            }
            _ if arg.starts_with("--app=") => parsed.app = Some(value_after_equals(arg, "--app")?),
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(norm_time(value_after_equals(arg, "--since")?)?)
            }
            _ if arg.starts_with("--until=") => {
                parsed.until = Some(norm_time(value_after_equals(arg, "--until")?)?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown sessions ask-history option: {arg}"),
            _ => query_parts.push(arg),
        }
    }
    parsed.query = query_parts.join(" ");
    if parsed.query.is_empty() {
        bail!("sessions ask-history requires a query");
    }
    Ok(CliCommand::Sessions(SessionsCommand::AskHistory(parsed)))
}

pub(crate) fn parse_sessions_incident_context(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsIncidentContextArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--since" => parsed.since = norm_time(flags.value("--since")?)?,
            "--until" => parsed.until = norm_time(flags.value("--until")?)?,
            "--host" => parsed.host = Some(flags.value("--host")?),
            "--app" => parsed.app = Some(flags.value("--app")?),
            "--query" => parsed.query = Some(flags.value("--query")?),
            "--severity-min" => parsed.severity_min = Some(flags.value("--severity-min")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--since=") => {
                parsed.since = norm_time(value_after_equals(arg, "--since")?)?
            }
            _ if arg.starts_with("--until=") => {
                parsed.until = norm_time(value_after_equals(arg, "--until")?)?
            }
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
            _ if arg.starts_with('-') => bail!("unknown sessions incident-context option: {arg}"),
            _ => bail!("unexpected positional argument for ai incident-context: {arg}"),
        }
    }
    if parsed.since.is_empty() {
        bail!("sessions incident-context requires --since");
    }
    if parsed.until.is_empty() {
        bail!("sessions incident-context requires --until");
    }
    Ok(CliCommand::Sessions(SessionsCommand::IncidentContext(
        parsed,
    )))
}

pub(crate) fn parse_sessions_incidents(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsIncidentsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
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
                parsed.since = Some(norm_time(value_after_equals(arg, "--since")?)?)
            }
            _ if arg.starts_with("--until=") => {
                parsed.until = Some(norm_time(value_after_equals(arg, "--until")?)?)
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
            _ if arg.starts_with('-') => bail!("unknown sessions incidents option: {arg}"),
            _ => bail!("unexpected sessions incidents argument: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Incidents(parsed)))
}

pub(crate) fn parse_sessions_investigate(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsInvestigateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
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
            "--term" => parsed.terms.push(flags.value("--term")?),
            "--detail" => {
                parsed.detail = SessionsOutputDetail::parse(&flags.value("--detail")?, "--detail")?
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
                parsed.since = Some(norm_time(value_after_equals(arg, "--since")?)?)
            }
            _ if arg.starts_with("--until=") => {
                parsed.until = Some(norm_time(value_after_equals(arg, "--until")?)?)
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
                    SessionsOutputDetail::parse(&value_after_equals(arg, "--detail")?, "--detail")?
            }
            _ if arg.starts_with("--max-bytes=") => {
                parsed.max_bytes = Some(parse_u32_flag(
                    "--max-bytes",
                    value_after_equals(arg, "--max-bytes")?,
                )? as usize)
            }
            _ if arg.starts_with('-') => bail!(
                "{}",
                super::super::super::suggest::unknown_option(
                    "sessions investigate",
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
            _ => bail!("unexpected sessions investigate argument: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Investigate(parsed)))
}

pub(crate) fn parse_sessions_assess(args: &[String]) -> Result<CliCommand> {
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
    let mut dry_run = false;
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--dry-run" => dry_run = true,
            "--model" => model = Some(flags.value("--model")?),
            "--project" => project = Some(flags.value("--project")?),
            "--tool" => tool = Some(flags.value("--tool")?),
            "--since" => from = Some(norm_time(flags.value("--since")?)?),
            "--until" => to = Some(norm_time(flags.value("--until")?)?),
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
            _ if arg.starts_with("--since=") => {
                from = Some(norm_time(value_after_equals(arg, "--since")?)?)
            }
            _ if arg.starts_with("--until=") => {
                to = Some(norm_time(value_after_equals(arg, "--until")?)?)
            }
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
            _ if arg.starts_with('-') => bail!("unknown sessions assess option: {arg}"),
            _ => {
                if incident_id.is_some() {
                    bail!("sessions assess: unexpected extra argument: {arg}");
                }
                incident_id = Some(arg);
            }
        }
    }
    let incident_id =
        incident_id.ok_or_else(|| anyhow!("sessions assess requires an <incident_id> argument"))?;
    Ok(CliCommand::Sessions(SessionsCommand::Assess(
        SessionsAssessArgs {
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
            dry_run,
        },
    )))
}

pub(crate) fn parse_sessions_llm_invocations(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsLlmInvocationsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--action" => parsed.action = Some(flags.value("--action")?),
            "--status" => parsed.status = Some(flags.value("--status")?),
            "--limit" => parsed.limit = Some(parse_i64_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(norm_time(value_after_equals(arg, "--since")?)?)
            }
            _ if arg.starts_with("--action=") => {
                parsed.action = Some(value_after_equals(arg, "--action")?)
            }
            _ if arg.starts_with("--status=") => {
                parsed.status = Some(value_after_equals(arg, "--status")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_i64_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ => bail!("unknown flag for sessions llm-invocations: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::LlmInvocations(
        parsed,
    )))
}

/// `cortex sessions skill-assess <skill>` — low-level alias for `cortex
/// assess skill`. Delegates flag parsing to the canonical `assess skill`
/// parser (`parse_assess_skill_from`) so the two entry points never drift,
/// then rewraps the result as `SessionsCommand::SkillAssess` instead of
/// `AssessCommand::Skill`.
pub(crate) fn parse_sessions_skill_assess(args: &[String]) -> Result<CliCommand> {
    let assess_cmd = super::super::assess::parse_assess_skill_from(args)?;
    let CliCommand::Assess(super::super::super::AssessCommand::Skill(skill_args)) = assess_cmd
    else {
        unreachable!("parse_assess_skill_from always returns AssessCommand::Skill");
    };
    Ok(CliCommand::Sessions(SessionsCommand::SkillAssess(
        skill_args,
    )))
}

#[cfg(test)]
#[path = "more_tests.rs"]
mod tests;
