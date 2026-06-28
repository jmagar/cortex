use super::*;

#[test]
fn parses_shell_index() {
    let args = vec![
        "index".to_string(),
        "--path".to_string(),
        "/tmp/.zsh_history".to_string(),
        "--json".to_string(),
    ];

    let command = parse_shell_command(&args).unwrap();

    match command {
        ShellCommand::Index(args) => {
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
        "atuin-index".to_string(),
        "--path".to_string(),
        "/tmp/atuin/history.db".to_string(),
        "--json".to_string(),
    ];

    let command = parse_shell_command(&args).unwrap();

    match command {
        ShellCommand::AtuinIndex(args) => {
            assert_eq!(args.path, "/tmp/atuin/history.db");
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_agent_command_ingest_spool() {
    let args = vec![
        "ingest-spool".to_string(),
        "--path".to_string(),
        "/tmp/commands.jsonl".to_string(),
    ];

    let command = parse_agent_command_command(&args).unwrap();

    match command {
        AgentCommandCommand::IngestSpool(args) => {
            assert_eq!(args.path, "/tmp/commands.jsonl");
            assert!(!args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_agent_command_wrap_after_separator() {
    let args = vec![
        "wrap".to_string(),
        "--spool".to_string(),
        "/tmp/commands.jsonl".to_string(),
        "--".to_string(),
        "echo".to_string(),
        "hello".to_string(),
    ];

    let command = parse_agent_command_command(&args).unwrap();

    match command {
        AgentCommandCommand::Wrap(args) => {
            assert_eq!(args.spool, "/tmp/commands.jsonl");
            assert_eq!(args.command, vec!["echo", "hello"]);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}
