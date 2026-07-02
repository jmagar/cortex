use anyhow::{Result, bail};

use self::more::{
    parse_sessions_ask_history, parse_sessions_assess, parse_sessions_incident_context,
    parse_sessions_incidents, parse_sessions_investigate, parse_sessions_llm_invocations,
    parse_sessions_similar, parse_sessions_skill_assess,
};
use self::ops::{
    parse_sessions_add, parse_sessions_checkpoints, parse_sessions_doctor, parse_sessions_errors,
    parse_sessions_index, parse_sessions_prune_checkpoints, parse_sessions_watch,
};
use super::super::parse_common::{
    FlagCursor, norm_time, parse_output_args, parse_u32_flag, value_after_equals,
};
use super::super::parse_logs::parse_sessions;
use super::super::{
    CliCommand, SessionsAbuseArgs, SessionsBlocksArgs, SessionsCommand, SessionsContextArgs,
    SessionsCorrelateArgs, SessionsListArgs, SessionsOutputDetail, SessionsSearchArgs,
};
use skill_incidents::{parse_sessions_skill_incidents, parse_sessions_skill_investigate};

mod more;
mod ops;
mod skill_incidents;
mod hooks;
mod skills;

const SESSIONS_SUBCOMMANDS: &[&str] = &[
    "search",
    "abuse",
    "correlate",
    "blocks",
    "context",
    "tools",
    "projects",
    "index",
    "add",
    "watch",
    "checkpoints",
    "errors",
    "prune-checkpoints",
    "doctor",
    "watch-status",
    "smoke-watch",
    "similar",
    "ask-history",
    "incident-context",
    "incidents",
    "investigate",
    "assess",
    "llm-invocations",
    "skills",
    "skill-incidents",
    "skill-investigate",
    "skill-assess",
    "hook-events",
    "hooks-backfill",
];

pub(crate) fn parse_sessions_command(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = match args.split_first() {
        None => ("", args),
        Some((subcommand, _)) if subcommand.starts_with('-') => ("", args),
        Some((subcommand, rest)) => (subcommand.as_str(), rest),
    };
    match subcommand {
        "" => parse_sessions(rest),
        "search" => parse_sessions_search(rest),
        "abuse" => parse_sessions_abuse(rest),
        "correlate" => parse_sessions_correlate(rest),
        "blocks" => parse_sessions_blocks(rest),
        "context" => parse_sessions_context(rest),
        "tools" => parse_sessions_tools(rest),
        "projects" => parse_sessions_projects(rest),
        "index" => parse_sessions_index(rest),
        "add" => parse_sessions_add(rest),
        "watch" => parse_sessions_watch(rest),
        "checkpoints" => parse_sessions_checkpoints(rest),
        "errors" => parse_sessions_errors(rest),
        "prune-checkpoints" => parse_sessions_prune_checkpoints(rest),
        "doctor" => parse_sessions_doctor(rest),
        "watch-status" => Ok(CliCommand::Sessions(SessionsCommand::WatchStatus(
            parse_output_args("sessions watch-status", rest)?,
        ))),
        "smoke-watch" => Ok(CliCommand::Sessions(SessionsCommand::SmokeWatch(
            parse_output_args("sessions smoke-watch", rest)?,
        ))),
        "similar" => parse_sessions_similar(rest),
        "ask-history" => parse_sessions_ask_history(rest),
        "incident-context" => parse_sessions_incident_context(rest),
        "incidents" => parse_sessions_incidents(rest),
        "investigate" => parse_sessions_investigate(rest),
        "assess" => parse_sessions_assess(rest),
        "llm-invocations" => parse_sessions_llm_invocations(rest),
        "skills" => self::skills::parse_sessions_skills(rest),
        "skill-incidents" => parse_sessions_skill_incidents(rest),
        "skill-investigate" => parse_sessions_skill_investigate(rest),
        "skill-assess" => parse_sessions_skill_assess(rest),
        "hook-events" => self::hooks::parse_sessions_hook_events(rest),
        "hooks-backfill" => self::hooks::parse_sessions_hooks_backfill(rest),
        _ => bail!(
            "{}",
            super::suggest::unknown_command(
                "sessions subcommand",
                subcommand,
                SESSIONS_SUBCOMMANDS
            )
        ),
    }
}

pub(crate) fn parse_sessions_search(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsSearchArgs::default();
    let mut query = Vec::new();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
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
            _ if arg.starts_with('-') => bail!("unknown sessions search option: {arg}"),
            _ => query.push(arg),
        }
    }
    parsed.query = query.join(" ");
    if parsed.query.is_empty() {
        bail!("sessions search requires a query");
    }
    Ok(CliCommand::Sessions(SessionsCommand::Search(parsed)))
}

pub(crate) fn parse_sessions_abuse(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsAbuseArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--before" => {
                parsed.before = Some(parse_u32_flag("--before", flags.value("--before")?)?)
            }
            "--after" => parsed.after = Some(parse_u32_flag("--after", flags.value("--after")?)?),
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
            _ if arg.starts_with("--before=") => {
                parsed.before = Some(parse_u32_flag(
                    "--before",
                    value_after_equals(arg, "--before")?,
                )?)
            }
            _ if arg.starts_with("--after=") => {
                parsed.after = Some(parse_u32_flag(
                    "--after",
                    value_after_equals(arg, "--after")?,
                )?)
            }
            _ if arg.starts_with("--term=") => {
                parsed.terms.push(value_after_equals(arg, "--term")?)
            }
            _ if arg.starts_with('-') => bail!("unknown sessions abuse option: {arg}"),
            _ => bail!("unexpected sessions abuse argument: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Abuse(parsed)))
}

pub(crate) fn parse_sessions_correlate(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsCorrelateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--session-id" => parsed.session_id = Some(flags.value("--session-id")?),
            "--ai-query" => parsed.ai_query = Some(flags.value("--ai-query")?),
            "--log-query" => parsed.log_query = Some(flags.value("--log-query")?),
            "--host" => parsed.host = Some(flags.value("--host")?),
            "--source" => parsed.source = Some(flags.value("--source")?),
            "--app" => parsed.app = Some(flags.value("--app")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--window-minutes" => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--severity-min" => parsed.severity_min = Some(flags.value("--severity-min")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--events-per-anchor" => {
                parsed.events_per_anchor = Some(parse_u32_flag(
                    "--events-per-anchor",
                    flags.value("--events-per-anchor")?,
                )?)
            }
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--session-id=") => {
                parsed.session_id = Some(value_after_equals(arg, "--session-id")?)
            }
            _ if arg.starts_with("--ai-query=") => {
                parsed.ai_query = Some(value_after_equals(arg, "--ai-query")?)
            }
            _ if arg.starts_with("--log-query=") => {
                parsed.log_query = Some(value_after_equals(arg, "--log-query")?)
            }
            _ if arg.starts_with("--host=") => {
                parsed.host = Some(value_after_equals(arg, "--host")?)
            }
            _ if arg.starts_with("--source=") => {
                parsed.source = Some(value_after_equals(arg, "--source")?)
            }
            _ if arg.starts_with("--app=") => parsed.app = Some(value_after_equals(arg, "--app")?),
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
            _ if arg.starts_with("--severity-min=") => {
                parsed.severity_min = Some(value_after_equals(arg, "--severity-min")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with("--events-per-anchor=") => {
                parsed.events_per_anchor = Some(parse_u32_flag(
                    "--events-per-anchor",
                    value_after_equals(arg, "--events-per-anchor")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown sessions correlate option: {arg}"),
            _ => bail!("unexpected sessions correlate argument: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Correlate(parsed)))
}

pub(crate) fn parse_sessions_blocks(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsBlocksArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => {
                parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)? as usize)
            }
            "--detail" => {
                parsed.detail = SessionsOutputDetail::parse(&flags.value("--detail")?, "--detail")?
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
                parsed.limit =
                    Some(parse_u32_flag("--limit", value_after_equals(arg, "--limit")?)? as usize)
            }
            _ if arg.starts_with("--detail=") => {
                parsed.detail =
                    SessionsOutputDetail::parse(&value_after_equals(arg, "--detail")?, "--detail")?
            }
            _ if arg.starts_with('-') => bail!(
                "{}",
                super::suggest::unknown_option(
                    "sessions blocks",
                    &arg,
                    &[
                        "--json",
                        "--project",
                        "--tool",
                        "--since",
                        "--until",
                        "--limit",
                        "--detail",
                    ],
                )
            ),
            _ => bail!("unexpected sessions blocks argument: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Blocks(parsed)))
}

pub(crate) fn parse_sessions_context(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsContextArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = flags.value("--project")?,
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--project=") => {
                parsed.project = value_after_equals(arg, "--project")?
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown sessions context option: {arg}"),
            _ if parsed.project.is_empty() => parsed.project = arg,
            _ => bail!("unexpected sessions context argument: {arg}"),
        }
    }
    if parsed.project.is_empty() {
        bail!("sessions context requires --project <PATH>");
    }
    Ok(CliCommand::Sessions(SessionsCommand::Context(parsed)))
}

pub(crate) fn parse_sessions_tools(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsListArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(norm_time(value_after_equals(arg, "--since")?)?)
            }
            _ if arg.starts_with("--until=") => {
                parsed.until = Some(norm_time(value_after_equals(arg, "--until")?)?)
            }
            _ => bail!("unknown sessions tools option: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Tools(parsed)))
}

pub(crate) fn parse_sessions_projects(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsListArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(norm_time(value_after_equals(arg, "--since")?)?)
            }
            _ if arg.starts_with("--until=") => {
                parsed.until = Some(norm_time(value_after_equals(arg, "--until")?)?)
            }
            _ => bail!("unknown sessions projects option: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Projects(parsed)))
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;
