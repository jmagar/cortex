//! Parse function for `cortex state clockskew`.
//!
//! Surface parity (2026-05-22): exposes the `clock_skew` MCP action and
//! `GET /api/clock-skew` REST route through the nested CLI command.

use anyhow::{Result, bail};

use super::super::args::ClockSkewArgs;
use super::super::{FlagCursor, norm_time, parse_u32_flag};

pub(crate) fn parse_clock_skew_args(args: &[String]) -> Result<ClockSkewArgs> {
    let mut parsed = ClockSkewArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--since")? {
            parsed.since = Some(norm_time(v)?);
        } else if let Some(v) = flags.match_value(&arg, "--limit")? {
            parsed.limit = Some(parse_u32_flag("--limit", v)?);
        } else {
            bail!("unknown clockskew option: {arg}");
        }
    }
    Ok(parsed)
}
