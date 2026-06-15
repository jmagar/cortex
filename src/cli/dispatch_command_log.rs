use std::path::PathBuf;

use anyhow::{Result, bail};
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::http_client::HttpClient;

    fn http_mode() -> CliMode {
        let client = HttpClient::discover(
            Some("http://127.0.0.1:1".to_string()),
            Some("token".to_string()),
        )
        .expect("http client");
        CliMode::Http(client)
    }

    #[tokio::test]
    async fn shell_import_commands_are_local_only() {
        let mode = http_mode();

        let shell_err = run_shell_index(
            &mode,
            ShellIndexArgs {
                path: "/tmp/history".to_string(),
                shell: "zsh".to_string(),
                json: false,
            },
        )
        .await
        .unwrap_err();
        assert!(shell_err.to_string().contains("shell index is local-only"));

        let atuin_err = run_shell_atuin_index(
            &mode,
            ShellAtuinIndexArgs {
                path: "/tmp/atuin.db".to_string(),
                json: true,
            },
        )
        .await
        .unwrap_err();
        assert!(
            atuin_err
                .to_string()
                .contains("shell atuin-index is local-only")
        );
    }

    #[tokio::test]
    async fn agent_command_spool_import_is_local_only() {
        let err = run_agent_command_ingest_spool(
            &http_mode(),
            AgentCommandIngestSpoolArgs {
                path: "/tmp/spool".to_string(),
                json: false,
            },
        )
        .await
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("agent-command ingest-spool is local-only")
        );
    }
}
