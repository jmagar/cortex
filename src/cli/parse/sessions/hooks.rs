use anyhow::{Result, bail};

use super::super::super::parse_common::{
    FlagCursor, norm_time, parse_positive_u64_flag, parse_u32_flag, value_after_equals,
};
use super::super::super::{
    CliCommand, SessionsCommand, SessionsHookEventsListArgs, SessionsHooksBackfillArgs,
};

pub(crate) fn parse_sessions_hook_events(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsHookEventsListArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--hook-event" => parsed.hook_event = Some(flags.value("--hook-event")?),
            "--hook" => parsed.hook_name = Some(flags.value("--hook")?),
            "--hook-source" => parsed.hook_source = Some(flags.value("--hook-source")?),
            "--status" => parsed.status = Some(flags.value("--status")?),
            "--evidence-kind" => parsed.evidence_kind = Some(flags.value("--evidence-kind")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--session-id" => parsed.session_id = Some(flags.value("--session-id")?),
            "--host" => parsed.host = Some(flags.value("--host")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--hook-event=") => {
                parsed.hook_event = Some(value_after_equals(arg, "--hook-event")?)
            }
            _ if arg.starts_with("--hook=") => {
                parsed.hook_name = Some(value_after_equals(arg, "--hook")?)
            }
            _ if arg.starts_with("--hook-source=") => {
                parsed.hook_source = Some(value_after_equals(arg, "--hook-source")?)
            }
            _ if arg.starts_with("--status=") => {
                parsed.status = Some(value_after_equals(arg, "--status")?)
            }
            _ if arg.starts_with("--evidence-kind=") => {
                parsed.evidence_kind = Some(value_after_equals(arg, "--evidence-kind")?)
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
            other => bail!(
                "{}",
                super::super::super::suggest::unknown_option(
                    "sessions hook-events",
                    other,
                    &[
                        "--json",
                        "--hook-event",
                        "--hook",
                        "--hook-source",
                        "--status",
                        "--evidence-kind",
                        "--tool",
                        "--project",
                        "--session-id",
                        "--host",
                        "--since",
                        "--until",
                        "--limit",
                    ],
                )
            ),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::HookEvents(parsed)))
}

pub(crate) fn parse_sessions_hooks_backfill(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsHooksBackfillArgs::default();
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
            other => bail!(
                "{}",
                super::super::super::suggest::unknown_option(
                    "sessions hooks-backfill",
                    other,
                    &["--json", "--dry-run", "--since", "--limit"],
                )
            ),
        }
    }
    Ok(CliCommand::Sessions(SessionsCommand::HooksBackfill(parsed)))
}
