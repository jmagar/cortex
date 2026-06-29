use anyhow::{Result, anyhow, bail};

use super::super::super::parse_common::{
    FlagCursor, norm_time, parse_positive_u64_flag, parse_u32_flag, value_after_equals,
};
use super::super::super::{
    CliCommand, SessionsAddArgs, SessionsCheckpointsArgs, SessionsCommand, SessionsDoctorArgs,
    SessionsErrorsArgs, SessionsIndexArgs, SessionsPruneCheckpointsArgs, SessionsWatchArgs,
};

pub(crate) fn parse_sessions_index(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsIndexArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--path" => parsed.path = Some(flags.value("--path")?),
            "--force" => parsed.force = true,
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            _ if arg.starts_with("--path=") => {
                parsed.path = Some(value_after_equals(arg, "--path")?)
            }
            _ if arg.starts_with("--since=") => {
                parsed.since = Some(norm_time(value_after_equals(arg, "--since")?)?)
            }
            _ => bail!("unknown sessions index option: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Index(parsed)))
}

pub(crate) fn parse_sessions_add(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsAddArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--file" => parsed.file = flags.value("--file")?,
            "--force" => parsed.force = true,
            _ if arg.starts_with("--file=") => parsed.file = value_after_equals(arg, "--file")?,
            _ => bail!("unknown sessions add option: {arg}"),
        }
    }
    if parsed.file.is_empty() {
        bail!("sessions add requires --file <PATH>");
    }
    Ok(CliCommand::Sessions(SessionsCommand::Add(parsed)))
}

pub(crate) fn parse_sessions_watch(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsWatchArgs::default();
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
            _ => bail!("unknown sessions watch option: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Watch(parsed)))
}

pub(crate) fn parse_sessions_checkpoints(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsCheckpointsArgs::default();
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
            _ => bail!("unknown sessions checkpoints option: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Checkpoints(parsed)))
}

pub(crate) fn parse_sessions_errors(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsErrorsArgs::default();
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
            _ => bail!("unknown sessions errors option: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Errors(parsed)))
}

pub(crate) fn parse_sessions_prune_checkpoints(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsPruneCheckpointsArgs::default();
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
            _ => bail!("unknown sessions prune-checkpoints option: {arg}"),
        }
    }
    if !parsed.missing_only {
        bail!("sessions prune-checkpoints requires --missing");
    }
    Ok(CliCommand::Sessions(SessionsCommand::PruneCheckpoints(
        parsed,
    )))
}

pub(crate) fn parse_sessions_doctor(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsDoctorArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--strict-permissions" => parsed.strict_permissions = true,
            _ => bail!("unknown sessions doctor option: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::Doctor(parsed)))
}
