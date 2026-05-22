use super::*;

#[test]
fn parse_routes_stats() {
    assert_eq!(
        parse_command(vec!["stats".to_string()]).unwrap(),
        CliCommand::Stats(OutputArgs::default())
    );
}
