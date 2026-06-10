//! Parse function for `cortex correlate-state`.
//!
//! Surface parity (cxih.4): exposes the `correlate_state` MCP action and
//! `GET /api/correlate-state` REST route as a top-level CLI subcommand.

use anyhow::{Result, bail};

use super::super::args::{CliCommand, CorrelateStateArgs};
use super::super::{FlagCursor, parse_u32_flag};

pub(crate) fn parse_correlate_state(args: &[String]) -> Result<CliCommand> {
    let mut parsed = CorrelateStateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--reference-time")? {
            parsed.reference_time = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--window-minutes")? {
            parsed.window_minutes = Some(parse_u32_flag("--window-minutes", v)?);
        } else if let Some(v) = flags.match_value(&arg, "--host")? {
            parsed.host = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--severity-min")? {
            parsed.severity_min = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--limit")? {
            parsed.limit = Some(parse_u32_flag("--limit", v)?);
        } else {
            bail!("unknown correlate-state option: {arg}");
        }
    }
    Ok(CliCommand::CorrelateState(parsed))
}
