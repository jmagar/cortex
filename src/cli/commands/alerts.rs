//! Parse functions for `cortex alerts` subcommands.

use anyhow::{Result, anyhow, bail};

use super::super::args::{
    AlertsCommand, CliCommand, NotifyCommand, NotifyRecentArgs, NotifyTestArgs, SigAckArgs,
    SigCommand, SigListArgs, SigUnackArgs,
};
use super::super::{FlagCursor, norm_time, parse_i64_flag, parse_u32_flag};

pub(crate) fn parse_alerts(args: &[String]) -> Result<CliCommand> {
    let (domain, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("alerts requires a subcommand (signatures|notifications)"))?;
    match domain.as_str() {
        "signatures" => parse_alert_signatures(rest),
        "notifications" => parse_alert_notifications(rest),
        _ => bail!(
            "{}",
            super::super::suggest::unknown_command(
                "alerts subcommand",
                domain,
                &["signatures", "notifications"],
            )
        ),
    }
}

fn parse_alert_signatures(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("alerts signatures requires a subcommand (list|ack|unack)"))?;
    let command = match subcommand.as_str() {
        "list" => SigCommand::List(parse_signature_list(rest)?),
        "ack" => SigCommand::Ack(parse_signature_ack(rest)?),
        "unack" => SigCommand::Unack(parse_signature_unack(rest)?),
        _ => bail!(
            "{}",
            super::super::suggest::unknown_command(
                "alerts signatures subcommand",
                subcommand,
                &["list", "ack", "unack"],
            )
        ),
    };
    Ok(CliCommand::Alerts(AlertsCommand::Signatures(command)))
}

fn parse_signature_list(args: &[String]) -> Result<SigListArgs> {
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
            bail!("unknown alerts signatures list option: {arg}");
        }
    }
    Ok(parsed)
}

fn parse_signature_ack(args: &[String]) -> Result<SigAckArgs> {
    let (hash, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("alerts signatures ack requires a signature hash"))?;
    if hash.starts_with('-') {
        bail!("alerts signatures ack requires a signature hash as the first positional argument");
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
            bail!("unknown alerts signatures ack option: {arg}");
        }
    }
    Ok(parsed)
}

fn parse_signature_unack(args: &[String]) -> Result<SigUnackArgs> {
    let (hash, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("alerts signatures unack requires a signature hash"))?;
    if hash.starts_with('-') {
        bail!("alerts signatures unack requires a signature hash as the first positional argument");
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
            bail!("unknown alerts signatures unack option: {arg}");
        }
    }
    Ok(parsed)
}

fn parse_alert_notifications(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("alerts notifications requires a subcommand (recent|test)"))?;
    let command = match subcommand.as_str() {
        "recent" => NotifyCommand::Recent(parse_notifications_recent(rest)?),
        "test" => NotifyCommand::Test(parse_notifications_test(rest)?),
        _ => bail!(
            "{}",
            super::super::suggest::unknown_command(
                "alerts notifications subcommand",
                subcommand,
                &["recent", "test"],
            )
        ),
    };
    Ok(CliCommand::Alerts(AlertsCommand::Notifications(command)))
}

fn parse_notifications_recent(args: &[String]) -> Result<NotifyRecentArgs> {
    let mut parsed = NotifyRecentArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--rule-id")? {
            parsed.rule_id = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--since")? {
            parsed.since = Some(norm_time(v)?);
        } else if let Some(v) = flags.match_value(&arg, "--limit")? {
            parsed.limit = Some(parse_i64_flag("--limit", v)?);
        } else {
            bail!("unknown alerts notifications recent option: {arg}");
        }
    }
    Ok(parsed)
}

fn parse_notifications_test(args: &[String]) -> Result<NotifyTestArgs> {
    let mut parsed = NotifyTestArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--body")? {
            parsed.body = Some(v);
        } else {
            bail!("unknown alerts notifications test option: {arg}");
        }
    }
    Ok(parsed)
}

#[cfg(test)]
#[path = "alerts_tests.rs"]
mod tests;
