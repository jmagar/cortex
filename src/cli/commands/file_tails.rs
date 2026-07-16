use anyhow::{Result, anyhow, bail};

use crate::cli::{FileTailAddArgs, FileTailCommand, FileTailIdArgs, FileTailListArgs, suggest};

pub(crate) fn parse_file_tail_command(args: &[String]) -> Result<FileTailCommand> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("filetail subcommand is required"))?;
    match command.as_str() {
        "list" => Ok(FileTailCommand::List(parse_list(rest)?)),
        "status" => Ok(FileTailCommand::Status(parse_list(rest)?)),
        "add" => Ok(FileTailCommand::Add(parse_add(rest)?)),
        "remove" => Ok(FileTailCommand::Remove(parse_id(rest)?)),
        "enable" => Ok(FileTailCommand::Enable(parse_id(rest)?)),
        "disable" => Ok(FileTailCommand::Disable(parse_id(rest)?)),
        _ => bail!(
            "{}",
            suggest::unknown_command(
                "filetail subcommand",
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
                suggest::unknown_option("filetail list", other, &["--json"])
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
            other if !other.starts_with('-') && id.is_none() => id = Some(other.to_string()),
            other => bail!(
                "{}",
                suggest::unknown_option("filetail", other, &["--id", "--json"])
            ),
        }
        i += 1;
    }
    Ok(FileTailIdArgs {
        id: id.ok_or_else(|| anyhow!("filetail requires an id"))?,
        json,
    })
}

fn parse_add(args: &[String]) -> Result<FileTailAddArgs> {
    let mut out = FileTailAddArgs {
        id: None,
        path: String::new(),
        tag: None,
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
                out.id = Some(required(args, i, "--id")?);
            }
            "--path" => {
                i += 1;
                out.path = required(args, i, "--path")?;
            }
            "--tag" => {
                i += 1;
                out.tag = Some(required(args, i, "--tag")?);
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
            other if !other.starts_with('-') && out.path.is_empty() => {
                out.path = other.to_string();
            }
            other => bail!(
                "{}",
                suggest::unknown_option(
                    "filetail add",
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
    if out.path.is_empty() {
        bail!("filetail add requires a path");
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
    "Usage: cortex ingest filetail list [--json]\n       cortex ingest filetail status [--json]\n       cortex ingest filetail add PATH [--id ID] [--tag TAG] [--host HOST] [--facility FACILITY] [--severity SEVERITY] [--from-start] [--json]\n       cortex ingest filetail remove ID [--json]\n       cortex ingest filetail enable ID [--json]\n       cortex ingest filetail disable ID [--json]"
}
