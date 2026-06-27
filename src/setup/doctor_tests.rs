use super::*;
use crate::setup::{PhaseTimer, SetupIssueKind};
use serial_test::serial;

struct EnvGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(name);
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

struct CwdGuard {
    previous: std::path::PathBuf,
}

impl CwdGuard {
    fn set(path: &Path) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self { previous }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.previous).unwrap();
    }
}

#[cfg(unix)]
fn write_executable(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, body).unwrap();
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

fn path_with_prepended(dir: &std::path::Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).unwrap()
}

#[test]
fn downgrade_dev_phase_converts_errors_to_warnings_without_issue_kind() {
    let phase = PhaseTimer::start("debug-wrapper-content").finish_with_issue(
        SetupStatus::Error,
        Some(SetupIssueKind::BlockingError),
        "stale wrapper",
    );

    let downgraded = downgrade_dev_phase(phase, "production binary installed");

    assert!(matches!(downgraded.status, SetupStatus::Warn));
    assert_eq!(downgraded.issue_kind, None);
    assert_eq!(downgraded.detail, "production binary installed");
    assert_eq!(downgraded.name, "debug-wrapper-content");
}

#[test]
fn downgrade_dev_phase_preserves_non_error_statuses() {
    let phase = PhaseTimer::start("debug-compose-content").finish(SetupStatus::Ok, "matches");

    let unchanged = downgrade_dev_phase(phase, "should not replace detail");

    assert!(matches!(unchanged.status, SetupStatus::Ok));
    assert_eq!(unchanged.detail, "matches");
}

#[test]
fn runtime_current_phase_reports_missing_script_as_error() {
    let dir = tempfile::tempdir().unwrap();

    let phase = runtime_current_phase(dir.path());

    assert!(matches!(phase.status, SetupStatus::Error));
    assert!(phase.detail.contains("scripts/check-runtime-current.sh"));
    assert!(phase.detail.contains("missing"));
}

#[cfg(unix)]
#[test]
fn runtime_current_phase_uses_last_stdout_line_on_success() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("scripts/check-runtime-current.sh");
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    std::fs::write(&script, "#!/bin/sh\nprintf 'first\\ncurrent ok\\n'\n").unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();

    let phase = runtime_current_phase(dir.path());

    assert!(matches!(phase.status, SetupStatus::Ok));
    assert_eq!(phase.detail, "current ok");
}

#[cfg(unix)]
#[test]
fn runtime_current_phase_combines_stdout_and_stderr_on_script_failure() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("scripts/check-runtime-current.sh");
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    write_executable(
        &script,
        "#!/bin/sh\nprintf 'stdout detail\\n'\nprintf 'stderr detail\\n' >&2\nexit 9\n",
    );

    let phase = runtime_current_phase(dir.path());

    assert!(matches!(phase.status, SetupStatus::Error));
    assert!(phase.detail.contains("stdout detail"));
    assert!(phase.detail.contains("stderr detail"));
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn run_setup_doctor_collects_temp_home_sections_without_live_services() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let cortex_home = home.join(".cortex");
    let repo = dir.path().join("repo");
    let bin_dir = dir.path().join("bin");
    let db_path = cortex_home.join("data/cortex.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(home.join(".claude/projects")).unwrap();
    std::fs::create_dir_all(home.join(".codex/sessions")).unwrap();
    std::fs::create_dir_all(home.join(".gemini/tmp")).unwrap();
    std::fs::create_dir_all(repo.join("scripts")).unwrap();
    std::fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &repo.join("scripts/check-runtime-current.sh"),
        "#!/bin/sh\nprintf 'runtime current\\n'\n",
    );
    write_executable(&bin_dir.join("cortex"), "#!/bin/sh\nexit 0\n");
    write_executable(
        &bin_dir.join("systemctl"),
        "#!/bin/sh\ncase \"$*\" in\n  *is-active*cortex-sessions-index.timer*) printf 'inactive\\n' ;;\n  *is-enabled*cortex-sessions-index.timer*) printf 'disabled\\n' ;;\n  *is-enabled*cortex-sessions-watch.service*) printf 'disabled\\n' ;;\n  *is-active*cortex-sessions-watch.service*) printf 'inactive\\n' ;;\n  *) printf 'ok\\n' ;;\nesac\nexit 0\n",
    );

    let _cwd = CwdGuard::set(&repo);
    let _home = EnvGuard::set("HOME", &home);
    let _cortex_home = EnvGuard::set("CORTEX_HOME", &cortex_home);
    let _db_path = EnvGuard::set("CORTEX_DB_PATH", &db_path);
    let _path = EnvGuard::set("PATH", path_with_prepended(&bin_dir));

    let report = run_setup_doctor().await.unwrap();

    assert_eq!(report.mode, "doctor");
    assert_eq!(report.home, cortex_home);
    assert!(
        report
            .phases
            .iter()
            .any(|phase| phase.name == "runtime-current" && phase.status == SetupStatus::Ok)
    );
    assert!(
        report
            .phases
            .iter()
            .any(|phase| phase.name == "ai-transcript-root-permissions"
                && phase.status == SetupStatus::Ok)
    );
    assert!(
        report
            .phases
            .iter()
            .any(|phase| phase.name == "sessions-watch-service-content")
    );
}
