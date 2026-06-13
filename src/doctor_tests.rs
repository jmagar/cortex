use super::*;
use serial_test::serial;

struct EnvGuard {
    name: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var(name).ok();
        unsafe {
            std::env::set_var(name, value);
        }
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe {
                std::env::set_var(self.name, value);
            },
            None => unsafe {
                std::env::remove_var(self.name);
            },
        }
    }
}

#[cfg(unix)]
fn executable_script(dir: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.path().join("check-runtime-current.sh");
    std::fs::write(&path, body).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

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
fn parse_systemctl_timestamp_utc_accepts_utc_and_standard_us_offsets() {
    assert_eq!(
        parse_systemctl_timestamp_utc("Tue 2026-05-19 22:30:09 UTC").as_deref(),
        Some("2026-05-19T22:30:09.000Z")
    );
    assert_eq!(
        parse_systemctl_timestamp_utc("Tue 2026-05-19 22:30:09 PST").as_deref(),
        Some("2026-05-20T06:30:09.000Z")
    );
    assert_eq!(
        parse_systemctl_timestamp_utc("Tue 2026-05-19 22:30:09 PDT").as_deref(),
        Some("2026-05-20T05:30:09.000Z")
    );
}

#[test]
fn parse_systemctl_timestamp_utc_rejects_malformed_human_forms() {
    assert!(parse_systemctl_timestamp_utc("2026-05-19 22:30:09 UTC").is_none());
    assert!(parse_systemctl_timestamp_utc("Tue 2026/05/19 22:30:09 UTC").is_none());
    assert!(parse_systemctl_timestamp_utc("Tue 2026-05-19 22:30 UTC").is_none());
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
fn diag_status_maps_compose_diagnostic_severity_to_setup_status() {
    assert!(matches!(
        diag_status(&DiagnosticSeverity::Info),
        SetupStatus::Ok
    ));
    assert!(matches!(
        diag_status(&DiagnosticSeverity::Warning),
        SetupStatus::Warn
    ));
    assert!(matches!(
        diag_status(&DiagnosticSeverity::Error),
        SetupStatus::Error
    ));
    assert!(matches!(
        diag_status(&DiagnosticSeverity::Unsafe),
        SetupStatus::Error
    ));
}

#[test]
fn status_label_is_fixed_width_without_color() {
    assert_eq!(status_label(&SetupStatus::Ok), "Ok   ");
    assert_eq!(status_label(&SetupStatus::Warn), "Warn ");
    assert_eq!(status_label(&SetupStatus::Error), "Error");
    assert_eq!(status_label(&SetupStatus::Skipped), "Skip ");
}

#[test]
fn first_meaningful_line_skips_blank_lines_and_preserves_original_when_all_blank() {
    assert_eq!(
        first_meaningful_line("\n\nactual error\nnext"),
        "actual error"
    );
    assert_eq!(first_meaningful_line("\n\n"), "\n\n");
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
            current_exe: "cortex".into(),
            path_cortex: None,
            repo_version: "0.0.0".into(),
            container_version: None,
            runtime_current: Some(true),
            runtime_current_error: None,
        },
        ai: serde_json::json!({}),
    };

    assert_eq!(report.error_count(), 0);
}

#[test]
fn json_error_count_includes_compose_binary_and_top_level_section_errors() {
    let report = JsonDoctorReport {
        setup: serde_json::json!({"blocking_errors": 1}),
        compose: serde_json::json!({
            "diagnostics": [
                {"severity": "warning"},
                {"severity": "error"},
                {"severity": "unsafe"}
            ]
        }),
        binary: BinaryDoctorReport {
            current_exe: "cortex".into(),
            path_cortex: None,
            repo_version: "0.0.0".into(),
            container_version: Some("cortex 0.0.1".into()),
            runtime_current: Some(false),
            runtime_current_error: Some("stale binary".into()),
        },
        ai: serde_json::json!({"error": "db unavailable"}),
    };

    assert_eq!(report.error_count(), 5);
}

#[test]
fn binary_runtime_error_count_only_flags_explicit_stale_runtime() {
    let mut report = BinaryDoctorReport {
        current_exe: "cortex".into(),
        path_cortex: None,
        repo_version: "0.0.0".into(),
        container_version: None,
        runtime_current: None,
        runtime_current_error: Some("unknown".into()),
    };
    assert_eq!(report.runtime_error_count(), 0);

    report.runtime_current = Some(true);
    assert_eq!(report.runtime_error_count(), 0);

    report.runtime_current = Some(false);
    assert_eq!(report.runtime_error_count(), 1);
}

#[cfg(unix)]
#[test]
#[serial]
fn runtime_current_status_reports_success_for_env_script() {
    let dir = tempfile::tempdir().unwrap();
    let script = executable_script(&dir, "#!/bin/sh\nprintf 'ok\\n'\n");
    let _guard = EnvGuard::set("CORTEX_RUNTIME_CHECK_SCRIPT", script);

    assert_eq!(runtime_current_status(), (Some(true), None));
}

#[cfg(unix)]
#[test]
#[serial]
fn runtime_current_status_reports_failure_output_for_env_script() {
    let dir = tempfile::tempdir().unwrap();
    let script = executable_script(
        &dir,
        "#!/bin/sh\nprintf 'stdout detail\\n'\nprintf 'stderr detail\\n' >&2\nexit 7\n",
    );
    let _guard = EnvGuard::set("CORTEX_RUNTIME_CHECK_SCRIPT", script);

    let (current, error) = runtime_current_status();

    assert_eq!(current, Some(false));
    let error = error.unwrap();
    assert!(error.contains("stdout detail"));
    assert!(error.contains("stderr detail"));
}

#[test]
#[serial]
fn runtime_current_script_path_ignores_missing_env_override() {
    let _guard = EnvGuard::set(
        "CORTEX_RUNTIME_CHECK_SCRIPT",
        "/tmp/cortex-missing-runtime-check-script",
    );

    let path = runtime_current_script_path();

    assert!(path.as_ref().is_none_or(
        |path| path != std::path::Path::new("/tmp/cortex-missing-runtime-check-script")
    ));
}

#[cfg(unix)]
#[test]
#[serial]
fn systemctl_unix_timestamp_parses_at_prefixed_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let systemctl = bin_dir.join("systemctl");
    std::fs::write(&systemctl, "#!/bin/sh\nprintf '@1779229809.123456\\n'\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&systemctl).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&systemctl, perms).unwrap();
    }
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let _path_guard = EnvGuard::set("PATH", path);

    assert_eq!(
        systemctl_unix_timestamp("cortex-ai-watch.service").as_deref(),
        Some("2026-05-19T22:30:09.000Z")
    );
}

#[cfg(unix)]
#[test]
#[serial]
fn ai_watcher_process_start_time_falls_back_to_human_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let systemctl = bin_dir.join("systemctl");
    std::fs::write(
        &systemctl,
        r#"#!/bin/sh
case "$*" in
  "--user show -p ExecMainStartTimestamp --value --timestamp=unix cortex-ai-watch.service") printf 'Tue 2026-05-19 22:30:09 EDT\n'; exit 0 ;;
  "--user show -p ExecMainStartTimestamp --value cortex-ai-watch.service") printf 'Tue 2026-05-19 22:30:09 EDT\n'; exit 0 ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&systemctl).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&systemctl, perms).unwrap();
    }
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let _path_guard = EnvGuard::set("PATH", path);

    assert_eq!(
        ai_watcher_process_start_time().as_deref(),
        Some("2026-05-20T02:30:09.000Z")
    );
}

#[cfg(unix)]
#[test]
#[serial]
fn ai_watcher_is_active_requires_success_and_active_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let systemctl = bin_dir.join("systemctl");
    std::fs::write(
        &systemctl,
        "#!/bin/sh\nprintf '%s\\n' \"$CORTEX_TEST_SYSTEMD_STATE\"\n[ \"$CORTEX_TEST_SYSTEMD_STATE\" = active ]\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&systemctl).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&systemctl, perms).unwrap();
    }
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let _path_guard = EnvGuard::set("PATH", path);
    let _state_guard = EnvGuard::set("CORTEX_TEST_SYSTEMD_STATE", "active");
    assert!(ai_watcher_is_active());
    drop(_state_guard);
    let _state_guard = EnvGuard::set("CORTEX_TEST_SYSTEMD_STATE", "inactive");
    assert!(!ai_watcher_is_active());
}

#[cfg(unix)]
#[test]
#[serial]
fn command_stdout_returns_trimmed_nonempty_success_only() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let probe = bin_dir.join("probe");
    std::fs::write(
        &probe,
        r#"#!/bin/sh
case "$1" in
  ok) printf ' value \n'; exit 0 ;;
  blank) printf '\n'; exit 0 ;;
  fail) printf 'nope\n'; exit 2 ;;
esac
"#,
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&probe).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&probe, perms).unwrap();
    }
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let _path_guard = EnvGuard::set("PATH", path);

    assert_eq!(command_stdout("probe", &["ok"]).as_deref(), Some("value"));
    assert_eq!(command_stdout("probe", &["blank"]), None);
    assert_eq!(command_stdout("probe", &["fail"]), None);
}

#[cfg(unix)]
#[test]
#[serial]
fn collect_binary_section_uses_runtime_status_detail() {
    let dir = tempfile::tempdir().unwrap();
    let script = executable_script(&dir, "#!/bin/sh\nprintf '\\ncontainer stale\\n'\nexit 1\n");
    let _guard = EnvGuard::set("CORTEX_RUNTIME_CHECK_SCRIPT", script);

    let section = collect_binary_section();

    assert_eq!(section.header, "Binary");
    assert_eq!(section.phases.len(), 1);
    assert_eq!(section.phases[0].status, SetupStatus::Error);
    assert_eq!(section.phases[0].name, "runtime_current");
    assert_eq!(section.phases[0].detail, "container stale");
}
