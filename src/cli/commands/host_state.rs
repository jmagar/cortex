//! Parse function for `cortex host-state`.
//!
//! Surface parity (cxih.4): exposes the `host_state` MCP action and
//! `GET /api/host-state` REST route as a top-level CLI subcommand.

use anyhow::{Result, bail};

use super::super::args::{CliCommand, HostStateArgs};
use super::super::{FlagCursor, parse_u32_flag};

pub(crate) fn parse_host_state(args: &[String]) -> Result<CliCommand> {
    let mut parsed = HostStateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--host-id")? {
            parsed.host_id = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--hostname")? {
            parsed.hostname = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--since")? {
            parsed.since = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--limit")? {
            parsed.limit = Some(parse_u32_flag("--limit", v)?);
        } else {
            bail!(
                "{}",
                super::super::suggest::unknown_option(
                    "host-state",
                    &arg,
                    &["--json", "--host-id", "--hostname", "--since", "--limit"],
                )
            );
        }
    }
    if parsed.host_id.is_none() && parsed.hostname.is_none() {
        bail!(
            "host-state requires --host-id ID or --hostname HOST\n\nUsage: cortex host-state [--host-id ID] [--hostname HOST] [--since TIME] [--limit N] [--json]"
        );
    }
    Ok(CliCommand::HostState(parsed))
}
