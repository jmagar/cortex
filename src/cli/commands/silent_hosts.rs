//! Parse function for `cortex silent-hosts`.
//!
//! Surface parity (2026-05-22): exposes the `silent_hosts` MCP action and
//! `GET /api/silent-hosts` REST route as a top-level CLI subcommand.

use anyhow::{Result, bail};

use super::super::args::{CliCommand, SilentHostsArgs};
use super::super::{FlagCursor, parse_u32_flag};

pub(crate) fn parse_silent_hosts(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SilentHostsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--silent-minutes")? {
            parsed.silent_minutes = Some(parse_u32_flag("--silent-minutes", v)?);
        } else {
            bail!("unknown silent-hosts option: {arg}");
        }
    }
    Ok(CliCommand::SilentHosts(parsed))
}
