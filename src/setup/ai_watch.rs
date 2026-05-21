use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use super::firstrun::{ensure_private_dir, parse_env};
use super::systemd::{
    systemctl_user_phase, systemctl_user_required_named_phase, systemctl_user_state,
};
use super::{
    check_file_phase, host_local_report_input, setup_path_value, setup_report,
    should_skip_ai_watch_systemd_enable, skipped_phase, AiWatchServiceAction, PhaseTimer,
    SetupIssueKind, SetupPhase, SetupReport, SetupStatus, AI_WATCH_SERVICE_ACTIVE_PHASE,
    AI_WATCH_SERVICE_ENABLED_PHASE,
};

pub async fn run_ai_watch_service_setup(action: AiWatchServiceAction) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = super::syslog_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let user_home = super::user_home_dir()?;
    let config_dir = user_home.join(".config/syslog-mcp");
    let watch_env_path = config_dir.join("ai-watch.env");
    let state_dir = user_home.join(".local/state/syslog-mcp");
    let systemd_dir = user_home.join(".config/systemd/user");
    let service_path = systemd_dir.join("syslog-ai-watch.service");
    let mut phases = Vec::new();

    match action {
        AiWatchServiceAction::Install => {
            let syslog_bin = super::resolve_syslog_binary()?;
            let db_path = super::resolve_ai_watch_db_path(&home, &user_home)?;
            phases.push(install_ai_watch_service_files(
                &watch_env_path,
                &service_path,
                &systemd_dir,
                &state_dir,
                &syslog_bin,
                &db_path,
                &user_home,
            )?);
            phases.push(transcript_root_permissions_phase(&user_home));
            phases.push(run_ai_watch_initial_index_phase(
                &syslog_bin,
                &watch_env_path,
            ));
            if should_skip_ai_watch_systemd_enable(&phases) {
                phases.push(skipped_phase(
                    "systemd-enable",
                    "skipped because earlier AI watch install checks failed",
                ));
                let elapsed_ms = started.elapsed().as_millis();
                return Ok(setup_report(
                    host_local_report_input(
                        action.as_str(),
                        elapsed_ms,
                        home,
                        env_path,
                        compose_dir,
                        data_dir,
                    ),
                    phases,
                ));
            }
            phases.push(systemctl_user_phase(&["daemon-reload"]));
            phases.push(systemctl_user_phase(&[
                "disable",
                "--now",
                "syslog-ai-index.timer",
            ]));
            phases.push(ai_index_timer_disabled_phase());
            phases.push(systemctl_user_phase(&[
                "reset-failed",
                "syslog-ai-watch.service",
            ]));
            phases.push(super::systemd::systemctl_user_required_phase(&[
                "enable",
                "--now",
                "syslog-ai-watch.service",
            ]));
            phases.push(systemctl_user_required_named_phase(
                AI_WATCH_SERVICE_ENABLED_PHASE,
                &["is-enabled", "syslog-ai-watch.service"],
            ));
            phases.push(systemctl_user_required_named_phase(
                AI_WATCH_SERVICE_ACTIVE_PHASE,
                &["is-active", "syslog-ai-watch.service"],
            ));
        }
        AiWatchServiceAction::Remove => {
            phases.push(systemctl_user_phase(&[
                "disable",
                "--now",
                "syslog-ai-watch.service",
            ]));
            phases.push(remove_ai_watch_service_files(
                &watch_env_path,
                &service_path,
            )?);
            phases.push(systemctl_user_phase(&["daemon-reload"]));
        }
        AiWatchServiceAction::Check => {
            let syslog_bin = super::resolve_syslog_binary()?;
            let db_path = super::resolve_ai_watch_db_path(&home, &user_home)?;
            phases.push(check_file_phase(
                "ai-watch-env",
                &watch_env_path,
                "run syslog setup ai-watch-service install",
            ));
            phases.push(check_file_phase(
                "ai-watch-service",
                &service_path,
                "run syslog setup ai-watch-service install",
            ));
            phases.push(check_ai_watch_service_content_phase(
                &watch_env_path,
                &service_path,
                &state_dir,
                &syslog_bin,
                &db_path,
                &user_home,
            ));
            phases.push(transcript_root_permissions_phase(&user_home));
            phases.push(ai_index_timer_disabled_phase());
            phases.push(systemctl_user_required_named_phase(
                AI_WATCH_SERVICE_ENABLED_PHASE,
                &["is-enabled", "syslog-ai-watch.service"],
            ));
            phases.push(systemctl_user_required_named_phase(
                AI_WATCH_SERVICE_ACTIVE_PHASE,
                &["is-active", "syslog-ai-watch.service"],
            ));
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

pub(super) fn ai_index_timer_disabled_phase() -> SetupPhase {
    let timer = PhaseTimer::start("ai-index-timer-disabled");
    let active = systemctl_user_state("is-active", "syslog-ai-index.timer");
    let enabled = systemctl_user_state("is-enabled", "syslog-ai-index.timer");
    if active.as_deref() == Some("active") || enabled.as_deref() == Some("enabled") {
        return timer.finish(
            SetupStatus::Error,
            format!(
                "syslog-ai-index.timer still active/enabled (active={active:?}, enabled={enabled:?})"
            ),
        );
    }
    timer.finish(
        SetupStatus::Ok,
        format!(
            "syslog-ai-index.timer inactive or absent (active={active:?}, enabled={enabled:?})"
        ),
    )
}

fn install_ai_watch_service_files(
    env_path: &Path,
    service_path: &Path,
    systemd_dir: &Path,
    state_dir: &Path,
    syslog_bin: &Path,
    db_path: &Path,
    user_home: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("ai-watch-service-files");
    if let Some(env_dir) = env_path.parent() {
        ensure_private_dir(env_dir)?;
    }
    ensure_private_dir(state_dir)?;
    std::fs::create_dir_all(systemd_dir)?;
    write_private_file(env_path, &ai_watch_env_file(db_path))?;
    std::fs::write(
        service_path,
        ai_watch_service_unit(syslog_bin, env_path, db_path, state_dir, user_home),
    )?;
    Ok(timer.finish(
        SetupStatus::Ok,
        format!("wrote {}, {}", env_path.display(), service_path.display()),
    ))
}

fn remove_ai_watch_service_files(env_path: &Path, service_path: &Path) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("ai-watch-service-files");
    for path in [env_path, service_path] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(timer.finish(SetupStatus::Ok, "removed syslog AI watch service files"))
}

pub(crate) fn check_ai_watch_service_content_phase(
    env_path: &Path,
    service_path: &Path,
    state_dir: &Path,
    syslog_bin: &Path,
    db_path: &Path,
    user_home: &Path,
) -> SetupPhase {
    let timer = PhaseTimer::start("ai-watch-service-content");
    let expected_env = ai_watch_env_file(db_path);
    let expected_unit = ai_watch_service_unit(syslog_bin, env_path, db_path, state_dir, user_home);
    let current_env = match std::fs::read_to_string(env_path) {
        Ok(raw) => raw,
        Err(error) => return timer.finish(SetupStatus::Error, error.to_string()),
    };
    let current_unit = match std::fs::read_to_string(service_path) {
        Ok(raw) => raw,
        Err(error) => return timer.finish(SetupStatus::Error, error.to_string()),
    };
    if current_env != expected_env {
        return timer.finish(
            SetupStatus::Error,
            format!(
                "{} does not match generated AI watch environment",
                env_path.display()
            ),
        );
    }
    if current_unit != expected_unit {
        return timer.finish(
            SetupStatus::Error,
            format!(
                "{} does not match generated AI watch unit",
                service_path.display()
            ),
        );
    }
    timer.finish(
        SetupStatus::Ok,
        "AI watch service files match generated content",
    )
}

pub(crate) fn ai_watch_env_file(db_path: &Path) -> String {
    let db_path = setup_path_value(db_path).expect("validated AI watch DB path");
    format!("SYSLOG_MCP_DB_PATH={db_path}\nSYSLOG_DOCKER_INGEST_ENABLED=false\nRUST_LOG=warn\n")
}

pub(crate) fn ai_watch_service_unit(
    syslog_bin: &Path,
    env_path: &Path,
    db_path: &Path,
    state_dir: &Path,
    user_home: &Path,
) -> String {
    let db_dir = db_path.parent().unwrap_or_else(|| Path::new("/"));
    let env_path = setup_path_value(env_path).expect("validated AI watch env path");
    let syslog_bin = setup_path_value(syslog_bin).expect("validated syslog binary path");
    let claude_root = setup_path_value(&user_home.join(".claude/projects"))
        .expect("validated Claude transcript root");
    let codex_root = setup_path_value(&user_home.join(".codex/sessions"))
        .expect("validated Codex transcript root");
    let user_local_bin =
        setup_path_value(&user_home.join(".local/bin")).expect("validated user local bin path");
    let user_cargo_bin =
        setup_path_value(&user_home.join(".cargo/bin")).expect("validated user cargo bin path");
    let cargo_target_dir = setup_path_value(&state_dir.join("cargo-target"))
        .expect("validated AI watch cargo target directory");
    let db_dir = setup_path_value(db_dir).expect("validated AI watch DB directory");
    let state_dir = setup_path_value(state_dir).expect("validated AI watch state directory");
    format!(
        "[Unit]\nDescription=syslog-mcp real-time local AI transcript watch\nDocumentation=https://github.com/jmagar/syslog-mcp\nAfter=default.target\nStartLimitIntervalSec=300\nStartLimitBurst=5\n\n[Service]\nType=simple\nEnvironmentFile={env_path}\nEnvironment=PATH={user_local_bin}:{user_cargo_bin}:/usr/local/bin:/usr/bin:/bin\nEnvironment=CARGO_TARGET_DIR={cargo_target_dir}\nWorkingDirectory=/\nExecStart={syslog_bin} ai watch --no-initial-scan --json\nRestart=on-failure\nRestartSec=5\nUMask=0077\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=strict\nProtectHome=read-only\nBindReadOnlyPaths=-{claude_root} -{codex_root}\nBindPaths={db_dir} {state_dir}\nReadWritePaths={db_dir} {state_dir}\n\n[Install]\nWantedBy=default.target\n"
    )
}

pub(crate) fn transcript_root_permissions_phase(user_home: &Path) -> SetupPhase {
    let timer = PhaseTimer::start("ai-transcript-root-permissions");
    let roots = [
        user_home.join(".claude/projects"),
        user_home.join(".codex/sessions"),
    ];
    let failures: Vec<String> = roots
        .iter()
        .filter_map(|root| transcript_root_permission_error(root))
        .collect();
    if failures.is_empty() {
        timer.finish(
            SetupStatus::Ok,
            "AI transcript roots are owned/readable/writable",
        )
    } else {
        timer.finish(SetupStatus::Error, failures.join("; "))
    }
}

fn transcript_root_permission_error(root: &Path) -> Option<String> {
    let metadata = match std::fs::metadata(root) {
        Ok(metadata) => metadata,
        Err(error) => return Some(format!("{}: {error}", root.display())),
    };
    if !metadata.is_dir() {
        return Some(format!("{} is not a directory", root.display()));
    }
    if std::fs::read_dir(root).is_err() {
        return Some(format!("{} is not readable", root.display()));
    }
    let probe = root.join(format!(".syslog-mcp-write-check-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(probe);
        }
        Err(error) => return Some(format!("{} is not writable: {error}", root.display())),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let current_uid = unsafe { libc::geteuid() };
        if metadata.uid() != current_uid {
            return Some(format!(
                "{} owner uid {} != current uid {}",
                root.display(),
                metadata.uid(),
                current_uid
            ));
        }
    }
    None
}

fn run_ai_watch_initial_index_phase(syslog_bin: &Path, env_path: &Path) -> SetupPhase {
    let timer = PhaseTimer::start("ai-watch-initial-index");
    let env = match std::fs::read_to_string(env_path) {
        Ok(raw) => parse_env(&raw),
        Err(error) => {
            return timer.finish(
                SetupStatus::Error,
                format!("read {}: {error}", env_path.display()),
            );
        }
    };
    let mut command = Command::new(syslog_bin);
    command.args(["ai", "index", "--json"]);
    for (key, value) in env {
        command.env(key, value);
    }
    match command.output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let (status, issue_kind) = ai_index_output_status(&stdout);
            timer.finish_with_issue(status, issue_kind, summarize_ai_index_output(&stdout))
        }
        Ok(output) => timer.finish_with_issue(
            SetupStatus::Error,
            Some(SetupIssueKind::BlockingError),
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("initial AI index failed")
                .to_string(),
        ),
        Err(error) => timer.finish_with_issue(
            SetupStatus::Error,
            Some(SetupIssueKind::BlockingError),
            error.to_string(),
        ),
    }
}

pub(crate) fn summarize_ai_index_output(stdout: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return "invalid ai index JSON output".to_string();
    };
    let summary = format!(
        "indexed files={} ingested={} duplicates={} parse_errors={} storage_blocked={} dropped_metadata_fields={} file_errors={}",
        value
            .get("discovered_files")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("ingested")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("skipped_dupes")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("parse_errors")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("storage_blocked_chunks")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("dropped_metadata_fields")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("file_errors")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len),
    );
    let (status, _) = ai_index_output_status(stdout);
    if matches!(status, SetupStatus::Warn) {
        format!(
            "{summary}; inspect with `syslog ai errors --limit 20`, `syslog ai checkpoints --errors`, then rerun `syslog ai index --json` after fixes"
        )
    } else {
        summary
    }
}

pub(crate) fn ai_index_output_status(stdout: &str) -> (SetupStatus, Option<SetupIssueKind>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return (SetupStatus::Error, Some(SetupIssueKind::BlockingError));
    };
    if value
        .get("storage_blocked_chunks")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0
    {
        return (SetupStatus::Error, Some(SetupIssueKind::BlockingError));
    }
    if value
        .get("parse_errors")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0
        || value
            .get("dropped_metadata_fields")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
        || value
            .get("file_errors")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|errors| !errors.is_empty())
    {
        return (SetupStatus::Warn, Some(SetupIssueKind::DataQualityWarning));
    }
    (SetupStatus::Ok, None)
}

// write_private_file lives in the parent module (setup.rs) to avoid duplication.
use super::write_private_file;
