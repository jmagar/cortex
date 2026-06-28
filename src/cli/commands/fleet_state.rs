//! Parse function for `cortex fleet-state`.
//!
//! Surface parity (cxih.4): exposes the `fleet_state` MCP action and
//! `GET /api/fleet-state` REST route as a top-level CLI subcommand.

use anyhow::{Result, bail};

use super::super::FlagCursor;
use super::super::args::FleetStateArgs;

pub(crate) fn parse_fleet_state_args(args: &[String]) -> Result<FleetStateArgs> {
    let mut parsed = FleetStateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if arg == "--exclude-ok" {
            parsed.include_ok = Some(false);
        } else if arg == "--include-ok" {
            parsed.include_ok = Some(true);
        } else if let Some(v) = flags.match_value(&arg, "--sort")? {
            match v.as_str() {
                "pressure" | "freshness" | "hostname" => parsed.sort = Some(v),
                other => bail!("--sort must be pressure, freshness, or hostname (got: {other})"),
            }
        } else {
            bail!("unknown fleet-state option: {arg}");
        }
    }
    Ok(parsed)
}
