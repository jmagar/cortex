//! Parse function for `syslog anomalies`.
//!
//! Surface parity (2026-05-22): exposes the `anomalies` MCP action and
//! `GET /api/anomalies` REST route as a top-level CLI subcommand.

use anyhow::{bail, Result};

use super::super::args::{AnomaliesArgs, CliCommand};
use super::super::{parse_u32_flag, FlagCursor};

pub(crate) fn parse_anomalies(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AnomaliesArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--recent-minutes")? {
            parsed.recent_minutes = Some(parse_u32_flag("--recent-minutes", v)?);
        } else if let Some(v) = flags.match_value(&arg, "--baseline-minutes")? {
            parsed.baseline_minutes = Some(parse_u32_flag("--baseline-minutes", v)?);
        } else {
            bail!("unknown anomalies option: {arg}");
        }
    }
    Ok(CliCommand::Anomalies(parsed))
}
