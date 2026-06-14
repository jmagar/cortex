use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use super::ai_watch::{run_ai_watch_service_setup, transcript_root_permissions_phase};
use super::debug_wrapper::{check_debug_compose_content_phase, check_debug_wrapper_content_phase};
use super::firstrun::filesystem_phase;
use super::{
    AiWatchServiceAction, PhaseTimer, SetupPhase, SetupReport, SetupReportInput, SetupStatus,
    check_file_phase, setup_report,
};

pub async fn run_setup_doctor() -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = super::cortex_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let user_home = super::user_home_dir()?;
    let repo_path = std::env::current_dir()?;
    let wrapper_path = user_home.join(".local/bin/cortex");
    let debug_override_path = compose_dir.join("docker-compose.override.yml");
    let mut phases = vec![
        filesystem_phase(super::SetupMode::Check, &home, &data_dir, &compose_dir)?,
        check_file_phase("env", &env_path, "run cortex setup"),
        check_file_phase(
            "compose-assets",
            &compose_dir.join("docker-compose.yml"),
            "run cortex setup repair",
        ),
        check_file_phase(
            "debug-wrapper",
            &wrapper_path,
            "run cortex setup debug-wrapper install",
        ),
        downgrade_dev_phase(
            check_debug_wrapper_content_phase(&wrapper_path, &repo_path),
            "production binary installed (not the dev wrapper — expected in production)",
        ),
        check_file_phase(
            "debug-compose",
            &debug_override_path,
            "run cortex setup debug-compose install",
        ),
        downgrade_dev_phase(
            check_debug_compose_content_phase(&debug_override_path, &repo_path),
            "override uses production config (not the debug build override — expected in production)",
        ),
        transcript_root_permissions_phase(&user_home),
        super::ai_watch::ai_index_timer_disabled_phase(),
    ];

    phases.extend(
        run_ai_watch_service_setup(AiWatchServiceAction::Check)
            .await?
            .phases,
    );
    phases.push(runtime_current_phase(&repo_path));

    let elapsed_ms = started.elapsed().as_millis();
    Ok(setup_report(
        SetupReportInput {
            mode: "doctor",
            elapsed_ms,
            home,
            env_path,
            compose_dir,
            data_dir,
            health_url: "setup doctor".to_string(),
            mcp_url: "setup doctor".to_string(),
        },
        phases,
    ))
}

/// Dev-mode checks (debug-wrapper-content, debug-compose-content) always fail
/// when a production binary/override is installed. In `setup doctor` that's the
/// expected steady state, so we downgrade Error → Warn with a clearer detail
/// and rewrite the issue kind accordingly. Other contexts (e.g. `cortex setup
/// debug-wrapper check`) keep the raw Error semantics.
pub(super) fn downgrade_dev_phase(phase: SetupPhase, detail: &str) -> SetupPhase {
    if matches!(phase.status, SetupStatus::Error) {
        SetupPhase {
            status: SetupStatus::Warn,
            issue_kind: None,
            detail: detail.to_string(),
            ..phase
        }
    } else {
        phase
    }
}

fn runtime_current_phase(repo_path: &Path) -> SetupPhase {
    let timer = PhaseTimer::start("runtime-current");
    let script = repo_path.join("scripts/check-runtime-current.sh");
    if !script.exists() {
        return timer.finish(SetupStatus::Error, format!("missing {}", script.display()));
    }
    match Command::new("bash")
        .arg(script)
        .arg("--allow-local-image")
        .current_dir(repo_path)
        .output()
    {
        Ok(output) if output.status.success() => timer.finish(
            SetupStatus::Ok,
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .last()
                .unwrap_or("runtime current")
                .to_string(),
        ),
        Ok(output) => timer.finish(
            SetupStatus::Error,
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .trim()
            .to_string(),
        ),
        Err(error) => timer.finish(SetupStatus::Error, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
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
        std::fs::create_dir_all(repo.join("scripts")).unwrap();
        std::fs::create_dir_all(&bin_dir).unwrap();
        write_executable(
            &repo.join("scripts/check-runtime-current.sh"),
            "#!/bin/sh\nprintf 'runtime current\\n'\n",
        );
        write_executable(&bin_dir.join("cortex"), "#!/bin/sh\nexit 0\n");
        write_executable(
            &bin_dir.join("systemctl"),
            "#!/bin/sh\ncase \"$*\" in\n  *is-active*cortex-ai-index.timer*) printf 'inactive\\n' ;;\n  *is-enabled*cortex-ai-index.timer*) printf 'disabled\\n' ;;\n  *is-enabled*cortex-ai-watch.service*) printf 'disabled\\n' ;;\n  *is-active*cortex-ai-watch.service*) printf 'inactive\\n' ;;\n  *) printf 'ok\\n' ;;\nesac\nexit 0\n",
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
                .any(|phase| phase.name == "ai-watch-service-content")
        );
    }
}
