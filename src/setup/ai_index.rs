use std::io::{self, ErrorKind};
use std::path::Path;
use std::time::Instant;

use super::{
    check_file_phase, host_local_report_input, setup_report, AiIndexTimerAction, PhaseTimer,
    SetupPhase, SetupReport, SetupStatus,
};
use super::systemd::{systemctl_user_phase, systemctl_user_required_phase};

pub async fn run_ai_index_timer_setup(action: AiIndexTimerAction) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = super::syslog_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let user_home = super::user_home_dir()?;
    let bin_path = user_home.join(".local/bin/syslog-ai-index");
    let systemd_dir = user_home.join(".config/systemd/user");
    let service_path = systemd_dir.join("syslog-ai-index.service");
    let timer_path = systemd_dir.join("syslog-ai-index.timer");
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
                "syslog-ai-index.timer",
            ]));
        }
        AiIndexTimerAction::Remove => {
            phases.push(systemctl_user_phase(&[
                "disable",
                "--now",
                "syslog-ai-index.timer",
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
                "run syslog setup ai-index-timer install",
            ));
            phases.push(check_file_phase(
                "ai-index-service",
                &service_path,
                "run syslog setup ai-index-timer install",
            ));
            phases.push(check_file_phase(
                "ai-index-timer",
                &timer_path,
                "run syslog setup ai-index-timer install",
            ));
            phases.push(systemctl_user_phase(&[
                "is-enabled",
                "syslog-ai-index.timer",
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
    Ok(timer.finish(SetupStatus::Ok, "removed syslog AI index timer files"))
}

pub(crate) fn ai_index_script() -> String {
    r#"#!/usr/bin/env bash
set -euo pipefail

STATE_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/syslog-mcp"
mkdir -p "$STATE_DIR"
LOCK_FILE="$STATE_DIR/ai-index.lock"
LOG_FILE="$STATE_DIR/ai-index.log"

if [[ -z "${SYSLOG_MCP_DB_PATH:-}" ]]; then
  if [[ -f "${HOME}/.claude/plugins/data/syslog-jmagar-lab/syslog.db" ]]; then
    export SYSLOG_MCP_DB_PATH="${HOME}/.claude/plugins/data/syslog-jmagar-lab/syslog.db"
  else
    export SYSLOG_MCP_DB_PATH="${SYSLOG_MCP_HOME:-${HOME}/.syslog-mcp}/data/syslog.db"
  fi
fi

export SYSLOG_DOCKER_INGEST_ENABLED="${SYSLOG_DOCKER_INGEST_ENABLED:-false}"
export RUST_LOG="${RUST_LOG:-warn}"

{
  printf '== %s ==\n' "$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
  command -v syslog
  syslog --version
  syslog ai index --json
} >>"$LOG_FILE" 2>&1
"#
    .to_string()
}

pub(crate) fn ai_index_service_unit(bin_path: &Path) -> String {
    format!(
        "[Unit]\nDescription=syslog-mcp local AI transcript index\nDocumentation=https://github.com/jmagar/syslog-mcp\n\n[Service]\nType=oneshot\nExecStart={}\n",
        bin_path.display()
    )
}

pub(crate) fn ai_index_timer_unit() -> &'static str {
    "[Unit]\nDescription=Run syslog-mcp local AI transcript index\n\n[Timer]\nOnBootSec=5min\nOnUnitActiveSec=30min\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
}

// write_executable_file lives in the parent module (setup.rs) to avoid duplication.
use super::write_executable_file;
