//! `cortex assess` — unified verb namespace for LLM-guarded and
//! deterministic incident assessment. Locked dispatcher shape consumed by
//! the `hooks` phase: `AssessCommand::Hooks` is a minimal stub that phase
//! replaces wholesale — do not add real hooks logic here. `mcp` is
//! implemented in this file (GH #104).

use anyhow::{Result, anyhow, bail};

use super::super::args::{
    AssessAbuseArgs, AssessCommand, AssessMcpArgs, AssessSkillArgs, CliCommand,
};
use super::super::parse_common::{FlagCursor, norm_time, parse_u32_flag};
use super::super::suggest;

pub(crate) fn parse_assess(args: &[String]) -> Result<CliCommand> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("assess requires a subcommand: skill, abuse, mcp, hooks"))?;
    match subcommand.as_str() {
        "skill" => parse_assess_skill_from(rest),
        "abuse" => parse_assess_abuse(rest),
        "mcp" => parse_assess_mcp_from(rest),
        "hooks" => bail!("cortex assess hooks is not yet implemented"),
        _ => bail!(
            "{}",
            suggest::unknown_command(
                "assess subcommand",
                subcommand,
                &["skill", "abuse", "mcp", "hooks"],
            )
        ),
    }
}

/// `pub(crate)` (not private) because `cortex sessions skill-assess`
/// forwards to this directly so the two entry points never drift on flag
/// parsing.
pub(crate) fn parse_assess_skill_from(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AssessSkillArgs::default();
    let mut positional: Option<String> = None;
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--all" => parsed.all = true,
            "--no-llm" => parsed.no_llm = true,
            "--plugin" => parsed.plugin = Some(flags.value("--plugin")?),
            "--model" => parsed.model = Some(flags.value("--model")?),
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--window-minutes" => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--correlation-window-minutes" => {
                parsed.correlation_window_minutes = Some(parse_u32_flag(
                    "--correlation-window-minutes",
                    flags.value("--correlation-window-minutes")?,
                )?)
            }
            other if !other.starts_with('-') && positional.is_none() => {
                positional = Some(other.to_string());
            }
            other => bail!(
                "{}",
                suggest::unknown_option(
                    "assess skill",
                    other,
                    &[
                        "--json",
                        "--all",
                        "--no-llm",
                        "--plugin",
                        "--model",
                        "--project",
                        "--tool",
                        "--since",
                        "--until",
                        "--limit",
                        "--window-minutes",
                        "--correlation-window-minutes",
                    ],
                )
            ),
        }
    }
    parsed.skill = positional;
    if parsed.skill.is_none() && parsed.plugin.is_none() {
        bail!(
            "assess skill: skill name or --plugin is required, e.g. `cortex assess skill cortex-frustration-assessment` or `cortex assess skill --plugin lavra`"
        );
    }
    Ok(CliCommand::Assess(AssessCommand::Skill(parsed)))
}

pub(crate) fn parse_assess_abuse(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AssessAbuseArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--no-llm" => parsed.no_llm = true,
            "--incident-id" => parsed.incident_id = Some(flags.value("--incident-id")?),
            "--model" => parsed.model = Some(flags.value("--model")?),
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--window-minutes" => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--correlation-window-minutes" => {
                parsed.correlation_window_minutes = Some(parse_u32_flag(
                    "--correlation-window-minutes",
                    flags.value("--correlation-window-minutes")?,
                )?)
            }
            other => bail!(
                "{}",
                suggest::unknown_option(
                    "assess abuse",
                    other,
                    &[
                        "--json",
                        "--no-llm",
                        "--incident-id",
                        "--model",
                        "--project",
                        "--tool",
                        "--since",
                        "--until",
                        "--limit",
                        "--window-minutes",
                        "--correlation-window-minutes",
                    ],
                )
            ),
        }
    }
    Ok(CliCommand::Assess(AssessCommand::Abuse(parsed)))
}

/// `pub(crate)` (not private) because `cortex sessions mcp-assess` forwards
/// to this directly so the two entry points never drift on flag parsing,
/// mirroring `parse_assess_skill_from`.
pub(crate) fn parse_assess_mcp_from(args: &[String]) -> Result<CliCommand> {
    let mut parsed = AssessMcpArgs::default();
    let mut positional: Option<String> = None;
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--all" => parsed.all = true,
            "--no-llm" => parsed.no_llm = true,
            "--server" => parsed.server = Some(flags.value("--server")?),
            "--tool-name" => parsed.tool_name = Some(flags.value("--tool-name")?),
            "--model" => parsed.model = Some(flags.value("--model")?),
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--since" => parsed.since = Some(norm_time(flags.value("--since")?)?),
            "--until" => parsed.until = Some(norm_time(flags.value("--until")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--window-minutes" => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--correlation-window-minutes" => {
                parsed.correlation_window_minutes = Some(parse_u32_flag(
                    "--correlation-window-minutes",
                    flags.value("--correlation-window-minutes")?,
                )?)
            }
            other if !other.starts_with('-') && positional.is_none() => {
                positional = Some(other.to_string());
            }
            other => bail!(
                "{}",
                suggest::unknown_option(
                    "assess mcp",
                    other,
                    &[
                        "--json",
                        "--all",
                        "--no-llm",
                        "--server",
                        "--tool-name",
                        "--model",
                        "--project",
                        "--tool",
                        "--since",
                        "--until",
                        "--limit",
                        "--window-minutes",
                        "--correlation-window-minutes",
                    ],
                )
            ),
        }
    }
    parsed.target = positional;
    if parsed.target.is_none() && parsed.server.is_none() && parsed.tool_name.is_none() {
        bail!(
            "assess mcp: an mcp server/tool name is required, e.g. `cortex assess mcp cortex` or `cortex assess mcp --server labby --tool-name search`"
        );
    }
    Ok(CliCommand::Assess(AssessCommand::Mcp(parsed)))
}

#[cfg(test)]
#[path = "assess_tests.rs"]
mod tests;
