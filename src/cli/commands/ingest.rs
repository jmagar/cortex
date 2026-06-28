//! Parse functions for the grouped `cortex ingest` command domain.

use anyhow::{Result, anyhow, bail};

use super::super::args::{CliCommand, IngestCommand, OutputArgs};

pub(crate) fn parse_ingest(args: &[String]) -> Result<CliCommand> {
    let (domain, rest) = args.split_first().ok_or_else(|| {
        anyhow!(
            "ingest requires a subcommand (shell|agent-command|inventory|file-tail|syslog|docker)"
        )
    })?;
    let command = match domain.as_str() {
        "shell" => {
            IngestCommand::Shell(super::super::parse_command_log::parse_shell_command(rest)?)
        }
        "agent-command" => IngestCommand::AgentCommand(
            super::super::parse_command_log::parse_agent_command_command(rest)?,
        ),
        "inventory" => IngestCommand::Inventory(parse_inventory_command(rest)?),
        "file-tail" => IngestCommand::FileTail(super::file_tails::parse_file_tail_command(rest)?),
        "syslog" => parse_syslog(rest)?,
        "docker" => parse_docker(rest)?,
        _ => bail!(
            "{}",
            super::super::suggest::unknown_command(
                "ingest subcommand",
                domain,
                &[
                    "shell",
                    "agent-command",
                    "inventory",
                    "file-tail",
                    "syslog",
                    "docker",
                ],
            )
        ),
    };
    Ok(CliCommand::Ingest(command))
}

pub(crate) fn parse_inventory_command(args: &[String]) -> Result<super::super::InventoryCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("ingest inventory subcommand is required: refresh or status"))?;
    if matches!(command.as_str(), "--help" | "-h" | "help") {
        bail!("{}", inventory_usage());
    }
    let mut json = false;
    for arg in rest {
        match arg.as_str() {
            "--json" => json = true,
            "--help" | "-h" => bail!("{}", inventory_usage()),
            other => bail!(
                "{}",
                super::super::suggest::unknown_option("ingest inventory", other, &["--json"])
            ),
        }
    }
    match command.as_str() {
        "refresh" => Ok(super::super::InventoryCommand::Refresh(
            super::super::InventoryArgs { json },
        )),
        "status" => Ok(super::super::InventoryCommand::Status(
            super::super::InventoryArgs { json },
        )),
        _ => bail!(
            "{}",
            super::super::suggest::unknown_command(
                "ingest inventory subcommand",
                command,
                &["refresh", "status"],
            )
        ),
    }
}

fn parse_syslog(args: &[String]) -> Result<IngestCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("ingest syslog requires a subcommand (status|test)"))?;
    let output = parse_output("ingest syslog", rest)?;
    match command.as_str() {
        "status" => Ok(IngestCommand::SyslogStatus(output)),
        "test" => {
            bail!("ingest syslog test is deferred; status is read-only and does not send frames")
        }
        _ => bail!(
            "{}",
            super::super::suggest::unknown_command(
                "ingest syslog subcommand",
                command,
                &["status", "test"],
            )
        ),
    }
}

fn parse_docker(args: &[String]) -> Result<IngestCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("ingest docker requires a subcommand (status|sources)"))?;
    let output = parse_output("ingest docker", rest)?;
    match command.as_str() {
        "status" => Ok(IngestCommand::DockerStatus(output)),
        "sources" => Ok(IngestCommand::DockerSources(output)),
        _ => bail!(
            "{}",
            super::super::suggest::unknown_command(
                "ingest docker subcommand",
                command,
                &["status", "sources"],
            )
        ),
    }
}

fn parse_output(context: &str, args: &[String]) -> Result<OutputArgs> {
    let mut output = OutputArgs::default();
    for arg in args {
        match arg.as_str() {
            "--json" => output.json = true,
            other => bail!(
                "{}",
                super::super::suggest::unknown_option(context, other, &["--json"])
            ),
        }
    }
    Ok(output)
}

fn inventory_usage() -> &'static str {
    "Usage: cortex ingest inventory refresh [--json]\n       cortex ingest inventory status [--json]"
}

#[cfg(test)]
#[path = "ingest_tests.rs"]
mod tests;
