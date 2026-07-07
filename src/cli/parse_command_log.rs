use anyhow::{Result, bail};

use super::{
    ShellAgentCommand, ShellAgentIndexArgs, ShellAgentWrapArgs, ShellAtuinIndexArgs, ShellCommand,
    ShellIndexArgs, ShellUserCommand,
};

pub(crate) fn parse_shell_command(args: &[String]) -> Result<ShellCommand> {
    let (group, rest) = args
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("shell requires a subcommand (user|agent)"))?;
    match group.as_str() {
        "user" => Ok(ShellCommand::User(parse_shell_user_command(rest)?)),
        "agent" => Ok(ShellCommand::Agent(parse_shell_agent_command(rest)?)),
        _ => bail!(
            "{}",
            super::suggest::unknown_command("shell subcommand", group, &["user", "agent"])
        ),
    }
}

fn parse_shell_user_command(args: &[String]) -> Result<ShellUserCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("shell user subcommand is required"))?;
    match command.as_str() {
        "index" => parse_shell_index(rest),
        "atuin-index" => parse_shell_atuin_index(rest),
        _ => bail!(
            "{}",
            super::suggest::unknown_command(
                "shell user subcommand",
                command,
                &["index", "atuin-index"],
            )
        ),
    }
}

pub(crate) fn parse_shell_agent_command(args: &[String]) -> Result<ShellAgentCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("shell agent subcommand is required"))?;
    match command.as_str() {
        "index" => parse_shell_agent_index(rest),
        "wrap" => parse_shell_agent_wrap(rest),
        _ => bail!(
            "{}",
            super::suggest::unknown_command("shell agent subcommand", command, &["index", "wrap"])
        ),
    }
}

/// Back-compat shim for the pre-restructure grammar: `ingest agent-command
/// {ingest-spool|wrap}`. `ingest-spool` maps to the same `Index` variant as
/// the canonical `index` verb.
pub(crate) fn parse_shell_agent_command_legacy(args: &[String]) -> Result<ShellAgentCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("agent-command subcommand is required"))?;
    match command.as_str() {
        "ingest-spool" => parse_shell_agent_index(rest),
        "wrap" => parse_shell_agent_wrap(rest),
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

fn parse_shell_index(args: &[String]) -> Result<ShellUserCommand> {
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
    Ok(ShellUserCommand::Index(ShellIndexArgs {
        path,
        shell,
        json,
    }))
}

fn parse_shell_atuin_index(args: &[String]) -> Result<ShellUserCommand> {
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
    Ok(ShellUserCommand::AtuinIndex(ShellAtuinIndexArgs {
        path,
        json,
    }))
}

fn parse_shell_agent_index(args: &[String]) -> Result<ShellAgentCommand> {
    let mut path = None;
    let mut json = false;
    let mut server = None;
    let mut token = None;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                i += 1;
                path = Some(required_value(args, i, "--path")?);
            }
            "--json" => json = true,
            "--server" => {
                i += 1;
                server = Some(required_value(args, i, "--server")?);
            }
            "--token" => {
                i += 1;
                token = Some(required_value(args, i, "--token")?);
            }
            other => bail!("unknown shell agent index argument: {other}"),
        }
        i += 1;
    }
    let path = path.ok_or_else(|| anyhow::anyhow!("shell agent index requires --path PATH"))?;
    Ok(ShellAgentCommand::Index(ShellAgentIndexArgs {
        path,
        json,
        server,
        token,
    }))
}

fn parse_shell_agent_wrap(args: &[String]) -> Result<ShellAgentCommand> {
    let mut spool = None;
    let mut probe = false;
    let mut command_start = None;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--spool" => {
                i += 1;
                spool = Some(required_value(args, i, "--spool")?);
            }
            "--probe" => {
                probe = true;
            }
            "--" => {
                command_start = Some(i + 1);
                break;
            }
            other => bail!("unknown shell agent wrap argument: {other}"),
        }
        i += 1;
    }
    // A probe is a liveness check the generated wrapper runs before delegating;
    // it needs neither a spool nor a command.
    if probe {
        return Ok(ShellAgentCommand::Wrap(ShellAgentWrapArgs {
            spool: spool.unwrap_or_default(),
            command: Vec::new(),
            probe: true,
        }));
    }
    let spool = spool.ok_or_else(|| anyhow::anyhow!("shell agent wrap requires --spool PATH"))?;
    let start =
        command_start.ok_or_else(|| anyhow::anyhow!("shell agent wrap requires -- COMMAND"))?;
    let command = args[start..].to_vec();
    if command.is_empty() {
        bail!("shell agent wrap requires COMMAND after --");
    }
    Ok(ShellAgentCommand::Wrap(ShellAgentWrapArgs {
        spool,
        command,
        probe: false,
    }))
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
