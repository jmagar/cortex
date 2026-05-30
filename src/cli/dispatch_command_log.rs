use std::path::PathBuf;

use anyhow::{bail, Result};
use cortex::command_log::{self, CommandLogImportResult};

use super::{
    AgentCommandIngestSpoolArgs, AgentCommandWrapArgs, CliMode, ShellAtuinIndexArgs, ShellIndexArgs,
};

pub(crate) async fn run_shell_index(mode: &CliMode, args: ShellIndexArgs) -> Result<()> {
    let CliMode::Local(service) = mode else {
        bail!("shell index is local-only; run without --http/--server/--token");
    };
    let result = service
        .import_shell_history(PathBuf::from(args.path), args.shell)
        .await?;
    print_import_result("shell index", &result, args.json)
}

pub(crate) async fn run_shell_atuin_index(mode: &CliMode, args: ShellAtuinIndexArgs) -> Result<()> {
    let CliMode::Local(service) = mode else {
        bail!("shell atuin-index is local-only; run without --http/--server/--token");
    };
    let result = service
        .import_atuin_history(PathBuf::from(args.path))
        .await?;
    print_import_result("shell atuin-index", &result, args.json)
}

pub(crate) async fn run_agent_command_ingest_spool(
    mode: &CliMode,
    args: AgentCommandIngestSpoolArgs,
) -> Result<()> {
    let CliMode::Local(service) = mode else {
        bail!("agent-command ingest-spool is local-only; run without --http/--server/--token");
    };
    let result = service
        .import_agent_command_spool(PathBuf::from(args.path))
        .await?;
    print_import_result("agent-command ingest-spool", &result, args.json)
}

pub(crate) fn run_agent_command_wrap(args: AgentCommandWrapArgs) -> Result<i32> {
    command_log::run_agent_command_wrapper(PathBuf::from(args.spool).as_path(), &args.command)
}

fn print_import_result(label: &str, result: &CommandLogImportResult, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(result)?);
    } else {
        println!("{label}");
        println!("scanned: {}", result.scanned);
        println!("imported: {}", result.imported);
        println!("skipped: {}", result.skipped);
        println!("skipped_duplicates: {}", result.skipped_duplicates);
        println!("errors: {}", result.errors);
    }
    Ok(())
}
