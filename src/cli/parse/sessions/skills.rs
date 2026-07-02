use anyhow::{Result, bail};

use super::super::super::parse_common::{
    FlagCursor, norm_time, parse_positive_u64_flag, parse_u32_flag, value_after_equals,
};
use super::super::super::{
    CliCommand, SessionsCommand, SessionsSkillsBackfillArgs, SessionsSkillsListArgs,
};

pub(crate) fn parse_sessions_skills(args: &[String]) -> Result<CliCommand> {
    match args.first().map(String::as_str) {
        Some("backfill") => parse_skills_backfill(&args[1..]),
        _ => parse_skills_list(args),
    }
}

fn parse_skills_list(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsSkillsListArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--skill" => parsed.skill = Some(flags.value("--skill")?),
            "--plugin" => parsed.plugin = Some(flags.value("--plugin")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--session-id" => parsed.session_id = Some(flags.value("--session-id")?),
            "--host" => parsed.host = Some(flags.value("--host")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--skill=") => {
                parsed.skill = Some(value_after_equals(arg, "--skill")?)
            }
            _ if arg.starts_with("--plugin=") => {
                parsed.plugin = Some(value_after_equals(arg, "--plugin")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--session-id=") => {
                parsed.session_id = Some(value_after_equals(arg, "--session-id")?)
            }
            _ if arg.starts_with("--host=") => {
                parsed.host = Some(value_after_equals(arg, "--host")?)
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
            _ => bail!("unknown sessions skills option: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Skills(parsed)))
}

fn parse_skills_backfill(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsSkillsBackfillArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--dry-run" => parsed.dry_run = true,
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--limit" => {
                parsed.limit = Some(parse_positive_u64_flag("--limit", flags.value("--limit")?)?)
            }
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(norm_time(value_after_equals(arg, "--since")?)?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_positive_u64_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ => bail!("unknown sessions skills backfill option: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::SkillsBackfill(
        parsed,
    )))
}
