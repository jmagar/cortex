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
    assert!(
        shell_err
            .to_string()
            .contains("shell user index is local-only")
    );

    let atuin_err = run_shell_atuin_index(
        &mode,
        ShellAtuinIndexArgs {
            path: Some("/tmp/atuin.db".to_string()),
            json: true,
        },
    )
    .await
    .unwrap_err();
    assert!(
        atuin_err
            .to_string()
            .contains("shell user atuinindex is local-only")
    );
}

#[test]
fn atuin_history_path_uses_standard_and_override_locations() {
    assert_eq!(
        resolve_atuin_history_path(
            Some(PathBuf::from("/tmp/custom-atuin.db")),
            Some(PathBuf::from("/tmp/xdg")),
            Some(PathBuf::from("/tmp/home")),
        ),
        PathBuf::from("/tmp/custom-atuin.db")
    );
    assert_eq!(
        resolve_atuin_history_path(
            None,
            Some(PathBuf::from("/tmp/xdg")),
            Some(PathBuf::from("/tmp/home")),
        ),
        PathBuf::from("/tmp/xdg/atuin/history.db")
    );
}

#[tokio::test]
async fn agent_command_spool_import_is_local_only() {
    let err = run_shell_agent_index_local(
        &http_mode(),
        ShellAgentIndexArgs {
            path: "/tmp/spool".to_string(),
            json: false,
            server: None,
            token: None,
        },
    )
    .await
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("shell agent index is local-only without --server")
    );
}
