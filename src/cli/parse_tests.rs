use super::super::OutputArgs;
use super::*;

#[test]
fn parse_routes_stats() {
    assert_eq!(
        parse_command(vec!["stats".to_string()]).unwrap(),
        CliCommand::Stats(OutputArgs::default())
    );
}

#[test]
fn parse_rejects_missing_command() {
    let err = parse_command(Vec::new()).unwrap_err().to_string();

    assert!(err.contains("CLI command is required"));
}

#[test]
fn parse_rejects_unknown_command() {
    let err = parse_command(vec!["wat".to_string()])
        .unwrap_err()
        .to_string();

    assert!(err.contains("unknown CLI command: wat"));
}
