//! Parse function for `syslog clock-skew`.
//!
//! Surface parity (2026-05-22): exposes the `clock_skew` MCP action and
//! `GET /api/clock-skew` REST route as a top-level CLI subcommand.

use anyhow::{bail, Result};

use super::super::args::{CliCommand, ClockSkewArgs};
use super::super::{parse_u32_flag, FlagCursor};

pub(crate) fn parse_clock_skew(args: &[String]) -> Result<CliCommand> {
    let mut parsed = ClockSkewArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--since")? {
            parsed.since = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--limit")? {
            parsed.limit = Some(parse_u32_flag("--limit", v)?);
        } else {
            bail!("unknown clock-skew option: {arg}");
        }
    }
    Ok(CliCommand::ClockSkew(parsed))
}
