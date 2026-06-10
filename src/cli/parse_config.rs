use anyhow::{Result, anyhow, bail};
use std::path::PathBuf;

use super::parse_common::{FlagCursor, value_after_equals};
use super::{
    CliCommand, ConfigCommand, ConfigGetArgs, ConfigListArgs, ConfigSetArgs, ConfigTarget,
    ConfigUnsetArgs,
};
pub(crate) fn parse_config(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("config requires a subcommand (get|set|unset|list)"))?;
    match subcommand.as_str() {
        "get" => Ok(CliCommand::Config(ConfigCommand::Get(parse_config_get(
            rest,
        )?))),
        "set" => Ok(CliCommand::Config(ConfigCommand::Set(parse_config_set(
            rest,
        )?))),
        "unset" => Ok(CliCommand::Config(ConfigCommand::Unset(
            parse_config_unset(rest)?,
        ))),
        "list" | "ls" => Ok(CliCommand::Config(ConfigCommand::List(parse_config_list(
            rest,
        )?))),
        other => bail!("unknown config subcommand: {other}"),
    }
}

pub(crate) fn parse_config_get(args: &[String]) -> Result<ConfigGetArgs> {
    let mut parsed = ConfigGetArgs::default();
    let mut positionals = Vec::new();
    parse_config_flags(
        args,
        &mut parsed.target,
        &mut parsed.toml_path,
        &mut parsed.json,
        &mut positionals,
        "get",
    )?;
    match positionals.len() {
        1 => parsed.key = positionals.into_iter().next().unwrap(),
        0 => bail!("config get requires a KEY"),
        _ => bail!("config get expects exactly one KEY"),
    }
    Ok(parsed)
}

pub(crate) fn parse_config_set(args: &[String]) -> Result<ConfigSetArgs> {
    let mut parsed = ConfigSetArgs::default();
    let mut positionals = Vec::new();
    parse_config_flags(
        args,
        &mut parsed.target,
        &mut parsed.toml_path,
        &mut parsed.json,
        &mut positionals,
        "set",
    )?;
    match positionals.len() {
        2 => {
            let mut iter = positionals.into_iter();
            parsed.key = iter.next().unwrap();
            parsed.value = iter.next().unwrap();
        }
        1 => {
            let only = positionals.into_iter().next().unwrap();
            let (k, v) = only
                .split_once('=')
                .ok_or_else(|| anyhow!("config set requires KEY VALUE or KEY=VALUE"))?;
            if k.is_empty() {
                bail!("config set KEY must not be empty");
            }
            parsed.key = k.to_string();
            parsed.value = v.to_string();
        }
        0 => bail!("config set requires KEY VALUE"),
        _ => bail!("config set expects KEY VALUE (got too many positionals)"),
    }
    Ok(parsed)
}

pub(crate) fn parse_config_unset(args: &[String]) -> Result<ConfigUnsetArgs> {
    let mut parsed = ConfigUnsetArgs::default();
    let mut positionals = Vec::new();
    parse_config_flags(
        args,
        &mut parsed.target,
        &mut parsed.toml_path,
        &mut parsed.json,
        &mut positionals,
        "unset",
    )?;
    match positionals.len() {
        1 => parsed.key = positionals.into_iter().next().unwrap(),
        0 => bail!("config unset requires a KEY"),
        _ => bail!("config unset expects exactly one KEY"),
    }
    Ok(parsed)
}

pub(crate) fn parse_config_list(args: &[String]) -> Result<ConfigListArgs> {
    let mut parsed = ConfigListArgs::default();
    let mut positionals = Vec::new();
    parse_config_flags(
        args,
        &mut parsed.target,
        &mut parsed.toml_path,
        &mut parsed.json,
        &mut positionals,
        "list",
    )?;
    if !positionals.is_empty() {
        bail!("config list does not take positional arguments");
    }
    Ok(parsed)
}

pub(crate) fn parse_config_flags(
    args: &[String],
    target: &mut ConfigTarget,
    toml_path: &mut Option<PathBuf>,
    json: &mut bool,
    positionals: &mut Vec<String>,
    sub: &str,
) -> Result<()> {
    let mut flags = FlagCursor::new(args);
    let mut target_set = false;
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => *json = true,
            "--env" => {
                if target_set && !matches!(target, ConfigTarget::Env) {
                    bail!("--env and --toml are mutually exclusive");
                }
                *target = ConfigTarget::Env;
                target_set = true;
            }
            "--toml" => {
                if target_set && !matches!(target, ConfigTarget::Toml) {
                    bail!("--env and --toml are mutually exclusive");
                }
                *target = ConfigTarget::Toml;
                target_set = true;
            }
            "--toml-path" => *toml_path = Some(PathBuf::from(flags.value("--toml-path")?)),
            _ if arg.starts_with("--toml-path=") => {
                *toml_path = Some(PathBuf::from(value_after_equals(arg, "--toml-path")?));
            }
            "-h" | "--help" => bail!("use `cortex --help` for usage"),
            _ if arg.starts_with('-') => bail!("unknown config {sub} option: {arg}"),
            _ => positionals.push(arg),
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "parse_config_tests.rs"]
mod tests;
