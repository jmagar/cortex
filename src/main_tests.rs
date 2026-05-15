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

#[test]
fn mode_parse_accepts_compose_namespace() {
    assert!(matches!(
        Mode::parse(vec!["compose".into(), "status".into(), "--json".into()]).unwrap(),
        Mode::Cli(_)
    ));
}

#[test]
fn mode_parse_accepts_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec!["setup".into(), "check".into(), "--json".into()]).unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_accepts_ai_index_timer_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "ai-index-timer".into(),
            "install".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_accepts_ai_watch_service_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "ai-watch-service".into(),
            "install".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_accepts_debug_wrapper_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "debug-wrapper".into(),
            "check".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_rejects_duplicate_ai_watch_service_actions() {
    let err = Mode::parse(vec![
        "setup".into(),
        "ai-watch-service".into(),
        "install".into(),
        "remove".into(),
    ])
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("ai-watch-service action specified more than once"));
}

#[test]
fn mode_parse_accepts_binary_doctor() {
    assert!(matches!(
        Mode::parse(vec!["doctor".into(), "binary".into(), "--json".into()]).unwrap(),
        Mode::DoctorBinary(_)
    ));
}
