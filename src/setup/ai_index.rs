use std::io::{self, ErrorKind};
use std::path::Path;
use std::time::Instant;

use super::systemd::{systemctl_user_phase, systemctl_user_required_phase};
use super::{
    AiIndexTimerAction, PhaseTimer, SetupPhase, SetupReport, SetupStatus, check_file_phase,
    host_local_report_input, setup_report,
};

pub async fn run_ai_index_timer_setup(action: AiIndexTimerAction) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = super::cortex_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let user_home = super::user_home_dir()?;
    let bin_path = user_home.join(".local/bin/cortex-ai-index");
    let systemd_dir = user_home.join(".config/systemd/user");
    let service_path = systemd_dir.join("cortex-ai-index.service");
    let timer_path = systemd_dir.join("cortex-ai-index.timer");
    let mut phases = Vec::new();

    match action {
        AiIndexTimerAction::Install => {
            phases.push(install_ai_index_timer_files(
                &bin_path,
                &systemd_dir,
                &service_path,
                &timer_path,
            )?);
            phases.push(systemctl_user_required_phase(&["daemon-reload"]));
            phases.push(systemctl_user_required_phase(&[
                "enable",
                "--now",
                "cortex-ai-index.timer",
            ]));
        }
        AiIndexTimerAction::Remove => {
            phases.push(systemctl_user_phase(&[
                "disable",
                "--now",
                "cortex-ai-index.timer",
            ]));
            phases.push(remove_ai_index_timer_files(
                &bin_path,
                &service_path,
                &timer_path,
            )?);
            phases.push(systemctl_user_phase(&["daemon-reload"]));
        }
        AiIndexTimerAction::Check => {
            phases.push(check_file_phase(
                "ai-index-bin",
                &bin_path,
                "run cortex setup ai-index-timer install",
            ));
            phases.push(check_file_phase(
                "ai-index-service",
                &service_path,
                "run cortex setup ai-index-timer install",
            ));
            phases.push(check_file_phase(
                "ai-index-timer",
                &timer_path,
                "run cortex setup ai-index-timer install",
            ));
            phases.push(systemctl_user_phase(&[
                "is-enabled",
                "cortex-ai-index.timer",
            ]));
        }
    }

    let elapsed_ms = started.elapsed().as_millis();
    Ok(setup_report(
        host_local_report_input(
            action.as_str(),
            elapsed_ms,
            home,
            env_path,
            compose_dir,
            data_dir,
        ),
        phases,
    ))
}

fn install_ai_index_timer_files(
    bin_path: &Path,
    systemd_dir: &Path,
    service_path: &Path,
    timer_path: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("ai-index-timer-files");
    if let Some(bin_dir) = bin_path.parent() {
        std::fs::create_dir_all(bin_dir)?;
    }
    std::fs::create_dir_all(systemd_dir)?;
    write_executable_file(bin_path, &ai_index_script())?;
    std::fs::write(service_path, ai_index_service_unit(bin_path))?;
    std::fs::write(timer_path, ai_index_timer_unit())?;
    Ok(timer.finish(
        SetupStatus::Ok,
        format!(
            "wrote {}, {}, {}",
            bin_path.display(),
            service_path.display(),
            timer_path.display()
        ),
    ))
}

fn remove_ai_index_timer_files(
    bin_path: &Path,
    service_path: &Path,
    timer_path: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("ai-index-timer-files");
    for path in [bin_path, service_path, timer_path] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(timer.finish(SetupStatus::Ok, "removed cortex AI index timer files"))
}

pub(crate) fn ai_index_script() -> String {
    r#"#!/usr/bin/env bash
set -euo pipefail

STATE_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/cortex"
mkdir -p "$STATE_DIR"
LOCK_FILE="$STATE_DIR/ai-index.lock"
LOG_FILE="$STATE_DIR/ai-index.log"

if [[ -z "${CORTEX_DB_PATH:-}" ]]; then
  if [[ -f "${HOME}/.claude/plugins/data/syslog-jmagar-lab/cortex.db" ]]; then
    export CORTEX_DB_PATH="${HOME}/.claude/plugins/data/syslog-jmagar-lab/cortex.db"
  else
    export CORTEX_DB_PATH="${CORTEX_HOME:-${HOME}/.cortex}/data/cortex.db"
  fi
fi

export CORTEX_DOCKER_INGEST_ENABLED="${CORTEX_DOCKER_INGEST_ENABLED:-false}"
export RUST_LOG="${RUST_LOG:-warn}"

{
  printf '== %s ==\n' "$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
  command -v cortex
  cortex --version
  cortex ai index --json
} >>"$LOG_FILE" 2>&1
"#
    .to_string()
}

pub(crate) fn ai_index_service_unit(bin_path: &Path) -> String {
    format!(
        "[Unit]\nDescription=cortex local AI transcript index\nDocumentation=https://github.com/jmagar/cortex\n\n[Service]\nType=oneshot\nExecStart={}\n",
        bin_path.display()
    )
}

pub(crate) fn ai_index_timer_unit() -> &'static str {
    "[Unit]\nDescription=Run cortex local AI transcript index\n\n[Timer]\nOnBootSec=5min\nOnUnitActiveSec=30min\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
}

// write_executable_file lives in the parent module (setup.rs) to avoid duplication.
use super::write_executable_file;

#[cfg(test)]
mod tests {
    use super::*;
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
    fn install_ai_index_timer_files_writes_script_service_and_timer() {
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("bin/cortex-ai-index");
        let systemd_dir = dir.path().join("systemd");
        let service_path = systemd_dir.join("cortex-ai-index.service");
        let timer_path = systemd_dir.join("cortex-ai-index.timer");

        let phase =
            install_ai_index_timer_files(&bin_path, &systemd_dir, &service_path, &timer_path)
                .unwrap();

        assert_eq!(phase.status, SetupStatus::Ok);
        let script = std::fs::read_to_string(&bin_path).unwrap();
        assert!(script.contains("cortex ai index --json"));
        assert!(script.contains("CORTEX_DOCKER_INGEST_ENABLED"));
        let service = std::fs::read_to_string(&service_path).unwrap();
        assert!(service.contains(&format!("ExecStart={}", bin_path.display())));
        let timer = std::fs::read_to_string(&timer_path).unwrap();
        assert!(timer.contains("OnUnitActiveSec=30min"));
    }

    #[test]
    fn remove_ai_index_timer_files_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("bin/cortex-ai-index");
        let service_path = dir.path().join("systemd/cortex-ai-index.service");
        let timer_path = dir.path().join("systemd/cortex-ai-index.timer");
        std::fs::create_dir_all(bin_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(service_path.parent().unwrap()).unwrap();
        std::fs::write(&bin_path, "script").unwrap();
        std::fs::write(&service_path, "service").unwrap();
        std::fs::write(&timer_path, "timer").unwrap();

        let first = remove_ai_index_timer_files(&bin_path, &service_path, &timer_path).unwrap();
        let second = remove_ai_index_timer_files(&bin_path, &service_path, &timer_path).unwrap();

        assert_eq!(first.status, SetupStatus::Ok);
        assert_eq!(second.status, SetupStatus::Ok);
        assert!(!bin_path.exists());
        assert!(!service_path.exists());
        assert!(!timer_path.exists());
    }

    #[test]
    fn service_and_timer_units_keep_host_user_timer_contract() {
        let service = ai_index_service_unit(Path::new("/home/me/.local/bin/cortex-ai-index"));
        let timer = ai_index_timer_unit();

        assert!(service.contains("Type=oneshot"));
        assert!(service.contains("ExecStart=/home/me/.local/bin/cortex-ai-index"));
        assert!(timer.contains("OnBootSec=5min"));
        assert!(timer.contains("Persistent=true"));
        assert!(timer.contains("WantedBy=timers.target"));
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn run_ai_index_timer_setup_install_check_and_remove_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let cortex_home = home.join(".cortex");
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&cortex_home).unwrap();
        std::fs::create_dir_all(&bin_dir).unwrap();
        write_executable(
            &bin_dir.join("systemctl"),
            "#!/bin/sh\ncase \"$*\" in\n  *is-enabled*) printf 'enabled\\n' ;;\n  *) printf 'ok\\n' ;;\nesac\nexit 0\n",
        );

        let _home = EnvGuard::set("HOME", &home);
        let _cortex_home = EnvGuard::set("CORTEX_HOME", &cortex_home);
        let _path = EnvGuard::set("PATH", path_with_prepended(&bin_dir));

        let install = run_ai_index_timer_setup(AiIndexTimerAction::Install)
            .await
            .unwrap();
        assert_eq!(install.mode, "ai-index-timer-install");
        assert!(
            install.phases.iter().any(
                |phase| phase.name == "ai-index-timer-files" && phase.status == SetupStatus::Ok
            )
        );
        assert!(home.join(".local/bin/cortex-ai-index").is_file());
        assert!(
            home.join(".config/systemd/user/cortex-ai-index.service")
                .is_file()
        );
        assert!(
            home.join(".config/systemd/user/cortex-ai-index.timer")
                .is_file()
        );

        let check = run_ai_index_timer_setup(AiIndexTimerAction::Check)
            .await
            .unwrap();
        assert_eq!(check.mode, "ai-index-timer-check");
        assert!(
            check
                .phases
                .iter()
                .any(|phase| phase.name == "systemctl-user" && phase.status == SetupStatus::Ok)
        );

        let remove = run_ai_index_timer_setup(AiIndexTimerAction::Remove)
            .await
            .unwrap();
        assert_eq!(remove.mode, "ai-index-timer-remove");
        assert!(!home.join(".local/bin/cortex-ai-index").exists());
        assert!(
            !home
                .join(".config/systemd/user/cortex-ai-index.service")
                .exists()
        );
        assert!(
            !home
                .join(".config/systemd/user/cortex-ai-index.timer")
                .exists()
        );
    }
}
