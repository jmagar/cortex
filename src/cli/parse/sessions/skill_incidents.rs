use anyhow::{Result, bail};

use super::super::super::parse_common::{
    FlagCursor, norm_time, parse_u32_flag, value_after_equals,
};
use super::super::super::{
    CliCommand, SessionsCommand, SessionsSkillIncidentsArgs, SessionsSkillInvestigateArgs,
};

pub(crate) fn parse_sessions_skill_incidents(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsSkillIncidentsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--skill" => parsed.skill = Some(flags.value("--skill")?),
            "--plugin" => parsed.plugin = Some(flags.value("--plugin")?),
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
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
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
            _ if arg.starts_with("--signal=") => {
                parsed.signals.push(value_after_equals(arg, "--signal")?)
            }
            _ if arg.starts_with("--min-score=") => {
                parsed.min_score = Some(value_after_equals(arg, "--min-score")?)
            }
            _ if arg.starts_with('-') => bail!("unknown sessions skill-incidents option: {arg}"),
            _ if parsed.skill.is_none() => parsed.skill = Some(arg),
            _ => bail!("unexpected sessions skill-incidents argument: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::SkillIncidents(
        parsed,
    )))
}

pub(crate) fn parse_sessions_skill_investigate(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsSkillInvestigateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--all" => parsed.all = true,
            "--incident-id" => parsed.incident_id = Some(flags.value("--incident-id")?),
            "--plugin" => parsed.plugin = Some(flags.value("--plugin")?),
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
            _ if arg.starts_with("--incident-id=") => {
                parsed.incident_id = Some(value_after_equals(arg, "--incident-id")?)
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
            _ if arg.starts_with('-') => bail!("unknown sessions skill-investigate option: {arg}"),
            // Bare positional binds to --skill.
            _ if parsed.skill.is_none() => parsed.skill = Some(arg),
            _ => bail!("unexpected sessions skill-investigate argument: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::SkillInvestigate(
        parsed,
    )))
}
