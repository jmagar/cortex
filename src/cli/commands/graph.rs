//! Parse functions for `cortex entity` and `cortex graph`.

use anyhow::{bail, Result};

use crate::cli::parse_common::{value_after_equals, FlagCursor};
use crate::cli::{parse_i64_flag, parse_u32_flag};
use crate::cli::{CliCommand, EntityArgs, GraphAroundArgs, GraphCommand};

const GRAPH_ENTITY_TYPES: &[&str] = &[
    "host",
    "container",
    "service",
    "app",
    "source_ip",
    "ai_project",
    "ai_session",
    "error_signature",
];

pub(crate) fn parse_entity(args: &[String]) -> Result<CliCommand> {
    let mut parsed = EntityArgs::default();
    let mut positionals = Vec::new();
    let mut cursor = FlagCursor::new(args);
    while let Some(arg) = cursor.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--alias-type" => parsed.alias_type = Some(cursor.value("--alias-type")?),
            "--alias-key" => parsed.alias_key = Some(cursor.value("--alias-key")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", cursor.value("--limit")?)?),
            "--evidence-sample-limit" => {
                parsed.evidence_sample_limit = Some(parse_u32_flag(
                    "--evidence-sample-limit",
                    cursor.value("--evidence-sample-limit")?,
                )?)
            }
            "--payload-budget" => {
                parsed.payload_budget = Some(parse_u32_flag(
                    "--payload-budget",
                    cursor.value("--payload-budget")?,
                )?)
            }
            _ if arg.starts_with("--alias-type=") => {
                parsed.alias_type = Some(value_after_equals(arg, "--alias-type")?)
            }
            _ if arg.starts_with("--alias-key=") => {
                parsed.alias_key = Some(value_after_equals(arg, "--alias-key")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with("--evidence-sample-limit=") => {
                parsed.evidence_sample_limit = Some(parse_u32_flag(
                    "--evidence-sample-limit",
                    value_after_equals(arg, "--evidence-sample-limit")?,
                )?)
            }
            _ if arg.starts_with("--payload-budget=") => {
                parsed.payload_budget = Some(parse_u32_flag(
                    "--payload-budget",
                    value_after_equals(arg, "--payload-budget")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown entity option: {arg}"),
            _ => positionals.push(arg.clone()),
        }
    }

    apply_entity_positionals(&mut parsed, &positionals, "entity")?;
    Ok(CliCommand::Entity(parsed))
}

pub(crate) fn parse_graph(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("graph subcommand is required"))?;
    match subcommand.as_str() {
        "around" => parse_graph_around(rest),
        other => bail!("unknown graph subcommand: {other}"),
    }
}

fn parse_graph_around(args: &[String]) -> Result<CliCommand> {
    let mut parsed = GraphAroundArgs {
        depth: Some(1),
        ..Default::default()
    };
    let mut positionals = Vec::new();
    let mut cursor = FlagCursor::new(args);
    while let Some(arg) = cursor.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--entity-id" => {
                parsed.entity_id =
                    Some(parse_i64_flag("--entity-id", cursor.value("--entity-id")?)?)
            }
            "--alias-type" => parsed.alias_type = Some(cursor.value("--alias-type")?),
            "--alias-key" => parsed.alias_key = Some(cursor.value("--alias-key")?),
            "--depth" => parsed.depth = Some(parse_u32_flag("--depth", cursor.value("--depth")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", cursor.value("--limit")?)?),
            "--evidence-sample-limit" => {
                parsed.evidence_sample_limit = Some(parse_u32_flag(
                    "--evidence-sample-limit",
                    cursor.value("--evidence-sample-limit")?,
                )?)
            }
            "--payload-budget" => {
                parsed.payload_budget = Some(parse_u32_flag(
                    "--payload-budget",
                    cursor.value("--payload-budget")?,
                )?)
            }
            _ if arg.starts_with("--entity-id=") => {
                parsed.entity_id = Some(parse_i64_flag(
                    "--entity-id",
                    value_after_equals(arg, "--entity-id")?,
                )?)
            }
            _ if arg.starts_with("--alias-type=") => {
                parsed.alias_type = Some(value_after_equals(arg, "--alias-type")?)
            }
            _ if arg.starts_with("--alias-key=") => {
                parsed.alias_key = Some(value_after_equals(arg, "--alias-key")?)
            }
            _ if arg.starts_with("--depth=") => {
                parsed.depth = Some(parse_u32_flag(
                    "--depth",
                    value_after_equals(arg, "--depth")?,
                )?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with("--evidence-sample-limit=") => {
                parsed.evidence_sample_limit = Some(parse_u32_flag(
                    "--evidence-sample-limit",
                    value_after_equals(arg, "--evidence-sample-limit")?,
                )?)
            }
            _ if arg.starts_with("--payload-budget=") => {
                parsed.payload_budget = Some(parse_u32_flag(
                    "--payload-budget",
                    value_after_equals(arg, "--payload-budget")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown graph around option: {arg}"),
            _ => positionals.push(arg.clone()),
        }
    }

    apply_graph_positionals(&mut parsed, &positionals)?;
    if parsed.entity_id.is_none()
        && !has_exact_lookup(parsed.entity_type.as_deref(), parsed.key.as_deref())
        && !has_alias_lookup(parsed.alias_type.as_deref(), parsed.alias_key.as_deref())
    {
        bail!("graph around requires --entity-id, <entity-type> <key>, <entity-type:key>, or --alias-type with --alias-key");
    }
    Ok(CliCommand::Graph(GraphCommand::Around(parsed)))
}

fn apply_entity_positionals(
    parsed: &mut EntityArgs,
    positionals: &[String],
    command_name: &str,
) -> Result<()> {
    match positionals {
        [] => {
            if !has_alias_lookup(parsed.alias_type.as_deref(), parsed.alias_key.as_deref()) {
                bail!(
                    "{command_name} requires <entity-type> <key> or --alias-type with --alias-key"
                );
            }
        }
        [combined] => {
            let (entity_type, key) = split_entity_key(combined)?;
            parsed.entity_type = Some(entity_type);
            parsed.key = Some(key);
        }
        [entity_type, key] => {
            validate_entity_type(entity_type)?;
            parsed.entity_type = Some(entity_type.clone());
            parsed.key = Some(key.clone());
        }
        _ => bail!("{command_name} accepts at most two positional arguments"),
    }
    Ok(())
}

fn apply_graph_positionals(parsed: &mut GraphAroundArgs, positionals: &[String]) -> Result<()> {
    match positionals {
        [] => {}
        [combined] => {
            let (entity_type, key) = split_entity_key(combined)?;
            parsed.entity_type = Some(entity_type);
            parsed.key = Some(key);
        }
        [entity_type, key] => {
            validate_entity_type(entity_type)?;
            parsed.entity_type = Some(entity_type.clone());
            parsed.key = Some(key.clone());
        }
        _ => bail!("graph around accepts at most two positional entity arguments"),
    }
    Ok(())
}

fn split_entity_key(value: &str) -> Result<(String, String)> {
    let Some((entity_type, key)) = value.split_once(':') else {
        bail!("single positional entity must use <entity-type:key>");
    };
    if key.trim().is_empty() {
        bail!("entity key must be non-empty");
    }
    validate_entity_type(entity_type)?;
    Ok((entity_type.to_string(), key.to_string()))
}

fn validate_entity_type(entity_type: &str) -> Result<()> {
    if GRAPH_ENTITY_TYPES.contains(&entity_type) {
        Ok(())
    } else {
        bail!("unsupported graph entity type: {entity_type}");
    }
}

fn has_exact_lookup(entity_type: Option<&str>, key: Option<&str>) -> bool {
    entity_type.is_some_and(|value| !value.trim().is_empty())
        && key.is_some_and(|value| !value.trim().is_empty())
}

fn has_alias_lookup(alias_type: Option<&str>, alias_key: Option<&str>) -> bool {
    alias_type.is_some_and(|value| !value.trim().is_empty())
        && alias_key.is_some_and(|value| !value.trim().is_empty())
}
