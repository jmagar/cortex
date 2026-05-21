use super::*;

#[test]
fn parse_systemctl_timestamp_utc_accepts_us_tz_abbrev() {
    // Legacy human-readable form, EDT → UTC, formatted as RFC3339 millis+Z.
    assert_eq!(
        parse_systemctl_timestamp_utc("Tue 2026-05-19 22:30:09 EDT").as_deref(),
        Some("2026-05-20T02:30:09.000Z")
    );
}

#[test]
fn parse_systemctl_timestamp_utc_rejects_unknown_tz() {
    // Non-US TZ abbreviations are not in the fallback table; caller falls
    // through to None and surfaces `ai_watch_start_unknown` upstream.
    assert!(parse_systemctl_timestamp_utc("Mon 2026-05-19 22:30:09 CEST").is_none());
    assert!(parse_systemctl_timestamp_utc("").is_none());
    assert!(parse_systemctl_timestamp_utc("n/a").is_none());
}

#[test]
fn section_counts_errors_warnings_and_passes() {
    let section = DoctorSection::new(
        "Test",
        vec![
            DoctorPhase::new(SetupStatus::Ok, "ok", "ok"),
            DoctorPhase::new(SetupStatus::Skipped, "skip", "skip"),
            DoctorPhase::new(SetupStatus::Warn, "warn", "warn"),
            DoctorPhase::new(SetupStatus::Error, "error", "error"),
        ],
    );

    assert_eq!(section.passed_count(), 2);
    assert_eq!(section.warning_count(), 1);
    assert_eq!(section.error_count(), 1);
}

#[test]
fn json_error_count_ignores_expected_production_dev_wrapper_errors() {
    let report = JsonDoctorReport {
        setup: serde_json::json!({
            "blocking_errors": 2,
            "phases": [
                {"name": "debug-wrapper-content", "status": "error"},
                {"name": "debug-compose-content", "status": "error"}
            ]
        }),
        compose: serde_json::json!({"diagnostics": []}),
        binary: BinaryDoctorReport {
            current_exe: "syslog".into(),
            path_syslog: None,
            repo_version: "0.0.0".into(),
            container_version: None,
            runtime_current: Some(true),
            runtime_current_error: None,
        },
        ai: serde_json::json!({}),
    };

    assert_eq!(report.error_count(), 0);
}
