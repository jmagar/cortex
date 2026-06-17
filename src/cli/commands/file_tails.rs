use anyhow::{Result, anyhow, bail};

use crate::cli::{
    CliCommand, FileTailAddArgs, FileTailCommand, FileTailIdArgs, FileTailListArgs, suggest,
};

pub(crate) fn parse_file_tail(args: &[String]) -> Result<CliCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("file-tail subcommand is required"))?;
    match command.as_str() {
        "list" => Ok(CliCommand::FileTail(FileTailCommand::List(parse_list(
            rest,
        )?))),
        "status" => Ok(CliCommand::FileTail(FileTailCommand::Status(parse_list(
            rest,
        )?))),
        "add" => Ok(CliCommand::FileTail(FileTailCommand::Add(parse_add(rest)?))),
        "remove" => Ok(CliCommand::FileTail(FileTailCommand::Remove(parse_id(
            rest,
        )?))),
        "enable" => Ok(CliCommand::FileTail(FileTailCommand::Enable(parse_id(
            rest,
        )?))),
        "disable" => Ok(CliCommand::FileTail(FileTailCommand::Disable(parse_id(
            rest,
        )?))),
        _ => bail!(
            "{}",
            suggest::unknown_command(
                "file-tail subcommand",
                command,
                &["list", "status", "add", "remove", "enable", "disable"],
            )
        ),
    }
}

fn parse_list(args: &[String]) -> Result<FileTailListArgs> {
    let mut out = FileTailListArgs { json: false };
    for arg in args {
        match arg.as_str() {
            "--json" => out.json = true,
            "--help" | "-h" => bail!("{}", usage()),
            other => bail!(
                "{}",
                suggest::unknown_option("file-tail list", other, &["--json"])
            ),
        }
    }
    Ok(out)
}

fn parse_id(args: &[String]) -> Result<FileTailIdArgs> {
    let mut id = None;
    let mut json = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--id" => {
                i += 1;
                id = Some(required(args, i, "--id")?);
            }
            "--json" => json = true,
            "--help" | "-h" => bail!("{}", usage()),
            other => bail!(
                "{}",
                suggest::unknown_option("file-tail", other, &["--id", "--json"])
            ),
        }
        i += 1;
    }
    Ok(FileTailIdArgs {
        id: id.ok_or_else(|| anyhow!("--id is required"))?,
        json,
    })
}

fn parse_add(args: &[String]) -> Result<FileTailAddArgs> {
    let mut out = FileTailAddArgs {
        id: String::new(),
        path: String::new(),
        tag: String::new(),
        host: None,
        facility: None,
        severity: None,
        start_at_end: true,
        json: false,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--id" => {
                i += 1;
                out.id = required(args, i, "--id")?;
            }
            "--path" => {
                i += 1;
                out.path = required(args, i, "--path")?;
            }
            "--tag" => {
                i += 1;
                out.tag = required(args, i, "--tag")?;
            }
            "--host" => {
                i += 1;
                out.host = Some(required(args, i, "--host")?);
            }
            "--facility" => {
                i += 1;
                out.facility = Some(required(args, i, "--facility")?);
            }
            "--severity" => {
                i += 1;
                out.severity = Some(required(args, i, "--severity")?);
            }
            "--from-start" => out.start_at_end = false,
            "--json" => out.json = true,
            "--help" | "-h" => bail!("{}", usage()),
            other => bail!(
                "{}",
                suggest::unknown_option(
                    "file-tail add",
                    other,
                    &[
                        "--id",
                        "--path",
                        "--tag",
                        "--host",
                        "--facility",
                        "--severity",
                        "--from-start",
                        "--json",
                    ],
                )
            ),
        }
        i += 1;
    }
    if out.id.is_empty() || out.path.is_empty() || out.tag.is_empty() || out.host.is_none() {
        bail!("file-tail add requires --id, --path, --tag, and --host");
    }
    Ok(out)
}

fn required(args: &[String], index: usize, flag: &str) -> Result<String> {
    let value = args
        .get(index)
        .ok_or_else(|| anyhow!("{flag} requires a value"))?;
    if value.trim().is_empty() || value.starts_with('-') {
        bail!("{flag} requires a value");
    }
    Ok(value.clone())
}

fn usage() -> &'static str {
    "Usage: cortex file-tail list [--json]\n       cortex file-tail status [--json]\n       cortex file-tail add --id ID --path PATH --tag TAG --host HOST [--facility FACILITY] [--severity SEVERITY] [--from-start] [--json]\n       cortex file-tail remove --id ID [--json]\n       cortex file-tail enable --id ID [--json]\n       cortex file-tail disable --id ID [--json]"
}
