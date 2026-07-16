use std::path::PathBuf;

use anyhow::{Result, bail};
use cortex::command_log::{self, CommandLogImportResult};

use super::{
    CliMode, ShellAgentIndexArgs, ShellAgentWrapArgs, ShellAtuinIndexArgs, ShellIndexArgs,
};

pub(crate) async fn run_shell_index(mode: &CliMode, args: ShellIndexArgs) -> Result<()> {
    let CliMode::Local(service) = mode else {
        bail!("shell user index is local-only; run without --http/--server/--token");
    };
    let result = service
        .import_shell_history(PathBuf::from(args.path), args.shell)
        .await?;
    print_import_result("shell user index", &result, args.json)
}

pub(crate) async fn run_shell_atuin_index(mode: &CliMode, args: ShellAtuinIndexArgs) -> Result<()> {
    let CliMode::Local(service) = mode else {
        bail!("shell user atuinindex is local-only; run without --http/--server/--token");
    };
    let result = service
        .import_atuin_history(PathBuf::from(args.path))
        .await?;
    print_import_result("shell user atuinindex", &result, args.json)
}

pub(crate) async fn run_shell_agent_index_local(
    mode: &CliMode,
    args: ShellAgentIndexArgs,
) -> Result<()> {
    let CliMode::Local(service) = mode else {
        bail!("shell agent index is local-only without --server; pass --server URL to forward");
    };
    let result = service
        .import_agent_command_spool(PathBuf::from(args.path))
        .await?;
    print_import_result("shell agent index", &result, args.json)
}

pub(crate) async fn run_shell_agent_index_remote(
    args: ShellAgentIndexArgs,
    server: String,
) -> Result<()> {
    let result = command_log::forward_agent_command_spool(
        std::path::Path::new(&args.path),
        &server,
        args.token.as_deref(),
    )
    .await?;
    print_import_result("shell agent index (forwarded)", &result, args.json)
}

pub(crate) fn run_shell_agent_wrap(args: ShellAgentWrapArgs) -> Result<i32> {
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

#[cfg(test)]
#[path = "dispatch_command_log_tests.rs"]
mod tests;
