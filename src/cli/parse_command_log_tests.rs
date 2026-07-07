use super::*;

#[test]
fn parses_shell_index() {
    let args = vec![
        "user".to_string(),
        "index".to_string(),
        "--path".to_string(),
        "/tmp/.zsh_history".to_string(),
        "--json".to_string(),
    ];

    let command = parse_shell_command(&args).unwrap();

    match command {
        ShellCommand::User(ShellUserCommand::Index(args)) => {
            assert_eq!(args.path, "/tmp/.zsh_history");
            assert_eq!(args.shell, "zsh");
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_shell_atuin_index() {
    let args = vec![
        "user".to_string(),
        "atuin-index".to_string(),
        "--path".to_string(),
        "/tmp/atuin/history.db".to_string(),
        "--json".to_string(),
    ];

    let command = parse_shell_command(&args).unwrap();

    match command {
        ShellCommand::User(ShellUserCommand::AtuinIndex(args)) => {
            assert_eq!(args.path, "/tmp/atuin/history.db");
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_shell_agent_index() {
    let args = vec![
        "index".to_string(),
        "--path".to_string(),
        "/tmp/commands.jsonl".to_string(),
    ];

    let command = parse_shell_agent_command(&args).unwrap();

    match command {
        ShellAgentCommand::Index(args) => {
            assert_eq!(args.path, "/tmp/commands.jsonl");
            assert!(!args.json);
            assert!(args.server.is_none());
            assert!(args.token.is_none());
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_shell_agent_index_with_server_and_token() {
    let args = vec![
        "index".to_string(),
        "--path".to_string(),
        "/tmp/commands.jsonl".to_string(),
        "--server".to_string(),
        "https://cortex.example.test".to_string(),
        "--token".to_string(),
        "secret".to_string(),
    ];

    let command = parse_shell_agent_command(&args).unwrap();

    match command {
        ShellAgentCommand::Index(args) => {
            assert_eq!(args.server.as_deref(), Some("https://cortex.example.test"));
            assert_eq!(args.token.as_deref(), Some("secret"));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_legacy_agent_command_ingest_spool_as_shell_agent_index() {
    let args = vec![
        "ingest-spool".to_string(),
        "--path".to_string(),
        "/tmp/commands.jsonl".to_string(),
    ];

    let command = parse_shell_agent_command_legacy(&args).unwrap();

    match command {
        ShellAgentCommand::Index(args) => {
            assert_eq!(args.path, "/tmp/commands.jsonl");
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_shell_agent_wrap_after_separator() {
    let args = vec![
        "wrap".to_string(),
        "--spool".to_string(),
        "/tmp/commands.jsonl".to_string(),
        "--".to_string(),
        "echo".to_string(),
        "hello".to_string(),
    ];

    let command = parse_shell_agent_command(&args).unwrap();

    match command {
        ShellAgentCommand::Wrap(args) => {
            assert_eq!(args.spool, "/tmp/commands.jsonl");
            assert_eq!(args.command, vec!["echo", "hello"]);
            assert!(!args.probe);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_shell_agent_wrap_probe_without_spool_or_command() {
    let args = vec!["wrap".to_string(), "--probe".to_string()];

    let command = parse_shell_agent_command(&args).unwrap();

    match command {
        ShellAgentCommand::Wrap(args) => {
            assert!(args.probe);
            assert!(args.command.is_empty());
        }
        other => panic!("unexpected command: {other:?}"),
    }
}
