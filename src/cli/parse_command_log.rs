use anyhow::{Result, bail};

use super::{
    AgentCommandCommand, AgentCommandIngestSpoolArgs, AgentCommandWrapArgs, CliCommand,
    ShellAtuinIndexArgs, ShellCommand, ShellIndexArgs,
};

pub(crate) fn parse_shell(args: &[String]) -> Result<CliCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("shell subcommand is required"))?;
    match command.as_str() {
        "index" => parse_shell_index(rest),
        "atuin-index" => parse_shell_atuin_index(rest),
        _ => bail!(
            "{}",
            super::suggest::unknown_command("shell subcommand", command, &["index", "atuin-index"])
        ),
    }
}

pub(crate) fn parse_agent_command(args: &[String]) -> Result<CliCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("agent-command subcommand is required"))?;
    match command.as_str() {
        "ingest-spool" => parse_agent_command_ingest_spool(rest),
        "wrap" => parse_agent_command_wrap(rest),
        _ => bail!(
            "{}",
            super::suggest::unknown_command(
                "agent-command subcommand",
                command,
                &["ingest-spool", "wrap"],
            )
        ),
    }
}

fn parse_shell_index(args: &[String]) -> Result<CliCommand> {
    let mut path = None;
    let mut shell = "zsh".to_string();
    let mut json = false;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                i += 1;
                path = Some(required_value(args, i, "--path")?);
            }
            "--shell" => {
                i += 1;
                shell = required_value(args, i, "--shell")?;
            }
            "--json" => json = true,
            other => bail!("unknown shell index argument: {other}"),
        }
        i += 1;
    }
    let path = path.ok_or_else(|| anyhow::anyhow!("shell index requires --path PATH"))?;
    if shell != "zsh" {
        bail!("shell index currently supports only --shell zsh");
    }
    Ok(CliCommand::Shell(ShellCommand::Index(ShellIndexArgs {
        path,
        shell,
        json,
    })))
}

fn parse_shell_atuin_index(args: &[String]) -> Result<CliCommand> {
    let mut path = None;
    let mut json = false;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                i += 1;
                path = Some(required_value(args, i, "--path")?);
            }
            "--json" => json = true,
            other => bail!("unknown shell atuin-index argument: {other}"),
        }
        i += 1;
    }
    let path = path.ok_or_else(|| anyhow::anyhow!("shell atuin-index requires --path PATH"))?;
    Ok(CliCommand::Shell(ShellCommand::AtuinIndex(
        ShellAtuinIndexArgs { path, json },
    )))
}

fn parse_agent_command_ingest_spool(args: &[String]) -> Result<CliCommand> {
    let mut path = None;
    let mut json = false;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                i += 1;
                path = Some(required_value(args, i, "--path")?);
            }
            "--json" => json = true,
            other => bail!("unknown agent-command ingest-spool argument: {other}"),
        }
        i += 1;
    }
    let path =
        path.ok_or_else(|| anyhow::anyhow!("agent-command ingest-spool requires --path PATH"))?;
    Ok(CliCommand::AgentCommand(AgentCommandCommand::IngestSpool(
        AgentCommandIngestSpoolArgs { path, json },
    )))
}

fn parse_agent_command_wrap(args: &[String]) -> Result<CliCommand> {
    let mut spool = None;
    let mut command_start = None;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--spool" => {
                i += 1;
                spool = Some(required_value(args, i, "--spool")?);
            }
            "--" => {
                command_start = Some(i + 1);
                break;
            }
            other => bail!("unknown agent-command wrap argument: {other}"),
        }
        i += 1;
    }
    let spool = spool.ok_or_else(|| anyhow::anyhow!("agent-command wrap requires --spool PATH"))?;
    let start =
        command_start.ok_or_else(|| anyhow::anyhow!("agent-command wrap requires -- COMMAND"))?;
    let command = args[start..].to_vec();
    if command.is_empty() {
        bail!("agent-command wrap requires COMMAND after --");
    }
    Ok(CliCommand::AgentCommand(AgentCommandCommand::Wrap(
        AgentCommandWrapArgs { spool, command },
    )))
}

fn required_value(args: &[String], index: usize, flag: &str) -> Result<String> {
    let value = args
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))?;
    if value.starts_with('-') {
        bail!("{flag} requires a value");
    }
    Ok(value.clone())
}

#[cfg(test)]
#[path = "parse_command_log_tests.rs"]
mod tests;
