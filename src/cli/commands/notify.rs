//! Parse functions for `cortex notify` subcommands.
//!
//! Extracted from `src/cli.rs` as part of Q-C1 (cli.rs split).

use anyhow::{Result, anyhow, bail};

use super::super::args::{CliCommand, NotifyCommand, NotifyRecentArgs, NotifyTestArgs};
use super::super::{FlagCursor, parse_i64_flag};

/// Dispatch `cortex notify <subcommand> [args]`.
pub(crate) fn parse_notify(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("notify requires a subcommand (recent|test)"))?;
    match subcommand.as_str() {
        "recent" => parse_notify_recent(rest),
        "test" => parse_notify_test(rest),
        _ => bail!(
            "{}",
            super::super::suggest::unknown_command(
                "notify subcommand",
                subcommand,
                &["recent", "test"],
            )
        ),
    }
}

fn parse_notify_recent(args: &[String]) -> Result<CliCommand> {
    let mut parsed = NotifyRecentArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--rule-id")? {
            parsed.rule_id = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--since")? {
            parsed.since = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--limit")? {
            parsed.limit = Some(parse_i64_flag("--limit", v)?);
        } else {
            bail!("unknown notify recent option: {arg}");
        }
    }
    Ok(CliCommand::Notify(NotifyCommand::Recent(parsed)))
}

fn parse_notify_test(args: &[String]) -> Result<CliCommand> {
    let mut parsed = NotifyTestArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--body")? {
            parsed.body = Some(v);
        } else {
            bail!("unknown notify test option: {arg}");
        }
    }
    Ok(CliCommand::Notify(NotifyCommand::Test(parsed)))
}
#[cfg(test)]
#[path = "notify_tests.rs"]
mod tests;
