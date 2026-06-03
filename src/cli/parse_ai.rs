use anyhow::{anyhow, bail, Result};

use super::parse_ai_more::{
    parse_ai_ask_history, parse_ai_assess, parse_ai_incident_context, parse_ai_incidents,
    parse_ai_investigate, parse_ai_similar,
};
use super::parse_common::{
    parse_output_args, parse_positive_u64_flag, parse_u32_flag, value_after_equals, FlagCursor,
};
use super::{
    AiAbuseArgs, AiAddArgs, AiBlocksArgs, AiCheckpointsArgs, AiCommand, AiContextArgs,
    AiCorrelateArgs, AiDoctorArgs, AiErrorsArgs, AiIndexArgs, AiListArgs, AiOutputDetail,
    AiPruneCheckpointsArgs, AiSearchArgs, AiWatchArgs, CliCommand,
};

const AI_SUBCOMMANDS: &[&str] = &[
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
];

pub(crate) fn parse_ai(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("ai requires a subcommand"))?;
    match subcommand.as_str() {
        "search" => parse_ai_search(rest),
        "abuse" => parse_ai_abuse(rest),
        "correlate" => parse_ai_correlate(rest),
        "blocks" => parse_ai_blocks(rest),
        "context" => parse_ai_context(rest),
        "tools" => parse_ai_tools(rest),
        "projects" => parse_ai_projects(rest),
        "index" => parse_ai_index(rest),
        "add" => parse_ai_add(rest),
        "watch" => parse_ai_watch(rest),
        "checkpoints" => parse_ai_checkpoints(rest),
        "errors" => parse_ai_errors(rest),
        "prune-checkpoints" => parse_ai_prune_checkpoints(rest),
        "doctor" => parse_ai_doctor(rest),
        "watch-status" => Ok(CliCommand::Ai(AiCommand::WatchStatus(parse_output_args(
            "ai watch-status",
            rest,
        )?))),
        "smoke-watch" => Ok(CliCommand::Ai(AiCommand::SmokeWatch(parse_output_args(
            "ai smoke-watch",
            rest,
        )?))),
        "similar" => parse_ai_similar(rest),
        "ask-history" => parse_ai_ask_history(rest),
        "incident-context" => parse_ai_incident_context(rest),
        "incidents" => parse_ai_incidents(rest),
        "investigate" => parse_ai_investigate(rest),
        "assess" => parse_ai_assess(rest),
        _ => bail!(
            "{}",
            super::suggest::unknown_command("ai subcommand", subcommand, AI_SUBCOMMANDS)
        ),
    }
}

pub(crate) fn parse_ai_search(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiSearchArgs::default();
    let mut query = Vec::new();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown ai search option: {arg}"),
            _ => query.push(arg),
        }
    }
    parsed.query = query.join(" ");
    if parsed.query.is_empty() {
        bail!("ai search requires a query");
    }
    Ok(CliCommand::Ai(AiCommand::Search(parsed)))
}

pub(crate) fn parse_ai_abuse(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiAbuseArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
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
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
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
            _ if arg.starts_with('-') => bail!("unknown ai abuse option: {arg}"),
            _ => bail!("unexpected ai abuse argument: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Abuse(parsed)))
}

pub(crate) fn parse_ai_correlate(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiCorrelateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--session-id" => parsed.session_id = Some(flags.value("--session-id")?),
            "--ai-query" => parsed.ai_query = Some(flags.value("--ai-query")?),
            "--log-query" => parsed.log_query = Some(flags.value("--log-query")?),
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--source-ip" => parsed.source_ip = Some(flags.value("--source-ip")?),
            "--app-name" => parsed.app_name = Some(flags.value("--app-name")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
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
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--source-ip=") => {
                parsed.source_ip = Some(value_after_equals(arg, "--source-ip")?)
            }
            _ if arg.starts_with("--app-name=") => {
                parsed.app_name = Some(value_after_equals(arg, "--app-name")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
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
            _ if arg.starts_with('-') => bail!("unknown ai correlate option: {arg}"),
            _ => bail!("unexpected ai correlate argument: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Correlate(parsed)))
}

pub(crate) fn parse_ai_blocks(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiBlocksArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            "--limit" => {
                parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)? as usize)
            }
            "--detail" => {
                parsed.detail = AiOutputDetail::parse(&flags.value("--detail")?, "--detail")?
            }
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ if arg.starts_with("--limit=") => {
                parsed.limit =
                    Some(parse_u32_flag("--limit", value_after_equals(arg, "--limit")?)? as usize)
            }
            _ if arg.starts_with("--detail=") => {
                parsed.detail =
                    AiOutputDetail::parse(&value_after_equals(arg, "--detail")?, "--detail")?
            }
            _ if arg.starts_with('-') => bail!(
                "{}",
                super::suggest::unknown_option(
                    "ai blocks",
                    &arg,
                    &[
                        "--json",
                        "--project",
                        "--tool",
                        "--from",
                        "--to",
                        "--limit",
                        "--detail",
                    ],
                )
            ),
            _ => bail!("unexpected ai blocks argument: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Blocks(parsed)))
}

pub(crate) fn parse_ai_context(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiContextArgs::default();
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
            _ if arg.starts_with('-') => bail!("unknown ai context option: {arg}"),
            _ if parsed.project.is_empty() => parsed.project = arg,
            _ => bail!("unexpected ai context argument: {arg}"),
        }
    }
    if parsed.project.is_empty() {
        bail!("ai context requires --project <PATH>");
    }
    Ok(CliCommand::Ai(AiCommand::Context(parsed)))
}

pub(crate) fn parse_ai_tools(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiListArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ => bail!("unknown ai tools option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Tools(parsed)))
}

pub(crate) fn parse_ai_projects(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiListArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--from" => parsed.from = Some(flags.value("--from")?),
            "--to" => parsed.to = Some(flags.value("--to")?),
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(value_after_equals(arg, "--from")?)
            }
            _ if arg.starts_with("--to=") => parsed.to = Some(value_after_equals(arg, "--to")?),
            _ => bail!("unknown ai projects option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Projects(parsed)))
}

pub(crate) fn parse_ai_index(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiIndexArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--path" => parsed.path = Some(flags.value("--path")?),
            "--force" => parsed.force = true,
            "--since" => parsed.since = Some(flags.value("--since")?),
            _ if arg.starts_with("--path=") => {
                parsed.path = Some(value_after_equals(arg, "--path")?)
            }
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(value_after_equals(arg, "--since")?)
            }
            _ => bail!("unknown ai index option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Index(parsed)))
}

pub(crate) fn parse_ai_add(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiAddArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--file" => parsed.file = flags.value("--file")?,
            "--force" => parsed.force = true,
            _ if arg.starts_with("--file=") => parsed.file = value_after_equals(arg, "--file")?,
            _ => bail!("unknown ai add option: {arg}"),
        }
    }
    if parsed.file.is_empty() {
        bail!("ai add requires --file <PATH>");
    }
    Ok(CliCommand::Ai(AiCommand::Add(parsed)))
}

pub(crate) fn parse_ai_watch(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiWatchArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--path" => parsed.path = Some(flags.value("--path")?),
            "--debounce-ms" => {
                parsed.debounce_ms =
                    parse_positive_u64_flag("--debounce-ms", flags.value("--debounce-ms")?)?;
            }
            "--settle-ms" => {
                parsed.settle_ms =
                    parse_positive_u64_flag("--settle-ms", flags.value("--settle-ms")?)?;
            }
            "--max-retries" => {
                parsed.max_retries =
                    parse_u32_flag("--max-retries", flags.value("--max-retries")?)?
                        .try_into()
                        .map_err(|_| anyhow!("--max-retries is too large"))?;
            }
            "--no-initial-scan" => parsed.no_initial_scan = true,
            _ if arg.starts_with("--path=") => {
                parsed.path = Some(value_after_equals(arg, "--path")?)
            }
            _ if arg.starts_with("--debounce-ms=") => {
                parsed.debounce_ms = parse_positive_u64_flag(
                    "--debounce-ms",
                    value_after_equals(arg, "--debounce-ms")?,
                )?;
            }
            _ if arg.starts_with("--settle-ms=") => {
                parsed.settle_ms = parse_positive_u64_flag(
                    "--settle-ms",
                    value_after_equals(arg, "--settle-ms")?,
                )?;
            }
            _ if arg.starts_with("--max-retries=") => {
                parsed.max_retries =
                    parse_u32_flag("--max-retries", value_after_equals(arg, "--max-retries")?)?
                        .try_into()
                        .map_err(|_| anyhow!("--max-retries is too large"))?;
            }
            _ => bail!("unknown ai watch option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Watch(parsed)))
}

pub(crate) fn parse_ai_checkpoints(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiCheckpointsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--errors" => parsed.errors_only = true,
            "--missing" => parsed.missing_only = true,
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ => bail!("unknown ai checkpoints option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Checkpoints(parsed)))
}

pub(crate) fn parse_ai_errors(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiErrorsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ => bail!("unknown ai errors option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Errors(parsed)))
}

pub(crate) fn parse_ai_prune_checkpoints(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiPruneCheckpointsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--missing" => parsed.missing_only = true,
            "--dry-run" => parsed.dry_run = true,
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ => bail!("unknown ai prune-checkpoints option: {arg}"),
        }
    }
    if !parsed.missing_only {
        bail!("ai prune-checkpoints requires --missing");
    }
    Ok(CliCommand::Ai(AiCommand::PruneCheckpoints(parsed)))
}

pub(crate) fn parse_ai_doctor(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AiDoctorArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--strict-permissions" => parsed.strict_permissions = true,
            _ => bail!("unknown ai doctor option: {arg}"),
        }
    }
    Ok(CliCommand::Ai(AiCommand::Doctor(parsed)))
}

#[cfg(test)]
#[path = "parse_ai_tests.rs"]
mod tests;
