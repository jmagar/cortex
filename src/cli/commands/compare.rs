//! Parse function for `cortex compare`.
//!
//! Surface parity (2026-05-22): exposes the `compare` MCP action and
//! `GET /api/compare` REST route as a top-level CLI subcommand. All four
//! time-range flags (`--a-from`, `--a-to`, `--b-from`, `--b-to`) are required;
//! missing flags are caught in `CompareArgs::into_request()` so the error
//! message points the operator at the missing flag.

use anyhow::{bail, Result};

use super::super::args::{CliCommand, CompareArgs};
use super::super::FlagCursor;

pub(crate) fn parse_compare(args: &[String]) -> Result<CliCommand> {
    let mut parsed = CompareArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--a-from")? {
            parsed.a_from = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--a-to")? {
            parsed.a_to = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--b-from")? {
            parsed.b_from = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--b-to")? {
            parsed.b_to = Some(v);
        } else {
            bail!("unknown compare option: {arg}");
        }
    }
    Ok(CliCommand::Compare(parsed))
}
