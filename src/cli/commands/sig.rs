//! Parse functions for `syslog sig` subcommands.
//!
//! Extracted from `src/cli.rs` as part of Q-C1 (cli.rs split).

use anyhow::{anyhow, bail, Result};

use super::super::args::{CliCommand, SigAckArgs, SigCommand, SigListArgs, SigUnackArgs};
use super::super::{parse_u32_flag, FlagCursor};

/// Dispatch `syslog sig <subcommand> [args]`.
pub(crate) fn parse_sig(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("sig requires a subcommand (list|ack|unack)"))?;
    match subcommand.as_str() {
        "list" => parse_sig_list(rest),
        "ack" => parse_sig_ack(rest),
        "unack" => parse_sig_unack(rest),
        _ => bail!("unknown sig subcommand: {subcommand}"),
    }
}

fn parse_sig_list(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SigListArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if arg == "--include-acknowledged" {
            parsed.include_acknowledged = true;
        } else if let Some(v) = flags.match_value(&arg, "--limit")? {
            parsed.limit = Some(parse_u32_flag("--limit", v)?);
        } else {
            bail!("unknown sig list option: {arg}");
        }
    }
    Ok(CliCommand::Sig(SigCommand::List(parsed)))
}

fn parse_sig_ack(args: &[String]) -> Result<CliCommand> {
    let (hash, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("sig ack requires a signature hash"))?;
    if hash.starts_with('-') {
        bail!("sig ack requires a signature hash as the first positional argument");
    }
    let mut parsed = SigAckArgs {
        signature_hash: hash.clone(),
        ..Default::default()
    };
    let mut flags = FlagCursor::new(rest);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--notes")? {
            parsed.notes = Some(v);
        } else {
            bail!("unknown sig ack option: {arg}");
        }
    }
    Ok(CliCommand::Sig(SigCommand::Ack(parsed)))
}

fn parse_sig_unack(args: &[String]) -> Result<CliCommand> {
    let (hash, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("sig unack requires a signature hash"))?;
    if hash.starts_with('-') {
        bail!("sig unack requires a signature hash as the first positional argument");
    }
    let mut parsed = SigUnackArgs {
        signature_hash: hash.clone(),
        ..Default::default()
    };
    let mut flags = FlagCursor::new(rest);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--reason")? {
            parsed.reason = Some(v);
        } else {
            bail!("unknown sig unack option: {arg}");
        }
    }
    Ok(CliCommand::Sig(SigCommand::Unack(parsed)))
}
#[cfg(test)]
#[path = "sig_tests.rs"]
mod tests;
