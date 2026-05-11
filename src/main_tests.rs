use super::Mode;

#[test]
fn mode_parse_accepts_single_binary_transport_commands() {
    assert_eq!(Mode::parse(vec![]).unwrap(), Mode::ServeMcp);
    assert_eq!(
        Mode::parse(vec!["serve".into(), "mcp".into()]).unwrap(),
        Mode::ServeMcp
    );
    assert_eq!(Mode::parse(vec!["mcp".into()]).unwrap(), Mode::StdioMcp);
    assert_eq!(Mode::parse(vec!["--help".into()]).unwrap(), Mode::Help);
    assert_eq!(
        Mode::parse(vec!["--version".into()]).unwrap(),
        Mode::Version
    );
    assert!(matches!(
        Mode::parse(vec!["stats".into(), "--json".into()]).unwrap(),
        Mode::Cli(_)
    ));
}

#[test]
fn mode_parse_rejects_unknown_commands() {
    let err = Mode::parse(vec!["serve".into(), "http".into()]).unwrap_err();
    assert!(err.to_string().contains("unknown command"));
}

#[test]
fn mode_parse_keeps_runtime_status_mcp_only() {
    let err = Mode::parse(vec!["status".into()]).unwrap_err();
    assert!(err.to_string().contains("unknown command"));
}

#[test]
fn mode_parse_accepts_ai_namespace() {
    assert!(matches!(
        Mode::parse(vec!["ai".into(), "tools".into(), "--json".into()]).unwrap(),
        Mode::Cli(_)
    ));
}
