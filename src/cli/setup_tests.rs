use super::*;

#[test]
fn setup_failure_helpers_separate_blocking_and_advisory_phases() {
    let report = SetupReport {
        mode: SetupMode::Check,
        data_dir: std::path::PathBuf::from("/tmp/syslog"),
        env_path: std::path::PathBuf::from("/tmp/syslog/.env"),
        phases: vec![
            SetupPhase {
                name: "ok",
                status: SetupStatus::Ok,
                detail: "fine".to_string(),
            },
            SetupPhase {
                name: "warn",
                status: SetupStatus::Warn,
                detail: "heads up".to_string(),
            },
            SetupPhase {
                name: "err",
                status: SetupStatus::Error,
                detail: "bad".to_string(),
            },
        ],
        has_errors: true,
    };

    assert_eq!(setup_blocking_failures(&report), vec!["err".to_string()]);
    assert_eq!(setup_advisory_failures(&report), vec!["warn".to_string()]);
    assert!(ensure_setup_success(&report).is_err());
}
