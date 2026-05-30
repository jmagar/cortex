use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use super::ai_watch::{run_ai_watch_service_setup, transcript_root_permissions_phase};
use super::debug_wrapper::{check_debug_compose_content_phase, check_debug_wrapper_content_phase};
use super::firstrun::filesystem_phase;
use super::{
    check_file_phase, setup_report, AiWatchServiceAction, PhaseTimer, SetupPhase, SetupReport,
    SetupReportInput, SetupStatus,
};

pub async fn run_setup_doctor() -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = super::syslog_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let user_home = super::user_home_dir()?;
    let repo_path = std::env::current_dir()?;
    let wrapper_path = user_home.join(".local/bin/syslog");
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
