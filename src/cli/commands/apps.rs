//! Parse function for `cortex apps`.
//!
//! Surface parity (2026-05-22): exposes the `apps` MCP action and
//! `GET /api/apps` REST route as a top-level CLI subcommand.

use anyhow::{bail, Result};

use super::super::args::{AppsArgs, CliCommand};
use super::super::{parse_u32_flag, FlagCursor};

pub(crate) fn parse_apps(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AppsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--hostname")? {
            parsed.hostname = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--from")? {
            parsed.from = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--to")? {
            parsed.to = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--limit")? {
            parsed.limit = Some(parse_u32_flag("--limit", v)?);
        } else if let Some(v) = flags.match_value(&arg, "--offset")? {
            parsed.offset = Some(parse_u32_flag("--offset", v)?);
        } else {
            bail!("unknown apps option: {arg}");
        }
    }
    Ok(CliCommand::Apps(parsed))
}
