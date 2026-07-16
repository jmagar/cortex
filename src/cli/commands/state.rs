//! Parse functions for the grouped `cortex state` command domain.

use anyhow::{Result, anyhow, bail};

use super::super::args::{CliCommand, StateCommand};

pub(crate) fn parse_state(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("state requires a subcommand (host|fleet|clockskew)"))?;
    match subcommand.as_str() {
        "host" => parse_state_host(rest),
        "fleet" => parse_state_fleet(rest),
        "clockskew" => parse_state_clock_skew(rest),
        _ => bail!(
            "{}",
            super::super::suggest::unknown_command(
                "state subcommand",
                subcommand,
                &["host", "fleet", "clockskew"],
            )
        ),
    }
}

fn parse_state_host(args: &[String]) -> Result<CliCommand> {
    Ok(CliCommand::State(StateCommand::Host(
        super::host_state::parse_host_state_args(args)?,
    )))
}

fn parse_state_fleet(args: &[String]) -> Result<CliCommand> {
    Ok(CliCommand::State(StateCommand::Fleet(
        super::fleet_state::parse_fleet_state_args(args)?,
    )))
}

fn parse_state_clock_skew(args: &[String]) -> Result<CliCommand> {
    Ok(CliCommand::State(StateCommand::ClockSkew(
        super::clock_skew::parse_clock_skew_args(args)?,
    )))
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
